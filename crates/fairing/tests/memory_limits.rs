//! Bounded command admission, ownership and per-update ordering.
#![allow(
    clippy::unwrap_used,
    reason = "integration assertions preserve rejected command details"
)]
use fairing::{action, inbox::QueueErrorKind, shell::Command, LaunchAction, Shell};
use std::{cell::RefCell, rc::Rc};

#[test]
fn shell_budget_does_not_consume_a_hidden_extra_slot_or_reorder_commands() -> fairing::Result<()> {
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access())
        .command_queue_capacity(2)
        .max_commands_per_update(1)
        .build(&ctx)?;
    let runs = Rc::new(RefCell::new(Vec::new()));
    for id in ["a", "b", "c"] {
        let runs = Rc::clone(&runs);
        shell.add(action(id, move |_| runs.borrow_mut().push(id)));
    }
    let handle = shell.handle();
    handle
        .try_send(Command::Launch(LaunchAction::run("a")))
        .unwrap();
    handle
        .try_send(Command::Launch(LaunchAction::run("b")))
        .unwrap();
    let rejected = handle
        .try_send(Command::Launch(LaunchAction::run("c")))
        .unwrap_err();
    assert_eq!(rejected.kind(), QueueErrorKind::Full);
    shell.logic(&ctx);
    assert_eq!(&*runs.borrow(), &["a"]);
    assert_eq!(handle.queue_stats().outstanding, 1);
    handle.try_send(rejected.into_inner()).unwrap();
    shell.logic(&ctx);
    shell.logic(&ctx);
    assert_eq!(&*runs.borrow(), &["a", "b", "c"]);
    assert_eq!(handle.queue_stats().outstanding, 0);
    assert_eq!(handle.queue_stats().high_water, 2);
    drop(shell);
    assert!(!handle.send(Command::Home));
    assert_eq!(
        handle.try_send(Command::Home).unwrap_err().kind(),
        QueueErrorKind::Disconnected
    );
    Ok(())
}

#[test]
fn shell_rejects_invalid_resource_configuration() {
    let ctx = fairing::egui::Context::default();
    for capacity in [0, usize::MAX] {
        assert!(Shell::builder(fairing::testing::single_level_access())
            .command_queue_capacity(capacity)
            .build(&ctx)
            .is_err());
    }
    assert!(Shell::builder(fairing::testing::single_level_access())
        .max_commands_per_update(0)
        .build(&ctx)
        .is_err());
}

#[test]
fn producer_display_failures_are_coalesced_until_acknowledged() -> fairing::Result<()> {
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access())
        .command_queue_capacity(1)
        .build(&ctx)?;
    let handle = shell.handle();
    let oversized = "x".repeat(fairing::notify::MAX_TOAST_TEXT_BYTES + 1);
    assert!(handle.try_toast(&oversized).is_err());
    handle.clone().toast(&oversized);
    assert_eq!(shell.take_display_status().discarded, 1);
    assert!(!shell.take_display_status().is_reduced());
    handle.try_send(Command::Home).unwrap();
    handle.toast("queue is full");
    assert_eq!(shell.take_display_status().discarded, 1);
    assert_eq!(handle.queue_stats().outstanding, 1);
    Ok(())
}

#[test]
fn latest_display_retention_does_not_evict_an_accepted_device_command() -> fairing::Result<()> {
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access())
        .toast_queue_capacity(1)
        .command_queue_capacity(4)
        .build(&ctx)?;
    let ran = Rc::new(RefCell::new(0u32));
    let observed = Rc::clone(&ran);
    shell.add(action("device", move |_| *observed.borrow_mut() += 1));
    let handle = shell.handle();
    handle.toast("old");
    let latest = String::from("latest");
    handle.toast(&latest);
    assert_eq!(
        latest, "latest",
        "borrowed toast input retains caller ownership"
    );
    handle
        .try_send(Command::Launch(LaunchAction::run("device")))
        .unwrap();
    shell.logic(&ctx);
    assert_eq!(*ran.borrow(), 1);
    assert_eq!(handle.queue_stats().outstanding, 0);
    assert_eq!(shell.take_display_status().discarded, 1);
    assert_eq!(shell.toasts().pending(), 1);
    for frame in 0..3 {
        let output = ctx.run_ui(
            fairing::egui::RawInput {
                time: Some(f64::from(frame) / 60.0),
                ..Default::default()
            },
            |ui| shell.frame(ui),
        );
        output.drop_without_applying_deltas();
    }
    assert_eq!(*ran.borrow(), 1);
    assert!(shell
        .toasts()
        .visible()
        .iter()
        .any(|t| t.toast.text == "latest"));
    assert!(!shell
        .toasts()
        .visible()
        .iter()
        .any(|t| t.toast.text == "old"));
    Ok(())
}

#[test]
fn display_loss_uses_the_prepared_warning_until_acknowledged() -> fairing::Result<()> {
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access()).build(&ctx)?;
    shell.report_display_status(fairing::memory::DisplayStatus {
        discarded: 1,
        allocation_failed: false,
    });
    let warning_shapes = |output: &fairing::egui::FullOutput| {
        output.shapes.iter().filter(|shape| {
        matches!(&shape.shape, fairing::egui::Shape::Rect(rect) if rect.fill == fairing::egui::Color32::from_rgb(255,208,64)
            && (rect.rect.width() - 2.0).abs() < 0.0001 && (rect.rect.height() - 2.0).abs() < 0.0001)
    }).count()
    };
    let mut observed = false;
    for _ in 0..3 {
        let output = ctx.run_ui(fairing::egui::RawInput::default(), |ui| shell.frame(ui));
        observed |= warning_shapes(&output) > 0;
        output.drop_without_applying_deltas();
    }
    assert!(observed);
    assert_eq!(shell.take_display_status().discarded, 1);
    let output = ctx.run_ui(fairing::egui::RawInput::default(), |ui| shell.frame(ui));
    assert_eq!(warning_shapes(&output), 0);
    output.drop_without_applying_deltas();
    Ok(())
}

#[test]
fn fallible_shell_factory_releases_reservation_and_skips_closed_or_full_work() -> fairing::Result<()>
{
    use fairing::inbox::TryBuildSendError;
    let ctx = fairing::egui::Context::default();
    let shell = Shell::builder(fairing::testing::single_level_access())
        .command_queue_capacity(1)
        .build(&ctx)?;
    let handle = shell.handle();
    assert!(matches!(
        handle.try_send_fallible_with(|| Err("cannot allocate payload")),
        Err(TryBuildSendError::Build("cannot allocate payload"))
    ));
    assert_eq!(handle.queue_stats().outstanding, 0);
    handle
        .try_send_fallible_with(|| Ok::<_, ()>(Command::Home))
        .unwrap();
    let calls = std::cell::Cell::new(0);
    assert!(matches!(
        handle.try_send_fallible_with(|| {
            calls.set(calls.get() + 1);
            Ok::<_, ()>(Command::Back)
        }),
        Err(TryBuildSendError::Queue(QueueErrorKind::Full))
    ));
    drop(shell);
    assert!(matches!(
        handle.try_send_fallible_with(|| {
            calls.set(calls.get() + 1);
            Ok::<_, ()>(Command::Back)
        }),
        Err(TryBuildSendError::Queue(QueueErrorKind::Disconnected))
    ));
    assert_eq!(calls.get(), 0);
    Ok(())
}
