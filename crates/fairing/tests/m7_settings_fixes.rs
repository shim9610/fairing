//! Regression tests for the built-in settings screens and the service simulators fixed before
//! 0.1.0: the sound screen's clamp, the power confirmation's lifetime, `MockWifi` and
//! `MockBluetooth`.

#![cfg(all(feature = "settings", feature = "mock"))]

use fairing::services::mock::{MockBluetooth, MockWifi, WifiMsg};
use fairing::services::{Backend, BluetoothBackend, Services, WifiBackend, WifiState};
use fairing::settings::{add_all, keys, SettingValue, SettingsConfig};
use fairing::testing::{single_level_access, Harness};
use fairing::widgets::WidgetPainters;
use fairing::{LaunchAction, Shell};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

// ── helpers ──────────────────────────────────────────────────────────────────────────────

/// Every text drawn this frame with where it was drawn.
fn drawn(h: &mut Harness) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => out.push((
                t.galley.text().to_owned(),
                t.galley.rect.translate(t.pos.to_vec2()),
            )),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn settle(h: &mut Harness) {
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    h.frames(2);
}

// ── settings.sound ───────────────────────────────────────────────────────────────────────

/// The volume slider shows an out-of-range stored volume clamped, as the shell applies it.
#[test]
fn the_sound_screen_clamps_a_stored_volume_like_the_shell() -> fairing::Result<()> {
    for (stored, want) in [(300_i64, 100.0_f32), (-5, 0.0)] {
        let seen: Rc<RefCell<Option<f32>>> = Rc::default();
        let sink = Rc::clone(&seen);
        let painters = WidgetPainters::new().slider(move |_, look| {
            *sink.borrow_mut() = Some(look.value);
        });
        let mut h = Harness::from_builder(move |ctx| {
            let mut shell = Shell::builder(single_level_access())
                .services(Services::null())
                .widget_painters(painters)
                .build(ctx)?;
            add_all(&mut shell, &SettingsConfig::default());
            Ok(shell)
        })?
        .with_size(1024.0, 1400.0);
        h.shell
            .restore_settings([(keys::AUDIO_VOLUME, SettingValue::Int(stored))]);
        h.shell.launch(LaunchAction::open("settings.sound"));
        settle(&mut h);
        let shown = *seen.borrow();
        assert_eq!(
            shown,
            Some(want),
            "audio.volume = Int({stored}) is applied as {want} by the shell but the slider shows {shown:?}"
        );
    }
    Ok(())
}

// ── settings.power ───────────────────────────────────────────────────────────────────────

/// A pending restart confirmation is gone when the power screen is left and opened again.
#[test]
fn the_power_confirmation_does_not_survive_leaving_the_screen() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access())
            .services(fairing::services::mock::services())
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        Ok(shell)
    })?
    .with_size(1024.0, 1400.0);
    h.shell.launch(LaunchAction::open("settings.power"));
    settle(&mut h);
    let restart = drawn(&mut h)
        .into_iter()
        .find(|(t, _)| t == "Restart")
        .map(|(_, r)| r);
    let Some(restart) = restart else {
        return Err(fairing::Error::Config("no Restart row drawn".to_owned()));
    };
    h.tap(restart.center());
    settle(&mut h);
    let asking = drawn(&mut h).iter().any(|(t, _)| t == "Restart now");
    assert!(
        asking,
        "precondition: tapping Restart asks for confirmation"
    );

    // Leave the screen, and come back.
    h.shell.back();
    settle(&mut h);
    let still_open = drawn(&mut h).iter().any(|(t, _)| t == "Restart now");
    assert!(!still_open, "precondition: the power screen was left");
    h.shell.launch(LaunchAction::open("settings.power"));
    settle(&mut h);
    let texts: Vec<String> = drawn(&mut h).into_iter().map(|(t, _)| t).collect();
    assert!(
        !texts.iter().any(|t| t == "Restart now"),
        "coming back to settings.power still shows the old confirmation: {texts:?}"
    );
    Ok(())
}

// ── MockWifi ─────────────────────────────────────────────────────────────────────────────

/// `MockWifi` scanning while connected stays connected.
#[test]
fn mock_wifi_stays_connected_through_a_scan() {
    let mut wifi = MockWifi::new();
    let t0 = Instant::now();
    wifi.poll_at(t0);
    assert!(wifi.connect("guest", None, false).is_ok());
    let t1 = t0 + wifi.connect_delay;
    wifi.poll_at(t1);
    assert!(
        matches!(wifi.snapshot().state, WifiState::Connected { .. }),
        "precondition: connected, got {:?}",
        wifi.snapshot().state
    );
    assert!(wifi.scan().is_ok());
    wifi.poll_at(t1 + wifi.scan_delay + Duration::from_millis(1));
    assert!(
        matches!(wifi.snapshot().state, WifiState::Connected { .. }),
        "after a scan the connected radio reports {:?}",
        wifi.snapshot().state
    );
}

/// `WifiMsg::Enabled(false)` calls off a pending connect, as `set_enabled(false)` does.
#[test]
fn mock_wifi_disabled_by_message_cancels_a_pending_connect() {
    let mut wifi = MockWifi::new();
    let control = wifi.control();
    let t0 = Instant::now();
    wifi.poll_at(t0);
    assert!(wifi.connect("guest", None, false).is_ok());
    assert!(control.send(WifiMsg::Enabled(false)));
    wifi.poll_at(t0 + Duration::from_millis(10));
    wifi.poll_at(t0 + wifi.connect_delay + Duration::from_millis(10));
    let snap = wifi.snapshot();
    assert!(
        !matches!(snap.state, WifiState::Connected { .. }) || snap.enabled,
        "the radio is off (enabled = {}) yet reports {:?}",
        snap.enabled,
        snap.state
    );
}

// ── MockBluetooth ────────────────────────────────────────────────────────────────────────

/// Turning `MockBluetooth` off stops discovery and finds nothing more.
#[test]
fn mock_bluetooth_off_stops_discovery() {
    let mut bt = MockBluetooth::new();
    let t0 = Instant::now();
    bt.poll_at(t0);
    assert!(bt.set_discovering(true).is_ok());
    assert!(bt.set_enabled(false).is_ok());
    bt.poll_at(t0 + bt.discover_delay + Duration::from_millis(10));
    let snap = bt.snapshot();
    assert!(
        !snap.discovering && snap.devices.len() == 1,
        "with the radio off: discovering = {}, devices = {:?}",
        snap.discovering,
        snap.devices.iter().map(|d| &d.name).collect::<Vec<_>>()
    );
}
