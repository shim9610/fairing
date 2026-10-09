//! M6 — the gesture navigation (A11). With `[nav_bar] style =
//! "gesture"` the nav bar is a band with the home indicator: up from the bottom edge is home —
//! the screen on show following the finger — up and a pause is the recent screens, along the
//! indicator is the task used before or after, and back stays a `back_edges` swipe.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::testing::{access_config, test_shell, Harness};
use fairing::workspace::{HomeTransition, Instance};
use fairing::{
    screen, screen_with, AccessEvent, ChromePolicy, Cx, LaunchAction, Lifecycle, Screen,
    ShellConfig, ShellEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

type Log = Rc<RefCell<Vec<Lifecycle>>>;

/// A screen that writes its lifecycle down.
struct Recorder {
    log: Log,
}

impl Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("r");
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push(event);
    }
}

fn gestures() -> ShellConfig {
    let mut config = ShellConfig::default();
    "gesture".clone_into(&mut config.nav_bar.style);
    config
}

/// A shell with screens `a`, `b` and `c`, and `a`'s and `b`'s lifecycles written down.
fn shell(config: ShellConfig) -> fairing::Result<(Harness, Log, Log)> {
    let (a_log, b_log): (Log, Log) = (Rc::default(), Rc::default());
    let (a, b) = (Rc::clone(&a_log), Rc::clone(&b_log));
    let mut h = test_shell(config, move |sh| {
        sh.add(screen_with("a", move || Recorder { log: Rc::clone(&a) }).title("A"));
        sh.add(screen_with("b", move || Recorder { log: Rc::clone(&b) }).title("B"));
        sh.add(
            screen("c", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("c");
            })
            .title("C"),
        );
    })?;
    h.frames(2);
    Ok((h, a_log, b_log))
}

/// `a` then `b`, each from home — two tasks, `b` on show.
fn two_tasks(h: &mut Harness) {
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
}

fn on_show(h: &Harness) -> Option<String> {
    h.shell
        .workspace()
        .focused()
        .map(|i| Instance::decl_id(i).to_owned())
}

/// Just above the screen's bottom edge — inside the indicator's band.
fn bottom(h: &Harness) -> f32 {
    h.screen_rect().max.y - 6.0
}

/// Press on the bottom edge at `x` and move up `dy` over `steps` frames, still holding.
fn lift(h: &mut Harness, x: f32, dy: f32, steps: usize) -> egui::Pos2 {
    let from = egui::pos2(x, bottom(h));
    h.press(from);
    h.frame();
    let mut at = from;
    for i in 1..=steps {
        // Step counts are tiny, so the conversions are exact.
        #[allow(clippy::cast_precision_loss)]
        let t = i as f32 / steps as f32;
        at = from + egui::vec2(0.0, -dy * t);
        h.move_to(at);
        h.frame();
    }
    at
}

fn let_go(h: &mut Harness, at: egui::Pos2) {
    h.release(at);
    h.frame();
    h.frames(3);
}

/// Along the indicator from its middle by `dx` over `steps` frames, then let go.
fn slide(h: &mut Harness, dx: f32, steps: usize) {
    let from = egui::pos2(h.screen_rect().center().x, bottom(h));
    h.drag(from, from + egui::vec2(dx, 0.0), steps);
    h.frames(3);
}

fn events(h: &mut Harness) -> Vec<ShellEvent> {
    h.shell.poll_events()
}

/// The band carries the home indicator — a pill the size of its tokens — and the buttons style
/// does not.
#[test]
fn the_gesture_bar_draws_the_home_indicator() -> fairing::Result<()> {
    fn pills(h: &mut Harness) -> usize {
        let m = h.shell.theme().metrics;
        let mut found = 0;
        for clipped in h.frame_shapes() {
            if let egui::Shape::Rect(r) = &clipped.shape {
                if (r.rect.width() - m.nav_indicator_length).abs() < 0.5
                    && (r.rect.height() - m.nav_indicator_thickness).abs() < 0.5
                {
                    found += 1;
                }
            }
        }
        found
    }
    let (mut h, ..) = shell(gestures())?;
    assert_eq!(pills(&mut h), 1);
    let (mut buttons, ..) = shell(ShellConfig::default())?;
    assert_eq!(pills(&mut buttons), 0);
    Ok(())
}

/// Up from the bottom edge and let go: home. The task stays alive, and it says so.
#[test]
fn up_from_the_bottom_edge_goes_home() -> fairing::Result<()> {
    let (mut h, a_log, _) = shell(gestures())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let _ = events(&mut h);
    let at = lift(&mut h, 400.0, 300.0, 5);
    assert!(h.shell.workspace().is_lifting(), "the screen follows");
    let_go(&mut h, at);
    assert!(h.shell.workspace().is_home());
    assert_eq!(h.shell.workspace().tasks().len(), 1, "the task lives on");
    assert!(events(&mut h)
        .iter()
        .any(|e| matches!(e, ShellEvent::WentHome)));
    let log = a_log.borrow().clone();
    assert_eq!(
        log.iter().filter(|e| **e == Lifecycle::Paused).count(),
        1,
        "{log:?}"
    );
    assert_eq!(log.last(), Some(&Lifecycle::Stopped), "{log:?}");
    Ok(())
}

/// The screen follows the finger: the higher, the further along — and let go short of the
/// threshold, slowly, it goes back down and goes on.
#[test]
fn a_short_slow_swipe_goes_back_down() -> fairing::Result<()> {
    let (mut h, a_log, _) = shell(gestures())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let at = lift(&mut h, 400.0, 24.0, 4);
    let low = h.shell.workspace().lift_progress();
    let at = {
        let higher = at + egui::vec2(0.0, -16.0);
        h.move_to(higher);
        h.frame();
        higher
    };
    let high = h.shell.workspace().lift_progress();
    assert!(high > low && low > 0.0, "{low} → {high}");
    // Stillness first, so the release is slow.
    h.frames(2);
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_home());
    assert!(!h.shell.workspace().is_lifting());
    assert_eq!(on_show(&h).as_deref(), Some("a"));
    let log = a_log.borrow().clone();
    assert_eq!(
        log.iter().rev().take(2).copied().collect::<Vec<_>>(),
        vec![Lifecycle::Resumed, Lifecycle::Paused],
        "paused for the lift, resumed after it: {log:?}"
    );
    Ok(())
}

/// The point the finger went down on stays under it: the screen's layer is scaled about the
/// press and moved with the finger.
#[test]
fn the_point_pressed_stays_under_the_finger() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let press = egui::pos2(400.0, bottom(&h));
    let at = lift(&mut h, 400.0, 120.0, 6);
    let layer = h
        .shell
        .workspace()
        .find("a")
        .map(Instance::layer_id)
        .ok_or_else(|| fail("no screen `a`"))?;
    let t = h
        .ctx
        .layer_transform_to_global(layer)
        .ok_or_else(|| fail("no transform"))?;
    assert!(t.scaling < 1.0, "it shrinks: {t:?}");
    let carried = t * press;
    assert!(
        (carried - at).length() < 1.5,
        "{carried:?} is not under the finger at {at:?}"
    );
    let_go(&mut h, at);
    Ok(())
}

/// Let go into home without `reduce`, the A2 close starts where the finger left the screen —
/// not back at its pane.
#[test]
fn a_lift_let_go_carries_on_into_home() -> fairing::Result<()> {
    let mut config = gestures();
    config.motion.reduce = false;
    let mut h = Harness::new(config, fairing::Services::builder().build())?;
    h.shell.add(
        screen("a", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("a");
        })
        .title("A"),
    );
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(1.0);
    let at = lift(&mut h, 400.0, 260.0, 4);
    h.release(at);
    h.frame();
    assert!(
        matches!(
            h.shell.workspace().home_transition(),
            HomeTransition::Closing { from: Some(_), .. }
        ),
        "{:?}",
        h.shell.workspace().home_transition()
    );
    h.run_for(1.0);
    assert!(h.shell.workspace().is_home() && !h.shell.is_animating());
    Ok(())
}

/// Up and a pause: the recent screens, the lifted screen carried into its card. Letting go after
/// is not home.
#[test]
fn up_and_a_pause_brings_the_recent_screens() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    let _ = events(&mut h);
    let at = lift(&mut h, 400.0, 120.0, 5);
    h.frames(15);
    assert!(h.shell.workspace().is_overview_open());
    assert!(events(&mut h)
        .iter()
        .any(|e| matches!(e, ShellEvent::OverviewRequested)));
    let_go(&mut h, at);
    assert!(h.shell.workspace().is_overview_open() && !h.shell.workspace().is_home());
    Ok(())
}

/// A finger standing still mid-swipe sends nothing, so on a panel that only draws when asked the
/// pause would never be seen: while a swipe is under way the shell keeps frames coming.
#[test]
fn a_swipe_held_still_keeps_the_frames_coming() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    // No clock, so nothing else asks for a frame.
    let _ = h.shell.remove("status.clock");
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let at = lift(&mut h, 400.0, 20.0, 3);
    h.frames(2);
    assert!(
        !h.shell.is_animating(),
        "the lift follows the finger, it does not animate"
    );
    assert!(h.ctx.has_requested_repaint(), "a frame is asked for");
    let_go(&mut h, at);
    h.frames(3);
    assert!(!h.ctx.has_requested_repaint(), "and not once it is over");
    Ok(())
}

/// Held into the overview, the lifted screen carries on into its card from where the finger has
/// it: on the frame the cards take over it is no bigger than it was under the finger — it does
/// not jump back to whole and shrink again.
#[test]
fn a_lift_held_into_the_overview_carries_on_from_where_it_is() -> fairing::Result<()> {
    let mut config = gestures();
    config.motion.reduce = false;
    let mut h = Harness::new(config, fairing::Services::builder().build())?;
    for id in ["a", "b"] {
        h.shell.add(
            screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label(id);
            })
            .title(id),
        );
    }
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(1.0);
    h.shell.home();
    h.run_for(1.0);
    h.shell.launch(LaunchAction::open("b"));
    h.run_for(1.0);
    let scale_of = |h: &Harness| {
        let layer = h.shell.workspace().find("b").map(Instance::layer_id)?;
        h.ctx.layer_transform_to_global(layer).map(|t| t.scaling)
    };
    let _ = lift(&mut h, 400.0, 110.0, 5);
    let lifted = scale_of(&h).ok_or_else(|| fail("no lifted screen"))?;
    assert!(lifted < 0.95, "{lifted}");
    let mut handed = None;
    for _ in 0..40 {
        h.frame();
        if h.shell.workspace().is_overview_open() {
            handed = scale_of(&h);
            break;
        }
    }
    let handed = handed.ok_or_else(|| fail("the pause never brought the overview"))?;
    assert!(
        handed <= lifted + 1e-3,
        "the screen grew back from {lifted} to {handed} when the cards took over"
    );
    Ok(())
}

/// A pause low down is a hand making up its mind, not the recent screens — a later pause higher
/// up is.
#[test]
fn a_pause_low_down_is_not_recents_yet() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    let at = lift(&mut h, 400.0, 20.0, 3);
    h.frames(15);
    assert!(!h.shell.workspace().is_overview_open());
    let higher = at + egui::vec2(0.0, -110.0);
    for i in 1..=4 {
        // Step counts are tiny, so the conversions are exact.
        #[allow(clippy::cast_precision_loss)]
        let t = i as f32 / 4.0;
        h.move_to(at + (higher - at) * t);
        h.frame();
    }
    h.frames(15);
    assert!(h.shell.workspace().is_overview_open());
    let_go(&mut h, higher);
    Ok(())
}

/// The recent screens behind a gate: the pause asks for an unlock and the screen goes back.
#[test]
fn the_recents_gate_holds_the_pause() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "maintainer"], Some("viewer"));
    "gesture".clone_into(&mut config.nav_bar.style);
    config
        .access
        .gates
        .insert("nav.recents".to_owned(), "maintainer".to_owned());
    let (mut h, ..) = shell(config)?;
    two_tasks(&mut h);
    let _ = events(&mut h);
    let at = lift(&mut h, 400.0, 120.0, 5);
    h.frames(15);
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_overview_open());
    assert!(!h.shell.workspace().is_home(), "the pause took the swipe");
    assert!(events(&mut h)
        .iter()
        .any(|e| matches!(e, ShellEvent::Access(AccessEvent::UnlockRequested { .. }))));
    Ok(())
}

/// `[workspace] overview = false`: the pause is reported, as the recents button is, and nothing
/// comes up.
#[test]
fn with_the_overview_off_a_pause_is_only_reported() -> fairing::Result<()> {
    let mut config = gestures();
    config.workspace.overview = false;
    let (mut h, ..) = shell(config)?;
    two_tasks(&mut h);
    let _ = events(&mut h);
    let at = lift(&mut h, 400.0, 120.0, 5);
    h.frames(15);
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_overview_open() && !h.shell.workspace().is_home());
    assert!(events(&mut h)
        .iter()
        .any(|e| matches!(e, ShellEvent::OverviewRequested)));
    Ok(())
}

/// Along the indicator: to the right, the task used before comes in; to the left, back to the
/// one it came from — the run keeps its order. The one that went is stopped, the one that came
/// resumed.
#[test]
fn sliding_the_indicator_switches_tasks() -> fairing::Result<()> {
    let (mut h, a_log, b_log) = shell(gestures())?;
    two_tasks(&mut h);
    a_log.borrow_mut().clear();
    b_log.borrow_mut().clear();
    slide(&mut h, 500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("a"));
    assert_eq!(
        b_log.borrow().clone(),
        vec![Lifecycle::Paused, Lifecycle::Stopped]
    );
    assert_eq!(a_log.borrow().last(), Some(&Lifecycle::Resumed));
    slide(&mut h, -500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("b"), "back where it came from");
    Ok(())
}

/// Under the finger the two screens slide together: the one on show by the finger's offset, the
/// one coming in a width behind it.
#[test]
fn the_screens_slide_with_the_finger() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    let layer_x = |h: &Harness, id: &str| {
        let ws = h.shell.workspace();
        let layer = ws
            .tasks()
            .iter()
            .find(|t| t.root_id() == Some(id))
            .and_then(|t| t.top())
            .map(Instance::layer_id)?;
        h.ctx
            .layer_transform_to_global(layer)
            .map(|t| t.translation.x)
    };
    let from = egui::pos2(h.screen_rect().center().x, bottom(&h));
    h.press(from);
    h.frame();
    for x in [100.0, 200.0, 300.0] {
        h.move_to(from + egui::vec2(x, 0.0));
        h.frame();
    }
    let width = h.shell.workspace().last_content().width();
    let out = layer_x(&h, "b").ok_or_else(|| fail("no outgoing layer"))?;
    let inn = layer_x(&h, "a").ok_or_else(|| fail("no incoming layer"))?;
    assert!((out - 300.0).abs() < 1.0, "{out}");
    assert!((inn - (300.0 - width)).abs() < 1.0, "{inn}");
    h.release(from + egui::vec2(300.0, 0.0));
    h.frames(3);
    Ok(())
}

/// With no task that way, the slide gives a little and lands back.
#[test]
fn a_slide_with_nowhere_to_go_stays() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    // `b` is the newest: nothing is used after it.
    slide(&mut h, -500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("b"));
    assert!(!h.shell.workspace().is_switching());
    Ok(())
}

/// A run of switches ends with a touch on the screen: after it, the task used before is the one
/// just left.
#[test]
fn a_touch_ends_the_run_of_switches() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    slide(&mut h, 500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("a"));
    // Still the run: `a` is the oldest, so further right there is nothing.
    slide(&mut h, 500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("a"));
    // A tap on the screen, and the run starts again from how they were last used.
    h.tap(h.screen_rect().center());
    h.frames(2);
    slide(&mut h, 500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("b"));
    Ok(())
}

/// At home, a slide brings back the task used last.
#[test]
fn at_home_a_slide_brings_back_the_last_task() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    h.shell.home();
    h.frames(3);
    slide(&mut h, 500.0, 5);
    assert!(!h.shell.workspace().is_home());
    assert_eq!(on_show(&h).as_deref(), Some("b"));
    Ok(())
}

/// Over the cards, up from the bottom edge is home.
#[test]
fn over_the_cards_a_swipe_up_goes_home() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    two_tasks(&mut h);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
    let at = lift(&mut h, 400.0, 300.0, 5);
    let_go(&mut h, at);
    assert!(h.shell.workspace().is_home() && !h.shell.workspace().is_overview_open());
    Ok(())
}

/// Back is still a `back_edges` swipe.
#[test]
fn back_edges_still_go_back() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("c"));
    h.frames(3);
    assert_eq!(on_show(&h).as_deref(), Some("c"));
    let y = h.screen_rect().center().y;
    h.drag(egui::pos2(4.0, y), egui::pos2(700.0, y), 6);
    h.frames(3);
    assert_eq!(on_show(&h).as_deref(), Some("a"));
    Ok(())
}

/// The buttons style has no bottom gestures, and a bar turned off takes them with it.
#[test]
fn no_bottom_gestures_without_the_gesture_bar() -> fairing::Result<()> {
    let (mut h, ..) = shell(ShellConfig::default())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let at = lift(&mut h, 400.0, 300.0, 5);
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_home(), "buttons");

    let mut off = gestures();
    off.nav_bar.enabled = false;
    let (mut h, ..) = shell(off)?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    let at = lift(&mut h, 400.0, 300.0, 5);
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_home(), "the bar off");
    Ok(())
}

/// With the on-screen keyboard up, a press on its keys is the keys' — even where the edge zone
/// reaches up past the band into them — while the band below the keyboard still lifts.
#[cfg(feature = "osk")]
#[test]
fn a_press_on_the_keyboard_is_the_keyboards() -> fairing::Result<()> {
    // An injected theme brings its own motion tokens: `reduce` goes on it.
    let mut theme = fairing::Theme::dark();
    theme.metrics.edge_px = 120.0;
    theme.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = fairing::Shell::builder(gestures())
            .theme(theme)
            .build(ctx)?;
        shell.add(screen("form", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            // Clear of the deep top edge zone too.
            ui.add_space(160.0);
            let mut text = String::new();
            ui.add_sized(
                [300.0, 48.0],
                egui::TextEdit::singleline(&mut text)
                    .id_salt("t")
                    .hint_text("Type here"),
            );
        }));
        Ok(shell)
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("form"));
    h.frames(3);
    // The field, found by its hint rather than at an assumed place under the space.
    h.tap_text("Type here")?;
    h.frames(3);
    let keys = h
        .shell
        .layout()
        .osk
        .ok_or_else(|| fail("the keyboard did not come up"))?;
    let on_keys = egui::pos2(keys.center().x, keys.max.y - 8.0);
    assert!(
        h.screen_rect().max.y - on_keys.y < 120.0,
        "the press is inside the edge zone"
    );
    h.drag(on_keys, on_keys + egui::vec2(0.0, -200.0), 5);
    h.frames(3);
    assert!(!h.shell.workspace().is_home(), "a press on the keys");
    let at = lift(&mut h, keys.center().x, 200.0, 5);
    assert!(h.shell.workspace().is_lifting(), "the band below the keys");
    let_go(&mut h, at);
    assert!(h.shell.workspace().is_home());
    Ok(())
}

/// Over an open shade a bottom swipe is the shade's: nothing under it is lifted.
#[cfg(feature = "overlay")]
#[test]
fn over_the_shade_the_bottom_edge_lifts_nothing() -> fairing::Result<()> {
    let (mut h, ..) = shell(gestures())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    assert!(!h.shell.overlay().is_closed());
    let at = lift(&mut h, 400.0, 120.0, 5);
    assert!(!h.shell.workspace().is_lifting());
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_home());
    Ok(())
}

/// Behind the lock screen nothing new starts: a slide along the indicator switches nothing.
#[test]
fn behind_the_lock_screen_a_slide_switches_nothing() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "maintainer"], Some("viewer"));
    "gesture".clone_into(&mut config.nav_bar.style);
    config.access.pin_table.pins = [("maintainer".to_owned(), "1234".to_owned())]
        .into_iter()
        .collect();
    let (mut h, ..) = shell(config)?;
    two_tasks(&mut h);
    h.shell.launch(LaunchAction::Lock);
    h.frames(3);
    assert!(h.shell.lock_screen_visible());
    let from = egui::pos2(h.screen_rect().center().x, bottom(&h));
    h.press(from);
    h.frame();
    h.move_to(from + egui::vec2(300.0, 0.0));
    h.frame();
    assert!(!h.shell.workspace().is_switching());
    h.release(from + egui::vec2(300.0, 0.0));
    h.frames(3);
    assert_eq!(on_show(&h).as_deref(), Some("b"));
    Ok(())
}

/// A screen keeps its own widget state through a quick switch — its scroll stays where it was
/// left, though two tasks' screens take turns on the same layer and the one sliding in is drawn
/// on another.
#[test]
fn a_screen_keeps_its_scroll_through_a_switch() -> fairing::Result<()> {
    let offsets: Rc<RefCell<[f32; 2]>> = Rc::default();
    let mut h = test_shell(gestures(), |sh| {
        for (index, id) in ["a", "b"].into_iter().enumerate() {
            let seen = Rc::clone(&offsets);
            sh.add(
                screen(id, move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                    // The height stated, as `layout::page` does: left to the parent, a scroll
                    // area finds nothing overflows.
                    let out = egui::ScrollArea::vertical()
                        .id_salt("list")
                        .max_height(cx.pane.rect.height())
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for row in 0..80 {
                                ui.label(format!("{id} {row}"));
                            }
                        });
                    if let Some(slot) = seen.borrow_mut().get_mut(index) {
                        *slot = out.state.offset.y;
                    }
                })
                .title(id),
            );
        }
    })?;
    h.frames(2);
    two_tasks(&mut h);
    // `b` scrolled down by the wheel (egui smooths a wheel over several frames).
    let middle = h.screen_rect().center();
    h.wheel(middle, egui::vec2(0.0, -600.0));
    h.run_for(0.5);
    let offset = |i: usize| offsets.borrow().get(i).copied().unwrap_or(f32::NAN);
    let scrolled = offset(1);
    assert!(scrolled > 100.0, "{scrolled}");
    slide(&mut h, 500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("a"));
    assert!(offset(0) < 1.0, "`a` at its own top");
    slide(&mut h, -500.0, 5);
    assert_eq!(on_show(&h).as_deref(), Some("b"));
    let back = offset(1);
    assert!(
        (back - scrolled).abs() < 1.0,
        "`b` back at {back}, not where it was left ({scrolled})"
    );
    Ok(())
}

/// A screen guarding its edges (`ChromePolicy::edge_guard`) keeps the bottom one too.
#[test]
fn an_edge_guard_keeps_the_bottom_edge() -> fairing::Result<()> {
    let mut h = test_shell(gestures(), |sh| {
        sh.add(
            screen("pad", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("sign here");
            })
            .chrome(ChromePolicy {
                edge_guard: true,
                ..ChromePolicy::default()
            }),
        );
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("pad"));
    h.frames(3);
    let at = lift(&mut h, 400.0, 300.0, 5);
    let_go(&mut h, at);
    assert!(!h.shell.workspace().is_home());
    Ok(())
}
