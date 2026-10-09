//! **Gesture handles** — thin strips at the edge of the glass,
//! swiped the way One Hand Operation+ handles are: straight in or diagonally, let go at once or
//! after a rest, each way bound to a `LaunchAction`. On the left, the right and the bottom edges.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::access::AccessEvent;
use fairing::gesture::{
    Edge, GestureHandle, HandleAction, HandleDirection, HandleEnd, HandleGesture, HandleLook,
};
use fairing::screen::ChromePolicy;
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{screen, Cx, LaunchAction, ShellConfig, ShellEvent};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// The smallest screen, drawing one label.
fn stub(id: &'static str) -> fairing::screen::ScreenDecl {
    screen(id, move |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label(id);
    })
}

/// A shell on `config` with the screens these tests open, a frame or two in.
fn shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = test_shell(config, |sh| {
        for id in ["batches", "service_menu", "a", "b", "up", "down"] {
            sh.add(stub(id));
        }
        sh.add(stub("kiosk").chrome(ChromePolicy {
            edge_guard: true,
            ..ChromePolicy::default()
        }));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events();
    Ok(h)
}

/// The declaration id on show.
fn focused(h: &Harness) -> Option<&str> {
    h.shell
        .workspace()
        .focused()
        .map(fairing::workspace::Instance::decl_id)
}

/// The gestures that said they completed since the last look, as (handle, gesture).
fn ran(h: &mut Harness) -> Vec<(String, HandleGesture)> {
    h.shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::Gesture { handle, gesture } => Some((handle, gesture)),
            _ => None,
        })
        .collect()
}

/// `mm` millimetres in points, at the shell's scale.
fn mm(h: &Harness, mm: f32) -> f32 {
    h.shell.scale().mm_to_du(mm)
}

/// The edge between the bars: its top and its bottom.
fn between_bars(h: &Harness) -> (f32, f32) {
    let layout = h.shell.layout();
    let screen = h.screen_rect();
    (
        layout.status.map_or(screen.min.y, |r| r.max.y),
        layout.nav.map_or(screen.max.y, |r| r.min.y),
    )
}

/// A point in the right handle's strip, `share` of the way down the edge between the bars.
fn right_strip(h: &Harness, share: f32) -> egui::Pos2 {
    let (top, bottom) = between_bars(h);
    egui::pos2(h.screen_rect().max.x - 1.0, top + (bottom - top) * share)
}

/// Straight in from the right edge.
const LEFTWARD: egui::Vec2 = egui::vec2(-1.0, 0.0);

/// A swipe from `from` by `delta`, over `frames` frames, let go at once.
fn swipe(h: &mut Harness, from: egui::Pos2, delta: egui::Vec2, frames: usize) {
    h.drag(from, from + delta, frames);
    h.frames(2);
}

/// A swipe from the right strip, `share` of the way down the edge between the bars.
fn swipe_in(h: &mut Harness, share: f32, delta: egui::Vec2, frames: usize) {
    let from = right_strip(h, share);
    swipe(h, from, delta, frames);
}

/// The right edge's handle: the batch list straight in.
fn right() -> GestureHandle {
    GestureHandle::new("right", Edge::Right).gesture(
        HandleGesture::short(HandleDirection::Straight),
        LaunchAction::open("batches"),
    )
}

/// **A swipe in from the strip runs its gesture**, and says so.
#[test]
fn a_swipe_in_from_the_strip_runs_its_gesture() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )]
    );
    assert_eq!(focused(&h), Some("batches"));
    Ok(())
}

/// **The way is the angle off straight in**: diagonally up and diagonally down are gestures of
/// their own.
#[test]
fn the_way_is_the_angle() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(
        right()
            .gesture(
                HandleGesture::short(HandleDirection::DiagonalUp),
                LaunchAction::open("up"),
            )
            .gesture(
                HandleGesture::short(HandleDirection::DiagonalDown),
                LaunchAction::open("down"),
            ),
    )?;
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, egui::vec2(-1.0, -1.0) * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::DiagonalUp)
        )]
    );
    assert_eq!(focused(&h), Some("up"));
    swipe_in(&mut h, 0.5, egui::vec2(-1.0, 1.0) * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::DiagonalDown)
        )]
    );
    assert_eq!(focused(&h), Some("down"));
    Ok(())
}

/// **A swipe short of the reach runs nothing**, and neither does one along the edge.
#[test]
fn a_swipe_short_of_the_reach_or_along_the_edge_runs_nothing() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 0.5, 4);
    assert!(ran(&mut h).is_empty(), "a swipe short of the reach ran");
    swipe_in(&mut h, 0.3, egui::vec2(-0.1, 1.0) * reach * 3.0, 8);
    assert!(ran(&mut h).is_empty(), "a stroke along the edge ran");
    // Along the edge past the slop, then in: the way is decided at the slop, and it was along.
    let from = right_strip(&h, 0.3);
    h.press(from);
    h.frame();
    h.move_to(from + egui::vec2(0.0, 30.0));
    h.frame();
    h.move_to(from + egui::vec2(-2.0 * reach, 30.0));
    h.frame();
    h.release(from + egui::vec2(-2.0 * reach, 30.0));
    h.frames(3);
    assert!(ran(&mut h).is_empty(), "a stroke that turned in ran");
    assert_ne!(focused(&h), Some("batches"));
    Ok(())
}

/// **A flick let go on the frame it passes the slop still runs**: the release is where
/// its way is decided, then.
#[test]
fn a_flick_let_go_as_it_passes_the_slop_still_runs() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    let reach = mm(&h, 10.0);
    let from = right_strip(&h, 0.5);
    h.press(from);
    h.frame();
    h.release(from + LEFTWARD * reach * 2.0);
    h.frames(3);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )]
    );
    assert_eq!(focused(&h), Some("batches"));
    Ok(())
}

/// The right handle with a long gesture straight in as well: the service menu.
fn right_with_long() -> GestureHandle {
    right().gesture(
        HandleGesture::long(HandleDirection::Straight),
        LaunchAction::open("service_menu"),
    )
}

/// **A swipe let go at once is the short gesture**, however long it took: a slow swipe that never
/// stops is no rest.
#[test]
fn a_swipe_that_never_stops_is_the_short_gesture() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right_with_long())?;
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )],
        "let go at once"
    );
    h.shell.home();
    h.frames(3);
    let _ = h.shell.poll_events();
    // Three quarters of a second on the way, past the reach for most of it, never still.
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 3.0, 45);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )],
        "a slow swipe that never stopped"
    );
    assert_eq!(focused(&h), Some("batches"));
    Ok(())
}

/// **A rest past the reach runs the long gesture there and then**, and the release runs nothing
/// more. Where the handle has no long gesture that way, the rest changes nothing and the release
/// runs the short one.
#[test]
fn a_rest_past_the_reach_runs_the_long_gesture() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right_with_long())?;
    let reach = mm(&h, 10.0);
    let from = right_strip(&h, 0.5);
    h.press(from);
    h.frame();
    for step in 1..=4u8 {
        h.move_to(from + LEFTWARD * reach * 0.5 * f32::from(step));
        h.frame();
    }
    h.frames(30);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::long(HandleDirection::Straight)
        )],
        "the rest did not run the long gesture"
    );
    assert_eq!(focused(&h), Some("service_menu"));
    h.release(from + LEFTWARD * reach * 2.0);
    h.frames(3);
    assert!(ran(&mut h).is_empty(), "the release ran the short one too");

    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    h.press(from);
    h.frame();
    for step in 1..=4u8 {
        h.move_to(from + LEFTWARD * reach * 0.5 * f32::from(step));
        h.frame();
    }
    h.frames(30);
    assert!(
        ran(&mut h).is_empty(),
        "a rest ran a long gesture there is none of"
    );
    h.release(from + LEFTWARD * reach * 2.0);
    h.frames(3);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )],
        "the release after the rest did not run the short one"
    );
    Ok(())
}

/// **Only the strip is the handle's.** A press just inward of it is not — on the left edge it is
/// back, which a handle there leaves the rest of the edge to — and neither is a press beside the
/// handle's stretch of the edge.
#[test]
fn only_the_strip_is_the_handles() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right().along(0.5, 1.0))?;
    let (reach, strip) = (mm(&h, 10.0), mm(&h, 2.0));
    let inward = right_strip(&h, 0.75) + LEFTWARD * (strip + 6.0);
    swipe(&mut h, inward, LEFTWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "a press inward of the strip ran it");
    swipe_in(&mut h, 0.25, LEFTWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "a press above its stretch ran it");

    // The left edge: the handle in its strip, back just inward of it.
    let mut h = shell(single_level_access())?;
    h.shell
        .add_gesture_handle(GestureHandle::new("left", Edge::Left).gesture(
            HandleGesture::short(HandleDirection::Straight),
            LaunchAction::open("batches"),
        ))?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    if focused(&h) != Some("b") {
        return Err(fail("`b` is not on show"));
    }
    let _ = h.shell.poll_events();
    let (top, bottom) = between_bars(&h);
    let mid = top.midpoint(bottom);
    let rightward = egui::vec2(1.0, 0.0);
    let half = h.screen_rect().width() * 0.5;
    swipe(&mut h, egui::pos2(strip + 6.0, mid), rightward * half, 30);
    h.frames(10);
    assert!(ran(&mut h).is_empty(), "a press inward of the strip ran it");
    assert_eq!(focused(&h), Some("a"), "back did not go back");
    swipe(&mut h, egui::pos2(1.0, mid), rightward * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "left".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )],
        "the strip did not answer"
    );
    assert_eq!(focused(&h), Some("batches"));
    Ok(())
}

/// **Three handles an edge, their stretches apart, on the left, the right and — with the nav bar
/// off — the bottom edges, each swiped its own edge's ways** — and a handle with an id already
/// there replaces the old one.
#[test]
fn handles_are_checked_when_added() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let shelf = |id: &str, from: f32, to: f32| GestureHandle::new(id, Edge::Right).along(from, to);
    assert!(h
        .shell
        .add_gesture_handle(GestureHandle::new("top", Edge::Top))
        .is_err());
    assert!(
        h.shell
            .add_gesture_handle(GestureHandle::new("side", Edge::Left).gesture(
                HandleGesture::long(HandleDirection::DiagonalRight),
                LaunchAction::open("up"),
            ))
            .is_err(),
        "right and in is no way from the left"
    );
    assert!(h.shell.add_gesture_handle(shelf("back", 0.6, 0.4)).is_err());
    h.shell.add_gesture_handle(shelf("one", 0.0, 0.3))?;
    h.shell.add_gesture_handle(shelf("two", 0.3, 0.6))?;
    assert!(
        h.shell.add_gesture_handle(shelf("over", 0.5, 0.7)).is_err(),
        "two handles overlap"
    );
    h.shell.add_gesture_handle(shelf("three", 0.6, 0.9))?;
    assert!(
        h.shell.add_gesture_handle(shelf("four", 0.9, 1.0)).is_err(),
        "a fourth on one edge"
    );
    h.shell.add_gesture_handle(shelf("two", 0.3, 0.5))?;
    h.shell
        .add_gesture_handle(GestureHandle::new("left", Edge::Left))?;
    let ids: Vec<&str> = h.shell.gesture_handles().collect();
    assert_eq!(ids, ["one", "two", "three", "left"]);

    let mut h = shell(no_nav())?;
    assert!(
        h.shell
            .add_gesture_handle(GestureHandle::new("bottom", Edge::Bottom).gesture(
                HandleGesture::short(HandleDirection::DiagonalUp),
                LaunchAction::open("up"),
            ))
            .is_err(),
        "up and in is no way from the bottom"
    );
    let floor = |id: &str, from: f32, to: f32| GestureHandle::new(id, Edge::Bottom).along(from, to);
    h.shell.add_gesture_handle(floor("b1", 0.0, 0.3))?;
    h.shell.add_gesture_handle(floor("b2", 0.3, 0.6))?;
    assert!(
        h.shell
            .add_gesture_handle(floor("b-over", 0.5, 0.7))
            .is_err(),
        "two handles overlap on the bottom"
    );
    h.shell.add_gesture_handle(floor("b3", 0.6, 1.0))?;
    assert!(
        h.shell
            .add_gesture_handle(GestureHandle::new("b4", Edge::Bottom).along(0.0, 0.1))
            .is_err(),
        "a fourth on the bottom"
    );
    let ids: Vec<&str> = h.shell.gesture_handles().collect();
    assert_eq!(ids, ["b1", "b2", "b3"]);
    Ok(())
}

/// The one-level config with the nav bar off, so the bottom edge is the handles'.
fn no_nav() -> ShellConfig {
    let mut config = single_level_access();
    config.nav_bar.enabled = false;
    config
}

/// A point in the bottom handle's strip, `share` of the way across the glass: its last two
/// points.
fn bottom_strip(h: &Harness, share: f32) -> egui::Pos2 {
    let glass = h.screen_rect();
    egui::pos2(glass.width() * share, glass.max.y - 2.0)
}

/// Straight up from the bottom edge.
const UPWARD: egui::Vec2 = egui::vec2(0.0, -1.0);

/// The bottom edge's handle: `up` straight up, `a` up and to the left, `b` up and to the right.
fn bottom() -> GestureHandle {
    GestureHandle::new("bottom", Edge::Bottom)
        .gesture(
            HandleGesture::short(HandleDirection::Straight),
            LaunchAction::open("up"),
        )
        .gesture(
            HandleGesture::short(HandleDirection::DiagonalLeft),
            LaunchAction::open("a"),
        )
        .gesture(
            HandleGesture::short(HandleDirection::DiagonalRight),
            LaunchAction::open("b"),
        )
}

/// **A bottom handle answers straight up and up either side**, from its strip at the
/// glass's bottom, over the desktop and over a screen alike.
#[test]
fn a_bottom_handle_answers_up_and_either_side() -> fairing::Result<()> {
    let mut h = shell(no_nav())?;
    h.shell.add_gesture_handle(bottom())?;
    let reach = mm(&h, 10.0);
    for (way, to, delta) in [
        (HandleDirection::Straight, "up", UPWARD),
        (HandleDirection::DiagonalLeft, "a", egui::vec2(-1.0, -1.0)),
        (HandleDirection::DiagonalRight, "b", egui::vec2(1.0, -1.0)),
    ] {
        let from = bottom_strip(&h, 0.5);
        swipe(&mut h, from, delta * reach * 2.0, 8);
        assert_eq!(
            ran(&mut h),
            [("bottom".to_owned(), HandleGesture::short(way))],
            "{way:?}"
        );
        assert_eq!(focused(&h), Some(to), "{way:?}");
    }
    // A stroke along the bottom, and one down off the glass, are nobody's.
    let (left, middle) = (bottom_strip(&h, 0.3), bottom_strip(&h, 0.5));
    swipe(&mut h, left, egui::vec2(1.0, 0.0) * reach * 3.0, 8);
    swipe(&mut h, middle, egui::vec2(0.0, 1.0) * reach, 8);
    assert!(ran(&mut h).is_empty(), "along or down ran a gesture");
    assert_eq!(focused(&h), Some("b"));
    // On the bottom, `along` is a stretch of the width between its ends: the right half here.
    h.shell.add_gesture_handle(bottom().along(0.5, 1.0))?;
    h.frames(2);
    let (left, right) = (bottom_strip(&h, 0.25), bottom_strip(&h, 0.75));
    swipe(&mut h, left, UPWARD * reach * 2.0, 8);
    assert!(
        ran(&mut h).is_empty(),
        "the left half is no longer the strip"
    );
    swipe(&mut h, right, UPWARD * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "bottom".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )]
    );
    Ok(())
}

/// **The bottom edge is the nav bar's or the handles'**: with the nav bar on — buttons
/// or gestures — a bottom handle is refused; with it off, the handle goes on and answers; and with
/// the nav bar turned on again while the handle is there, the bar has the edge and the handle
/// stands aside.
#[test]
fn the_bottom_edge_is_the_nav_bars_or_the_handles() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    assert!(
        h.shell.add_gesture_handle(bottom()).is_err(),
        "a bottom handle beside the nav bar's buttons"
    );
    let mut config = single_level_access();
    "gesture".clone_into(&mut config.nav_bar.style);
    let mut h = shell(config)?;
    assert!(
        h.shell.add_gesture_handle(bottom()).is_err(),
        "a bottom handle beside the nav bar's gestures"
    );
    assert_eq!(h.shell.gesture_handles().count(), 0);

    let mut h = shell(no_nav())?;
    h.shell.add_gesture_handle(bottom())?;
    let reach = mm(&h, 10.0);
    let from = bottom_strip(&h, 0.5);
    swipe(&mut h, from, UPWARD * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "bottom".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )]
    );
    h.shell.nav_bar_mut().enabled = true;
    h.frames(3);
    if h.shell.layout().nav.is_none() {
        return Err(fail("the nav bar did not come back"));
    }
    let _ = h.shell.poll_events();
    swipe(&mut h, from, UPWARD * reach * 2.0, 8);
    assert!(
        ran(&mut h).is_empty(),
        "the bottom handle answered beside the nav bar"
    );
    Ok(())
}

/// Each handle's strip as its painter was last told it, by id.
type Strips = Rc<RefCell<std::collections::BTreeMap<String, egui::Rect>>>;

/// A shell on `config` whose handle painter writes every strip down, with a screen `a` and a
/// screen `full` that hides both bars.
fn recording(config: ShellConfig, strips: &Strips) -> fairing::Result<Harness> {
    let record = Rc::clone(strips);
    let mut h = Harness::from_builder(move |ctx| {
        let mut config = config;
        config.motion.reduce = true;
        let mut shell = fairing::Shell::builder(config)
            .gesture_handle_painter(move |_: &egui::Painter, look: &mut HandleLook<'_>| {
                record.borrow_mut().insert(look.id.to_owned(), look.rect);
            })
            .build(ctx)?;
        shell.add(stub("a"));
        shell.add(stub("full").chrome(ChromePolicy::fullscreen()));
        Ok(shell)
    })?;
    h.frames(2);
    Ok(h)
}

/// Put `handle` on (or in its own place) and read back the strip it was drawn at.
fn strip_of(
    h: &mut Harness,
    strips: &Strips,
    handle: GestureHandle,
) -> fairing::Result<egui::Rect> {
    let id = handle.id().to_owned();
    strips.borrow_mut().clear();
    h.shell.add_gesture_handle(handle)?;
    h.frames(2);
    strips
        .borrow()
        .get(&id)
        .copied()
        .ok_or_else(|| fail(format!("`{id}` was not drawn")))
}

/// Whether `rect` runs from `a` to `b` along the axis `pick` reads, to half a point.
fn spans(rect: egui::Rect, pick: fn(egui::Rect) -> (f32, f32), a: f32, b: f32) -> bool {
    let (from, to) = pick(rect);
    (from - a).abs() < 0.5 && (to - b).abs() < 0.5
}

/// The span of a rect from top to bottom.
fn vertical(r: egui::Rect) -> (f32, f32) {
    (r.min.y, r.max.y)
}

/// The span of a rect from left to right.
fn horizontal(r: egui::Rect) -> (f32, f32) {
    (r.min.x, r.max.x)
}

/// **A side strip's ends are the handle's to choose**: between the edge zones by
/// default — the bars where they show, `edge_px` in from the glass where they are hidden. With
/// `HandleEnd::Bar` it runs to the bars, or to the glass where they are hidden; with
/// `HandleEnd::Glass`, to the glass over the bars, and a swipe in from beside the status bar is
/// then the handle's.
#[test]
fn a_side_strips_ends_are_the_handles_to_choose() -> fairing::Result<()> {
    let strips = Strips::default();
    let mut h = recording(single_level_access(), &strips)?;
    let glass = h.screen_rect();
    // `None` leaves the ends as they come: the default is part of what is checked.
    let side = |ends: Option<(HandleEnd, HandleEnd)>| {
        let handle = GestureHandle::new("side", Edge::Right).gesture(
            HandleGesture::short(HandleDirection::Straight),
            LaunchAction::open("a"),
        );
        match ends {
            Some((from, to)) => handle.ends(from, to),
            None => handle,
        }
    };
    let (under_status, over_nav) = between_bars(&h);
    let rect = strip_of(&mut h, &strips, side(None))?;
    assert!(
        spans(rect, vertical, under_status, over_nav),
        "by default, between the bars: {rect:?}"
    );
    let rect = strip_of(
        &mut h,
        &strips,
        side(Some((HandleEnd::Bar, HandleEnd::Bar))),
    )?;
    assert!(
        spans(rect, vertical, under_status, over_nav),
        "to the bars where they show: {rect:?}"
    );
    let rect = strip_of(
        &mut h,
        &strips,
        side(Some((HandleEnd::EdgeZone, HandleEnd::Glass))),
    )?;
    assert!(
        spans(rect, vertical, under_status, glass.max.y),
        "under the status bar to the glass's bottom: {rect:?}"
    );

    // Beside the status bar: the handle's only where the strip reaches the glass's top.
    let reach = mm(&h, 10.0);
    let beside_status = egui::pos2(glass.max.x - 1.0, glass.min.y + 10.0);
    let _ = h.shell.poll_events();
    swipe(&mut h, beside_status, LEFTWARD * reach * 2.0, 8);
    assert!(
        ran(&mut h).is_empty(),
        "a strip under the status bar took a swipe beside it"
    );
    let rect = strip_of(
        &mut h,
        &strips,
        side(Some((HandleEnd::Glass, HandleEnd::Glass))),
    )?;
    assert!(
        spans(rect, vertical, glass.min.y, glass.max.y),
        "glass to glass: {rect:?}"
    );
    swipe(&mut h, beside_status, LEFTWARD * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "side".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )],
        "a glass-to-glass strip did not take the swipe beside the status bar"
    );

    // Over a screen that hides the bars.
    h.shell.launch(LaunchAction::open("full"));
    h.frames(5);
    if h.shell.layout().status.is_some() || h.shell.layout().nav.is_some() {
        return Err(fail("the bars did not hide"));
    }
    let zones = h.shell.layout().edge_zones;
    let (top_zone, bottom_zone) = (zones[0], zones[1]);
    let rect = strip_of(&mut h, &strips, side(None))?;
    assert!(
        spans(rect, vertical, top_zone.max.y, bottom_zone.min.y),
        "by default, between the edge zones: {rect:?}"
    );
    let rect = strip_of(
        &mut h,
        &strips,
        side(Some((HandleEnd::Bar, HandleEnd::Bar))),
    )?;
    assert!(
        spans(rect, vertical, glass.min.y, glass.max.y),
        "to the glass where the bars are hidden: {rect:?}"
    );

    Ok(())
}

/// **A bottom strip's ends are its left and right**: the glass's last two millimetres,
/// clear of the back gesture's bands by default, and the whole width with `HandleEnd::Glass` or
/// `HandleEnd::Bar` — no bar sits at its ends.
#[test]
fn a_bottom_strips_ends_are_its_left_and_right() -> fairing::Result<()> {
    let strips = Strips::default();
    let mut h = recording(no_nav(), &strips)?;
    let glass = h.screen_rect();
    let zones = h.shell.layout().edge_zones;
    let (left_zone, right_zone) = (zones[2], zones[3]);
    let floor = |ends: Option<(HandleEnd, HandleEnd)>| match ends {
        Some((from, to)) => bottom().ends(from, to),
        None => bottom(),
    };
    let rect = strip_of(&mut h, &strips, floor(None))?;
    assert!(
        spans(rect, horizontal, left_zone.max.x, right_zone.min.x)
            && spans(rect, vertical, glass.max.y - mm(&h, 2.0), glass.max.y),
        "by default, the glass's last two millimetres clear of the back bands: {rect:?}"
    );
    for ends in [
        (HandleEnd::Glass, HandleEnd::Glass),
        (HandleEnd::Bar, HandleEnd::Bar),
    ] {
        let rect = strip_of(&mut h, &strips, floor(Some(ends)))?;
        assert!(
            spans(rect, horizontal, glass.min.x, glass.max.x),
            "{ends:?}: the whole width: {rect:?}"
        );
    }
    Ok(())
}

/// **A gated gesture asks to unlock instead of running** — and says it completed all the same.
#[test]
fn a_gated_gesture_asks_to_unlock_and_still_says_so() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "service"], Some("top"));
    config
        .access
        .gates
        .insert("service".to_owned(), "service".to_owned());
    let mut h = shell(config)?;
    h.shell
        .add_gesture_handle(GestureHandle::new("right", Edge::Right).gesture(
            HandleGesture::short(HandleDirection::Straight),
            HandleAction::new(LaunchAction::open("service_menu")).gate("service"),
        ))?;
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::Gesture { handle, .. } if handle == "right")),
        "it went by quietly: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::Access(AccessEvent::UnlockRequested { gate, then })
                if gate.as_str() == "service" && then.is_some()
        )),
        "it did not ask to unlock: {events:?}"
    );
    assert_ne!(focused(&h), Some("service_menu"), "it ran past the gate");
    Ok(())
}

/// **Nothing answers over the shade.**
#[cfg(feature = "overlay")]
#[test]
fn nothing_answers_over_the_shade() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(10);
    if h.shell.overlay().is_closed() {
        return Err(fail("the shade did not open"));
    }
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "it answered over the shade");
    Ok(())
}

/// **Nothing answers over the recent screens, on a screen that guards its edges, or with
/// `[gesture] enabled = false`.**
#[test]
fn nothing_answers_over_the_cards_or_a_guarded_screen() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    let reach = mm(&h, 10.0);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
    if !h.shell.workspace().is_overview_open() {
        return Err(fail("the recent screens did not come up"));
    }
    let _ = h.shell.poll_events();
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert!(
        ran(&mut h).is_empty(),
        "it answered over the recent screens"
    );

    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    h.shell.launch(LaunchAction::open("kiosk"));
    h.frames(3);
    let _ = h.shell.poll_events();
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "it answered on a guarded screen");
    assert_eq!(focused(&h), Some("kiosk"));

    let mut config = single_level_access();
    config.gesture.enabled = false;
    let mut h = shell(config)?;
    h.shell.add_gesture_handle(right())?;
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "it answered with gestures off");
    Ok(())
}

/// A shell with a form whose one text field brings up the on-screen keyboard, noting what is
/// typed in it — with `handle`, or without any.
#[cfg(feature = "osk")]
fn form(handle: Option<GestureHandle>, typed: &Rc<RefCell<String>>) -> fairing::Result<Harness> {
    form_on(single_level_access(), handle, typed)
}

/// The same form, on `config`.
#[cfg(feature = "osk")]
fn form_on(
    config: ShellConfig,
    handle: Option<GestureHandle>,
    typed: &Rc<RefCell<String>>,
) -> fairing::Result<Harness> {
    let text = Rc::clone(typed);
    let mut theme = fairing::Theme::dark();
    theme.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = fairing::Shell::builder(config).theme(theme).build(ctx)?;
        shell.add(screen("form", move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.add_space(160.0);
            let mut text = text.borrow_mut();
            ui.add_sized(
                [300.0, 48.0],
                egui::TextEdit::singleline(&mut *text)
                    .id_salt("t")
                    .hint_text("Type here"),
            );
        }));
        shell.add(stub("batches"));
        shell.add(stub("up"));
        if let Some(handle) = handle {
            shell.add_gesture_handle(handle)?;
        }
        Ok(shell)
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("form"));
    h.frames(3);
    Ok(h)
}

/// A point on the page clear of the field: the right-hand third, a screen inset down.
#[cfg(feature = "osk")]
fn beside_the_field(h: &Harness) -> egui::Pos2 {
    let content = h.shell.layout().content;
    let inset = h.shell.theme().metrics.screen_inset;
    egui::pos2(content.min.x + content.width() * 0.7, content.min.y + inset)
}

/// Put a finger on the form's field, so the keyboard starts to come up.
#[cfg(feature = "osk")]
fn focus_the_field(h: &mut Harness) {
    // By its hint, not at an assumed place under the space.
    let field = h
        .text_rect("Type here")
        .map_or(h.shell.layout().content.center(), |r| r.center());
    h.press(field);
    h.frame();
    h.release(field);
    h.frame();
}

/// **The strip ends above the keyboard**: a swipe from the keys' side is the keys', and one from
/// above them is still the handle's.
#[cfg(feature = "osk")]
#[test]
fn the_strip_ends_above_the_keyboard() -> fairing::Result<()> {
    let mut h = form(Some(right()), &Rc::default())?;
    focus_the_field(&mut h);
    // Up and still.
    h.frames(30);
    let keys = h
        .shell
        .layout()
        .osk
        .ok_or_else(|| fail("the keyboard did not come up"))?;
    let _ = h.shell.poll_events();
    let reach = mm(&h, 10.0);
    let edge = h.screen_rect().max.x - 1.0;
    swipe(
        &mut h,
        egui::pos2(edge, keys.center().y),
        LEFTWARD * reach * 2.0,
        8,
    );
    assert!(ran(&mut h).is_empty(), "the strip took a press on the keys");
    swipe(
        &mut h,
        egui::pos2(edge, keys.min.y - 40.0),
        LEFTWARD * reach * 2.0,
        8,
    );
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )],
        "the strip above the keys did not answer"
    );
    Ok(())
}

/// **The keys at the edge type while the keyboard comes up**: the keyboard slides, a
/// strip lags it by a frame and egui's hit test by another, so a strip that ended where the keys
/// were a frame ago took the top row's outermost key from under the finger. A tap there types the
/// same with a handle as without one, at each moment of the slide.
#[cfg(feature = "osk")]
#[test]
fn the_keys_at_the_edge_type_while_the_keyboard_comes_up() -> fairing::Result<()> {
    let mut typed_any = false;
    for frames in 3..=8 {
        let mut outcome = Vec::new();
        for handle in [None, Some(right())] {
            let typed = Rc::new(RefCell::new(String::new()));
            let mut h = form(handle, &typed)?;
            focus_the_field(&mut h);
            h.frames(frames);
            let keys = h
                .shell
                .layout()
                .osk
                .ok_or_else(|| fail("the keyboard did not start up"))?;
            // The top row, in the strip's width at the right edge.
            h.tap(egui::pos2(
                h.screen_rect().max.x - 4.0,
                keys.min.y + keys.height() * 0.1,
            ));
            h.frames(3);
            outcome.push(typed.borrow().clone());
        }
        typed_any |= outcome.iter().any(|t| !t.is_empty());
        assert_eq!(
            outcome.first(),
            outcome.last(),
            "{frames} frames into the slide: without a handle, then with one"
        );
    }
    assert!(typed_any, "no tap typed anything: the test missed the keys");
    Ok(())
}

/// **The strip stays off the keys while they go down**: with the keyboard on its way
/// out, a swipe from the keys' side is still not the handle's.
#[cfg(feature = "osk")]
#[test]
fn the_strip_stays_off_the_keys_while_they_go_down() -> fairing::Result<()> {
    let mut h = form(Some(right()), &Rc::default())?;
    focus_the_field(&mut h);
    h.frames(30);
    // A tap beside the field takes the focus away, and the keyboard goes.
    let beside = beside_the_field(&h);
    h.tap(beside);
    let mut going = None;
    for _ in 0..120 {
        h.frame();
        if let Some(keys) = h.shell.layout().osk {
            if !h.shell.osk().is_shown() && keys.height() > 60.0 {
                going = Some(keys);
                break;
            }
        }
    }
    let keys = going.ok_or_else(|| fail("the keyboard did not go down"))?;
    let _ = h.shell.poll_events();
    let reach = mm(&h, 10.0);
    let on_the_keys = egui::pos2(h.screen_rect().max.x - 1.0, keys.center().y);
    swipe(&mut h, on_the_keys, LEFTWARD * reach * 2.0, 8);
    assert!(
        ran(&mut h).is_empty(),
        "the strip took a press on the keys going down"
    );
    Ok(())
}

/// **A bottom handle stands aside for the keyboard**: while the keyboard is up the keys
/// lie where the strip was, so a swipe up from there is the keys', and the space bar types under
/// it.
#[cfg(feature = "osk")]
#[test]
fn a_bottom_handle_stands_aside_for_the_keyboard() -> fairing::Result<()> {
    let typed = Rc::new(RefCell::new(String::new()));
    let mut h = form_on(no_nav(), Some(bottom()), &typed)?;
    focus_the_field(&mut h);
    h.frames(30);
    let space = h
        .shell
        .osk()
        .key_rect(" ")
        .ok_or_else(|| fail("the keyboard did not come up"))?;
    // The space bar's lower edge, inside the band the strip took before the keyboard came up.
    let floor = h.screen_rect().max.y;
    let place = egui::pos2(space.center().x, space.max.y - 3.0);
    if place.y < floor - mm(&h, 2.0) {
        return Err(fail(format!(
            "the space bar {space:?} is not where the strip was"
        )));
    }
    let _ = h.shell.poll_events();
    h.tap(place);
    h.frames(2);
    assert_eq!(typed.borrow().as_str(), " ", "the space bar did not type");
    let reach = mm(&h, 10.0);
    swipe(&mut h, place, UPWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "the strip took a swipe on the keys");
    Ok(())
}

/// **The space bar types under a bottom handle while the keyboard comes up**, the first time and
/// every time after: from the frame after the keyboard starts up, a tap on the space
/// bar's lower edge — where the strip was — types the same with the handle as without it. On the
/// very frame a later showing starts, the strip's guard from the frame before still takes such a
/// tap: the documented limit.
#[cfg(feature = "osk")]
#[test]
fn the_space_bar_types_under_a_bottom_handle_while_the_keyboard_comes_up() -> fairing::Result<()> {
    let mut typed_any = false;
    for again in [false, true] {
        for frames in 1..=8 {
            let mut outcome = Vec::new();
            for handle in [None, Some(bottom())] {
                let typed = Rc::new(RefCell::new(String::new()));
                let mut h = form_on(no_nav(), handle, &typed)?;
                if again {
                    // Up once and down again: the strip is back at the glass's bottom.
                    focus_the_field(&mut h);
                    h.frames(30);
                    h.tap(beside_the_field(&h));
                    h.frames(120);
                    if h.shell.layout().osk.is_some() {
                        return Err(fail("the keyboard did not go down"));
                    }
                }
                focus_the_field(&mut h);
                h.frames(frames);
                let Some(space) = h.shell.osk().key_rect(" ") else {
                    return Err(fail("the keyboard did not start up"));
                };
                h.tap(egui::pos2(space.center().x, space.max.y - 3.0));
                h.frames(3);
                outcome.push(typed.borrow().clone());
            }
            typed_any |= outcome.iter().any(|t| !t.is_empty());
            assert_eq!(
                outcome.first(),
                outcome.last(),
                "showing again {again}, {frames} frames in: without a handle, then with one"
            );
        }
    }
    assert!(
        typed_any,
        "no tap typed anything: the test missed the space bar"
    );
    Ok(())
}

/// What a handle painter was told, per frame.
#[derive(Debug, Clone)]
struct Told {
    rect: egui::Rect,
    gesture: Option<HandleGesture>,
    reach: f32,
    rest: f32,
    done: bool,
    swiping: bool,
}

/// **A handle painter is told the strip at rest, and the swipe from it**: the gesture it would
/// be, how far it has reached, how far a rest has gone, and when a long gesture has run.
#[test]
fn a_handle_painter_is_told_the_swipe() -> fairing::Result<()> {
    let told: Rc<RefCell<Vec<Told>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let mut h = Harness::from_builder(move |ctx| {
        let mut config = single_level_access();
        config.motion.reduce = true;
        let mut shell = fairing::Shell::builder(config)
            .gesture_handle_painter(move |_: &egui::Painter, look: &mut HandleLook<'_>| {
                let swipe = look.swipe;
                record.borrow_mut().push(Told {
                    rect: look.rect,
                    gesture: swipe.and_then(|s| s.gesture),
                    reach: swipe.map_or(0.0, |s| s.reach),
                    rest: swipe.map_or(0.0, |s| s.rest),
                    done: swipe.is_some_and(|s| s.done),
                    swiping: swipe.is_some(),
                });
            })
            .build(ctx)?;
        shell.add(stub("service_menu"));
        shell.add(stub("batches"));
        shell.add_gesture_handle(right().gesture(
            HandleGesture::long(HandleDirection::Straight),
            LaunchAction::open("service_menu"),
        ))?;
        Ok(shell)
    })?;
    h.frames(3);
    let at_rest = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called at rest"))?;
    assert!(!at_rest.swiping);
    let strip = mm(&h, 2.0);
    assert!(
        (at_rest.rect.width() - strip).abs() < 0.5
            && (at_rest.rect.max.x - h.screen_rect().max.x).abs() < 0.5,
        "the strip is not the edge's two millimetres: {:?}",
        at_rest.rect
    );
    let reach = mm(&h, 10.0);
    let from = right_strip(&h, 0.5);
    h.press(from);
    h.frame();
    h.move_to(from + LEFTWARD * reach * 0.6);
    h.frame();
    let early = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("not told the swipe"))?;
    assert!(
        early.swiping && early.reach > 0.4 && early.reach < 1.0,
        "{early:?}"
    );
    assert_eq!(
        early.gesture,
        Some(HandleGesture::short(HandleDirection::Straight))
    );
    h.move_to(from + LEFTWARD * reach * 2.0);
    h.frames(10);
    let resting = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("not told"))?;
    assert!(
        resting.reach >= 1.0 && resting.rest > 0.0 && !resting.done,
        "{resting:?}"
    );
    h.frames(30);
    let long = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("not told"))?;
    assert!(long.done, "not told the long gesture ran: {long:?}");
    assert_eq!(
        long.gesture,
        Some(HandleGesture::long(HandleDirection::Straight))
    );
    h.release(from + LEFTWARD * reach * 2.0);
    h.frames(2);
    Ok(())
}

/// **The built-in handle shows its strip where it asked to, and the swipe's arrow**: invisible at
/// rest by default, a faint strip with `visible(true)`.
#[test]
fn the_built_in_handle_shows_what_it_asked_for() -> fairing::Result<()> {
    let strip_drawn = |h: &mut Harness| {
        let strip = h.screen_rect().max.x - mm(h, 2.0);
        h.frame_shapes().into_iter().any(|clipped| {
            matches!(clipped.shape, egui::Shape::Rect(r) if (r.rect.min.x - strip).abs() < 0.5
                && (r.rect.max.x - h.screen_rect().max.x).abs() < 0.5)
        })
    };
    // A disc just ahead of the finger, on the line it runs along: the arrow's.
    let disc_ahead_of = |h: &mut Harness, finger: egui::Pos2| {
        h.frame_shapes().into_iter().any(|clipped| {
            matches!(clipped.shape, egui::Shape::Circle(c) if c.fill != egui::Color32::TRANSPARENT
                && (c.center.y - finger.y).abs() < 0.5
                && c.center.x < finger.x
                && c.center.x > finger.x - 80.0)
        })
    };
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    h.frames(2);
    assert!(!strip_drawn(&mut h), "an invisible handle shows at rest");
    h.shell.add_gesture_handle(right().visible(true))?;
    h.frames(2);
    assert!(strip_drawn(&mut h), "a visible handle does not show");
    let reach = mm(&h, 10.0);
    let from = right_strip(&h, 0.5);
    let finger = from + LEFTWARD * reach * 2.0;
    assert!(!disc_ahead_of(&mut h, finger), "an arrow before the swipe");
    h.press(from);
    h.frame();
    h.move_to(finger);
    h.frame();
    assert!(
        disc_ahead_of(&mut h, finger),
        "no arrow ahead of the finger while the handle is swiped"
    );
    h.release(finger);
    h.frames(2);
    assert!(
        !disc_ahead_of(&mut h, finger),
        "the arrow outlived the swipe"
    );

    // From the bottom, up and to the left: the disc is up and to the left of the finger.
    let mut h = shell(no_nav())?;
    h.shell.add_gesture_handle(bottom())?;
    h.frames(2);
    let from = bottom_strip(&h, 0.5);
    let finger = from + egui::vec2(-1.0, -1.0) * reach * 1.5;
    h.press(from);
    h.frame();
    h.move_to(finger);
    h.frame();
    let up_left = h.frame_shapes().into_iter().any(|clipped| {
        matches!(clipped.shape, egui::Shape::Circle(c) if c.fill != egui::Color32::TRANSPARENT && {
            let ahead = finger - c.center;
            ahead.x > 10.0 && ahead.y > 10.0 && (ahead.x - ahead.y).abs() < 1.0 && ahead.x < 80.0
        })
    });
    assert!(up_left, "no arrow up and to the left of the finger");
    h.release(finger);
    h.frames(2);
    Ok(())
}

/// **A swipe the prompt takes from under the finger is over**: a long gated gesture brings up
/// the prompt mid-swipe, and once the prompt is gone the handle is at rest — nothing of the swipe
/// is left to draw.
#[test]
fn a_swipe_the_prompt_takes_is_over() -> fairing::Result<()> {
    let told: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let mut h = Harness::from_builder(move |ctx| {
        let mut config = access_config(&["viewer", "operator"], Some("top"));
        config.access.pin_table.pins = [("operator".to_owned(), "1234".to_owned())]
            .into_iter()
            .collect();
        config
            .access
            .gates
            .insert("service".to_owned(), "operator".to_owned());
        config.motion.reduce = true;
        let mut shell = fairing::Shell::builder(config)
            .gesture_handle_painter(move |_: &egui::Painter, look: &mut HandleLook<'_>| {
                record.borrow_mut().push(look.swipe.is_some());
            })
            .build(ctx)?;
        shell.add(stub("service_menu"));
        shell.add_gesture_handle(GestureHandle::new("right", Edge::Right).gesture(
            HandleGesture::long(HandleDirection::Straight),
            HandleAction::new(LaunchAction::open("service_menu")).gate("service"),
        ))?;
        Ok(shell)
    })?;
    h.frames(3);
    let reach = mm(&h, 10.0);
    let from = right_strip(&h, 0.5);
    let finger = from + LEFTWARD * reach * 2.0;
    h.press(from);
    h.frame();
    h.move_to(finger);
    h.frames(31);
    if !h.shell.unlock_prompt_visible() {
        return Err(fail("the gated long gesture did not bring up the prompt"));
    }
    h.release(finger);
    h.frames(2);
    h.shell.back();
    h.frames(3);
    if h.shell.unlock_prompt_visible() {
        return Err(fail("back did not put the prompt away"));
    }
    told.borrow_mut().clear();
    h.frames(2);
    let told = told.borrow();
    assert!(
        !told.is_empty(),
        "the handle is not drawn once the prompt is gone"
    );
    assert!(
        told.iter().all(|swiping| !swiping),
        "the swipe the prompt took is still drawn"
    );
    Ok(())
}

/// **The desktop under a strip does not move with its swipe**: a fast swipe in from the
/// strip at home runs the handle and leaves the page where it was. Just inward of the strip, the
/// same swipe is the page's.
#[test]
fn a_swipe_from_the_strip_leaves_the_desktop_page() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.desktop.columns = 1;
    config.desktop.rows = 1;
    let mut h = test_shell(config, |sh| {
        for id in ["p0", "p1", "p2"] {
            sh.add(stub(id).title(id).icon(fairing::icon::FOLDER).desktop());
        }
        sh.add(stub("batches"));
    })?;
    h.frames(3);
    if h.shell.desktop().pages().len() != 3 {
        return Err(fail("the desktop is not three pages"));
    }
    h.shell.add_gesture_handle(right())?;
    h.frames(2);
    let reach = mm(&h, 10.0);
    // Three reaches in four frames: a fling, were it the page's.
    let fling = |h: &mut Harness, from: egui::Pos2| {
        let mut paged = false;
        h.press(from);
        h.frame();
        for step in 1..=4u8 {
            h.move_to(from + LEFTWARD * reach * 0.75 * f32::from(step));
            h.frame();
            paged |= h.shell.desktop().swipe().is_dragging();
        }
        h.release(from + LEFTWARD * reach * 3.0);
        h.frames(30);
        paged
    };
    let from = right_strip(&h, 0.5);
    let paged = fling(&mut h, from);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )]
    );
    assert!(!paged, "the page followed the handle's swipe");
    assert_eq!(h.shell.desktop().page(), 0);

    h.shell.home();
    h.frames(10);
    let _ = h.shell.poll_events();
    let inward = from + LEFTWARD * (mm(&h, 2.0) + 6.0);
    let paged = fling(&mut h, inward);
    assert!(
        ran(&mut h).is_empty(),
        "a swipe inward of the strip ran the handle"
    );
    assert!(
        paged && h.shell.desktop().page() == 1,
        "the swipe inward of the strip was not the page's"
    );
    Ok(())
}

/// A screen that runs to the edges: a list of rows as wide as the glass, each a button, dragged
/// to scroll as on a touch screen (egui does that only once it has seen a touch). It notes how far
/// the list has scrolled and how many rows were pressed.
fn edge_to_edge(scrolled: &Rc<Cell<f32>>, pressed: &Rc<Cell<u32>>) -> fairing::screen::ScreenDecl {
    let (scrolled, pressed) = (Rc::clone(scrolled), Rc::clone(pressed));
    screen("list", move |ui: &mut egui::Ui, cx: &mut Cx| {
        // The height stated, as `layout::page` does: left to the parent, nothing overflows.
        let out = egui::ScrollArea::vertical()
            .id_salt("list")
            .max_height(cx.pane.rect.height())
            .scroll_source(egui::scroll_area::ScrollSource::ALL)
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                for row in 0..200 {
                    let size = [ui.available_width(), 40.0];
                    if ui
                        .add_sized(size, egui::Button::new(format!("row {row}")))
                        .clicked()
                    {
                        pressed.set(pressed.get() + 1);
                    }
                }
            });
        scrolled.set(out.state.offset.y);
    })
    .chrome(ChromePolicy::fullscreen())
}

/// **Nothing under a strip sees a press that begins in it**, the way a One Hand
/// Operation+ handle takes its edge: over a list that runs to the edge, a swipe from the strip runs
/// the handle and leaves the list where it was, a stroke along the edge in the strip scrolls
/// nothing, and a tap there presses nothing. Just inward of the strip the rows have the finger
/// back: a tap presses one, a stroke scrolls them.
#[test]
fn nothing_under_a_strip_sees_its_presses() -> fairing::Result<()> {
    let scrolled = Rc::new(Cell::new(0.0_f32));
    let pressed = Rc::new(Cell::new(0_u32));
    let list = edge_to_edge(&scrolled, &pressed);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(stub("up"));
        sh.add(list);
    })?;
    h.frames(2);
    h.shell
        .add_gesture_handle(GestureHandle::new("right", Edge::Right).gesture(
            HandleGesture::short(HandleDirection::DiagonalUp),
            LaunchAction::open("up"),
        ))?;
    h.shell.launch(LaunchAction::open("list"));
    h.frames(5);
    let _ = h.shell.poll_events();
    let reach = mm(&h, 10.0);
    let edge = right_strip(&h, 0.5);
    let up = egui::vec2(0.0, -1.0);
    let still = |scrolled: &Rc<Cell<f32>>| scrolled.get().abs() < 0.5;

    swipe(&mut h, edge, (LEFTWARD + up) * reach * 2.0, 20);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::DiagonalUp)
        )]
    );
    assert!(
        still(&scrolled),
        "the list scrolled under the handle's swipe"
    );
    h.shell.back();
    h.frames(5);
    if focused(&h) != Some("list") {
        return Err(fail("back did not bring the list back"));
    }

    swipe(&mut h, edge, up * reach * 3.0, 20);
    assert!(
        ran(&mut h).is_empty(),
        "a stroke along the edge ran the handle"
    );
    assert!(
        still(&scrolled),
        "a stroke along the edge in the strip scrolled the list"
    );
    h.tap(edge);
    h.frames(2);
    assert_eq!(pressed.get(), 0, "a tap in the strip pressed a row");

    let inward = edge + LEFTWARD * (mm(&h, 2.0) + 6.0);
    h.tap(inward);
    h.frames(2);
    assert_eq!(
        pressed.get(),
        1,
        "a tap just inward of the strip pressed nothing"
    );
    swipe(&mut h, inward, up * reach * 3.0, 20);
    assert!(
        !still(&scrolled),
        "a stroke just inward of the strip scrolled nothing"
    );
    Ok(())
}

/// **A popup a screen opens over a strip leaves the strip's presses to the handle**: the
/// strip's guard stays on top, whichever came first.
#[test]
fn a_popup_over_a_strip_leaves_it_to_the_handle() -> fairing::Result<()> {
    let pressed = Rc::new(Cell::new(0_u32));
    let counter = Rc::clone(&pressed);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(stub("batches"));
        sh.add(screen("menu", move |ui: &mut egui::Ui, _: &mut Cx| {
            // A popup against the right edge, as a dropdown's list may open.
            let glass = ui.ctx().content_rect();
            let rect = egui::Rect::from_min_max(
                egui::pos2(glass.max.x - 200.0, glass.center().y - 100.0),
                egui::pos2(glass.max.x, glass.center().y + 100.0),
            );
            let _ = egui::Area::new(egui::Id::new("menu.popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .show(ui.ctx(), |ui| {
                    if ui
                        .add_sized(rect.size(), egui::Button::new("item"))
                        .clicked()
                    {
                        counter.set(counter.get() + 1);
                    }
                });
        }));
    })?;
    h.frames(2);
    // The handle first, so its guard is the older layer.
    h.shell.add_gesture_handle(right())?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("menu"));
    h.frames(5);
    let _ = h.shell.poll_events();
    let edge = right_strip(&h, 0.5);
    h.tap(edge);
    h.frames(2);
    assert_eq!(pressed.get(), 0, "the popup took a tap in the strip");
    h.tap(edge + LEFTWARD * 100.0);
    h.frames(2);
    assert_eq!(pressed.get(), 1, "the popup lost a tap beside the strip");
    let reach = mm(&h, 10.0);
    swipe(&mut h, edge, LEFTWARD * reach * 2.0, 8);
    assert_eq!(
        ran(&mut h),
        [(
            "right".to_owned(),
            HandleGesture::short(HandleDirection::Straight)
        )]
    );
    Ok(())
}

/// A screen that is one button, glass to glass, counting its presses — with its edges guarded or
/// not.
fn edge_button(guard: bool, pressed: &Rc<Cell<u32>>) -> fairing::screen::ScreenDecl {
    let pressed = Rc::clone(pressed);
    screen("edge", move |ui: &mut egui::Ui, _: &mut Cx| {
        let size = ui.available_size();
        if ui.add_sized(size, egui::Button::new("edge")).clicked() {
            pressed.set(pressed.get() + 1);
        }
    })
    .chrome(ChromePolicy {
        edge_guard: guard,
        ..ChromePolicy::fullscreen()
    })
}

/// **Where the handles do not answer, the strip is not there at all**: on a screen that
/// guards its edges, and with gestures off, a tap at the very edge reaches the screen.
#[test]
fn where_the_handles_rest_the_edge_is_the_screens() -> fairing::Result<()> {
    for (guard, gestures) in [(true, true), (false, false)] {
        let pressed = Rc::new(Cell::new(0_u32));
        let button = edge_button(guard, &pressed);
        let mut config = single_level_access();
        config.gesture.enabled = gestures;
        let mut h = test_shell(config, move |sh| {
            sh.add(button);
        })?;
        h.frames(2);
        h.shell.add_gesture_handle(right())?;
        h.shell.launch(LaunchAction::open("edge"));
        h.frames(5);
        let edge = right_strip(&h, 0.5);
        h.tap(edge);
        h.frames(2);
        assert_eq!(
            pressed.get(),
            1,
            "edge_guard {guard}, gestures {gestures}: a strip took the tap"
        );
    }
    Ok(())
}

/// **A handle taken off answers no more.**
#[test]
fn a_removed_handle_answers_no_more() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    h.shell.add_gesture_handle(right())?;
    assert!(h.shell.remove_gesture_handle("right"));
    assert!(
        !h.shell.remove_gesture_handle("right"),
        "it was there twice"
    );
    let reach = mm(&h, 10.0);
    swipe_in(&mut h, 0.5, LEFTWARD * reach * 2.0, 8);
    assert!(ran(&mut h).is_empty(), "the removed handle answered");
    assert_eq!(h.shell.gesture_handles().count(), 0);
    Ok(())
}
