//! The M1 integration test for services/mock plus access plus config, along with the Mock
//! scenario tests.
//!
//! The rules: time is the shell's monotonic clock alone (`Backend::poll_at(now)`) — headless,
//! `Harness::run_for(secs)` is the logical time, so no `sleep` is needed (blocking is refused
//! anyway).
//! The tests are written as `fn … -> fairing::Result<()>` (the `panic!`/`unwrap` lint).

use fairing::access::AccessEvent;
use fairing::services::mock::MockWifi;
use fairing::services::{Services, WifiState};
use fairing::testing::{access_config, access_config_mode, single_level_access, Harness};
use fairing::{screen, LaunchAction, ShellEvent};
use std::time::Duration;

/// With one level in the table a gate always passes, and there is no prompt event either.
#[test]
fn single_level_table_never_prompts() -> fairing::Result<()> {
    let mut h = Harness::new(
        single_level_access(),
        Services::builder()
            .clock(fairing::services::null::NullClock)
            .build(),
    )?;
    h.shell.add(screen("admin", |ui, _cx| {
        ui.label("x");
    }));
    h.shell.handle().launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(h.shell.workspace().find("admin").is_some());
    assert!(h
        .shell
        .poll_events()
        .iter()
        .all(|ev| !matches!(ev, ShellEvent::Access(AccessEvent::UnlockRequested { .. }))));
    Ok(())
}

/// Two or more levels with no `default_gate` is a config error at start-up.
#[test]
fn unassigned_gate_with_two_levels_is_a_config_error() {
    let cfg = access_config(&["viewer", "maintainer"], None);
    let ctx = egui::Context::default();
    let result = fairing::Shell::new(cfg, Services::null(), &ctx);
    assert!(matches!(result, Err(fairing::Error::Config(_))));
}

/// `routing` mode draws no prompt and raises only the `UnlockRequested` event.
/// `unlock_prompt_visible()` staying `false` is what tells it from `prompt` mode, which opens the
/// shell's prompt over the same request (`m3_access.rs`).
#[test]
fn routing_mode_never_prompts_but_emits_event() -> fairing::Result<()> {
    let mut h = Harness::new(
        access_config_mode(&["viewer", "maintainer"], Some("top"), "routing"),
        Services::builder()
            .clock(fairing::services::null::NullClock)
            .build(),
    )?;
    h.shell.add(screen("admin", |ui, _cx| {
        ui.label("x");
    }));
    h.shell.handle().launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(!h.shell.unlock_prompt_visible());
    let events = h.shell.poll_events();
    assert!(matches!(
        events.as_slice(),
        [ShellEvent::Access(AccessEvent::UnlockRequested { .. })]
    ));
    Ok(())
}

#[test]
fn mock_wifi_scan_completes_via_next_wake() -> fairing::Result<()> {
    // The scan returns at once and completion comes on the next snapshot plus next_wake. It does not block the
    // UI thread (the test finishing is itself the proof). The 50 ms delay is 3 frames of virtual time.
    let mut wifi = MockWifi::new();
    wifi.scan_delay = Duration::from_millis(50);
    let services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .wifi(wifi)
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    h.frames(1);
    assert!(h.shell.services_mut().wifi.scan().is_ok());
    assert_eq!(
        h.shell.services().wifi.snapshot().state,
        WifiState::Scanning
    );
    h.frames(1);
    assert_eq!(
        h.shell.services().wifi.snapshot().state,
        WifiState::Scanning,
        "still scanning 1/60 s later"
    );
    h.run_for(0.2);
    assert_eq!(h.shell.services().wifi.snapshot().state, WifiState::Idle);
    assert!(
        !h.repaint_requested,
        "back to idle, there is no immediate repaint request"
    );
    Ok(())
}
