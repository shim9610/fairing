//! Regression tests for the workspace fixes before 0.1.0: lifecycle around replaced and removed
//! resident screens, back gestures that something else interrupts, covered screens, the overview's
//! way back, background resizes and the builder's chrome shortcuts.

use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::workspace::Instance;
use fairing::{screen, screen_of, screen_with, Cx, LaunchAction, Lifecycle, Screen, ShellEvent};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

type Log = Rc<RefCell<Vec<(String, Lifecycle)>>>;

/// Writes every `on_lifecycle` down with its tag.
struct Rec {
    tag: String,
    log: Log,
}

impl Screen for Rec {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label(self.tag.as_str());
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push((self.tag.clone(), event));
    }
}

fn plain(id: &'static str) -> fairing::ScreenDecl {
    screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
        ui.label(id);
    })
}

fn drag_by(h: &mut Harness, from: egui::Pos2, step: egui::Vec2, frames: usize) -> egui::Pos2 {
    let mut pos = from;
    for _ in 0..frames {
        pos += step;
        h.move_to(pos);
        h.frame();
    }
    pos
}

fn closed_ids(events: &[ShellEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            ShellEvent::ScreenClosed { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect()
}

// ── Replaced and removed resident screens ───────────────────────────────────────────────────────

/// Replacing an open resident screen tells the replaced screen `Destroyed`, never its replacement.
#[test]
fn replacing_a_resident_screen_sends_destroyed_to_the_replaced_one() -> fairing::Result<()> {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let old = Rec {
        tag: "old".into(),
        log: Rc::clone(&log),
    };
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(screen_of("x", old));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("x"));
    h.frames(3);
    assert!(h.shell.workspace().find("x").is_some());
    log.borrow_mut().clear();

    h.shell.add(screen_of(
        "x",
        Rec {
            tag: "new".into(),
            log: Rc::clone(&log),
        },
    ));
    h.frames(3);
    let got = log.borrow().clone();
    assert!(
        !got.iter()
            .any(|(tag, e)| tag == "new" && *e == Lifecycle::Destroyed),
        "the replacement never had an instance, yet it was told Destroyed: {got:?}"
    );
    assert!(
        got.iter()
            .any(|(tag, e)| tag == "old" && *e == Lifecycle::Destroyed),
        "the replaced screen's instance closed, but it never heard Destroyed: {got:?}"
    );
    Ok(())
}

/// Removing an open resident screen delivers `Destroyed` to it.
#[test]
fn removing_an_open_resident_screen_delivers_destroyed() -> fairing::Result<()> {
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let rec = Rec {
        tag: "x".into(),
        log: Rc::clone(&log),
    };
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(screen_of("x", rec));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("x"));
    h.frames(3);
    log.borrow_mut().clear();
    assert!(h.shell.remove("x"));
    h.frames(3);
    let got = log.borrow().clone();
    assert!(
        got.iter().any(|(_, e)| *e == Lifecycle::Destroyed),
        "removed while open: no Destroyed reached the screen: {got:?}"
    );
    Ok(())
}

// ── A back gesture ended under the finger ───────────────────────────────────────────────────────

/// A back release whose gesture was already ended reports no still-open screen closed.
#[test]
fn an_interrupted_back_gesture_does_not_report_the_screen_under_it_closed() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(plain("a"));
        sh.add(plain("b"));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    let _ = h.shell.poll_events();

    let from = egui::pos2(4.0, 300.0);
    h.press(from);
    h.frame();
    let mid = drag_by(&mut h, from, egui::vec2(15.0, 0.0), 10);
    // A back from outside lands mid-gesture.
    h.shell.handle().back();
    let end = drag_by(&mut h, mid, egui::vec2(15.0, 0.0), 6);
    h.release(end);
    h.frames(4);

    let events = h.shell.poll_events();
    let closed = closed_ids(&events);
    assert!(
        h.shell.workspace().find("a").is_some(),
        "`a` is still open (it was the root)"
    );
    assert!(
        !closed.iter().any(|id| id == "a"),
        "ScreenClosed for `a`, which is still open: {closed:?}"
    );
    Ok(())
}

// ── Background resizes ──────────────────────────────────────────────────────────────────────────

/// A background task hears one `Resized` however often the content changed size.
#[test]
fn a_background_task_does_not_pile_up_stale_resizes() -> fairing::Result<()> {
    let seen: Rc<RefCell<Vec<Option<Lifecycle>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("bg", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            sink.borrow_mut().push(cx.event);
            ui.label("bg");
        }));
        sh.add(plain("fg"));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("bg"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("fg"));
    h.frames(3);

    // The content flips between two widths 40 times while `bg` sits in the background.
    for i in 0..40 {
        let w = if i % 2 == 0 { 1000.0 } else { 1024.0 };
        h.set_size(w, 600.0);
        h.frame();
    }
    seen.borrow_mut().clear();
    // Bring `bg` back.
    h.shell.launch(LaunchAction::open("bg"));
    h.frames(60);
    let resizes = seen
        .borrow()
        .iter()
        .filter(|e| matches!(e, Some(Lifecycle::Resized(_))))
        .count();
    assert!(
        resizes <= 2,
        "`bg` was fed {resizes} stale Resized events, one per frame, after coming back"
    );
    Ok(())
}

// ── Home through the overview ───────────────────────────────────────────────────────────────────

/// "Close all" in the overview reports `WentHome`.
#[test]
fn close_all_reports_went_home() -> fairing::Result<()> {
    let mut h = test_shell(fairing::ShellConfig::default(), |sh| {
        sh.add(plain("a"));
        sh.add(plain("b"));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
    let _ = h.shell.poll_events();
    let button = h
        .shell
        .workspace()
        .overview_close_all_rect()
        .ok_or_else(|| fail("no Close all"))?;
    h.tap(button.center());
    h.frames(3);
    assert!(h.shell.workspace().is_home());
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(e, ShellEvent::WentHome)),
        "home reached by Close all, but no WentHome: {events:?}"
    );
    Ok(())
}

// ── The other pane acting on its own ────────────────────────────────────────────────────────────

/// The other pane opening a screen on its own leaves the user's back swipe running.
#[test]
fn the_other_pane_opening_on_its_own_keeps_the_users_back_gesture() -> fairing::Result<()> {
    back_swipe_left_pane(true)
}

/// A back swipe in the left pane of a split pops that pane.
#[test]
fn a_back_swipe_in_the_left_pane_pops() -> fairing::Result<()> {
    back_swipe_left_pane(false)
}

fn back_swipe_left_pane(interrupt: bool) -> fairing::Result<()> {
    use std::cell::Cell;
    let go = Rc::new(Cell::new(false));
    let g = Rc::clone(&go);
    let mut h = test_shell(fairing::ShellConfig::default(), move |sh| {
        sh.add(plain("a"));
        sh.add(plain("b"));
        sh.add(plain("c"));
        sh.add(screen(
            "watch",
            move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                ui.label("watch");
                if g.replace(false) {
                    cx.open("c");
                }
            },
        ));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::Open {
        id: "watch".into(),
        in_other_pane: true,
    });
    h.frames(3);
    if !h.shell.workspace().is_split() {
        return Err(fail("no split"));
    }
    let left = h
        .shell
        .workspace()
        .pane_rect(0)
        .ok_or_else(|| fail("no left pane"))?;
    h.tap(left.center());
    h.frames(2);
    if h.shell.workspace().focused_pane() != 0 {
        return Err(fail("left pane not focused"));
    }
    let _ = h.shell.poll_events();

    // Back-swipe the left pane: `b` follows the finger over `a`.
    let from = egui::pos2(left.min.x + 4.0, left.center().y);
    h.press(from);
    h.frame();
    let mid = drag_by(&mut h, from, egui::vec2(12.0, 0.0), 8);
    // The other pane's screen opens something on its own, mid-gesture.
    go.set(interrupt);
    let end = drag_by(&mut h, mid, egui::vec2(12.0, 0.0), 6);
    h.release(end);
    h.frames(4);

    let ws = h.shell.workspace();
    let left_top = ws
        .pane_task(0)
        .and_then(fairing::workspace::Task::top)
        .map(Instance::decl_id)
        .map(str::to_owned);
    let events = h.shell.poll_events();
    let closed = closed_ids(&events);
    assert_eq!(
        left_top.as_deref(),
        Some("a"),
        "the user's confirmed back swipe should have popped `b` (closed events: {closed:?})"
    );
    Ok(())
}

// ── A pop under the open shade ──────────────────────────────────────────────────────────────────

/// A screen uncovered by a pop under the open shade is not told `Resumed`.
#[cfg(feature = "overlay")]
#[test]
fn a_pop_under_the_open_shade_does_not_resume_the_screen_below() -> fairing::Result<()> {
    use std::cell::Cell;
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let a_log = Rc::clone(&log);
    let quit = Rc::new(Cell::new(false));
    let q = Rc::clone(&quit);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen_with("a", move || Rec {
            tag: "a".into(),
            log: Rc::clone(&a_log),
        }));
        sh.add(screen("b", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            ui.label("b");
            if q.replace(false) {
                cx.finish();
            }
        }));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    if h.shell.overlay().is_closed() {
        return Err(fail("the shade did not open"));
    }
    log.borrow_mut().clear();
    quit.set(true);
    h.frames(4);
    if h.shell.workspace().find("b").is_some() {
        return Err(fail("`b` did not finish"));
    }
    assert!(!h.shell.overlay().is_closed(), "the shade is still open");
    let got = log.borrow().clone();
    assert_ne!(
        got.last().map(|(_, e)| *e),
        Some(Lifecycle::Resumed),
        "`a` is under the open shade, yet its last event is Resumed: {got:?}"
    );
    Ok(())
}

// ── Chrome shortcuts in any order ───────────────────────────────────────────────────────────────

/// `.keep_awake()` before `.fullscreen()` is kept.
#[test]
fn keep_awake_survives_a_later_fullscreen() {
    let decl = plain("video").keep_awake().fullscreen();
    assert!(
        decl.chrome_policy().keep_awake,
        "`.keep_awake().fullscreen()` lost keep_awake"
    );
}

/// `.osk(..)` before `.fullscreen()` is kept.
#[test]
fn osk_mode_survives_a_later_fullscreen() {
    let decl = plain("kiosk").osk(fairing::OskMode::Off).fullscreen();
    assert_eq!(
        decl.chrome_policy().osk,
        fairing::OskMode::Off,
        "`.osk(Off).fullscreen()` lost the keyboard mode"
    );
}

// ── A thrown card and the way back ──────────────────────────────────────────────────────────────

/// Throwing the last card away in the overview reports `WentHome`.
#[test]
fn throwing_the_last_card_away_reports_went_home() -> fairing::Result<()> {
    let mut h = test_shell(fairing::ShellConfig::default(), |sh| {
        sh.add(plain("a"));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(0.6);
    h.shell.launch(LaunchAction::OpenOverview);
    h.run_for(0.6);
    let card = h
        .shell
        .workspace()
        .overview_cards_drawn()
        .first()
        .copied()
        .ok_or_else(|| fail("no card"))?;
    let _ = h.shell.poll_events();
    let from = card.rect.center();
    h.drag(from, from - egui::vec2(0.0, 300.0), 3);
    h.run_for(1.5);
    assert!(
        h.shell.workspace().find("a").is_none(),
        "the card was thrown away"
    );
    assert!(h.shell.workspace().is_home());
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(e, ShellEvent::WentHome)),
        "home reached by throwing the last card away, but no WentHome: {events:?}"
    );
    Ok(())
}

/// A card still flying when back takes the cards down ends its task.
#[test]
fn a_card_still_flying_when_back_takes_the_cards_down_ends_its_task() -> fairing::Result<()> {
    throw_then_back(Some(600))
}

/// A card thrown then back, with the default timings, ends its task.
#[test]
fn a_throw_then_back_with_default_timings_ends_the_task() -> fairing::Result<()> {
    throw_then_back(None)
}

fn throw_then_back(throw_ms: Option<u64>) -> fairing::Result<()> {
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(fairing::ShellConfig::default(), services)?;
    for id in ["a", "b"] {
        h.shell.add(plain(id));
    }
    if let Some(ms) = throw_ms {
        let mut tokens = h.shell.theme().motion;
        tokens.overview_throw = fairing::motion::Tween {
            duration: std::time::Duration::from_millis(ms),
            easing: tokens.overview_throw.easing,
        };
        h.shell.set_motion(tokens);
    }
    h.frames(2);
    for id in ["a", "b"] {
        h.shell.launch(LaunchAction::open(id));
        h.run_for(0.6);
        h.shell.home();
        h.run_for(0.6);
    }
    h.shell.launch(LaunchAction::open("b"));
    h.run_for(0.6);
    h.shell.launch(LaunchAction::OpenOverview);
    h.run_for(0.6);
    let key = h
        .shell
        .workspace()
        .tasks()
        .iter()
        .find(|t| t.root_id() == Some("b"))
        .and_then(|t| t.iter().next().map(Instance::id))
        .ok_or_else(|| fail("no task b"))?;
    let card = h
        .shell
        .workspace()
        .overview_cards_drawn()
        .iter()
        .find(|c| c.key == key)
        .copied()
        .ok_or_else(|| fail("no card b"))?;
    let from = card.rect.center();
    h.drag(from, from - egui::vec2(0.0, 300.0), 3);
    h.frames(1);
    if h.shell.workspace().find("b").is_none() {
        return Err(fail("the throw landed too early for this test"));
    }
    h.shell.back();
    h.run_for(1.5);
    assert!(
        !h.shell.workspace().is_overview_open(),
        "the cards went back down"
    );
    assert!(
        h.shell.workspace().find("b").is_none(),
        "the thrown card's task survived the way back"
    );
    Ok(())
}

// ── Eviction under the top ──────────────────────────────────────────────────────────────────────

/// Evicting an instance deep under the top does not snap the push running above it.
#[test]
fn an_eviction_under_the_top_does_not_snap_the_push() -> fairing::Result<()> {
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    let l = Rc::clone(&log);
    h.shell.add(
        screen_with("a", move || Rec {
            tag: "a".into(),
            log: Rc::clone(&l),
        })
        .evict_after(std::time::Duration::from_secs(1)),
    );
    h.shell.add(plain("b"));
    h.shell.add(plain("c"));
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(0.8);
    h.shell.launch(LaunchAction::open("b"));
    let push = h.shell.theme().motion.push.duration.as_secs_f64();
    h.run_for(push + 0.18); // the push has landed: `a` is Stopped under `b`
    h.run_for(0.5); // about 0.7 s after `a` stopped, inside its 1 s eviction
                    // A long push for `c` (1 s), so that `a`'s deadline falls inside it.
    let mut tokens = h.shell.theme().motion;
    tokens.push.duration = std::time::Duration::from_secs(1);
    h.shell.set_motion(tokens);
    h.shell.launch(LaunchAction::open("c"));
    // Step through the push; note whether `a` went while it was still running.
    let mut frames_animating = 0;
    let mut evicted_mid_push = false;
    for _ in 0..120 {
        let had_a = h.shell.workspace().find("a").is_some();
        h.frame();
        let animating = h.shell.workspace().is_animating();
        if animating {
            frames_animating += 1;
        }
        if had_a && h.shell.workspace().find("a").is_none() {
            evicted_mid_push = frames_animating > 0;
            // The push is 1 s (~60 frames). Right after the eviction it must still be running.
            assert!(
                animating,
                "`a` was evicted under the top and the push jumped to its end \
                 after {frames_animating} frames"
            );
        }
        if !animating && frames_animating > 0 {
            break;
        }
    }
    if !evicted_mid_push {
        return Err(fail(
            "`a` was not evicted during the push - timing of the test is off",
        ));
    }
    Ok(())
}
