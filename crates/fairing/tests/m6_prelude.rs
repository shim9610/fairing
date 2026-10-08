//! **One import is enough for an ordinary app**. Before the prelude, matching
//! `ShellEvent` meant finding `InstanceId` in `workspace`, `SettingKey` in `settings` and
//! `PowerRequest` in `services`; a helper returning a declaration had to find `ScreenDecl` in
//! `screen`. This file uses nothing but the prelude and the test harness.

use fairing::prelude::*;
use fairing::testing::{single_level_access, Harness};

/// A declaration built in a helper — which needs the type's name.
fn dashboard() -> ScreenDecl {
    screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        ui.heading("Dashboard");
        if ui.button("Hello").clicked() {
            cx.shell.toast("Hello");
        }
    })
    .title("Dashboard")
    .icon(icon::GAUGE)
}

#[test]
fn the_prelude_builds_a_shell_and_matches_its_events() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.shell.add(dashboard());
    h.shell.launch(LaunchAction::open("dashboard"));
    h.frames(3);
    let mut opened: Option<(String, InstanceId)> = None;
    for event in h.shell.poll_events() {
        match event {
            ShellEvent::ScreenOpened { id, instance } => opened = Some((id, instance)),
            ShellEvent::SettingChanged { key, value } => {
                let _: (SettingKey, SettingValue) = (key, value);
            }
            ShellEvent::PowerRequest(request) => {
                let _: PowerRequest = request;
            }
            _ => {}
        }
    }
    let (id, instance) = opened.ok_or_else(|| Error::Config("no ScreenOpened".to_owned()))?;
    assert_eq!(id, "dashboard");
    assert_ne!(instance, InstanceId::NONE);
    Ok(())
}
