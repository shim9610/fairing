//! The workspace — either the Home view or its panes, each showing a task stack. One
//! pane, or two side by side or one above the other while split (A8 — the split's
//! geometry and motion are in `split.rs`, the recent screens in `overview.rs`).
//!
//! The render convention: the desktop and the screen instances are each drawn as an `Area`
//! (`Order::Background`) on a different `LayerId`, and through a transition the layer is moved
//! whole with `Context::set_transform_layer` and faded with `Ui::set_opacity`. Screen code knows
//! none of it. The screen layer ids are a **finite set** (the Pane stack depth = the slot) — egui's
//! `Areas` never prunes, so a new id per instance would grow the memory for ever (the head comment
//! of `instance.rs`).
//!
//! The transition rules (A2 · A3):
//! - **Interruption**: a new launch / back / close mid-transition finishes the one in progress at
//!   once (`Workspace::settle`) and starts (there is no queue). The exception is A2's reverse
//!   continuation — home during an A2 open closes from the same `t`, and resuming the same task
//!   during an A2 close opens from the same `t`.
//! - **Lifecycle**: at the start, only `Paused` on the side losing focus; at the end, on a push
//!   `Stopped` below and `Resumed` above, on a pop `Destroyed` for the one leaving, and on a home
//!   close `Stopped` for the whole task (or `Destroyed` where a root pop ends the task). An A2
//!   open's first `ui` comes at `t ≥ 0.5`.
//! - **Blocking input**: through a transition every screen and desktop `Area` is left
//!   `interactable(false)` — egui 0.36's hit test filters out that layer's widgets whole through
//!   `Areas::is_interactable` (`context.rs` `begin_pass_mut`). A transparent shield `Area` over the
//!   chrome is the M2 gesture shield's job.
//! - **z order**: `Areas::end_pass` does `order.sort_by_key(|l| (l.order, wants_to_be_on_top))` —
//!   **a stable sort**, so among layers that called `move_to_top` on the same frame the existing
//!   relative order stays. So `draw_panes` flags **only the one layer that should be on top** each
//!   frame (`raises_outgoing`): Pushing = the one coming in, Popping = **the one going out**, A2 =
//!   the top, Idle = the top. egui also raises an `Area` that is `!visible_last_frame` by itself (a
//!   desktop coming back into view, the incoming slot on a pop's first frame), so without the "only
//!   one" rule the order gets dragged along by history.
//! - **Slot warm-up**: an egui `Area` with no `AreaState` runs its first frame as a sizing pass and
//!   throws the drawing away whole (`containers/area.rs:444`). `Areas::set_state` is `pub(crate)`,
//!   so the state cannot be planted; instead the shell draws the slot `Area` once beforehand with
//!   nothing in it and **consumes the sizing pass first** (`Workspace::warm_layer_slots`).

mod gesture_nav;
mod instance;
mod overview;
mod painters;
mod split;
mod task;
mod transition;

pub use instance::{Instance, InstanceId, InstanceScreen};
pub use overview::DrawnCard;
pub(crate) use painters::RecentsPainters;
pub use painters::{
    RecentCardCx, RecentCardPainter, RecentsGroundCx, RecentsGroundPainter, RecentsOver,
};
pub use split::SplitAxis;
pub use task::Task;
#[doc(hidden)]
pub use transition::{HomeTransition, StackTransition};

use crate::desktop::{DesktopAction, DesktopCtx, DesktopView};
use crate::icons::{IconRef, IconStyle};
use crate::motion::{Animated, Easing, Tween};
use crate::screen::{BarMode, ChromePolicy, CxParts, Lifecycle, PaneInfo, SplitSupport};
use crate::shell::Layout;
use crate::theme::{ColorRole, MotionTokens};
use egui::emath::TSTransform;
use egui::{Color32, Pos2, Rect};
use gesture_nav::{Lift, QuickOrder, Switch};
pub(crate) use overview::Mode as OverviewMode;

/// How far a lift fades two panes at the top of its travel — they fade where they
/// stand, as under the overview, rather than one shrinking away from the other.
const LIFT_SPLIT_FADE: f32 = 0.4;
use overview::{Card, Overview, Picked};
use painters::paint_ground;
use split::{Phase, Released, Split};
use std::time::Instant;

/// How many slots have a sizing pass run through them on the first frame (a stack depth of up to 4 is free).
const WARM_SLOTS: u32 = 4;

/// The layer slots a pane may borrow: pane `i` borrows from `i × PANE_SLOTS` up, so the two panes'
/// screens never share a layer. A stack deeper than this in one pane would — and it draws only its
/// top two anyway.
const PANE_SLOTS: u32 = 32;

/// The divider's `Area` — the band, its grip, the focus line, and the handle a finger takes.
const DIVIDER_ID: &str = "fairing.split.divider";

/// The workspace view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkspaceView {
    /// The desktop.
    #[default]
    Home,
    /// A Pane (a task).
    Tasks,
}

/// A Pane identifier (a leaf of the M5 tree).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(pub u8);

/// A Pane — the area showing one task. One, or two while split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pane {
    /// id.
    pub id: PaneId,
    /// The task attached (an index into `Workspace::tasks`).
    pub task: Option<usize>,
}

/// [`Workspace::ui`]'s result.
#[derive(Debug, Default)]
pub(crate) struct WorkspaceOutput {
    /// What happened on the desktop (an icon tap, and so on).
    pub(crate) desktop_action: Option<DesktopAction>,
    /// The screens the overview closed (a card thrown away, "Close all") — the shell reports each
    /// as `ScreenClosed`.
    pub(crate) closed: Vec<(String, InstanceId)>,
}

/// A back gesture held out of the way ([`Workspace::hold_back_gesture`]): the drag, the pane it
/// is in and the screen it is dragging.
pub(crate) struct HeldBack {
    stack: StackTransition,
    pane: usize,
    top: Option<InstanceId>,
}

/// The workspace.
pub struct Workspace {
    view: WorkspaceView,
    tasks: Vec<Task>,
    panes: Vec<Pane>,
    home: HomeTransition,
    stack: StackTransition,
    /// The OSK inset (M2, A5). `pane_info` hands it to the screen every frame.
    inset_bottom: f32,
    /// The task ended by a root pop — the task version of `StackTransition::Popping.outgoing`. It
    /// is drawn from here only through the A2 close (in neither the stack nor `find`), then buried.
    closing_task: Option<Task>,
    /// An instance that has ended but whose `Destroyed` notification has not gone out yet.
    graveyard: Vec<Instance>,
    next_id: u64,
    last_content: Rect,
    /// The current Pane task's A2 origin — the last icon Rect the shell handed over. Used by paths
    /// where the shell hands no Rect, such as a root pop → home (`icon_rect` is the last value the
    /// desktop drew, so it is stable).
    origin: Option<Rect>,
    /// A transition has ended — put any layer transform left over back to `IDENTITY` on the next
    /// `ui` (`set_transform_layer` is sticky, so a value stays on a layer that is not drawn).
    layers_dirty: bool,
    /// How many slots of each pane have already had their sizing pass consumed (= the next slot
    /// number to warm up, from the pane's base).
    warmed_slots: [u32; 2],
    /// The instances being cleared by the clear-top cross-fade, **from the top down**.
    /// Only the first (originally the top) is drawn — those below it are covered anyway. All
    /// go to the graveyard at the end of the transition.
    clearing: Vec<Instance>,
    /// The clear-top cross-fade's driving value `t ∈ [0, 1]` (raw progress). Why it is not a
    /// [`StackTransition`] variant is in [`transition::clear_top_alpha`]'s docs.
    clear_fade: Animated<f32>,
    /// The `p` at which the back gesture was **taken hold of again** (A3 interruption and
    /// re-entry). A new press's `dx / W` is added to this. It is 0 on a fresh start.
    back_grab: f32,
    /// Last frame's motion tokens. [`Workspace::ui`] caches them from `parts.theme.motion` —
    /// [`Workspace::clear_top_to`] is the one entry point the shell hands no tokens to,
    /// so it reads them here. Before the first `ui` they are the defaults.
    tokens: MotionTokens,
    /// The focused pane, as an index into `panes` (the pane last pressed). 0 with one pane.
    focus: usize,
    /// The split, while there are two panes (A8).
    split: Option<Split>,
    /// `[workspace] split_axis`: `None` follows the content's shape.
    axis_rule: Option<SplitAxis>,
    /// Each pane's rect as last drawn, before the screen inset — what a `Cx` made outside the
    /// draw, the back gesture's width and the focus test go by.
    pane_rects: [Rect; 2],
    /// The rect each pane's screen was last laid out in (its visible rect, grown to its minimum).
    pane_laid: [Rect; 2],
    /// The task leaving with its pane — a root pop in a split. Drawn sliding out with the pane,
    /// then buried (the split version of `closing_task`).
    leaving_task: Option<Task>,
    /// Where the divider's handle was last drawn (`Rect::NOTHING` with one pane). A press there
    /// moves the divider, not the focus.
    divider_handle: Rect,
    /// Last frame's divider thickness and touch target — what a split decided outside the draw
    /// (whether one can hold, its first ratio) measures with.
    split_metrics: (f32, f32),
    /// The overview, while it is up (A10).
    overview: Option<Overview>,
    /// What the overview closed in `tick` (a thrown card landing), for the next `ui` to report.
    overview_closed: Vec<(String, InstanceId)>,
    /// The shade or the unlock prompt is over the screens on show ([`Workspace::set_covered`]).
    covered: bool,
    /// The lift: the screen on show following a finger up from the bottom edge.
    lift: Option<Lift>,
    /// The quick switch: two tasks' screens sliding with a finger along the indicator.
    switch: Option<Switch>,
    /// The order a run of quick switches walks, while the run lasts.
    quick: Option<QuickOrder>,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("view", &self.view)
            .field("tasks", &self.tasks.len())
            .field("home", &self.home)
            .finish_non_exhaustive()
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::new()
    }
}

impl Workspace {
    /// The Home view, with one Pane.
    #[must_use]
    pub fn new() -> Self {
        Self {
            view: WorkspaceView::Home,
            tasks: Vec::new(),
            panes: vec![Pane {
                id: PaneId(0),
                task: None,
            }],
            home: HomeTransition::Idle,
            stack: StackTransition::Idle,
            inset_bottom: 0.0,
            closing_task: None,
            graveyard: Vec::new(),
            next_id: 1,
            last_content: Rect::NOTHING,
            origin: None,
            layers_dirty: false,
            warmed_slots: [0; 2],
            clearing: Vec::new(),
            clear_fade: Animated::new(1.0),
            back_grab: 0.0,
            tokens: MotionTokens::default(),
            focus: 0,
            split: None,
            axis_rule: None,
            pane_rects: [Rect::NOTHING; 2],
            pane_laid: [Rect::NOTHING; 2],
            leaving_task: None,
            divider_handle: Rect::NOTHING,
            split_metrics: (8.0, 48.0),
            overview: None,
            overview_closed: Vec::new(),
            covered: false,
            lift: None,
            switch: None,
            quick: None,
        }
    }

    /// The view.
    #[must_use]
    pub fn view(&self) -> WorkspaceView {
        self.view
    }

    /// Whether it is home.
    #[must_use]
    pub fn is_home(&self) -> bool {
        self.view == WorkspaceView::Home
    }

    /// Every live task. The one leaving through a transition (a pop's `outgoing`, a root pop's task) is not here.
    #[must_use]
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    /// A fresh instance id.
    pub(crate) fn alloc_id(&mut self) -> InstanceId {
        let id = InstanceId(self.next_id);
        self.next_id += 1;
        id
    }

    /// The focused pane's task.
    fn pane_task_index(&self) -> Option<usize> {
        self.panes.get(self.focus).and_then(|p| p.task)
    }

    fn set_pane_task(&mut self, index: Option<usize>) {
        if let Some(pane) = self.panes.get_mut(self.focus) {
            pane.task = index;
        }
    }

    /// The task pane `pane` shows (the leaving one included, while its pane slides out).
    fn task_of_pane(&self, pane: usize) -> Option<&Task> {
        if self.leaving_pane() == Some(pane) {
            if let Some(task) = self.leaving_task.as_ref() {
                return Some(task);
            }
        }
        self.panes
            .get(pane)
            .and_then(|p| p.task)
            .and_then(|i| self.tasks.get(i))
    }

    /// [`Workspace::task_of_pane`], mutable.
    fn task_of_pane_mut(&mut self, pane: usize) -> Option<&mut Task> {
        if self.leaving_pane() == Some(pane) && self.leaving_task.is_some() {
            return self.leaving_task.as_mut();
        }
        let index = self.panes.get(pane).and_then(|p| p.task)?;
        self.tasks.get_mut(index)
    }

    /// The pane on its way out, while one is.
    fn leaving_pane(&self) -> Option<usize> {
        match self.split.as_ref().map(Split::phase) {
            Some(Phase::Leaving(pane)) => Some(pane),
            _ => None,
        }
    }

    /// Whether two panes are up — or one of them is on its way in or out.
    #[must_use]
    pub fn is_split(&self) -> bool {
        self.split.is_some()
    }

    /// The focused pane, as an index: 0 is the first pane (left, or top), 1 the second.
    #[must_use]
    pub fn focused_pane(&self) -> usize {
        self.focus
    }

    /// The task pane `pane` shows.
    #[must_use]
    pub fn pane_task(&self, pane: usize) -> Option<&Task> {
        self.task_of_pane(pane)
    }

    /// Where pane `pane` was last drawn, before the screen inset.
    #[must_use]
    pub fn pane_rect(&self, pane: usize) -> Option<Rect> {
        let rect = if self.split.is_some() {
            *self.pane_rects.get(pane)?
        } else if pane == 0 {
            self.last_content
        } else {
            return None;
        };
        rect.is_positive().then_some(rect)
    }

    /// Where the focused pane's screen was last laid out — the content with one pane.
    pub(crate) fn focused_pane_rect(&self) -> Rect {
        if self.split.is_some() {
            if let Some(rect) = self.pane_laid.get(self.focus).filter(|r| r.is_positive()) {
                return *rect;
            }
        }
        self.last_content
    }

    /// The divider's place along the axis, as a share of the content, while split.
    #[must_use]
    pub fn split_ratio(&self) -> Option<f32> {
        self.split.as_ref().map(Split::ratio)
    }

    /// Where the divider's handle was last drawn: the band, a touch target across.
    #[must_use]
    pub fn divider_rect(&self) -> Option<Rect> {
        self.divider_handle
            .is_positive()
            .then_some(self.divider_handle)
    }

    /// The axis the panes divide along, for this content.
    #[must_use]
    pub fn split_axis(&self) -> SplitAxis {
        self.axis_for(self.last_content)
    }

    fn axis_for(&self, content: Rect) -> SplitAxis {
        self.axis_rule
            .unwrap_or_else(|| SplitAxis::for_content(content))
    }

    /// `[workspace] split_axis` — `None` follows the content's shape.
    pub(crate) fn set_split_axis(&mut self, rule: Option<SplitAxis>) {
        self.axis_rule = rule;
    }

    /// The least length along the axis each pane keeps in `content` — from its top screen's
    /// `SplitSupport`. A screen that is never split (it can only be here by being pushed after the
    /// split began) gets the default.
    fn pane_mins(&self, content: Rect, axis: SplitAxis) -> [f32; 2] {
        let target = self.split_metrics.1;
        let min = |pane: usize| {
            let support = self
                .task_of_pane(pane)
                .and_then(Task::top)
                .map_or(SplitSupport::Yes, Instance::split_support);
            split::min_len(support, content, axis, target)
                .or_else(|| split::min_len(SplitSupport::Yes, content, axis, target))
                .unwrap_or(0.0)
        };
        [min(0), min(1)]
    }

    /// Move the focus to pane `pane` — a press on it. A transition in progress lands
    /// first; nothing else changes: both panes stay `Resumed`, only `PaneInfo::is_focused` and
    /// whose chrome policy rules move.
    pub(crate) fn focus_pane(&mut self, pane: usize) {
        if pane == self.focus || pane >= self.panes.len() {
            return;
        }
        if !matches!(self.split.as_ref().map(Split::phase), Some(Phase::Split)) {
            return;
        }
        self.settle();
        self.focus = pane;
    }

    /// Take a back gesture the finger is still dragging out of the way, while the other pane acts
    /// on its own: focusing that pane for a moment settles every transition, and the user's drag
    /// must not be one of them. `None` where no drag is under way.
    pub(crate) fn hold_back_gesture(&mut self) -> Option<HeldBack> {
        if !matches!(
            self.stack,
            StackTransition::DraggingBack {
                confirmed: None,
                ..
            }
        ) {
            return None;
        }
        Some(HeldBack {
            stack: std::mem::replace(&mut self.stack, StackTransition::Idle),
            pane: self.focus,
            top: self.active_task().and_then(Task::top).map(Instance::id),
        })
    }

    /// Put back what [`Workspace::hold_back_gesture`] took: the finger carries on where it was.
    /// Where the pane under it changed in the meantime the drag is over, and the screen it was
    /// dragging, still on top, is told `Resumed`.
    pub(crate) fn resume_back_gesture(&mut self, held: HeldBack) {
        let top = self.active_task().and_then(Task::top).map(Instance::id);
        if self.focus == held.pane
            && top == held.top
            && matches!(self.stack, StackTransition::Idle)
            && self.view == WorkspaceView::Tasks
            && self.overview.is_none()
        {
            self.stack = held.stack;
            return;
        }
        if let Some(instance) = held.top.and_then(|id| self.instance_mut(id)) {
            instance.queue(Lifecycle::Resumed);
        }
    }

    /// Focus the pane showing `instance` — a screen asking for something from its own pane.
    pub(crate) fn focus_pane_of(&mut self, instance: InstanceId) {
        if self.split.is_none() {
            return;
        }
        let pane = (0..self.panes.len()).find(|&pane| {
            self.task_of_pane(pane)
                .is_some_and(|task| task.instance(instance).is_some())
        });
        if let Some(pane) = pane {
            self.focus_pane(pane);
        }
    }

    /// The pane showing instance `instance` — `None` where it is in neither (or with one pane, in
    /// a task not on show).
    pub(crate) fn pane_holding(&self, instance: InstanceId) -> Option<usize> {
        (0..self.panes.len()).find(|&pane| {
            self.task_of_pane(pane)
                .is_some_and(|task| task.instance(instance).is_some())
        })
    }

    /// A screen in the pane the user is not in opened something on its own — a timer, an answer
    /// arriving — and the push went to its pane, which took the focus to do it. The push finishes
    /// at once (stack transitions are drawn in the focused pane) and the focus goes back to
    /// `pane`, where the user is: their keyboard, bars and back button stay theirs (the
    /// focus is the pane of the last input).
    pub(crate) fn return_focus(&mut self, pane: usize) {
        if self.split.is_none() || pane >= self.panes.len() || pane == self.focus {
            return;
        }
        self.settle();
        self.focus_pane(pane);
    }

    /// Focus the pane under `at` — where the press landed on a pane's own screen. A press on
    /// anything drawn over the panes (the shade, the on-screen keyboard, the unlock prompt, the
    /// overview's cards, a toast) or on the divider's handle leaves the focus where it is: typing
    /// on the keyboard over the other pane must not hand that pane the focus.
    pub(crate) fn focus_at(&mut self, ctx: &egui::Context, at: Pos2) {
        if self.split.is_none() || self.overview.is_some() || self.divider_handle.contains(at) {
            return;
        }
        // The topmost layer that takes input there, as last frame drew it.
        let Some(layer) = ctx.layer_id_at(at) else {
            return;
        };
        let on_a_pane = (0..self.panes.len()).any(|pane| {
            self.task_of_pane(pane)
                .and_then(Task::top)
                .is_some_and(|top| top.layer_id() == layer)
        });
        if !on_a_pane {
            return;
        }
        if let Some(pane) = self.pane_rects.iter().position(|r| r.contains(at)) {
            self.focus_pane(pane);
        }
    }

    /// Whether a split can hold with a screen of `incoming` support in the other pane:
    /// in the Tasks view, the focused screen allowing it (`ChromePolicy::allow_split` and its own
    /// `SplitSupport`), and a ratio in this content at which both keep their minimums.
    #[must_use]
    pub fn can_split(&self, incoming: SplitSupport) -> bool {
        self.entering_ratio(incoming).is_some()
    }

    /// The ratio a split with `incoming` in the other pane would start at — an even split, or as
    /// near as both minimums allow. `None` where no split can hold.
    fn entering_ratio(&self, incoming: SplitSupport) -> Option<f32> {
        if self.view != WorkspaceView::Tasks
            || matches!(
                self.split.as_ref().map(Split::phase),
                Some(Phase::Leaving(_))
            )
        {
            return None;
        }
        let top = self.focused()?;
        if !top.chrome().allow_split {
            return None;
        }
        let content = self.last_content;
        if !content.is_positive() {
            return None;
        }
        let axis = self.axis_for(content);
        let (divider, target) = self.split_metrics;
        let kept = split::min_len(top.split_support(), content, axis, target)?;
        let coming = split::min_len(incoming, content, axis, target)?;
        // The focused pane keeps its side; the other side is the incoming screen's.
        let mins = if self.split.is_some() && self.focus == 1 {
            [coming, kept]
        } else {
            [kept, coming]
        };
        let (lo, hi) = split::bounds(content, axis, divider, mins)?;
        let ratio = self.split.as_ref().map_or(0.5, Split::ratio);
        Some(ratio.clamp(lo, hi))
    }

    /// **Open `instance` in the other pane** (`cx.open_in_other_pane`): with one pane, the
    /// second pane of a new split — the pane already there keeps the first half and the new one
    /// slides in (A8); with two, a push onto the pane not focused. Focus moves to it. The caller
    /// asks [`Workspace::can_split`] first; where no split can hold this opens in the same pane.
    pub(crate) fn open_in_other_pane(&mut self, instance: Instance, tokens: &MotionTokens) {
        let Some(ratio) = self.entering_ratio(instance.split_support()) else {
            self.open(instance, None, tokens);
            return;
        };
        self.settle();
        if self.split.is_some() {
            self.focus = 1 - self.focus;
            self.open(instance, None, tokens);
            return;
        }
        self.tasks.push(Task::new(instance));
        let index = self.tasks.len() - 1;
        self.enter_split(index, ratio, tokens);
    }

    /// **Show task `task` in the other pane** — the Overview's "split" and a launch that found its
    /// screen in a task of its own. With two panes the other pane's task goes to the background
    /// (`Paused`, then `Stopped`) and this one comes forward (`Resumed`), at once. `false` where it
    /// is the focused task itself, or no split can hold.
    pub(crate) fn show_in_other_pane(&mut self, task: usize, tokens: &MotionTokens) -> bool {
        if task >= self.tasks.len() || self.pane_task_index() == Some(task) {
            return false;
        }
        let support = self
            .tasks
            .get(task)
            .and_then(Task::top)
            .map_or(SplitSupport::Yes, Instance::split_support);
        let Some(ratio) = self.entering_ratio(support) else {
            return false;
        };
        self.settle();
        if self.split.is_none() {
            self.enter_split(task, ratio, tokens);
            return true;
        }
        let other = 1 - self.focus;
        self.focus = other;
        if self.pane_task_index() == Some(task) {
            return true;
        }
        if let Some(old) = self.active_task_mut() {
            if let Some(top) = old.top_mut() {
                top.queue(Lifecycle::Paused);
            }
            for instance in old.iter_mut() {
                instance.queue(Lifecycle::Stopped);
            }
        }
        self.set_pane_task(Some(task));
        if let Some(shown) = self.tasks.get_mut(task) {
            shown.touch();
            if let Some(top) = shown.top_mut() {
                top.queue(Lifecycle::Resumed);
            }
        }
        self.layers_dirty = true;
        true
    }

    /// Whether the overview is up.
    #[must_use]
    pub fn is_overview_open(&self) -> bool {
        self.overview.is_some()
    }

    /// The session changed under the overview: its cards get split buttons where a split may
    /// come up now, and lose them where it may not.
    pub(crate) fn set_overview_beside(&mut self, beside: bool) {
        let over_tasks = self.view == WorkspaceView::Tasks;
        if let Some(overview) = self.overview.as_mut() {
            overview.set_beside(beside && over_tasks);
        }
    }

    /// Whether the overview is up as the split control's picker — a card goes beside the pane.
    #[must_use]
    pub fn is_overview_picking(&self) -> bool {
        self.overview.as_ref().is_some_and(Overview::is_picker)
    }

    /// Where the overview's cards were drawn last frame — for a tour or a test pressing them.
    #[doc(hidden)]
    #[must_use]
    pub fn overview_cards_drawn(&self) -> &[DrawnCard] {
        self.overview.as_ref().map_or(&[], Overview::drawn)
    }

    /// Where the overview's "Close all" was drawn last frame.
    #[doc(hidden)]
    #[must_use]
    pub fn overview_close_all_rect(&self) -> Option<Rect> {
        self.overview.as_ref().and_then(Overview::close_all_rect)
    }

    /// **Bring the overview up** (A10): over the task on show, whose screen shrinks into
    /// its card, or over the desktop. `mode` says what the cards are for — the recent screens
    /// (with split buttons where a split may come up) or the split control's picker. The screens
    /// on show are `Paused` while it covers them.
    pub(crate) fn open_overview(&mut self, mode: OverviewMode, tokens: &MotionTokens) {
        if self.overview.is_some() {
            return;
        }
        self.settle();
        let over_tasks = self.view == WorkspaceView::Tasks;
        let current = over_tasks
            .then(|| self.active_task().and_then(root_key))
            .flatten();
        if over_tasks {
            self.queue_focused(Lifecycle::Paused);
        }
        // Over the desktop there is no pane on show for a card to go beside.
        let mode = match mode {
            OverviewMode::Recents { beside } => OverviewMode::Recents {
                beside: beside && over_tasks,
            },
            OverviewMode::Picker => OverviewMode::Picker,
        };
        self.overview = Some(Overview::open(current, mode, !over_tasks, tokens));
        self.layers_dirty = true;
    }

    /// The overview goes back to what was on show (back, a tap past the cards).
    pub(crate) fn close_overview(&mut self, tokens: &MotionTokens) {
        let done = self
            .overview
            .as_mut()
            .is_some_and(|o| !o.is_returning() && o.go_back(tokens));
        if done {
            self.finish_overview_return();
        }
    }

    /// The overview is gone, back to what it covered: those screens are `Resumed`. A card thrown
    /// away and still flying ends its task all the same.
    fn finish_overview_return(&mut self) {
        let thrown = self.overview.as_mut().and_then(Overview::take_thrown);
        self.overview = None;
        self.layers_dirty = true;
        if let Some(key) = thrown {
            let closed = self.close_card(key);
            self.overview_closed.extend(closed);
        }
        if self.view == WorkspaceView::Tasks {
            self.queue_focused(Lifecycle::Resumed);
        }
    }

    /// The overview gone at once — something else takes over the motion.
    fn drop_overview(&mut self) {
        // A card thrown away and still flying ends its task all the same.
        let thrown = self.overview.as_mut().and_then(Overview::take_thrown);
        self.overview = None;
        self.layers_dirty = true;
        if let Some(key) = thrown {
            let closed = self.close_card(key);
            self.overview_closed.extend(closed);
        }
    }

    /// The task whose root instance is `key`.
    fn task_by_key(&self, key: InstanceId) -> Option<usize> {
        self.tasks
            .iter()
            .position(|task| root_key(task) == Some(key))
    }

    /// The cards, in the order the overview shows them: the task on show first, then the rest by
    /// when they were last used. The picker shows only the tasks that can go beside the focused
    /// screen.
    fn overview_cards(
        &self,
        access: &crate::access::Access,
        registry: &crate::screen::Registry,
    ) -> Vec<Card> {
        let Some(overview) = self.overview.as_ref() else {
            return Vec::new();
        };
        let shown: Vec<usize> = if self.view == WorkspaceView::Tasks {
            self.panes.iter().filter_map(|p| p.task).collect()
        } else {
            Vec::new()
        };
        let table = access.table();
        let bottom = table.get(table.bottom()).map(|d| d.label.clone());
        let mut order: Vec<usize> = (0..self.tasks.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.tasks.get(i).map(Task::last_active)));
        if let Some(current) = overview.current().and_then(|key| self.task_by_key(key)) {
            order.retain(|&i| i != current);
            order.insert(0, current);
        }
        if overview.is_picker() {
            // The picker offers only what can really go beside the focused screen.
            let fits = self.beside_candidates();
            order.retain(|i| fits.contains(i));
        }
        let can_beside = overview.has_beside() && self.view == WorkspaceView::Tasks;
        order
            .into_iter()
            .filter_map(|index| {
                let task = self.tasks.get(index)?;
                let root = task.iter().next()?;
                let top = task.top()?;
                let gate = registry
                    .screen(top.decl_id())
                    .map(crate::screen::ScreenDecl::gate_name);
                let badge = gate
                    .and_then(|gate| access.hint(&gate))
                    .filter(|label| Some(label) != bottom.as_ref());
                let beside =
                    can_beside && !shown.contains(&index) && self.can_split(top.split_support());
                Some(Card {
                    key: root.id(),
                    title: root.title().to_owned(),
                    icon: root.icon().cloned(),
                    last_active: task.last_active(),
                    badge,
                    beside,
                })
            })
            .collect()
    }

    /// End the task behind card `key`: every instance closed, the carousel closing the gap.
    /// Returns what closed, for `ScreenClosed`.
    fn close_card(&mut self, key: InstanceId) -> Vec<(String, InstanceId)> {
        let Some(index) = self.task_by_key(key) else {
            return Vec::new();
        };
        let ids: Vec<InstanceId> = self
            .tasks
            .get(index)
            .map(|task| task.iter().map(Instance::id).collect())
            .unwrap_or_default();
        let tokens = self.tokens;
        let closed = self.close_where(|instance| ids.contains(&instance.id()), &tokens);
        let left = self.tasks.len();
        if let Some(overview) = self.overview.as_mut() {
            overview.forget_current(key);
            overview.closed(index, left);
        }
        closed
    }

    /// Act on what the overview was asked. Returns what closed.
    fn apply_overview(&mut self, picked: Picked) -> Vec<(String, InstanceId)> {
        let tokens = self.tokens;
        match picked {
            Picked::Dismiss => {
                self.close_overview(&tokens);
                Vec::new()
            }
            Picked::Close(key) => self.close_card(key),
            Picked::CloseAll => {
                let tokens = self.tokens;
                let closed = self.close_where(|_| true, &tokens);
                self.drop_overview();
                closed
            }
            Picked::Beside(key) => {
                // The cards go only once the task is really beside; where it no longer fits (the
                // content changed under the cards) they stay up rather than vanish on a tap.
                if let Some(index) = self.task_by_key(key) {
                    if self.show_in_other_pane(index, &tokens) {
                        self.drop_overview();
                        // Everything the cards covered is on show again.
                        self.queue_focused(Lifecycle::Resumed);
                    } else {
                        log::info!("overview: that screen does not fit beside this one here");
                    }
                }
                Vec::new()
            }
            Picked::Resume(key) => {
                self.resume_card(key, &tokens);
                Vec::new()
            }
        }
    }

    /// A card tapped: a task on show goes back to its pane (the overview's way back); any other
    /// comes forward — over a task, through the A2 fallback as a screen opened with no icon does
    /// (A10); in a split, into the focused pane.
    fn resume_card(&mut self, key: InstanceId, tokens: &MotionTokens) {
        let Some(index) = self.task_by_key(key) else {
            return;
        };
        let on_show = (self.view == WorkspaceView::Tasks)
            .then(|| self.panes.iter().position(|p| p.task == Some(index)))
            .flatten();
        if let Some(pane) = on_show {
            if pane != self.focus {
                self.focus = pane.min(self.panes.len().saturating_sub(1));
            }
            self.close_overview(tokens);
            return;
        }
        self.drop_overview();
        if self.view == WorkspaceView::Tasks && self.split.is_some() {
            // The other pane's screen was covered too; it is on show again.
            let other = 1 - self.focus.min(1);
            if let Some(top) = self.task_of_pane_mut(other).and_then(Task::top_mut) {
                top.queue(Lifecycle::Resumed);
            }
            self.resume_task(index, None, tokens);
            return;
        }
        if self.view == WorkspaceView::Tasks {
            // Home first, at once — then the A2 fallback opens the card's task.
            self.view = WorkspaceView::Home;
            self.finish_home(true);
        }
        self.resume_task(index, None, tokens);
    }

    /// The task used last of those that can go beside the focused screen now — what the split
    /// control puts in the other pane when the shell's overview is off.
    #[must_use]
    pub fn most_recent_other_task(&self) -> Option<usize> {
        self.beside_candidates().first().copied()
    }

    /// The tasks that can go beside the focused screen now, the one used last first: not on show
    /// in either pane, with a top screen whose `SplitSupport` fits next to the focused one.
    fn beside_candidates(&self) -> Vec<usize> {
        let shown: Vec<usize> = if self.view == WorkspaceView::Tasks {
            self.panes.iter().filter_map(|p| p.task).collect()
        } else {
            Vec::new()
        };
        let mut fits: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(index, _)| !shown.contains(index))
            .filter(|(_, task)| {
                task.top()
                    .is_some_and(|top| self.can_split(top.split_support()))
            })
            .map(|(index, _)| index)
            .collect();
        fits.sort_by_key(|&i| std::cmp::Reverse(self.tasks.get(i).map(Task::last_active)));
        fits
    }

    /// Why a split cannot come up beside the focused screen now — `None` where one can. The split
    /// control says this instead of doing nothing.
    pub(crate) fn split_blocker(&self) -> Option<SplitBlocker> {
        if self.view != WorkspaceView::Tasks || self.focused().is_none() {
            return Some(SplitBlocker::NothingOnShow);
        }
        // The smallest partner a screen can name: if even that does not fit, nothing will.
        let smallest = SplitSupport::MinSize(egui::Vec2::ZERO);
        if !self.can_split(smallest) {
            return Some(SplitBlocker::Refused);
        }
        if self.beside_candidates().is_empty() {
            return Some(SplitBlocker::NothingFits);
        }
        None
    }

    /// Something is being opened from outside the cards — a launch, a notification, a command from
    /// another thread: the overview gets out of the way at once, so what opens is on show rather
    /// than hidden under it, and the screens it covered are on show again.
    pub(crate) fn clear_overview(&mut self) {
        if self.overview.is_none() {
            return;
        }
        self.drop_overview();
        if self.view == WorkspaceView::Tasks {
            self.queue_focused(Lifecycle::Resumed);
        }
    }

    /// One pane becomes two: task `task` in the second pane, sliding in (A8). Its top is
    /// `Resumed` once it is in.
    fn enter_split(&mut self, task: usize, ratio: f32, tokens: &MotionTokens) {
        if let Some(shown) = self.tasks.get_mut(task) {
            shown.touch();
        }
        self.panes.truncate(1);
        self.panes.push(Pane {
            id: PaneId(1),
            task: Some(task),
        });
        self.focus = 1;
        self.split = Some(Split::enter(1, ratio, tokens));
        self.layers_dirty = true;
        if tokens.reduce {
            self.finish_entering();
        }
    }

    /// The split is in: the pane that came in is `Resumed`.
    fn finish_entering(&mut self) {
        if let Some(top) = self.task_of_pane_mut(1).and_then(Task::top_mut) {
            top.queue(Lifecycle::Resumed);
        }
    }

    /// **Back to one pane**, keeping pane `keep` (by the split control, or the divider pushed to
    /// an end). The other pane slides out (A8); its task stays alive in the background — `Paused`
    /// now, `Stopped` once it is out. `false` with one pane, or a pane already leaving.
    pub(crate) fn unsplit(&mut self, keep: usize, tokens: &MotionTokens) -> bool {
        if keep > 1
            || !matches!(
                self.split.as_ref().map(Split::phase),
                Some(Phase::Split | Phase::Entering(_))
            )
        {
            return false;
        }
        self.settle();
        let leaving = 1 - keep;
        if let Some(top) = self.task_of_pane_mut(leaving).and_then(Task::top_mut) {
            top.queue(Lifecycle::Paused);
        }
        self.start_leaving(leaving, tokens);
        true
    }

    /// Start pane `leaving` sliding out; the focus is the other pane's from now.
    fn start_leaving(&mut self, leaving: usize, tokens: &MotionTokens) {
        self.focus = 1 - leaving;
        let done = self
            .split
            .as_mut()
            .is_some_and(|split| split.leave(leaving, tokens));
        self.layers_dirty = true;
        if done {
            self.finish_leaving(leaving);
        }
    }

    /// Pane `leaving` is out: one pane again. A task that ended with it (a root pop) is buried;
    /// one that stays alive is `Stopped`. The pane left fills the content, and says so.
    fn finish_leaving(&mut self, leaving: usize) {
        let pane = (leaving < self.panes.len()).then(|| self.panes.remove(leaving));
        if let Some(task) = self.leaving_task.take() {
            self.bury_task(task);
        } else if let Some(task) = pane
            .and_then(|p| p.task)
            .and_then(|i| self.tasks.get_mut(i))
        {
            for instance in task.iter_mut() {
                if !instance.is_stopped() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
        }
        self.split = None;
        self.focus = 0;
        if let Some(pane) = self.panes.first_mut() {
            pane.id = PaneId(0);
        }
        self.layers_dirty = true;
        self.queue_pane_resize(0, self.last_content);
    }

    /// One pane again, at once — a pane's task is gone (a session downgrade, an eviction): the
    /// other pane fills the content with no animation, as `close_where` closes.
    fn collapse_to_live_pane(&mut self) {
        if self.split.is_none() {
            return;
        }
        let live = self.panes.iter().position(|p| p.task.is_some());
        let both = self.panes.iter().all(|p| p.task.is_some());
        if both {
            return;
        }
        let keep = live.unwrap_or(0);
        let kept = self.panes.get(keep).copied();
        self.panes.clear();
        self.panes.push(Pane {
            id: PaneId(0),
            task: kept.and_then(|p| p.task),
        });
        if let Some(task) = self.leaving_task.take() {
            self.bury_task(task);
        }
        self.split = None;
        self.focus = 0;
        self.layers_dirty = true;
        self.queue_pane_resize(0, self.last_content);
    }

    /// Back to one pane at once and with no animation, keeping the focused pane — the Home view
    /// coming back, or a screen that is never split arriving in a pane.
    fn collapse_to_focused(&mut self) {
        if self.split.is_none() {
            return;
        }
        let kept = self.panes.get(self.focus).copied();
        let other = 1 - self.focus.min(1);
        if let Some(task) = self.leaving_task.take() {
            self.bury_task(task);
        } else if let Some(task) = self
            .panes
            .get(other)
            .and_then(|p| p.task)
            .and_then(|i| self.tasks.get_mut(i))
        {
            if let Some(top) = task.top_mut() {
                top.queue(Lifecycle::Paused);
            }
            for instance in task.iter_mut() {
                if !instance.is_stopped() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
        }
        self.panes.clear();
        self.panes.push(Pane {
            id: PaneId(0),
            task: kept.and_then(|p| p.task),
        });
        self.split = None;
        self.focus = 0;
        self.layers_dirty = true;
        self.queue_pane_resize(0, self.last_content);
    }

    /// Queue `Resized` on every instance of pane `pane`'s task, at `rect`'s size.
    fn queue_pane_resize(&mut self, pane: usize, rect: Rect) {
        if !rect.is_positive() {
            return;
        }
        let size = rect.size();
        if let Some(task) = self.task_of_pane_mut(pane) {
            for instance in task.iter_mut() {
                instance.tell_size(size);
            }
        }
    }

    /// The split came to rest after a change: each pane's instances hear their new size, once
    /// (the end of a tween, the divider's release).
    fn queue_split_resize(&mut self) {
        let content = self.last_content;
        let Some(split) = self.split.as_ref() else {
            return;
        };
        if !content.is_positive() {
            return;
        }
        let axis = self.axis_for(content);
        let [first, second, _] = split::rest(content, axis, split.ratio(), self.split_metrics.0);
        self.queue_pane_resize(0, first);
        self.queue_pane_resize(1, second);
    }

    /// The content resized (a rotation, a window, the chrome hiding): every pane's instances hear
    /// their new size — the whole content with one pane, each pane's share with two.
    pub(crate) fn content_resized(&mut self, content: Rect) {
        self.last_content = content;
        if self.split.is_none() {
            for instance in self.tasks.iter_mut().flat_map(Task::iter_mut) {
                instance.tell_size(content.size());
            }
            return;
        }
        // The background tasks take the whole content; the two shown take their shares.
        let shown: Vec<usize> = self.panes.iter().filter_map(|p| p.task).collect();
        for (index, task) in self.tasks.iter_mut().enumerate() {
            if shown.contains(&index) {
                continue;
            }
            for instance in task.iter_mut() {
                instance.tell_size(content.size());
            }
        }
        self.queue_split_resize();
    }

    /// Every instance at rest in a pane of the size it was last told — or told now. The tweens
    /// tell their panes at their end, but a task also changes size without one: put into
    /// the other pane, resumed into a split, sent to the background when its pane leaves. Asked
    /// once a frame with nothing moving; an instance already told its size hears nothing.
    fn sync_sizes(&mut self) {
        let content = self.last_content;
        if !content.is_positive() || self.is_animating() {
            return;
        }
        if self.split.as_ref().is_some_and(Split::is_dragging) {
            return;
        }
        let mut shown: [Option<(usize, Rect)>; 2] = [None; 2];
        if self.view == WorkspaceView::Tasks {
            match self.split.as_ref() {
                Some(split) if split.phase() == Phase::Split => {
                    let axis = self.axis_for(content);
                    let [first, second, _] =
                        split::rest(content, axis, split.ratio(), self.split_metrics.0);
                    for (slot, (pane, rect)) in shown.iter_mut().zip([(0, first), (1, second)]) {
                        *slot = self.panes.get(pane).and_then(|p| p.task).map(|t| (t, rect));
                    }
                }
                Some(_) => return,
                None => {
                    shown[0] = self.pane_task_index().map(|t| (t, content));
                }
            }
        }
        for (index, task) in self.tasks.iter_mut().enumerate() {
            let size = shown
                .iter()
                .flatten()
                .find(|(t, _)| *t == index)
                .map_or(content.size(), |(_, rect)| rect.size());
            for instance in task.iter_mut() {
                instance.keep_size(size);
            }
        }
    }

    /// Pull the Pane indices in after task `index` has gone.
    fn shift_panes_after_remove(&mut self, index: usize) {
        for pane in &mut self.panes {
            pane.task = match pane.task {
                Some(i) if i == index => None,
                Some(i) if i > index => Some(i - 1),
                other => other,
            };
        }
    }

    /// The task attached to the Pane.
    #[doc(hidden)]
    #[must_use]
    pub fn active_task(&self) -> Option<&Task> {
        self.pane_task_index().and_then(|i| self.tasks.get(i))
    }

    /// The task attached to the Pane (mutable).
    pub(crate) fn active_task_mut(&mut self) -> Option<&mut Task> {
        let i = self.pane_task_index()?;
        self.tasks.get_mut(i)
    }

    /// The focused screen (the top of the Tasks view).
    #[must_use]
    pub fn focused(&self) -> Option<&Instance> {
        if self.view == WorkspaceView::Tasks {
            self.active_task().and_then(Task::top)
        } else {
            None
        }
    }

    /// Find a live instance by declaration id (across every task).
    #[must_use]
    pub fn find(&self, decl_id: &str) -> Option<&Instance> {
        self.tasks.iter().find_map(|t| t.find(decl_id))
    }

    /// The index of the task holding an instance of `decl_id` (searching **whole** stacks, so a
    /// `Single` screen in another task is found). Where several tasks have one, the task created
    /// first.
    #[must_use]
    pub(crate) fn find_task(&self, decl_id: &str) -> Option<usize> {
        self.tasks.iter().position(|t| t.find(decl_id).is_some())
    }

    /// Queue an event on every live instance (`AccessChanged`, `Resized`). An instance leaving
    /// through a transition (about to be `Destroyed`) does not receive it.
    pub(crate) fn queue_all(&mut self, event: Lifecycle) {
        for instance in self.tasks.iter_mut().flat_map(Task::iter_mut) {
            instance.queue(event);
        }
    }

    /// Find by instance id (mutable).
    pub(crate) fn instance_mut(&mut self, id: InstanceId) -> Option<&mut Instance> {
        self.tasks.iter_mut().find_map(|t| t.instance_mut(id))
    }

    /// The policy deciding the chrome right now. Home takes the default policy.
    ///
    /// **Split, it is the focused pane's** — except that a bar is hidden only when
    /// both panes' screens hide it: one fullscreen pane must not take the bars from the other.
    #[must_use]
    pub fn chrome_policy(&self) -> ChromePolicy {
        // The overview is the shell's own view: the bars stay.
        if self.overview.is_some() {
            return ChromePolicy::default();
        }
        let mut policy = self
            .focused()
            .map_or_else(ChromePolicy::default, Instance::chrome);
        if self.split.is_some() && self.view == WorkspaceView::Tasks {
            let other = self
                .task_of_pane(1 - self.focus.min(1))
                .and_then(Task::top)
                .map(Instance::chrome);
            if let Some(other) = other {
                if policy.status_bar == BarMode::Hide {
                    policy.status_bar = other.status_bar;
                }
                if policy.nav_bar == BarMode::Hide {
                    policy.nav_bar = other.nav_bar;
                }
                // A screen on show that keeps the display awake keeps it awake whichever pane
                // has the focus — a video in the other pane must not go dark under it.
                policy.keep_awake |= other.keep_awake;
            }
        }
        policy
    }

    /// Whether a transition is running (the repaint policy). The clear-top cross-fade is a transition too.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.home.is_active()
            || self.stack.is_active()
            || self.is_clearing()
            || self.split.as_ref().is_some_and(Split::is_moving)
            || self.overview.as_ref().is_some_and(Overview::is_moving)
            || self.lift.as_ref().is_some_and(Lift::is_moving)
            || self.switch.as_ref().is_some_and(Switch::is_moving)
    }

    /// Whether the screens stop taking input: a transition that moves them wholesale — A2, A3,
    /// the clear-top fade, a split coming or going, the overview coming or going. The divider
    /// settling or evening out does not: the panes only reflow, and a tap right after letting go
    /// of it has to land.
    fn blocks_input(&self) -> bool {
        self.home.is_active()
            || self.stack.is_active()
            || self.is_clearing()
            || self.split.as_ref().is_some_and(Split::is_tweening)
            || self.overview.as_ref().is_some_and(Overview::is_tweening)
            || self.lift.is_some()
            || self.switch.is_some()
    }

    /// Whether the clear-top cross-fade is running.
    #[doc(hidden)]
    #[must_use]
    pub fn is_clearing(&self) -> bool {
        !self.clearing.is_empty()
    }

    /// The home transition's state.
    #[doc(hidden)]
    #[must_use]
    pub fn home_transition(&self) -> &HomeTransition {
        &self.home
    }

    /// The stack transition's state.
    #[doc(hidden)]
    #[must_use]
    pub fn stack_transition(&self) -> &StackTransition {
        &self.stack
    }

    /// Start the back gesture (A3, M2): only with a stack of 2 or more and no transition running.
    /// On starting, `Paused` on the one leaving (the top). `true` if it started.
    ///
    /// **Mid-cancel it is taken hold of where it stands** (A3, "taking hold again during a
    /// cancelled gesture continues from the current p"). The `dx / W` of the
    /// [`Workspace::drag_gesture_back`] that follows is added to the `p` it was caught at — the
    /// finger starts again from the screen's left edge but the screen carries on from where it was.
    ///
    /// **It does not start at the root (a stack of 1).** Popping to the root ends the task and goes
    /// home, which is an A2 close (the icon zoom), whose render mapping is nothing like
    /// A3's horizontal 1:1 following, so the finger cannot be carried straight over. On a `false`
    /// the shell releases this press with [`GestureEngine::cancel`] so the widget below receives it —
    /// the back button and `back()` still go home.
    ///
    /// [`GestureEngine::cancel`]: crate::gesture::GestureEngine::cancel
    pub(crate) fn begin_gesture_back(&mut self) -> bool {
        // Under the cards nothing is on show to be swiped away.
        if self.overview.is_some() {
            return false;
        }
        if let Some(p) = self.stack.regrab_back() {
            self.back_grab = p;
            if let Some(top) = self.active_task_mut().and_then(Task::top_mut) {
                top.queue(Lifecycle::Paused);
            }
            return true;
        }
        if self.view != WorkspaceView::Tasks || self.is_animating() {
            return false;
        }
        let Some(task) = self.active_task_mut() else {
            return false;
        };
        if task.len() < 2 {
            return false;
        }
        if let Some(top) = task.top_mut() {
            top.queue(Lifecycle::Paused);
        }
        self.back_grab = 0.0;
        self.stack.begin_back();
        true
    }

    /// Gesture progress: `p = clamp(dx / W, 0, 1)`, `v_p = v_x / W`. On a gesture taken hold of
    /// again, the `p` it was caught at is added.
    pub(crate) fn drag_gesture_back(&mut self, p: f32, v_p: f32) {
        self.stack
            .drag_back((self.back_grab + p).clamp(0.0, 1.0), v_p);
    }

    /// The `p` the back gesture under way was taken hold of again at (A3 re-entry; 0 means a fresh
    /// gesture, or none under way).
    #[doc(hidden)]
    #[must_use]
    pub fn gesture_back_grab(&self) -> f32 {
        self.back_grab
    }

    /// The release: confirmed at `p ≥ 0.33`, or at `v_x ≥ fling` (the caller decides and hands it
    /// over as `confirm`). Confirmed, the top comes off the stack as the outgoing instance;
    /// cancelled, it is restored with `Resumed`.
    ///
    /// The (declaration id, instance id) of what came off the stack. `None` for a cancel, and for
    /// a release whose gesture something else already ended (a `back` from elsewhere mid-drag):
    /// nothing was closed by it.
    pub fn release_gesture_back(
        &mut self,
        confirm: bool,
        tokens: &MotionTokens,
    ) -> Option<(String, InstanceId)> {
        if !matches!(
            self.stack,
            StackTransition::DraggingBack {
                confirmed: None,
                ..
            }
        ) {
            return None;
        }
        // Let go, the gesture is over: the next one starts fresh unless it catches this one.
        self.back_grab = 0.0;
        let outgoing = if confirm {
            self.active_task_mut().and_then(Task::pop)
        } else {
            if let Some(top) = self.active_task_mut().and_then(Task::top_mut) {
                top.queue(Lifecycle::Resumed);
            }
            None
        };
        let closed = outgoing.as_ref().map(|o| (o.decl_id().to_owned(), o.id()));
        // The outgoing slot takes len so it does not collide with the new top (len−1).
        let base = Self::slot_base(self.focus);
        let outgoing = outgoing.map(|mut o| {
            let len = self.active_task().map_or(0, Task::len);
            o.set_layer_slot(base.saturating_add(u32::try_from(len).unwrap_or(u32::MAX)));
            o
        });
        // The width W is the focused pane's, as last drawn (= the content's with one pane — the same
        // value as the shell's `dx / W`). Where not one frame has been drawn yet (a headless unit test),
        // 1 — the old absolute decision as it was.
        let width = self.focused_pane_rect().width();
        let width = if width.is_finite() && width > 1.0 {
            width
        } else {
            1.0
        };
        self.stack
            .release_back(confirm, outgoing, tokens.spring, tokens.reduce, width);
        if tokens.reduce {
            self.settle_stack();
        }
        closed
    }

    /// The shade or the unlock prompt covers the screens on show (the shell says so as it
    /// changes). A screen that comes to the top under them is `Paused`, not `Resumed`.
    pub(crate) fn set_covered(&mut self, covered: bool) {
        self.covered = covered;
    }

    /// What a screen coming to the top of a task on show hears: `Resumed`, or `Paused` while the
    /// shade, the prompt or the overview's cards are over it — they resume it when they go.
    fn on_show(&self) -> Lifecycle {
        if self.covered || self.overview.is_some() {
            Lifecycle::Paused
        } else {
            Lifecycle::Resumed
        }
    }

    /// Queue a lifecycle notification on the screens on show — `Paused` when the overlay leaves
    /// `Closed`, `Resumed` when it comes back (M2). Split, both panes' tops: the shade and
    /// the prompt cover them both.
    pub(crate) fn queue_focused(&mut self, event: Lifecycle) {
        if self.split.is_some() {
            for pane in 0..self.panes.len() {
                if let Some(top) = self.task_of_pane_mut(pane).and_then(Task::top_mut) {
                    top.queue(event);
                }
            }
            return;
        }
        if let Some(top) = self.active_task_mut().and_then(Task::top_mut) {
            top.queue(event);
        }
    }

    /// The first layer slot pane `pane` borrows.
    fn slot_base(pane: usize) -> u32 {
        u32::try_from(pane).unwrap_or(0).saturating_mul(PANE_SLOTS)
    }

    /// Replace the motion-token cache. [`Workspace::ui`] calls it every frame with
    /// `parts.theme.motion` — the entry points that take no tokens as an argument, such as
    /// `Workspace::clear_top_to`, read this value.
    pub(crate) fn set_motion(&mut self, tokens: &MotionTokens) {
        self.tokens = *tokens;
    }

    /// The cached motion tokens.
    #[must_use]
    pub fn motion(&self) -> &MotionTokens {
        &self.tokens
    }

    /// The bottom height the OSK covers — handed to the screen every frame as `PaneInfo.inset_bottom` (A5).
    pub(crate) fn set_inset_bottom(&mut self, inset: f32) {
        self.inset_bottom = inset.max(0.0);
    }

    /// Finish every running transition at once — called by the shell when it starts something of
    /// **higher priority** (the shade). The same thing a new launch / back / close
    /// does before starting.
    pub(crate) fn settle_transitions(&mut self) {
        self.settle();
    }

    /// Finish every running transition at once ("there is no queue"). The interruption rule:
    /// a new launch / back / close mid-transition settles here and then starts. The exception
    /// (A2's reverse continuation) is handled directly by [`Workspace::go_home`] and
    /// [`Workspace::resume_task`].
    fn settle(&mut self) {
        self.settle_home();
        self.settle_stack();
        self.settle_split();
        self.drop_gestures();
    }

    /// Land a split on its way in or out, and a divider still springing (the same "no queue" rule).
    fn settle_split(&mut self) {
        let Some(split) = self.split.as_mut() else {
            return;
        };
        if split.is_dragging() || !split.is_moving() {
            return;
        }
        let phase = split.phase();
        // Run it out: the longest step a tick takes, until nothing moves (a spring may take a
        // second or two of steps; the guard is only against one that never settles).
        let mut left = None;
        let mut guard = 0;
        while split.is_moving() && guard < 4096 {
            left = left.or(split.tick(crate::motion::MAX_DT));
            guard += 1;
        }
        match (phase, left) {
            (_, Some(pane)) | (Phase::Leaving(pane), None) => self.finish_leaving(pane),
            (Phase::Entering(_), None) => self.finish_entering(),
            (Phase::Split, None) => {}
        }
    }

    fn settle_home(&mut self) {
        if self.home.is_active() {
            let closing = matches!(self.home, HomeTransition::Closing { .. });
            self.home.finish();
            self.finish_home(closing);
        }
    }

    fn settle_stack(&mut self) {
        let clearing = self.is_clearing();
        if clearing {
            self.finish_clearing();
        }
        if self.stack.is_active() {
            if let Some(out) = self.stack.finish() {
                self.bury(out);
            }
        } else if !clearing {
            return;
        }
        self.finish_stack();
    }

    /// Send the instances being cleared by the clear-top cross-fade to the graveyard (`Destroyed`).
    fn finish_clearing(&mut self) {
        self.clear_fade.snap(1.0);
        for instance in std::mem::take(&mut self.clearing) {
            self.bury(instance);
        }
    }

    /// Start an A2 open (immediately under reduce). It records the `origin`.
    fn start_home_open(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        self.origin = icon_rect;
        if tokens.reduce {
            self.finish_home(false);
        } else {
            self.home.open(icon_rect, tokens.home_open);
        }
    }

    /// Start an A2 close (immediately under reduce). Mid-`Opening` it reverses from the same `t`.
    fn start_home_close(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        if tokens.reduce {
            self.finish_home(true);
        } else {
            self.home.close(icon_rect, tokens.home_close);
        }
    }

    /// Open a new instance. From home, the root of a new task plus an A2; in Tasks, a push plus an
    /// A3. A transition in progress is finished first (there is no queue).
    pub fn open(&mut self, instance: Instance, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        self.settle();
        // A screen that is never split takes the whole content: the split ends first.
        if self.split.is_some() && instance.split_support() == SplitSupport::No {
            self.collapse_to_focused();
        }
        if self.view == WorkspaceView::Home || self.active_task().is_none() {
            self.tasks.push(Task::new(instance));
            let index = self.tasks.len() - 1;
            self.set_pane_task(Some(index));
            self.view = WorkspaceView::Tasks;
            self.start_home_open(icon_rect, tokens);
        } else {
            if let Some(task) = self.active_task_mut() {
                // At the start the side leaving (losing focus) gets `Paused` only — `Stopped` comes at the end.
                if let Some(top) = task.top_mut() {
                    top.queue(Lifecycle::Paused);
                }
                task.push(instance);
            }
            if tokens.reduce {
                self.finish_stack();
            } else {
                self.stack.push(tokens.push);
            }
        }
    }

    /// Bring a task whose root is `decl_id` to the front, if there is one (an icon re-tapped from
    /// home). To search whole stacks, [`Workspace::find_task`] plus [`Workspace::resume_task`].
    #[cfg(test)]
    pub(crate) fn resume_task_with_root(
        &mut self,
        decl_id: &str,
        icon_rect: Option<Rect>,
        tokens: &MotionTokens,
    ) -> bool {
        match self.tasks.iter().position(|t| t.root_id() == Some(decl_id)) {
            Some(index) => self.resume_task(index, icon_rect, tokens),
            None => false,
        }
    }

    /// Bring task `index` to the front (a `Single` screen already in another task brings that task
    /// forward).
    ///
    /// - From home, an A2 open. Where it is **the very task being A2-closed**, it reverses from the
    ///   same `t` and opens again (the A2 interruption rule — after the `Paused` already sent, a
    ///   `Resumed` at the end).
    /// - Switching to another task in the Tasks view gives the previous task a `Paused` on top then
    ///   `Stopped` throughout, and the new task's top a `Resumed` — **immediately** (the
    ///   overview's own motion is A10's).
    /// - The stack is unchanged (clear-top happens only when the screen is in the current task).
    ///   Already at the front, it does nothing and returns `true`; out of range, `false`.
    pub(crate) fn resume_task(
        &mut self,
        index: usize,
        icon_rect: Option<Rect>,
        tokens: &MotionTokens,
    ) -> bool {
        if index >= self.tasks.len() {
            return false;
        }
        self.settle_stack();
        let from_home = self.view == WorkspaceView::Home;
        let previous = self.pane_task_index();
        if !from_home && previous == Some(index) {
            return true;
        }
        // Shown in the other pane already: it only takes the focus.
        if !from_home && self.split.is_some() {
            let other = 1 - self.focus.min(1);
            if self.panes.get(other).and_then(|p| p.task) == Some(index) {
                self.focus_pane(other);
                return true;
            }
        }
        let closing_same = from_home
            && matches!(self.home, HomeTransition::Closing { .. })
            && previous == Some(index);
        if !closing_same {
            self.settle_home();
        }
        if let Some(prev) = previous
            .filter(|_| !from_home)
            .and_then(|p| self.tasks.get_mut(p))
        {
            if let Some(top) = prev.top_mut() {
                top.queue(Lifecycle::Paused);
            }
            for instance in prev.iter_mut() {
                instance.queue(Lifecycle::Stopped);
            }
        }
        self.set_pane_task(Some(index));
        if let Some(task) = self.tasks.get_mut(index) {
            task.touch();
        }
        self.view = WorkspaceView::Tasks;
        if from_home {
            // On `closing_same`, `HomeTransition::open` carries on from `1 − t`.
            self.start_home_open(icon_rect, tokens);
        } else {
            self.origin = icon_rect.or(self.origin);
            self.finish_home(false);
        }
        true
    }

    /// If `decl_id` is inside the current task, pop everything above it and resume (`Single`'s
    /// clear-top). A transition in progress is finished first.
    ///
    /// The screens cleared away disappear in a **cross-fade** over `tokens.clear_top` (160 ms
    /// `CubicOut`). Whichever was on top gets a `Paused` at the start, all of them
    /// a `Destroyed` at the end, and the new top a `Resumed` at the end ("notifications
    /// during a transition come at the end of it"). Immediately under `reduce`.
    ///
    /// The tokens are last frame's, cached by [`Workspace::ui`] — the shell hands this entry point
    /// none.
    pub(crate) fn clear_top_to(&mut self, decl_id: &str) -> bool {
        if self
            .active_task()
            .is_none_or(|task| task.find(decl_id).is_none())
        {
            return false;
        }
        self.settle();
        let Some(task) = self.active_task_mut() else {
            return false;
        };
        if task.find(decl_id).is_none() {
            return false;
        }
        let popped = task.clear_top(decl_id);
        if popped.is_empty() {
            // Already at the top — nothing to clear away, so no transition either.
            if let Some(top) = task.top_mut() {
                top.queue(Lifecycle::Resumed);
            }
            return true;
        }
        for (i, mut instance) in popped.into_iter().enumerate() {
            if i == 0 {
                instance.queue(Lifecycle::Paused);
            }
            self.clearing.push(instance);
        }
        let tween = self.tokens.clear_top;
        if self.tokens.reduce || tween.duration.is_zero() {
            self.finish_clearing();
            self.finish_stack();
        } else {
            self.clear_fade.snap(0.0);
            self.clear_fade.to(
                1.0,
                Tween {
                    duration: tween.duration,
                    easing: Easing::Linear,
                },
            );
            self.layers_dirty = true;
        }
        true
    }

    /// How many instances are being cleared by the clear-top cross-fade (0 = it is not running).
    #[must_use]
    pub fn clearing(&self) -> usize {
        self.clearing.len()
    }

    /// The current opacity of the screen being cleared, `1 → 0`. `None` while it is not running.
    #[must_use]
    pub fn clear_top_alpha(&self) -> Option<f32> {
        self.is_clearing()
            .then(|| transition::clear_top_alpha(self.clear_fade.value(), &self.tokens))
    }

    /// Pop the top (back, `cx.finish`). With the stack emptied, it goes home (an A2 close). `true` if
    /// a pop happened. The one leaving gets a `Paused` at the start and a `Destroyed` at the end; the
    /// new top a `Resumed` at the end.
    pub fn pop(&mut self, tokens: &MotionTokens) -> bool {
        self.settle();
        let base = Self::slot_base(self.focus);
        let Some(task) = self.active_task_mut() else {
            return false;
        };
        if task.len() <= 1 {
            // Popped to the root: the task ends and it goes home.
            return self.close_active_task(tokens);
        }
        let Some(mut outgoing) = task.pop() else {
            return false;
        };
        outgoing.queue(Lifecycle::Paused);
        // The outgoing one is drawn off the stack, so its slot is stated (= the length after the pop, different from the new top's).
        let depth = u32::try_from(task.len()).unwrap_or(u32::MAX);
        outgoing.set_layer_slot(base.saturating_add(depth));
        if tokens.reduce {
            self.bury(outgoing);
            self.finish_stack();
        } else {
            self.stack.pop(outgoing, tokens.pop);
        }
        true
    }

    /// Close a particular instance (`cx.finish`). On the top it is the same as a pop; otherwise it is
    /// removed quietly (with no animation — a transition in progress is finished).
    pub(crate) fn close_instance(&mut self, id: InstanceId, tokens: &MotionTokens) -> bool {
        if self
            .active_task()
            .and_then(Task::top)
            .is_some_and(|t| t.id() == id)
        {
            return self.pop(tokens);
        }
        !self
            .close_where(|instance| instance.id() == id, tokens)
            .is_empty()
    }

    /// The home button: the task stays alive (`Stopped`) and can be returned to from recents. An A2
    /// close (the fallback with no icon Rect). **Mid-A2-open** it reverses from the same `t` (A2).
    /// Already home (a close in progress included), it does nothing.
    pub(crate) fn go_home(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        if self.overview.is_some() {
            // Home from the overview: straight there — the screens are already in their cards.
            self.drop_overview();
            if self.view == WorkspaceView::Tasks {
                self.view = WorkspaceView::Home;
                self.finish_home(true);
            }
            return;
        }
        if self.view == WorkspaceView::Home {
            return;
        }
        self.settle_stack();
        self.settle_split();
        self.drop_gestures();
        // Split, both panes go with it: the focused one closes on its icon, the other fades
        // with the screens (A2), and the split ends once home is reached.
        self.queue_focused(Lifecycle::Paused);
        self.leave_for_home(icon_rect, None, tokens);
    }

    /// The A2 close to home, from the pane — or from `from`, a lifted screen let go.
    fn leave_for_home(
        &mut self,
        icon_rect: Option<Rect>,
        from: Option<Rect>,
        tokens: &MotionTokens,
    ) {
        self.view = WorkspaceView::Home;
        let target = icon_rect.or(self.origin);
        self.origin = target;
        match from {
            Some(from) if !tokens.reduce => self.home.close_from(target, from, tokens.home_close),
            _ => self.start_home_close(target, tokens),
        }
    }

    /// **Start the lift**: the screen on show follows a finger up from the bottom edge,
    /// shrinking as it rises — to the overview's card scale `travel` du up. Only over a task with
    /// nothing moving; `Paused` on the screens on show. `true` where it started.
    pub(crate) fn begin_lift(&mut self, press: Pos2, travel: f32) -> bool {
        if self.view != WorkspaceView::Tasks
            || self.overview.is_some()
            || self.lift.is_some()
            || self.switch.is_some()
            || self.is_animating()
            || self.active_task().is_none()
        {
            return false;
        }
        self.queue_focused(Lifecycle::Paused);
        self.lift = Some(Lift::new(press, travel));
        true
    }

    /// The lifting finger is `offset` du from where it went down, rising at `velocity_up` du/s.
    pub(crate) fn drag_lift(&mut self, offset: egui::Vec2, velocity_up: f32) {
        if let Some(lift) = self.lift.as_mut() {
            lift.drag(offset, velocity_up);
        }
    }

    /// Whether a lift is on — the finger's, or springing back.
    #[must_use]
    pub fn is_lifting(&self) -> bool {
        self.lift.is_some()
    }

    /// How far up the lift is, `0..=1` (0 with none).
    #[doc(hidden)]
    #[must_use]
    pub fn lift_progress(&self) -> f32 {
        self.lift.as_ref().map_or(0.0, Lift::progress)
    }

    /// The lift let go short of home: the screen springs back down, and is `Resumed` there.
    pub(crate) fn lift_back(&mut self, tokens: &MotionTokens) {
        if let Some(lift) = self.lift.as_mut() {
            lift.release(tokens.spring, tokens.reduce);
            if tokens.reduce {
                self.finish_lift();
            }
        }
    }

    /// The lift let go into home: the A2 close carries on from where the finger left the screen
    /// (one pane), or from the panes as they stand (two). `false` with no lift.
    pub(crate) fn lift_home(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) -> bool {
        let Some(lift) = self.lift.take() else {
            return false;
        };
        self.layers_dirty = true;
        let from = self
            .split
            .is_none()
            .then(|| lift.rect(self.focused_pane_rect()));
        self.leave_for_home(icon_rect, from, tokens);
        true
    }

    /// The lift held: the overview comes up, the screen carrying on from where the finger has it
    /// into its card. It was paused when the lift began, so it is not paused again.
    pub(crate) fn lift_to_overview(&mut self, mode: OverviewMode, theme: &crate::theme::Theme) {
        let Some(lift) = self.lift.take() else {
            return;
        };
        self.layers_dirty = true;
        let tokens = theme.motion;
        let current = self.active_task().and_then(root_key);
        let mut overview = Overview::open(current, mode, false, &tokens);
        if self.split.is_none() && !tokens.reduce {
            overview.carry_from(lift.rect(self.last_content));
        }
        self.overview = Some(overview);
    }

    /// The lift is back down: the screens on show go on.
    fn finish_lift(&mut self) {
        if self.lift.take().is_some() {
            self.layers_dirty = true;
            self.queue_focused(Lifecycle::Resumed);
        }
    }

    /// The lift over this frame's home values: one pane scales about the press point
    /// and moves with the finger; two fade where they stand, as under the overview.
    fn lifted(&self, mut home: HomeFrame) -> HomeFrame {
        let Some(lift) = self.lift.as_ref() else {
            return home;
        };
        if self.split.is_some() {
            home.screen_opacity *= 1.0 - LIFT_SPLIT_FADE * lift.progress();
        } else {
            home.screen_scale *= lift.scale();
            home.screen_pivot = Some(lift.pivot());
            home.screen_shift += lift.shift();
        }
        home
    }

    /// A launch, back or close takes over: a lift or a switch under way is dropped where it is —
    /// the screen on show back in place and going on.
    fn drop_gestures(&mut self) {
        if self.lift.take().is_some() {
            self.layers_dirty = true;
            self.queue_focused(Lifecycle::Resumed);
        }
        if let Some(switch) = self.switch.take() {
            self.layers_dirty = true;
            if switch.confirmed() == Some(true) {
                // Let go through already: it lands now.
                self.switch = Some(switch);
                self.finish_switch();
            } else {
                self.queue_focused(Lifecycle::Resumed);
            }
        }
    }

    /// The tasks' root ids by when they were last used, the one on show first.
    fn tasks_by_recency(&self) -> Vec<InstanceId> {
        let mut order: Vec<usize> = (0..self.tasks.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.tasks.get(i).map(Task::last_active)));
        if let Some(current) = self.pane_task_index() {
            order.retain(|&i| i != current);
            order.insert(0, current);
        }
        order
            .into_iter()
            .filter_map(|i| self.tasks.get(i).and_then(root_key))
            .collect()
    }

    /// The task used last — what a slide along the indicator at home brings back.
    #[must_use]
    pub fn most_recent_task(&self) -> Option<usize> {
        (0..self.tasks.len()).max_by_key(|&i| self.tasks.get(i).map(Task::last_active))
    }

    /// **Start the quick switch**: the screen on show slides with a finger along the
    /// indicator, and the task used before it (to the right) or after it (to the left) slides in
    /// beside it. One pane, over a task, with nothing moving and another task to go to. `Paused`
    /// on the screen on show; `true` where it started.
    pub(crate) fn begin_switch(&mut self, width: f32) -> bool {
        if self.view != WorkspaceView::Tasks
            || self.split.is_some()
            || self.overview.is_some()
            || self.lift.is_some()
            || self.switch.is_some()
            || self.is_animating()
        {
            return false;
        }
        let Some(from) = self.active_task().and_then(root_key) else {
            return false;
        };
        // A run goes on while it stands on the task on show; anything else began a new one.
        if self.quick.as_ref().and_then(QuickOrder::current) != Some(from) {
            self.quick = Some(QuickOrder::new(self.tasks_by_recency()));
        }
        let (older, newer) = self.quick.as_ref().map_or((None, None), |run| {
            run.neighbours(|key| self.task_by_key(key).is_some())
        });
        if older.is_none() && newer.is_none() {
            return false;
        }
        self.queue_focused(Lifecycle::Paused);
        self.switch = Some(Switch::new(from, older, newer, width));
        true
    }

    /// The sliding finger is `offset` du along the indicator from where it went down, at
    /// `velocity` du/s.
    pub(crate) fn drag_switch(&mut self, offset: f32, velocity: f32) {
        if let Some(switch) = self.switch.as_mut() {
            switch.drag(offset, velocity);
        }
    }

    /// The slide let go: through to the task coming in where `through` (and there is one that
    /// way), or back.
    pub(crate) fn release_switch(&mut self, through: bool, tokens: &MotionTokens) {
        if let Some(switch) = self.switch.as_mut() {
            switch.release(through, tokens.spring, tokens.reduce);
            if tokens.reduce {
                self.finish_switch();
            }
        }
    }

    /// Whether a quick switch is on — the finger's, or landing.
    #[must_use]
    pub fn is_switching(&self) -> bool {
        self.switch.is_some()
    }

    /// How far the quick switch has slid, as a fraction of the pane's width (positive to the
    /// right; 0 with none).
    #[doc(hidden)]
    #[must_use]
    pub fn switch_progress(&self) -> f32 {
        self.switch.as_ref().map_or(0.0, Switch::progress)
    }

    /// The run of quick switches is over — the screen on show was touched, or something else
    /// moved on. The next slide starts from the tasks as they were last used.
    pub(crate) fn end_quick_switch(&mut self) {
        self.quick = None;
    }

    /// The switch has landed: through, the task that came in is on show (`Resumed`) and the one
    /// that went is `Stopped`; back, the one on show goes on.
    fn finish_switch(&mut self) {
        let Some(switch) = self.switch.take() else {
            return;
        };
        self.layers_dirty = true;
        let incoming = switch
            .incoming()
            .filter(|_| switch.confirmed() == Some(true));
        let Some((key, index)) = incoming.and_then(|key| Some((key, self.task_by_key(key)?)))
        else {
            self.queue_focused(Lifecycle::Resumed);
            return;
        };
        if let Some(previous) = self
            .task_by_key(switch.from)
            .and_then(|i| self.tasks.get_mut(i))
        {
            for instance in previous.iter_mut() {
                instance.queue(Lifecycle::Stopped);
            }
        }
        self.set_pane_task(Some(index));
        if let Some(task) = self.tasks.get_mut(index) {
            task.touch();
            if let Some(top) = task.top_mut() {
                top.queue(Lifecycle::Resumed);
            }
        }
        // Its icon is not where it came from; home finds it again.
        self.origin = None;
        if let Some(run) = self.quick.as_mut() {
            run.step_to(key);
        }
    }

    /// End the active task (popping to the root, an Overview swipe). The task comes straight off the
    /// stack (it is in neither `find` nor `tasks`) and is drawn as `Workspace::closing_task` only
    /// through the A2 close, then `Destroyed` at the end of the transition. The origin is the last
    /// icon Rect the shell handed over (`Workspace::origin`).
    pub(crate) fn close_active_task(&mut self, tokens: &MotionTokens) -> bool {
        self.close_active_task_with(self.origin, tokens)
    }

    /// Hand [`Workspace::close_active_task`] the A2 close's origin (the icon Rect) directly.
    pub(crate) fn close_active_task_with(
        &mut self,
        icon_rect: Option<Rect>,
        tokens: &MotionTokens,
    ) -> bool {
        self.settle();
        let Some(index) = self.pane_task_index() else {
            return false;
        };
        if index >= self.tasks.len() {
            return false;
        }
        let mut task = self.tasks.remove(index);
        self.shift_panes_after_remove(index);
        if let Some(top) = task.top_mut() {
            top.queue(Lifecycle::Paused);
        }
        // Split, the task ends with its pane: it slides out and the other pane fills the content
        // ("a stack emptied ends the split"). Home is not where this goes.
        if self.split.is_some() && self.panes.len() == 2 {
            let leaving = self.focus;
            self.leaving_task = Some(task);
            self.start_leaving(leaving, tokens);
            return true;
        }
        self.view = WorkspaceView::Home;
        self.origin = None;
        if tokens.reduce {
            self.bury_task(task);
            self.finish_home(true);
        } else {
            self.closing_task = Some(task);
            self.home.close(icon_rect, tokens.home_close);
        }
        true
    }

    /// The resident screen of a declaration that is being replaced or removed goes to its open
    /// instance, wherever that is — on a stack, leaving through a transition or waiting for its
    /// `Destroyed` — so what that instance hears from now on reaches the screen it ran, not
    /// whatever the registry holds under the id next. Without an instance it is dropped here.
    pub(crate) fn adopt_resident(&mut self, decl_id: &str, screen: Box<dyn crate::screen::Screen>) {
        let outgoing = match &mut self.stack {
            StackTransition::Popping { outgoing, .. }
            | StackTransition::DraggingBack { outgoing, .. } => outgoing.as_deref_mut(),
            StackTransition::Idle | StackTransition::Pushing { .. } => None,
        };
        let mut all = self
            .tasks
            .iter_mut()
            .chain(self.closing_task.iter_mut())
            .chain(self.leaving_task.iter_mut())
            .flat_map(Task::iter_mut)
            .chain(outgoing)
            .chain(self.graveyard.iter_mut());
        if let Some(instance) = all.find(|i| i.decl_id() == decl_id && i.is_resident()) {
            instance.adopt(screen);
        }
    }

    /// `remove(id)`: end every instance of that declaration. No animation. With none at all, a
    /// transition in progress is left alone.
    pub(crate) fn close_decl(&mut self, decl_id: &str, tokens: &MotionTokens) {
        let _closed = self.close_where(|instance| instance.decl_id() == decl_id, tokens);
    }

    /// End every instance matching a condition (an instance failing its gate on a session downgrade,
    /// `evict_after`). No animation. It hands back the (declaration id, instance id) pairs closed —
    /// the shell raises `ScreenClosed`. With none matching, a transition in progress is left alone.
    /// Where the instance leaving through a transition matches, only the transition is finished (it
    /// is already closed, so it is not in the list).
    ///
    /// **A transition in progress is cut short only when the foreground (the active Pane's task,
    /// mid-transition) is what closes.** A background task's `evict_after` expiring, or another
    /// task's tidying up, must not snap the foreground's A2 / A3 to its end — only a user command
    /// interrupts a transition, and memory policy stays out of sight.
    pub fn close_where(
        &mut self,
        mut pred: impl FnMut(&Instance) -> bool,
        tokens: &MotionTokens,
    ) -> Vec<(String, InstanceId)> {
        // Of the active task only the top two are drawn — the one on show and the one a push or
        // a pop slides it over. An instance stopped deeper down goes without touching the motion.
        let active_index = self.pane_task_index();
        let in_active = active_index
            .and_then(|i| self.tasks.get(i))
            .into_iter()
            .flat_map(|task| task.iter().skip(task.len().saturating_sub(2)))
            .any(&mut pred);
        let in_stacks = in_active || self.tasks.iter().flat_map(Task::iter).any(&mut pred);
        let in_transit = self.closing_task.iter().flat_map(Task::iter).any(&mut pred)
            || self.leaving_task.iter().flat_map(Task::iter).any(&mut pred)
            || matches!(&self.stack, StackTransition::Popping { outgoing: Some(out), .. } if pred(out));
        if !in_stacks && !in_transit {
            return Vec::new();
        }
        if in_active || in_transit {
            self.settle();
        }
        let mut closed = Vec::new();
        if !in_stacks {
            return closed;
        }
        // Every task on show — the focused pane's, and the other pane's while split.
        let shown: Vec<usize> = if self.view == WorkspaceView::Tasks {
            self.panes.iter().filter_map(|p| p.task).collect()
        } else {
            Vec::new()
        };
        let mut changed_shown: Vec<usize> = Vec::new();
        let mut buried = Vec::new();
        for (index, task) in self.tasks.iter_mut().enumerate() {
            if !task.iter().any(&mut pred) {
                // A task with no match is left alone (`last_active` too).
                continue;
            }
            let was_top = task.top().map(Instance::id);
            let mut all = Vec::new();
            while let Some(instance) = task.pop() {
                all.push(instance);
            }
            all.reverse();
            for mut instance in all {
                if pred(&instance) {
                    closed.push((instance.decl_id().to_owned(), instance.id()));
                    if shown.contains(&index) && was_top == Some(instance.id()) {
                        instance.queue(Lifecycle::Paused);
                    }
                    buried.push(instance);
                } else {
                    task.push(instance);
                }
            }
            // Only a task on show whose top changed has a screen newly on show.
            if shown.contains(&index) && task.top().map(Instance::id) != was_top {
                changed_shown.push(index);
            }
        }
        for instance in buried {
            self.bury(instance);
        }
        // The new top gets a `Resumed`. **Only where something really came out of a task on show** — a
        // background task being tidied up must not make a lifecycle event on the foreground screen (and
        // mid-transition it would throw the order out as well).
        let on_top = self.on_show();
        for index in changed_shown {
            if let Some(top) = self.tasks.get_mut(index).and_then(Task::top_mut) {
                top.queue(on_top);
            }
        }
        self.prune_empty_tasks(tokens);
        closed
    }

    /// Frame stage 5: end the `Stopped` spawned instances whose `evict_after` has passed
    /// (`Instance::evict_due`). `now` is the shell's time.
    pub(crate) fn evict_expired(
        &mut self,
        now: Instant,
        tokens: &MotionTokens,
    ) -> Vec<(String, InstanceId)> {
        self.close_where(|instance| instance.evict_due(now), tokens)
    }

    /// The earliest moment [`Workspace::evict_expired`] has something to end, so that an idle
    /// panel wakes for it.
    #[must_use]
    pub(crate) fn next_eviction(&self) -> Option<Instant> {
        self.tasks
            .iter()
            .flat_map(Task::iter)
            .filter_map(Instance::evict_at)
            .min()
    }

    fn prune_empty_tasks(&mut self, _tokens: &MotionTokens) {
        let mut index = 0;
        while index < self.tasks.len() {
            if self.tasks.get(index).is_some_and(Task::is_empty) {
                self.tasks.remove(index);
                self.shift_panes_after_remove(index);
            } else {
                index += 1;
            }
        }
        // A pane whose task has gone takes the split with it: the other pane fills the content.
        self.collapse_to_live_pane();
        if self.view == WorkspaceView::Tasks && self.active_task().is_none() {
            // The active task has gone entirely — home, with no animation.
            self.view = WorkspaceView::Home;
            self.origin = None;
            self.finish_home(true);
        }
    }

    fn bury(&mut self, mut instance: Instance) {
        instance.queue(Lifecycle::Destroyed);
        self.graveyard.push(instance);
    }

    fn bury_task(&mut self, mut task: Task) {
        while let Some(instance) = task.pop() {
            self.bury(instance);
        }
    }

    /// Handle the end of a home transition: on an open, `Resumed` on the top; on a close, `Stopped`
    /// throughout the task (or `Destroyed` for a task ended by a root pop).
    fn finish_home(&mut self, closing: bool) {
        self.layers_dirty = true;
        let index = self.pane_task_index();
        if closing {
            if let Some(task) = index.and_then(|i| self.tasks.get_mut(i)) {
                for instance in task.iter_mut() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
            // Split, the other pane's task went home too: stopped, and one pane again.
            if self.split.is_some() {
                let other = 1 - self.focus.min(1);
                if let Some(task) = self
                    .panes
                    .get(other)
                    .and_then(|p| p.task)
                    .and_then(|i| self.tasks.get_mut(i))
                {
                    for instance in task.iter_mut() {
                        if !instance.is_stopped() {
                            instance.queue(Lifecycle::Stopped);
                        }
                    }
                }
                if let Some(task) = self.leaving_task.take() {
                    self.bury_task(task);
                }
                self.panes.truncate(1);
                if let Some(pane) = self.panes.first_mut() {
                    pane.id = PaneId(0);
                }
                self.split = None;
                self.focus = 0;
            }
            self.set_pane_task(None);
            if let Some(task) = self.closing_task.take() {
                self.bury_task(task);
            }
        } else if let Some(top) = index
            .and_then(|i| self.tasks.get_mut(i))
            .and_then(Task::top_mut)
        {
            top.queue(Lifecycle::Resumed);
        }
    }

    /// Handle the end of a stack transition: `Resumed` on the top, `Stopped` below it.
    fn finish_stack(&mut self) {
        self.layers_dirty = true;
        let on_top = self.on_show();
        if let Some(task) = self.active_task_mut() {
            let len = task.len();
            for (i, instance) in task.iter_mut().enumerate() {
                if i + 1 == len {
                    instance.queue(on_top);
                } else if !instance.is_stopped() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
        }
    }

    /// Advance the frame: move the transition values by `dt` and wrap up any that finished.
    pub fn tick(&mut self, dt: f32) {
        let closing = matches!(self.home, HomeTransition::Closing { .. });
        if self.home.tick(dt) {
            self.finish_home(closing);
        }
        if self.lift.as_mut().is_some_and(|lift| lift.tick(dt)) {
            self.finish_lift();
        }
        if self.switch.as_mut().is_some_and(|switch| switch.tick(dt)) {
            self.finish_switch();
        }
        if let Some(done) = self.stack.tick(dt) {
            if let Some(out) = done {
                self.bury(out);
            }
            self.finish_stack();
        }
        if self.is_clearing() && !self.clear_fade.tick(dt) {
            self.finish_clearing();
            self.finish_stack();
        }
        self.tick_split(dt);
        if let Some(ticked) = self.overview.as_mut().map(|o| o.tick(dt)) {
            if ticked.returned {
                self.finish_overview_return();
            }
            if let Some(key) = ticked.thrown {
                let closed = self.close_card(key);
                self.overview_closed.extend(closed);
            }
        }
    }

    /// Advance the split (A8): an enter finished resumes the pane that came in, a leave finished
    /// takes its pane out, and a split come to rest after a change sizes its panes once.
    fn tick_split(&mut self, dt: f32) {
        let Some(split) = self.split.as_mut() else {
            return;
        };
        let phase = split.phase();
        let left = split.tick(dt);
        let entered = matches!(phase, Phase::Entering(_)) && split.phase() == Phase::Split;
        let resize = split.take_resize();
        if let Some(pane) = left {
            self.finish_leaving(pane);
            return;
        }
        if entered {
            self.finish_entering();
        }
        if resize {
            self.queue_split_resize();
        }
    }

    /// Frame stage 9: draw the desktop or the Pane. Zero heap allocation per frame — it only works
    /// out values and registers `Area`s (cloning a `Context` bumps an `Arc` refcount).
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        registry: &mut crate::screen::Registry,
        desktop: &mut DesktopView,
        layout: &Layout,
        painters: &mut RecentsPainters,
    ) -> WorkspaceOutput {
        let ctx = ui.ctx().clone();
        let content = layout.content;
        self.last_content = content;
        let tokens = parts.theme.motion;
        // `clear_top_to` takes no tokens as an argument — they are cached here.
        self.set_motion(&tokens);
        self.split_metrics = (
            parts.theme.metrics.split_divider,
            parts.theme.metrics.touch_target,
        );
        let mut out = WorkspaceOutput::default();

        // A press moves the focus to the pane under it — before anything is drawn, so the
        // screens drawn this frame already know which of them has it.
        if self.split.is_some() && !self.blocks_input() {
            let press = ctx.input(|i| {
                i.pointer
                    .any_pressed()
                    .then(|| crate::drag::press_point(i))
                    .flatten()
            });
            if let Some(at) = press {
                self.focus_at(&ctx, at);
            }
        }
        self.hold_split(content);
        if self.layers_dirty {
            self.layers_dirty = false;
            self.reset_layer_transforms(&ctx);
        }
        // The sizing pass is run through even the slot that may be used next frame.
        self.warm_layer_slots(&ctx, content);
        let frame = self.pane_frame(content);
        self.pane_rects = frame.visible;
        self.pane_laid = frame.laid;
        // A2 maps onto the focused pane — the whole content with one — and the lift over it.
        let focused_pane = pane_of(frame.visible, self.focus);
        let home = self.lifted(self.home_frame(focused_pane, &tokens));

        // The desktop layer — painted plain under the overview over a task, where it is the
        // backdrop the screen shrinks against (A10).
        out.desktop_action = self.desktop_ui(
            &ctx,
            parts,
            desktop,
            content,
            &home,
            painters.ground.as_mut(),
        );

        // The Pane layer(s), then the divider over them — or, under the overview, the screen on
        // show shrinking into its card (A10), and nothing at all once the cards cover the content.
        let look = self.overview.as_ref().map(|o| {
            let deck = o.deck(content, parts.theme);
            (o.is_full(), o.screen_look(&deck))
        });
        match look {
            Some((true, _)) => {}
            Some((false, look)) => {
                let mut shrinking = home;
                // Split, the panes fade where they stand; one pane shrinks into its card.
                if self.split.is_none() {
                    shrinking.screen_scale *= look.scale;
                    shrinking.screen_pivot = Some(look.pivot);
                    shrinking.screen_shift += look.shift;
                }
                shrinking.screen_opacity *= look.opacity;
                self.draw_panes(&ctx, parts, registry, content, &frame, &shrinking, &tokens);
            }
            None => {
                self.draw_panes(&ctx, parts, registry, content, &frame, &home, &tokens);
                self.divider_ui(&ctx, parts.theme, content, &frame);
            }
        }
        if look.is_some() {
            self.divider_handle = Rect::NOTHING;
        }
        self.end_stray_divider_drag(&ctx, content);
        if self.overview.is_some() {
            let cards = self.overview_cards(parts.access, registry);
            let picked = self
                .overview
                .as_mut()
                .and_then(|overview| overview.ui(&ctx, parts, content, &cards, &tokens, painters));
            if let Some(picked) = picked {
                let closed = self.apply_overview(picked);
                out.closed.extend(closed);
            }
        }
        out.closed.append(&mut self.overview_closed);
        // A3's "blocking input: a shield mid-tween" (M1 had `interactable(false)` alone). The back
        // gesture, where the finger is leading, is excluded — the gesture engine already has the same
        // `Area` up throughout it (shell stage 11), which would draw it twice in one frame.
        if self.needs_shield() {
            let screen = screen_of(layout);
            if screen.is_positive() {
                let _ = crate::gesture::shield::show(&ctx, screen);
                // Always the top within Foreground (it does not lean on the registration order).
                ctx.move_to_top(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new(crate::gesture::shield::AREA_ID),
                ));
            }
        }
        self.sync_sizes();
        out
    }

    /// The desktop layer, where it shows: the home view, A2 flying over it, or the recent screens'
    /// ground — the backdrop the screen shrinks against under the overview over a task (A10), plain
    /// or a painter's. Returns what was tapped on it, where it takes input.
    fn desktop_ui(
        &self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        desktop: &mut DesktopView,
        content: Rect,
        home: &HomeFrame,
        ground: Option<&mut painters::RecentsGroundPainter>,
    ) -> Option<DesktopAction> {
        // The plain backdrop the screen shrinks against: under the overview over a task, and
        // under a lift — the overview it may become has the same one.
        let backdrop_only =
            (self.overview.is_some() || self.lift.is_some()) && self.view == WorkspaceView::Tasks;
        let show_desktop =
            self.view == WorkspaceView::Home || self.home.is_active() || backdrop_only;
        if !show_desktop {
            ctx.set_transform_layer(DesktopView::layer_id(), TSTransform::IDENTITY);
            return None;
        }
        let transform = scale_about(content.center(), home.desktop_scale);
        ctx.set_transform_layer(DesktopView::layer_id(), transform);
        let interactable = !self.home.is_active() && self.overview.is_none();
        let action = egui::Area::new(DesktopView::area_id())
            .order(egui::Order::Background)
            .fixed_pos(content.min)
            .default_size(content.size())
            .constrain(false)
            .fade_in(false)
            .interactable(interactable)
            .show(ctx, |ui| {
                ui.set_clip_rect(content);
                ui.set_min_size(content.size());
                if backdrop_only {
                    // Whole from the start: the screen shrinking into its card covers it.
                    let over = painters::RecentsOver::Task;
                    paint_ground(ui.painter(), parts.theme, ground, content, over, 1.0);
                    return None;
                }
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
                child.set_opacity(home.desktop_opacity);
                let mut dcx = DesktopCtx {
                    theme: parts.theme,
                    access: parts.access,
                    icons: &mut *parts.icons,
                    now: parts.now,
                    legibility: desktop.legibility(),
                    strings: parts.strings,
                };
                desktop.ui(&mut child, content, &mut dcx)
            })
            .inner;
        action.filter(|_| interactable)
    }

    /// Whether a tween is running and input has to be blocked (A3 · A2, "blocking input"). The
    /// back gesture, where a finger is driving the value (`confirmed == None`), is **input itself**
    /// and is not blocked.
    fn needs_shield(&self) -> bool {
        if self.home.is_active() {
            return true;
        }
        // A lift or a switch let go moves on its own; while the finger has it, the gesture
        // engine's shield is up already.
        if self.lift.as_ref().is_some_and(Lift::is_released)
            || self
                .switch
                .as_ref()
                .is_some_and(|s| s.confirmed().is_some())
        {
            return true;
        }
        if self.split.as_ref().is_some_and(Split::is_tweening) {
            return true;
        }
        if self.overview.as_ref().is_some_and(Overview::is_tweening) {
            return true;
        }
        if self.is_clearing() {
            return true;
        }
        match self.stack {
            StackTransition::Idle => false,
            StackTransition::DraggingBack { confirmed, .. } => confirmed.is_some(),
            StackTransition::Pushing { .. } | StackTransition::Popping { .. } => true,
        }
    }

    /// Tidy up the layer transforms left after a transition. The slots are renumbered by stack depth
    /// (before drawing) and all those layers put back to `IDENTITY` — whichever are drawn will be set
    /// again straight away.
    fn reset_layer_transforms(&mut self, ctx: &egui::Context) {
        for pane in 0..self.panes.len().max(1) {
            let base = Self::slot_base(pane);
            if let Some(task) = self.task_of_pane_mut(pane) {
                for (depth, instance) in task.iter_mut().enumerate() {
                    let slot = base.saturating_add(u32::try_from(depth).unwrap_or(u32::MAX));
                    instance.set_layer_slot(slot);
                    ctx.set_transform_layer(instance.layer_id(), TSTransform::IDENTITY);
                }
            }
        }
    }

    /// The slot warm-up. An egui `Area` with no `AreaState` runs its first frame as a
    /// sizing pass (`ui_builder.invisible()`) and throws that frame's output away whole — which is
    /// what leaves an incoming screen blank on the frame a transition starts. `Areas::set_state` is
    /// `pub(crate)`, so the state cannot be planted; instead **an empty `Area` is drawn once
    /// beforehand** to consume that pass first. Exactly once per slot, and nothing is drawn on that
    /// frame.
    ///
    /// [`WARM_SLOTS`] of them are run through on the first frame, and as the stack deepens the
    /// **next slot to be used** (`depth + 1`) is run through in advance — a launch or back request is
    /// handled at frame stage 14 and that instance is first drawn on the following frame, so this
    /// frame's warm-up is always a step ahead.
    fn warm_layer_slots(&mut self, ctx: &egui::Context, content: Rect) {
        let depth_of = |ws: &Self, pane: usize| ws.task_of_pane(pane).map_or(0, Task::len);
        let split = self.split.is_some();
        // The first pane's slots also serve a second pane left alone — it moves to them.
        let first = depth_of(self, 0)
            .max(self.closing_task.as_ref().map_or(0, Task::len))
            .max(if split { depth_of(self, 1) } else { 0 });
        let need = |depth: usize, least: u32| {
            u32::try_from(depth)
                .unwrap_or(u32::MAX)
                .saturating_add(1)
                .max(least)
                .min(PANE_SLOTS)
        };
        // The second pane's only while it is up: a pane coming in starts outside the content, so
        // its first frame — the one a sizing pass would blank — is not on the glass anyway.
        let wanted = [
            need(first, WARM_SLOTS),
            if split { need(depth_of(self, 1), 0) } else { 0 },
        ];
        for (pane, want) in wanted.into_iter().enumerate() {
            let base = Self::slot_base(pane);
            while self.warmed_slots.get(pane).is_some_and(|w| *w < want) {
                let Some(warmed) = self.warmed_slots.get_mut(pane) else {
                    break;
                };
                let slot = base.saturating_add(*warmed);
                *warmed = warmed.saturating_add(1);
                warm_slot(ctx, slot, content);
            }
        }
    }
}

/// Run one slot's sizing pass with nothing in it (see [`Workspace::warm_layer_slots`]).
fn warm_slot(ctx: &egui::Context, slot: u32, content: Rect) {
    egui::Area::new(Instance::slot_area_id(slot))
        .order(egui::Order::Background)
        .fixed_pos(content.min)
        .default_size(content.size())
        .constrain(false)
        .fade_in(false)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_min_size(content.size());
        });
}

impl Workspace {
    /// This frame's A2 values ([`HomeFrame::IDLE`] where there are none).
    fn home_frame(&self, content: Rect, tokens: &MotionTokens) -> HomeFrame {
        match self.home {
            HomeTransition::Idle => HomeFrame::IDLE,
            HomeTransition::Opening {
                t,
                icon_rect: Some(icon),
            } => HomeFrame::card(&transition::a2_open(t.value(), icon, content, tokens)),
            HomeTransition::Closing {
                t,
                icon_rect: Some(icon),
                from,
            } => {
                // A lifted screen let go into home: the card starts where the finger
                // left the screen, and the screen shrinks inside it as it fades.
                let m = transition::a2_close(t.value(), icon, from.unwrap_or(content), tokens);
                let mut frame = HomeFrame::card(&m);
                if from.is_some() {
                    frame.fit_screen(m.card_rect, content);
                }
                frame
            }
            HomeTransition::Opening { t, icon_rect: None } => {
                HomeFrame::fallback(&transition::a2_fallback_open(t.value(), tokens))
            }
            HomeTransition::Closing {
                t,
                icon_rect: None,
                from,
            } => {
                let m = transition::a2_fallback_close(t.value(), tokens);
                let mut frame = HomeFrame::fallback(&m);
                if let Some(from) = from {
                    let shrunk =
                        Rect::from_center_size(from.center(), from.size() * m.screen_scale);
                    frame.fit_screen(shrunk, content);
                }
                frame
            }
        }
    }

    /// Hold the split inside what its panes allow (a screen with a larger minimum came to the top,
    /// the content resized under it). Where no ratio holds both any more, back to one pane.
    fn hold_split(&mut self, content: Rect) {
        let Some(phase) = self.split.as_ref().map(Split::phase) else {
            return;
        };
        if !content.is_positive() {
            return;
        }
        let axis = self.axis_for(content);
        // A screen that never shares came to the top of a pane — a task resumed into it, a pop
        // that uncovered it: one pane again, the focused one (a push of one ends the split as it
        // opens, `Workspace::open`). Without this it would sit in half the content at a minimum
        // it never named.
        let target = self.split_metrics.1;
        let refused = (0..self.panes.len()).any(|pane| {
            self.task_of_pane(pane)
                .and_then(Task::top)
                .is_some_and(|top| {
                    split::min_len(top.split_support(), content, axis, target).is_none()
                })
        });
        if refused && phase == Phase::Split {
            self.collapse_to_focused();
            return;
        }
        let mins = self.pane_mins(content, axis);
        match split::bounds(content, axis, self.split_metrics.0, mins) {
            Some(bounds) => {
                if let Some(split) = self.split.as_mut() {
                    split.keep_within(Some(bounds));
                }
            }
            None if phase == Phase::Split => self.collapse_to_focused(),
            None => {}
        }
    }

    /// This frame's pane rects: the content with one pane, the split's geometry with two.
    fn pane_frame(&self, content: Rect) -> PaneFrame {
        let Some(split) = self.split.as_ref() else {
            return PaneFrame::single(content);
        };
        let axis = self.axis_for(content);
        let mins = self.pane_mins(content, axis);
        let g = split.geometry(content, axis, self.split_metrics.0, mins);
        PaneFrame {
            visible: g.visible,
            laid: g.laid,
            squeeze: g.squeeze,
            divider: Some((g.divider, g.divider_alpha)),
            other: Some(1 - self.focus.min(1)),
        }
    }

    /// The height the OSK covers of a pane at `visible` — the part of the content's bottom inset
    /// that reaches up into it.
    fn pane_inset(&self, visible: Rect, content: Rect) -> f32 {
        if !visible.is_positive() {
            return 0.0;
        }
        (self.inset_bottom - (content.max.y - visible.max.y)).max(0.0)
    }

    /// The Pane (stack) render: mid-transition it draws two instances on offset layers
    /// (A3). The layer slots are lent out here by stack depth (a finite set), from each pane's base.
    /// During an A2, the placeholder card is drawn first inside the top instance's `Area` (the
    /// card's colour is the declaration's `background` or surface, and the icon the task root's),
    /// and the real screen is drawn over it from `screen_opacity > 0` (an open's `t ≥ 0.5`) on.
    ///
    /// **Split, the other pane is drawn still** — its top, in its own rect — and every transition
    /// is the focused pane's, inside its rect. Each screen is cut at its pane's edge on
    /// the glass, so a push slides within its pane and not across the other.
    #[allow(clippy::too_many_arguments)] // The frame's parts, and the three things worked out for it.
    #[allow(clippy::too_many_lines)] // One pane's draw as it always was, and the other pane's before it.
    fn draw_panes(
        &mut self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        registry: &mut crate::screen::Registry,
        content: Rect,
        frame: &PaneFrame,
        home: &HomeFrame,
        tokens: &MotionTokens,
    ) {
        let closing = matches!(self.home, HomeTransition::Closing { .. });
        if !(self.view == WorkspaceView::Tasks || closing) {
            return;
        }
        let focus = self.focus.min(1);
        let laid = pane_of(frame.laid, focus);
        // An A2 card flies between the pane and its icon, so it is cut only at the content's edge.
        let clip = if home.card.is_some() {
            content
        } else {
            pane_of(frame.visible, focus)
        };
        let insets = [
            self.pane_inset(frame.visible[0], content),
            self.pane_inset(frame.visible[1], content),
        ];
        let inset = pane_of(insets, focus);
        let split = self.split.is_some();
        let width = laid.width();
        let a3 = a3_mapping(&self.stack, width, tokens);
        let animating = self.blocks_input();
        let pushing = matches!(self.stack, StackTransition::Pushing { .. });
        // The top is on its way right with the finger (an unconfirmed gesture, or a cancel returning).
        let back_top_moves = matches!(
            self.stack,
            StackTransition::DraggingBack { outgoing: None, .. }
        );
        let popping = matches!(self.stack, StackTransition::Popping { .. })
            || matches!(
                self.stack,
                StackTransition::DraggingBack {
                    outgoing: Some(_),
                    ..
                }
            );
        // The clear-top cross-fade: only the top being cleared fades out by alpha — it is drawn in
        // place over the screen below, so its layer is raised as the outgoing one's is.
        let clear_alpha = self.clear_top_alpha();
        let raise_outgoing = raises_outgoing(&self.stack) || clear_alpha.is_some();
        let bases = [Self::slot_base(0), Self::slot_base(1)];
        let leaving = self.leaving_pane();
        // The quick switch: the screen on show slides with the finger and the next
        // task's comes in beside it, on the second pane's slots — one pane only.
        let (switch_out, switch_in) = match self.switch.as_ref() {
            Some(switch) if !split => {
                let (out, inn) = switch.offsets();
                (out, switch.incoming().map(|key| (key, inn)))
            }
            _ => (0.0, None),
        };
        let Self {
            tasks,
            panes,
            stack,
            closing_task,
            clearing,
            leaving_task,
            ..
        } = self;

        let draw = &mut Draw { parts, registry };

        // The other pane: its top, still — or sliding with its pane on the way in or out.
        if let Some(other) = frame.other {
            let task = if leaving == Some(other) && leaving_task.is_some() {
                leaving_task.as_mut()
            } else {
                panes
                    .get(other)
                    .and_then(|p| p.task)
                    .and_then(|i| tasks.get_mut(i))
            };
            if let Some(task) = task {
                let base = pane_of(bases, other);
                for (depth, instance) in task.iter_mut().enumerate() {
                    instance.set_layer_slot(
                        base.saturating_add(u32::try_from(depth).unwrap_or(u32::MAX)),
                    );
                }
                if let Some(top) = task.top_mut() {
                    let params = DrawParams {
                        inset_bottom: pane_of(insets, other),
                        x: 0.0,
                        opacity: home.screen_opacity,
                        scale: 1.0,
                        pivot: None,
                        shift: egui::Vec2::ZERO,
                        dim: pane_of(frame.squeeze, other) * split::SQUEEZE_DIM,
                        shadow: 0.0,
                        focused: !animating,
                        raise: false,
                        clip: pane_of(frame.visible, other),
                        seat: Seat::Beside,
                    };
                    draw_instance(
                        ctx,
                        draw,
                        top,
                        pane_of(frame.laid, other),
                        &params,
                        None,
                        None,
                    );
                }
            }
        }

        if let Some((key, x_in)) = switch_in {
            if let Some(task) = tasks.iter_mut().find(|t| root_key(t) == Some(key)) {
                let depth = u32::try_from(task.len().saturating_sub(1)).unwrap_or(u32::MAX);
                if let Some(top) = task.top_mut() {
                    top.set_layer_slot(pane_of(bases, 1).saturating_add(depth));
                    let params = DrawParams {
                        inset_bottom: inset,
                        x: x_in,
                        opacity: 1.0,
                        scale: 1.0,
                        pivot: None,
                        shift: egui::Vec2::ZERO,
                        dim: 0.0,
                        shadow: 0.0,
                        focused: false,
                        raise: false,
                        clip,
                        seat: Seat::Alone,
                    };
                    draw_instance(ctx, draw, top, laid, &params, None, None);
                }
            }
        }

        let look = PaneLook { clip, split, inset };
        draw_outgoing(ctx, draw, stack, laid, &a3, home, &look);
        draw_clearing(ctx, draw, clearing, laid, clear_alpha, &look);

        let pane_task = panes
            .get(focus)
            .and_then(|p| p.task)
            .and_then(|i| tasks.get_mut(i));
        let Some(task) = pane_task.or(closing_task.as_mut()) else {
            return;
        };
        let len = task.len();
        let base = pane_of(bases, focus);
        for (depth, instance) in task.iter_mut().enumerate() {
            instance.set_layer_slot(base.saturating_add(u32::try_from(depth).unwrap_or(u32::MAX)));
        }
        if (pushing || back_top_moves) && len >= 2 {
            if let Some(below) = task.iter_mut().nth(len - 2) {
                let params = DrawParams {
                    inset_bottom: inset,
                    x: a3.x_out,
                    opacity: home.screen_opacity,
                    scale: home.screen_scale,
                    pivot: home.screen_pivot,
                    shift: home.screen_shift,
                    dim: a3.dim,
                    shadow: 0.0,
                    focused: false,
                    raise: false,
                    clip,
                    seat: Seat::of(split),
                };
                draw_instance(ctx, draw, below, laid, &params, None, None);
            }
        }
        // The top (plus the root, for the card's icon). The two are different slots, so they can be borrowed at once.
        let mut iter = task.iter_mut();
        let Some(root) = iter.next() else {
            return;
        };
        let (root_icon, top) = match iter.last() {
            Some(top) => (root.icon(), top),
            None => (None, root),
        };
        let x = if pushing || back_top_moves {
            a3.x_in
        } else if popping {
            a3.x_out
        } else {
            switch_out
        };
        let squeeze = pane_of(frame.squeeze, focus) * split::SQUEEZE_DIM;
        let params = DrawParams {
            inset_bottom: inset,
            x,
            opacity: home.screen_opacity,
            scale: home.screen_scale,
            pivot: home.screen_pivot,
            shift: home.screen_shift,
            dim: if popping {
                a3.dim.max(squeeze)
            } else {
                squeeze
            },
            shadow: if pushing || back_top_moves {
                a3.shadow
            } else {
                0.0
            },
            focused: !animating,
            raise: !raise_outgoing,
            clip,
            seat: Seat::of(split),
        };
        draw_instance(ctx, draw, top, laid, &params, home.card.as_ref(), root_icon);
    }

    /// The divider: the band between the panes with a grip at its middle, the focused
    /// pane's outline, and the handle a finger moves it by — a drag follows 1:1, a release springs
    /// to where both panes keep their minimums or closes the pane it was pushed into, a double
    /// tap evens it out.
    ///
    /// Two `Area`s: what is drawn is in one that is click-through, so that it can cover the
    /// content without taking a press from the panes, and the handle is a second, no bigger than
    /// the handle — an interactable `Area` takes every press inside its own rect.
    fn divider_ui(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::theme::Theme,
        content: Rect,
        frame: &PaneFrame,
    ) {
        let Some((band, alpha)) = frame.divider else {
            self.divider_handle = Rect::NOTHING;
            return;
        };
        let axis = self.axis_for(content);
        let target = theme.metrics.touch_target;
        let grow = ((target - axis.along(band.size())) / 2.0).max(0.0);
        let handle = match axis {
            SplitAxis::SideBySide => band.expand2(egui::vec2(grow, 0.0)),
            SplitAxis::Stacked => band.expand2(egui::vec2(0.0, grow)),
        };
        let resting = matches!(self.split.as_ref().map(Split::phase), Some(Phase::Split));
        let live = resting
            && self.view == WorkspaceView::Tasks
            && !self.home.is_active()
            && !self.stack.is_active()
            && !self.is_clearing();
        self.divider_handle = if live { handle } else { Rect::NOTHING };
        let dragging = self.split.as_ref().is_some_and(Split::is_dragging);

        // What is drawn: the band, the grip, the focus outline.
        let chrome = egui::Id::new(DIVIDER_ID);
        let focus = pane_of(frame.visible, self.focus);
        paint_divider(
            ctx,
            theme,
            content,
            DividerLook {
                band,
                alpha,
                axis,
                dragging,
                focus: resting.then_some(focus),
            },
        );
        if !live {
            return;
        }
        // The handle.
        let handle_id = chrome.with("handle");
        ctx.move_to_top(egui::LayerId::new(egui::Order::Background, handle_id));
        let action = egui::Area::new(handle_id)
            .order(egui::Order::Background)
            .fixed_pos(handle.min)
            .default_size(handle.size())
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                let response = ui.interact(
                    handle,
                    handle_id.with("grip"),
                    egui::Sense::click_and_drag(),
                );
                crate::drag::claim_if_held(&response);
                if response.double_clicked() {
                    Some(DividerAction::Even)
                } else if response.drag_stopped() {
                    Some(DividerAction::Release)
                } else if response.dragged() || response.is_pointer_button_down_on() {
                    response.interact_pointer_pos().map(DividerAction::Drag)
                } else {
                    None
                }
            })
            .inner;
        if let Some(action) = action {
            self.apply_divider(ctx, action, content, axis);
        }
    }

    /// A divider drag lives only while its handle is drawn and a finger is down. The handle stops
    /// being drawn while anything else moves (a push in either pane, home, the overview — any of
    /// them can arrive from another thread mid-drag), and then it never hears the finger lift:
    /// left alone, the divider would stay "under a finger" for good — a pane squeezed past its
    /// minimum and dimmed, the panes never told `Resized`. So the drag is let go here, where it
    /// stands, as a lift would have.
    fn end_stray_divider_drag(&mut self, ctx: &egui::Context, content: Rect) {
        if !self.split.as_ref().is_some_and(Split::is_dragging) {
            return;
        }
        let held = ctx.input(|i| i.pointer.any_down()) && self.divider_handle.is_positive();
        if held {
            return;
        }
        let axis = self.axis_for(content);
        self.apply_divider(ctx, DividerAction::LetGo, content, axis);
    }

    /// Act on what the divider's handle was asked.
    fn apply_divider(
        &mut self,
        ctx: &egui::Context,
        action: DividerAction,
        content: Rect,
        axis: SplitAxis,
    ) {
        let len = axis.along(content.size()).max(1.0);
        let mins = self.pane_mins(content, axis);
        let bounds = split::bounds(content, axis, self.split_metrics.0, mins);
        let tokens = self.tokens;
        match action {
            DividerAction::Drag(at) => {
                let ratio = (axis.coord(at) - axis.coord(content.min)) / len;
                let velocity = ctx.input(|i| axis.along(i.pointer.velocity())) / len;
                if let Some(split) = self.split.as_mut() {
                    split.drag(ratio, velocity);
                }
            }
            DividerAction::Release | DividerAction::LetGo => {
                let dismiss = matches!(action, DividerAction::Release);
                let released = self
                    .split
                    .as_mut()
                    .map(|split| split.release(bounds, tokens.spring, tokens.reduce, len, dismiss));
                if let Some(Released::Close(pane)) = released {
                    self.unsplit(1 - pane, &tokens);
                }
            }
            DividerAction::Even => {
                if let Some(split) = self.split.as_mut() {
                    split.even(bounds, &tokens);
                }
            }
        }
    }

    /// Frame stage 14: lifecycle propagation. It drains every instance's queue (the one leaving
    /// through a transition included) into `on_lifecycle` and drops any instance that received a
    /// `Destroyed`. Each instance's `Cx` sees the rect of the pane it is in.
    pub(crate) fn flush_lifecycle(
        &mut self,
        parts: &mut CxParts<'_>,
        registry: &mut crate::screen::Registry,
    ) {
        let rect = self.last_content;
        let inset = self.inset_bottom;
        let split = self.split.is_some();
        // Where each task's screens are: the pane showing it, or the whole content.
        let mut placed: Vec<(usize, Rect)> = Vec::new();
        if split {
            for (pane, p) in self.panes.iter().enumerate() {
                if let (Some(task), Some(laid)) = (p.task, self.pane_laid.get(pane)) {
                    if laid.is_positive() {
                        placed.push((task, *laid));
                    }
                }
            }
        }
        let metrics = parts.theme.metrics;
        for (index, task) in self.tasks.iter_mut().enumerate() {
            let outer = placed
                .iter()
                .find(|(t, _)| *t == index)
                .map_or(rect, |(_, r)| *r);
            let shown = placed.iter().any(|(t, _)| *t == index);
            for instance in task.iter_mut() {
                let inner = screen_rect(outer, &instance.chrome(), &metrics);
                let pane = pane_info(outer, inner, instance.id(), false, inset, split && shown);
                let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
                instance.flush_lifecycle(&mut cx);
            }
        }
        let in_transit = self
            .closing_task
            .iter_mut()
            .chain(self.leaving_task.iter_mut())
            .flat_map(Task::iter_mut)
            .chain(self.clearing.iter_mut());
        for instance in in_transit {
            let inner = screen_rect(rect, &instance.chrome(), &metrics);
            let pane = pane_info(rect, inner, instance.id(), false, inset, false);
            let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
            instance.flush_lifecycle(&mut cx);
        }
        if let StackTransition::Popping {
            outgoing: Some(outgoing),
            ..
        }
        | StackTransition::DraggingBack {
            outgoing: Some(outgoing),
            ..
        } = &mut self.stack
        {
            let inner = screen_rect(rect, &outgoing.chrome(), &metrics);
            let pane = pane_info(rect, inner, outgoing.id(), false, inset, false);
            let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
            outgoing.flush_lifecycle(&mut cx);
        }
        for mut instance in self.graveyard.drain(..) {
            let inner = screen_rect(rect, &instance.chrome(), &metrics);
            let pane = pane_info(rect, inner, instance.id(), false, inset, false);
            let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
            instance.flush_lifecycle(&mut cx);
            parts
                .shell
                .waker()
                .context()
                .set_transform_layer(instance.layer_id(), TSTransform::IDENTITY);
        }
    }

    /// The content Rect last drawn.
    #[doc(hidden)]
    #[must_use]
    pub fn last_content(&self) -> Rect {
        self.last_content
    }
}

/// This frame's A2 render values — the card path ([`transition::A2Mapping`]) and the fallback
/// ([`transition::A2Fallback`]) in one shape.
#[derive(Debug, Clone, Copy)]
struct HomeFrame {
    desktop_scale: f32,
    desktop_opacity: f32,
    screen_opacity: f32,
    screen_scale: f32,
    /// What the screen scales about — `None` is its pane's centre (the A2 fallback); the overview
    /// moves it so the screen lands on its card (A10).
    screen_pivot: Option<egui::Pos2>,
    /// How far the screen is moved after scaling — the lift's finger, and a lifted
    /// screen carried on into home or into its card.
    screen_shift: egui::Vec2,
    card: Option<CardFrame>,
}

/// One frame of the placeholder card.
#[derive(Debug, Clone, Copy)]
struct CardFrame {
    rect: Rect,
    radius: f32,
    icon_size: f32,
    icon_opacity: f32,
}

impl HomeFrame {
    const IDLE: Self = Self {
        desktop_scale: 1.0,
        desktop_opacity: 1.0,
        screen_opacity: 1.0,
        screen_scale: 1.0,
        screen_pivot: None,
        screen_shift: egui::Vec2::ZERO,
        card: None,
    };

    /// Draw the real screen inside `card` rather than over its pane: scaled to fit it, and
    /// centred on it.
    fn fit_screen(&mut self, card: Rect, pane: Rect) {
        let k = (card.width() / pane.width().max(1.0))
            .min(card.height() / pane.height().max(1.0))
            .max(0.0);
        self.screen_scale = k;
        self.screen_pivot = Some(pane.center());
        self.screen_shift = card.center() - pane.center();
    }

    fn card(m: &transition::A2Mapping) -> Self {
        Self {
            desktop_scale: m.desktop_scale,
            desktop_opacity: m.desktop_opacity,
            screen_opacity: m.screen_opacity,
            screen_scale: 1.0,
            screen_pivot: None,
            screen_shift: egui::Vec2::ZERO,
            card: Some(CardFrame {
                rect: m.card_rect,
                radius: m.card_radius,
                icon_size: m.icon_size,
                icon_opacity: m.icon_opacity,
            }),
        }
    }

    fn fallback(m: &transition::A2Fallback) -> Self {
        Self {
            desktop_scale: m.desktop_scale,
            desktop_opacity: m.desktop_opacity,
            screen_opacity: m.screen_opacity,
            screen_scale: m.screen_scale,
            screen_pivot: None,
            screen_shift: egui::Vec2::ZERO,
            card: None,
        }
    }
}

/// [`draw_instance`]'s arguments — the layer transform, the fade, the dim, the shadow and the focus.
#[derive(Debug, Clone, Copy)]
struct DrawParams {
    /// The layer's x offset (A3).
    x: f32,
    /// The screen's opacity. At 0 the screen's `ui` is not called (the card alone).
    opacity: f32,
    /// The layer's scale (the A2 fallback, pivoted at the centre; A10, at `pivot`).
    scale: f32,
    /// What `scale` is about, where not the centre.
    pivot: Option<egui::Pos2>,
    /// How far the layer is moved after scaling, on top of `x` (the lift).
    shift: egui::Vec2,
    /// The dim alpha laid over it.
    dim: f32,
    /// The strength of the shadow band on the left.
    shadow: f32,
    /// Whether it takes input (`false` mid-transition = input blocked).
    focused: bool,
    /// This layer is this frame's topmost — `move_to_top`. Exactly one may be `true` per frame
    /// (the z-order convention in the module's head comment).
    raise: bool,
    /// The OSK inset (`PaneInfo.inset_bottom`).
    inset_bottom: f32,
    /// Where the screen is cut, on the glass — its pane's visible rect (the content's, while an A2
    /// card flies). Taken back through the layer's transform, so a slide moves the screen and not
    /// the edge it is cut at.
    clip: Rect,
    /// Which pane it is in (`PaneInfo.is_split`, and `is_focused` with `focused`).
    seat: Seat,
}

/// Which pane a screen is drawn in, as its `PaneInfo` tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seat {
    /// The only pane.
    Alone,
    /// The focused one of two.
    Focused,
    /// The other one of two.
    Beside,
}

impl Seat {
    /// The focused pane's seat: alone, or focused of two where `split`.
    fn of(split: bool) -> Self {
        if split {
            Self::Focused
        } else {
            Self::Alone
        }
    }

    fn is_split(self) -> bool {
        self != Self::Alone
    }

    fn has_focus(self) -> bool {
        self != Self::Beside
    }
}

/// What every screen drawn in the focused pane shares this frame.
#[derive(Debug, Clone, Copy)]
struct PaneLook {
    clip: Rect,
    split: bool,
    inset: f32,
}

/// This frame's pane rects — one pane, or the split's two and its divider.
#[derive(Debug, Clone, Copy)]
struct PaneFrame {
    /// Each pane's visible rect (`Rect::NOTHING` for a second pane that is not there).
    visible: [Rect; 2],
    /// The rect each pane's screen is laid out in.
    laid: [Rect; 2],
    /// How far past its minimum each pane is squeezed.
    squeeze: [f32; 2],
    /// The divider's band and alpha, while split.
    divider: Option<(Rect, f32)>,
    /// The pane not focused, while split.
    other: Option<usize>,
}

impl PaneFrame {
    fn single(content: Rect) -> Self {
        Self {
            visible: [content, Rect::NOTHING],
            laid: [content, Rect::NOTHING],
            squeeze: [0.0; 2],
            divider: None,
            other: None,
        }
    }
}

/// Why the split control cannot bring a second pane up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SplitBlocker {
    /// Home, or nothing on show to put a screen beside.
    NothingOnShow,
    /// The screen on show does not share the content (`allow_split = false`, `SplitSupport::No`,
    /// or a minimum that leaves no room).
    Refused,
    /// No other task's screen fits beside it.
    NothingFits,
}

/// What the divider's handle was asked this frame.
#[derive(Debug, Clone, Copy)]
enum DividerAction {
    /// Under a finger, at this point.
    Drag(Pos2),
    /// Let go.
    Release,
    /// A drag cut short — the handle went before the lift was heard: back inside the minimums,
    /// never closing a pane nobody chose to close.
    LetGo,
    /// Tapped twice.
    Even,
}

/// How the divider looks this frame.
#[derive(Debug, Clone, Copy)]
struct DividerLook {
    band: Rect,
    alpha: f32,
    axis: SplitAxis,
    /// Under a finger: the grip takes the primary colour.
    dragging: bool,
    /// The focused pane's rect, to outline — at rest only.
    focus: Option<Rect>,
}

/// The divider's band and grip and the focused pane's outline, in a click-through `Area` over
/// the panes.
fn paint_divider(
    ctx: &egui::Context,
    theme: &crate::theme::Theme,
    content: Rect,
    look: DividerLook,
) {
    let chrome = egui::Id::new(DIVIDER_ID);
    ctx.move_to_top(egui::LayerId::new(egui::Order::Background, chrome));
    let DividerLook {
        band,
        alpha,
        axis,
        dragging,
        focus,
    } = look;
    egui::Area::new(chrome)
        .order(egui::Order::Background)
        .fixed_pos(content.min)
        .default_size(content.size())
        .constrain(false)
        .fade_in(false)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(content);
            let painter = ui.painter();
            painter.rect_filled(
                band,
                0.0,
                theme.color(ColorRole::Background).gamma_multiply(alpha),
            );
            // The grip: half the band thick, an icon long.
            let thick = (axis.along(band.size()) / 2.0).max(2.0);
            let long = theme.control.icon;
            let grip = Rect::from_center_size(
                band.center(),
                match axis {
                    SplitAxis::SideBySide => egui::vec2(thick, long),
                    SplitAxis::Stacked => egui::vec2(long, thick),
                },
            );
            let ink = if dragging {
                theme.color(ColorRole::Primary)
            } else {
                theme.color(ColorRole::ControlEdge)
            };
            painter.rect_filled(grip, thick / 2.0, ink.gamma_multiply(alpha));
            if let Some(focus) = focus.filter(Rect::is_positive) {
                painter.rect_stroke(
                    focus,
                    0.0,
                    egui::Stroke::new(theme.control.stroke_mark, theme.color(ColorRole::Focus)),
                    egui::StrokeKind::Inside,
                );
            }
        });
}

/// A task's key: its root instance (stable while its index in `tasks` is not).
fn root_key(task: &Task) -> Option<InstanceId> {
    task.iter().next().map(Instance::id)
}

/// The item of a pair at `index` (the second for anything past it).
fn pane_of<T: Copy>(pair: [T; 2], index: usize) -> T {
    let [first, second] = pair;
    if index == 0 {
        first
    } else {
        second
    }
}

/// The Rect a screen actually receives. The shell shrinks the content Rect by the inset
/// before handing it over — [`ChromePolicy::inset`]'s value where there is one, otherwise
/// [`crate::theme::Metrics::screen_inset`] (12 by default). The Pane background, the A2 card and the
/// A3 dim use the Rect **before** the inset as it stands. Where a Pane is narrower than
/// twice the inset, it is pinched so as not to invert.
fn screen_rect(content: Rect, chrome: &ChromePolicy, metrics: &crate::theme::Metrics) -> Rect {
    let inset = chrome
        .inset_or(metrics.screen_inset)
        .min(content.width() / 2.0 - 1.0)
        .min(content.height() / 2.0 - 1.0)
        .max(0.0);
    if inset <= 0.0 {
        return content;
    }
    content.shrink(inset)
}

fn pane_info(
    outer: Rect,
    rect: Rect,
    instance: InstanceId,
    focused: bool,
    inset_bottom: f32,
    split: bool,
) -> PaneInfo {
    PaneInfo {
        rect,
        outer,
        is_split: split,
        is_focused: focused,
        inset_bottom,
        instance,
    }
}

/// Whether the layer to `move_to_top` this frame is **the one going out**.
///
/// `Areas::end_pass` sorts by `sort_by_key(|l| (l.order, wants_to_be_on_top.contains(l)))` — **a
/// stable sort**, so raising several layers on one frame leaves their existing relative order in
/// place and z gets dragged along by "last frame's history" (the defect where slot 0, raised during
/// an A2, stayed above slot 1 through a pop as well). So [`Workspace::draw_panes`] flags **only
/// one** per frame:
///
/// | State | The layer that comes on top |
/// |---|---|
/// | `Pushing` | The one coming in (the top instance) |
/// | `Popping` | **The one going out** (A3's iOS-style pop) |
/// | A2 (opening/closing) | The top — above a desktop that has come back into view |
/// | `Idle` | The top |
fn raises_outgoing(stack: &StackTransition) -> bool {
    matches!(
        stack,
        StackTransition::Popping {
            outgoing: Some(_),
            ..
        } | StackTransition::DraggingBack {
            outgoing: Some(_),
            ..
        }
    )
}

/// The screen Rect a [`Layout`] was built from — the union of the four edge zones (`edge_zones` is
/// cut out of that Rect, so it is recovered exactly). The tween shield has to use **the same Rect**
/// as the gesture shield, so that egui does not warn about an id clash when both are up on the same
/// frame.
fn screen_of(layout: &Layout) -> Rect {
    layout
        .edge_zones
        .iter()
        .fold(Rect::NOTHING, |acc, zone| acc.union(*zone))
}

/// This frame's A3 mapping. Push and pop ease `t`; the back gesture takes `p` as it stands (A3).
fn a3_mapping(stack: &StackTransition, width: f32, tokens: &MotionTokens) -> transition::A3Mapping {
    match stack {
        StackTransition::Idle => transition::A3Mapping::IDLE,
        StackTransition::Pushing { t } => {
            transition::a3_push(tokens.push.easing.apply(t.value()), width, tokens)
        }
        StackTransition::Popping { t, .. } => {
            transition::a3_pop(tokens.pop.easing.apply(t.value()), width, tokens)
        }
        // The back gesture: s = p, with no easing.
        StackTransition::DraggingBack { p, .. } => transition::a3_pop(p.value(), width, tokens),
    }
}

/// The outgoing instance, off the stack, during a pop or a confirmed gesture — to the right, by the incoming mapping (`x_in`).
fn draw_outgoing(
    ctx: &egui::Context,
    draw: &mut Draw<'_, '_>,
    stack: &mut StackTransition,
    content: Rect,
    a3: &transition::A3Mapping,
    home: &HomeFrame,
    look: &PaneLook,
) {
    if let StackTransition::Popping {
        outgoing: Some(outgoing),
        ..
    }
    | StackTransition::DraggingBack {
        outgoing: Some(outgoing),
        ..
    } = stack
    {
        let params = DrawParams {
            inset_bottom: look.inset,
            x: a3.x_in,
            opacity: home.screen_opacity,
            scale: home.screen_scale,
            pivot: home.screen_pivot,
            shift: home.screen_shift,
            dim: 0.0,
            shadow: a3.shadow,
            focused: false,
            raise: true,
            clip: look.clip,
            seat: Seat::of(look.split),
        };
        draw_instance(ctx, draw, outgoing, content, &params, None, None);
    }
}

/// The top screen being cleared by the clear-top cross-fade. It is drawn in place over the
/// screen below with only its alpha going `1 → 0` — the ones being cleared beneath it are covered
/// anyway and are not drawn.
fn draw_clearing(
    ctx: &egui::Context,
    draw: &mut Draw<'_, '_>,
    clearing: &mut [Instance],
    content: Rect,
    alpha: Option<f32>,
    look: &PaneLook,
) {
    let (Some(alpha), Some(top)) = (alpha, clearing.first_mut()) else {
        return;
    };
    let params = DrawParams {
        inset_bottom: look.inset,
        x: 0.0,
        opacity: alpha,
        scale: 1.0,
        pivot: None,
        shift: egui::Vec2::ZERO,
        dim: 0.0,
        shadow: 0.0,
        focused: false,
        raise: true,
        clip: look.clip,
        seat: Seat::of(look.split),
    };
    draw_instance(ctx, draw, top, content, &params, None, None);
}

/// What a draw helper needs to reach a screen: the [`CxParts`] a `Cx` is made from, and the
/// registry a **resident** screen is borrowed from at call time. They travel together
/// through every helper below, so they are one argument rather than two.
struct Draw<'a, 'p> {
    parts: &'a mut CxParts<'p>,
    registry: &'a mut crate::screen::Registry,
}

/// A uniform scale about a centre pivot: `p' = c + s (p − c)`.
#[must_use]
pub(crate) fn scale_about(center: egui::Pos2, scale: f32) -> TSTransform {
    if (scale - 1.0).abs() < 1e-5 {
        return TSTransform::IDENTITY;
    }
    TSTransform::new(center.to_vec2() * (1.0 - scale), scale)
}

/// An alpha of `0..=1` → `u8`. The loss is the quantisation intended, so the lint is lifted per item.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn alpha_u8(alpha: f32) -> u8 {
    (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The card icon's size quantisation step (px). The icon polyline cache's key is an integer size,
/// so drawing 48 → 96 in pixel steps would make 49 entries on the first transition — grouping them
/// into 4 px steps (13 entries) keeps the cache bounded.
const CARD_ICON_STEP: f32 = 4.0;

/// Draw one screen instance into its own slot `Area`. Where there is a `card`, the card goes first
/// (opaque), and where `params.opacity > 0` the real screen goes over it with `set_opacity`. With
/// neither, the `Area` is not opened at all (the first-`ui` contract). `card_icon` is the task
/// root's icon (or the instance's own, where there is none).
fn draw_instance(
    ctx: &egui::Context,
    draw: &mut Draw<'_, '_>,
    instance: &mut Instance,
    content: Rect,
    params: &DrawParams,
    card: Option<&CardFrame>,
    card_icon: Option<&IconRef>,
) {
    let show_screen = params.opacity > 0.0;
    if !show_screen && card.is_none() {
        return;
    }
    let layer = instance.layer_id();
    let shift = egui::vec2(params.x, 0.0) + params.shift;
    let transform = if (params.scale - 1.0).abs() >= 1e-5 {
        TSTransform::from_translation(shift)
            * scale_about(params.pivot.unwrap_or(content.center()), params.scale)
    } else if shift.length() < 0.01 {
        TSTransform::IDENTITY
    } else {
        TSTransform::from_translation(shift)
    };
    ctx.set_transform_layer(layer, transform);
    if params.raise {
        ctx.move_to_top(layer);
    }
    let event = if show_screen {
        instance.next_closure_event()
    } else {
        None
    };
    // The Rect the screen receives is the inset one. The background, the card and the dim take `content` as it stands.
    let inner = screen_rect(content, &instance.chrome(), &draw.parts.theme.metrics);
    let pane = pane_info(
        content,
        inner,
        instance.id(),
        params.focused && params.seat.has_focus(),
        params.inset_bottom,
        params.seat.is_split(),
    );
    // The pane's edge on the glass, in this layer's own space.
    let clip = transform.inverse() * params.clip;
    let background = instance
        .background()
        .map_or(draw.parts.theme.palette.surface, |role| {
            draw.parts.theme.color(role)
        });
    // `fade_in(false)`: egui fades a newly visible `Area` in automatically over `animation_time` (80 ms)
    // and requests a repaint — A2/A3's opacity is settled by the mapping, so it is turned off.
    egui::Area::new(instance.area_id())
        .order(egui::Order::Background)
        .fixed_pos(content.min)
        .default_size(content.size())
        .constrain(false)
        .fade_in(false)
        .interactable(params.focused)
        .show(ctx, |ui| {
            ui.set_clip_rect(clip);
            ui.set_min_size(content.size());
            if let Some(card) = card {
                // The card is placed on the glass, and the layer may be scaled (a lifted screen
                // carried on into home): it is drawn through the layer's inverse.
                let k = transform.scaling.max(1e-3);
                let rect = transform.inverse() * card.rect;
                let painter = ui.painter();
                painter.rect_filled(rect, card.radius / k, background);
                if card.icon_opacity > 0.0 {
                    if let Some(icon) = card_icon.or_else(|| instance.icon()) {
                        let size = (card.icon_size / CARD_ICON_STEP).round() * CARD_ICON_STEP / k;
                        let icon_rect =
                            Rect::from_center_size(rect.center(), egui::Vec2::splat(size));
                        let mut faded = painter.clone();
                        faded.set_opacity(card.icon_opacity);
                        let style = IconStyle::sized(size);
                        draw.parts
                            .icons
                            .paint(&faded, icon_rect, icon, &style, draw.parts.theme);
                    }
                }
            }
            if show_screen {
                // The background (the whole Pane) and the dim are drawn to the Rect **before** the inset
                // and the screen to the inset child. The background and the dim clone the painter
                // rather than making another `Ui` — `Ui::new_child` allocates a `UiStack`, so the only
                // child is the screen's, keeping the per-frame allocations from growing.
                let mut backdrop = ui.painter().clone();
                backdrop.set_opacity(params.opacity);
                backdrop.rect_filled(content, 0.0, background);
                // The screen's widgets are known by the instance, not by the layer slot it is
                // lent this frame: a screen drawn on another slot — sliding in on the second
                // pane's during a quick switch — keeps its scroll and focus, and two
                // tasks' roots, both lent slot 0 in their turn, no longer share them.
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .id(egui::Id::new(("fairing.instance", instance.id()))),
                );
                child.set_clip_rect(inner.intersect(clip));
                if params.opacity < 1.0 {
                    child.set_opacity(params.opacity);
                }
                let mut cx = draw.parts.cx_in(pane, event, Some(&mut *draw.registry));
                instance.ui(&mut child, &mut cx);
                if params.dim > 0.0 {
                    backdrop.rect_filled(
                        content,
                        0.0,
                        Color32::from_black_alpha(alpha_u8(params.dim)),
                    );
                }
            }
            if params.shadow > 0.0 {
                paint_shadow_band(ctx, layer, content, params.shadow, clip);
            }
        });
}

/// The width of one step of the shadow band (px) = 8 px / 4 steps.
const SHADOW_STEP_PX: f32 = transition::SHADOW_BAND_PX / 4.0;

/// The A3 shadow band: four 2 px steps in the 8 px outside the layer's left edge — in layer
/// coordinates, so it moves with the transform. Being outside the `Ui`'s clip (the content), a
/// separate `Painter` clipped to the band itself is made for it.
fn paint_shadow_band(
    ctx: &egui::Context,
    layer: egui::LayerId,
    content: Rect,
    strength: f32,
    clip: Rect,
) {
    let band = Rect::from_min_max(
        egui::pos2(content.min.x - transition::SHADOW_BAND_PX, content.min.y),
        egui::pos2(content.min.x, content.max.y),
    );
    // Cut at the pane's edge as well: in a split the band must not fall on the other pane.
    let painter = egui::Painter::new(ctx.clone(), layer, band.intersect(clip));
    let mut x0 = band.min.x;
    for alpha in transition::SHADOW_BAND_ALPHA {
        let rect = Rect::from_min_max(
            egui::pos2(x0, band.min.y),
            egui::pos2(x0 + SHADOW_STEP_PX, band.max.y),
        );
        painter.rect_filled(
            rect,
            0.0,
            Color32::from_black_alpha(alpha_u8(alpha * strength)),
        );
        x0 += SHADOW_STEP_PX;
    }
}

#[cfg(test)]
mod tests {
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
}
