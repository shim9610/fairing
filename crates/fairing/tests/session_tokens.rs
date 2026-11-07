//! Product-level split-capability, multi-permission and revocation witnesses.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "Test failures retain the violated contract"
)]

use fairing::access::{
    AccessPolicy, AuthMethod, AuthOutcome, AuthRequestId, AuthResponse, Authenticator, Credential,
    Gate, SessionController, SessionError, SessionGrant, SessionReadError, SessionToken, Subject,
};
use fairing::services::Waker;
use fairing::shell::Command;
use fairing::testing::Harness;
use fairing::{action, screen, LaunchAction, Level, Shell, ShellConfig};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Authority {
    request: Option<AuthRequestId>,
    response: Option<AuthResponse>,
    ended: Vec<fairing::access::SessionId>,
    methods: bool,
    panic_on_end: bool,
}
struct Auth(Rc<RefCell<Authority>>);
impl Authenticator for Auth {
    fn methods(&self) -> Vec<AuthMethod> {
        if self.0.borrow().methods {
            vec![AuthMethod::External {
                label: "Reader".into(),
            }]
        } else {
            Vec::new()
        }
    }
    fn begin(&mut self, request: AuthRequestId, _: &Gate, _: Instant) {
        self.0.borrow_mut().request = Some(request);
    }
    fn submit(&mut self, request: AuthRequestId, _: Credential, _: Instant) -> AuthOutcome {
        self.0.borrow_mut().request = Some(request);
        AuthOutcome::Pending
    }
    fn poll(&mut self, _: Instant) -> Option<AuthResponse> {
        self.0.borrow_mut().response.take()
    }
    fn session_ended(&mut self, session: fairing::access::SessionId, _: Instant) {
        self.0.borrow_mut().ended.push(session);
        assert!(!self.0.borrow().panic_on_end, "end-session callback fault");
    }
}
fn subject(id: &str) -> Subject {
    Subject {
        id: Some(id.into()),
        level: Level(0),
        ..Subject::default()
    }
}
type SessionHarness = (Harness, Rc<RefCell<Authority>>, Rc<Cell<usize>>);

fn harness() -> fairing::Result<SessionHarness> {
    let state = Rc::new(RefCell::new(Authority {
        methods: true,
        ..Authority::default()
    }));
    let auth = Rc::clone(&state);
    let count = Rc::new(Cell::new(0));
    let work = Rc::clone(&count);
    let h = Harness::from_builder(move |ctx| {
        let mut config = ShellConfig::default();
        config.access.session_tokens = true;
        config.motion.reduce = true;
        let mut shell = Shell::builder(config)
            .authenticator(Auth(auth))
            .command_queue_capacity(2)
            .max_commands_per_update(2)
            .build(ctx)?;
        shell.add(action("operate", move |_| work.set(work.get() + 1)).gate("measurement.run"));
        shell.add(
            screen("report", |ui, _| {
                ui.label("PROTECTED REPORT");
            })
            .gate("results.view"),
        );
        Ok(shell)
    })?;
    Ok((h, state, count))
}
fn issue(
    h: &Harness,
    name: &str,
    grants: Vec<SessionGrant>,
    expiry: Option<Instant>,
) -> (SessionController, SessionToken) {
    SessionController::issue(
        subject(name),
        grants,
        expiry,
        h.shell.now(),
        Waker::new(&h.ctx),
    )
    .unwrap()
}
fn login(h: &mut Harness, state: &Rc<RefCell<Authority>>, token: SessionToken) {
    h.shell.launch(LaunchAction::run("operate"));
    let request = state.borrow().request.expect("authentication challenge");
    state.borrow_mut().response = Some(request.respond(AuthOutcome::Session(token)));
    h.frame();
    assert!(
        h.shell.access().session_token().is_some(),
        "current session token was not installed"
    );
}

#[test]
fn controller_revoke_and_disposal_invalidate_all_readers_without_resurrection() {
    let now = Instant::now();
    let ctx = egui::Context::default();
    let (mut controller, token) = SessionController::issue(
        subject("a"),
        vec![SessionGrant::base("read")],
        None,
        now,
        Waker::new(&ctx),
    )
    .unwrap();
    let peer = token.clone();
    assert!(token.snapshot(now).unwrap().allows(&Gate::from("read")));
    controller.revoke();
    assert!(matches!(
        token.snapshot(now),
        Err(SessionReadError::Revoked)
    ));
    assert!(matches!(peer.snapshot(now), Err(SessionReadError::Revoked)));
    assert_eq!(
        controller.replace_grants(vec![SessionGrant::base("read")], None, now),
        Err(SessionError::Ended)
    );
    let (controller, token) = SessionController::issue(
        subject("b"),
        vec![SessionGrant::base("read")],
        None,
        now,
        Waker::new(&ctx),
    )
    .unwrap();
    drop(controller);
    assert!(
        matches!(token.snapshot(now), Err(SessionReadError::Revoked)),
        "controller disposal left authority alive"
    );
}

#[test]
fn independent_permissions_and_temporary_expiry_never_restore_removed_base_rights() {
    let now = Instant::now();
    let ctx = egui::Context::default();
    let end = now + Duration::from_secs(10);
    let (mut controller, token) = SessionController::issue(
        subject("engineer-analyst"),
        vec![
            SessionGrant::base("configure"),
            SessionGrant::base("export"),
        ],
        None,
        now,
        Waker::new(&ctx),
    )
    .unwrap();
    let old = token.snapshot(now).unwrap();
    controller
        .replace_grants(vec![SessionGrant::temporary("configure", end)], None, now)
        .unwrap();
    assert!(
        !old.allows(&Gate::from("export")),
        "old snapshot survived a new publication"
    );
    assert!(token
        .snapshot(now)
        .unwrap()
        .allows(&Gate::from("configure")));
    assert!(!token.snapshot(now).unwrap().allows(&Gate::from("export")));
    assert!(
        !token
            .snapshot(end)
            .unwrap()
            .allows(&Gate::from("configure")),
        "expired elevation restored old rights"
    );
}

#[test]
fn expired_sessions_cannot_be_renewed_and_invalid_publications_preserve_previous_rights() {
    let now = Instant::now();
    let end = now + Duration::from_secs(2);
    let ctx = egui::Context::default();
    let (mut controller, token) = SessionController::issue(
        subject("a"),
        vec![SessionGrant::base("read")],
        Some(end),
        now,
        Waker::new(&ctx),
    )
    .unwrap();
    assert_eq!(
        controller.replace_grants(vec![SessionGrant::base("other")], None, now),
        Err(SessionError::UnknownGate)
    );
    assert!(token.snapshot(now).unwrap().allows(&Gate::from("read")));
    assert!(matches!(
        token.snapshot(end),
        Err(SessionReadError::Expired)
    ));
    assert_eq!(
        controller.replace_grants(vec![SessionGrant::base("read")], None, end),
        Err(SessionError::Ended)
    );
    assert!(matches!(
        token.snapshot(end),
        Err(SessionReadError::Revoked)
    ));
}

#[test]
fn token_mode_denies_legacy_bypasses_and_supports_one_level_multi_permission_sessions(
) -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    assert!(
        !h.shell.access().allows(&Gate::from("measurement.run")),
        "single-level bypass opened token mode"
    );
    h.shell.access_mut().set_subject(subject("injected"));
    h.frame();
    assert!(!h.shell.access().allows(&Gate::from("measurement.run")));
    let (_controller, token) = issue(
        &h,
        "operator",
        vec![
            SessionGrant::base("measurement.run"),
            SessionGrant::base("results.view"),
        ],
        None,
    );
    login(&mut h, &state, token);
    assert_eq!(count.get(), 1);
    assert!(h.shell.access().allows(&Gate::from("results.view")));
    assert!(!h.shell.access().allows(&Gate::from("settings.credentials")));
    Ok(())
}

#[test]
fn issuer_revocation_preempts_a_full_command_queue_and_closes_protected_content(
) -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    let (mut controller, token) = issue(
        &h,
        "a",
        vec![
            SessionGrant::base("measurement.run"),
            SessionGrant::base("results.view"),
        ],
        None,
    );
    login(&mut h, &state, token);
    h.shell.launch(LaunchAction::open("report"));
    h.frame();
    assert!(h.texts().iter().any(|(text, _)| text == "PROTECTED REPORT"));
    let handle = h.shell.handle();
    assert!(handle
        .try_send(Command::Launch(LaunchAction::run("operate")))
        .is_ok());
    assert!(handle
        .try_send(Command::Launch(LaunchAction::run("operate")))
        .is_ok());
    controller.revoke();
    h.frame();
    assert_eq!(count.get(), 1, "revoked authority executed queued work");
    assert!(
        h.shell.access().session_token().is_none(),
        "revoked session token was retained"
    );
    assert!(
        !h.texts().iter().any(|(text, _)| text == "PROTECTED REPORT"),
        "revoked authority exposed protected content"
    );
    assert_eq!(state.borrow().ended.as_slice(), &[controller.id()]);
    h.frame();
    assert_eq!(count.get(), 1);
    Ok(())
}

#[test]
fn same_level_permission_publication_revalidates_open_screens_without_login() -> fairing::Result<()>
{
    let (mut h, state, _) = harness()?;
    let (mut controller, token) = issue(
        &h,
        "a",
        vec![
            SessionGrant::base("measurement.run"),
            SessionGrant::base("results.view"),
        ],
        None,
    );
    login(&mut h, &state, token);
    h.shell.launch(LaunchAction::open("report"));
    h.frame();
    controller
        .replace_grants(
            vec![SessionGrant::base("measurement.run")],
            None,
            h.shell.now(),
        )
        .unwrap();
    h.frame();
    assert!(h.shell.access().session_token().is_some());
    assert!(!h.texts().iter().any(|(text, _)| text == "PROTECTED REPORT"));
    assert!(h.shell.access().allows(&Gate::from("measurement.run")));
    Ok(())
}

#[test]
fn revocation_is_observed_by_direct_calls_and_logic_without_a_prompt_or_frame(
) -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    let (mut controller, token) = issue(&h, "a", vec![SessionGrant::base("measurement.run")], None);
    login(&mut h, &state, token);
    controller.revoke();
    h.shell.launch(LaunchAction::run("operate"));
    assert_eq!(count.get(), 1, "direct call bypassed revoked token");
    assert!(h.shell.access().session_token().is_none());
    let (new_controller, token) = issue(&h, "b", vec![SessionGrant::base("measurement.run")], None);
    let request = state.borrow().request.unwrap();
    state.borrow_mut().response = Some(request.respond(AuthOutcome::Session(token)));
    h.shell.logic(&h.ctx);
    assert!(h.shell.access().session_token().is_some());
    drop(new_controller);
    h.shell.logic(&h.ctx);
    assert!(h.shell.access().session_token().is_none());
    Ok(())
}

struct OnlyMeasurement;
impl AccessPolicy for OnlyMeasurement {
    fn allows(&self, _: &Subject, gate: &Gate) -> bool {
        gate.as_str() == "measurement.run"
    }
}
#[test]
fn custom_policy_cannot_add_unissued_rights_and_logout_drops_tokens_without_methods(
) -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    let (mut controller, token) = issue(
        &h,
        "a",
        vec![
            SessionGrant::base("measurement.run"),
            SessionGrant::base("results.view"),
        ],
        None,
    );
    login(&mut h, &state, token);
    h.shell.access_mut().set_policy(Box::new(OnlyMeasurement));
    h.frame();
    assert!(
        !h.shell.access().allows(&Gate::from("results.view")),
        "local policy failed to narrow token rights"
    );
    controller
        .replace_grants(Vec::new(), None, h.shell.now())
        .unwrap();
    h.frame();
    assert!(
        !h.shell.access().allows(&Gate::from("measurement.run")),
        "local policy added unissued rights"
    );
    state.borrow_mut().methods = false;
    h.shell.handle().logout();
    h.frame();
    assert!(h.shell.access().session_token().is_none());
    h.shell.launch(LaunchAction::run("operate"));
    assert_eq!(count.get(), 1);
    Ok(())
}

#[test]
fn stale_responses_cannot_replace_a_new_device_session_and_old_controllers_are_isolated(
) -> fairing::Result<()> {
    let (mut h, state, _) = harness()?;
    h.shell.launch(LaunchAction::run("operate"));
    let old_request = state.borrow().request.unwrap();
    let (mut old, old_token) = issue(&h, "old", vec![SessionGrant::base("measurement.run")], None);
    h.shell.back();
    h.shell.launch(LaunchAction::run("operate"));
    let current = state.borrow().request.unwrap();
    assert_ne!(old_request, current);
    state.borrow_mut().response = Some(old_request.respond(AuthOutcome::Session(old_token)));
    h.frame();
    assert!(
        h.shell.access().session_token().is_none(),
        "stale response installed authority"
    );
    let (_new, new_token) = issue(&h, "new", vec![SessionGrant::base("measurement.run")], None);
    let new_id = new_token.id();
    state.borrow_mut().response = Some(current.respond(AuthOutcome::Session(new_token)));
    h.frame();
    old.revoke();
    h.frame();
    assert_eq!(h.shell.access().session_token().unwrap().id(), new_id);
    assert_eq!(
        h.shell.access().session().subject.id.as_deref(),
        Some("new")
    );
    Ok(())
}

#[test]
fn routed_authentication_requires_a_current_challenge_and_never_accepts_unsolicited_tokens(
) -> fairing::Result<()> {
    let mut cfg = ShellConfig::default();
    cfg.access.session_tokens = true;
    cfg.access.mode = "routing".into();
    let mut h = Harness::new(cfg, fairing::Services::builder().build())?;
    let calls = Rc::new(Cell::new(0));
    let work = Rc::clone(&calls);
    h.shell
        .add(action("operate", move |_| work.set(work.get() + 1)).gate("measurement.run"));
    h.shell.launch(LaunchAction::run("operate"));
    let request = h
        .shell
        .poll_events()
        .into_iter()
        .find_map(|event| match event {
            fairing::ShellEvent::Access(fairing::access::AccessEvent::SessionRequested {
                request,
                ..
            }) => Some(request),
            _ => None,
        })
        .expect("routing did not issue a correlation challenge");
    let (_controller, token) = issue(
        &h,
        "routed",
        vec![SessionGrant::base("measurement.run")],
        None,
    );
    h.shell
        .accept_auth_response(request.respond(AuthOutcome::Session(token.clone())));
    assert_eq!(calls.get(), 1);
    assert!(h.shell.access().session_token().is_some());
    h.shell.launch(LaunchAction::Logout);
    h.shell
        .accept_auth_response(request.respond(AuthOutcome::Session(token)));
    assert!(
        h.shell.access().session_token().is_none(),
        "consumed challenge reinstalled a detached session"
    );
    Ok(())
}

#[test]
fn one_command_revoking_its_session_prevents_the_next_command_in_the_same_update(
) -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    let (controller, token) = issue(
        &h,
        "a",
        vec![
            SessionGrant::base("measurement.run"),
            SessionGrant::base("revoke"),
        ],
        None,
    );
    let controller = Rc::new(RefCell::new(controller));
    let captured = Rc::clone(&controller);
    h.shell
        .add(action("revoke", move |_| captured.borrow_mut().revoke()));
    login(&mut h, &state, token);
    let handle = h.shell.handle();
    assert!(handle
        .try_send(Command::Launch(LaunchAction::run("revoke")))
        .is_ok());
    assert!(handle
        .try_send(Command::Launch(LaunchAction::run("operate")))
        .is_ok());
    h.frame();
    assert_eq!(handle.queue_stats().outstanding, 0);
    assert_eq!(count.get(), 1, "a command following revocation executed");
    assert!(h.shell.access().session_token().is_none());
    Ok(())
}

#[test]
fn legacy_grants_and_off_mode_cannot_authorize_token_mode() -> fairing::Result<()> {
    let mut cfg = ShellConfig::default();
    cfg.access.session_tokens = true;
    cfg.access.mode = "off".into();
    assert!(fairing::access::Access::from_config(&cfg.access).is_err());
    let (mut h, state, count) = harness()?;
    h.shell.launch(LaunchAction::run("operate"));
    let request = state.borrow().request.unwrap();
    state.borrow_mut().response = Some(request.respond(AuthOutcome::Granted(subject("legacy"))));
    h.frame();
    assert!(h.shell.access().session_token().is_none());
    assert_eq!(count.get(), 0);
    Ok(())
}

#[test]
fn shell_expiry_and_temporary_permissions_use_the_current_update_clock() -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    let end = h.shell.now() + Duration::from_secs(1);
    let (_controller, token) = issue(
        &h,
        "a",
        vec![
            SessionGrant::base("measurement.run"),
            SessionGrant::temporary("results.view", end),
        ],
        Some(end + Duration::from_secs(1)),
    );
    login(&mut h, &state, token);
    h.shell.launch(LaunchAction::open("report"));
    h.frame();
    h.run_for(1.1);
    assert!(h.shell.access().session_token().is_some());
    assert!(
        !h.shell.access().allows(&Gate::from("results.view")),
        "expired temporary gate remained allowed"
    );
    assert!(!h.texts().iter().any(|(text, _)| text == "PROTECTED REPORT"));
    h.run_for(1.1);
    assert!(
        h.shell.access().session_token().is_none(),
        "expired full session was retained"
    );
    h.shell.launch(LaunchAction::run("operate"));
    assert_eq!(count.get(), 1);
    Ok(())
}

struct CountingPolicy(Rc<Cell<usize>>);
impl AccessPolicy for CountingPolicy {
    fn allows(&self, _: &Subject, _: &Gate) -> bool {
        self.0.set(self.0.get() + 1);
        true
    }
}
#[test]
fn absent_or_faulted_authority_never_dispatches_policy_callbacks() -> fairing::Result<()> {
    let (mut h, _, _) = harness()?;
    let calls = Rc::new(Cell::new(0));
    h.shell
        .access_mut()
        .set_policy(Box::new(CountingPolicy(Rc::clone(&calls))));
    assert!(!h.shell.access().allows(&Gate::from("measurement.run")));
    assert_eq!(calls.get(), 0, "absent token dispatched a policy callback");
    h.shell.quarantine();
    assert!(!h.shell.access().allows(&Gate::from("measurement.run")));
    assert_eq!(
        calls.get(),
        0,
        "fault quarantine dispatched a policy callback"
    );
    Ok(())
}

#[test]
fn subject_replacement_invalidates_old_requests_even_if_session_notification_panics(
) -> fairing::Result<()> {
    let (mut h, state, _) = harness()?;
    let (_controller, token) = issue(&h, "old", vec![SessionGrant::base("measurement.run")], None);
    login(&mut h, &state, token);
    h.shell.add(action("extra", |_| {}).gate("extra"));
    h.shell.launch(LaunchAction::run("extra"));
    let old = state.borrow().request.unwrap();
    let (_late, token) = issue(
        &h,
        "late",
        vec![SessionGrant::base("measurement.run")],
        None,
    );
    state.borrow_mut().panic_on_end = true;
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        h.shell.access_mut().set_subject(subject("replacement"))
    }));
    assert!(failed.is_err());
    state.borrow_mut().panic_on_end = false;
    h.shell
        .accept_auth_response(old.respond(AuthOutcome::Session(token)));
    assert!(
        h.shell.access().session_token().is_none(),
        "callback panic preserved an old authentication request"
    );
    Ok(())
}

struct RevokingPolicy(Rc<RefCell<SessionController>>);
impl AccessPolicy for RevokingPolicy {
    fn allows(&self, _: &Subject, _: &Gate) -> bool {
        self.0.borrow_mut().revoke();
        true
    }
}
#[test]
fn policy_callback_revocation_is_rechecked_before_a_protected_effect() -> fairing::Result<()> {
    let (mut h, state, count) = harness()?;
    let (controller, token) = issue(&h, "a", vec![SessionGrant::base("measurement.run")], None);
    login(&mut h, &state, token);
    h.shell
        .access_mut()
        .set_policy(Box::new(RevokingPolicy(Rc::new(RefCell::new(controller)))));
    h.shell.launch(LaunchAction::run("operate"));
    assert_eq!(
        count.get(),
        1,
        "policy callback revocation admitted a stale protected effect"
    );
    Ok(())
}
