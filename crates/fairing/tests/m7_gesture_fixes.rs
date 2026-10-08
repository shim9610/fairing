//! Regression tests for the gesture fixes made before 0.1.0: a swipe cancelled between frames
//! still ends, a press that shares a frame with the last release is heard, the back gesture's
//! re-grab, a region placed everywhere, and the pause at home.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::gesture::{GestureRegion, Phase, RegionCx, RegionPlaceCx, RegionTouch};
use fairing::overlay::OverlayState;
use fairing::testing::{access_config, test_shell, Harness};
use fairing::{screen, Cx, LaunchAction, ShellConfig, ShellEvent};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// Two levels, a PIN for the upper one, and an `svc` screen behind it.
fn gated() -> ShellConfig {
    let mut config = access_config(&["viewer", "operator"], Some("bottom"));
    config.access.pin_table.pins = [("operator".to_owned(), "1234".to_owned())]
        .into_iter()
        .collect();
    config
        .access
        .gates
        .insert("svc".to_owned(), "operator".to_owned());
    config
}

fn gestures(mut config: ShellConfig) -> ShellConfig {
    "gesture".clone_into(&mut config.nav_bar.style);
    config
}

/// A shell on `config` with screens `a`, `b`, `c` and `svc`, a couple of frames in.
fn shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = test_shell(config, |sh| {
        for id in ["a", "b", "c", "svc"] {
            sh.add(
                screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                    ui.label(id);
                })
                .title(id),
            );
        }
    })?;
    h.frames(2);
    let _ = h.shell.poll_events();
    Ok(h)
}

/// Move the held finger by `step` for `frames` frames.
fn drag_by(h: &mut Harness, from: egui::Pos2, step: egui::Vec2, frames: usize) -> egui::Pos2 {
    let mut pos = from;
    for _ in 0..frames {
        pos += step;
        h.move_to(pos);
        h.frame();
    }
    pos
}

/// Screens a → b, and a back swipe from the left edge 120 px in, still held.
fn mid_back_swipe(config: ShellConfig) -> fairing::Result<(Harness, egui::Pos2)> {
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    let from = egui::pos2(4.0, 300.0);
    h.press(from);
    h.frame();
    let at = drag_by(&mut h, from, egui::vec2(20.0, 0.0), 6);
    if !h.shell.workspace().is_animating() {
        return Err(fail("the back gesture did not start"));
    }
    Ok((h, at))
}

// ---------------------------------------------------------------------------------------------
// A swipe cancelled between frames still ends.
// ---------------------------------------------------------------------------------------------

/// An unlock request mid back-swipe ends the swipe, so the screens settle instead of hanging.
#[test]
fn back_gesture_settles_after_an_unlock_request_mid_drag() -> fairing::Result<()> {
    let (mut h, at) = mid_back_swipe(gated())?;
    // Something asks for the operator level while the finger is mid-swipe.
    h.shell.launch(LaunchAction::open("svc"));
    if !h.shell.unlock_prompt_visible() {
        return Err(fail("the gated launch did not bring up the prompt"));
    }
    h.frame();
    h.release(at);
    h.frames(30);
    assert!(
        !h.shell.workspace().is_animating(),
        "the back gesture is still mid-drag 30 frames after the finger lifted: {:?}",
        h.shell.workspace().stack_transition()
    );
    // And once the prompt is answered, the screens are not wedged either.
    for key in [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
    ] {
        h.key(key);
    }
    h.frames(10);
    assert!(
        !h.shell.workspace().is_animating(),
        "still mid-drag after the unlock: {:?}",
        h.shell.workspace().stack_transition()
    );
    Ok(())
}

/// A lock mid back-swipe ends the swipe, so the screens do not stay mid-drag behind the lock.
#[test]
fn back_gesture_settles_after_a_lock_mid_drag() -> fairing::Result<()> {
    let (mut h, at) = mid_back_swipe(gated())?;
    h.shell.handle().launch(LaunchAction::Lock);
    h.frame();
    if !h.shell.unlock_prompt_visible() {
        return Err(fail("no lock screen"));
    }
    h.release(at);
    h.frames(30);
    assert!(
        !h.shell.workspace().is_animating(),
        "the back gesture is still mid-drag behind the lock screen: {:?}",
        h.shell.workspace().stack_transition()
    );
    Ok(())
}

/// A shade pull under way when an unlock request comes in settles once the finger lifts.
#[test]
fn shade_settles_after_an_unlock_request_mid_pull() -> fairing::Result<()> {
    let mut h = shell(gated())?;
    let from = egui::pos2(500.0, 10.0);
    h.press(from);
    h.frame();
    let at = drag_by(&mut h, from, egui::vec2(0.0, 30.0), 5);
    if !matches!(h.shell.overlay().state(), OverlayState::Dragging { .. }) {
        return Err(fail(format!(
            "the shade is not following the finger: {:?}",
            h.shell.overlay().state()
        )));
    }
    h.shell.launch(LaunchAction::open("svc"));
    h.frame();
    h.release(at);
    h.frames(60);
    let state = h.shell.overlay().state();
    assert!(
        matches!(state, OverlayState::Closed | OverlayState::Open),
        "the shade never settled after the finger lifted: {state:?}"
    );
    Ok(())
}

/// A fast shade pull cut off by a lock goes back up: the cancelled pull does not open the shade
/// behind the lock screen.
#[test]
fn shade_stays_shut_behind_a_lock_that_cut_a_pull_off() -> fairing::Result<()> {
    let mut h = shell(gated())?;
    let from = egui::pos2(500.0, 10.0);
    h.press(from);
    h.frame();
    let _ = drag_by(&mut h, from, egui::vec2(0.0, 60.0), 5);
    if !matches!(h.shell.overlay().state(), OverlayState::Dragging { .. }) {
        return Err(fail(format!(
            "the shade is not following the finger: {:?}",
            h.shell.overlay().state()
        )));
    }
    h.shell.handle().launch(LaunchAction::Lock);
    h.frames(60);
    let state = h.shell.overlay().state();
    assert!(
        matches!(state, OverlayState::Closed),
        "the lock cut the pull off, yet the shade is {state:?}"
    );
    Ok(())
}

/// A gesture-nav lift under way when an unlock request comes in goes back down.
#[test]
fn lift_settles_after_an_unlock_request_mid_swipe() -> fairing::Result<()> {
    let mut h = shell(gestures(gated()))?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let from = egui::pos2(400.0, h.screen_rect().max.y - 6.0);
    h.press(from);
    h.frame();
    let at = drag_by(&mut h, from, egui::vec2(0.0, -15.0), 4);
    if !h.shell.workspace().is_lifting() {
        return Err(fail("the screen is not lifted"));
    }
    h.shell.launch(LaunchAction::open("svc"));
    h.frame();
    h.release(at);
    h.frames(60);
    assert!(
        !h.shell.workspace().is_lifting(),
        "the lift is still on 60 frames after the finger lifted"
    );
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// A release and the next press in one frame.
// ---------------------------------------------------------------------------------------------

/// What a probe region heard.
type Phases = Rc<RefCell<Vec<Phase>>>;

struct Probe {
    rect: egui::Rect,
    log: Phases,
}

impl GestureRegion for Probe {
    fn place(&mut self, _: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        Some(self.rect)
    }

    fn touch(&mut self, touch: &RegionTouch, _: &mut RegionCx<'_>) {
        self.log.borrow_mut().push(touch.phase);
    }
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    }
}

/// A quick double tap on a slow frame: the region hears both touches whole, the second one
/// coming down in the frame the first one lifted.
#[test]
fn region_hears_a_press_that_shares_a_frame_with_the_last_release() -> fairing::Result<()> {
    let mut h = shell(gated())?;
    let log = Phases::default();
    let rect = egui::Rect::from_center_size(egui::pos2(512.0, 300.0), egui::vec2(300.0, 200.0));
    h.shell.add_gesture_region(
        "pad",
        Probe {
            rect,
            log: Rc::clone(&log),
        },
    )?;
    h.frames(2);
    let c = rect.center();
    h.press(c);
    h.frame();
    // One frame: the first touch lifts, the second comes down.
    h.push_event(button(c, false));
    h.push_event(button(c, true));
    h.frame();
    h.move_to(c);
    h.frame();
    h.release(c);
    h.frames(2);
    let phases = log.borrow().clone();
    let started = phases.iter().filter(|p| **p == Phase::Started).count();
    let ended = phases.iter().filter(|p| **p == Phase::Ended).count();
    assert_eq!(
        (started, ended),
        (2, 2),
        "two touches, each heard whole: {phases:?}"
    );
    Ok(())
}

/// The emergency gesture sees a corner press that came down in the frame the last touch lifted.
#[test]
fn emergency_sees_a_press_that_shares_a_frame_with_the_last_release() -> fairing::Result<()> {
    let mut h = shell(gated())?;
    let corner = egui::pos2(1014.0, 10.0);
    let middle = egui::pos2(500.0, 300.0);
    h.press(middle);
    h.frame();
    h.push_event(button(middle, false));
    h.push_event(egui::Event::PointerMoved(corner));
    h.push_event(button(corner, true));
    h.frame();
    // Three seconds still in the corner.
    for _ in 0..180 {
        h.move_to(corner);
        h.frame();
    }
    let fired = h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::Emergency));
    assert!(
        fired,
        "three seconds in the corner, and no emergency gesture"
    );
    h.release(corner);
    h.frame();
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The back gesture's re-grab.
// ---------------------------------------------------------------------------------------------

/// Two screens a → b on a `reduce = false` shell, so the back gesture's return spring runs.
fn animated_two_screens() -> fairing::Result<Harness> {
    let mut h = Harness::new(
        fairing::testing::single_level_access(),
        fairing::Services::null(),
    )?;
    for id in ["a", "b"] {
        h.shell.add(screen(id, |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("s");
        }));
    }
    h.frames(2);
    for id in ["a", "b"] {
        h.shell.launch(LaunchAction::open(id));
        for _ in 0..90 {
            h.frame();
            if !h.shell.is_animating() {
                break;
            }
        }
    }
    Ok(h)
}

fn still(h: &mut Harness, pos: egui::Pos2, frames: usize) {
    for _ in 0..frames {
        h.move_to(pos);
        h.frame();
    }
}

/// A back swipe 0.3 W in, held still and let go: the screen is on its way back.
fn let_go_short(h: &mut Harness, start: egui::Pos2) {
    h.press(start);
    h.frame();
    let end = drag_by(h, start, egui::vec2(15.0, 0.0), 20);
    still(h, end, 12);
    h.release(end);
    h.frame();
    h.frames(2);
}

/// Taken hold of again mid-return, the screen carries on from the caught `p` once, not twice.
#[test]
fn back_regrab_carries_on_from_the_caught_p_once() -> fairing::Result<()> {
    let mut h = animated_two_screens()?;
    let start = egui::pos2(5.0, 300.0);
    let_go_short(&mut h, start);
    // Taken hold of again, and dragged 10 px a frame for 20 frames: dx = 200 px.
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(10.0, 0.0), 20);
    let grab = h.shell.workspace().gesture_back_grab();
    if grab < 0.05 {
        return Err(fail(format!("no re-grab happened: {grab}")));
    }
    let shown = h.shell.workspace().stack_transition().t();
    let expected = (grab + 200.0 / 1024.0).clamp(0.0, 1.0);
    assert!(
        (shown - expected).abs() < 0.03,
        "caught at {grab:.3}, dragged 200 px more: the screen should be at {expected:.3}, it is at {shown:.3}"
    );
    h.release(end);
    h.frames(60);
    Ok(())
}

/// A fresh back gesture after a re-grabbed one starts from 0, not from the old grab.
#[test]
fn fresh_back_gesture_does_not_start_from_a_stale_grab() -> fairing::Result<()> {
    let mut h = animated_two_screens()?;
    let start = egui::pos2(5.0, 300.0);
    let_go_short(&mut h, start);
    // Taken hold of again, barely moved, let go still: it goes back to rest.
    h.press(start);
    h.frame();
    let end = drag_by(&mut h, start, egui::vec2(7.0, 0.0), 3);
    let grab = h.shell.workspace().gesture_back_grab();
    if grab < 0.05 {
        return Err(fail(format!("no re-grab happened: {grab}")));
    }
    still(&mut h, end, 12);
    h.release(end);
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    assert!(
        h.shell.workspace().gesture_back_grab().abs() < 1e-6,
        "the grab outlived its gesture"
    );
    // A fresh gesture: the frame it passes the slop.
    h.press(start);
    h.frame();
    h.move_to(start + egui::vec2(20.0, 0.0));
    h.frame();
    let shown = h.shell.workspace().stack_transition().t();
    assert!(
        shown < 0.05,
        "a fresh 20 px pull is drawn at {shown:.3}: the last gesture's grab ({grab:.3}) leaked into it"
    );
    h.release(start + egui::vec2(20.0, 0.0));
    h.frames(60);
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// A region placed everywhere.
// ---------------------------------------------------------------------------------------------

struct Everywhere;

impl GestureRegion for Everywhere {
    fn place(&mut self, _: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        Some(egui::Rect::EVERYTHING)
    }

    fn touch(&mut self, _: &RegionTouch, _: &mut RegionCx<'_>) {}
}

/// A region placed at `Rect::EVERYTHING` is clipped to the glass rather than handed on infinite.
#[test]
fn an_infinite_region_is_clipped_to_the_glass() -> fairing::Result<()> {
    let mut h = shell(gated())?;
    h.shell.add_gesture_region("all", Everywhere)?;
    h.frames(3);
    h.tap(egui::pos2(500.0, 300.0));
    let at = h.ctx.layer_id_at(egui::pos2(500.0, 300.0));
    assert!(at.is_some(), "nothing is hit at the glass's middle");
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The pause at home.
// ---------------------------------------------------------------------------------------------

/// At home a pause just past the slop is a hand making up its mind, not the recent screens.
#[test]
fn at_home_a_pause_low_down_is_not_recents() -> fairing::Result<()> {
    let mut h = shell(gestures(gated()))?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    if !h.shell.workspace().is_home() {
        return Err(fail("not at home"));
    }
    let from = egui::pos2(400.0, h.screen_rect().max.y - 6.0);
    h.press(from);
    h.frame();
    let at = drag_by(&mut h, from, egui::vec2(0.0, -7.0), 3);
    h.frames(20);
    assert!(
        !h.shell.workspace().is_overview_open(),
        "a 21 px pause at home brought the recent screens"
    );
    h.release(at);
    h.frames(3);
    Ok(())
}

/// At home a pause well up the bottom edge still brings the recent screens.
#[test]
fn at_home_a_pause_high_up_is_recents() -> fairing::Result<()> {
    let mut h = shell(gestures(gated()))?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    let from = egui::pos2(400.0, h.screen_rect().max.y - 6.0);
    h.press(from);
    h.frame();
    let at = drag_by(&mut h, from, egui::vec2(0.0, -20.0), 8);
    still(&mut h, at, 20);
    let asked = h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OverviewRequested));
    assert!(
        asked,
        "a 160 px pause at home did not ask for the recent screens"
    );
    h.release(at);
    h.frames(3);
    Ok(())
}
