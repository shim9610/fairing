//! The M2 integration test for the gesture back, the page swipe, `edge_guard` and the emergency, `keep_awake`
//! and the clear-top crossfade (A3 · A4).
//!
//! The rules are the M1 transition test's: to see the animations, a config with `motion.reduce = false` is handed to
//! `Harness::new`, and layer offsets are read through `ctx.layer_transform_to_global(instance.layer_id())`
//! **inside a drawn frame**. Everything is `fn … -> fairing::Result<()>`.
//!
//! The synthetic finger: egui's `pointer.velocity()` is a regression over the last 100 ms window, so at 60 Hz
//! `step px/frame ≈ step × 60 px/s`. "Letting go at a velocity of 0" is made by dragging to the target, then
//! stopping for longer than the window (6 frames) before letting go ([`still`]).

use fairing::services::mock::MockDisplay;
use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::workspace::Instance;
use fairing::{
    screen, screen_with, Cx, Error, LaunchAction, LaunchMode, Lifecycle, Screen, Services, Shell,
    ShellEvent,
};
// The policy-guard tests look at the shade's state.
#[cfg(feature = "overlay")]
use fairing::{testing::access_config, AccessEvent, ChromePolicy};
use std::cell::RefCell;
use std::rc::Rc;

/// The screen width (the reference board's).
const W: f32 = 1024.0;

/// A `reduce = false` shell (with the Null clock) — for the tests that check the animations.
fn animated(setup: impl FnOnce(&mut Shell)) -> fairing::Result<Harness> {
    let services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    setup(&mut h.shell);
    Ok(h)
}

fn require<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| Error::Config(format!("there is no {what}")))
}

/// The instance layer's global x offset as of this frame.
fn layer_x(h: &Harness, decl_id: &str) -> Option<f32> {
    let layer = h.shell.workspace().find(decl_id).map(Instance::layer_id)?;
    h.ctx
        .layer_transform_to_global(layer)
        .map(|t| t.translation.x)
}

/// Until the transition is over (3 s at most).
fn run_until_idle(h: &mut Harness) -> fairing::Result<()> {
    for _ in 0..180 {
        h.frame();
        if !h.shell.is_animating() {
            return Ok(());
        }
    }
    Err(Error::Config(
        "the transition did not end inside 3 s".to_owned(),
    ))
}

/// Held down, `frames` frames in place (emptying the velocity window). It does not let go.
fn still(h: &mut Harness, pos: egui::Pos2, frames: usize) {
    for _ in 0..frames {
        h.move_to(pos);
        h.frame();
    }
}

/// From where it is, it drags for `frames` frames at `step` px/frame (never letting go). The last position.
fn drag_by(h: &mut Harness, from: egui::Pos2, step: egui::Vec2, frames: usize) -> egui::Pos2 {
    let mut pos = from;
    for _ in 0..frames {
        pos += step;
        h.move_to(pos);
        h.frame();
    }
    pos
}

/// A `(declaration id, lifecycle)` log.
type Log = Rc<RefCell<Vec<(String, Lifecycle)>>>;

/// A screen that writes each `on_lifecycle` down with its own id.
struct Recorder {
    id: String,
    log: Log,
}

impl Recorder {
    fn new(id: &str, log: &Log) -> Self {
        Self {
            id: id.to_owned(),
            log: Rc::clone(log),
        }
    }
}

impl Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("rec");
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push((self.id.clone(), event));
    }
}

// ── A3 · the gesture back ────────────────────────────────────────────────────

/// A3: letting go at the left edge with `dx = 0.2W, v = 900` confirms and `v = 0` cancels.
/// Mid-gesture the top layer's x is 1:1 with the finger.
#[test]
fn gesture_back_confirms_by_velocity_and_cancels_by_distance() -> fairing::Result<()> {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&log);
    let mut h = animated(move |sh| {
        sh.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("a");
        }));
        sh.add(screen_with("b", move || Recorder::new("b", &sink)));
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    run_until_idle(&mut h)?;
    h.shell.handle().launch(LaunchAction::open("b"));
    run_until_idle(&mut h)?;
    let _ = h.shell.poll_events();
    log.borrow_mut().clear();

    // ① Confirming: 14 frames × 15 px = 210 px ≈ 0.2 W, at a velocity of 900 px/s.
    let from = egui::pos2(4.0, 300.0);
    h.press(from);
    h.frame();
    let end = drag_by(&mut h, from, egui::vec2(15.0, 0.0), 14);
    let dx = end.x - from.x;
    assert!((dx - 0.2 * W).abs() < 8.0, "dx = {dx}");
    let x = require(layer_x(&h, "b"), "b's layer")?;
    assert!(
        (x - dx).abs() <= 1.0,
        "1:1 with the finger: x = {x}, dx = {dx}"
    );
    assert!(
        log.borrow().iter().any(|(_, e)| *e == Lifecycle::Paused),
        "the outgoing side is Paused at the start"
    );
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert!(h.shell.workspace().find("b").is_none(), "confirmed → pop");
    assert!(
        h.shell
            .poll_events()
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "b")),
        "ScreenClosed"
    );
    assert_eq!(
        log.borrow().last().map(|(_, e)| *e),
        Some(Lifecycle::Destroyed)
    );

    // ② Cancelling: the same distance, but stopping before the release takes the velocity to 0.
    h.shell.handle().launch(LaunchAction::open("b"));
    run_until_idle(&mut h)?;
    let _ = h.shell.poll_events();
    log.borrow_mut().clear();
    h.press(from);
    h.frame();
    let end = drag_by(&mut h, from, egui::vec2(15.0, 0.0), 14);
    still(&mut h, end, 12);
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert!(
        h.shell.workspace().find("b").is_some(),
        "cancelled → as it was"
    );
    let events = log.borrow().clone();
    assert_eq!(
        events.last().map(|(_, e)| *e),
        Some(Lifecycle::Resumed),
        "a cancel restores the outgoing side with Resumed: {events:?}"
    );
    // Once settled, egui removes the IDENTITY transform — absent means 0.
    assert!(
        layer_x(&h, "b").unwrap_or(0.0).abs() < 1.0,
        "it comes back into place"
    );
    Ok(())
}

// ── A4 · the desktop page swipe ──────────────────────────────────────────────

/// Three pages with one icon each (`columns = rows = 1`).
fn three_pages() -> fairing::Result<Harness> {
    let mut cfg = single_level_access();
    cfg.desktop.columns = 1;
    cfg.desktop.rows = 1;
    let services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(cfg, services)?;
    for id in ["p0", "p1", "p2"] {
        h.shell.add(
            screen(id, |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("page");
            })
            .title(id)
            .icon(fairing::icon::FOLDER)
            .desktop(),
        );
    }
    h.frames(3);
    Ok(h)
}

/// A4: N = 3, W = 1024. The release rule, the rubber band, the indicator weights and no icon tap
/// mid-drag.
#[test]
fn page_swipe_release_rules() -> fairing::Result<()> {
    let mut h = three_pages()?;
    assert_eq!(h.shell.desktop().pages().len(), 3);
    assert_eq!(h.shell.desktop().page(), 0);
    let start = egui::pos2(W / 2.0, 200.0);

    // ① dx = −300, v = −900 → 1 (confirmed by velocity).
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(-15.0, 0.0), 20);
    assert!(
        h.shell.desktop().swipe().is_dragging(),
        "crossing the sideways slop drives the page"
    );
    let pos = h.shell.desktop().page_pos();
    assert!((pos - 300.0 / W).abs() < 1e-2, "1:1 tracking: {pos}");
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert_eq!(h.shell.desktop().page(), 1);

    // ② The same distance from page 0 but at velocity 0 → rounding leaves it where it was.
    h.shell.desktop_mut().set_page(0);
    h.frames(2);
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(-15.0, 0.0), 20);
    still(&mut h, end, 12);
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert_eq!(h.shell.desktop().page(), 0, "page 0.293 rounds to 0");

    // ③ Dragging further at the last page rubber-bands: dx = −400 → pos = 2.117, and letting go gives 2.
    h.shell.desktop_mut().set_page(2);
    h.frames(2);
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(-20.0, 0.0), 20);
    let pos = h.shell.desktop().page_pos();
    assert!(
        (pos - 2.117).abs() < 1e-2,
        "a rubber band of 0.3 with a ceiling of 0.15: {pos}"
    );
    // The indicators crossfade — the rubber band takes it a little past 2, so dot 2 dims slightly.
    let swipe = h.shell.desktop().swipe();
    assert!(
        (swipe.dot_weight(2) - 0.883).abs() < 2e-2,
        "{}",
        swipe.dot_weight(2)
    );
    assert!(swipe.dot_weight(1).abs() < 1e-6);
    still(&mut h, end, 12);
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert_eq!(h.shell.desktop().page(), 2);
    assert!((h.shell.desktop().page_pos() - 2.0).abs() < 1e-3);

    // ④ In between, two dots are half lit each (only two pages are drawn).
    h.shell.desktop_mut().set_page(0);
    h.frames(2);
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(-W / 20.0, 0.0), 10);
    let swipe = h.shell.desktop().swipe();
    assert!(
        (swipe.dot_weight(0) - 0.5).abs() < 5e-2,
        "{}",
        swipe.dot_weight(0)
    );
    assert!(
        (swipe.dot_weight(1) - 0.5).abs() < 5e-2,
        "{}",
        swipe.dot_weight(1)
    );
    let [(lo, _), (hi, _)] = swipe.visible();
    assert_eq!((lo, hi), (0, 1), "two pages only");
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;

    Ok(())
}

/// A4's "no icon tap mid-drag": a sideways drag started on an icon does not open a screen.
/// Dragging vertically does not move the page either (the sideways > vertical rule).
#[test]
fn page_drag_cancels_the_icon_press() -> fairing::Result<()> {
    let mut h = three_pages()?;
    let icon = require(h.shell.desktop().icon_rect("p0"), "the p0 icon")?;
    let from = icon.center();

    h.press(from);
    h.frame();
    assert!(
        h.shell.desktop().is_pressed(),
        "pressed at once on the pressing frame"
    );
    let end = drag_by(&mut h, from, egui::vec2(-15.0, 0.0), 20);
    assert!(h.shell.desktop().swipe().is_dragging());
    assert!(
        !h.shell.desktop().is_pressed(),
        "crossing the slop cancels the press"
    );
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert!(
        h.shell.workspace().is_home(),
        "a press that ended as a drag opens no screen"
    );
    assert_eq!(h.shell.desktop().page(), 1);

    // A vertical drag does not drive the page.
    h.shell.desktop_mut().set_page(0);
    h.frames(2);
    let from = require(h.shell.desktop().icon_rect("p0"), "the p0 icon")?.center();
    h.press(from);
    h.frame();
    let end = drag_by(&mut h, from, egui::vec2(-4.0, 15.0), 10);
    assert!(
        !h.shell.desktop().swipe().is_dragging(),
        "a bigger vertical is ignored"
    );
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;
    assert_eq!(h.shell.desktop().page(), 0);
    Ok(())
}

/// An indicator tap moves it with a spring, and `icon_rect` goes by the **settled page** even mid-swipe
/// (so the A2 starting point does not wobble).
#[test]
fn indicator_tap_springs_and_icon_rect_stays_on_the_settled_page() -> fairing::Result<()> {
    let mut h = three_pages()?;
    let settled = require(h.shell.desktop().icon_rect("p0"), "the p0 icon")?;

    let start = egui::pos2(W / 2.0, 200.0);
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(-15.0, 0.0), 10);
    let during = require(h.shell.desktop().icon_rect("p0"), "the p0 icon")?;
    assert!(
        (during.min.x - settled.min.x).abs() < 1.0,
        "in place even mid-swipe: {} vs {}",
        during.min.x,
        settled.min.x
    );
    h.release(end);
    h.frame();
    run_until_idle(&mut h)?;

    let dot = require(h.shell.desktop().page_indicator_rect(2), "indicator 2")?;
    h.tap(dot.center());
    run_until_idle(&mut h)?;
    assert_eq!(h.shell.desktop().page(), 2);
    Ok(())
}

// ── the policy guard ─────────────────────────────────────────────────────────

/// On an `edge_guard` screen a top or left pull is ignored and only a 2 s hold on the top
/// corner is left. Passing the gate opens the shade, and failing it gives `UnlockRequested`.
///
/// It looks at the shade's state, so only where the `overlay` feature is on.
#[cfg(feature = "overlay")]
#[test]
fn edge_guard_emergency_hold_two_seconds() -> fairing::Result<()> {
    // One level — `chrome.emergency` passes → the shade opens.
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("kiosk", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("kiosk");
            })
            .chrome(ChromePolicy {
                edge_guard: true,
                ..ChromePolicy::default()
            }),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("kiosk"));
    h.frames(3);
    let _ = h.shell.poll_events();

    // A top pull: the engine does not even start.
    h.drag(egui::pos2(10.0, 8.0), egui::pos2(10.0, 300.0), 12);
    h.frames(2);
    assert!(
        h.shell.overlay().is_closed(),
        "edge_guard refuses the shade"
    );
    // A left pull: no gesture back either.
    h.drag(egui::pos2(4.0, 300.0), egui::pos2(300.0, 300.0), 12);
    h.frames(2);
    assert!(h.shell.workspace().find("kiosk").is_some());
    let _ = h.shell.poll_events();

    // A 2 s hold on the top right corner → the emergency gesture.
    h.hold(egui::pos2(1010.0, 10.0), 121);
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(e, ShellEvent::Emergency)),
        "ShellEvent::Emergency: {events:?}"
    );
    assert!(
        h.shell.policy_driver().emergency_progress() >= 1.0,
        "the progress ring fills"
    );
    h.release(egui::pos2(1010.0, 10.0));
    h.frames(2);
    assert!(h.shell.overlay().is_open(), "passing opens the shade");

    // Two levels — failing gives `UnlockRequested { chrome.emergency, then: OpenOverlay }`.
    let mut h = test_shell(access_config(&["viewer", "admin"], Some("top")), |sh| {
        sh.add(
            screen("kiosk", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("kiosk");
            })
            .chrome(ChromePolicy {
                edge_guard: true,
                ..ChromePolicy::default()
            }),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("kiosk"));
    h.frames(3);
    let _ = h.shell.poll_events();
    h.hold(egui::pos2(10.0, 10.0), 121);
    let events = h.shell.poll_events();
    assert!(events.iter().any(|e| matches!(e, ShellEvent::Emergency)));
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::Access(AccessEvent::UnlockRequested { gate, then })
                if gate.as_str() == "chrome.emergency"
                    && matches!(then, Some(LaunchAction::OpenOverlay))
        )),
        "UnlockRequested: {events:?}"
    );
    assert!(h.shell.overlay().is_closed());
    Ok(())
}

/// `allow_peek = false` plus a hidden status bar refuses the top edge. A visible bar does not refuse it.
#[cfg(feature = "overlay")]
#[test]
fn allow_peek_false_blocks_the_hidden_bar_pull() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("locked", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("locked");
            })
            .chrome(ChromePolicy {
                status_bar: fairing::screen::BarMode::Hide,
                allow_peek: false,
                ..ChromePolicy::default()
            }),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("locked"));
    h.frames(3);
    h.drag(egui::pos2(10.0, 4.0), egui::pos2(10.0, 300.0), 12);
    h.frames(2);
    assert!(
        h.shell.overlay().is_closed(),
        "a hidden bar plus no peek = refused"
    );
    assert!(
        h.shell.gestures().active_edge().is_none(),
        "the engine does not start the swipe"
    );
    Ok(())
}

/// The skeleton smoke test: opening a `keep_awake` screen gives `DisplayBackend::set_idle_inhibit(true)`
/// and closing it gives false.
#[test]
fn keep_awake_screen_drives_idle_inhibit() -> fairing::Result<()> {
    let mut h = Harness::new(
        {
            let mut c = single_level_access();
            c.motion.reduce = true;
            c
        },
        Services::builder()
            .clock(fairing::services::null::NullClock)
            .display(MockDisplay::default())
            .build(),
    )?;
    h.shell.add(
        screen("awake", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("awake");
        })
        .keep_awake(),
    );
    h.frames(2);
    assert_eq!(h.shell.services().display.idle_inhibited(), Some(false));
    assert_eq!(h.shell.policy_driver().keep_awake(), Some(false));
    h.shell.handle().launch(LaunchAction::open("awake"));
    h.frames(2);
    assert_eq!(h.shell.services().display.idle_inhibited(), Some(true));
    assert_eq!(h.shell.policy_driver().keep_awake(), Some(true));
    h.shell.handle().home();
    h.frames(3);
    assert_eq!(h.shell.services().display.idle_inhibited(), Some(false));
    let _ = test_shell(single_level_access(), |_| {})?;
    Ok(())
}

// ── the clear-top crossfade ──────────────────────────────────────────────────

/// From a → b → c, opening a again through `Single` reuse has the screens being cleared
/// go over 160 ms of `CubicOut`. With `reduce` it is instant.
#[test]
fn clear_top_reuse_crossfades_160ms() -> fairing::Result<()> {
    let log: Log = Rc::new(RefCell::new(Vec::new()));

    let make = |h: &mut Harness, log: &Log| {
        for id in ["a", "b", "c"] {
            let (name, sink) = (id.to_owned(), Rc::clone(log));
            h.shell.add(
                screen_with(id, move || Recorder::new(&name, &sink)).launch(LaunchMode::Single),
            );
        }
    };

    let mut h = animated(|_| {})?;
    make(&mut h, &log);
    for id in ["a", "b", "c"] {
        h.shell.handle().launch(LaunchAction::open(id));
        run_until_idle(&mut h)?;
    }
    let _ = h.shell.poll_events();
    log.borrow_mut().clear();

    h.shell.handle().launch(LaunchAction::open("a"));
    h.frame();
    assert!(h.shell.workspace().is_clearing(), "the crossfade starts");
    assert_eq!(h.shell.workspace().clearing(), 2, "b and c are cleared");
    assert!(
        h.shell.workspace().find("b").is_none(),
        "the stack shrinks straight away"
    );
    let closed = h.shell.poll_events();
    assert_eq!(
        closed
            .iter()
            .filter(|e| matches!(e, ShellEvent::ScreenClosed { .. }))
            .count(),
        2
    );

    // Around 80 ms: the alpha is between the two ends.
    h.frames(4);
    let alpha = require(h.shell.workspace().clear_top_alpha(), "the alpha")?;
    assert!(alpha > 0.0 && alpha < 1.0, "a mid alpha: {alpha}");
    // 160 ms later: the end plus Destroyed.
    run_until_idle(&mut h)?;
    assert!(!h.shell.workspace().is_clearing());
    let events = log.borrow().clone();
    for id in ["b", "c"] {
        assert!(
            events
                .iter()
                .any(|(name, e)| name == id && *e == Lifecycle::Destroyed),
            "{id} Destroyed: {events:?}"
        );
    }
    assert!(
        events
            .iter()
            .any(|(name, e)| name == "a" && *e == Lifecycle::Resumed),
        "a Resumed"
    );
    assert_eq!(h.shell.workspace().tasks().len(), 1);

    // With reduce it is instant.
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    let services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(cfg, services)?;
    make(&mut h, &log);
    log.borrow_mut().clear();
    for id in ["a", "b", "c"] {
        h.shell.handle().launch(LaunchAction::open(id));
        h.frames(2);
    }
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frame();
    assert!(!h.shell.workspace().is_clearing(), "reduce is instant");
    assert!(!h.shell.is_animating());
    Ok(())
}
