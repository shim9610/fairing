//! **Saved settings come back through one explicit call**. The shell writes no files: a
//! change reaches the integrator as `SettingChanged`, to store where the device keeps such things,
//! and `Shell::restore_settings` hands the stored values back at start — into the table, to the
//! backends, the theme and the language — with no gate and no event of its own.
#![cfg(feature = "mock")]

use fairing::access::AccessEvent;
use fairing::settings::{keys, SettingKey, SettingValue};
use fairing::testing::{access_config, single_level_access, Harness};
use fairing::{Shell, ShellConfig, ShellEvent};

/// A shell on the mock services, two frames in.
fn shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .services(fairing::services::mock::services())
            .build(ctx)
    })?;
    h.frames(2);
    Ok(h)
}

fn key(name: &'static str) -> SettingKey {
    SettingKey::from(name)
}

/// Every text drawn on the next frame.
fn texts(h: &mut Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// What is restored is in the table and at the backend, and no `SettingChanged` says so — the
/// values are stored already.
#[test]
fn restored_values_reach_the_table_and_the_backend_without_an_event() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let _ = h.shell.poll_events();
    h.shell.restore_settings([
        ("app.heater", SettingValue::Int(42)),
        (keys::DISPLAY_BRIGHTNESS, SettingValue::Int(30)),
    ]);
    h.frames(1);
    assert_eq!(
        h.shell.settings().get(&key("app.heater")),
        Some(&SettingValue::Int(42))
    );
    assert_eq!(h.shell.services().display.brightness(), Some(30));
    let events = h.shell.poll_events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::SettingChanged { .. })),
        "{events:?}"
    );
    Ok(())
}

/// Restoring checks no gate: with every gate at the top level, a saved value still goes in and no
/// unlock is asked for — where the same change from the panel is refused.
#[test]
fn restoring_checks_no_gate() -> fairing::Result<()> {
    let mut h = shell(access_config(&["viewer", "admin"], Some("admin")))?;
    let _ = h.shell.poll_events();
    h.shell.set_setting(key("app.heater"), SettingValue::Int(1));
    let refused = h.shell.poll_events();
    assert!(
        h.shell.settings().get(&key("app.heater")).is_none(),
        "a change past the gate went in"
    );
    assert!(
        refused
            .iter()
            .any(|e| matches!(e, ShellEvent::Access(AccessEvent::UnlockRequested { .. }))),
        "{refused:?}"
    );
    h.shell
        .restore_settings([("app.heater", SettingValue::Int(42))]);
    let events = h.shell.poll_events();
    assert_eq!(
        h.shell.settings().get(&key("app.heater")),
        Some(&SettingValue::Int(42))
    );
    assert!(
        !events.iter().any(|e| matches!(e, ShellEvent::Access(_))),
        "{events:?}"
    );
    Ok(())
}

/// The theme comes back at once: the light palette is on the next frame, with no crossfade to
/// watch from the dark the config started in.
#[test]
fn a_restored_theme_is_there_at_once() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    assert!(h.shell.theme().dark, "the config starts dark");
    h.shell
        .restore_settings([(keys::THEME_DARK, SettingValue::Bool(false))]);
    h.frames(1);
    assert!(!h.shell.theme().dark);
    let light = *h.shell.palettes().1;
    assert_eq!(
        h.shell.theme().palette,
        light,
        "the light palette is not on at once"
    );
    Ok(())
}

/// The language comes back too: the 12-hour clock restored with Korean reads 오전 or 오후 on the
/// next frame.
#[test]
fn a_restored_language_draws_on_the_next_frame() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.restore_settings([
        (keys::UI_CLOCK_12H, SettingValue::Bool(true)),
        (keys::UI_LOCALE, SettingValue::Text("ko".into())),
    ]);
    h.frames(1);
    let all = texts(&mut h);
    assert!(
        all.iter()
            .any(|t| t.starts_with("오전 ") || t.starts_with("오후 ")),
        "{all:?}"
    );
    Ok(())
}

/// Airplane mode goes last and only ever turns the radios off: restored off, it leaves Wi-Fi to
/// its own key; restored on, it wins over a Wi-Fi key that came after it.
#[test]
fn airplane_mode_restores_after_the_radios() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.restore_settings([
        (keys::WIFI_ENABLED, SettingValue::Bool(false)),
        (keys::RADIO_AIRPLANE, SettingValue::Bool(false)),
    ]);
    assert!(
        !h.shell.services().wifi.enabled(),
        "airplane mode off turned Wi-Fi back on"
    );
    h.shell.restore_settings([
        (keys::RADIO_AIRPLANE, SettingValue::Bool(true)),
        (keys::WIFI_ENABLED, SettingValue::Bool(true)),
    ]);
    assert!(
        !h.shell.services().wifi.enabled(),
        "Wi-Fi is on in airplane mode"
    );
    assert!(!h.shell.services().bluetooth.enabled());
    Ok(())
}
