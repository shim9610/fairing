//! The M2 integration test for the gesture engine plus the shade (A1).
//!
//! The cases: A1 frame 6 `y = 146` and scrim `0.152`
//! (H = 480 is made with `with_size(1024, 568)`: content = 568 − 88 = 480 < 0.85 × 568), a release
//! that confirms → settling (the physical settle is ≈ 390 ms, `Open` inside 450 ms) and
//! `y == H`, a fling of `v ≤ −1000` → `Closed`, the nested-scroll handoff (the list offset at the
//! moment of the press decides who owns it), closing by a scrim tap and by back, the
//! `Paused`/`Resumed` boundaries, a hidden screen's peek, `edge_guard` / `allow_peek = false`
//! refusing, a tile tap → `SettingChanged` (the Mock Wi-Fi) and a locked tile → `UnlockRequested`,
//! and a pull from the bottom of a visible status bar (y = 30).
//!
//! The rules: `fairing::Result<()>`, and positions come from `overlay().tile_rect` / `list_rect` /
//! `frame().panel` / `item_rect` — never from a copy of the layout formula.
// Turning the `overlay` feature off drops this target from the build too (checked by `--no-default-features --all-targets`).
#![cfg(feature = "overlay")]

use fairing::gesture::Recognizer;
use fairing::notify::{Notification, NotificationId};
use fairing::overlay::OverlayState;
use fairing::screen::{BarMode, ChromePolicy};
use fairing::settings::SettingValue;
use fairing::testing::{access_config, single_level_access, Harness};
use fairing::{
    screen, screen_with, AccessEvent, Cx, LaunchAction, Lifecycle, Services, ShellEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

/// `Option` → `Result` (the tests do not use `unwrap`).
fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("missing: {what}")))
}

/// The A1 board: 1024×568 → H = 480. Two warm-up frames.
fn a1_harness() -> fairing::Result<Harness> {
    let mut h = Harness::new(single_level_access(), Services::null())?.with_size(1024.0, 568.0);
    h.frames(2);
    Ok(h)
}

/// The A1 sequence: press at (10,8) → 12 frames at (10,300). It does not let go.
fn a1_pull(h: &mut Harness) -> egui::Pos2 {
    let from = egui::pos2(10.0, 8.0);
    let to = egui::pos2(10.0, 300.0);
    h.press(from);
    h.frame();
    for i in 1u8..=12 {
        h.move_to(from + (to - from) * (f32::from(i) / 12.0));
        h.frame();
    }
    to
}

/// Held down, it moves `step` at a time for `frames` frames from `from` (never letting go). It returns the last position.
fn pull(h: &mut Harness, from: egui::Pos2, step: egui::Vec2, frames: usize) -> egui::Pos2 {
    h.press(from);
    h.frame();
    let mut pos = from;
    for _ in 0..frames {
        pos += step;
        h.move_to(pos);
        h.frame();
    }
    pos
}

/// It runs frames until `done` goes true (at most `max`). The number of frames run.
fn settle(h: &mut Harness, max: usize, done: impl Fn(&Harness) -> bool) -> fairing::Result<usize> {
    for n in 0..=max {
        if done(h) {
            return Ok(n);
        }
        h.frame();
    }
    Err(fairing::Error::Config(format!(
        "it did not settle inside {max} frames: {:?}",
        h.shell.overlay().state()
    )))
}

/// It opens the shade by command and lets it settle.
fn open_shade(h: &mut Harness) -> fairing::Result<()> {
    h.shell.launch(LaunchAction::OpenOverlay);
    settle(h, 90, |h| h.shell.overlay().is_open())?;
    Ok(())
}

/// A1: **a visible status bar stays where it is even with the shade open.** The panel's
/// top is the top of the content (= below the bar), and being uncovered it takes no `1 − clamp(y/64)`
/// fade either. An alarm icon must not disappear on a device while the shade is open.
#[test]
fn open_shade_leaves_the_visible_status_bar_alone() -> fairing::Result<()> {
    let mut h = a1_harness()?;
    let bar = need(h.shell.layout().status, "the status bar Rect")?;
    assert!(
        bar.height() > 0.0,
        "the bar has to be visible for this test to mean anything"
    );
    open_shade(&mut h)?;
    let panel = need(h.shell.overlay().frame().panel, "panel")?;
    assert!(
        panel.min.y >= bar.max.y - 0.5,
        "the panel starts below the bar: bar {bar:?}, panel {panel:?}"
    );
    assert!(
        (h.shell.overlay().status_bar_opacity() - 1.0).abs() > f32::EPSILON,
        "the overlay's own fade value is as it was — it is the shell that reads the layout and does not use it"
    );
    // The bar really has to be drawn: an item Rect stays inside it.
    let clock = need(h.shell.status_bar().item_rect("status.clock"), "the clock")?;
    assert!(
        bar.contains_rect(clock) && !panel.contains_rect(clock),
        "the clock is inside the bar and outside the panel: clock {clock:?}"
    );
    Ok(())
}

/// The skeleton smoke test: press the top edge at (10,8) → 12 frames at (10,300) — `y = 146` on frame 6 (A1).
#[test]
fn top_edge_drag_follows_the_finger_one_to_one() -> fairing::Result<()> {
    let mut h = a1_harness()?;
    let from = egui::pos2(10.0, 8.0);
    let to = egui::pos2(10.0, 300.0);
    h.press(from);
    h.frame();
    for i in 1u8..=6 {
        h.move_to(from + (to - from) * (f32::from(i) / 12.0));
        h.frame();
    }
    let overlay = h.shell.overlay();
    assert!(
        matches!(overlay.state(), OverlayState::Dragging { .. }),
        "{:?}",
        overlay.state()
    );
    // The drag is 1:1, so y is how far the finger went — an absolute value with nothing to do with the metrics.
    assert!((overlay.y() - 146.0).abs() < 0.5, "y = {}", overlay.y());
    // H comes out of the bar's thickness, so no absolute value is written in. The scrim is the same, being
    // proportional to y/H (since the metrics became physical, 480 is no longer a constant).
    let height = overlay.height();
    assert!(height > 0.0, "H = {height}");
    // A1's rule, written in: the scrim is `0.5 × y/H`, linear.
    let expected_scrim = 0.5 * 146.0 / height;
    assert!(
        (overlay.scrim_alpha() - expected_scrim).abs() < 0.002,
        "scrim = {} (expected {expected_scrim}, H = {height})",
        overlay.scrim_alpha()
    );
    assert!(h.shell.gestures().is_active(), "the shield is up");
    Ok(())
}

/// The A1 render mapping — **curtain-style**: mid-pull (y = H/2) the panel's
/// content is still pinned to the top of the shade and only `[top, top + y]` is visible. The tile row is
/// **inside** the visible area, and the bottom of the panel (the end of the notification list, the footer
/// and the handle below it) is still **outside** the curtain — the old `top = y − H` mapping had it exactly
/// the other way round (only the footer visible and the tiles off the top of the screen).
///
/// The shade's top is **the top of the content, not the top of the screen**: a visible status bar keeps its
/// own band and the shade comes down below it (A1, the iOS control centre and the One UI quick panel).
#[test]
fn half_pulled_shade_reveals_the_panel_from_the_top() -> fairing::Result<()> {
    let mut h = a1_harness()?;
    let top = h.shell.layout().content.min.y;
    assert!(
        top > h.screen_rect().min.y,
        "this harness lays the status bar out visible — the shade starts below it"
    );
    // Pull to exactly H/2. H comes out of the bar's thickness, so **the step comes out of H** — writing in
    // 20 px × 12 would stop being H/2 the moment the metrics change.
    let half = h.shell.overlay().height() * 0.5;
    let steps = 12;
    #[expect(clippy::cast_precision_loss, reason = "steps is a small integer")]
    let step = half / steps as f32;
    let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, step), steps);
    let overlay = h.shell.overlay();
    let (y, height) = (overlay.y(), overlay.height());
    assert!((y - height * 0.5).abs() < 0.5, "y = {y}, H = {height}");
    let panel = need(overlay.frame().panel, "panel")?;
    assert!(
        (panel.min.y - top).abs() < 0.5,
        "the content is pinned to the top: {panel:?}"
    );
    assert!(
        (panel.max.y - (top + y)).abs() < 0.5,
        "the end of the visible area = y: {panel:?}"
    );
    // The tile row: inside the visible area (and near the top — the first row sits 12 px below the panel's head).
    let tile = need(overlay.tile_rect("tile.wifi"), "tile.wifi")?;
    assert!(
        panel.contains_rect(tile),
        "the tile row is inside the visible area: {tile:?} ⊄ {panel:?}"
    );
    // Near the top: within a couple of tile gaps (`screen_inset`), whatever the finger.
    let gap = h.shell.theme().metrics.screen_inset;
    assert!(
        tile.min.y - top < 2.5 * gap,
        "the tile row is near the panel's top: {tile:?}"
    );
    // Only the head of the notification list shows; its end (and the footer and handle below it) is outside the curtain.
    let list = need(overlay.list_rect(), "list_rect")?;
    assert!(
        list.min.y < panel.max.y,
        "the head of the list shows: {list:?}"
    );
    assert!(
        list.max.y > panel.max.y,
        "the end of the list and the footer are outside the curtain: {list:?} / {panel:?}"
    );
    // Letting go opens it as it is (0.5 ≥ the 0.33 snap), and the open panel is all of the top of the screen down to y = H.
    h.release(pos);
    h.frame();
    settle(&mut h, 60, |h| h.shell.overlay().is_open())?;
    let overlay = h.shell.overlay();
    let open = need(overlay.frame().panel, "panel")?;
    assert!((open.min.y - top).abs() < 0.5, "{open:?}");
    assert!(
        (open.max.y - (top + overlay.height())).abs() < 0.5,
        "{open:?}"
    );
    let list = need(overlay.list_rect(), "list_rect")?;
    assert!(
        open.contains_rect(list),
        "open, the whole list is inside: {list:?}"
    );
    Ok(())
}

/// The A1 release: letting go after 12 frames gives `Settling{opening}` → `Open` by the physical settle,
/// `y == H`, and a top icon opacity of 0. The settle is ≈ 390 ms at x0 = −188 · v0 ≈ 1460 · k 400, so 450 ms
/// (27 frames) is the ceiling (`shade.rs`'s docs). The tap on the releasing frame must not leak
/// past the shield and have the status bar's `tap_opens_shade` close it again — it is still open 3 frames later.
#[test]
fn release_past_snap_opens_and_settles_within_450ms() -> fairing::Result<()> {
    let mut h = a1_harness()?;
    let to = a1_pull(&mut h);
    assert!((h.shell.overlay().y() - 292.0).abs() < 0.5);
    h.release(to);
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: true }
    );
    assert!(
        !h.shell.gestures().is_active(),
        "the shield comes down on the releasing frame"
    );
    let n = settle(&mut h, 40, |h| h.shell.overlay().is_open())?;
    assert!(n <= 27, "Open inside 450 ms: {n} frames");
    assert!(
        n >= 15,
        "the physical settle was not brought forward: {n} frames"
    );
    let overlay = h.shell.overlay();
    assert!((overlay.y() - overlay.height()).abs() < 1e-3, "y == H");
    assert!(overlay.status_bar_opacity().abs() < 1e-6);
    h.frames(3);
    assert!(
        h.shell.overlay().is_open(),
        "no tap leaked from the releasing frame"
    );
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OverlayToggled(true))));
    Ok(())
}

/// A1: however far it was pulled, letting go at `v ≤ −1000` gives `Closed`.
#[test]
fn fling_up_closes_even_when_far() -> fairing::Result<()> {
    let mut h = a1_harness()?;
    let mut pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 40.0), 10);
    assert!(
        h.shell.overlay().y() > 380.0,
        "y = {}",
        h.shell.overlay().y()
    );
    // The 100 ms window regression: −16.7 px per frame ≈ −1000 px/s.
    for _ in 0..8 {
        pos.y -= 16.7;
        h.move_to(pos);
        h.frame();
    }
    h.release(pos);
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false },
        "a fling the other way closes it"
    );
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    Ok(())
}

/// A1's nested scrolling with the shade open — the list offset at the moment of the press decides who owns
/// that gesture: upwards at offset 0 = `Dragging` from H (1:1), upwards at offset > 0 = a scroll, and dragging
/// down to reach 0 still ends that gesture as a scroll (no rubber band and no handoff); the next gesture is judged afresh.
#[test]
fn nested_scroll_hands_off_to_shade_once_per_gesture() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    for i in 0..10 {
        h.shell.notify(
            Notification::new(NotificationId::of(&format!("n{i}")), format!("Item {i}"))
                .body("body"),
        );
    }
    open_shade(&mut h)?;
    h.frames(2);
    let list = need(h.shell.overlay().list_rect(), "list_rect")?;
    let height = h.shell.overlay().height();
    assert!(h.shell.overlay().list_offset().abs() < 0.5);
    let p = list.center();
    // A. Offset 0 plus 40 px upwards (5 frames × 8 px, v ≈ 480 < a fling) → a shade drag from H, 1:1.
    let up = pull(&mut h, p, egui::vec2(0.0, -8.0), 5);
    assert!(
        matches!(h.shell.overlay().state(), OverlayState::Dragging { .. }),
        "{:?}",
        h.shell.overlay().state()
    );
    assert!(
        (h.shell.overlay().y() - (height - 40.0)).abs() < 0.5,
        "y = {} (H − 40 = {})",
        h.shell.overlay().y(),
        height - 40.0
    );
    assert!(
        h.shell.overlay().list_offset().abs() < 0.5,
        "the list did not scroll"
    );
    h.release(up);
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: true },
        "progress 0.92 ≥ 0.33 → it opens again"
    );
    settle(&mut h, 60, |h| h.shell.overlay().is_open())?;
    // B. The wheel makes offset > 0 (at offset 0 a drag belongs to the shade, so the wheel is the only way).
    h.move_to(p);
    h.frame();
    for _ in 0..2 {
        h.push_event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -60.0),
            modifiers: egui::Modifiers::default(),
            phase: egui::TouchPhase::Move,
        });
        h.frame();
    }
    h.frames(30);
    let off = h.shell.overlay().list_offset();
    assert!(off > 20.0, "a wheel scroll: {off}");
    // Offset > 0 plus upwards → the list scrolls (the shade stays Open).
    let up = pull(&mut h, p, egui::vec2(0.0, -8.0), 5);
    assert!(
        h.shell.overlay().is_open(),
        "{:?}",
        h.shell.overlay().state()
    );
    let scrolled = h.shell.overlay().list_offset();
    assert!(scrolled > off + 20.0, "scrolled: {off} → {scrolled}");
    h.release(up);
    h.frame();
    h.frames(40);
    // C. Offset > 0 (with the inertia after the release) plus 600 px downwards (more than the maximum offset) →
    // reaching 0 still ends that gesture as a scroll.
    let off = h.shell.overlay().list_offset();
    assert!(off > 0.0);
    let down = pull(&mut h, p, egui::vec2(0.0, 30.0), 20);
    assert!(
        h.shell.overlay().is_open(),
        "no handoff: {:?}",
        h.shell.overlay().state()
    );
    assert!(
        h.shell.overlay().list_offset().abs() < 0.5,
        "it stops at 0: {}",
        h.shell.overlay().list_offset()
    );
    assert!(
        (h.shell.overlay().y() - height).abs() < 1e-3,
        "no rubber band: y = {}",
        h.shell.overlay().y()
    );
    h.release(down);
    h.frame();
    h.frames(40);
    // D. A new gesture: offset 0 plus upwards → the shade again.
    let up = pull(&mut h, p, egui::vec2(0.0, -8.0), 5);
    assert!(
        matches!(h.shell.overlay().state(), OverlayState::Dragging { .. }),
        "{:?}",
        h.shell.overlay().state()
    );
    h.release(up);
    h.frame();
    settle(&mut h, 60, |h| h.shell.overlay().is_open())?;
    Ok(())
}

/// With the shade open, dragging a notification row sideways by a third of the width or more
/// (or flinging it) and letting go clears that notification (`NotificationDismissed`); a vertical drag or a tap does not.
///
/// **The shade's close × had a press target smaller than a touch target.** It used the drawn size,
/// a multiple of `shade.close` (0.72), as the hit Rect — 9.4 mm at the gloved default, short of `finger_hard_mm`
/// (10.1 mm). The prescription is the same as for `switch`: **keep the drawn size and widen only
/// the hit Rect.**
///
/// So this looks at the Rect's size and at **whether pressing that place really does clear it** together — code
/// that says it widened the Rect but never passed it to `ui.interact` would pass a size assertion on its own.
#[test]
fn the_shade_close_button_is_a_full_touch_target() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    h.shell
        .notify(Notification::new(NotificationId::of("x1"), "Close target"));
    open_shade(&mut h)?;
    h.frames(2);

    let hit = need(h.shell.overlay().close_hit_rect(), "close_hit_rect")?;
    let touch = h.shell.theme().metrics.touch_target;
    assert!(
        hit.width() >= touch - 0.5 && hit.height() >= touch - 0.5,
        "the close hit {:?} is short of the touch target {touch}",
        hit.size()
    );

    // Press **outside** the drawn disc and **inside** the hit Rect. If the widened area is not really wired up,
    // nothing is cleared here.
    let drawn = h.shell.theme().metrics.row_height * h.shell.theme().components.shade.close;
    let probe = egui::pos2(hit.center().x - drawn * 0.5 - 2.0, hit.center().y);
    assert!(
        hit.contains(probe),
        "the probe {probe:?} is outside the hit Rect"
    );
    assert_eq!(h.shell.notifications().len(), 1);
    let _ = h.shell.poll_events();
    h.tap(probe);
    // One cleared with the × is also cleared **after it has flown out** (the same path as the swipe).
    h.frames(2);
    assert_eq!(
        h.shell.notifications().len(),
        1,
        "it was cleared the moment it was pressed"
    );
    h.frames(40);
    assert_eq!(
        h.shell.notifications().len(),
        0,
        "a place outside the drawn disc but inside the hit Rect was pressed and nothing was cleared"
    );
    Ok(())
}

#[test]
fn notification_row_swipe_dismisses_it() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    for i in 0..3 {
        h.shell.notify(Notification::new(
            NotificationId::of(&format!("s{i}")),
            format!("Swipe {i}"),
        ));
    }
    open_shade(&mut h)?;
    h.frames(2);
    let list = need(h.shell.overlay().list_rect(), "list_rect")?;
    let p = list.center();
    assert_eq!(h.shell.notifications().len(), 3);
    let _ = h.shell.poll_events();
    // A tap does not clear it (only `NotificationTapped`).
    h.tap(p);
    let events = h.shell.poll_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::NotificationTapped(_))));
    assert_eq!(h.shell.notifications().len(), 3);
    assert!(h.shell.overlay().is_open());
    // Drag 360 px (> 1024 / 3) to the **left** and let go → cleared. The right is the opening side.
    let end = pull(&mut h, p, egui::vec2(-30.0, 0.0), 12);
    assert!(
        h.shell.overlay().is_open(),
        "a sideways drag does not move the shade"
    );
    assert!(
        (h.shell.overlay().y() - h.shell.overlay().height()).abs() < 1e-3,
        "y = {}",
        h.shell.overlay().y()
    );
    h.release(end);
    // **It is not cleared yet while it is flying out** — clearing it at once leaves nothing to see going.
    h.frame();
    assert_eq!(
        h.shell.notifications().len(),
        3,
        "it was cleared the moment it was let go — there is no leaving animation"
    );
    // The second step: once the card is all the way out, **the space** folds down to 0. The notification is still in
    // the list meanwhile, and the rows below come up to fill the gap. Vanishing at a stroke leaves no idea what went.
    let mut shrink = Vec::new();
    for _ in 0..60 {
        h.frame();
        if let Some((_, left)) = h.shell.overlay().dismissing() {
            assert_eq!(
                h.shell.notifications().len(),
                3,
                "it is still there while it folds"
            );
            shrink.push(left);
        }
    }
    assert!(
        shrink.len() >= 6,
        "the space went in one frame — there is nothing to see coming up: {shrink:?}"
    );
    assert!(
        shrink
            .windows(2)
            .all(|w| w.get(1) <= w.first().map(|v| v + 1e-3).as_ref()),
        "the space grows back: {shrink:?}"
    );
    let flat = shrink.last().copied().unwrap_or(1.0);
    assert!(flat < 0.05, "it did not fold all the way: {flat}");
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::NotificationDismissed(_))),
        "{events:?}"
    );
    assert_eq!(h.shell.notifications().len(), 2, "one was cleared");
    assert!(h.shell.overlay().is_open());
    Ok(())
}

/// The vertical distance from the top of the list to the centre of the first row.
fn first_row_y(h: &Harness) -> f32 {
    h.shell.theme().metrics.notification_row_height / 2.0
}

/// Pushing it to the right opens **the screen that notification points at** — only for a notification carrying a
/// destination. The direction is what tells them apart: the left throws it away, the right opens it. A notification
/// with nowhere to go does not drag to the right at all — better than looking as though something will happen and
/// then nothing does.
#[test]
fn swiping_a_routed_notification_right_opens_its_screen() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.shell.add(
        fairing::screen(
            "net.detail",
            |_ui: &mut egui::Ui, _cx: &mut fairing::Cx<'_>| {},
        )
        .title("Net"),
    );
    h.frames(2);
    h.shell.notify(
        Notification::new(NotificationId::of("routed"), "Sensor 3 offline")
            .action(fairing::LaunchAction::open("net.detail")),
    );
    open_shade(&mut h)?;
    h.frames(2);
    let list = need(h.shell.overlay().list_rect(), "list_rect")?;
    // With one notification the **middle** of the list is empty space — aim at the first row.
    let p = egui::pos2(list.center().x, list.top() + first_row_y(&h));
    let _ = h.shell.poll_events();

    let end = pull(&mut h, p, egui::vec2(30.0, 0.0), 12);
    h.release(end);
    h.frames(30);
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::NotificationTapped(_))),
        "it was pushed right and nothing opened: {events:?}"
    );
    assert_eq!(h.shell.notifications().len(), 1, "opening is not clearing");
    assert!(
        !h.shell.overlay().is_open(),
        "having opened a screen, the shade has to get out of the way"
    );
    Ok(())
}

/// A notification with nowhere to go **does not drag to the right.** If it did it would look as though it would open.
#[test]
fn a_notification_with_nowhere_to_go_does_not_swipe_right() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    h.shell.notify(Notification::new(
        NotificationId::of("plain"),
        "Backup done",
    ));
    open_shade(&mut h)?;
    h.frames(2);
    let list = need(h.shell.overlay().list_rect(), "list_rect")?;
    let p = egui::pos2(list.center().x, list.top() + first_row_y(&h));
    let _ = h.shell.poll_events();

    let end = pull(&mut h, p, egui::vec2(30.0, 0.0), 12);
    h.release(end);
    h.frames(40);
    let events = h.shell.poll_events();
    assert!(
        !events.iter().any(|e| matches!(
            e,
            ShellEvent::NotificationDismissed(_) | ShellEvent::NotificationTapped(_)
        )),
        "it was pushed right and something happened: {events:?}"
    );
    assert_eq!(h.shell.notifications().len(), 1, "it stays as it is");
    assert!(h.shell.overlay().is_open());
    Ok(())
}

/// A scrim tap → `Closing` (= `Settling{opening: false}`) → `Closed`; back does the same.
#[test]
fn scrim_tap_and_back_close_the_shade() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    open_shade(&mut h)?;
    let panel = need(h.shell.overlay().frame().panel, "panel")?;
    let below = egui::pos2(
        panel.center().x,
        f32::midpoint(panel.max.y, h.screen_rect().max.y),
    );
    assert!(!panel.contains(below), "a place on the scrim");
    h.tap(below);
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false },
        "a scrim tap → Closing"
    );
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    // A tap on an empty place on the panel does not close it (the panel's floor takes it).
    open_shade(&mut h)?;
    let panel = need(h.shell.overlay().frame().panel, "panel")?;
    h.tap(egui::pos2(panel.min.x + 4.0, panel.center().y));
    assert!(
        h.shell.overlay().is_open(),
        "{:?}",
        h.shell.overlay().state()
    );
    // Back.
    h.shell.back();
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false }
    );
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    Ok(())
}

/// A screen that writes its lifecycle down in order.
struct Recorder {
    log: Rc<RefCell<Vec<Lifecycle>>>,
}

impl fairing::Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("r");
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push(event);
    }
}

/// Leaving `Closed` gives the focused screen `Paused` and coming back gives it `Resumed`;
/// `OverlayToggled(true/false)` fires on the same boundaries.
#[test]
fn overlay_pauses_and_resumes_focused_screen() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = Rc::clone(&log);
    h.shell
        .add(screen_with("r", move || Recorder { log: Rc::clone(&l) }));
    h.frames(2);
    h.shell.launch(LaunchAction::open("r"));
    // `Resumed` only once the push tween (220 ms) is over (the A3 lifecycle).
    settle(&mut h, 40, |h| !h.shell.is_animating())?;
    h.frame();
    assert_eq!(*log.borrow(), vec![Lifecycle::Created, Lifecycle::Resumed]);
    let _ = h.shell.poll_events();
    // Starting the pull = leaving Closed → Paused.
    let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 8.0), 4);
    assert_eq!(
        *log.borrow(),
        vec![Lifecycle::Created, Lifecycle::Resumed, Lifecycle::Paused]
    );
    let events = h.shell.poll_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::OverlayToggled(true))));
    // A release short of the snap → closed → Resumed.
    h.release(pos);
    h.frame();
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    h.frame();
    assert_eq!(
        *log.borrow(),
        vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Resumed
        ]
    );
    let events = h.shell.poll_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::OverlayToggled(false))));
    assert!(!events
        .iter()
        .any(|e| matches!(e, ShellEvent::OverlayToggled(true))));
    Ok(())
}

/// On a `.fullscreen()` screen a short pull and release leaves the status row as a peek
/// (`OverlayFrame::peek` plus the clock item), and it goes 2 s later. Pulling further makes the status row the panel's top.
#[test]
fn hidden_status_bar_peeks_then_opens() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.shell.add(
        screen("fs", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("fs");
        })
        .fullscreen(),
    );
    h.frames(2);
    h.shell.launch(LaunchAction::open("fs"));
    h.frames(3);
    assert!(
        h.shell.layout().status.is_none(),
        "the status bar is hidden"
    );
    assert!(h.shell.status_bar().item_rect("status.clock").is_none());
    // A short pull: 32 px ≥ half the status bar's height, progress 32/510 < 0.33, v ≈ 480 < a fling → closed plus a peek.
    let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 8.0), 4);
    assert!(
        h.shell.overlay().frame().status_row.is_some(),
        "while pulling, the panel's top is the status row"
    );
    h.release(pos);
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false }
    );
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    h.frame();
    assert!(h.shell.overlay().frame().peek.is_some(), "the peek row");
    assert!(
        h.shell.status_bar().item_rect("status.clock").is_some(),
        "the clock is on the peek row"
    );
    h.run_for(2.1);
    assert!(
        h.shell.overlay().frame().peek.is_none(),
        "it goes 2 s later"
    );
    assert!(h.shell.status_bar().item_rect("status.clock").is_none());
    // Pulling further → the panel plus the status row.
    let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 40.0), 6);
    let frame = h.shell.overlay().frame();
    assert!(frame.panel.is_some() && frame.status_row.is_some());
    assert!(h.shell.status_bar().item_rect("status.clock").is_some());
    h.release(pos);
    h.frame();
    settle(&mut h, 60, |h| h.shell.overlay().is_open())?;
    Ok(())
}

/// `edge_guard` refuses every edge, and `allow_peek = false` plus `Hide` refuses that bar's edge —
/// pulling leaves it `Closed` and the engine `Passed`.
#[test]
fn edge_guard_blocks_top_pull_and_allow_peek_false_blocks_hidden_bar() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.shell.add(
        screen("guard", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("guard");
        })
        .chrome(ChromePolicy {
            edge_guard: true,
            ..ChromePolicy::default()
        }),
    );
    h.shell.add(
        screen("nopeek", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("nopeek");
        })
        .chrome(ChromePolicy {
            status_bar: BarMode::Hide,
            allow_peek: false,
            ..ChromePolicy::default()
        }),
    );
    h.frames(2);
    for id in ["guard", "nopeek"] {
        h.shell.launch(LaunchAction::open(id));
        h.frames(3);
        let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 30.0), 6);
        assert!(h.shell.overlay().is_closed(), "{id}: the pull was ignored");
        assert!(
            matches!(h.shell.gestures().recognizer(), Recognizer::Passed { .. }),
            "{id}: {:?}",
            h.shell.gestures().recognizer()
        );
        assert!(!h.shell.gestures().is_active());
        h.release(pos);
        h.frames(2);
        h.shell.handle().home();
        h.frames(3);
    }
    // Back on the default policy the same pull opens the shade.
    let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 30.0), 6);
    assert!(matches!(
        h.shell.overlay().state(),
        OverlayState::Dragging { .. }
    ));
    h.release(pos);
    h.frame();
    Ok(())
}

/// A `tile.wifi` tap → `Toggle(wifi.enabled)` → `SettingChanged` plus the Mock backend
/// following; at level 2 with `default_gate = "top"` a locked tile gives `UnlockRequested { gate: tile.wifi }`.
#[test]
fn tile_tap_toggles_mock_wifi_and_locked_tile_requests_unlock() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?;
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    assert!(h.shell.services().wifi.enabled(), "the Mock starts on");
    let cell = need(h.shell.overlay().tile_rect("tile.wifi"), "tile.wifi")?;
    let _ = h.shell.poll_events();
    h.tap(cell.center());
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::SettingChanged { key, value: SettingValue::Bool(false) }
                if key.0.as_ref() == "wifi.enabled"
        )),
        "{events:?}"
    );
    assert!(!h.shell.services().wifi.enabled(), "the backend followed");
    h.frames(2);
    assert!(
        h.shell.overlay().is_open(),
        "a tile tap does not close the shade"
    );
    // A locked tile.
    let mut cfg = access_config(&["viewer", "admin"], Some("top"));
    cfg.access
        .gates
        .insert("overlay.open".to_owned(), "viewer".to_owned());
    let mut h = Harness::new(cfg, fairing::services::mock::services())?;
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    let cell = need(h.shell.overlay().tile_rect("tile.wifi"), "tile.wifi")?;
    assert!(
        h.shell
            .overlay()
            .tiles()
            .iter()
            .any(|(t, _, allowed)| t.id == "tile.wifi" && !allowed),
        "the render filter: locked"
    );
    let _ = h.shell.poll_events();
    h.tap(cell.center());
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::Access(AccessEvent::UnlockRequested { gate, .. }) if gate.as_str() == "tile.wifi"
        )),
        "{events:?}"
    );
    assert!(
        h.shell.services().wifi.enabled(),
        "a locked tile does not change anything"
    );
    Ok(())
}

/// A1: a visible status bar (32 px > `edge_px` 24) is an edge zone across its whole thickness —
/// a press at (10, 30) pulls the shade and (10, 40) is `Passed`.
#[test]
fn visible_status_bar_bottom_starts_the_shade_pull() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    // The bar's thickness comes out of physical units, so **the inside and outside coordinates come out of the real thickness**.
    let bar = h.shell.layout().status.map_or(0.0, |r| r.height());
    assert!(
        bar > 0.0,
        "the bar has to be visible for this test to mean anything"
    );
    let pos = pull(
        &mut h,
        egui::pos2(10.0, bar - 2.0),
        egui::vec2(0.0, 10.0),
        4,
    );
    assert!(
        matches!(h.shell.overlay().state(), OverlayState::Dragging { .. }),
        "inside the bar ({}) is the edge zone: {:?}",
        bar - 2.0,
        h.shell.overlay().state()
    );
    h.release(pos);
    h.frame();
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    let pos = pull(
        &mut h,
        egui::pos2(10.0, bar + 8.0),
        egui::vec2(0.0, 10.0),
        4,
    );
    assert!(h.shell.overlay().is_closed());
    assert!(matches!(
        h.shell.gestures().recognizer(),
        Recognizer::Passed { .. }
    ));
    h.release(pos);
    h.frame();
    Ok(())
}

/// The A1 interruption rule "home or back while opening → `Settling → Closed`": home closes the shade just
/// as back does (the same when pressed on the home screen — the nav bar's home is eaten by the scrim, but
/// `ShellHandle::home()` and an integrator's call come through here).
#[test]
fn home_closes_the_shade() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.shell.add(screen("s", |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label("s");
    }));
    h.frames(2);
    h.shell.launch(LaunchAction::open("s"));
    settle(&mut h, 40, |h| !h.shell.is_animating())?;
    open_shade(&mut h)?;
    h.shell.home();
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false },
        "home → Closing"
    );
    settle(&mut h, 60, |h| h.shell.overlay().is_closed())?;
    assert!(h.shell.workspace().is_home());
    // Opening it again on the home screen and pressing home closes it too (before the `is_home` early return).
    open_shade(&mut h)?;
    h.shell.home();
    h.frame();
    assert_eq!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false }
    );
    Ok(())
}

/// Where `overlay.open` does not pass, a top pull is **quietly ignored** — it does not open and
/// no event comes out either (the unlock prompt belongs to the tap and command path, `launch(OpenOverlay)`).
#[test]
fn blocked_top_pull_is_ignored_without_events() -> fairing::Result<()> {
    let mut h = Harness::new(
        access_config(&["viewer", "admin"], Some("top")),
        Services::null(),
    )?
    .with_size(1024.0, 568.0);
    h.frames(2);
    let _ = h.shell.poll_events();
    let pos = a1_pull(&mut h);
    assert!(
        h.shell.overlay().is_closed(),
        "{:?}",
        h.shell.overlay().state()
    );
    assert!(h.shell.overlay().y().abs() < 1e-6, "y = 0");
    h.release(pos);
    h.frames(2);
    assert!(h.shell.overlay().is_closed());
    let events = h.shell.poll_events();
    assert!(
        !events.iter().any(|e| matches!(
            e,
            ShellEvent::OverlayToggled(_) | ShellEvent::Access(AccessEvent::UnlockRequested { .. })
        )),
        "a refused pull raises no event: {events:?}"
    );
    Ok(())
}

/// When the shade starts while something of lower priority (a screen transition) is running, the
/// lower one is **finished at once** — the two never run together.
#[test]
fn shade_start_settles_a_running_screen_transition() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?.with_size(1024.0, 568.0);
    h.shell.add(screen("t", |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label("t");
    }));
    h.frames(2);
    h.shell.launch(LaunchAction::open("t"));
    h.frame();
    assert!(
        h.shell.workspace().is_animating(),
        "the A2 open transition is running"
    );
    // Cross the slop (12 px) at the top edge to start the shade.
    let from = egui::pos2(10.0, 8.0);
    h.press(from);
    h.frame();
    h.move_to(from + egui::vec2(0.0, 32.0));
    h.frame();
    assert!(
        matches!(h.shell.overlay().state(), OverlayState::Dragging { .. }),
        "{:?}",
        h.shell.overlay().state()
    );
    assert!(
        !h.shell.workspace().is_animating(),
        "once the shade starts, the transition settles at once"
    );
    Ok(())
}

/// With `[notify] dismiss_button = false` **there is no × at all.**
///
/// On a device used with a finger alone the swipe is enough and the card is tidier. On a device where the swipe
/// does not take well — gloves, a resistive panel — the button has to be there, so this is **policy, not taste.**
///
/// Turning it off must not leave no way to clear one — the left swipe has to keep working.
#[test]
fn turning_off_the_dismiss_button_hides_it_but_keeps_the_swipe() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.notify.dismiss_button = false;
    let mut h = Harness::new(config, Services::null())?;
    h.frames(2);
    for i in 0..3 {
        h.shell.notify(Notification::new(
            NotificationId::of(&format!("n{i}")),
            format!("Note {i}"),
        ));
    }
    open_shade(&mut h)?;
    h.frames(2);

    let hit = h.shell.overlay().close_hit_rect();
    assert!(
        hit.is_none_or(|r| r == egui::Rect::NOTHING),
        "the × is off and there is still somewhere to press: {hit:?}"
    );

    // A way to clear it has to remain — the left swipe.
    let list = need(h.shell.overlay().list_rect(), "list_rect")?;
    let p = egui::pos2(list.center().x, list.top() + first_row_y(&h));
    let _ = h.shell.poll_events();
    let end = pull(&mut h, p, egui::vec2(-30.0, 0.0), 12);
    h.release(end);
    h.frames(60);
    assert_eq!(
        h.shell.notifications().len(),
        2,
        "turning the × off killed the swipe with it"
    );
    Ok(())
}

/// Straight after boot **the dark toggle matches the real theme.**
///
/// It used to look only at the settings memory (`theme.dark`). Straight after boot that value is not there, so it
/// drew as `Off` and showed something the shell already had on as off. For the same reason Wi-Fi and Bluetooth ask
/// the service for the real state, the theme looks at **what is being drawn now**.
#[test]
fn the_theme_tile_matches_the_theme_at_boot() -> fairing::Result<()> {
    for dark in [true, false] {
        let mut config = single_level_access();
        config.shell.theme = if dark { "dark" } else { "light" }.to_owned();
        let mut h = Harness::new(config, Services::null())?;
        h.frames(2);
        open_shade(&mut h)?;
        h.frames(2);
        assert_eq!(
            h.shell.theme().dark,
            dark,
            "the shell's theme differs from the setting (dark = {dark})"
        );
        let Some(rect) = h.shell.overlay().tile_rect("tile.theme") else {
            return Err(fairing::Error::Config("there is no tile.theme".to_owned()));
        };
        // A tile that is on is painted `Primary` — read from the colour at the centre of the disc.
        let want = h.shell.theme().color(fairing::ColorRole::Primary);
        let painted = h.frame_shapes().iter().rev().any(|s| match &s.shape {
            egui::Shape::Circle(c) => rect.contains(c.center) && c.fill == want,
            _ => false,
        });
        assert_eq!(
            painted,
            dark,
            "the theme is dark = {dark} and the tile is {}",
            if painted { "on" } else { "off" }
        );
    }
    Ok(())
}

/// Opening a Slider tile takes **that tile out of the row and down to the left of the slider**, and the tiles left
/// behind fill the gap.
///
/// It used to send a **copy** of the icon down and leave the tile in the row, so both were visible. Now the tile
/// itself moves, so pressing the disc that came down — being that same tile — folds it back.
#[test]
fn opening_a_slider_tile_moves_it_out_of_the_row() -> fairing::Result<()> {
    // The brightness tile is only alive if the backend gives it a value — with `Services::null()` it is `Unavailable`.
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?;
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);

    let at = |h: &Harness, id: &str| h.shell.overlay().tile_rect(id);
    let Some((home, wifi_before, dark_before)) = at(&h, "tile.brightness")
        .zip(at(&h, "tile.wifi"))
        .zip(at(&h, "tile.theme"))
        .map(|((b, w), d)| (b, w, d))
    else {
        return Err(fairing::Error::Config("there is no tile Rect".to_owned()));
    };

    h.tap(home.center());
    h.frames(40); // until the tween is over

    let Some((moved, wifi_after, dark_after)) = at(&h, "tile.brightness")
        .zip(at(&h, "tile.wifi"))
        .zip(at(&h, "tile.theme"))
        .map(|((b, w), d)| (b, w, d))
    else {
        return Err(fairing::Error::Config("there is no tile Rect".to_owned()));
    };

    assert!(
        moved.center().y > home.center().y + home.height() * 0.5,
        "the tile did not come down below the row: {home:?} → {moved:?}"
    );
    assert!(
        moved.width() < home.width(),
        "it did not get smaller on the way down: {} → {}",
        home.width(),
        moved.width()
    );
    // **The tiles left behind fill the gap** — being centred, they come in from both sides.
    assert!(
        wifi_after.center().x > wifi_before.center().x,
        "the left tile did not come in: {} → {}",
        wifi_before.center().x,
        wifi_after.center().x
    );
    assert!(
        dark_after.center().x < dark_before.center().x,
        "the right tile did not come in: {} → {}",
        dark_before.center().x,
        dark_after.center().x
    );

    // The disc that came down is that tile — pressing it folds it back.
    h.tap(moved.center());
    h.frames(40);
    let Some(back) = at(&h, "tile.brightness") else {
        return Err(fairing::Error::Config("there is no tile Rect".to_owned()));
    };
    assert!(
        (back.center() - home.center()).length() < 1.0,
        "it was pressed again and did not go back into place: {home:?} → {back:?}"
    );
    Ok(())
}

/// A built-in tile goes to the built-in settings screen that pairs with it **without being attached to one** —
/// a long press on Wi-Fi gives the Wi-Fi settings. Only where that screen is registered.
#[test]
fn a_builtin_tile_falls_back_to_its_builtin_settings_screen() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?;
    h.shell.add(fairing::screen(
        "settings.wifi",
        |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("wifi settings");
        },
    ));
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    let cell = need(h.shell.overlay().tile_rect("tile.wifi"), "tile.wifi")?;
    let _ = h.shell.poll_events();
    h.hold(cell.center(), 40);
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "settings.wifi")),
        "the built-in pair did not open: {events:?}"
    );
    Ok(())
}

/// A7: **a long press on a tile goes where it was attached.**
///
/// Where it goes is the integrator's to decide (the crate knows no settings screen names). Attached or not, the
/// fact of the long press goes out as `TileLongPressed`, so anyone wanting to raise a menu takes that.
///
/// It uses the engine's `Gesture::LongPress` rather than egui's `long_touched()` — the latter only stands up when
/// there are real touch events, and never fires at all on a panel that passes touches through as the mouse.
#[test]
fn holding_a_tile_opens_what_the_integrator_bound_to_it() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?;
    h.shell.add(fairing::screen(
        "settings.wifi",
        |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("wifi settings");
        },
    ));
    h.shell.add(
        fairing::overlay::tile(
            "tile.wifi",
            fairing::overlay::TileKind::Toggle(fairing::settings::SettingKey::from("wifi.enabled")),
        )
        .long_press(LaunchAction::open("settings.wifi")),
    );
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    let cell = need(h.shell.overlay().tile_rect("tile.wifi"), "tile.wifi")?;
    let on_before = h.shell.services().wifi.enabled();
    let _ = h.shell.poll_events();

    // Held for more than 500 ms. It opens before the release.
    h.hold(cell.center(), 40);
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::TileLongPressed { id } if id == "tile.wifi"
        )),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "settings.wifi")),
        "the attached screen did not open: {events:?}"
    );
    // **A long press must not toggle as well** — Wi-Fi going off as the settings open would be an accident.
    assert_eq!(
        h.shell.services().wifi.enabled(),
        on_before,
        "the long press produced a tap too"
    );
    h.frames(20);
    assert!(
        !h.shell.overlay().is_open(),
        "having taken us to another screen, the shade goes back up"
    );
    Ok(())
}

/// With no screen to pair with, a long press **changes no screen** and only raises the event — taking that and
/// raising a menu, or not, is the integrator's. (The `Services::null()` harness registers no built-in settings
/// screens, so `tile.wifi`'s default pair is empty too.)
#[test]
fn holding_a_plain_tile_only_reports_it() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?;
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    let cell = need(h.shell.overlay().tile_rect("tile.wifi"), "tile.wifi")?;
    let on_before = h.shell.services().wifi.enabled();
    let _ = h.shell.poll_events();
    h.hold(cell.center(), 40);
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::TileLongPressed { id } if id == "tile.wifi"
        )),
        "{events:?}"
    );
    assert!(
        h.shell.overlay().is_open(),
        "with nowhere to go the shade stays as it is too"
    );
    // Lifting the finger gives no tap either — a long press was just let go of.
    h.release(cell.center());
    h.frames(3);
    assert_eq!(
        h.shell.services().wifi.enabled(),
        on_before,
        "a long press and release toggled Wi-Fi"
    );
    Ok(())
}

/// **Something the crate knows nothing of, slotted into the quick-settings panel.**
///
/// `tile_panel` takes only the tile's shape and the opening and closing animation from the shell, and the inside of
/// the expanded row is drawn wholly by the integrator — several gauges, a small chart, a device-specific control,
/// whatever. Until now only turning an existing tile off or reordering them was possible.
#[test]
fn an_integrator_tile_draws_its_own_expanded_row() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.overlay.tiles = vec!["tile.wifi".to_owned(), "tile.hopper".to_owned()];
    let mut h = Harness::new(config, fairing::services::mock::services())?;
    let drawn = std::rc::Rc::new(std::cell::Cell::new(0_u32));
    let seen = std::rc::Rc::clone(&drawn);
    let body = std::rc::Rc::new(std::cell::Cell::new(egui::Rect::NOTHING));
    let body_seen = std::rc::Rc::clone(&body);
    h.shell.add(
        fairing::overlay::tile_panel("tile.hopper", 168.0, move |ui, _cx| {
            seen.set(seen.get() + 1);
            body_seen.set(ui.max_rect());
        })
        .label("Hoppers"),
    );
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);

    let cell = need(h.shell.overlay().tile_rect("tile.hopper"), "tile.hopper")?;
    assert_eq!(drawn.get(), 0, "it is closed and the body was drawn");

    h.tap(cell.center());
    h.frames(40);
    assert!(
        drawn.get() > 0,
        "the tile was pressed and the body was not drawn"
    );
    let rect = body.get();
    assert!(rect.is_positive(), "the body Rect is empty: {rect:?}");
    assert!(
        rect.height() >= 168.0 - 1.0,
        "it did not get the height it declared (168): {}",
        rect.height()
    );
    // A drag that starts inside the row is not taken by the shade — a slider can go in there.
    let owned = need(h.shell.overlay().expanded_rect(), "the expanded row Rect")?;
    assert!(
        owned.contains(rect.center()),
        "{owned:?} does not cover the body"
    );

    // Open, the tile **has come down** to the left of the row — pressing that place closes it.
    let moved = need(h.shell.overlay().tile_rect("tile.hopper"), "tile.hopper")?;
    assert!(
        moved.center().y > cell.center().y + cell.height() * 0.5,
        "the tile did not come down to the row: {cell:?} → {moved:?}"
    );
    let before = drawn.get();
    h.tap(moved.center());
    h.frames(40);
    let after = drawn.get();
    h.frames(5);
    assert_eq!(
        drawn.get(),
        after,
        "it is closed and the body is still being drawn"
    );
    assert!(
        after > before,
        "it is drawn while the closing animation runs"
    );
    assert!(
        h.shell.overlay().expanded_rect().is_none(),
        "it is closed and the row keeps taking the drag"
    );
    Ok(())
}

/// The height of one gauge row — one finger (the same rule as `draw_gauges_row`).
fn gauge_row_y(h: &Harness, row: usize) -> f32 {
    #[expect(clippy::cast_precision_loss, reason = "the row count is small")]
    let n = row as f32;
    h.shell.theme().metrics.touch_target * (n + 0.5)
}

/// **Gauge rows the crate draws from a declaration alone.**
///
/// The row count, each row's name, colour and unit and whether it can be worked are written down as a declaration,
/// and the finger height, the press, the track thickness and the value display come from the same rule as the built-in slider.
#[test]
fn a_declared_gauge_row_is_drawn_and_written_by_the_crate() -> fairing::Result<()> {
    use fairing::overlay::{tile, Gauge, TileKind};
    use fairing::settings::SettingKey;

    let mut config = single_level_access();
    config.overlay.tiles = vec!["tile.wifi".to_owned(), "tile.hopper".to_owned()];
    let mut h = Harness::new(config, fairing::services::mock::services())?;
    h.shell.add(
        tile(
            "tile.hopper",
            TileKind::Gauges {
                rows: vec![
                    Gauge::new("hopper.a", "Resin A"),
                    Gauge::new("hopper.b", "Resin B").color(fairing::ColorRole::Warning),
                    Gauge::new("hopper.c", "Solvent").read_only(true),
                ],
                label_width: 96.0,
            },
        )
        .label("Hoppers"),
    );
    for key in ["hopper.a", "hopper.b", "hopper.c"] {
        h.shell
            .set_setting(SettingKey::from(key), SettingValue::Int(20));
    }
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);

    let cell = need(h.shell.overlay().tile_rect("tile.hopper"), "tile.hopper")?;
    h.tap(cell.center());
    h.frames(40);

    // The row count decides the height — the three declared get as much space as three fingers.
    let body = need(h.shell.overlay().expanded_rect(), "the expanded row")?;
    let row_h = h.shell.theme().metrics.touch_target;
    assert!(
        (body.height() - row_h * 3.0).abs() < 1.0,
        "three rows and the height is {} (one row is {row_h})",
        body.height()
    );

    // Drag the second row's track all the way right → only that row's key changes.
    let y = body.min.y + gauge_row_y(&h, 1);
    let _ = h.shell.poll_events();
    let from = egui::pos2(body.min.x + body.width() * 0.6, y);
    let end = pull(&mut h, from, egui::vec2(20.0, 0.0), 12);
    h.release(end);
    h.frames(4);
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::SettingChanged { key, .. } if key.0.as_ref() == "hopper.b"
        )),
        "the second row was not written: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            ShellEvent::SettingChanged { key, .. } if key.0.as_ref() == "hopper.a"
        )),
        "another row was written too: {events:?}"
    );

    // A read-only row is not written by dragging it.
    let y = body.min.y + gauge_row_y(&h, 2);
    let _ = h.shell.poll_events();
    let from = egui::pos2(body.min.x + body.width() * 0.6, y);
    let end = pull(&mut h, from, egui::vec2(20.0, 0.0), 12);
    h.release(end);
    h.frames(4);
    let events = h.shell.poll_events();
    assert!(
        !events.iter().any(|e| matches!(
            e,
            ShellEvent::SettingChanged { key, .. } if key.0.as_ref() == "hopper.c"
        )),
        "a read-only row was written: {events:?}"
    );
    Ok(())
}

/// Every string drawn this frame, so a test can ask what reached the screen.
fn drawn_text(h: &mut Harness) -> String {
    fn walk(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// How many footer icon buttons were hit-testable this frame, by their fixed ids.
fn footer_buttons(h: &Harness) -> Vec<&'static str> {
    [
        "fairing.overlay.footer.lock",
        "fairing.overlay.footer.settings",
    ]
    .into_iter()
    .filter(|id| {
        h.ctx
            .read_response(egui::Id::new(*id))
            .is_some_and(|r| r.rect.is_positive())
    })
    .collect()
}

/// **The footer buttons are the device's to choose.**
///
/// They used to be drawn unconditionally, so a device with no lock concept — or one that never
/// registered `settings.home`, which is what the settings button opens — carried buttons it could
/// not remove. Only "clear all" was conditional, and then on whether there was anything to clear.
///
/// The subject's name and the level dot stay whatever the list says: those are state, not controls.
#[test]
fn the_footer_buttons_follow_the_config() -> fairing::Result<()> {
    // The default keeps what shipped before.
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?
        .with_size(1024.0, 568.0);
    h.frames(2);
    open_shade(&mut h)?;
    assert_eq!(
        footer_buttons(&h),
        [
            "fairing.overlay.footer.lock",
            "fairing.overlay.footer.settings"
        ],
        "the default footer lost a button"
    );

    // An empty list draws none of them.
    let mut config = single_level_access();
    config.overlay.footer = Vec::new();
    let mut bare =
        Harness::new(config, fairing::services::mock::services())?.with_size(1024.0, 568.0);
    bare.frames(2);
    open_shade(&mut bare)?;
    assert!(
        footer_buttons(&bare).is_empty(),
        "`footer = []` still drew {:?}",
        footer_buttons(&bare)
    );
    // The name and the dot are state, not controls — they stay.
    let subject = bare.shell.access().session().subject.id.clone();
    let text = drawn_text(&mut bare);
    if let Some(name) = subject {
        assert!(
            text.contains(&name),
            "the subject `{name}` went with the buttons: {text:?}"
        );
    }

    // And one at a time.
    let mut config = single_level_access();
    config.overlay.footer = vec!["settings".to_owned()];
    let mut one =
        Harness::new(config, fairing::services::mock::services())?.with_size(1024.0, 568.0);
    one.frames(2);
    open_shade(&mut one)?;
    assert_eq!(
        footer_buttons(&one),
        ["fairing.overlay.footer.settings"],
        "`footer = [\"settings\"]` did not leave the settings button alone"
    );
    Ok(())
}

/// **The shade's lock button reports**.
///
/// `[overlay] footer` made the button the device's to remove. It was still a button that did
/// nothing when pressed: `Shell::launch` wrote `LaunchAction::Lock` to the log and dropped it, so an integrator
/// who kept the button had no way to give it a meaning either. Now the shell draws it, closes the
/// shade so whatever happens next is visible, and hands the press over.
#[test]
fn the_shade_lock_button_reports_and_closes_the_shade() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), fairing::services::mock::services())?
        .with_size(1024.0, 568.0);
    h.frames(2);
    open_shade(&mut h)?;
    let _ = h.shell.poll_events();

    let lock = need(
        h.ctx
            .read_response(egui::Id::new("fairing.overlay.footer.lock"))
            .map(|r| r.rect),
        "the lock button",
    )?;
    h.tap(lock.center());
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events.contains(&ShellEvent::LockRequested),
        "pressing the lock button has to reach the integrator, got {events:?}"
    );
    settle(&mut h, 90, |h| !h.shell.overlay().is_open())?;
    Ok(())
}

/// The `tile.lock` quick tile takes the same road as the footer button.
#[test]
fn the_lock_tile_reports_too() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.overlay.tiles = vec!["tile.lock".to_owned()];
    let mut h = Harness::new(config, fairing::services::mock::services())?.with_size(1024.0, 568.0);
    h.frames(2);
    open_shade(&mut h)?;
    let _ = h.shell.poll_events();

    let tile = need(h.shell.overlay().tile_rect("tile.lock"), "the lock tile")?;
    h.tap(tile.center());
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events.contains(&ShellEvent::LockRequested),
        "the tile reports as well, got {events:?}"
    );
    Ok(())
}

/// **A tile that opens a screen can still be lit**.
///
/// `TileKind::Action` had no on/off of its own — a tile that opens a screen is not a switch — so
/// the crate drew it in the off colour and there was nowhere for the device to say otherwise. A
/// conveyor that is running, a heater at temperature, a door that is open: the device knows and the
/// crate cannot. `lit_by` names the setting the answer lives in; faking it with a `Toggle` would
/// turn a tile that opens a screen into a switch.
#[test]
fn an_action_tile_lights_from_the_setting_it_names() -> fairing::Result<()> {
    use fairing::overlay::{tile, TileKind, TileState};
    use fairing::settings::SettingValue;

    let mut config = single_level_access();
    config.overlay.tiles = vec!["tile.conveyor".to_owned(), "tile.plain".to_owned()];
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = fairing::Shell::builder(config)
            .services(fairing::services::mock::services())
            .build(ctx)?;
        shell.add(
            tile(
                "tile.conveyor",
                TileKind::Action(LaunchAction::open("app.conveyor")),
            )
            .label("Conveyor")
            .lit_by("app.conveyor.running"),
        );
        shell.add(
            tile(
                "tile.plain",
                TileKind::Action(LaunchAction::open("nowhere")),
            )
            .label("Plain"),
        );
        Ok(shell)
    })?
    .with_size(1024.0, 568.0);
    h.frames(2);
    open_shade(&mut h)?;

    assert_eq!(
        h.shell.overlay().tile_state("tile.conveyor"),
        Some(TileState::Off),
        "nothing written yet, so it is off"
    );

    h.shell
        .set_setting("app.conveyor.running".into(), SettingValue::Bool(true));
    h.frames(2);
    assert_eq!(
        h.shell.overlay().tile_state("tile.conveyor"),
        Some(TileState::On),
        "the device said the conveyor is running, so the tile is lit"
    );
    assert_eq!(
        h.shell.overlay().tile_state("tile.plain"),
        Some(TileState::Off),
        "an Action tile with no `lit_by` is unchanged — off, as it always was"
    );

    h.shell
        .set_setting("app.conveyor.running".into(), SettingValue::Bool(false));
    h.frames(2);
    assert_eq!(
        h.shell.overlay().tile_state("tile.conveyor"),
        Some(TileState::Off),
        "and back off again"
    );
    Ok(())
}

/// **A quick tile's name is cut with an ellipsis, not clipped at both ends**.
///
/// The labels are laid out at the front of the frame and the tile row is drawn later, so the two
/// used different widths: `layout_no_wrap` made a galley as wide as the text, the tile centred it
/// and clipped, and "Bluetooth" reached the screen as "luetoot" — a letter gone from *each* end,
/// which reads as a rendering fault rather than as a name too long for its tile.
///
/// The galley is the thing to measure. A painted text shape wider than the tile it belongs to is
/// exactly the defect, whatever it happens to say.
#[test]
fn a_tile_label_is_never_wider_than_its_tile() -> fairing::Result<()> {
    let mut h = a1_harness()?;
    open_shade(&mut h)?;

    // A tile label is the only text in the panel laid out against a width, so a galley whose job
    // carries a finite `max_width` **is** one — no guessing from position or font size.
    let mut bounded = 0_u32;
    let over = h.frame_shapes().into_iter().find_map(|c| match c.shape {
        egui::Shape::Text(t) if t.galley.job.wrap.max_width.is_finite() => {
            bounded += 1;
            (t.galley.rect.width() > t.galley.job.wrap.max_width + 1.0)
                .then(|| (t.galley.rect.width(), t.galley.job.wrap.max_width))
        }
        _ => None,
    });

    assert!(
        bounded > 0,
        "no text was laid out against a width at all — the tile labels stopped being bounded, \
         which is the state the overflow came from"
    );
    assert!(
        over.is_none(),
        "a label is {over:?} — wider than the width it was laid out against, so it is centred \
         and clipped at both ends rather than cut with an ellipsis"
    );
    Ok(())
}
