//! Security boundaries: device writes, expired sessions and secret-free diagnostics.
#![cfg(all(feature = "settings", feature = "mock"))]

use fairing::access::{Access, AccessEvent, Gate, Subject};
use fairing::testing::{access_config, Harness};
use fairing::widgets::WidgetPainters;
use fairing::{action, LaunchAction, Level, SettingValue, Shell, ShellConfig, ShellEvent};
use std::cell::Cell;
use std::rc::Rc;

fn missing(what: &str) -> fairing::Error {
    fairing::Error::Config(what.to_owned())
}

fn config() -> ShellConfig {
    let mut cfg = access_config(&["viewer", "operator"], Some("bottom"));
    cfg.motion.reduce = true;
    cfg.access
        .pin_table
        .pins
        .insert("operator".to_owned(), "7391".to_owned());
    cfg
}

fn operator() -> Subject {
    Subject {
        level: Level(1),
        ..Subject::default()
    }
}

fn harness(cfg: ShellConfig, painters: WidgetPainters) -> fairing::Result<Harness> {
    Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(cfg)
            .services(fairing::services::mock::services())
            .widget_painters(painters)
            .build(ctx)?;
        fairing::settings::add_all(&mut shell, &fairing::settings::SettingsConfig::default());
        Ok(shell)
    })
}

fn unlock(h: &mut Harness) {
    for key in [
        egui::Key::Num7,
        egui::Key::Num3,
        egui::Key::Num9,
        egui::Key::Num1,
    ] {
        h.key(key);
    }
    h.frames(2);
}

/// A radio screen may be readable while its setting is protected. A denied write must
/// leave the backend alone; the same pending write must work once authorized.
#[test]
fn radio_screens_wait_for_setting_authorization() -> fairing::Result<()> {
    for (screen, key, label) in [
        ("settings.wifi", "wifi.enabled", "Wi-Fi"),
        ("settings.bluetooth", "bluetooth.enabled", "Bluetooth"),
    ] {
        let mut cfg = config();
        cfg.access
            .gates
            .insert(key.to_owned(), "operator".to_owned());
        let mut h = harness(cfg, WidgetPainters::new())?;
        h.shell.launch(LaunchAction::open(screen));
        h.frames(6);
        let enabled = |h: &Harness| {
            if key == "wifi.enabled" {
                h.shell.services().wifi.snapshot().enabled
            } else {
                h.shell.services().bluetooth.snapshot().enabled
            }
        };
        let before = enabled(&h);
        assert!(!h.shell.access().allows(&Gate::from(key)));
        // The lower of the matching labels is the switch row, below the page title.
        let row = h
            .texts()
            .into_iter()
            .filter(|(text, _)| text == label)
            .max_by(|a, b| a.1.center().y.total_cmp(&b.1.center().y))
            .ok_or_else(|| missing("radio row not drawn"))?
            .1;
        let _ = h.shell.poll_events();
        h.tap(row.center());
        h.frames(2);
        assert_eq!(
            enabled(&h),
            before,
            "unauthorized {key} reached its backend"
        );
        assert!(h.shell.settings().get(&key.to_owned().into()).is_none());
        assert!(h.shell.unlock_prompt_visible());
        let events = h.shell.poll_events();
        assert!(events.iter().any(|e| matches!(e, ShellEvent::Access(
            AccessEvent::UnlockRequested { gate, .. }) if gate.as_str() == key)));
        assert!(!events
            .iter()
            .any(|e| matches!(e, ShellEvent::SettingChanged { .. })));
        unlock(&mut h);
        assert_eq!(enabled(&h), !before, "authorized {key} was not applied");
        assert_eq!(
            h.shell.settings().get(&key.to_owned().into()),
            Some(&SettingValue::Bool(!before))
        );
    }
    Ok(())
}

/// The brightness slider uses the same guarded path as radio switches.
#[test]
fn brightness_waits_for_setting_authorization() -> fairing::Result<()> {
    let mut cfg = config();
    cfg.access
        .gates
        .insert("display.brightness".to_owned(), "operator".to_owned());
    let axis = Rc::new(Cell::new(None::<egui::Rect>));
    let captured = Rc::clone(&axis);
    let painters = WidgetPainters::new().slider(move |_, look| captured.set(Some(look.axis)));
    let mut h = harness(cfg, painters)?;
    h.shell.launch(LaunchAction::open("settings.display"));
    h.frames(6);
    let before = h.shell.services().display.brightness();
    let axis = axis
        .get()
        .ok_or_else(|| missing("brightness slider not drawn"))?;
    let _ = h.shell.poll_events();
    h.tap(egui::pos2(axis.right(), axis.center().y));
    h.frames(2);
    assert!(
        h.shell.unlock_prompt_visible(),
        "slider did not request a change"
    );
    assert_eq!(h.shell.services().display.brightness(), before);
    assert!(!h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::SettingChanged { .. })));
    unlock(&mut h);
    assert_ne!(h.shell.services().display.brightness(), before);
    Ok(())
}

/// An overdue lock and timeout both take effect even if the wake frame carries input.
#[test]
fn overdue_idle_and_session_deadlines_precede_wake_input() -> fairing::Result<()> {
    for (idle, timeout) in [(60, 0), (0, 60), (60, 60)] {
        for with_input in [false, true] {
            let mut cfg = config();
            cfg.access.unlock_mode = "switch".to_owned();
            cfg.access.idle_lock_secs = idle;
            cfg.access.session_timeout_secs = timeout;
            let mut h = harness(cfg, WidgetPainters::new())?;
            h.shell.handle().set_subject(operator());
            h.frames(3);
            h.sleep(61.0);
            if with_input {
                h.move_to(egui::pos2(500.0, 300.0));
            }
            h.frame();
            assert_eq!(h.shell.access().session().subject.level, Level(0));
            assert_eq!(h.shell.lock_screen_visible(), idle != 0);
        }
    }
    Ok(())
}

/// Input before the deadline still starts a new inactivity interval.
#[test]
fn timely_input_renews_idle_and_session_deadlines() -> fairing::Result<()> {
    let mut cfg = config();
    cfg.access.unlock_mode = "switch".to_owned();
    cfg.access.idle_lock_secs = 60;
    cfg.access.session_timeout_secs = 60;
    let mut h = harness(cfg, WidgetPainters::new())?;
    h.shell.handle().set_subject(operator());
    h.frames(3);
    h.sleep(50.0);
    h.move_to(egui::pos2(500.0, 300.0));
    h.frame();
    h.sleep(20.0);
    h.frame();
    assert_eq!(h.shell.access().session().subject.level, Level(1));
    assert!(!h.shell.lock_screen_visible());
    h.sleep(41.0);
    h.frame();
    assert_eq!(h.shell.access().session().subject.level, Level(0));
    assert!(h.shell.lock_screen_visible());
    Ok(())
}

/// The command queue checks a protected action against the current, unexpired session.
#[test]
fn queued_actions_cannot_use_expired_grants() -> fairing::Result<()> {
    let mut cfg = config();
    cfg.access.temporary_secs = 1;
    cfg.access
        .gates
        .insert("privileged".to_owned(), "operator".to_owned());
    let count = Rc::new(Cell::new(0));
    let ran = Rc::clone(&count);
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(cfg).build(ctx)?;
        shell.add(action("privileged", move |_| ran.set(ran.get() + 1)));
        Ok(shell)
    })?;
    h.frames(3);
    h.shell.launch(LaunchAction::run("privileged"));
    h.frames(3);
    unlock(&mut h);
    assert_eq!(
        count.get(),
        1,
        "control: the grant never reached the action"
    );
    h.sleep(2.0);
    h.shell.handle().launch(LaunchAction::run("privileged"));
    h.frame();
    assert_eq!(count.get(), 1, "the expired grant executed another action");
    assert_eq!(h.shell.access().session().subject.level, Level(0));
    assert!(h.shell.unlock_prompt_visible());
    // A deliberate fresh grant from the trusted integrator still takes effect in queue order.
    h.shell.handle().set_subject(operator());
    h.shell.handle().launch(LaunchAction::run("privileged"));
    h.frame();
    assert_eq!(count.get(), 2);
    Ok(())
}

#[test]
fn config_debug_redacts_both_secret_tables() {
    let mut cfg = config();
    cfg.access
        .pattern_table
        .patterns
        .insert("operator".to_owned(), "1-2-3-6-9".to_owned());
    for output in [
        format!("{cfg:?}"),
        format!("{:?}", cfg.access),
        format!("{:?}", cfg.access.pin_table),
        format!("{:?}", cfg.access.pattern_table),
    ] {
        assert!(!output.contains("7391"));
        assert!(!output.contains("1-2-3-6-9"));
        assert!(
            output.contains("operator"),
            "safe level context should remain"
        );
    }
}

#[test]
fn invalid_patterns_leave_no_secret_in_errors() -> fairing::Result<()> {
    for pattern in ["1-3-6-9", "1-2-private-token-4", "1-2-1-4"] {
        let mut cfg = config();
        cfg.access
            .pattern_table
            .patterns
            .insert("operator".to_owned(), pattern.to_owned());
        let error = Access::from_config(&cfg.access)
            .err()
            .ok_or_else(|| missing("invalid pattern accepted"))?;
        let text = error.to_string();
        assert!(text.contains("pattern_table") && text.contains("operator"));
        assert!(!text.contains(pattern));
        assert!(!text.contains("private-token"));
        assert!(!text.contains("1-2-3-6-9"), "reconstructed path leaked");
    }
    Ok(())
}

#[test]
fn toml_errors_leave_no_credential_source_or_invalid_value() -> fairing::Result<()> {
    for input in [
        "[access.pin_table]\noperator = 739173917391\n",
        "[access.pattern_table]\noperator = [\"private-pattern-token\"]\n",
        "[access.pin_table]\noperator = \"private-pin-token\" trailing\n",
    ] {
        let error = ShellConfig::from_toml(input)
            .err()
            .ok_or_else(|| missing("malformed configuration accepted"))?;
        let text = error.to_string();
        for secret in ["739173917391", "private-pattern-token", "private-pin-token"] {
            assert!(!text.contains(secret));
        }
    }
    Ok(())
}
