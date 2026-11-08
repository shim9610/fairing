//! Protected host integration must retain execution-time grants and replacement isolation.
#![allow(
    clippy::unwrap_used,
    reason = "public integration regression assertions"
)]
use fairing::{
    access::{
        AccessEvent, AuthOutcome, Permission, Protected, SessionController, SessionGrant, Subject,
    },
    egui,
    testing::Harness,
    Gate, LaunchAction, ProtectedShell, Shell, ShellConfig, ShellEvent,
};
use std::time::Duration;

#[derive(Clone, Copy)]
struct Operate;
impl Permission for Operate {
    fn gate(self) -> Gate {
        Gate::borrowed("device.operate")
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<ProtectedShell> {
    let config = ShellConfig::default().configure(|c| {
        c.access.session_tokens = true;
        c.motion.reduce = true;
    });
    let mut shell = Shell::builder(config).build_protected(ctx)?;
    shell.add(Protected::new(
        Operate,
        fairing::action("work", |cx| {
            if let Some(count) = cx.app_mut::<usize>() {
                *count += 1;
            }
        }),
    ));
    Ok(shell)
}

fn login(h: &mut Harness<ProtectedShell>, expires: bool) -> SessionController {
    h.shell.cancel_authentication();
    let _ = h.shell.poll_events();
    h.shell.launch(LaunchAction::run("work"));
    let request = h
        .shell
        .poll_events()
        .into_iter()
        .find_map(|event| match event {
            ShellEvent::Access(AccessEvent::SessionRequested { request, .. }) => Some(request),
            _ => None,
        })
        .unwrap();
    let now = h.now();
    let (owner, token) = SessionController::issue(
        Subject::default(),
        vec![SessionGrant::base_for(Operate)],
        expires.then(|| now + Duration::from_secs(1)),
        now,
        fairing::services::Waker::new(&h.ctx),
    )
    .unwrap();
    h.shell
        .accept_auth_response_with(request.respond(AuthOutcome::Session(token)), &mut ());
    owner
}

#[test]
fn protected_harness_lends_app_state_and_revocation_blocks_later_work() -> fairing::Result<()> {
    let mut h = Harness::from_builder(build)?.with_app(0usize);
    h.frame();
    h.shell.handle().launch(LaunchAction::run("work"));
    h.frame();
    assert_eq!(h.app_mut::<usize>().copied(), Some(0));
    let mut owner = login(&mut h, false);
    h.shell.handle().launch(LaunchAction::run("work"));
    h.frame();
    assert_eq!(h.app_mut::<usize>().copied(), Some(1));
    owner.revoke();
    h.shell.handle().launch(LaunchAction::run("work"));
    h.frame();
    assert_eq!(h.app_mut::<usize>().copied(), Some(1));
    assert!(h.shell.observe().session_token().is_none());
    Ok(())
}

#[test]
#[allow(clippy::panic, reason = "inject a replacement factory unwind")]
fn replacement_closes_old_handles_and_failures_leave_protected_dispatch_blocked(
) -> fairing::Result<()> {
    let mut h = Harness::from_builder(build)?;
    let old = h.shell.handle();
    h.shell.restart_with(|| build(&h.ctx)).unwrap();
    assert!(old.try_send(fairing::shell::Command::Home).is_err());
    assert!(!h.shell.observe().is_faulted());
    let current = h.shell.handle();
    let result = h
        .shell
        .restart_with(|| Err(fairing::Error::Config("synthetic factory failure".into())));
    assert!(matches!(
        result,
        Err(fairing::recovery::RestartError::Build(_))
    ));
    assert!(h.shell.is_faulted());
    assert!(current.try_send(fairing::shell::Command::Home).is_err());
    assert!(h
        .shell
        .restart_with(|| panic!("synthetic factory unwind"))
        .is_err());
    assert!(h.shell.is_faulted());
    h.shell.restart_with(|| build(&h.ctx)).unwrap();
    h.frame();
    assert!(!h.shell.is_faulted());
    assert!(h.shell.observe().session_token().is_none());
    Ok(())
}

#[cfg(feature = "settings")]
#[test]
fn protected_settings_register_with_application_values_without_implying_write_grants(
) -> fairing::Result<()> {
    let mut h = Harness::from_builder(build)?;
    fairing::settings::add_all_protected(
        &mut h.shell,
        &fairing::settings::SettingsConfig::default().ignoring_capabilities(),
        |_| Operate,
    );
    assert!(h.shell.observe().has_screen("settings.display"));
    assert!(h.shell.observe().has_screen("settings.credentials"));
    let _owner = login(&mut h, false);
    assert!(h.shell.observe().allows(&Operate.gate()));
    assert!(!h
        .shell
        .observe()
        .allows(&Gate::borrowed("display.brightness")));
    assert!(!h
        .shell
        .observe()
        .allows(&Gate::borrowed("settings.credentials.remove")));
    h.frames(2);
    Ok(())
}

#[test]
fn virtual_deadline_is_enforced_before_queued_protected_work() -> fairing::Result<()> {
    let mut h = Harness::from_builder(build)?.with_app(0usize);
    h.frame();
    let _owner = login(&mut h, true);
    h.sleep(2.0);
    h.shell.handle().launch(LaunchAction::run("work"));
    h.frame();
    assert_eq!(h.app_mut::<usize>().copied(), Some(0));
    assert!(h.shell.observe().session_token().is_none());
    Ok(())
}
