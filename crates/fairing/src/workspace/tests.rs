use super::{Instance, InstanceId, Lifecycle, Workspace, WorkspaceView};
use crate::motion::Tween;
use crate::screen::cx::fixture::Fixture;
use crate::screen::{screen_with, Cx, Registry, Screen, ScreenDecl};
use crate::theme::MotionTokens;
use crate::workspace::{HomeTransition, StackTransition};
use egui::{pos2, Rect};
use std::time::Duration;

struct Blank;

impl Screen for Blank {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("x");
    }
}

fn decl(id: &str) -> ScreenDecl {
    screen_with(id, || Blank)
}

fn instance(ws: &mut Workspace, id: &str) -> Instance {
    let mut d = decl(id);
    let screen = d.spawn();
    Instance::new(ws.alloc_id(), &d, screen)
}

fn evicting(ws: &mut Workspace, id: &str, after: Duration) -> Instance {
    let mut d = decl(id).evict_after(after);
    let screen = d.spawn();
    Instance::new(ws.alloc_id(), &d, screen)
}

fn animated() -> MotionTokens {
    MotionTokens::default()
}

fn reduced() -> MotionTokens {
    MotionTokens {
        reduce: true,
        push: Tween::instant(),
        pop: Tween::instant(),
        home_open: Tween::instant(),
        home_close: Tween::instant(),
        ..MotionTokens::default()
    }
}

fn icon() -> Rect {
    Rect::from_min_size(pos2(100.0, 200.0), egui::vec2(96.0, 96.0))
}

/// Run until the transition finishes (`tick` caps dt at 50 ms).
fn finish(ws: &mut Workspace) {
    for _ in 0..200 {
        if !ws.is_animating() {
            return;
        }
        ws.tick(0.05);
    }
}

/// Drain the closure queue (`queue` puts the same event into both queues).
fn drain(instance: &mut Instance) -> Vec<Lifecycle> {
    let mut out = Vec::new();
    while let Some(e) = instance.next_closure_event() {
        out.push(e);
    }
    out
}

fn drain_find(ws: &mut Workspace, decl_id: &str) -> Vec<Lifecycle> {
    let id = ws.find(decl_id).map(Instance::id);
    id.and_then(|id| ws.instance_mut(id))
        .map_or_else(Vec::new, drain)
}

/// A headless flush — empty the graveyard and let the `Destroyed` notifications go out.
fn flush(ws: &mut Workspace, fixture: &mut Fixture) {
    let mut parts = fixture.parts();
    // Every screen here is a factory one, so the instance holds it and the registry is never
    // asked for anything.
    ws.flush_lifecycle(&mut parts, &mut Registry::new());
}

/// `find_task` looks at whole stacks — it finds an instance that is not the root too.
#[test]
fn find_task_searches_whole_stacks() {
    let mut ws = Workspace::new();
    let tokens = reduced();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    ws.go_home(None, &tokens);
    let c = instance(&mut ws, "c");
    ws.open(c, None, &tokens);
    assert_eq!(ws.tasks().len(), 2);
    assert_eq!(ws.find_task("a"), Some(0));
    assert_eq!(
        ws.find_task("b"),
        Some(0),
        "it finds one that is not the root"
    );
    assert_eq!(ws.find_task("c"), Some(1));
    assert_eq!(ws.find_task("zzz"), None);
    assert!(
        !ws.resume_task_with_root("b", None, &tokens),
        "a root-relative wrapper knows nothing of b"
    );
    assert!(ws.resume_task_with_root("a", None, &tokens));
    assert_eq!(ws.active_task().and_then(|t| t.root_id()), Some("a"));
    assert_eq!(
        ws.active_task().map(Task_len),
        Some(2),
        "the stack is as it was"
    );
}

#[allow(non_snake_case)]
fn Task_len(task: &super::Task) -> usize {
    task.len()
}

/// To another task in the Tasks view: the previous task's top `Paused` → `Stopped` throughout,
/// the new task's top `Resumed`, and no animation. Out of range is `false`; already at the front
/// is `true` and does nothing.
#[test]
fn resume_task_switch_is_immediate_and_stops_previous() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, Some(icon()), &tokens);
    ws.go_home(Some(icon()), &tokens);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    // Even with a transition (an A2 fallback open) running, the resume finishes it first.
    assert!(ws.is_animating());
    let _ = drain_find(&mut ws, "a");
    let _ = drain_find(&mut ws, "b");
    assert!(!ws.resume_task(9, None, &tokens));
    assert!(ws.resume_task(0, None, &tokens));
    assert!(!ws.is_animating(), "crossing between tasks is instant");
    assert_eq!(ws.view(), WorkspaceView::Tasks);
    assert_eq!(ws.active_task().and_then(|t| t.root_id()), Some("a"));
    assert_eq!(
        drain_find(&mut ws, "b"),
        vec![Lifecycle::Resumed, Lifecycle::Paused, Lifecycle::Stopped],
        "Paused then Stopped after the interrupted open's Resumed"
    );
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);
    assert!(ws.resume_task(0, None, &tokens), "already in front");
    assert_eq!(drain_find(&mut ws, "a").len(), 0);
}

/// The A2 interruption rule: resuming the same task mid-close reverses from the same `t`; another task settles.
#[test]
fn resume_during_closing_reverses_same_task_only() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, Some(icon()), &tokens);
    finish(&mut ws);
    assert!(!ws.is_animating());
    ws.go_home(Some(icon()), &tokens);
    for _ in 0..4 {
        ws.tick(1.0 / 60.0);
    }
    let t = ws.home_transition().t();
    assert!(matches!(
        ws.home_transition(),
        HomeTransition::Closing { .. }
    ));
    let _ = drain_find(&mut ws, "a");
    assert!(ws.resume_task(0, Some(icon()), &tokens));
    assert!(matches!(
        ws.home_transition(),
        HomeTransition::Opening { .. }
    ));
    assert!(
        (ws.home_transition().t() - (1.0 - t)).abs() < 1e-4,
        "it carries t on"
    );
    assert!(
        drain_find(&mut ws, "a").is_empty(),
        "it carries on with no Stopped"
    );
    finish(&mut ws);
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);

    // Resuming another task finishes the close (Stopped throughout) and opens the new one.
    ws.go_home(Some(icon()), &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    ws.go_home(None, &tokens);
    ws.tick(1.0 / 60.0);
    let _ = drain_find(&mut ws, "a");
    let _ = drain_find(&mut ws, "b");
    assert!(ws.resume_task(0, Some(icon()), &tokens));
    assert!(matches!(
        ws.home_transition(),
        HomeTransition::Opening { .. }
    ));
    assert!(ws.home_transition().t() < 0.05, "from the start");
    assert_eq!(drain_find(&mut ws, "b"), vec![Lifecycle::Stopped]);
    assert_eq!(ws.home_transition().icon_rect(), Some(icon()));
}

/// Home mid-open: `HomeTransition::close` takes over the `t` and there is `Paused, Stopped` with no `Resumed`.
#[test]
fn home_during_opening_reverses_without_resumed() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, Some(icon()), &tokens);
    for _ in 0..3 {
        ws.tick(1.0 / 60.0);
    }
    let t = ws.home_transition().t();
    ws.go_home(Some(icon()), &tokens);
    assert!(ws.is_home());
    assert!(matches!(
        ws.home_transition(),
        HomeTransition::Closing { .. }
    ));
    assert!((ws.home_transition().t() - (1.0 - t)).abs() < 1e-4);
    finish(&mut ws);
    assert!(!ws.is_animating());
    assert_eq!(
        drain_find(&mut ws, "a"),
        vec![Lifecycle::Created, Lifecycle::Paused, Lifecycle::Stopped]
    );
    assert!(
        ws.active_task().is_none(),
        "the Pane is empty but the task lives on"
    );
    assert_eq!(ws.tasks().len(), 1);
    // Already home (the close finished) does nothing.
    ws.go_home(None, &tokens);
    assert!(!ws.is_animating());
}

/// A new launch mid-transition: the current transition finishes at once (with its end events) and the next starts — there is no queue.
#[test]
fn launch_during_push_settles_then_starts() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    for _ in 0..3 {
        ws.tick(1.0 / 60.0);
    }
    assert!(matches!(
        ws.stack_transition(),
        StackTransition::Pushing { .. }
    ));
    let _ = drain_find(&mut ws, "a");
    let _ = drain_find(&mut ws, "b");
    let c = instance(&mut ws, "c");
    ws.open(c, None, &tokens);
    assert!(matches!(
        ws.stack_transition(),
        StackTransition::Pushing { .. }
    ));
    assert!(ws.stack_transition().t() < 1e-6, "a new push starts at 0");
    assert_eq!(
        drain_find(&mut ws, "a"),
        vec![Lifecycle::Stopped],
        "the settle's end events"
    );
    assert_eq!(
        drain_find(&mut ws, "b"),
        vec![Lifecycle::Resumed, Lifecycle::Paused],
        "Resumed from the settle, Paused from the new push's start"
    );
    assert_eq!(ws.active_task().map(Task_len), Some(3));
}

/// A pop: `Paused` on the one leaving at the start, `Destroyed` at the end; the new top gets a
/// `Resumed` at the end. The outgoing one's slot is `len(after pop)`, so it differs from the new
/// top's (`len − 1`).
#[test]
fn pop_pauses_outgoing_then_destroys_at_end() -> crate::Result<()> {
    let mut ws = Workspace::new();
    let tokens = animated();
    let mut fixture = Fixture::new()?;
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    let b_id = ws.find("b").map(Instance::id);
    let _ = drain_find(&mut ws, "a");
    let _ = drain_find(&mut ws, "b");
    assert!(ws.pop(&tokens));
    assert!(ws.find("b").is_none(), "it leaves the stack at once");
    let StackTransition::Popping {
        outgoing: Some(out),
        ..
    } = ws.stack_transition()
    else {
        return Err(crate::Error::Config("it is not Popping".to_owned()));
    };
    assert_eq!(Some(out.id()), b_id);
    assert_eq!(out.layer_slot(), 1);
    assert_eq!(ws.find("a").map(Instance::layer_slot), Some(0));
    assert!(
        drain_find(&mut ws, "a").is_empty(),
        "at the start only the outgoing side's Paused"
    );
    finish(&mut ws);
    assert!(!ws.is_animating());
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);
    flush(&mut ws, &mut fixture);
    assert!(ws.graveyard.is_empty(), "dropped after Destroyed");
    Ok(())
}

/// A root pop: the task comes straight off the stack (`find` does not see it) and lives on as
/// `closing_task` through the A2 close before being `Destroyed` at the end. Invisible to `find`
/// though it is, `close_where` still makes the transition finish.
#[test]
fn root_pop_closes_task_through_home_transition() -> crate::Result<()> {
    let mut ws = Workspace::new();
    let tokens = animated();
    let mut fixture = Fixture::new()?;
    let a = instance(&mut ws, "a");
    ws.open(a, Some(icon()), &tokens);
    finish(&mut ws);
    assert!(ws.pop(&tokens));
    assert!(ws.is_home());
    assert!(ws.find("a").is_none() && ws.tasks().is_empty());
    assert!(matches!(
        ws.home_transition(),
        HomeTransition::Closing { .. }
    ));
    assert_eq!(
        ws.home_transition().icon_rect(),
        Some(icon()),
        "the starting point is the Rect given at open"
    );
    assert!(ws.closing_task.is_some());
    // With no name match, the transition stands.
    assert_eq!(ws.close_where(|i| i.decl_id() == "zzz", &tokens).len(), 0);
    assert!(ws.is_animating());
    // Where the one leaving matches, only the transition is finished and the list is empty (it is already closed).
    assert_eq!(ws.close_where(|i| i.decl_id() == "a", &tokens).len(), 0);
    assert!(!ws.is_animating());
    assert!(ws.closing_task.is_none());
    assert_eq!(ws.graveyard.len(), 1);
    flush(&mut ws, &mut fixture);
    assert!(ws.graveyard.is_empty());
    assert!(!ws.pop(&tokens), "at home there is nothing to pop");
    Ok(())
}

/// `close_where`: only the matches come out; one that was on top gets a `Paused` then
/// `Destroyed`, the new top a `Resumed`, an emptied task is tidied away, and with the active task
/// emptied it goes home with no animation.
#[test]
fn close_where_removes_matches_and_prunes() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    let _ = drain_find(&mut ws, "a");
    let b_id = ws.find("b").map(Instance::id);
    let closed = ws.close_where(|i| i.decl_id() == "b", &tokens);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed.first().map(|(id, _)| id.as_str()), Some("b"));
    assert_eq!(closed.first().map(|(_, id)| *id), b_id);
    assert!(!ws.is_animating(), "no animation");
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);
    assert_eq!(ws.graveyard.len(), 1);
    assert_eq!(
        ws.graveyard.first_mut().map(drain),
        Some(vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Destroyed
        ]),
        "the one that was on top gets Paused then Destroyed"
    );
    let closed = ws.close_where(|_| true, &tokens);
    assert_eq!(closed.len(), 1);
    assert!(ws.is_home() && ws.tasks().is_empty());
    assert!(!ws.is_animating());
    assert_eq!(ws.close_where(|_| true, &tokens).len(), 0);
}

/// `evict_expired` = `close_where(evict_due)`: only spawned ones whose `evict_after` has passed since going home.
#[test]
fn evict_expired_drops_stopped_owned_after_duration() -> crate::Result<()> {
    let mut ws = Workspace::new();
    let tokens = reduced();
    let mut fixture = Fixture::new()?;
    let t0 = fixture.now;
    let e = evicting(&mut ws, "e", Duration::from_millis(100));
    ws.open(e, None, &tokens);
    ws.go_home(None, &tokens);
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    ws.go_home(None, &tokens);
    flush(&mut ws, &mut fixture);
    assert_eq!(
        ws.evict_expired(t0 + Duration::from_millis(99), &tokens)
            .len(),
        0
    );
    let closed = ws.evict_expired(t0 + Duration::from_millis(100), &tokens);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed.first().map(|(id, _)| id.as_str()), Some("e"));
    assert!(
        ws.find("e").is_none() && ws.find("a").is_some(),
        "a, which has no evict_after, lives on"
    );
    assert_eq!(ws.tasks().len(), 1);
    Ok(())
}

/// `queue_all` reaches every live instance (other tasks included) and excludes the ones leaving.
#[test]
fn queue_all_reaches_every_live_instance() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    ws.go_home(None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    let c = instance(&mut ws, "c");
    ws.open(c, None, &tokens);
    finish(&mut ws);
    assert!(ws.pop(&tokens));
    let _ = drain_find(&mut ws, "a");
    let _ = drain_find(&mut ws, "b");
    ws.queue_all(Lifecycle::AccessChanged);
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::AccessChanged]);
    assert_eq!(drain_find(&mut ws, "b"), vec![Lifecycle::AccessChanged]);
    let StackTransition::Popping {
        outgoing: Some(out),
        ..
    } = &mut ws.stack
    else {
        return;
    };
    assert!(
        !drain(out).contains(&Lifecycle::AccessChanged),
        "the outgoing side does not get it"
    );
}

/// `clear_top_to` finishes the transition and clears away what is above (whichever was on top gets a `Paused` then `Destroyed`).
#[test]
fn clear_top_settles_and_buries_above() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    let c = instance(&mut ws, "c");
    ws.open(c, None, &tokens);
    ws.tick(1.0 / 60.0);
    assert!(
        !ws.clear_top_to("zzz"),
        "if it is not there the transition is left alone"
    );
    assert!(ws.is_animating());
    let _ = drain_find(&mut ws, "a");
    assert!(ws.clear_top_to("a"));
    // A 160 ms `CubicOut` cross-fade. The stack shrinks at once, but the screen cleared away stays and is drawn.
    assert!(ws.is_animating() && ws.is_clearing());
    assert_eq!(ws.clearing(), 2);
    assert_eq!(ws.clear_top_alpha(), Some(1.0));
    assert!(ws.graveyard.is_empty(), "not Destroyed yet");
    // 5 frames ≈ 83 ms: t = 83/160, alpha = 1 − CubicOut(t) = (1 − t)³.
    for _ in 0..5 {
        ws.tick(1.0 / 60.0);
    }
    let t = ws.clear_fade.value();
    assert!((t - 5.0 / 60.0 / 0.16).abs() < 1e-3, "t = {t}");
    let alpha = ws.clear_top_alpha().unwrap_or(-1.0);
    assert!(alpha > 0.0 && alpha < 1.0, "a mid alpha: {alpha}");
    assert!((alpha - (1.0 - t).powi(3)).abs() < 1e-3, "{alpha}");
    for _ in 0..5 {
        ws.tick(1.0 / 60.0);
    }
    assert!(!ws.is_animating() && ws.clear_top_alpha().is_none());
    assert_eq!(ws.active_task().map(Task_len), Some(1));
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);
    assert_eq!(ws.graveyard.len(), 2);
    assert_eq!(
        ws.graveyard.first_mut().map(drain),
        Some(vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Destroyed
        ]),
        "c: Resumed from the settle, Paused as it is cleared, Destroyed"
    );
    assert_eq!(
        ws.graveyard.get_mut(1).map(drain),
        Some(vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Stopped,
            Lifecycle::Destroyed
        ]),
        "b: Stopped from the settle, then Destroyed"
    );
    assert!(ws.clear_top_to("a"), "already on top");
    assert!(
        !ws.is_clearing(),
        "with nothing to clear there is no transition either"
    );
}

/// Under `reduce`, clear-top is immediate too.
#[test]
fn clear_top_is_immediate_when_reduced() {
    let mut ws = Workspace::new();
    let tokens = reduced();
    ws.set_motion(&tokens);
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    assert!(ws.clear_top_to("a"));
    assert!(!ws.is_clearing() && !ws.is_animating());
    assert_eq!(ws.graveyard.len(), 1);
    assert_eq!(ws.active_task().map(Task_len), Some(1));
}

/// A3 interruption and re-entry: taken hold of again mid-cancel, it carries on **from the current p**.
#[test]
fn gesture_back_regrabs_during_cancel() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    let _ = drain_find(&mut ws, "a");
    let _ = drain_find(&mut ws, "b");

    assert!(ws.begin_gesture_back());
    assert!((ws.gesture_back_grab() - 0.0).abs() < 1e-6);
    ws.drag_gesture_back(0.4, 0.0);
    assert!((ws.stack_transition().t() - 0.4).abs() < 1e-6);
    let _ = ws.release_gesture_back(false, &tokens);
    assert_eq!(ws.stack_transition().back_confirmed(), Some(false));
    assert_eq!(
        drain_find(&mut ws, "b"),
        vec![Lifecycle::Paused, Lifecycle::Resumed]
    );

    // It is caught while the return spring is running (= while it is not yet `Idle`). `p` being 0..1,
    // it hits `Animated`'s absolute settle test (0.5) and finishes on the first tick — raised
    // as a request against `Animated`.
    let p = ws.stack_transition().t();
    assert!((p - 0.4).abs() < 1e-6, "where it was let go: {p}");

    // Take hold again: `confirmed` is released and the new dx is added to where it was caught.
    assert!(
        ws.begin_gesture_back(),
        "it is caught even while coming back from a cancel"
    );
    assert_eq!(ws.stack_transition().back_confirmed(), None);
    assert!((ws.gesture_back_grab() - p).abs() < 1e-6);
    assert_eq!(drain_find(&mut ws, "b"), vec![Lifecycle::Paused]);
    ws.drag_gesture_back(0.1, 0.0);
    assert!(
        (ws.stack_transition().t() - (p + 0.1)).abs() < 1e-6,
        "it carries on from the current p: {}",
        ws.stack_transition().t()
    );

    // Confirmed, b comes off the stack and is Destroyed at the end of the transition.
    let _ = ws.release_gesture_back(true, &tokens);
    assert_eq!(ws.active_task().map(Task_len), Some(1));
    for _ in 0..120 {
        ws.tick(1.0 / 60.0);
        if !ws.is_animating() {
            break;
        }
    }
    assert!(!ws.is_animating());
    assert_eq!(ws.graveyard.len(), 1);
    assert_eq!(
        ws.graveyard.first_mut().map(drain),
        Some(vec![Lifecycle::Destroyed])
    );
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);
}

/// The back gesture does not start at the root (a stack of 1) — kept as it is, since its mapping
/// differs from an A2 close and the finger cannot be carried over.
#[test]
fn gesture_back_does_not_start_at_the_root() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    assert!(!ws.begin_gesture_back());
    assert!(!ws.is_animating());
}

/// reduce: every transition is immediate, and the event order is a fixed sequence.
#[test]
fn reduce_gives_the_fixed_lifecycle_sequence() -> crate::Result<()> {
    let mut ws = Workspace::new();
    let tokens = reduced();
    let mut fixture = Fixture::new()?;
    let a = instance(&mut ws, "a");
    ws.open(a, Some(icon()), &tokens);
    assert!(!ws.is_animating());
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    assert!(ws.pop(&tokens));
    assert_eq!(
        drain_find(&mut ws, "a"),
        vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Stopped,
            Lifecycle::Resumed,
        ]
    );
    assert_eq!(
        ws.graveyard.first_mut().map(drain),
        Some(vec![
            Lifecycle::Created,
            Lifecycle::Resumed,
            Lifecycle::Paused,
            Lifecycle::Destroyed
        ])
    );
    ws.go_home(Some(icon()), &tokens);
    assert!(ws.is_home() && !ws.is_animating());
    assert_eq!(
        drain_find(&mut ws, "a"),
        vec![Lifecycle::Paused, Lifecycle::Stopped]
    );
    assert!(ws.resume_task_with_root("a", Some(icon()), &tokens));
    assert_eq!(drain_find(&mut ws, "a"), vec![Lifecycle::Resumed]);
    assert!(ws.pop(&tokens), "the root pop");
    assert!(ws.is_home() && ws.tasks().is_empty() && !ws.is_animating());
    flush(&mut ws, &mut fixture);
    assert!(ws.graveyard.is_empty());
    Ok(())
}

/// `close_instance`: on the top it pops, otherwise quietly (the transition finishes).
#[test]
fn close_instance_top_pops_and_hidden_is_silent() {
    let mut ws = Workspace::new();
    let tokens = animated();
    let a = instance(&mut ws, "a");
    ws.open(a, None, &tokens);
    finish(&mut ws);
    let b = instance(&mut ws, "b");
    ws.open(b, None, &tokens);
    finish(&mut ws);
    let a_id = ws.find("a").map(Instance::id);
    let b_id = ws.find("b").map(Instance::id);
    assert!(!ws.close_instance(InstanceId(99), &tokens));
    assert!(a_id.is_some_and(|id| ws.close_instance(id, &tokens)));
    assert!(!ws.is_animating(), "a hidden one with no animation");
    assert_eq!(ws.active_task().map(Task_len), Some(1));
    assert!(b_id.is_some_and(|id| ws.close_instance(id, &tokens)));
    assert!(ws.is_home() && matches!(ws.home_transition(), HomeTransition::Closing { .. }));
}

/// `scale_about` keeps the centre fixed.
#[test]
fn scale_about_keeps_center() {
    let c = pos2(512.0, 300.0);
    let t = super::scale_about(c, 0.92);
    let moved = t * c;
    assert!((moved - c).length() < 1e-3);
    let corner = t * pos2(0.0, 0.0);
    assert!((corner.x - c.x * (1.0 - 0.92)).abs() < 1e-3);
    assert_eq!(
        super::scale_about(c, 1.0),
        egui::emath::TSTransform::IDENTITY
    );
}
