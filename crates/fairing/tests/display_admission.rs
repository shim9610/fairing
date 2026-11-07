//! Display floods must refuse new display work while preserving retained state and commands.
#![allow(clippy::unwrap_used, reason = "regression assertions")]
use fairing::{
    notify::{
        AdmissionError, CenterChange, Notification, NotificationCenter, NotificationId, Toast,
        ToastQueue,
    },
    LaunchAction, Shell,
};
use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

#[test]
fn persistent_flood_is_refused_and_updates_still_work() {
    let mut center = NotificationCenter::new(2);
    for id in [1, 2] {
        center
            .try_push(Notification::new(NotificationId(id), "protected").persistent())
            .unwrap();
    }
    for id in 3..4096 {
        let error = center
            .try_push(Notification::new(NotificationId(id), "private patient text").persistent())
            .unwrap_err();
        assert_eq!(error.kind(), AdmissionError::Full);
        assert!(!format!("{error:?}").contains("patient"));
        assert_eq!(error.into_inner().id, NotificationId(id));
        assert_eq!(center.len(), 2);
        assert_eq!(center.unread(), 2);
    }
    assert_eq!(
        center
            .try_push(Notification::new(NotificationId(1), "resolved"))
            .unwrap(),
        CenterChange::Updated
    );
    center
        .try_push(Notification::new(NotificationId(3), "new fault").persistent())
        .unwrap();
    assert!(center.get(NotificationId(2)).is_some());
    assert!(center.get(NotificationId(3)).is_some());
    assert_eq!(center.len(), 2);
}

#[test]
fn toast_queue_refuses_new_items_and_reuses_freed_slots() {
    let mut queue = ToastQueue::new(1, Duration::from_secs(1)).with_pending_capacity(2);
    queue.try_push_text("a").unwrap();
    queue.try_push_text("b").unwrap();
    queue.try_push_text("a").unwrap();
    assert_eq!(queue.pending(), 2);
    assert_eq!(queue.try_push_text("c"), Err(AdmissionError::Full));
    let error = queue.try_push(Toast::new("owned rejection")).unwrap_err();
    assert_eq!(error.kind(), AdmissionError::Full);
    assert_eq!(error.into_inner().text, "owned rejection");
    let theme = fairing::Theme::default();
    let _ = queue.tick(0.0, Instant::now(), &theme.motion, theme.components.toast);
    assert_eq!(queue.visible().len(), 1);
    assert_eq!(queue.pending(), 1);
    queue.try_push_text("c").unwrap();
    assert_eq!(queue.pending(), 2);
}

#[test]
fn spare_payload_capacity_and_disabled_display_cannot_bypass_limits() {
    let mut center = NotificationCenter::new(1);
    let title = String::with_capacity(fairing::notify::MAX_NOTIFICATION_TEXT_BYTES + 1);
    assert_eq!(
        center
            .try_push(Notification::new(NotificationId(1), title))
            .unwrap_err()
            .kind(),
        AdmissionError::TooLarge
    );
    let mut queue = ToastQueue::new(1, Duration::ZERO).with_pending_capacity(0);
    assert_eq!(queue.try_push_text("hidden"), Err(AdmissionError::Full));
    let mut glyph = String::with_capacity(fairing::notify::MAX_TOAST_TEXT_BYTES + 1);
    glyph.push('x');
    assert_eq!(
        queue
            .try_push(Toast::new("small").icon(fairing::icons::IconRef::Glyph(glyph)))
            .unwrap_err()
            .kind(),
        AdmissionError::TooLarge
    );
    assert_eq!(queue.pending(), 0);
}

#[test]
fn refusing_display_does_not_refuse_unrelated_authorized_work() -> fairing::Result<()> {
    let ctx = fairing::egui::Context::default();
    let mut shell = Shell::builder(fairing::testing::single_level_access())
        .toast_queue_capacity(0)
        .build(&ctx)?;
    assert_eq!(shell.try_toast("omit me"), Err(AdmissionError::Full));
    let runs = Rc::new(Cell::new(0));
    let count = Rc::clone(&runs);
    shell.add(fairing::action("work", move |_| count.set(count.get() + 1)));
    shell.handle().try_toast("also omitted").unwrap();
    shell.handle().launch(LaunchAction::run("work"));
    shell.logic(&ctx);
    assert_eq!(runs.get(), 1);
    assert_eq!(shell.toasts().pending(), 0);
    assert!(shell.toasts().visible().is_empty());
    Ok(())
}
