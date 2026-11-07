use super::Shell;
use crate::access::{AuthMethod, AuthOutcome, Authenticator, Credential, Gate, Subject};
use crate::services::{Backend, Services};
use crate::testing::{access_config, single_level_access};
use crate::{action, LaunchAction, Level};
use egui::{Context, RawInput};
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Counter {
    polls: Rc<Cell<usize>>,
    wake: Option<Instant>,
}
impl Backend for Counter {
    fn poll(&mut self) {
        self.polls.set(self.polls.get() + 1);
    }
    fn next_wake(&self) -> Option<Instant> {
        self.wake
    }
}

fn counted() -> crate::Result<(Context, Shell, Rc<Cell<usize>>)> {
    let ctx = Context::default();
    let polls = Rc::new(Cell::new(0));
    let services = Services::builder()
        .custom(Counter {
            polls: Rc::clone(&polls),
            wake: None,
        })
        .build();
    let shell = Shell::builder(single_level_access())
        .services(services)
        .build(&ctx)?;
    Ok((ctx, shell, polls))
}

#[test]
fn logic_processes_commands_and_app_state_without_a_ui_pass() -> crate::Result<()> {
    let (ctx, mut shell, polls) = counted()?;
    shell.add(action("append", |cx| {
        if let Some(app) = cx.app_mut::<Vec<u8>>() {
            app.push(7);
        }
    }));
    shell.handle().launch(LaunchAction::run("append"));
    let mut app = Vec::<u8>::new();
    let wall = shell.clock_wall + Duration::from_secs(2);
    let pass = ctx.cumulative_pass_nr();
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_at(ctx, &mut Some(&mut app), wall);
    });
    assert_eq!(app, [7]);
    assert_eq!(polls.get(), 1);
    assert_eq!(shell.frame_no(), 0);
    assert_eq!(ctx.cumulative_pass_nr(), pass);
    Ok(())
}

#[test]
fn a_visible_pass_reuses_logic_without_polling_twice_or_repeating_actions() -> crate::Result<()> {
    let (ctx, mut shell, polls) = counted()?;
    let runs = Rc::new(Cell::new(0));
    let count = Rc::clone(&runs);
    shell.add(action("once", move |_| count.set(count.get() + 1)));
    shell.handle().launch(LaunchAction::run("once"));
    let wall = shell.clock_wall + Duration::from_secs(1);
    let out = ctx.run_ui(
        RawInput {
            time: Some(1.0),
            ..RawInput::default()
        },
        |ui| {
            shell.logic_at(ui.ctx(), &mut None, wall);
            shell.frame(ui);
        },
    );
    out.drop_without_applying_deltas();
    assert_eq!(polls.get(), 1);
    assert_eq!(runs.get(), 1);
    assert_eq!(shell.frame_no(), 1);
    Ok(())
}

#[test]
fn ui_time_does_not_double_count_a_gap_already_spent_in_logic() -> crate::Result<()> {
    let (ctx, mut shell, _) = counted()?;
    let start = shell.now();
    let wall = shell.clock_wall;
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_at(ctx, &mut None, wall + Duration::from_secs(5));
    });
    let out = ctx.run_ui(
        RawInput {
            time: Some(1000.0),
            ..RawInput::default()
        },
        |ui| {
            shell.logic_at(ui.ctx(), &mut None, wall + Duration::from_secs(6));
            shell.frame(ui);
        },
    );
    out.drop_without_applying_deltas();
    assert_eq!(shell.now().duration_since(start), Duration::from_secs(6));
    Ok(())
}

#[test]
fn hidden_ticks_expire_sessions_before_queued_privileged_actions() -> crate::Result<()> {
    let ctx = Context::default();
    let mut cfg = access_config(&["viewer", "operator"], Some("top"));
    cfg.access.session_timeout_secs = 1;
    let mut shell = Shell::builder(cfg).build(&ctx)?;
    let runs = Rc::new(Cell::new(0));
    let count = Rc::clone(&runs);
    shell.add(action("privileged", move |_| count.set(count.get() + 1)));
    shell.access.set_subject_at(
        Subject {
            level: Level(1),
            ..Subject::default()
        },
        shell.mono,
    );
    let wall = shell.clock_wall;
    // Leave user activity in egui's last UI input. run_logic deliberately retains it.
    let out = ctx.run_ui(
        RawInput {
            time: Some(0.0),
            events: vec![egui::Event::Text("previous activity".into())],
            ..RawInput::default()
        },
        |ui| shell.frame(ui),
    );
    out.drop_without_applying_deltas();
    let activity = shell.access.session().last_activity;
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_at(ctx, &mut None, wall + Duration::from_millis(500));
    });
    assert_eq!(
        shell.access.session().last_activity,
        activity,
        "old input cannot renew the session"
    );
    shell.handle().launch(LaunchAction::run("privileged"));
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_at(ctx, &mut None, wall + Duration::from_secs(2));
    });
    assert_eq!(shell.access.session().subject.level, Level(0));
    assert_eq!(runs.get(), 0);
    Ok(())
}

#[test]
fn hidden_logic_arms_backend_deadlines_without_running_a_ui_clock() -> crate::Result<()> {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    let ctx = Context::default();
    let start = Instant::now();
    let services = Services::builder()
        .custom(Counter {
            polls: Rc::new(Cell::new(0)),
            wake: Some(start + Duration::from_secs(10)),
        })
        .build();
    let mut shell = Shell::builder(single_level_access())
        .services(services)
        .build(&ctx)?;
    shell.clock_wall = start;
    shell.mono = start;
    for _ in 0..3 {
        let _ = ctx.run_logic(&RawInput::default(), |_| {});
    }
    let delay = Arc::new(AtomicU64::new(u64::MAX));
    let seen = Arc::clone(&delay);
    ctx.set_request_repaint_callback(move |info| {
        seen.fetch_min(info.delay.as_secs(), Ordering::Relaxed);
    });
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_at(ctx, &mut None, start + Duration::from_secs(3));
    });
    assert_eq!(delay.load(Ordering::Relaxed), 7);
    Ok(())
}

#[test]
fn continuous_repaint_policy_does_not_spin_hidden_logic() -> crate::Result<()> {
    let (ctx, mut shell, _) = counted()?;
    shell.config.shell.repaint = crate::config::RepaintMode::Continuous;
    for _ in 0..4 {
        let _ = ctx.run_logic(&RawInput::default(), |ctx| shell.logic(ctx));
    }
    assert!(!ctx.has_requested_repaint());
    Ok(())
}

struct PendingAuth {
    request: Option<crate::access::AuthRequestId>,
    ready: Rc<Cell<bool>>,
    polls: Rc<Cell<usize>>,
}
impl Authenticator for PendingAuth {
    fn begin(
        &mut self,
        request: crate::access::AuthRequestId,
        _: &crate::access::Gate,
        _: Instant,
    ) {
        self.request = Some(request);
    }
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Pin {
            len: 1,
            max_len: 1,
            shuffle: false,
        }]
    }
    fn submit(
        &mut self,
        request: crate::access::AuthRequestId,
        _: Credential,
        _: Instant,
    ) -> AuthOutcome {
        self.request = Some(request);
        AuthOutcome::Pending
    }
    fn poll(&mut self, _: Instant) -> Option<crate::access::AuthResponse> {
        let request = self.request?;
        let outcome = {
            self.polls.set(self.polls.get() + 1);
            self.ready.take().then_some(AuthOutcome::Granted(Subject {
                level: Level(1),
                ..Subject::default()
            }))
        };
        outcome.map(|outcome| request.respond(outcome))
    }
    fn cancel(&mut self) {
        self.ready.set(false);
    }
}

#[test]
fn queued_logout_cancels_a_ready_hidden_authentication_reply() -> crate::Result<()> {
    let ctx = Context::default();
    let ready = Rc::new(Cell::new(false));
    let polls = Rc::new(Cell::new(0));
    let auth = PendingAuth {
        request: None,
        ready: Rc::clone(&ready),
        polls: Rc::clone(&polls),
    };
    let mut shell = Shell::builder(access_config(&["viewer", "operator"], Some("top")))
        .authenticator(auth)
        .build(&ctx)?;
    shell.request_unlock(Gate::from("privileged"), None);
    shell.submit_credential(Credential::Pin("1".into()), &mut None);
    ready.set(true);
    shell.handle().logout();
    let _ = ctx.run_logic(&RawInput::default(), |ctx| shell.logic(ctx));
    assert_eq!(shell.access.session().subject.level, Level(0));
    assert_eq!(polls.get(), 0);
    assert!(!ready.get());
    Ok(())
}

#[test]
fn a_ready_authentication_reply_runs_its_action_without_a_ui_pass() -> crate::Result<()> {
    let ctx = Context::default();
    let ready = Rc::new(Cell::new(false));
    let polls = Rc::new(Cell::new(0));
    let auth = PendingAuth {
        request: None,
        ready: Rc::clone(&ready),
        polls,
    };
    let mut shell = Shell::builder(access_config(&["viewer", "operator"], Some("top")))
        .authenticator(auth)
        .build(&ctx)?;
    shell.add(action("privileged", |cx| {
        if let Some(count) = cx.app_mut::<usize>() {
            *count += 1;
        }
    }));
    shell.request_unlock(
        Gate::from("privileged"),
        Some(LaunchAction::run("privileged")),
    );
    shell.submit_credential(Credential::Pin("1".into()), &mut None);
    ready.set(true);
    let mut count = 0usize;
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_with(ctx, &mut count);
    });
    assert_eq!(count, 1);
    assert_eq!(shell.access.session().subject.level, Level(1));
    assert_eq!(shell.frame_no(), 0);
    Ok(())
}

#[test]
fn revocation_between_logic_and_ui_is_enforced_before_screen_drawing() -> crate::Result<()> {
    let ctx = Context::default();
    let mut shell =
        Shell::builder(access_config(&["viewer", "operator"], Some("top"))).build(&ctx)?;
    let drawn = Rc::new(Cell::new(0));
    let count = Rc::clone(&drawn);
    shell.add(crate::screen("protected", move |_, _| {
        count.set(count.get() + 1);
    }));
    shell.handle().set_subject(Subject {
        level: Level(1),
        ..Subject::default()
    });
    shell.handle().launch(LaunchAction::open("protected"));
    let out = ctx.run_ui(RawInput::default(), |ui| {
        shell.logic(ui.ctx());
        shell.access.set_subject_at(Subject::default(), shell.now());
        shell.frame(ui);
    });
    out.drop_without_applying_deltas();
    assert_eq!(drawn.get(), 0);
    assert!(shell.workspace.find("protected").is_none());
    Ok(())
}

#[test]
fn hidden_eviction_flushes_destroyed_requests_with_the_app_state() -> crate::Result<()> {
    struct Saver;
    impl crate::Screen for Saver {
        fn ui(&mut self, _: &mut egui::Ui, _: &mut crate::Cx<'_>) {}
        fn on_lifecycle(&mut self, event: crate::Lifecycle, cx: &mut crate::Cx<'_>) {
            if event == crate::Lifecycle::Destroyed {
                if let Some(saved) = cx.app_mut::<bool>() {
                    *saved = true;
                }
                cx.set_setting("app.saved", crate::SettingValue::Bool(true));
            }
        }
    }
    let ctx = Context::default();
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    let mut shell = Shell::builder(cfg).build(&ctx)?;
    shell.add(crate::screen_with("saver", || Saver).evict_after(Duration::from_secs(1)));
    let wall = shell.clock_wall;
    let mut saved = false;
    shell.handle().launch(LaunchAction::open("saver"));
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_with(ctx, &mut saved);
    });
    shell.handle().home();
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_with(ctx, &mut saved);
    });
    let _ = ctx.run_logic(&RawInput::default(), |ctx| {
        shell.logic_at(ctx, &mut Some(&mut saved), wall + Duration::from_secs(3));
    });
    assert!(saved);
    assert!(shell.workspace.find("saver").is_none());
    assert!(shell.poll_events().iter().any(|event| matches!(event,
        crate::ShellEvent::SettingChanged { key, value: crate::SettingValue::Bool(true) }
        if key.0.as_ref() == "app.saved"
    )));
    assert_eq!(shell.frame_no(), 0);
    Ok(())
}

#[test]
fn background_created_has_a_viewport_and_later_resize_is_delivered() -> crate::Result<()> {
    struct SizedScreen {
        events: Rc<std::cell::RefCell<Vec<egui::Vec2>>>,
        drawn: Rc<Cell<egui::Vec2>>,
    }
    impl crate::Screen for SizedScreen {
        fn ui(&mut self, _: &mut egui::Ui, cx: &mut crate::Cx<'_>) {
            self.drawn.set(cx.pane.rect.size());
        }
        fn on_lifecycle(&mut self, event: crate::Lifecycle, cx: &mut crate::Cx<'_>) {
            if matches!(
                event,
                crate::Lifecycle::Created | crate::Lifecycle::Resized(_)
            ) {
                self.events.borrow_mut().push(cx.pane.rect.size());
            }
        }
    }
    let ctx = Context::default();
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    cfg.status_bar.enabled = false;
    cfg.nav_bar.enabled = false;
    let mut shell = Shell::builder(cfg).build(&ctx)?;
    let sizes = Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = Rc::clone(&sizes);
    let drawn = Rc::new(Cell::new(egui::Vec2::ZERO));
    let body_size = Rc::clone(&drawn);
    shell.add(crate::screen_with("sized", move || SizedScreen {
        events: Rc::clone(&seen),
        drawn: Rc::clone(&body_size),
    }));
    shell.handle().launch(LaunchAction::open("sized"));
    let mut input = RawInput::default();
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .inner_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(640.0, 480.0),
    ));
    let _ = ctx.run_logic(&input, |ctx| shell.logic(ctx));
    let initial = sizes
        .borrow()
        .first()
        .copied()
        .ok_or_else(|| crate::Error::Config("Created was not delivered".into()))?;
    assert!(initial.x > 0.0 && initial.y > 0.0 && initial.is_finite());
    input.screen_rect = Some(egui::Rect::from_min_size(
        egui::Pos2::ZERO,
        egui::vec2(800.0, 600.0),
    ));
    let out = ctx.run_ui(input, |ui| {
        shell.logic(ui.ctx());
        shell.frame(ui);
    });
    out.drop_without_applying_deltas();
    let resized = sizes
        .borrow()
        .last()
        .copied()
        .ok_or_else(|| crate::Error::Config("Resized was not delivered".into()))?;
    assert!(
        sizes.borrow().len() >= 2,
        "Created and Resized must both arrive"
    );
    assert!(resized.x > initial.x && resized.y > initial.y);
    assert_eq!(
        resized,
        drawn.get(),
        "Resized describes the pane actually drawn"
    );
    Ok(())
}

#[test]
fn a_backend_deadline_armed_by_a_lifecycle_callback_is_not_stranded() -> crate::Result<()> {
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };
    struct Deadline(Rc<Cell<Option<Instant>>>);
    impl Backend for Deadline {
        fn next_wake(&self) -> Option<Instant> {
            self.0.get()
        }
    }
    struct Armer(Rc<Cell<Option<Instant>>>);
    impl crate::Screen for Armer {
        fn ui(&mut self, _: &mut egui::Ui, _: &mut crate::Cx<'_>) {}
        fn on_lifecycle(&mut self, event: crate::Lifecycle, cx: &mut crate::Cx<'_>) {
            if event == crate::Lifecycle::Created {
                self.0.set(Some(cx.now + Duration::from_secs(5)));
            }
        }
    }
    let ctx = Context::default();
    let armed = Rc::new(Cell::new(None));
    let services = Services::builder()
        .custom(Deadline(Rc::clone(&armed)))
        .build();
    let mut shell = Shell::builder(single_level_access())
        .services(services)
        .build(&ctx)?;
    let factory_deadline = Rc::clone(&armed);
    shell.add(crate::screen_with("armer", move || {
        Armer(Rc::clone(&factory_deadline))
    }));
    shell.handle().launch(LaunchAction::open("armer"));
    for _ in 0..4 {
        let _ = ctx.run_logic(&RawInput::default(), |_| {});
    }
    let delay = Arc::new(AtomicU64::new(u64::MAX));
    let seen = Arc::clone(&delay);
    ctx.set_request_repaint_callback(move |info| {
        seen.fetch_min(info.delay.as_secs(), Ordering::Relaxed);
    });
    let _ = ctx.run_logic(&RawInput::default(), |ctx| shell.logic(ctx));
    assert_eq!(delay.load(Ordering::Relaxed), 5);
    Ok(())
}
