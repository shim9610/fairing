//! The M2 notification integration test — the notifications, the toasts, the heads-up and the status
//! bar's notification item (A6).
//!
//! The rules: `fn … -> fairing::Result<()>`, and
//! positions come from `shell.status_bar().item_rect(id)` · `shell.heads_up().rect()` · `ActiveToast::rect`
//! rather than a copy of the layout formula. The tests that look at a curve are built with `Harness::new`
//! (= `motion.reduce = false`) and those that look only at ordering use `test_shell` (reduce).

use fairing::notify::{HeadsUpPhase, ToastPhase};
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{Cx, LaunchAction, Notification, NotificationId, Services, ShellEvent};

/// It turns a value that was not found into an error rather than a `panic!` (the lint).
fn missing(what: &str) -> fairing::Error {
    fairing::Error::Config(format!("could not find {what}"))
}

/// A harness that leaves the animations alone (`motion.reduce = false`).
fn animated() -> fairing::Result<Harness> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    Ok(h)
}

/// The skeleton smoke test: a `notify` from another thread lands in the centre on the next frame and the heads-up shows.
#[test]
fn notify_from_thread_lands_in_center_and_heads_up() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    let handle = h.shell.handle();
    std::thread::scope(|s| {
        s.spawn(move || {
            handle.notify(Notification::new(NotificationId::of("t"), "hello").body("world"));
        });
    });
    h.frames(2);
    assert_eq!(h.shell.notifications().len(), 1);
    assert_eq!(h.shell.notifications().unread(), 1);
    assert_eq!(h.shell.heads_up().visible(), Some(NotificationId::of("t")));
    h.shell.handle().toast("hi");
    h.frames(2);
    assert_eq!(h.shell.toasts().visible().len(), 1);
    // The handle is used from any thread — clearing goes round by mpsc too.
    let handle = h.shell.handle();
    std::thread::scope(|s| {
        s.spawn(move || {
            handle.toast("the second");
            handle.dismiss_notification(NotificationId::of("t"));
        });
    });
    h.frames(2);
    assert!(
        h.shell.notifications().is_empty(),
        "cleared from the thread"
    );
    assert!(
        h.shell.poll_events().iter().any(
            |e| matches!(e, ShellEvent::NotificationDismissed(id) if *id == NotificationId::of("t"))
        ),
        "the clearing goes out as an event"
    );
    Ok(())
}

/// Re-sending the same id is an update — the count and the unread count do not grow and the heads-up does not show again.
#[test]
fn same_id_updates_without_second_heads_up() -> fairing::Result<()> {
    let mut h = animated()?;
    let id = NotificationId::of("job");
    h.shell
        .notify(Notification::new(id, "Starting").body("0 %"));
    // Until the entry is over and it is `Holding`.
    let entry = h.shell.theme().motion.heads_up_in.duration.as_secs_f64();
    h.run_for(entry + 0.08);
    assert_eq!(h.shell.heads_up().phase(), Some(HeadsUpPhase::Holding));
    h.shell
        .notify(Notification::new(id, "Running").body("50 %").progress(0.5));
    h.frames(2);
    assert_eq!(h.shell.notifications().len(), 1, "dedup");
    assert_eq!(
        h.shell.notifications().unread(),
        1,
        "the unread count is as it was too"
    );
    assert_eq!(
        h.shell.notifications().get(id).map(|n| n.title.as_str()),
        Some("Running"),
        "the content is updated"
    );
    assert_eq!(
        h.shell.heads_up().phase(),
        Some(HeadsUpPhase::Holding),
        "an update does not raise the banner again (it does not go back to `Entering`)"
    );
    Ok(())
}

/// The A6 heads-up timeline: 220 ms entering → 4 s holding → 200 ms leaving. Nothing is scheduled after that.
#[test]
fn heads_up_timeline() -> fairing::Result<()> {
    let mut h = animated()?;
    let id = NotificationId::of("t");
    h.shell.notify(Notification::new(id, "Title").body("Body"));
    h.frame();
    assert_eq!(h.shell.heads_up().phase(), Some(HeadsUpPhase::Entering));
    assert!(h.shell.heads_up().y() < 0.0, "it comes down from above");
    let m = h.shell.theme().motion;
    let (entry, hold, exit) = (
        m.heads_up_in.duration.as_secs_f64(),
        m.heads_up_hold.as_secs_f64(),
        m.heads_up_out.duration.as_secs_f64(),
    );
    h.run_for(entry + 0.03);
    assert_eq!(h.shell.heads_up().phase(), Some(HeadsUpPhase::Holding));
    assert!(
        h.shell.heads_up().y().abs() < 0.5,
        "y = {}",
        h.shell.heads_up().y()
    );
    assert!(
        h.shell.heads_up().next_deadline().is_some(),
        "while holding it schedules an expiry time"
    );
    h.run_for(hold - 0.5);
    assert_eq!(
        h.shell.heads_up().visible(),
        Some(id),
        "as it was until the hold is up"
    );
    h.run_for(0.5 + exit + 0.3);
    assert_eq!(
        h.shell.heads_up().visible(),
        None,
        "the hold and the exit are over"
    );
    assert!(h.shell.heads_up().next_deadline().is_none());
    Ok(())
}

/// A heads-up tap → the notification's action runs plus `NotificationTapped`. A swipe up → it goes with no action.
#[test]
fn heads_up_tap_runs_the_action_and_swipe_up_dismisses() -> fairing::Result<()> {
    let mut h = animated()?;
    h.shell.add(fairing::screen(
        "target",
        |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("target");
        },
    ));
    let id = NotificationId::of("t");
    h.shell.notify(
        Notification::new(id, "Title")
            .body("Body")
            .action(LaunchAction::open("target")),
    );
    h.run_for(0.3);
    let rect = h
        .shell
        .heads_up()
        .rect()
        .ok_or_else(|| missing("the heads-up Rect"))?;
    h.tap(rect.center());
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::NotificationTapped(tapped) if *tapped == id)),
        "the tap event: {events:?}"
    );
    assert!(
        h.shell.workspace().find("target").is_some(),
        "the notification's action ran"
    );
    h.run_for(0.3);
    assert_eq!(
        h.shell.heads_up().visible(),
        None,
        "it leaves after the tap"
    );

    // A swipe up: `dy < −H/3` → it goes with no action.
    let id = NotificationId::of("u");
    h.shell.notify(
        Notification::new(id, "the second")
            .body("Body")
            .action(LaunchAction::open("target")),
    );
    h.run_for(0.3);
    let rect = h
        .shell
        .heads_up()
        .rect()
        .ok_or_else(|| missing("the heads-up Rect"))?;
    let from = rect.center();
    h.drag(from, egui::pos2(from.x, rect.min.y - 40.0), 4);
    assert_eq!(
        h.shell.heads_up().phase(),
        Some(HeadsUpPhase::Leaving),
        "it goes over to the leaving spring on the releasing frame"
    );
    h.run_for(0.4);
    assert_eq!(
        h.shell.heads_up().visible(),
        None,
        "swept away by the swipe"
    );
    assert!(
        !h.shell
            .poll_events()
            .iter()
            .any(|e| matches!(e, ShellEvent::NotificationTapped(_))),
        "a swipe does not run the action"
    );
    assert_eq!(
        h.shell.notifications().len(),
        2,
        "sweeping the banner away leaves the notification in the centre"
    );
    Ok(())
}

/// Opening the shade absorbs the heads-up with no exit and takes the unread count to 0.
// Turning the `overlay` feature off drops this case alone (the other notification tests stay).
#[cfg(feature = "overlay")]
#[test]
fn opening_the_shade_absorbs_the_heads_up() -> fairing::Result<()> {
    let mut h = animated()?;
    h.shell
        .notify(Notification::new(NotificationId::of("t"), "Title"));
    h.frames(2);
    assert!(h.shell.heads_up().visible().is_some());
    h.shell.launch(LaunchAction::OpenOverlay);
    h.run_for(0.5);
    assert!(h.shell.overlay().is_open());
    assert_eq!(h.shell.heads_up().visible(), None, "absorbed with no exit");
    assert_eq!(h.shell.notifications().unread(), 0);
    assert_eq!(h.shell.notifications().len(), 1, "it stays in the list");
    Ok(())
}

/// The A6 toasts: two at a time, the third waits, the one that came first sits 8 px higher, a tap makes it leave
/// at once, and the last one going leaves it idle.
#[test]
fn toast_queue_shows_two_and_lifts_the_first() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.notify.toast_ms = 200;
    let mut h = Harness::new(cfg, Services::null())?;
    h.frames(2);
    for text in ["a", "b", "c"] {
        h.shell.toast(text);
    }
    h.frames(2);
    assert_eq!(h.shell.toasts().visible().len(), 2, "two at a time");
    assert_eq!(h.shell.toasts().pending(), 1, "the third waits");
    let lift = h
        .shell
        .toasts()
        .visible()
        .first()
        .map(|t| t.lift.target())
        .ok_or_else(|| missing("the first toast"))?;
    assert!(
        (lift - 8.0).abs() < f32::EPSILON,
        "the toast that came first sits 8 px higher: {lift}"
    );
    // Once the first toast expires (200 ms plus 160 entering plus 200 leaving) the third comes in from the queue.
    h.run_for(0.7);
    assert!(
        h.shell
            .toasts()
            .visible()
            .iter()
            .any(|t| t.toast.text == "c"),
        "the waiting toast came in"
    );
    // A tap → it leaves at once.
    let rect = h
        .shell
        .toasts()
        .visible()
        .first()
        .map(|t| t.rect)
        .ok_or_else(|| missing("the toast Rect"))?;
    h.tap(rect.center());
    assert!(
        h.shell
            .toasts()
            .visible()
            .first()
            .is_none_or(|t| t.phase == ToastPhase::Leaving),
        "the tapped toast is leaving"
    );
    // With all of them gone it is idle — looked at 3 frames after the last animation frame (the harness's comment).
    h.run_for(1.0);
    assert!(h.shell.toasts().visible().is_empty());
    assert_eq!(h.shell.toasts().pending(), 0);
    h.frames(3);
    assert!(!h.repaint_requested, "idle at 0 fps with no toast");
    Ok(())
}

/// The unread badge follows the notification count and goes to 0 when the shade opens. A
/// notification that does not pass the gate stays in the list with its content hidden ("1 notification").
#[test]
fn status_notifications_badge_tracks_unread() -> fairing::Result<()> {
    let mut cfg = access_config(&["viewer", "admin"], Some("bottom"));
    // Every unassigned gate passes (bottom) and only `secret` is admin — the shade opens and only the gated
    // notification is covered.
    cfg.access
        .gates
        .insert("secret".to_owned(), "admin".to_owned());
    cfg.status_bar.right.push("status.notifications".to_owned());
    let mut h = test_shell(cfg, |_| {})?;
    h.frames(2);
    let empty = h
        .shell
        .status_bar()
        .item_rect("status.notifications")
        .ok_or_else(|| missing("the notification item"))?;
    for i in 0..3u8 {
        h.shell
            .notify(Notification::new(NotificationId(u64::from(i)), "Title"));
    }
    h.shell.notify(
        Notification::new(NotificationId::of("secret"), "Secret")
            .body("this must not show")
            .gate("secret"),
    );
    h.frames(2);
    assert_eq!(h.shell.notifications().unread(), 4);
    assert_eq!(
        h.shell.status_bar().unread(),
        4,
        "the badge follows the unread count"
    );
    let badged = h
        .shell
        .status_bar()
        .item_rect("status.notifications")
        .ok_or_else(|| missing("the notification item"))?;
    assert!(
        badged.width() > empty.width(),
        "a badge widens the item: {} → {}",
        empty.width(),
        badged.width()
    );
    // A notification that does not pass the gate has its content covered (the list draws one condensed row).
    assert_eq!(h.shell.notifications().hidden(h.shell.access()), 1);
    let secret = h
        .shell
        .notifications()
        .get(NotificationId::of("secret"))
        .ok_or_else(|| missing("the gated notification"))?;
    assert!(!fairing::notify::shows_content(secret, h.shell.access()));
    Ok(())
}

/// A notification item tap opens the shade through `tap_action = OpenOverlay`, and opening the shade takes
/// the unread count to 0 so the badge goes.
#[cfg(feature = "overlay")]
#[test]
fn notifications_item_tap_opens_the_shade_and_clears_the_badge() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.status_bar.right.push("status.notifications".to_owned());
    // The whole-bar tap is turned off, leaving the **item tap** (`tap_action`) path alone.
    cfg.status_bar.tap_opens_shade = false;
    let mut h = test_shell(cfg, |_| {})?;
    h.frames(2);
    h.shell
        .notify(Notification::new(NotificationId::of("t"), "Title"));
    h.frames(2);
    assert_eq!(h.shell.status_bar().unread(), 1);
    let item = h
        .shell
        .status_bar()
        .item_rect("status.notifications")
        .ok_or_else(|| missing("the notification item"))?;
    h.tap(item.center());
    h.frames(2);
    assert!(
        h.shell.overlay().is_open(),
        "a notification item tap opens the shade: {:?}",
        h.shell.overlay().state()
    );
    assert_eq!(h.shell.notifications().unread(), 0);
    assert_eq!(h.shell.status_bar().unread(), 0, "the badge goes to 0 too");
    Ok(())
}

/// Going past the cap (`[notify] max_items`) drops the oldest non-persistent notifications, and the badge number
/// freezes at "99+" past 99 — more digits no longer move the status bar's width.
#[test]
fn overflowing_notifications_cap_the_badge() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.notify.max_items = 100;
    cfg.status_bar.right.push("status.notifications".to_owned());
    let mut h = test_shell(cfg, |_| {})?;
    h.frames(2);
    let width = |h: &mut Harness| -> fairing::Result<f32> {
        h.frames(2);
        Ok(h.shell
            .status_bar()
            .item_rect("status.notifications")
            .ok_or_else(|| missing("the notification item"))?
            .width())
    };
    let push_upto = |h: &mut Harness, upto: u64| {
        let from = h.shell.notifications().unread() as u64;
        for i in from..upto {
            h.shell
                .notify(Notification::new(NotificationId(i), "Title"));
        }
    };
    push_upto(&mut h, 9);
    let w9 = width(&mut h)?;
    push_upto(&mut h, 99);
    let w99 = width(&mut h)?;
    push_upto(&mut h, 150);
    let w99plus = width(&mut h)?;
    assert_eq!(h.shell.notifications().len(), 100, "it keeps to the cap");
    assert_eq!(
        h.shell.status_bar().unread(),
        100,
        "the unread count does not exceed how many are kept"
    );
    assert!(w9 < w99 && w99 < w99plus, "{w9} < {w99} < {w99plus}");
    push_upto(&mut h, 200);
    let after = width(&mut h)?;
    assert!(
        (after - w99plus).abs() < f32::EPSILON,
        "past 99 the badge's width freezes: {w99plus} → {after}"
    );
    Ok(())
}

/// The A6 stack: two toasts **do not look glued together**. The place narrows by exactly as much as the push-up
/// (8 px), so the gap between the places is larger than that — once the entry and the push-up settle a gap is left between the two boxes.
#[test]
fn stacked_toasts_keep_a_visible_gap() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    h.shell.toast("first");
    h.frames(2);
    h.shell.toast("later");
    // Until the 160 ms entry plus the 120 ms push-up are over (inside the default 3 s hold).
    h.run_for(0.4);
    let visible = h.shell.toasts().visible();
    assert_eq!(visible.len(), 2, "two at a time");
    let lower = visible
        .first()
        .map(|t| t.rect)
        .ok_or_else(|| missing("the lower toast"))?;
    let upper = visible
        .get(1)
        .map(|t| t.rect)
        .ok_or_else(|| missing("the upper toast"))?;
    assert!(
        upper.max.y < lower.min.y,
        "the upper and lower do not overlap: {upper:?} / {lower:?}"
    );
    let gap = lower.min.y - upper.max.y;
    assert!(
        gap >= 4.0,
        "a gap is left between the boxes even after the push-up: {gap} px"
    );
    Ok(())
}
