//! The backend traits the built-in UI leans on, and the custom backend slot.
//!
//! fairing ships no system code: the integrator plugs in backends. These tests stand in for an
//! integrator — the `Mock*` backends play the device — and check that what the shell draws and
//! asks for follows them: the status items appear with the capability, the setting table reaches
//! the audio backend, a new notification asks for a cue, the shade's volume tile reads the
//! backend, and a backend of the integrator's own takes part in the frame.

use fairing::services::mock::{MockAudio, MockDisplay, MockNetwork, NetworkMsg};
use fairing::services::null::NullClock;
use fairing::services::{AudioBackend, AudioSnapshot, Backend, Cue, ServiceResult, Waker};
use fairing::settings::{keys, SettingValue};
use fairing::testing::{single_level_access, Harness};
use fairing::{
    screen, Cx, Error, LaunchAction, Notification, NotificationId, Services, ShellConfig,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

fn require<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| Error::Config(format!("there is no {what}")))
}

fn harness(config: ShellConfig, services: Services) -> fairing::Result<Harness> {
    let mut h = Harness::new(config, services)?;
    h.frames(2);
    Ok(h)
}

fn with_status(ids: &[&str]) -> ShellConfig {
    let mut config = single_level_access();
    config.status_bar.right = ids.iter().map(|id| (*id).to_owned()).collect();
    config
}

const ITEMS: [&str; 3] = ["status.volume", "status.ethernet", "status.brightness"];

/// With the `Null` defaults the three items stay hidden whatever the config lists; with backends
/// that report the capability they are drawn: a capability not reported is not drawn.
#[test]
fn status_items_follow_the_backends_capabilities() -> fairing::Result<()> {
    let bare = harness(with_status(&ITEMS), Services::null())?;
    for id in ITEMS {
        assert!(
            bare.shell.status_bar().item_rect(id).is_none(),
            "{id} with no backend"
        );
    }
    let services = Services::builder()
        .clock(NullClock)
        .audio(MockAudio::new(40))
        .network(MockNetwork::new())
        .display(MockDisplay::new(70))
        .build();
    let equipped = harness(with_status(&ITEMS), services)?;
    for id in ITEMS {
        assert!(
            equipped.shell.status_bar().item_rect(id).is_some(),
            "{id} with its backend"
        );
    }
    Ok(())
}

/// A cable pulled out leaves the item on screen, dimmed — the cable is the thing to check.
#[test]
fn a_wired_link_going_down_keeps_the_item() -> fairing::Result<()> {
    let network = MockNetwork::new();
    let control = network.control();
    let services = Services::builder()
        .clock(NullClock)
        .network(network)
        .build();
    let mut h = harness(with_status(&["status.ethernet"]), services)?;
    assert!(control.send(NetworkMsg::Link {
        iface: "eth0".to_owned(),
        up: false,
    }));
    h.frames(2);
    assert_eq!(h.shell.services().network.ethernet_up(), Some(false));
    assert!(h.shell.status_bar().item_rect("status.ethernet").is_some());
    Ok(())
}

/// `audio.volume` and `audio.muted` reach the backend through the setting table.
#[test]
fn volume_and_mute_settings_reach_the_audio_backend() -> fairing::Result<()> {
    let services = Services::builder()
        .clock(NullClock)
        .audio(MockAudio::new(60))
        .build();
    let mut h = harness(single_level_access(), services)?;
    h.shell
        .handle()
        .set_setting(keys::AUDIO_VOLUME, SettingValue::Int(25));
    h.shell
        .handle()
        .set_setting(keys::AUDIO_MUTED, SettingValue::Bool(true));
    h.frames(2);
    assert_eq!(
        h.shell.services().audio.volume(),
        Some(AudioSnapshot {
            level: 25,
            muted: true
        })
    );
    Ok(())
}

/// An audio backend that only writes down the cues it is asked for.
struct CueLog(Rc<RefCell<Vec<Cue>>>);

impl Backend for CueLog {}

impl AudioBackend for CueLog {
    fn volume(&self) -> Option<AudioSnapshot> {
        None
    }

    fn set_volume(&mut self, _level: u8) -> ServiceResult {
        Ok(())
    }

    fn set_muted(&mut self, _muted: bool) -> ServiceResult {
        Ok(())
    }

    fn play_cue(&mut self, cue: Cue) {
        self.0.borrow_mut().push(cue);
    }
}

/// A new notification asks for `Cue::Notify`. An update to one already shown does not, and with
/// `ui.silent` on nothing is asked for — the notification still arrives.
#[test]
fn a_new_notification_asks_for_a_cue_unless_silent() -> fairing::Result<()> {
    let cues = Rc::new(RefCell::new(Vec::new()));
    let services = Services::builder()
        .clock(NullClock)
        .audio(CueLog(Rc::clone(&cues)))
        .build();
    let mut h = harness(single_level_access(), services)?;
    h.shell
        .notify(Notification::new(NotificationId::of("a"), "first"));
    assert_eq!(*cues.borrow(), vec![Cue::Notify]);
    h.shell
        .notify(Notification::new(NotificationId::of("a"), "first, again"));
    assert_eq!(cues.borrow().len(), 1, "an update is not a new one");

    h.shell
        .handle()
        .set_setting(keys::UI_SILENT, SettingValue::Bool(true));
    h.frames(1);
    h.shell
        .notify(Notification::new(NotificationId::of("b"), "second"));
    assert_eq!(cues.borrow().len(), 1, "silent asks for no cue");
    assert_eq!(h.shell.notifications().len(), 2, "but it still arrives");
    Ok(())
}

/// The shade's volume tile reads the audio backend, like the brightness tile reads the display:
/// a change made outside the UI (the volume keys) shows on the next frame.
#[cfg(feature = "overlay")]
#[test]
fn the_volume_tile_reads_the_audio_backend() -> fairing::Result<()> {
    use fairing::overlay::TileState;
    use fairing::services::mock::AudioMsg;

    let tile_value = |h: &Harness| match h.shell.overlay().tile_state("tile.volume") {
        Some(TileState::Value(v)) => Some(v),
        _ => None,
    };
    let audio = MockAudio::new(60);
    let control = audio.control();
    let mut config = single_level_access();
    config.overlay.tiles = vec!["tile.volume".to_owned()];
    let services = Services::builder().clock(NullClock).audio(audio).build();
    let mut h = harness(config, services)?;
    h.shell.launch(LaunchAction::OpenOverlay);
    h.run_for(1.0);
    let before = require(tile_value(&h), "volume tile value")?;
    assert!((before - 0.6).abs() < 1e-4, "{before}");
    assert!(control.send(AudioMsg::Volume(AudioSnapshot {
        level: 20,
        muted: false,
    })));
    h.frames(2);
    let after = require(tile_value(&h), "volume tile value")?;
    assert!((after - 0.2).abs() < 1e-4, "{after}");
    Ok(())
}

/// A backend of the integrator's own: attached once at start-up, polled every frame, and found by
/// its type from a screen.
#[derive(Default)]
struct DoorSensor {
    attached: u32,
    polls: u32,
    seen_by_screen: u32,
}

impl Backend for DoorSensor {
    fn attach(&mut self, _waker: Waker) {
        self.attached += 1;
    }

    fn poll_at(&mut self, _now: Instant) {
        self.polls += 1;
    }
}

#[test]
fn a_custom_backend_is_attached_polled_and_found_by_type() -> fairing::Result<()> {
    let services = Services::builder()
        .clock(NullClock)
        .custom(DoorSensor::default())
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    h.shell
        .add(screen("door", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            ui.label("door");
            if let Some(door) = cx.services.custom_mut::<DoorSensor>() {
                door.seen_by_screen += 1;
            }
        }));
    h.shell.launch(LaunchAction::open("door"));
    h.frames(5);
    let door = require(h.shell.services().custom::<DoorSensor>(), "door sensor")?;
    assert_eq!(door.attached, 1, "the Waker is handed over once");
    assert!(door.polls >= 5, "polled every frame: {}", door.polls);
    assert!(door.seen_by_screen > 0, "the screen found it by its type");
    Ok(())
}
