//! **Gesture regions** — a stretch of the glass that takes every touch
//! beginning in it, placed, told and drawn by a `GestureRegion` of the integrator's: a part of a
//! screen as a trackpad, say.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::access::AccessEvent;
use fairing::gesture::{
    Edge, GestureHandle, GestureRegion, HandleDirection, HandleGesture, Phase, RegionCx,
    RegionLook, RegionPlaceCx, RegionTouch, MAX_GESTURE_REGIONS,
};
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

/// A screen that is one button over all its content, counting its presses.
fn button(id: &'static str, pressed: &Rc<Cell<u32>>) -> fairing::screen::ScreenDecl {
    let pressed = Rc::clone(pressed);
    screen(id, move |ui: &mut egui::Ui, _: &mut Cx| {
        let size = ui.available_size();
        if ui.add_sized(size, egui::Button::new(id)).clicked() {
            pressed.set(pressed.get() + 1);
        }
    })
}

/// A shell on `config` with the screens these tests open, a frame or two in.
fn shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = test_shell(config, |sh| {
        for id in ["a", "b", "service_menu"] {
            sh.add(stub(id));
        }
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

/// What a probe region heard and was asked.
#[derive(Debug, Default)]
struct Heard {
    /// Each touch's phase, in order.
    phases: Vec<Phase>,
    /// The touches, in order.
    touches: Vec<RegionTouch>,
    /// How many times it was asked where it is.
    placed: u32,
    /// What its painting was told last: the rect, and the touch.
    painted: Option<(egui::Rect, Option<RegionTouch>)>,
}

type Log = Rc<RefCell<Heard>>;

/// A region over `rect` that writes down what it hears and what it is drawn with — placed while
/// `here` holds, and running `then` (behind a gate, where one is named) when a touch reaches
/// `on`.
struct Probe {
    rect: egui::Rect,
    log: Log,
    here: Rc<Cell<bool>>,
    then: Option<(Phase, LaunchAction, Option<&'static str>)>,
}

impl Probe {
    fn new(rect: egui::Rect, log: &Log) -> Self {
        Self {
            rect,
            log: Rc::clone(log),
            here: Rc::new(Cell::new(true)),
            then: None,
        }
    }

    fn then(mut self, on: Phase, action: LaunchAction, gate: Option<&'static str>) -> Self {
        self.then = Some((on, action, gate));
        self
    }
}

impl GestureRegion for Probe {
    fn place(&mut self, _: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        self.log.borrow_mut().placed += 1;
        self.here.get().then_some(self.rect)
    }

    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        {
            let mut log = self.log.borrow_mut();
            log.phases.push(touch.phase);
            log.touches.push(*touch);
        }
        if let Some((on, action, gate)) = self.then.clone() {
            if touch.phase == on {
                match gate {
                    Some(gate) => cx.launch_gated(action, gate),
                    None => cx.launch(action),
                }
            }
        }
    }

    fn paint(&mut self, _: &egui::Painter, look: &mut RegionLook<'_>) {
        self.log.borrow_mut().painted = Some((look.rect, look.touch));
    }
}

/// The app's pointer, which the trackpad moves and a screen would draw.
#[derive(Debug, Default)]
struct Pointer {
    at: egui::Pos2,
    clicks: u32,
}

/// The guide's trackpad: over `rect` on the `remote` screen, the keyboard down; the pointer goes
/// twice as far as the finger, and a tap is a click.
struct Trackpad {
    rect: egui::Rect,
}

impl GestureRegion for Trackpad {
    fn place(&mut self, cx: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        (cx.focused == Some("remote") && cx.keys.is_none()).then_some(self.rect)
    }

    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        let Some(pointer) = cx.app_mut::<Pointer>() else {
            return;
        };
        match touch.phase {
            Phase::Moved => pointer.at += touch.delta * 2.0,
            Phase::Ended if !touch.moved => pointer.clicks += 1,
            _ => {}
        }
    }
}

/// **A part of a screen as a trackpad**: a finger on it moves the pointer the app keeps,
/// twice as far, and a tap on it is a click. The screen under it — one button — hears neither,
/// beside it the button presses, and at home, where the trackpad stands aside, its place is no
/// trackpad.
#[test]
fn a_trackpad_moves_the_apps_pointer() -> fairing::Result<()> {
    let pressed = Rc::new(Cell::new(0_u32));
    let remote = button("remote", &pressed);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(remote);
    })?
    .with_app(Pointer::default());
    h.frames(2);
    let content = h.shell.layout().content;
    let pad = egui::Rect::from_min_size(
        content.max - egui::vec2(260.0, 220.0),
        egui::vec2(240.0, 200.0),
    );
    h.shell
        .add_gesture_region("trackpad", Trackpad { rect: pad })?;
    h.frames(2);
    h.tap(pad.center());
    let clicks_at_home = h.app_mut::<Pointer>().map(|p| p.clicks);
    assert_eq!(clicks_at_home, Some(0), "the trackpad answered at home");

    h.shell.launch(LaunchAction::open("remote"));
    h.frames(5);
    let from = pad.center();
    h.drag(from, from + egui::vec2(30.0, -20.0), 10);
    h.frames(2);
    let pointer = h
        .app_mut::<Pointer>()
        .ok_or_else(|| fail("the pointer is gone"))?;
    assert!(
        (pointer.at.to_vec2() - egui::vec2(60.0, -40.0)).length() < 0.5,
        "the pointer did not go twice as far as the finger: {:?}",
        pointer.at
    );
    assert_eq!(pointer.clicks, 0, "a drag clicked");
    h.tap(pad.center());
    let clicks = h.app_mut::<Pointer>().map(|p| p.clicks);
    assert_eq!(clicks, Some(1), "a tap on the trackpad did not click");
    // Out and back to where it came down: it went somewhere, so it is no click.
    h.press(from);
    h.frame();
    h.move_to(from + egui::vec2(40.0, 0.0));
    h.frame();
    h.move_to(from);
    h.frame();
    h.release(from);
    h.frames(2);
    let clicks = h.app_mut::<Pointer>().map(|p| p.clicks);
    assert_eq!(clicks, Some(1), "out and back clicked");
    assert_eq!(pressed.get(), 0, "the button under the trackpad heard it");
    h.tap(content.min + egui::vec2(80.0, 80.0));
    assert_eq!(
        pressed.get(),
        1,
        "the button beside the trackpad lost a tap"
    );
    let clicks = h.app_mut::<Pointer>().map(|p| p.clicks);
    assert_eq!(clicks, Some(1), "a tap beside the trackpad clicked");
    Ok(())
}

/// The middle of the content, 200 points a side.
fn middle(h: &Harness) -> egui::Rect {
    egui::Rect::from_center_size(h.shell.layout().content.center(), egui::vec2(200.0, 200.0))
}

/// **A region hears its touch from the press to the release**: `Started` where it came down,
/// `Moved` every frame after — a finger at rest included, with the time it has been held — and
/// `Ended`, where it was let go. A touch that began outside the region is none of its business,
/// wherever it goes.
#[test]
fn a_region_hears_its_touch_to_its_end() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.frames(2);
    let from = rect.center();
    h.press(from);
    h.frame();
    for _ in 0..10 {
        h.move_to(from);
        h.frame();
    }
    h.move_to(from + egui::vec2(40.0, 0.0));
    h.frame();
    h.release(from + egui::vec2(40.0, 0.0));
    h.frame();
    {
        let heard = log.borrow();
        assert_eq!(heard.phases.first(), Some(&Phase::Started));
        assert_eq!(heard.phases.last(), Some(&Phase::Ended));
        assert_eq!(heard.phases.len(), 13, "{:?}", heard.phases);
        let rested = heard
            .touches
            .get(10)
            .ok_or_else(|| fail("no tenth frame"))?;
        assert!(
            rested.phase == Phase::Moved
                && !rested.moved
                && rested.held.as_millis() >= 150
                && (rested.origin - from).length() < 0.5,
            "{rested:?}"
        );
        let went = heard.touches.get(11).ok_or_else(|| fail("no move"))?;
        assert!(
            went.moved && (went.delta - egui::vec2(40.0, 0.0)).length() < 0.5,
            "{went:?}"
        );
    }
    log.borrow_mut().phases.clear();
    let outside = rect.min - egui::vec2(40.0, 40.0);
    h.drag(outside, rect.center(), 8);
    h.frames(2);
    assert!(
        log.borrow().phases.is_empty(),
        "a touch from outside reached it"
    );
    Ok(())
}

/// **A press let go within its own frame** — a quick tap on a slow frame — is heard whole:
/// `Started`, then `Ended`.
#[test]
fn a_quick_tap_on_a_slow_frame_is_heard_whole() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.frames(2);
    h.press(rect.center());
    h.release(rect.center());
    h.frame();
    h.frames(2);
    assert_eq!(log.borrow().phases, [Phase::Started, Phase::Ended]);
    Ok(())
}

/// **A region runs the shell's actions** once it has heard the touch — and a gated one asks to
/// unlock first.
#[test]
fn a_region_runs_actions_and_a_gated_one_asks_first() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let rect = middle(&h);
    h.shell.add_gesture_region(
        "go",
        Probe::new(rect, &Log::default()).then(Phase::Ended, LaunchAction::open("a"), None),
    )?;
    h.frames(2);
    h.tap(rect.center());
    h.frames(3);
    assert_eq!(focused(&h), Some("a"));

    let mut config = access_config(&["viewer", "service"], Some("top"));
    config
        .access
        .gates
        .insert("service".to_owned(), "service".to_owned());
    let mut h = shell(config)?;
    h.shell.add_gesture_region(
        "go",
        Probe::new(rect, &Log::default()).then(
            Phase::Ended,
            LaunchAction::open("service_menu"),
            Some("service"),
        ),
    )?;
    h.frames(2);
    h.tap(rect.center());
    let events = h.shell.poll_events();
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

/// What a region was placed by, each frame: the focused screen, and its pane.
type Told = Rc<RefCell<Vec<(Option<String>, egui::Rect)>>>;

/// A probe that also notes what it is placed by.
struct Placed(Probe, Told);

impl GestureRegion for Placed {
    fn place(&mut self, cx: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        self.1
            .borrow_mut()
            .push((cx.focused.map(str::to_owned), cx.pane));
        self.0.place(cx)
    }

    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        self.0.touch(touch, cx);
    }
}

/// **A region is placed where it says, and stands aside where it says**: it is asked every
/// frame, and a frame it answers `None` its place is the screen's. Asked over a screen, it is
/// told which screen and where its pane is.
#[test]
fn a_region_stands_aside_where_it_says() -> fairing::Result<()> {
    let pressed = Rc::new(Cell::new(0_u32));
    let target = button("target", &pressed);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(target);
    })?;
    h.frames(2);
    let told = Told::default();
    let log = Log::default();
    let rect = middle(&h);
    let probe = Probe::new(rect, &log);
    let here = Rc::clone(&probe.here);

    h.shell
        .add_gesture_region("probe", Placed(probe, Rc::clone(&told)))?;
    h.shell.launch(LaunchAction::open("target"));
    h.frames(5);
    let last = told.borrow().last().cloned();
    assert_eq!(
        last,
        Some((Some("target".to_owned()), h.shell.layout().content)),
        "not told the screen and its pane"
    );
    h.tap(rect.center());
    assert_eq!(pressed.get(), 0, "the button heard a tap in the region");
    assert_eq!(log.borrow().phases, [Phase::Started, Phase::Ended]);
    here.set(false);
    h.frames(3);
    h.tap(rect.center());
    assert_eq!(pressed.get(), 1, "the region did not stand aside");
    assert_eq!(log.borrow().phases.len(), 2, "a region aside heard a touch");
    Ok(())
}

/// **Regions rest over the shell's own surfaces and with gestures off**: over the
/// recent screens, and with `[gesture] enabled = false`, a region is not even asked where it is,
/// and a tap in its place reaches what is under it.
#[test]
fn regions_rest_over_the_cards_and_with_gestures_off() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(5);
    if !h.shell.workspace().is_overview_open() {
        return Err(fail("the recent screens did not come up"));
    }
    let placed = log.borrow().placed;
    h.frames(3);
    assert_eq!(
        log.borrow().placed,
        placed,
        "asked where it is over the cards"
    );
    // A tap over the cards (which may well put them away) is the cards'.
    h.tap(rect.center());
    assert!(
        log.borrow().phases.is_empty(),
        "it heard a touch over the cards"
    );

    let pressed = Rc::new(Cell::new(0_u32));
    let target = button("target", &pressed);
    let mut config = single_level_access();
    config.gesture.enabled = false;
    let mut h = test_shell(config, move |sh| {
        sh.add(target);
    })?;
    h.frames(2);
    let log = Log::default();
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.shell.launch(LaunchAction::open("target"));
    h.frames(5);
    h.tap(rect.center());
    assert_eq!(
        log.borrow().placed,
        0,
        "asked where it is with gestures off"
    );
    assert!(
        log.borrow().phases.is_empty(),
        "it heard a touch with gestures off"
    );
    assert_eq!(pressed.get(), 1, "the screen did not get the tap");
    Ok(())
}

/// **Nothing answers in a region over the shade**.
#[cfg(feature = "overlay")]
#[test]
fn nothing_answers_in_a_region_over_the_shade() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(10);
    if h.shell.overlay().is_closed() {
        return Err(fail("the shade did not open"));
    }
    h.tap(rect.center());
    h.frames(2);
    assert!(
        log.borrow().phases.is_empty(),
        "it heard a touch over the shade"
    );
    Ok(())
}

/// **A touch the shade takes is told so**: the shade opening over a region mid-touch
/// takes the touch, and the region hears it `Cancelled`.
#[cfg(feature = "overlay")]
#[test]
fn a_touch_the_shade_takes_is_cancelled() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.frames(2);
    let from = rect.center();
    h.press(from);
    h.frame();
    h.move_to(from + egui::vec2(20.0, 0.0));
    h.frame();
    h.shell.launch(LaunchAction::OpenOverlay);
    for step in 1..=10_u8 {
        h.move_to(from + egui::vec2(20.0 + f32::from(step), 0.0));
        h.frame();
    }
    if h.shell.overlay().is_closed() {
        return Err(fail("the shade did not open"));
    }
    h.release(from + egui::vec2(30.0, 0.0));
    h.frames(2);
    let phases = log.borrow().phases.clone();
    assert_eq!(
        phases.last(),
        Some(&Phase::Cancelled),
        "the region was not told the shade took its touch: {phases:?}"
    );
    assert!(!phases.contains(&Phase::Ended), "{phases:?}");
    Ok(())
}

/// **A touch taken from a region is told so**: a gated action a touch runs brings up
/// the unlock prompt over the regions, and the region hears its touch `Cancelled`, then nothing
/// more of that press.
#[test]
fn a_touch_taken_from_a_region_is_cancelled() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "operator"], Some("top"));
    config.access.pin_table.pins = [("operator".to_owned(), "1234".to_owned())]
        .into_iter()
        .collect();
    config
        .access
        .gates
        .insert("service".to_owned(), "operator".to_owned());
    let mut h = shell(config)?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell.add_gesture_region(
        "probe",
        Probe::new(rect, &log).then(
            Phase::Started,
            LaunchAction::open("service_menu"),
            Some("service"),
        ),
    )?;
    h.frames(2);
    let from = rect.center();
    h.press(from);
    h.frame();
    for step in 1..=5_u8 {
        h.move_to(from + egui::vec2(4.0 * f32::from(step), 0.0));
        h.frame();
    }
    if !h.shell.unlock_prompt_visible() {
        return Err(fail("the gated action did not bring up the prompt"));
    }
    h.release(from + egui::vec2(20.0, 0.0));
    h.frames(2);
    let phases = log.borrow().phases.clone();
    assert_eq!(phases.first(), Some(&Phase::Started), "{phases:?}");
    assert_eq!(
        phases.last(),
        Some(&Phase::Cancelled),
        "the region was not told its touch was taken: {phases:?}"
    );
    assert_eq!(
        phases.iter().filter(|p| **p == Phase::Cancelled).count(),
        1,
        "{phases:?}"
    );
    assert!(!phases.contains(&Phase::Ended), "{phases:?}");
    Ok(())
}

/// **A press in a region is its own to the release**, wherever the region is after: one that
/// stands aside mid-touch still hears the touch end. One taken off mid-touch takes the press
/// with it: a region with its id, added in its place, hears nothing of it.
#[test]
fn a_press_stays_with_its_region() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    let probe = Probe::new(rect, &log);
    let here = Rc::clone(&probe.here);
    h.shell.add_gesture_region("probe", probe)?;
    h.frames(2);
    let from = rect.center();
    h.press(from);
    h.frame();
    here.set(false);
    h.move_to(from + egui::vec2(30.0, 0.0));
    h.frame();
    h.release(from + egui::vec2(30.0, 0.0));
    h.frame();
    assert_eq!(
        log.borrow().phases,
        [Phase::Started, Phase::Moved, Phase::Ended]
    );

    let mut h = shell(single_level_access())?;
    let (first, second) = (Log::default(), Log::default());
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &first))?;
    h.frames(2);
    h.press(from);
    h.frame();
    assert!(h.shell.remove_gesture_region("probe"));
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &second))?;
    h.move_to(from + egui::vec2(30.0, 0.0));
    h.frame();
    h.release(from + egui::vec2(30.0, 0.0));
    h.frames(2);
    assert_eq!(first.borrow().phases, [Phase::Started]);
    assert!(
        second.borrow().phases.is_empty(),
        "the new region heard the old one's press: {:?}",
        second.borrow().phases
    );
    Ok(())
}

/// **The emergency gesture works through a region**: two seconds still in a top
/// corner, under a region over the whole glass.
#[test]
fn the_emergency_gesture_works_through_a_region() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let glass = h.screen_rect();
    h.shell.add_gesture_region("all", Probe::new(glass, &log))?;
    h.frames(2);
    h.hold(egui::pos2(glass.max.x - 14.0, glass.min.y + 10.0), 121);
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(e, ShellEvent::Emergency)),
        "no emergency through the region: {events:?}"
    );
    assert_eq!(log.borrow().phases.first(), Some(&Phase::Started));
    Ok(())
}

/// **A region is drawn with its rect, and its touch while it has one.**
#[test]
fn a_region_is_drawn_with_its_rect_and_touch() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let log = Log::default();
    let rect = middle(&h);
    h.shell
        .add_gesture_region("probe", Probe::new(rect, &log))?;
    h.frames(2);
    assert_eq!(log.borrow().painted, Some((rect, None)));
    let from = rect.center();
    h.press(from);
    h.frame();
    h.move_to(from + egui::vec2(10.0, 5.0));
    h.frame();
    let painted = log.borrow().painted;
    assert!(
        matches!(painted, Some((r, Some(t))) if r == rect
            && t.phase == Phase::Moved
            && (t.pos - (from + egui::vec2(10.0, 5.0))).length() < 0.5),
        "{painted:?}"
    );
    h.release(from + egui::vec2(10.0, 5.0));
    h.frames(2);
    assert_eq!(log.borrow().painted, Some((rect, None)));
    Ok(())
}

/// **Ids are one namespace for the handles and the regions; eight regions at most** — and a
/// region with an id already there replaces the old one in its place.
#[test]
fn regions_are_checked_when_added() -> fairing::Result<()> {
    let mut h = shell(single_level_access())?;
    let rect = middle(&h);
    let probe = || Probe::new(rect, &Log::default());
    h.shell
        .add_gesture_handle(GestureHandle::new("edge", Edge::Right))?;
    assert!(
        h.shell.add_gesture_region("edge", probe()).is_err(),
        "a region took a handle's id"
    );
    h.shell.add_gesture_region("r1", probe())?;
    assert!(
        h.shell
            .add_gesture_handle(GestureHandle::new("r1", Edge::Left))
            .is_err(),
        "a handle took a region's id"
    );
    for n in 2..=MAX_GESTURE_REGIONS {
        h.shell.add_gesture_region(format!("r{n}"), probe())?;
    }
    assert!(
        h.shell.add_gesture_region("one-more", probe()).is_err(),
        "a ninth region"
    );
    h.shell.add_gesture_region("r1", probe())?;
    let ids: Vec<&str> = h.shell.gesture_regions().collect();
    assert_eq!(ids, ["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8"]);
    assert!(
        !h.shell.remove_gesture_region("edge"),
        "a handle went as a region"
    );
    assert!(
        !h.shell.remove_gesture_handle("r2"),
        "a region went as a handle"
    );
    assert!(h.shell.remove_gesture_region("r2"));
    assert!(!h.shell.remove_gesture_region("r2"), "it was there twice");
    assert_eq!(h.shell.gesture_regions().count(), 7);
    assert_eq!(h.shell.gesture_handles().collect::<Vec<_>>(), ["edge"]);
    Ok(())
}

/// The right edge's handle: `a` straight in.
fn right() -> GestureHandle {
    GestureHandle::new("right", Edge::Right).gesture(
        HandleGesture::short(HandleDirection::Straight),
        LaunchAction::open("a"),
    )
}

/// **The later of a region and a handle is on top**: a region added after a handle
/// takes the presses in the strip it covers, and a handle added after a region takes its strip
/// back.
#[test]
fn the_later_of_a_region_and_a_handle_is_on_top() -> fairing::Result<()> {
    let ran = |h: &mut Harness| {
        h.shell
            .poll_events()
            .into_iter()
            .any(|e| matches!(e, ShellEvent::Gesture { .. }))
    };
    let mut h = shell(single_level_access())?;
    let content = h.shell.layout().content;
    let glass = h.screen_rect();
    let side = egui::Rect::from_min_max(
        egui::pos2(glass.max.x - 120.0, content.min.y),
        egui::pos2(glass.max.x, content.max.y),
    );
    let from = egui::pos2(glass.max.x - 1.0, content.center().y);
    let reach = h.shell.scale().mm_to_du(10.0);
    let swipe_in = |h: &mut Harness| {
        h.drag(from, from - egui::vec2(reach * 2.0, 0.0), 8);
        h.frames(2);
    };

    let log = Log::default();
    h.shell.add_gesture_handle(right())?;
    h.shell.add_gesture_region("side", Probe::new(side, &log))?;
    h.frames(2);
    let _ = h.shell.poll_events();
    swipe_in(&mut h);
    assert!(!ran(&mut h), "the handle under the region ran");
    assert_eq!(log.borrow().phases.first(), Some(&Phase::Started));

    let mut h = shell(single_level_access())?;
    let log = Log::default();
    h.shell.add_gesture_region("side", Probe::new(side, &log))?;
    h.shell.add_gesture_handle(right())?;
    h.frames(2);
    let _ = h.shell.poll_events();
    swipe_in(&mut h);
    assert!(ran(&mut h), "the handle over the region did not run");
    assert!(
        log.borrow().phases.is_empty(),
        "the region under the handle heard its swipe"
    );
    Ok(())
}
