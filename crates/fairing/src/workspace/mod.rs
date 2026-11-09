//! The workspace — either the Home view or its panes, each showing a task stack. One
//! pane, or two side by side or one above the other while split (A8 — the split's
//! geometry and motion are in `split.rs`, the recent screens in `overview.rs`).
//!
//! The render convention: the desktop and the screen instances are each drawn as an `Area`
//! (`Order::Background`) on a different `LayerId`, and through a transition the layer is moved
//! whole with `Context::set_transform_layer` and faded with `Ui::set_opacity`.
//! The workspace supplies these transforms around the screen callback. It reuses
//! layer IDs by pane/stack slot rather than issuing one per instance. The number used grows
//! with stack depth; pane slot ranges are disjoint only below the `PANE_SLOTS` depth boundary.
//! Transform and opacity handling use egui's
//! [Context](https://docs.rs/egui/0.37.0/egui/struct.Context.html) and
//! [Ui](https://docs.rs/egui/0.37.0/egui/struct.Ui.html) APIs.
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
//!   `interactable(false)`, using egui's area interaction control. A transparent shield `Area` over the
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
mod gestures;
mod lifecycle;
mod overview_flow;
mod render;
mod split_flow;

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

/// Each pane's layer range starts at `i × PANE_SLOTS`. Slots are distinct between panes
/// while stack depths stay below this range width; deeper stacks can overlap the other pane's range.
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

    /// The task used last of those that can go beside the focused screen now — what the split
    /// control puts in the other pane when the shell's overview is off.
    #[must_use]
    pub fn most_recent_other_task(&self) -> Option<usize> {
        self.beside_candidates().first().copied()
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
    /// The icon's size as a multiple of `metrics.icon_size` (`transition::A2Mapping::icon_grow`).
    icon_grow: f32,
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
                icon_grow: m.icon_grow,
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
    let chrome = egui::Id::unique(DIVIDER_ID);
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
                        let icon_size = draw.parts.theme.metrics.icon_size * card.icon_grow;
                        let size = (icon_size / CARD_ICON_STEP).round() * CARD_ICON_STEP / k;
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
                        .scope_id(egui::Id::unique(("fairing.instance", instance.id()))),
                );
                child.set_clip_rect(inner.intersect(clip));
                if params.opacity < 1.0 {
                    child.set_opacity(params.opacity);
                }
                let mut cx = draw.parts.cx_in(pane, event, Some(&mut *draw.registry));
                instance.ui(&mut child, &mut cx);
                // The dim and the band take the palette's `Scrim` hue with their own alpha, as
                // the desktop's shadows do — a plain black on a warm or tinted palette is a hole.
                let scrim = draw.parts.theme.color(ColorRole::Scrim);
                if params.dim > 0.0 {
                    backdrop.rect_filled(content, 0.0, scrim_alpha(scrim, params.dim));
                }
            }
            if params.shadow > 0.0 {
                let scrim = draw.parts.theme.color(ColorRole::Scrim);
                paint_shadow_band(ctx, layer, content, params.shadow, scrim, clip);
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
    scrim: Color32,
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
        painter.rect_filled(rect, 0.0, scrim_alpha(scrim, alpha * strength));
        x0 += SHADOW_STEP_PX;
    }
}

/// The scrim's hue at `alpha` (0..=1) — the palette's shade, not a black.
fn scrim_alpha(scrim: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(scrim.r(), scrim.g(), scrim.b(), alpha_u8(alpha))
}

#[cfg(test)]
mod tests;
