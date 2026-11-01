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
