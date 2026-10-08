//! The M1 integration test for the workspace plus the A2/A3 transitions: the A2 card, the A3 layer offset, the
//! lifecycle timing of a transition and the layer slots, along with further transition and lifecycle timing tests.
//!
//! The rules: the animations are checked by handing `Harness::new` a config with `motion.reduce = false`.
//! Layer offsets are asserted through `ctx.layer_transform_to_global(instance.layer_id())` — `layer_id` is keyed
//! on the slot (the stack depth), so it is read **inside a drawn frame** (`set_transform_layer` is sticky, so it
//! is still there right after the frame). The tests are written as `fn … -> fairing::Result<()>` (the
//! `panic!`/`unwrap` lint).
//!
//! The frame numbers: the test steps `Clock` on each frame, and a trait screen writes that value down along with
//! each `on_lifecycle` it gets — so that "`Paused` alone on the starting frame, `Stopped`/`Resumed` on the last"
//! can be asserted frame by frame. Headless time accumulates in 1/60 s steps.

use fairing::desktop::DesktopView;
use fairing::motion::Easing;
use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::theme::MotionTokens;
use fairing::workspace::{HomeTransition, Instance, StackTransition};
use fairing::{
    icon, screen, screen_with, Cx, Error, LaunchAction, Lifecycle, Screen, Shell, ShellEvent,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The 60 fps frame interval.
const DT: f32 = 1.0 / 60.0;

/// The test side's frame number.
type Clock = Rc<Cell<u64>>;

/// A (frame, event) log.
type Log = Rc<RefCell<Vec<(u64, Lifecycle)>>>;

/// A screen that writes each `on_lifecycle` down with the frame number.
struct Recorder {
    log: Log,
    clock: Clock,
}

impl Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("r");
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push((self.clock.get(), event));
    }
}

fn recorder(id: &str, log: &Log, clock: &Clock) -> fairing::screen::ScreenDecl {
    let (log, clock) = (Rc::clone(log), Rc::clone(clock));
    screen_with(id, move || Recorder {
        log: Rc::clone(&log),
        clock: Rc::clone(&clock),
    })
}

fn new_log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn take(log: &Log) -> Vec<(u64, Lifecycle)> {
    log.borrow_mut().drain(..).collect()
}

fn events_only(log: &Log) -> Vec<Lifecycle> {
    take(log).into_iter().map(|(_, e)| e).collect()
}

/// A `reduce = false` shell (with the Null clock).
fn animated(setup: impl FnOnce(&mut Shell)) -> fairing::Result<Harness> {
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    setup(&mut h.shell);
    Ok(h)
}

/// One frame (the test clock +1). What comes back is that frame's number.
fn step(h: &mut Harness, clock: &Clock) -> u64 {
    clock.set(clock.get() + 1);
    h.frame();
    clock.get()
}

/// It runs until the transition is over. What comes back is the frame number where `is_animating()` went `false`.
fn run_until_idle(h: &mut Harness, clock: &Clock) -> fairing::Result<u64> {
    for _ in 0..120 {
        let frame = step(h, clock);
        if !h.shell.is_animating() {
            return Ok(frame);
        }
    }
    Err(Error::Config(
        "the transition did not end inside 120 frames".to_owned(),
    ))
}

fn layer_x(h: &Harness, decl_id: &str) -> Option<f32> {
    let layer = h.shell.workspace().find(decl_id).map(Instance::layer_id)?;
    h.ctx
        .layer_transform_to_global(layer)
        .map(|t| t.translation.x)
}

fn require<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| Error::Config(format!("there is no {what}")))
}

/// How many registrations egui's `Areas` holds (`count()` is crate-private, so it counts the `AreaState`s in the `Debug` output).
fn area_count(h: &Harness) -> usize {
    h.ctx
        .memory(|m| format!("{:?}", m.areas()))
        .matches("AreaState {")
        .count()
}

/// The A2 open's first `ui` comes at `t ≥ 0.5`; the card Rect is `lerp(icon, pane, CubicOut(t))`.
#[test]
fn a2_first_ui_at_t_half_and_card_rect() -> fairing::Result<()> {
    let seen = Rc::new(Cell::new(false));
    let s = Rc::clone(&seen);
    let mut h = animated(|sh| {
        sh.add(
            screen("a", move |ui: &mut egui::Ui, _: &mut Cx| {
                s.set(true);
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.frames(2);
    let icon_rect = require(h.shell.desktop().icon_rect("a"), "the icon Rect")?;
    h.shell.launch(LaunchAction::open("a"));
    let mut first_ui_frame = None;
    for frame in 1..=30 {
        h.frame();
        let t = h.shell.workspace().home_transition().t();
        assert!(h.shell.is_animating() && h.repaint_requested);
        if seen.get() {
            assert!(t >= 0.5, "the first ui frame {frame} has t = {t}");
            assert!(
                matches!(
                    h.shell.workspace().home_transition(),
                    HomeTransition::Opening {
                        icon_rect: Some(_),
                        ..
                    }
                ),
                "the card path"
            );
            first_ui_frame = Some(frame);
            break;
        }
        assert!(t < 0.5, "ui is not called below t = 0.5 (frame {frame})");
    }
    let frame = require(first_ui_frame, "the first ui frame")?;
    // A 240 ms tween: 0.5 is 120 ms = 7.2 frames → the 8th frame.
    assert!((7..=9).contains(&frame), "the first ui frame = {frame}");
    assert_eq!(
        h.shell.workspace().home_transition().icon_rect(),
        Some(icon_rect)
    );
    // The pure mapping against A2's own figures is a unit test now
    // (`workspace::transition::tests::a2_open_matches_the_spec_figures`).
    Ok(())
}

/// During the A2 open the desktop layer scales `1 → 0.92` about its centre (`layer_transform_to_global`), and once
/// it is over the transform is gone (IDENTITY = the entry is removed).
#[test]
fn a2_scales_desktop_layer_and_restores_it() -> fairing::Result<()> {
    let mut h = animated(|sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.frames(2);
    assert!(h
        .ctx
        .layer_transform_to_global(DesktopView::layer_id())
        .is_none());
    h.shell.launch(LaunchAction::open("a"));
    let tokens = MotionTokens::default();
    for frame in 1..=5 {
        h.frame();
        let t = h.shell.workspace().home_transition().t();
        let expected = 1.0 + (tokens.desktop_scale - 1.0) * Easing::CubicOut.apply(t);
        let transform = require(
            h.ctx.layer_transform_to_global(DesktopView::layer_id()),
            "the desktop layer's transform",
        )?;
        assert!(
            (transform.scaling - expected).abs() < 2e-3,
            "frame {frame}: scaling {} != {expected}",
            transform.scaling
        );
        let center = h.shell.layout().content.center();
        assert!(
            (transform * center - center).length() < 1e-2,
            "the centre pivot"
        );
    }
    h.run_for(0.5);
    assert!(!h.shell.is_animating());
    assert!(
        h.ctx
            .layer_transform_to_global(DesktopView::layer_id())
            .is_none(),
        "IDENTITY once it is over"
    );
    Ok(())
}

/// The fallback (with no icon Rect): the screen is drawn from the first frame with a layer scale of `0.96 → 1` plus
/// a fade, over 200 ms.
#[test]
fn a2_fallback_without_icon_fades_and_scales_in_200ms() -> fairing::Result<()> {
    let seen = Rc::new(Cell::new(false));
    let s = Rc::clone(&seen);
    let mut h = animated(|sh| {
        sh.add(screen("f", move |ui: &mut egui::Ui, _: &mut Cx| {
            s.set(true);
            ui.label("f");
        }));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("f"));
    assert!(matches!(
        h.shell.workspace().home_transition(),
        HomeTransition::Opening {
            icon_rect: None,
            ..
        }
    ));
    h.frame();
    assert!(
        seen.get(),
        "the fallback draws the screen from the first frame"
    );
    let layer = require(h.shell.workspace().find("f"), "f")?.layer_id();
    let t = h.shell.workspace().home_transition().t();
    assert!((t - DT / 0.2).abs() < 1e-3, "a 200 ms tween: t = {t}");
    let transform = require(
        h.ctx.layer_transform_to_global(layer),
        "the screen layer's transform",
    )?;
    // A2's figure, written in rather than read back from the implementation: 0.96 → 1.
    let expected = 0.96 + (1.0 - 0.96) * Easing::CubicOut.apply(t);
    assert!((transform.scaling - expected).abs() < 2e-3);
    h.frames(9);
    assert!(
        h.shell.is_animating(),
        "10 frames = 167 ms is not there yet"
    );
    h.frames(4);
    assert!(!h.shell.is_animating(), "14 frames = 233 ms is over");
    assert!(
        h.ctx.layer_transform_to_global(layer).is_none(),
        "IDENTITY once it is over"
    );
    Ok(())
}

/// 7 frames after a push the incoming layer's `x == (1 − CubicOut(7/60/0.22))·W ± 1 px` and the layer below
/// is at `−parallax·W·s`. The two layer ids differ.
#[test]
fn a3_push_layer_offset_at_frame_seven() -> fairing::Result<()> {
    let mut h = animated(|sh| {
        sh.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("a");
        }));
        sh.add(screen("b", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("b");
        }));
    })?;
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(0.5);
    assert!(!h.shell.is_animating());
    h.shell.launch(LaunchAction::open("b"));
    h.frames(7);
    let tokens = MotionTokens::default();
    let width = h.shell.layout().content.width();
    let raw = 7.0 * DT / 0.22;
    let eased = Easing::CubicOut.apply(raw);
    let StackTransition::Pushing { t } = h.shell.workspace().stack_transition() else {
        return Err(Error::Config("it is not Pushing".to_owned()));
    };
    assert!((t.value() - raw).abs() < 1e-3, "t = {}", t.value());
    let x_in = require(layer_x(&h, "b"), "b's layer transform")?;
    assert!(
        (x_in - (1.0 - eased) * width).abs() < 1.0,
        "x_in = {x_in}, expected {}",
        (1.0 - eased) * width
    );
    let x_out = require(layer_x(&h, "a"), "a's layer transform")?;
    assert!(
        (x_out + tokens.parallax * width * eased).abs() < 1.0,
        "x_out = {x_out}"
    );
    let a = require(h.shell.workspace().find("a"), "a")?;
    let b = require(h.shell.workspace().find("b"), "b")?;
    assert_ne!(a.layer_id(), b.layer_id());
    assert_eq!((a.layer_slot(), b.layer_slot()), (0, 1));
    h.run_for(0.5);
    assert!(!h.shell.is_animating());
    assert!(layer_x(&h, "b").is_none(), "IDENTITY once it is over");
    Ok(())
}

/// Regression: **during a pop the outgoing layer is above the incoming one** (A3's iOS-style
/// pop). In `Areas::order` later is higher, and `Memory::layer_ids()` hands that order over as it is (`Areas::order()`
/// itself is `pub(crate)`). The regression only reproduces after opening once through A2 and coming in — the cause
/// was that A2 having `move_to_top`ed slot 0 followed through as far as the pop.
#[test]
fn pop_keeps_the_outgoing_layer_above_the_incoming_one() -> fairing::Result<()> {
    let clock: Clock = Rc::new(Cell::new(0));
    let mut h = animated(|sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
        sh.add(screen("b", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("b");
        }));
    })?;
    step(&mut h, &clock);
    step(&mut h, &clock);
    // Open through A2 (slot 0 goes up here) and push on top of it.
    h.shell.launch(LaunchAction::open("a"));
    run_until_idle(&mut h, &clock)?;
    h.shell.launch(LaunchAction::open("b"));
    run_until_idle(&mut h, &clock)?;
    let (incoming, outgoing) = (Instance::slot_layer_id(0), Instance::slot_layer_id(1));
    assert_eq!(
        require(h.shell.workspace().find("b"), "b")?.layer_id(),
        outgoing
    );

    h.shell.back();
    // Back starts the transition where it stands — the next frame is the pop's first drawing.
    for frame in 1..=3 {
        step(&mut h, &clock);
        let StackTransition::Popping { .. } = h.shell.workspace().stack_transition() else {
            return Err(Error::Config(format!("frame {frame}: it is not Popping")));
        };
        let order: Vec<egui::LayerId> = h.ctx.memory(|m| m.layer_ids().collect());
        let at = |layer| order.iter().position(|l| *l == layer);
        let i = require(at(incoming), "the incoming layer")?;
        let o = require(at(outgoing), "the outgoing layer")?;
        assert!(
            o > i,
            "frame {frame}: the outgoing slot 1 (at {o}) has to come after (above) the incoming slot 0 (at {i})"
        );
    }
    run_until_idle(&mut h, &clock)?;
    // Once the pop is over the top one left (slot 0) is back at the very top.
    let order: Vec<egui::LayerId> = h.ctx.memory(|m| m.layer_ids().collect());
    let at = |layer| order.iter().position(|l| *l == layer);
    assert!(
        at(incoming) > at(outgoing),
        "Idle has the topmost instance above"
    );
    Ok(())
}

/// Regression: **the incoming screen really is drawn on the first frame of the first push transition.**
/// An egui `Area` with no `AreaState` runs its first frame as a sizing pass and throws the output away
/// (`containers/area.rs:444`) — with no slot warm-up, this frame's marker shape count is 0.
#[test]
fn first_push_frame_already_paints_the_incoming_screen() -> fairing::Result<()> {
    /// The marker colour only the incoming screen draws (a value used nowhere else).
    const MARK: egui::Color32 = egui::Color32::from_rgb(1, 2, 3);

    fn marks(shapes: &[egui::epaint::ClippedShape]) -> usize {
        shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) => rect.fill == MARK,
                _ => false,
            })
            .count()
    }

    let mut h = animated(|sh| {
        sh.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("a");
        }));
        sh.add(screen("b", |ui: &mut egui::Ui, _: &mut Cx| {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, MARK);
        }));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(0.5);
    assert!(!h.shell.is_animating());
    assert_eq!(marks(&h.frame_shapes()), 0, "b is not open yet");

    h.shell.launch(LaunchAction::open("b"));
    let shapes = h.frame_shapes();
    let StackTransition::Pushing { t } = h.shell.workspace().stack_transition() else {
        return Err(Error::Config("it is not Pushing".to_owned()));
    };
    assert!(
        t.value() < 0.2,
        "it has to be the first push frame: t = {}",
        t.value()
    );
    assert!(
        marks(&shapes) > 0,
        "the incoming screen's shape count > 0 (thrown away as a sizing pass it would be 0)"
    );
    // It goes on being drawn on the next frame too (confirming the warm-up is not a one-off exception).
    assert!(marks(&h.frame_shapes()) > 0);
    Ok(())
}

/// The incremental warm-up: a slot **beyond** the four put up on the first frame is prepared a frame before it is
/// used too — pushing a fifth at depth 4 is drawn from the first frame as well.
#[test]
fn a_slot_beyond_the_initial_warm_up_is_ready_before_it_is_used() -> fairing::Result<()> {
    /// The marker colour only the fifth (slot 4) screen draws.
    const MARK: egui::Color32 = egui::Color32::from_rgb(4, 5, 6);

    fn marks(shapes: &[egui::epaint::ClippedShape]) -> usize {
        shapes
            .iter()
            .filter(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) => rect.fill == MARK,
                _ => false,
            })
            .count()
    }

    let mut h = animated(|sh| {
        for id in ["s0", "s1", "s2", "s3"] {
            sh.add(screen(id, move |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label(id);
            }));
        }
        sh.add(screen("s4", |ui: &mut egui::Ui, _: &mut Cx| {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, 0.0, MARK);
        }));
    })?;
    h.frames(2);
    for id in ["s0", "s1", "s2", "s3"] {
        h.shell.launch(LaunchAction::open(id));
        h.run_for(0.5);
        assert!(!h.shell.is_animating(), "{id}'s transition has to be over");
    }
    assert_eq!(
        require(h.shell.workspace().find("s3"), "s3")?.layer_slot(),
        3
    );

    h.shell.launch(LaunchAction::open("s4"));
    let shapes = h.frame_shapes();
    let StackTransition::Pushing { .. } = h.shell.workspace().stack_transition() else {
        return Err(Error::Config("it is not Pushing".to_owned()));
    };
    assert_eq!(
        require(h.shell.workspace().find("s4"), "s4")?.layer_slot(),
        4
    );
    assert!(
        marks(&shapes) > 0,
        "slot 4 has to be drawn from the first frame too"
    );
    Ok(())
}

/// On a push's starting frame only the one below gets `Paused`, and on the last frame the one below gets
/// `Stopped` and the one above `Resumed`; a pop's start gives the outgoing side `Paused` and its end `Destroyed`
/// plus `Resumed` for the new top; a home close's start gives `Paused` and its end `Stopped`.
#[test]
fn transition_lifecycle_timing() -> fairing::Result<()> {
    let clock: Clock = Rc::new(Cell::new(0));
    let (a_log, b_log) = (new_log(), new_log());
    let mut h = animated(|sh| {
        sh.add(recorder("a", &a_log, &clock).icon(icon::GAUGE).desktop());
        sh.add(recorder("b", &b_log, &clock));
    })?;
    step(&mut h, &clock);
    step(&mut h, &clock);
    h.shell.launch(LaunchAction::open("a"));
    let opened = run_until_idle(&mut h, &clock)?;
    assert_eq!(
        take(&a_log).last().map(|(f, e)| (*f, *e)),
        Some((opened, Lifecycle::Resumed))
    );

    // push
    h.shell.launch(LaunchAction::open("b"));
    let start = step(&mut h, &clock);
    assert_eq!(
        take(&a_log),
        vec![(start, Lifecycle::Paused)],
        "the starting frame: only the one below is Paused"
    );
    assert_eq!(take(&b_log), vec![(start, Lifecycle::Created)]);
    let end = run_until_idle(&mut h, &clock)?;
    assert!(end > start + 5, "a push's 220 ms is several frames");
    assert_eq!(
        take(&a_log),
        vec![(end, Lifecycle::Stopped)],
        "the last frame: the one below is Stopped"
    );
    assert_eq!(
        take(&b_log),
        vec![(end, Lifecycle::Resumed)],
        "the last frame: the one above is Resumed"
    );

    // pop
    h.shell.back();
    let start = step(&mut h, &clock);
    assert_eq!(
        take(&b_log),
        vec![(start, Lifecycle::Paused)],
        "the starting frame: the outgoing side is Paused"
    );
    assert_eq!(take(&a_log).len(), 0);
    let end = run_until_idle(&mut h, &clock)?;
    assert_eq!(
        take(&b_log),
        vec![(end, Lifecycle::Destroyed)],
        "the last frame: the outgoing side is Destroyed"
    );
    assert_eq!(
        take(&a_log),
        vec![(end, Lifecycle::Resumed)],
        "the last frame: the new top is Resumed"
    );
    assert!(h.shell.workspace().find("b").is_none());

    // home (the A2 close, with an icon)
    h.shell.home();
    let start = step(&mut h, &clock);
    assert!(matches!(
        h.shell.workspace().home_transition(),
        HomeTransition::Closing {
            icon_rect: Some(_),
            ..
        }
    ));
    assert_eq!(take(&a_log), vec![(start, Lifecycle::Paused)]);
    let end = run_until_idle(&mut h, &clock)?;
    assert_eq!(
        take(&a_log),
        vec![(end, Lifecycle::Stopped)],
        "the end of the home close: the whole task is Stopped"
    );
    assert!(h.shell.workspace().is_home());
    assert!(h.shell.workspace().find("a").is_some(), "the task lives on");
    h.frames(3);
    assert!(!h.repaint_requested, "idle");
    Ok(())
}

/// The two instances drawn during a transition are on different layers, and opening and closing over and over
/// does not grow egui's `Areas` (the slot = the stack depth).
#[test]
fn layer_slots_are_finite_and_distinct_during_transition() -> fairing::Result<()> {
    let mut h = animated(|sh| {
        for id in ["a", "b", "c"] {
            sh.add(
                screen(id, |ui: &mut egui::Ui, _: &mut Cx| {
                    ui.label("x");
                })
                .icon(icon::GAUGE)
                .desktop(),
            );
        }
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(0.5);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    let a = require(h.shell.workspace().find("a"), "a")?.layer_id();
    let b = require(h.shell.workspace().find("b"), "b")?.layer_id();
    assert_ne!(a, b);
    assert!(h
        .ctx
        .memory(|m| m.areas().is_visible(&a) && m.areas().is_visible(&b)));
    h.run_for(0.5);

    let cycle = |h: &mut Harness| {
        h.shell.launch(LaunchAction::open("c"));
        h.run_for(0.3);
        h.shell.back();
        h.run_for(0.3);
        h.shell.home();
        h.run_for(0.3);
        h.shell.launch(LaunchAction::open("a"));
        h.run_for(0.3);
    };
    for _ in 0..3 {
        cycle(&mut h);
    }
    let after_three = area_count(&h);
    for _ in 0..17 {
        cycle(&mut h);
    }
    let after_twenty = area_count(&h);
    assert_eq!(after_three, after_twenty, "Areas is constant");
    // The desktop plus screen slots 0 · 1 · 2 plus the transition shield (M2, A3's "a shield during a tween"). The
    // chrome Areas (the overlay, the OSK and the toasts) do not come up in this scenario. Room is left for egui's own Areas.
    assert!(after_twenty <= 7, "Areas = {after_twenty}");
    for instance in h
        .shell
        .workspace()
        .tasks()
        .iter()
        .flat_map(fairing::workspace::Task::iter)
    {
        assert!(instance.layer_slot() <= 2);
    }
    Ok(())
}

/// A new launch during a transition finishes the current one at once (the end events) and starts the next (there is no
/// queue). Home during an A2 open carries `t` on and closes backwards, giving `Paused → Stopped` with no `Resumed`.
#[test]
fn launch_during_transition_settles_current_one() -> fairing::Result<()> {
    let clock: Clock = Rc::new(Cell::new(0));
    let (a_log, b_log, c_log) = (new_log(), new_log(), new_log());
    let mut h = animated(|sh| {
        sh.add(recorder("a", &a_log, &clock).icon(icon::GAUGE).desktop());
        sh.add(recorder("b", &b_log, &clock));
        sh.add(recorder("c", &c_log, &clock));
    })?;
    step(&mut h, &clock);
    step(&mut h, &clock);
    h.shell.launch(LaunchAction::open("a"));
    run_until_idle(&mut h, &clock)?;
    let _ = take(&a_log);

    // A launch during an A3 push
    h.shell.launch(LaunchAction::open("b"));
    for _ in 0..3 {
        step(&mut h, &clock);
    }
    let _ = (take(&a_log), take(&b_log));
    h.shell.launch(LaunchAction::open("c"));
    let frame = step(&mut h, &clock);
    let StackTransition::Pushing { t } = h.shell.workspace().stack_transition() else {
        return Err(Error::Config("it is not a new push".to_owned()));
    };
    assert!(t.value() < 0.1, "a new push starts at 0: t = {}", t.value());
    assert_eq!(
        take(&a_log),
        vec![(frame, Lifecycle::Stopped)],
        "the settle's end events"
    );
    assert_eq!(
        take(&b_log),
        vec![(frame, Lifecycle::Resumed), (frame, Lifecycle::Paused)],
        "Resumed from the settle, Paused from the new push's start"
    );
    assert_eq!(take(&c_log), vec![(frame, Lifecycle::Created)]);
    assert_eq!(
        h.shell
            .workspace()
            .active_task()
            .map(fairing::workspace::Task::len),
        Some(3)
    );
    run_until_idle(&mut h, &clock)?;
    let _ = (take(&a_log), take(&b_log), take(&c_log));

    // Home during an A2 open: carrying on backwards.
    h.shell.home();
    run_until_idle(&mut h, &clock)?;
    let _ = (take(&a_log), take(&b_log), take(&c_log));
    h.shell.launch(LaunchAction::open("a"));
    for _ in 0..3 {
        step(&mut h, &clock);
    }
    assert!(matches!(
        h.shell.workspace().home_transition(),
        HomeTransition::Opening { .. }
    ));
    let t = h.shell.workspace().home_transition().t();
    h.shell.home();
    let start = step(&mut h, &clock);
    let HomeTransition::Closing { t: closing, .. } = h.shell.workspace().home_transition() else {
        return Err(Error::Config("it is not Closing".to_owned()));
    };
    assert!(
        closing.value() > 1.0 - t - 1e-3 && closing.value() < 1.0,
        "it carries t on"
    );
    assert!(h.shell.workspace().is_home());
    let end = run_until_idle(&mut h, &clock)?;
    assert_eq!(
        take(&c_log),
        vec![(start, Lifecycle::Paused), (end, Lifecycle::Stopped)],
        "Paused → Stopped with no Resumed"
    );
    assert!(take(&a_log).is_empty() && take(&b_log).is_empty());
    Ok(())
}

/// Input is refused during a transition: the screen `Area` is `interactable(false)`, so a button cannot be pressed, and once it is over it can.
#[test]
fn transition_blocks_input() -> fairing::Result<()> {
    let clicks = Rc::new(Cell::new(0u32));
    let rect = Rc::new(Cell::new(egui::Rect::NOTHING));
    let (c, r) = (Rc::clone(&clicks), Rc::clone(&rect));
    let mut h = animated(|sh| {
        sh.add(
            screen("a", move |ui: &mut egui::Ui, _: &mut Cx| {
                let response = ui.button("tap");
                r.set(response.rect);
                if response.clicked() {
                    c.set(c.get() + 1);
                }
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(9);
    assert!(h.shell.is_animating() && rect.get().is_positive());
    h.tap(rect.get().center());
    assert!(
        h.shell.is_animating(),
        "still transitioning 3 frames after the tap"
    );
    assert_eq!(clicks.get(), 0, "it is not pressed during a transition");
    h.run_for(0.5);
    assert!(!h.shell.is_animating());
    h.tap(rect.get().center());
    assert_eq!(clicks.get(), 1, "it is pressed once it is over");
    Ok(())
}

/// A root pop: home at once (`is_home`, no `find`, `ScreenClosed` plus `WentHome`), but the outgoing task is drawn
/// during the A2 close (`Paused`) and `Destroyed` at the end.
#[test]
fn root_pop_animates_home_and_destroys_task() -> fairing::Result<()> {
    let clock: Clock = Rc::new(Cell::new(0));
    let a_log = new_log();
    let mut h = animated(|sh| {
        sh.add(recorder("a", &a_log, &clock).icon(icon::GAUGE).desktop());
    })?;
    step(&mut h, &clock);
    step(&mut h, &clock);
    h.shell.launch(LaunchAction::open("a"));
    run_until_idle(&mut h, &clock)?;
    let _ = (take(&a_log), h.shell.poll_events());
    h.shell.back();
    let start = step(&mut h, &clock);
    let ws = h.shell.workspace();
    assert!(ws.is_home() && ws.find("a").is_none() && ws.tasks().is_empty());
    assert!(
        matches!(
            ws.home_transition(),
            HomeTransition::Closing {
                icon_rect: Some(_),
                ..
            }
        ),
        "the starting point is the icon Rect given at open"
    );
    assert!(h.shell.is_animating());
    let events = h.shell.poll_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "a")));
    assert!(events.iter().any(|e| matches!(e, ShellEvent::WentHome)));
    assert_eq!(take(&a_log), vec![(start, Lifecycle::Paused)]);
    let end = run_until_idle(&mut h, &clock)?;
    assert_eq!(take(&a_log), vec![(end, Lifecycle::Destroyed)]);
    assert!(
        h.shell.desktop().icon_rect("a").is_some(),
        "the icon is as it was"
    );
    Ok(())
}

/// `reduce = true` (`test_shell`): every transition is instant and the sequence is the settled lifecycle order.
#[test]
fn reduce_motion_transitions_are_immediate() -> fairing::Result<()> {
    let clock: Clock = Rc::new(Cell::new(0));
    let (a_log, b_log) = (new_log(), new_log());
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(recorder("a", &a_log, &clock).icon(icon::GAUGE).desktop());
        sh.add(recorder("b", &b_log, &clock));
    })?;
    step(&mut h, &clock);
    for action in [
        Some(LaunchAction::open("a")),
        Some(LaunchAction::open("b")),
        None,
        None,
    ] {
        match action {
            Some(action) => h.shell.launch(action),
            None if h.shell.workspace().find("b").is_some() => h.shell.back(),
            None => h.shell.home(),
        }
        assert!(!h.shell.is_animating(), "reduce: no transition");
        step(&mut h, &clock);
        assert!(!h.shell.is_animating());
    }
    h.frames(3);
    assert!(!h.repaint_requested, "idle");
    assert_eq!(
        events_only(&a_log),
        vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Stopped,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Stopped,
        ]
    );
    assert_eq!(
        events_only(&b_log),
        vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Destroyed,
        ]
    );
    assert!(h.shell.workspace().is_home() && h.shell.workspace().find("a").is_some());
    Ok(())
}

/// Single-2: on the home screen re-tapping an icon brings that task forward with the A2 zoom, and from the Tasks view
/// moving to another task is instant (the previous task `Paused → Stopped`, the new one `Resumed`).
#[test]
fn resume_from_home_zooms_and_task_switch_is_immediate() -> fairing::Result<()> {
    let clock: Clock = Rc::new(Cell::new(0));
    let (a_log, b_log) = (new_log(), new_log());
    let mut h = animated(|sh| {
        sh.add(recorder("a", &a_log, &clock).icon(icon::GAUGE).desktop());
        sh.add(recorder("b", &b_log, &clock).icon(icon::GAUGE).desktop());
    })?;
    step(&mut h, &clock);
    step(&mut h, &clock);
    h.shell.launch(LaunchAction::open("a"));
    run_until_idle(&mut h, &clock)?;
    let a_id = require(h.shell.workspace().find("a"), "a")?.id();
    h.shell.home();
    run_until_idle(&mut h, &clock)?;
    h.shell.launch(LaunchAction::open("b"));
    run_until_idle(&mut h, &clock)?;
    assert_eq!(h.shell.workspace().tasks().len(), 2);
    let _ = (take(&a_log), take(&b_log));

    // The Tasks view → another task: instant.
    h.shell.launch(LaunchAction::open("a"));
    assert!(!h.shell.is_animating(), "crossing between tasks is instant");
    let frame = step(&mut h, &clock);
    assert!(!h.shell.is_animating());
    assert_eq!(
        h.shell.workspace().active_task().and_then(|t| t.root_id()),
        Some("a")
    );
    assert_eq!(
        require(h.shell.workspace().find("a"), "a")?.id(),
        a_id,
        "the same instance"
    );
    assert_eq!(
        take(&b_log),
        vec![(frame, Lifecycle::Paused), (frame, Lifecycle::Stopped)]
    );
    assert_eq!(take(&a_log), vec![(frame, Lifecycle::Resumed)]);

    // Home → re-tapping the icon: the A2 zoom.
    h.shell.home();
    run_until_idle(&mut h, &clock)?;
    let _ = take(&a_log);
    let icon_rect = require(h.shell.desktop().icon_rect("a"), "the icon Rect")?;
    h.tap(icon_rect.center());
    assert!(matches!(
        h.shell.workspace().home_transition(),
        HomeTransition::Opening { icon_rect: Some(r), .. } if *r == icon_rect
    ));
    assert!(!h.shell.workspace().is_home());
    let end = run_until_idle(&mut h, &clock)?;
    assert_eq!(take(&a_log), vec![(end, Lifecycle::Resumed)]);
    assert_eq!(
        h.shell.workspace().tasks().len(),
        2,
        "the task count does not grow"
    );
    Ok(())
}

/// A background task's `evict_after` expiring does not break the foreground's A2 — only a user command interrupts a
/// transition, and the memory policy stays out of sight. `Workspace::close_where` only calls
/// `settle()` where the matching instance is **the active Pane task or is mid-transition**.
#[test]
fn background_evict_does_not_cut_a_running_home_transition() -> fairing::Result<()> {
    let clock = Clock::default();
    let log = new_log();
    let mut h = animated(|sh| {
        sh.add(
            screen_with("e", || Blank)
                .icon(icon::ACTIVITY)
                .desktop()
                .evict_after(std::time::Duration::from_millis(300)),
        );
        sh.add(recorder("a", &log, &clock).icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    // Open e and go home — the task is alive and the evict clock runs from the moment of `Stopped`.
    h.shell.launch(LaunchAction::open("e"));
    run_until_idle(&mut h, &clock)?;
    h.shell.home();
    run_until_idle(&mut h, &clock)?;
    assert!(
        h.shell.workspace().find("e").is_some(),
        "the task lives on at home too"
    );
    // Wait a little so the evict deadline (300 ms) falls in the middle of the A2 (240 ms), then open a.
    h.frames(10);
    h.shell.launch(LaunchAction::open("a"));
    let mut previous = 0.0_f32;
    let mut evicted_at = None;
    let mut settled_at = None;
    for frame in 1..=60 {
        h.frame();
        let t = h.shell.workspace().home_transition().t();
        let animating = h.shell.is_animating();
        if h.shell
            .poll_events()
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "e"))
        {
            evicted_at = Some((frame, t, animating));
        }
        if settled_at.is_none() {
            assert!(
                t >= previous,
                "frame {frame}: t went backwards ({previous} → {t})"
            );
            previous = t;
            if !animating {
                settled_at = Some(frame);
            }
        }
    }
    let (frame, t, animating) = require(evicted_at, "e's evict")?;
    let settled = require(settled_at, "the A2's last frame")?;
    assert!(
        animating && t < 1.0,
        "the background evict (frame {frame}, t = {t}) snapped the A2 to its end"
    );
    assert!(
        settled > frame,
        "the A2 has to carry on past the evict (frame {frame}) too (last frame {settled})"
    );
    assert!(
        (13..=17).contains(&settled),
        "the A2's 240 ms = 14.4 frames: last frame {settled}"
    );
    assert!(h.shell.workspace().find("e").is_none(), "e was evicted");
    assert!(h.shell.workspace().find("a").is_some());
    Ok(())
}

/// A generative screen that draws nothing (something to evict).
struct Blank;

impl Screen for Blank {
    fn ui(&mut self, _ui: &mut egui::Ui, _cx: &mut Cx<'_>) {}
}
