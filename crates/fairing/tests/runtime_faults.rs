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
