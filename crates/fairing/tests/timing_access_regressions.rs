//! Cancellation and ordering at authentication/session boundaries.

use fairing::access::{AuthMethod, AuthOutcome, Authenticator, Credential, Gate, Subject};
use fairing::testing::{access_config, Harness};
use fairing::{action, LaunchAction, Level, Shell};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Instant;

struct DeferredAuth {
    ready: Rc<Cell<bool>>,
    calls: Rc<RefCell<Vec<String>>>,
    pending: bool,
}

impl Authenticator for DeferredAuth {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Pin {
            len: 1,
            max_len: 4,
            shuffle: false,
        }]
    }

    fn begin(&mut self, gate: &Gate, _: Instant) {
        self.calls
            .borrow_mut()
            .push(format!("begin {}", gate.as_str()));
        self.pending = false;
        self.ready.set(false);
    }

    fn submit(&mut self, _: Credential, _: Instant) -> AuthOutcome {
        self.pending = true;
        self.calls.borrow_mut().push("submit".into());
        AuthOutcome::Pending
    }

    fn poll(&mut self, _: Instant) -> Option<AuthOutcome> {
        if !self.pending || !self.ready.take() {
            return None;
        }
        self.pending = false;
        Some(AuthOutcome::Granted(Subject {
            id: Some("old-user".into()),
            level: Level(2),
            ..Subject::default()
        }))
    }

    fn cancel(&mut self) {
        self.calls.borrow_mut().push("cancel".into());
        self.pending = false;
        self.ready.set(false);
    }
}

struct Rig {
    h: Harness,
    ready: Rc<Cell<bool>>,
    calls: Rc<RefCell<Vec<String>>>,
    runs: Rc<Cell<u32>>,
}

fn rig() -> fairing::Result<Rig> {
    rig_with_timeout(0)
}

fn rig_with_timeout(seconds: u64) -> fairing::Result<Rig> {
    let mut config = access_config(&["viewer", "operator", "admin"], Some("top"));
    config.access.session_timeout_secs = seconds;
    config.motion.reduce = true;
    let ready = Rc::new(Cell::new(false));
    let calls = Rc::new(RefCell::new(Vec::new()));
    let runs = Rc::new(Cell::new(0));
    let auth = DeferredAuth {
        ready: Rc::clone(&ready),
        calls: Rc::clone(&calls),
        pending: false,
    };
    let count = Rc::clone(&runs);
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config).authenticator(auth).build(ctx)?;
        for id in ["first", "second"] {
            let count = Rc::clone(&count);
            shell.add(action(id, move |_| count.set(count.get() + 1)));
        }
        Ok(shell)
    })?;
    h.frames(3);
    Ok(Rig {
        h,
        ready,
        calls,
        runs,
    })
}

fn start_check(rig: &mut Rig) {
    rig.h.shell.launch(LaunchAction::run("first"));
    rig.h.frames(2);
    rig.h.key(egui::Key::Num1);
    assert_eq!(
        rig.calls.borrow().last().map(String::as_str),
        Some("submit")
    );
}

#[test]
fn a_new_gate_cancels_the_previous_gates_pending_answer() -> fairing::Result<()> {
    let mut rig = rig()?;
    start_check(&mut rig);
    rig.h.shell.handle().launch(LaunchAction::run("second"));
    rig.ready.set(true);
    rig.h.frame();
    assert_eq!(rig.runs.get(), 0);
    assert_eq!(rig.h.shell.access().session().subject.level, Level(0));
    assert_eq!(
        &*rig.calls.borrow(),
        &["begin first", "submit", "cancel", "begin second"]
    );
    rig.h.key(egui::Key::Num2);
    rig.ready.set(true);
    rig.h.frame();
    assert_eq!(rig.runs.get(), 1);
    Ok(())
}

#[test]
fn logout_cancels_pending_authentication_at_the_initial_subject_too() -> fairing::Result<()> {
    for lock_screen in [false, true] {
        let mut rig = rig()?;
        if lock_screen {
            rig.h.shell.launch(LaunchAction::Lock);
        }
        start_check(&mut rig);
        rig.h.shell.handle().logout();
        rig.ready.set(true);
        rig.h.frame();
        assert_eq!(rig.runs.get(), 0);
        assert_eq!(rig.h.shell.access().session().subject.level, Level(0));
        assert_eq!(rig.h.shell.lock_screen_visible(), lock_screen);
        if lock_screen {
            rig.h.key(egui::Key::Num2);
            rig.ready.set(true);
            rig.h.frame();
            assert!(!rig.h.shell.lock_screen_visible());
            assert_eq!(
                rig.runs.get(),
                0,
                "logout dropped the earlier deferred action"
            );
        }
    }
    Ok(())
}

#[test]
fn an_integrator_subject_replacement_cancels_the_previous_users_check() -> fairing::Result<()> {
    for direct in [false, true] {
        for level in [Level(0), Level(1)] {
            let mut rig = rig()?;
            start_check(&mut rig);
            let subject = Subject {
                id: Some("replacement".into()),
                level,
                ..Subject::default()
            };
            if direct {
                rig.h.shell.access_mut().set_subject(subject);
            } else {
                rig.h.shell.handle().set_subject(subject);
            }
            rig.ready.set(true);
            rig.h.frame();
            assert_eq!(
                rig.h.shell.access().session().subject.id.as_deref(),
                Some("replacement")
            );
            assert_eq!(rig.h.shell.access().session().subject.level, level);
            assert_eq!(rig.runs.get(), 0);
            assert!(!rig.h.shell.unlock_prompt_visible());
        }
    }
    Ok(())
}

#[test]
fn same_gate_requests_keep_their_pending_check() -> fairing::Result<()> {
    let mut rig = rig()?;
    start_check(&mut rig);
    rig.h.shell.handle().launch(LaunchAction::run("first"));
    rig.ready.set(true);
    rig.h.frame();
    assert_eq!(rig.runs.get(), 1);
    assert_eq!(&*rig.calls.borrow(), &["begin first", "submit"]);
    Ok(())
}

#[test]
fn a_same_level_session_timeout_cancels_the_expired_users_check() -> fairing::Result<()> {
    let mut rig = rig_with_timeout(1)?;
    rig.h.shell.handle().set_subject(Subject {
        id: Some("expired-user".into()),
        level: Level(0),
        ..Subject::default()
    });
    rig.h.frame();
    start_check(&mut rig);
    rig.h.sleep(2.0);
    rig.ready.set(true);
    rig.h.frame();
    assert_eq!(rig.runs.get(), 0);
    assert_eq!(rig.h.shell.access().session().subject.id, None);
    assert!(!rig.h.shell.unlock_prompt_visible());
    Ok(())
}

#[test]
fn relocking_cancels_pending_answers_and_drops_deferred_actions() -> fairing::Result<()> {
    let mut rig = rig()?;
    rig.h.shell.launch(LaunchAction::Lock);
    start_check(&mut rig);
    rig.h.shell.handle().launch(LaunchAction::Lock);
    rig.ready.set(true);
    rig.h.frame();
    assert_eq!(rig.h.shell.access().session().subject.level, Level(0));
    assert!(rig.h.shell.lock_screen_visible());
    assert_eq!(rig.runs.get(), 0);
    rig.h.key(egui::Key::Num2);
    rig.ready.set(true);
    rig.h.frame();
    assert!(!rig.h.shell.lock_screen_visible());
    assert_eq!(rig.runs.get(), 0, "the old deferred action was dropped");
    Ok(())
}

#[test]
fn direct_subject_changes_are_observed_before_starting_a_new_check() -> fairing::Result<()> {
    let mut rig = rig()?;
    rig.h.shell.access_mut().set_subject(Subject {
        id: Some("replacement".into()),
        level: Level(0),
        ..Subject::default()
    });
    start_check(&mut rig);
    rig.ready.set(true);
    rig.h.frame();
    assert_eq!(rig.runs.get(), 1);
    Ok(())
}

#[test]
fn a_command_deferred_by_the_frame_cap_rechecks_expired_access() -> fairing::Result<()> {
    let mut rig = rig_with_timeout(1)?;
    rig.h.shell.handle().set_subject(Subject {
        level: Level(2),
        ..Subject::default()
    });
    rig.h.frame();
    for _ in 0..256 {
        rig.h.shell.handle().set_badge("absent", None);
    }
    rig.h.shell.handle().launch(LaunchAction::run("first"));
    rig.h.frame();
    assert_eq!(rig.runs.get(), 0);
    rig.h.sleep(2.0);
    rig.h.frame();
    assert_eq!(rig.runs.get(), 0);
    assert_eq!(rig.h.shell.access().session().subject.level, Level(0));
    assert!(rig.h.shell.unlock_prompt_visible());
    Ok(())
}

#[test]
fn relock_and_logout_keep_the_authentication_lockout() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "admin"], Some("top"));
    config.motion.reduce = true;
    config
        .access
        .pin_table
        .pins
        .insert("admin".into(), "2".into());
    config.access.pin_table.attempt_limit = Some(1);
    config.access.pin_table.lock_secs = Some(60);
    let mut h = Harness::new(config, fairing::Services::null())?;
    h.frames(3);
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    h.key(egui::Key::Num1);
    h.shell.handle().launch(LaunchAction::Lock);
    h.shell.handle().logout();
    h.frames(2);
    h.key(egui::Key::Num2);
    assert!(h.shell.lock_screen_visible());
    assert_eq!(h.shell.access().session().subject.level, Level(0));
    h.sleep(61.0);
    h.frame();
    h.key(egui::Key::Num2);
    assert!(!h.shell.lock_screen_visible());
    assert_eq!(h.shell.access().session().subject.level, Level(1));
    Ok(())
}
