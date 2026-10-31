//! Regression coverage for ordered state updates and policy semantics.
#![cfg(all(feature = "mock", feature = "overlay"))]

use fairing::access::{Access, AccessPolicy, Gate, Subject};
use fairing::desktop::Badge;
use fairing::notify::{Notification, NotificationId};
use fairing::services::{
    Backend, Capabilities, ServiceError, ServiceResult, WifiBackend, WifiSnapshot,
};
use fairing::settings::{keys, SettingValue};
use fairing::testing::{access_config, single_level_access, Harness};
use fairing::{action, screen, Cx, LaunchAction, Level, Shell, ShellEvent};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn shell() -> fairing::Result<Harness> {
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    Harness::new(cfg, fairing::services::mock::services())
}

#[test]
fn saved_settings_restore_the_live_radio_state() -> fairing::Result<()> {
    let mut h = shell()?;
    let _ = h.shell.poll_events();
    for (key, value) in [
        (keys::WIFI_ENABLED, false),
        (keys::BLUETOOTH_ENABLED, false),
        (keys::RADIO_AIRPLANE, true),
        (keys::RADIO_AIRPLANE, false),
    ] {
        h.shell.set_setting(key.into(), SettingValue::Bool(value));
    }
    let live = (
        h.shell.services().wifi.enabled(),
        h.shell.services().bluetooth.enabled(),
    );
    assert_eq!(live, (true, true));
    let mut saved = std::collections::BTreeMap::new();
    for event in h.shell.poll_events() {
        if let ShellEvent::SettingChanged { key, value } = event {
            saved.insert(key, value);
        }
    }
    let mut restarted = shell()?;
    restarted.shell.restore_settings(saved);
    let restored = (
        restarted.shell.services().wifi.enabled(),
        restarted.shell.services().bluetooth.enabled(),
    );
    assert_eq!(restored, live);
    Ok(())
}

/// A valid backend whose commands complete only at the next poll, as guide 06 permits.
struct DeferredWifi {
    enabled: bool,
    auto_complete: bool,
    pending: Vec<bool>,
    commands: Rc<RefCell<Vec<bool>>>,
    reject: Rc<Cell<bool>>,
    external: Rc<Cell<Option<bool>>>,
}
impl Backend for DeferredWifi {
    fn capabilities(&self) -> Capabilities {
        Capabilities::WIFI
    }
    fn poll(&mut self) {
        if self.auto_complete {
            for value in self.pending.drain(..) {
                self.enabled = value;
            }
        }
        if let Some(value) = self.external.take() {
            self.enabled = value;
        }
    }
}
impl WifiBackend for DeferredWifi {
    fn snapshot(&self) -> WifiSnapshot {
        WifiSnapshot {
            enabled: self.enabled,
            ..WifiSnapshot::default()
        }
    }
    fn set_enabled(&mut self, on: bool) -> ServiceResult {
        self.commands.borrow_mut().push(on);
        if self.reject.get() {
            return Err(ServiceError::new(
                fairing::services::ErrorKind::Denied,
                "rejected for test",
            ));
        }
        self.pending.push(on);
        Ok(())
    }
    fn scan(&mut self) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
    fn connect(&mut self, _: &str, _: Option<&str>, _: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
    fn disconnect(&mut self) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
    fn forget(&mut self, _: &str) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

#[test]
fn set_then_toggle_preserves_command_order_with_deferred_backends() -> fairing::Result<()> {
    for poll_between in [false, true] {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let backend = DeferredWifi {
            enabled: true,
            auto_complete: true,
            pending: Vec::new(),
            commands: Rc::clone(&commands),
            reject: Rc::new(Cell::new(false)),
            external: Rc::new(Cell::new(None)),
        };
        let services = fairing::Services::builder().wifi(backend).build();
        let mut h = Harness::new(single_level_access(), services)?;
        h.frames(3);
        h.shell
            .handle()
            .set_setting(keys::WIFI_ENABLED, SettingValue::Bool(false));
        if poll_between {
            h.frame();
        }
        h.shell
            .handle()
            .launch(LaunchAction::Toggle(keys::WIFI_ENABLED.into()));
        h.frames(2);
        let actual = h.shell.services().wifi.enabled();
        assert!(actual);
        assert_eq!(&*commands.borrow(), &[false, true]);
    }
    Ok(())
}

#[test]
fn toggles_follow_external_state_after_confirmation_and_ignore_rejected_requests(
) -> fairing::Result<()> {
    for reject in [false, true] {
        let commands = Rc::new(RefCell::new(Vec::new()));
        let external = Rc::new(Cell::new(None));
        let backend = DeferredWifi {
            enabled: true,
            auto_complete: true,
            pending: Vec::new(),
            commands: Rc::clone(&commands),
            reject: Rc::new(Cell::new(reject)),
            external: Rc::clone(&external),
        };
        let mut h = Harness::new(
            single_level_access(),
            fairing::Services::builder().wifi(backend).build(),
        )?;
        h.frames(2);
        h.shell
            .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(false));
        if !reject {
            h.frame(); // The original request has been confirmed.
            external.set(Some(true));
            h.frame(); // An independent backend update supersedes the confirmed setting.
        }
        h.shell
            .handle()
            .launch(LaunchAction::Toggle(keys::WIFI_ENABLED.into()));
        h.frame();
        assert_eq!(&*commands.borrow(), &[false, false]);
        assert_eq!(h.shell.services().wifi.enabled(), reject);
    }
    Ok(())
}

#[test]
fn rejected_queued_requests_do_not_poison_later_toggles() -> fairing::Result<()> {
    let commands = Rc::new(RefCell::new(Vec::new()));
    let reject = Rc::new(Cell::new(false));
    let backend = DeferredWifi {
        enabled: true,
        auto_complete: true,
        pending: Vec::new(),
        commands: Rc::clone(&commands),
        reject: Rc::clone(&reject),
        external: Rc::new(Cell::new(None)),
    };
    let mut h = Harness::new(
        single_level_access(),
        fairing::Services::builder().wifi(backend).build(),
    )?;
    h.frames(2);
    h.shell
        .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(false));
    h.shell
        .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(true));
    reject.set(true);
    h.frame(); // The first target completes; sending the queued target is rejected.
    reject.set(false);
    h.shell
        .launch(LaunchAction::Toggle(keys::WIFI_ENABLED.into()));
    h.frame();
    assert_eq!(&*commands.borrow(), &[false, true, true]);
    assert!(h.shell.services().wifi.enabled());
    Ok(())
}

#[test]
fn unchanged_snapshots_do_not_acknowledge_unfinished_radio_commands() -> fairing::Result<()> {
    let commands = Rc::new(RefCell::new(Vec::new()));
    let external = Rc::new(Cell::new(None));
    let backend = DeferredWifi {
        enabled: true,
        auto_complete: false,
        pending: Vec::new(),
        commands: Rc::clone(&commands),
        reject: Rc::new(Cell::new(false)),
        external: Rc::clone(&external),
    };
    let mut h = Harness::new(
        single_level_access(),
        fairing::Services::builder().wifi(backend).build(),
    )?;
    h.frames(2);
    for on in [false, true] {
        h.shell
            .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(on));
    }
    h.frame(); // The worker has not completed either command; the cache is still true.
    external.set(Some(false));
    h.frame(); // The first command completes, while the latest target is still true.
    h.shell
        .handle()
        .launch(LaunchAction::Toggle(keys::WIFI_ENABLED.into()));
    h.frame();
    external.set(Some(true));
    h.frame(); // The second target completes; the queued toggle must now send false.
    assert_eq!(commands.borrow().last(), Some(&false));
    Ok(())
}

struct UnreportedAudio(Rc<RefCell<Vec<bool>>>);

impl Backend for UnreportedAudio {
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
}

impl fairing::services::AudioBackend for UnreportedAudio {
    fn volume(&self) -> Option<fairing::services::AudioSnapshot> {
        None
    }

    fn set_volume(&mut self, _: u8) -> ServiceResult {
        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> ServiceResult {
        self.0.borrow_mut().push(muted);
        Ok(())
    }
}

#[test]
fn an_audio_backend_without_feedback_does_not_stall_queued_mute_writes() -> fairing::Result<()> {
    let commands = Rc::new(RefCell::new(Vec::new()));
    let mut h = Harness::new(
        single_level_access(),
        fairing::Services::builder()
            .audio(UnreportedAudio(Rc::clone(&commands)))
            .build(),
    )?;
    h.frames(2);
    h.shell
        .set_setting(keys::AUDIO_MUTED.into(), SettingValue::Bool(false));
    h.shell
        .launch(LaunchAction::Toggle(keys::AUDIO_MUTED.into()));
    h.frame();
    h.shell
        .launch(LaunchAction::Toggle(keys::AUDIO_MUTED.into()));
    assert_eq!(&*commands.borrow(), &[false, true, false]);
    Ok(())
}

#[test]
fn enabling_one_radio_leaves_airplane_mode_without_enabling_the_other() -> fairing::Result<()> {
    let mut h = shell()?;
    h.shell
        .set_setting(keys::RADIO_AIRPLANE.into(), SettingValue::Bool(true));
    h.shell
        .set_setting(keys::WIFI_ENABLED.into(), SettingValue::Bool(true));
    assert!(h.shell.services().wifi.enabled());
    assert!(!h.shell.services().bluetooth.enabled());
    assert_eq!(
        h.shell.settings().get(&keys::RADIO_AIRPLANE.into()),
        Some(&SettingValue::Bool(false))
    );
    Ok(())
}

#[test]
fn unread_tracking_survives_removing_read_items_and_retention() -> fairing::Result<()> {
    let mut h = shell()?;
    h.frames(3);
    let old = NotificationId::of("old-read");
    let new = NotificationId::of("new-unread");
    h.shell.notify(Notification::new(old, "read"));
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(2);
    h.shell.handle().toggle_overlay();
    h.frames(2);
    h.shell.notify(Notification::new(new, "unread"));
    assert!(h.shell.dismiss_notification(old));
    assert_eq!(h.shell.notifications().unread(), 1);
    assert!(h.shell.dismiss_notification(new));
    assert_eq!(h.shell.notifications().unread(), 0);
    let mut center = fairing::notify::NotificationCenter::new(1);
    center.push(Notification::new(old, "old"));
    center.push(Notification::new(new, "new"));
    assert_eq!(center.unread(), 1);
    assert!(center.dismiss(new));
    assert_eq!(center.unread(), 0);
    Ok(())
}

#[test]
fn literal_level_names_take_precedence_over_aliases() -> fairing::Result<()> {
    for name in ["bottom", "top"] {
        let mut cfg = access_config(&["viewer", name, "maintainer"], Some("top"));
        cfg.access.gates.insert("service".into(), name.into());
        let access = Access::from_config(&cfg.access)?;
        assert_eq!(access.table().index_of(name), Some(Level(1)));
        assert_eq!(access.session().subject.level, Level(0));
        assert!(!access.allows(&Gate::from("service")));
    }
    let mut cfg = access_config(&["viewer", "operator"], Some("top"));
    cfg.access.gates.insert("public".into(), "bottom".into());
    cfg.access.gates.insert("service".into(), "top".into());
    let access = Access::from_config(&cfg.access)?;
    assert!(access.allows(&Gate::from("public")));
    assert!(!access.allows(&Gate::from("service")));
    Ok(())
}

#[test]
fn dismissing_the_only_unread_item_clears_the_badge() -> fairing::Result<()> {
    let mut h = shell()?;
    h.frames(3);
    let read = NotificationId::of("read");
    let unread = NotificationId::of("unread");
    h.shell.notify(Notification::new(read, "Read item"));
    assert_eq!(h.shell.notifications().unread(), 1);
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    assert_eq!(h.shell.notifications().unread(), 0);
    h.shell.handle().toggle_overlay();
    h.frames(3);
    h.shell.notify(Notification::new(unread, "Unread item"));
    assert_eq!(h.shell.notifications().unread(), 1);
    assert!(h.shell.dismiss_notification(unread));
    assert_eq!(h.shell.notifications().len(), 1);
    assert_eq!(
        h.shell.notifications().get(read).map(|n| n.title.as_str()),
        Some("Read item")
    );
    assert_eq!(h.shell.notifications().unread(), 0);
    Ok(())
}

struct MutablePolicy(Rc<Cell<bool>>, &'static str);
impl AccessPolicy for MutablePolicy {
    fn allows(&self, _: &Subject, gate: &Gate) -> bool {
        gate.as_str() != self.1 || self.0.get()
    }
}

#[test]
fn revoked_policy_closes_existing_screens_without_a_session_reset() -> fairing::Result<()> {
    for replace_policy in [false, true] {
        let mut cfg = access_config(&["viewer", "operator"], Some("bottom"));
        cfg.motion.reduce = true;
        let allowed = Rc::new(Cell::new(true));
        let policy = MutablePolicy(Rc::clone(&allowed), "restricted");
        let mut h = Harness::from_builder(move |ctx| {
            let mut sh = Shell::builder(cfg).build(ctx)?;
            sh.access_mut().set_policy(Box::new(policy));
            sh.add(screen("restricted", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("RESTRICTED-CONTENT");
            }));
            Ok(sh)
        })?;
        h.shell.launch(LaunchAction::open("restricted"));
        h.frames(5);
        assert!(h.texts().iter().any(|(t, _)| t == "RESTRICTED-CONTENT"));
        if replace_policy {
            h.shell.access_mut().set_policy(Box::new(MutablePolicy(
                Rc::new(Cell::new(false)),
                "restricted",
            )));
        } else {
            allowed.set(false);
        }
        h.frames(3);
        assert!(!h.shell.access().allows(&Gate::from("restricted")));
        assert!(h.shell.workspace().find("restricted").is_none());
        assert!(!h.texts().iter().any(|(t, _)| t == "RESTRICTED-CONTENT"));
        let subject = h.shell.access().session().subject.clone();
        h.shell.handle().set_subject(subject);
        h.frame();
        assert!(h.shell.workspace().find("restricted").is_none());
    }
    Ok(())
}

#[test]
fn policy_revocation_hides_panels_in_one_frame_even_during_an_animated_close() -> fairing::Result<()>
{
    for gate in ["overlay.open", "nav.recents"] {
        for returning in [false, true] {
            let allowed = Rc::new(Cell::new(true));
            let mut cfg = access_config(&["viewer", "operator"], Some("bottom"));
            cfg.motion.reduce = false;
            let mut h = Harness::new(cfg, fairing::services::mock::services())?;
            h.shell
                .access_mut()
                .set_policy(Box::new(MutablePolicy(Rc::clone(&allowed), gate)));
            h.frames(3);
            h.shell.launch(if gate == "overlay.open" {
                LaunchAction::OpenOverlay
            } else {
                LaunchAction::OpenOverview
            });
            h.frames(60);
            if gate == "overlay.open" {
                assert!(!h.shell.overlay().is_closed());
            } else {
                assert!(h.shell.workspace().is_overview_open());
            }
            if returning {
                h.shell.back();
                h.frame();
            }
            allowed.set(false);
            h.frame();
            assert!(h.shell.overlay().is_closed());
            assert!(!h.shell.workspace().is_overview_open());
        }
    }
    Ok(())
}

#[test]
fn simultaneous_deadlines_are_applied_before_commands_and_input() -> fairing::Result<()> {
    for wake_input in [false, true] {
        let mut cfg = access_config(&["viewer", "operator", "maintainer"], Some("bottom"));
        cfg.motion.reduce = true;
        cfg.access.temporary_secs = 1;
        cfg.access.session_timeout_secs = 1;
        cfg.access
            .pin_table
            .pins
            .insert("maintainer".into(), "9876".into());
        cfg.access
            .gates
            .insert("elevate".into(), "maintainer".into());
        cfg.access.gates.insert("operate".into(), "operator".into());
        let count = Rc::new(Cell::new(0));
        let ran = Rc::clone(&count);
        let mut h = Harness::from_builder(move |ctx| {
            let mut sh = Shell::builder(cfg).build(ctx)?;
            sh.add(action("elevate", |_| {}));
            sh.add(action("operate", move |_| ran.set(ran.get() + 1)));
            Ok(sh)
        })?;
        h.frames(3);
        h.shell.handle().set_subject(Subject {
            level: Level(1),
            ..Subject::default()
        });
        h.frames(2);
        h.shell.launch(LaunchAction::run("elevate"));
        h.frames(3);
        for key in [
            egui::Key::Num9,
            egui::Key::Num8,
            egui::Key::Num7,
            egui::Key::Num6,
        ] {
            h.key(key);
        }
        h.frames(2);
        assert_eq!(h.shell.access().session().subject.level, Level(2));
        h.sleep(2.0);
        if wake_input {
            h.move_to(egui::pos2(500.0, 300.0));
        }
        h.shell.handle().launch(LaunchAction::run("operate"));
        h.frame();
        assert_eq!(h.shell.access().session().subject.level, Level(0));
        assert_eq!(count.get(), 0);
        h.frame();
        assert_eq!(h.shell.access().session().subject.level, Level(0));
    }
    Ok(())
}

#[test]
fn queued_badges_apply_to_newly_registered_icons() -> fairing::Result<()> {
    let mut h = shell()?;
    h.frames(3);
    h.shell.add(
        screen("fresh", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("fresh");
        })
        .icon(fairing::icons::builtin::WIFI)
        .desktop(),
    );
    h.shell.handle().set_badge("fresh", Some(Badge::Count(7)));
    h.frames(2);
    let badge = |h: &Harness| {
        h.shell
            .desktop()
            .pages()
            .iter()
            .flat_map(fairing::desktop::Page::slots)
            .find(|(_, _, slot)| slot.id == "fresh")
            .and_then(|(_, _, slot)| slot.badge.clone())
    };
    assert_eq!(badge(&h), Some(Badge::Count(7)));
    h.shell.handle().set_badge("fresh", Some(Badge::Count(7)));
    h.frame();
    assert_eq!(badge(&h), Some(Badge::Count(7)));
    Ok(())
}
