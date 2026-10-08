//! M5 — two panes (A8).
//!
//! What a device gets from the shell's split: a screen opened "in the other pane" splits the
//! content with the one on show, the pane last pressed has the focus, the divider follows a
//! finger and settles where both screens keep their minimums, and the split ends when a pane's
//! stack empties, when the divider is pushed to an end, or when the Home view comes back.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::screen::ScreenDecl;
use fairing::testing::{test_shell, Harness};
use fairing::workspace::Instance;
use fairing::{
    screen, screen_with, Cx, LaunchAction, Lifecycle, PaneInfo, Screen, ScreenValue, ShellConfig,
    ShellEvent, SplitSupport,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// What each screen last saw of its pane, by declaration id.
type Seen = Rc<RefCell<Vec<(String, PaneInfo)>>>;

/// A screen that writes down the pane it was drawn in.
fn noting(id: &'static str, seen: &Seen) -> ScreenDecl {
    let seen = Rc::clone(seen);
    screen(id, move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        ui.label(id);
        let mut seen = seen.borrow_mut();
        seen.retain(|(name, _)| name != id);
        seen.push((id.to_owned(), cx.pane));
    })
}

fn pane_of(seen: &Seen, id: &str) -> Option<PaneInfo> {
    seen.borrow()
        .iter()
        .find(|(name, _)| name == id)
        .map(|(_, pane)| *pane)
}

/// A shell with screens `a`, `b` and `c`, each noting its pane.
fn shell(seen: &Seen) -> fairing::Result<Harness> {
    shell_with(seen, ShellConfig::default(), |_| {})
}

fn shell_with(
    seen: &Seen,
    config: ShellConfig,
    more: impl FnOnce(&mut fairing::Shell),
) -> fairing::Result<Harness> {
    let mut h = test_shell(config, |sh| {
        for id in ["a", "b", "c"] {
            sh.add(noting(id, seen));
        }
        more(sh);
    })?;
    h.frames(2);
    Ok(h)
}

fn open_beside(h: &mut Harness, id: &str) {
    h.shell.launch(LaunchAction::Open {
        id: id.to_owned(),
        in_other_pane: true,
    });
    h.frames(3);
}

fn split_events(h: &mut Harness) -> Vec<bool> {
    h.shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::SplitToggled(on) => Some(on),
            _ => None,
        })
        .collect()
}

/// `a` on show, then `b` beside it.
fn split_ab(h: &mut Harness) -> fairing::Result<()> {
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    open_beside(h, "b");
    if !h.shell.workspace().is_split() {
        return Err(fail("`b` did not open beside `a`"));
    }
    Ok(())
}

/// "open in the other pane" splits the content — the pane already there keeps the first
/// half, the new screen takes the second and the focus, and both screens are told they are split.
#[test]
fn opening_in_the_other_pane_splits_and_focuses_it() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let ws = h.shell.workspace();
    assert_eq!(
        ws.pane_task(0).and_then(|t| t.root_id()),
        Some("a"),
        "the first pane"
    );
    assert_eq!(ws.pane_task(1).and_then(|t| t.root_id()), Some("b"));
    assert_eq!(ws.focused_pane(), 1);
    let (left, right) = (
        ws.pane_rect(0).ok_or_else(|| fail("no first pane"))?,
        ws.pane_rect(1).ok_or_else(|| fail("no second pane"))?,
    );
    assert!(
        left.max.x <= right.min.x,
        "side by side on a landscape panel"
    );
    assert!(
        (left.width() - right.width()).abs() < 1.0,
        "even: {left:?} {right:?}"
    );
    let a = pane_of(&seen, "a").ok_or_else(|| fail("`a` not drawn"))?;
    let b = pane_of(&seen, "b").ok_or_else(|| fail("`b` not drawn"))?;
    assert!(a.is_split && b.is_split);
    assert!(b.is_focused && !a.is_focused);
    assert!(
        left.contains_rect(a.rect) && right.contains_rect(b.rect),
        "each screen in its own pane"
    );
    assert_eq!(split_events(&mut h), vec![true]);
    Ok(())
}

/// The pane last pressed has the focus, and the chrome policy is the focused pane's.
#[test]
fn a_press_moves_the_focus() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let left = h
        .shell
        .workspace()
        .pane_rect(0)
        .ok_or_else(|| fail("no first pane"))?;
    h.tap(left.center());
    h.frames(1);
    assert_eq!(h.shell.workspace().focused_pane(), 0);
    let a = pane_of(&seen, "a").ok_or_else(|| fail("`a` not drawn"))?;
    assert!(a.is_focused);
    Ok(())
}

/// Drag the divider's handle from where it is to `to`, in a few steps.
fn drag_divider(h: &mut Harness, to: egui::Pos2) -> fairing::Result<()> {
    let handle = h
        .shell
        .workspace()
        .divider_rect()
        .ok_or_else(|| fail("no divider"))?;
    h.drag(handle.center(), to, 6);
    h.frames(2);
    Ok(())
}

/// A8: the divider follows the finger, and a release keeps it where both panes keep their
/// minimums — a quarter of the content each by default.
#[test]
fn the_divider_follows_the_finger_and_settles_inside_the_minimums() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let content = h.shell.workspace().last_content();
    let y = content.center().y;
    drag_divider(&mut h, egui::pos2(content.min.x + content.width() * 0.4, y))?;
    let ratio = h.shell.workspace().split_ratio().unwrap_or(0.0);
    assert!((ratio - 0.4).abs() < 0.02, "followed to 0.4: {ratio}");
    drag_divider(
        &mut h,
        egui::pos2(content.min.x + content.width() * 0.15, y),
    )?;
    let ratio = h.shell.workspace().split_ratio().unwrap_or(0.0);
    assert!(
        (0.24..0.27).contains(&ratio),
        "back to the first pane's minimum: {ratio}"
    );
    assert!(h.shell.workspace().is_split(), "still two panes");
    Ok(())
}

/// A double tap on the divider evens the panes out (A8).
#[test]
fn a_double_tap_evens_the_divider_out() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let content = h.shell.workspace().last_content();
    drag_divider(
        &mut h,
        egui::pos2(content.min.x + content.width() * 0.35, content.center().y),
    )?;
    let handle = h
        .shell
        .workspace()
        .divider_rect()
        .ok_or_else(|| fail("no divider"))?;
    h.tap(handle.center());
    h.tap(handle.center());
    h.frames(2);
    let ratio = h.shell.workspace().split_ratio().unwrap_or(0.0);
    assert!((ratio - 0.5).abs() < 0.01, "even: {ratio}");
    Ok(())
}

/// Pushed to an end, the divider closes the pane it was pushed into; that pane's task stays
/// alive in the background.
#[test]
fn pushing_the_divider_to_an_end_closes_that_pane() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let _ = split_events(&mut h);
    let content = h.shell.workspace().last_content();
    drag_divider(&mut h, egui::pos2(content.min.x + 4.0, content.center().y))?;
    let ws = h.shell.workspace();
    assert!(!ws.is_split());
    assert_eq!(
        ws.pane_task(0).and_then(|t| t.root_id()),
        Some("b"),
        "the pane left"
    );
    assert!(ws.find("a").is_some(), "`a` lives on in the background");
    assert_eq!(split_events(&mut h), vec![false]);
    Ok(())
}

/// Back at a pane's root ends that task — and with it the split; the other pane fills the
/// content, and this is not the way home.
#[test]
fn back_at_a_panes_root_ends_its_task_and_the_split() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    h.shell.back();
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(!ws.is_split() && !ws.is_home());
    assert!(ws.find("b").is_none(), "`b`'s task ended");
    assert_eq!(ws.focused().map(Instance::decl_id), Some("a"));
    Ok(())
}

/// Home from a split takes both panes home; both tasks stay alive, and the next open is one
/// pane again.
#[test]
fn home_from_a_split_goes_home_with_both() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    h.shell.home();
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(ws.is_home() && !ws.is_split());
    assert!(ws.find("a").is_some() && ws.find("b").is_some());
    h.shell.launch(LaunchAction::open("c"));
    h.frames(3);
    assert!(!h.shell.workspace().is_split());
    Ok(())
}

/// A screen that is never split opens in the same pane, and one whose minimum the
/// content cannot give leaves the split unmade.
#[test]
fn a_screen_that_cannot_share_opens_in_the_same_pane() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(&seen, ShellConfig::default(), |sh| {
        sh.add(
            screen("solo", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("solo");
            })
            .split(SplitSupport::No),
        );
        sh.add(
            screen("wide", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("wide");
            })
            .split(SplitSupport::MinSize(egui::vec2(900.0, 100.0))),
        );
    })?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    open_beside(&mut h, "solo");
    assert!(!h.shell.workspace().is_split());
    assert_eq!(
        h.shell.workspace().focused().map(Instance::decl_id),
        Some("solo")
    );
    h.shell.back();
    h.frames(3);
    open_beside(&mut h, "wide");
    assert!(
        !h.shell.workspace().is_split(),
        "900 and a quarter do not fit"
    );
    Ok(())
}

/// The split control (the tile, the nav bar's `"split"`) without the shell's overview: the task
/// used last comes in beside the one on show, and the control again goes back to one pane, keeping
/// the focused one. (With the overview it opens the picker — `m5_overview.rs`.)
#[test]
fn the_split_control_toggles_the_split() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut config = ShellConfig::default();
    config.workspace.overview = false;
    let mut h = shell_with(&seen, config, |_| {})?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::ToggleSplit);
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(ws.is_split());
    assert_eq!(ws.pane_task(1).and_then(|t| t.root_id()), Some("a"));
    h.shell.launch(LaunchAction::ToggleSplit);
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(!ws.is_split());
    assert_eq!(
        ws.focused().map(Instance::decl_id),
        Some("a"),
        "the focused pane stayed"
    );
    let requested = h
        .shell
        .poll_events()
        .into_iter()
        .filter(|e| matches!(e, ShellEvent::SplitRequested))
        .count();
    assert_eq!(requested, 2, "reported every time");
    Ok(())
}

/// Off in `[workspace]`, the split control is only reported and "the other pane" is this one.
#[test]
fn with_the_split_off_the_control_is_only_reported() -> fairing::Result<()> {
    let seen: Seen = Rc::new(RefCell::new(Vec::new()));
    let mut config = ShellConfig::default();
    config.workspace.split = false;
    let mut h = shell_with(&seen, config, |_| {})?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    open_beside(&mut h, "b");
    assert!(!h.shell.workspace().is_split());
    assert_eq!(
        h.shell.workspace().focused().map(Instance::decl_id),
        Some("b")
    );
    Ok(())
}

/// A screen recording its lifecycle.
struct Recorder(Rc<RefCell<Vec<Lifecycle>>>);

impl Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("recorder");
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.0.borrow_mut().push(event);
    }
}

/// `Resized` comes once at the end of the enter tween, with the pane's size — not every frame of
/// it (A8).
#[test]
fn each_pane_hears_its_size_once_when_the_split_is_in() -> fairing::Result<()> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut config = ShellConfig::default();
    config.motion.reduce = false;
    let services = fairing::services::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(config, services)?;
    let recorder = Rc::clone(&log);
    h.shell.add(fairing::screen_with("r", move || {
        Recorder(Rc::clone(&recorder))
    }));
    h.shell
        .add(screen("b", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("b");
        }));
    h.frames(2);
    h.shell.launch(LaunchAction::open("r"));
    h.run_for(0.6);
    log.borrow_mut().clear();
    h.shell.launch(LaunchAction::Open {
        id: "b".to_owned(),
        in_other_pane: true,
    });
    h.run_for(0.1);
    assert!(
        !log.borrow()
            .iter()
            .any(|e| matches!(e, Lifecycle::Resized(_))),
        "nothing mid-tween"
    );
    h.run_for(0.5);
    let sizes: Vec<egui::Vec2> = log
        .borrow()
        .iter()
        .filter_map(|e| match e {
            Lifecycle::Resized(size) => Some(*size),
            _ => None,
        })
        .collect();
    let pane = h
        .shell
        .workspace()
        .pane_rect(0)
        .ok_or_else(|| fail("no first pane"))?;
    assert_eq!(sizes.len(), 1, "once: {sizes:?}");
    assert!(
        sizes
            .first()
            .is_some_and(|s| (s.x - pane.width()).abs() < 1.0),
        "the pane's width: {sizes:?} vs {pane:?}"
    );
    Ok(())
}

/// A viewer/maintainer table where everything is the viewer's but `workspace.split`, which is
/// the maintainer's — and the session is the viewer's.
fn split_gated() -> ShellConfig {
    let mut config = fairing::testing::access_config(&["viewer", "maintainer"], Some("bottom"));
    config
        .access
        .gates
        .insert("workspace.split".to_owned(), "maintainer".to_owned());
    config
}

/// Short of `workspace.split`, the split control asks the session up instead of
/// splitting, and "open in the other pane" opens in the same pane — the screen still opens.
#[test]
fn the_split_gate_holds_a_split_back() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let mut h = shell_with(&seen, split_gated(), |_| {})?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    let _ = h.shell.poll_events();
    h.shell.launch(LaunchAction::ToggleSplit);
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(!ws.is_split() && !ws.is_overview_open());
    assert!(h.shell.poll_events().iter().any(|e| matches!(
        e,
        ShellEvent::Access(fairing::access::AccessEvent::UnlockRequested { gate, .. })
            if gate.as_str() == "workspace.split"
    )));
    open_beside(&mut h, "c");
    let ws = h.shell.workspace();
    assert!(!ws.is_split());
    assert_eq!(ws.focused().map(Instance::decl_id), Some("c"));
    Ok(())
}

fn pane(h: &Harness, index: usize) -> fairing::Result<egui::Rect> {
    h.shell
        .workspace()
        .pane_rect(index)
        .ok_or_else(|| fail(format!("no pane {index}")))
}

/// A press on something drawn over the panes — here the shade — leaves the focus where it is:
/// typing on the keyboard over the other pane must not hand that pane the focus.
#[cfg(feature = "overlay")] // Without the shade there is nothing over the panes to press.
#[test]
fn a_press_on_what_covers_the_panes_keeps_the_focus() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    assert_eq!(h.shell.workspace().focused_pane(), 1);
    let left = pane(&h, 0)?;
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    let at = egui::pos2(left.center().x, left.max.y - 20.0);
    let covered = h
        .ctx
        .layer_id_at(at)
        .is_some_and(|layer| layer.order != egui::Order::Background);
    assert!(
        covered,
        "the shade should cover {at:?} for this test to mean anything"
    );
    h.tap(at);
    h.frames(2);
    assert_eq!(
        h.shell.workspace().focused_pane(),
        1,
        "a press on the shade moved the focus to the pane under it"
    );
    Ok(())
}

/// A divider drag cut short — the handle went (here the cards came up) before the finger lifted
/// — is let go where it stands: the divider does not stay "under a finger" for good, with a pane
/// squeezed past its minimum.
#[test]
fn a_divider_drag_cut_short_is_let_go() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let content = h.shell.layout().content;
    let handle = h
        .shell
        .workspace()
        .divider_rect()
        .ok_or_else(|| fail("no divider"))?;
    // Into the first pane's minimum (a quarter), short of closing it (a tenth).
    let to = egui::pos2(content.min.x + content.width() * 0.15, handle.center().y);
    h.press(handle.center());
    h.frames(1);
    for step in 1..=6_u8 {
        let t = f32::from(step) / 6.0;
        h.move_to(handle.center() + (to - handle.center()) * t);
        h.frames(1);
    }
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(2);
    h.release(to);
    h.frames(2);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(ws.is_split() && !ws.is_overview_open());
    let ratio = ws.split_ratio().ok_or_else(|| fail("no ratio"))?;
    assert!(ratio > 0.24, "the divider stayed squeezed at {ratio}");
    Ok(())
}

/// A screen that opens `target` on its own — on the first frame a shared flag is up, with no press
/// in its pane (an answer arriving, a timer).
fn opener(id: &'static str, target: &'static str, go: &Rc<Cell<bool>>) -> ScreenDecl {
    let go = Rc::clone(go);
    screen(id, move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        ui.label(id);
        if go.replace(false) {
            cx.open(target);
        }
    })
}

/// The focus is the pane of the last input. A screen in the other pane opening something
/// on its own opens it in its pane — and the user keeps theirs.
#[test]
fn a_screen_opening_on_its_own_in_the_other_pane_keeps_the_users_focus() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let go = Rc::new(Cell::new(false));
    let g = Rc::clone(&go);
    let mut h = shell_with(&seen, ShellConfig::default(), move |sh| {
        sh.add(opener("watch", "c", &g));
    })?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    open_beside(&mut h, "watch");
    let left = pane(&h, 0)?;
    h.tap(left.center());
    h.frames(2);
    assert_eq!(h.shell.workspace().focused_pane(), 0);
    go.set(true);
    h.frames(4);
    let ws = h.shell.workspace();
    assert_eq!(ws.focused_pane(), 0, "the other pane took the focus");
    assert_eq!(
        ws.pane_task(1)
            .and_then(fairing::workspace::Task::top)
            .map(Instance::decl_id),
        Some("c"),
        "`c` opens in the pane of the screen that opened it"
    );
    Ok(())
}

/// A parent that writes down the results it is handed.
struct Parent {
    name: &'static str,
    got: Rc<RefCell<Vec<String>>>,
}

impl Screen for Parent {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label(self.name);
    }

    fn on_result(&mut self, from: &str, value: ScreenValue, _cx: &mut Cx<'_>) {
        if let ScreenValue::Text(text) = value {
            self.got
                .borrow_mut()
                .push(format!("{}<-{from}:{text}", self.name));
        }
    }
}

/// A child finishing in the other pane — on its own, the user working in the first — hands its
/// result to its own parent, not to the focused pane's screen.
#[test]
fn a_result_goes_to_the_parent_in_its_own_pane() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let got = Rc::new(RefCell::new(Vec::new()));
    let go = Rc::new(Cell::new(false));
    let (g1, g2, done) = (Rc::clone(&got), Rc::clone(&got), Rc::clone(&go));
    let mut h = shell_with(&seen, ShellConfig::default(), move |sh| {
        sh.add(screen_with("left", move || Parent {
            name: "left",
            got: Rc::clone(&g1),
        }));
        sh.add(screen_with("right", move || Parent {
            name: "right",
            got: Rc::clone(&g2),
        }));
        sh.add(screen("pick", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            ui.label("pick");
            if done.replace(false) {
                cx.finish_with(ScreenValue::Text("x".to_owned()));
            }
        }));
    })?;
    h.shell.launch(LaunchAction::open("left"));
    h.frames(3);
    open_beside(&mut h, "right");
    h.shell.launch(LaunchAction::open("pick"));
    h.frames(3);
    let left = pane(&h, 0)?;
    h.tap(left.center());
    h.frames(2);
    assert_eq!(h.shell.workspace().focused_pane(), 0);
    go.set(true);
    h.frames(4);
    assert_eq!(*got.borrow(), vec!["right<-pick:x".to_owned()]);
    assert_eq!(h.shell.workspace().focused_pane(), 0);
    Ok(())
}

/// A screen on show that keeps the display awake keeps it awake whichever pane has the focus.
#[test]
fn keep_awake_holds_from_either_pane() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let mut h = shell_with(&seen, ShellConfig::default(), |sh| {
        sh.add(
            screen("video", |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
                ui.label("video");
            })
            .keep_awake(),
        );
    })?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    open_beside(&mut h, "video");
    let left = pane(&h, 0)?;
    h.tap(left.center());
    h.frames(2);
    assert_eq!(h.shell.workspace().focused_pane(), 0);
    assert!(h.shell.workspace().chrome_policy().keep_awake);
    Ok(())
}

/// A drag cut short within a tenth of an end closes nothing: nobody let go there.
#[test]
fn a_drag_cut_short_near_an_end_closes_nothing() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let mut h = shell(&seen)?;
    split_ab(&mut h)?;
    let content = h.shell.layout().content;
    let handle = h
        .shell
        .workspace()
        .divider_rect()
        .ok_or_else(|| fail("no divider"))?;
    let to = egui::pos2(content.min.x + content.width() * 0.04, handle.center().y);
    h.press(handle.center());
    h.frames(1);
    for step in 1..=6_u8 {
        let t = f32::from(step) / 6.0;
        h.move_to(handle.center() + (to - handle.center()) * t);
        h.frames(1);
    }
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(2);
    h.release(to);
    h.frames(2);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(4);
    assert!(
        h.shell.workspace().is_split(),
        "a pane closed that nobody let go of"
    );
    Ok(())
}

/// What a screen has heard of its size.
type Sizes = Rc<RefCell<Vec<(String, egui::Vec2)>>>;

/// A screen that writes down every `Resized` it hears.
fn sizing(id: &'static str, heard: &Sizes) -> ScreenDecl {
    let heard = Rc::clone(heard);
    screen(id, move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        ui.label(id);
        if let Some(Lifecycle::Resized(size)) = cx.event {
            heard.borrow_mut().push((id.to_owned(), size));
        }
    })
}

/// A task that changes size without a tween of its own — here put into the other pane from the
/// background, where it last had the whole content — hears its new size.
#[test]
fn a_task_put_into_a_pane_hears_its_new_size() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let heard: Sizes = Rc::default();
    let h2 = Rc::clone(&heard);
    let mut h = shell_with(&seen, ShellConfig::default(), move |sh| {
        sh.add(sizing("sized", &h2));
    })?;
    h.shell.launch(LaunchAction::open("sized"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    open_beside(&mut h, "b");
    heard.borrow_mut().clear();
    // `sized` goes into the pane `b` was in.
    h.shell.launch(LaunchAction::Open {
        id: "sized".to_owned(),
        in_other_pane: true,
    });
    h.frames(4);
    let pane = pane(&h, 1)?;
    let heard = heard.borrow();
    assert!(
        heard
            .iter()
            .any(|(id, size)| id == "sized" && (size.x - pane.width()).abs() < 1.0),
        "never told its pane's width: {heard:?} vs {pane:?}"
    );
    Ok(())
}

/// The panes take a tap while the divider settles: a press right after letting go of it moves the
/// focus, rather than being lost to a spring nobody is watching.
#[test]
fn the_panes_take_a_press_while_the_divider_settles() -> fairing::Result<()> {
    let seen: Seen = Rc::default();
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(ShellConfig::default(), services)?;
    for id in ["a", "b"] {
        h.shell.add(noting(id, &seen));
    }
    h.frames(2);
    h.shell.launch(LaunchAction::open("a"));
    h.run_for(0.6);
    h.shell.launch(LaunchAction::Open {
        id: "b".to_owned(),
        in_other_pane: true,
    });
    h.run_for(0.6);
    assert_eq!(h.shell.workspace().focused_pane(), 1);
    let handle = h
        .shell
        .workspace()
        .divider_rect()
        .ok_or_else(|| fail("no divider"))?;
    let content = h.shell.layout().content;
    // Into the first pane's minimum, so the release springs back out of it.
    let to = egui::pos2(content.min.x + content.width() * 0.18, handle.center().y);
    h.drag(handle.center(), to, 4);
    assert!(
        h.shell.workspace().is_animating(),
        "the divider springs back"
    );
    let left = pane(&h, 0)?;
    h.tap(egui::pos2(left.min.x + 40.0, left.center().y));
    assert_eq!(h.shell.workspace().focused_pane(), 0);
    Ok(())
}

/// A8's timings are `[motion.panes]` and A10's `[motion.overview]`, like every other motion.
#[test]
fn the_split_and_overview_timings_are_motion_tokens() -> fairing::Result<()> {
    let config = ShellConfig::from_toml(
        "[motion.panes]\nenter_ms = 300\neven_ms = 90\n[motion.overview]\nin_ms = 120\nthrow_ms = 80\n",
    )?;
    let tokens = fairing::theme::MotionTokens::from_config(&config.motion);
    assert_eq!(tokens.panes_enter.duration.as_millis(), 300);
    assert_eq!(tokens.panes_leave.duration.as_millis(), 220, "the default");
    assert_eq!(tokens.panes_even.duration.as_millis(), 90);
    assert_eq!(tokens.overview_in.duration.as_millis(), 120);
    assert_eq!(tokens.overview_throw.duration.as_millis(), 80);
    Ok(())
}
