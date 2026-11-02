//! Runtime fault injection: fail closed, discard partial output and never replay work.
#![allow(
    clippy::panic,
    clippy::unwrap_used,
    reason = "synthetic faults exercise the runtime boundary"
)]
use fairing::{action, screen, LaunchAction, Shell};
use std::{cell::Cell, rc::Rc};

#[test]
fn failed_ui_erases_output_and_cannot_execute_queued_payment_or_clipboard() -> fairing::Result<()> {
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    let paid = Rc::new(Cell::new(0));
    let seen = Rc::clone(&paid);
    shell.add(action("pay", move |_| seen.set(seen.get() + 1)));
    let handle = shell.handle();
    let producer = handle.clone();
    let resumed = Rc::new(Cell::new(false));
    let continued = Rc::clone(&resumed);
    shell.add(screen(
        "fault",
        move |ui: &mut fairing::egui::Ui, cx: &mut fairing::Cx<'_>| {
            ui.label("sensitive test output");
            ui.ctx().copy_text("synthetic secret".into());
            producer.launch(LaunchAction::run("pay"));
            cx.with_app::<u32, ()>(|state, cx| {
                *state += 1;
                cx.launch(LaunchAction::run("pay"));
                panic!("synthetic borrowed-state failure");
            });
            continued.set(true);
        },
    ));
    shell.launch(LaunchAction::open("fault"));
    let mut state = 0u32;
    for _ in 0..6 {
        let mut output = ctx.run_ui(fairing::egui::RawInput::default(), |ui| {
            shell.frame_with(ui, &mut state);
        });
        output.textures_delta.clear();
        if shell.is_faulted() {
            assert!(output.shapes.is_empty(), "partial private drawing escaped");
            assert!(
                output.platform_output.commands.is_empty(),
                "clipboard command escaped"
            );
        }
    }
    assert!(shell.is_faulted());
    assert!(!resumed.get(), "loan failure resumed the caller body");
    assert_eq!(
        state, 1,
        "callback was replayed or business state was falsely rolled back"
    );
    assert_eq!(paid.get(), 0);
    assert!(
        !shell.access().allows(&fairing::access::Gate::from("pay")),
        "single-level mode bypassed fault denial"
    );
    assert_eq!(
        handle
            .try_send(fairing::shell::Command::Home)
            .unwrap_err()
            .kind(),
        fairing::inbox::QueueErrorKind::Disconnected
    );
    shell.logic_with(&ctx, &mut state);
    shell.launch(LaunchAction::run("pay"));
    assert_eq!(paid.get(), 0);
    assert_eq!(shell.poll_events().len(), 0);
    // An independent business queue remains usable after the shell fault.
    let (work, receive) = fairing::inbox::Inbox::bounded(1).unwrap();
    for i in 0..1000 {
        work.try_send(i).unwrap();
        assert_eq!(receive.try_recv(), Ok(i));
    }
    Ok(())
}

#[test]
fn logic_and_direct_action_panics_latch_without_replaying_or_publishing_events(
) -> fairing::Result<()> {
    for queued in [false, true] {
        let ctx = fairing::egui::Context::default();
        let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
        let calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&calls);
        shell.add(action("fail", move |cx| {
            seen.set(seen.get() + 1);
            cx.set_setting("app.uncommitted", fairing::SettingValue::Bool(true));
            panic!("synthetic action failure");
        }));
        if queued {
            shell.handle().launch(LaunchAction::run("fail"));
            shell.logic(&ctx);
        } else {
            shell.launch(LaunchAction::run("fail"));
        }
        assert!(shell.is_faulted());
        assert_eq!(shell.poll_events().len(), 0);
        shell.logic(&ctx);
        shell.launch(LaunchAction::run("fail"));
        assert_eq!(calls.get(), 1);
    }
    Ok(())
}

#[test]
fn a_nested_backend_failure_stops_the_outer_command_batch() -> fairing::Result<()> {
    struct Fails(fairing::egui::Context);
    impl fairing::services::Backend for Fails {}
    impl fairing::services::DisplayBackend for Fails {
        fn brightness(&self) -> Option<u8> {
            Some(50)
        }
        fn set_brightness(&mut self, _: u8) -> fairing::services::ServiceResult {
            self.0.copy_text("private backend output".into());
            panic!("synthetic backend failure");
        }
        fn set_idle_inhibit(&mut self, _: bool) {}
        fn set_power(&mut self, _: bool) -> fairing::services::ServiceResult {
            Ok(())
        }
    }
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    shell.services_mut().display = Box::new(Fails(ctx.clone()));
    let calls = Rc::new(Cell::new(0));
    let seen = Rc::clone(&calls);
    shell.add(action("pay", move |_| seen.set(seen.get() + 1)));
    let handle = shell.handle();
    handle
        .try_send(fairing::shell::Command::SetSetting {
            key: "display.brightness".into(),
            value: fairing::SettingValue::Int(20),
        })
        .unwrap();
    handle.launch(LaunchAction::run("pay"));
    shell.logic(&ctx);
    assert!(shell.is_faulted());
    assert_eq!(calls.get(), 0);
    assert_eq!(ctx.output(|output| output.commands.len()), 0);
    assert_eq!(
        handle.queue_stats().outstanding,
        1,
        "outer batch continued after nested failure"
    );
    shell.logic(&ctx);
    assert_eq!(handle.queue_stats().outstanding, 1);
    assert_eq!(shell.poll_events().len(), 0);
    Ok(())
}

#[test]
fn restart_replaces_the_shell_without_replaying_old_work() -> Result<(), Box<dyn std::error::Error>>
{
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    let old = shell.handle();
    let payments = Rc::new(Cell::new(0));
    let seen = payments.clone();
    shell.add(action("pay", move |_| seen.set(seen.get() + 1)));
    old.launch(LaunchAction::run("pay"));
    shell.restart_with(|| {
        assert_eq!(
            old.try_send(fairing::shell::Command::Home)
                .unwrap_err()
                .kind(),
            fairing::inbox::QueueErrorKind::Disconnected,
            "admission must close before the factory runs"
        );
        let mut fresh = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
        let seen = payments.clone();
        fresh.add(action("pay", move |_| seen.set(seen.get() + 1)));
        Ok(fresh)
    })?;
    assert!(!shell.is_faulted());
    shell.logic(&ctx);
    assert_eq!(payments.get(), 0, "old queued work was replayed");
    assert_eq!(
        old.try_send(fairing::shell::Command::Home)
            .unwrap_err()
            .kind(),
        fairing::inbox::QueueErrorKind::Disconnected
    );
    shell.handle().launch(LaunchAction::run("pay"));
    shell.logic(&ctx);
    assert_eq!(
        payments.get(),
        1,
        "new handle did not reach the replacement"
    );
    assert!(!shell.take_restart_request());
    Ok(())
}

#[test]
fn failed_restart_stays_closed_and_allows_a_later_developer_attempt() -> fairing::Result<()> {
    use fairing::recovery::RestartError;
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    assert!(matches!(
        shell.restart_with(|| Err(fairing::Error::Config("test".into()))),
        Err(RestartError::Build(_))
    ));
    assert!(shell.is_faulted());
    assert!(matches!(
        shell.restart_with(|| panic!("synthetic factory failure")),
        Err(RestartError::Panicked)
    ));
    assert!(shell.is_faulted());
    assert!(matches!(
        shell.restart_with(|| {
            let mut replacement =
                Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
            replacement.quarantine();
            Ok(replacement)
        }),
        Err(RestartError::FaultedReplacement)
    ));
    assert!(shell.is_faulted());
    assert!(shell
        .restart_with(|| Shell::builder(fairing::testing::single_level_access()).build(&ctx))
        .is_ok());
    assert!(!shell.is_faulted());
    Ok(())
}

#[test]
fn disposal_unwind_quarantines_the_replacement() -> fairing::Result<()> {
    struct BadDrop;
    impl Drop for BadDrop {
        fn drop(&mut self) {
            panic!("synthetic old callback destructor failure");
        }
    }
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    let guard = BadDrop;
    shell.add(action("old", move |_| {
        let _ = &guard;
    }));
    let old = shell.handle();
    let mut fresh_handle = None;
    let result = shell.restart_with(|| {
        let fresh = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
        fresh_handle = Some(fresh.handle());
        Ok(fresh)
    });
    assert!(matches!(
        result,
        Err(fairing::recovery::RestartError::Panicked)
    ));
    assert!(shell.is_faulted());
    for handle in [old, fresh_handle.unwrap()] {
        assert_eq!(
            handle
                .try_send(fairing::shell::Command::Home)
                .unwrap_err()
                .kind(),
            fairing::inbox::QueueErrorKind::Disconnected
        );
    }
    Ok(())
}

#[test]
fn fault_guidance_is_opt_in_and_the_button_only_records_a_request() -> fairing::Result<()> {
    use fairing::egui::{self, Event, PointerButton, RawInput};
    let ctx = egui::Context::default();
    let view = fairing::recovery::RecoveryUi {
        heading: "Application recovery".into(),
        message: "Contact the operator at station 12".into(),
        restart_label: Some("Request recovery".into()),
    };
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    let callbacks = Rc::new(Cell::new(0));
    let seen = callbacks.clone();
    shell.add(action("blocked", move |_| seen.set(seen.get() + 1)));
    shell.quarantine();
    let mut output = ctx.run_ui(RawInput::default(), |ui| shell.frame(ui));
    output.textures_delta.clear();
    assert!(
        output.shapes.is_empty(),
        "default shell showed an unsolicited menu"
    );
    // Measure the public view itself; drive actual pointer events at its button below.
    let mut rect = egui::Rect::NOTHING;
    ctx.run_ui(RawInput::default(), |ui| {
        rect = view.show(ui).unwrap().rect;
    })
    .drop_without_applying_deltas();
    shell.set_recovery_ui(Some(view));
    let mut output = ctx.run_ui(RawInput::default(), |ui| shell.frame(ui));
    output.textures_delta.clear();
    assert!(!output.shapes.is_empty(), "opt-in guidance did not render");
    assert!(!shell.take_restart_request());
    for pressed in [true, false] {
        let input = RawInput {
            events: vec![
                Event::PointerMoved(rect.center()),
                Event::PointerButton {
                    pos: rect.center(),
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| shell.frame(ui))
            .drop_without_applying_deltas();
    }
    assert!(
        shell.take_restart_request(),
        "button click was not exposed to the host"
    );
    assert!(!shell.take_restart_request(), "request was not consumed");
    assert!(shell.is_faulted(), "view performed an unsolicited restart");
    shell.launch(LaunchAction::run("blocked"));
    shell.logic(&ctx);
    assert_eq!(callbacks.get(), 0);
    shell.set_recovery_ui(None);
    let mut output = ctx.run_ui(RawInput::default(), |ui| shell.frame(ui));
    output.textures_delta.clear();
    assert_eq!(output.shapes.len(), 0);
    Ok(())
}
