//! `Shell` — the top-level object. One [`Shell::frame`] per frame draws everything.
//!
//! The frame order is a fixed contract.
//! [`Shell::logic`] / [`Shell::logic_with`] can run state stages 1–4 plus eviction and
//! lifecycle requests without drawing. The shell/state runners use them in `App::logic`;
//! a following UI pass reuses that update and records only its fresh input activity.
//! Background calls use wall time and schedule state deadlines, leaving visual animation
//! and gesture processing to UI passes.
//!
//! ```text
//! Shell::frame(ui)
//!  1. now / dt — now on the real time between frames, dt = stable_dt capped at 50 ms;
//!               revoke expired access and enforce overdue idle locks before commands or input
//!               (changed declarations are rebuilt before queued icon updates)
//!  2. handle.drain()        — the command queue, through mpsc try_recv (+ M2 notify/toast/dismiss/toggle_overlay)
//!  3. services.poll()       — pin snapshots, reconcile pending Bool targets, gather next_wake
//!  4. access                — revalidate policy, record input, poll the authenticator
//!  5. osk.update()          — current keyboard policy, bounds and reserved band (A5)
//!     gestures.update()     — edge swipes and long presses (M2)
//!     guard.update()        — the edge_guard mask · the emergency gesture (chrome.emergency) · keep_awake
//!     overlay.update()      — consume the top pull (priority: the shade > the back gesture)
//!     gesture back          — the left edge → Workspace::*_gesture_back (A3)
//!     animations / workspace / desktop / toasts / heads_up / theme_fade tick
//!     evict · registry dirty → desktop.rebuild + rebuild the overlay tiles
//!     the overlay's Closed boundary → Paused/Resumed on the focused screen
//!  6. layout = compute_layout(the screen, the focused screen's chrome policy, the OSK inset)
//!  7. status_bar   Panel::top     (the Background layer; opacity 1 − y/64 while the shade is pulled)
//!  8. nav_bar      Panel::bottom
//!  9. workspace    CentralPanel → the desktop/screen Area (Order::Background, layer transforms)
//! 10. osk.ui()     Area(Order::Middle)      — a key tap injects events (drawn after 12 instead, in
//!                                            Order::Foreground, while the prompt is up)
//! 11. overlay.ui() Area(Order::Foreground)  — the scrim → the panel → the status row/peek (with the status bar hidden) → the shield → the emergency progress ring
//! 12. prompt.ui()  Area(Order::Foreground)  — the unlock prompt or the lock screen, modal
//! 13. heads_up.ui() · toasts.ui()  Area(Order::Tooltip)
//! 14. handle the outputs and requests → launch/back/home, the overlay, heads-up and toast actions, lifecycle propagation
//! 15. repaint      — request_repaint while anything is animating (the overlay, the OSK, a toast, a heads-up, the theme,
//!                    the status bar cross-fade and the emergency ring included), otherwise after, until the next scheduled
//!                    moment (a clock boundary · next_wake · a toast or heads-up expiring · a peek · the OSK debounce ·
//!                    something being held · the toast waiting queue)
//! 16. events       — the integrator takes them with poll_events
//! ```
mod authentication;
mod navigation;
mod render;
mod settings_dispatch;
mod update;

mod back_route;
mod background;
#[cfg(all(test, feature = "mock"))]
mod background_tests;
mod builder;
mod env_scale;
mod event;
mod gesture_regions;
mod handle;
mod input_route;
mod layout;
pub(crate) mod slots;

pub use builder::ShellBuilder;
pub use event::ShellEvent;
pub use handle::{Command, ShellHandle};
#[cfg(test)]
pub(crate) use layout::edge_zones;
pub use layout::Layout;
pub(crate) use layout::{compute_layout, LayoutInput};

use crate::access::prompt::{Prompt, PromptAction, PromptCx, Purpose};
use crate::access::{
    Access, AccessEvent, AuthMethod, AuthOutcome, Authenticator, ChangeReason, Credential, Gate,
    SessionChange,
};
mod bool_ack;

use crate::chrome::bar::{BarItemRow, BarPainter};
use crate::chrome::{
    NavAction, NavBar, NavStyle, PolicyDriver, StatusBar, StatusBarAction, EMERGENCY_GATE,
};

/// The `overlay.open` gate. It is decided every frame, so it is a borrowed constant —
/// `Gate::from(&str)` is always `Owned` and would make a heap allocation per frame.
pub const OVERLAY_OPEN_GATE: Gate = Gate::borrowed("overlay.open");
/// The `nav.recents` gate: bringing the overview up. Going back from it is never gated.
pub const RECENTS_GATE: Gate = Gate::borrowed("nav.recents");

/// A lift's travel as a share of its pane's height (A11's `travel_ratio`).
const LIFT_TRAVEL_RATIO: f32 = 0.35;
/// A lift's travel, at least (mm).
const LIFT_TRAVEL_MIN_MM: f32 = 15.0;
/// A lift's travel, at most (mm).
const LIFT_TRAVEL_MAX_MM: f32 = 60.0;
/// A lift's travel, at most, as a share of the screen's short side.
const LIFT_TRAVEL_SHORT_SIDE: f32 = 0.6;
/// How far up a lift has to be for a pause to bring the recent screens up — lower
/// down, a pause is a hand making up its mind.
const LIFT_HOLD_MIN: f32 = 0.25;
/// The `workspace.split` gate: **entering** a split — the split control, a card's split
/// button, `cx.open_in_other_pane`. Back to one pane is never gated.
pub const SPLIT_GATE: Gate = Gate::borrowed("workspace.split");

/// What the shell says itself — looked up through `Strings` like the shade's labels, so the
/// active language's table reaches them.
mod labels {
    /// The split control at home: nothing on show to put a screen beside.
    pub(super) const SPLIT_NOTHING_ON_SHOW: &str = "Open a screen to split it with another";
    /// The split control over a screen that keeps the content to itself.
    pub(super) const SPLIT_REFUSED: &str = "This screen can't be shown in split view";
    /// The split control where no other screen fits beside the one on show.
    pub(super) const SPLIT_NOTHING_FITS: &str = "No other screen fits beside this one";
    /// The lock screen's date: `{y}` is the year, `{mm}` and `{dd}` the month and day in two
    /// digits, `{m}` and `{d}` the same without the leading zero.
    pub(super) const DATE: &str = "{y}-{mm}-{dd}";
}
use crate::config::{RepaintMode, ShellConfig};
use crate::desktop::{Badge, DesktopAction, DesktopView};
use crate::error::Result;
use crate::gesture::{Edge, EdgeMask, Gesture, GestureEngine, GestureFrame, Phase};
use crate::i18n::Strings;
use crate::icons::{CustomIconId, IconDef, IconPainter, IconSet};
use crate::motion::{AnimationStore, ReleaseRule, MAX_DT};
use crate::notify::{
    HeadsUp, HeadsUpAction, Notification, NotificationCenter, NotificationId, ToastAction,
    ToastInput, ToastQueue,
};
use crate::screen::{
    ActionDecl, BackAction, BarMode, CxParts, CxRequest, Decl, DeclKind, LaunchAction, LaunchMode,
    Lifecycle, PaneInfo, Registry, ScreenDecl,
};
use crate::services::Services;
use crate::settings::{keys, SettingKey, SettingValue, SettingsView};
use crate::theme::{Theme, ThemeFade};
use crate::workspace::{Instance, InstanceId, OverviewMode, SplitBlocker, Task, Workspace};
use background::LogicState;
use slots::{OskSlot, OverlaySlot, OverlaySlotAction};
use std::time::{Duration, Instant};

/// For this long after a touch is released, pointer movement is not counted as a mouse — because a
/// touch emits pointer events along with it (`CursorPolicy::Auto`).
const TOUCH_GRACE: Duration = Duration::from_millis(400);

/// The shell.
#[allow(clippy::struct_excessive_bools)] // Independent switches, not a state machine — one flag each.
pub struct Shell {
    faulted: bool,
    boundary_active: bool,
    recovery_ui: Option<crate::recovery::RecoveryUi>,
    restart_requested: bool,
    config: ShellConfig,
    theme: Theme,
    services: Services,
    access: Access,
    registry: Registry,
    workspace: Workspace,
    desktop: DesktopView,
    status_bar: StatusBar,
    nav_bar: NavBar,
    animations: AnimationStore,
    icons: IconSet,
    settings: SettingsView,
    /// Accepted Bool requests not yet reflected in a backend snapshot.
    pending_bools: std::collections::BTreeMap<SettingKey, PendingBool>,
    strings: Strings,
    /// The locale whose letters were last looked for in the loaded fonts.
    script_checked: Option<String>,
    handle: ShellHandle,
    rx: crate::inbox::BoundedInbox<Command>,
    /// Maximum commands received in one UI or logic update.
    commands_per_update: usize,
    events: Vec<ShellEvent>,
    requests: Vec<CxRequest>,
    layout: Layout,
    frame_no: u64,
    last_dt: f32,
    /// Monotonic time: input deltas for a frame-only host, wall deltas once the host
    /// uses `logic`. Headless frame-only tests retain their virtual `RawInput::time`.
    mono: Instant,
    /// Last wall-clock observation, also refreshed by input-driven frames before opting in.
    clock_wall: Instant,
    /// Present once the host uses `App::logic`; UI passes then share its wall clock.
    logic_state: Option<LogicState>,
    /// How many frames `next_wake` has pointed into the past in a row (for diagnosing a backend fault).
    stale_wake_frames: u64,
    /// How many passes before the first frame were skipped waiting for the font families.
    font_waits: u32,
    /// The integrator painter drawing the whole status bar. With one, [`StatusBar::ui`] is never called.
    status_bar_painter: Option<BarPainter>,
    /// The integrator painter drawing the whole nav bar.
    nav_bar_painter: Option<BarPainter>,
    /// The integrator's placement of the nav bar's items (rung 4). Never set alongside a
    /// painter — `build` drops it there.
    nav_bar_layout: Option<crate::chrome::NavLayout>,
    /// The integrator's painters for the recent screens' cards and ground (rung 5), lent
    /// to the workspace each frame.
    recents_painters: crate::workspace::RecentsPainters,
    /// The integrator's widget painters (rung 5), lent to every widget the shell and its
    /// screens draw.
    widget_painters: fairing_widgets::widgets::WidgetPainters,
    /// The integrator's gesture handle painter.
    handle_painter: Option<crate::gesture::HandlePainter>,
    /// The buffer for the item list handed to a painter (only the values are overwritten each frame).
    bar_rows: Vec<BarItemRow>,
    /// The physical scale policy. How the density is used — separate from "what is known" (`DisplayInfo`).
    scale_policy: crate::unit::ScalePolicy,
    /// The physical-unit metrics spec. `theme.metrics` is the result of resolving this
    /// against `scale` every frame. `None` means the integrator injected the theme whole, so its
    /// `metrics` is left alone — theme injection is a rung above tokens on the ladder.
    metrics_spec: Option<crate::theme::MetricsSpec>,
    /// The component metrics spec. Resolved every frame by the same rule as
    /// `metrics_spec` — `None` means an injected theme, so `theme.components` is left alone.
    component_spec: Option<crate::theme::ComponentSpec>,
    /// The control vocabulary's lengths. `None` means an injected theme, as above.
    control_spec: Option<crate::theme::ControlSpec>,
    elevation_spec: Option<crate::theme::ElevationSpec>,
    /// The physical size the integrator pinned (mm). Where there is one, it wins over the backend's value.
    physical_mm_pin: Option<(f32, f32)>,
    /// Whether `physical_mm_pin` came from `FAIRING_PHYSICAL_MM` rather than from the integrator.
    env_physical_mm: bool,
    /// `FAIRING_PPI`, already converted to px per mm.
    env_px_per_mm: Option<f32>,
    /// This frame's scale. Settled at frame stage 0 and fixed for the whole of that frame.
    scale: crate::unit::Scale,
    /// The `pixels_per_point` last committed to egui. It is committed again only when it changes —
    /// `set_zoom_factor` throws the font atlas away whole.
    committed_ppp: f32,
    /// The gesture engine (M2).
    gestures: GestureEngine,
    /// The chrome policy driver: `edge_guard` · the emergency gesture · `keep_awake` (M2).
    guard: PolicyDriver,
    /// The shade (feature `overlay`).
    overlay: OverlaySlot,
    /// OSK (feature `osk`).
    osk: OskSlot,
    /// The notification centre (M2).
    notifications: NotificationCenter,
    /// The toast queue (M2).
    toasts: ToastQueue,
    display_status: crate::memory::DisplayStatus,
    /// The heads-up banner (M2).
    heads_up: HeadsUp,
    /// The theme cross-fade (M2, A7).
    theme_fade: ThemeFade,
    /// Set by [`Shell::theme_mut`]; the next frame re-applies the theme to egui's style.
    theme_dirty: bool,
    /// The hidden entry points. Registration order is kept — where several watch the same
    /// corner, the one registered first sees it first.
    hidden: Vec<crate::access::HiddenEntry>,
    /// The snapshot `Cx::knock_remaining` reads. Filled at the start of the frame.
    hidden_remaining: std::collections::BTreeMap<String, u8>,
    /// Where this press began (for the tap decision — paired on the release frame into a [`Tap`]).
    tap_press: Option<(egui::Pos2, f32)>,
    /// The gesture regions — the handles and the integrator's own — in the
    /// order they were added: a later one is above an earlier one.
    gesture_regions: Vec<gesture_regions::RegionSlot>,
    /// This frame's region zones — what the engine was handed, kept for the guards and the
    /// drawing.
    region_zones: crate::gesture::RegionZones,
    region_frame_deferred: bool,
    /// The touch a region is following, from its press to its end.
    region_track: Option<gesture_regions::RegionTrack>,
    /// What a region asked for while it heard its touch (kept for its capacity).
    region_out: crate::gesture::RegionOut,
    /// Last frame's screen Rect. [`Shell::knock`] called outside a frame needs a coordinate system to
    /// give the trigger.
    last_screen: egui::Rect,
    /// The image decoder the integrator wired in. The crate does not decode images.
    image_loader: Option<crate::desktop::ImageLoader>,
    /// The last touch time (`CursorPolicy::Auto`). `None` = no touch yet.
    last_touch: Option<Instant>,
    /// Whether a mouse has been judged to be in use right now.
    mouse_active: bool,
    /// The dark/light palette pair. `set_theme_dark` chooses its target from here.
    /// **It is not what cross-fades** — [`ThemeFade`] interpolates one `Palette` only.
    palettes: (crate::theme::Palette, crate::theme::Palette),
    /// Whether the overlay was `Closed` last frame (the `OverlayToggled` boundary).
    overlay_was_closed: bool,
    /// Whether the focused screen was covered last frame — by the shade or the prompt (the
    /// Paused/Resumed boundary).
    focus_covered: bool,
    /// The unlock prompt and the lock screen.
    prompt: Prompt,
    auth_request: Option<crate::access::AuthRequestId>,
    routed_auth: Option<(Gate, Option<LaunchAction>)>,
    /// What asked for an unlock behind the lock screen — asked again once it is gone.
    after_lock: Option<(Gate, Option<LaunchAction>)>,
    /// Last frame's OSK target (the `OskToggled` boundary).
    osk_was_shown: bool,
    /// Whether two panes were up last frame (the `SplitToggled` boundary).
    split_was: bool,
    /// The bottom swipe under way was held into the recent screens: its release is
    /// not home.
    bottom_held: bool,
    /// The edges that take the back gesture (`[nav_bar] back_edges`).
    back_edges: EdgeMask,
    /// The nav items added since the last frame. The next frame checks them against the nav bar
    /// as it stands then — after the integrator's own setup — and warns about any it will not
    /// draw.
    nav_item_checks: Vec<String>,
}

/// A frame taking longer than this is warned about.
const SLOW_FRAME: Duration = Duration::from_millis(33);

/// Where the toasts stand. On the keyboard while it is up: the content is not pushed, so
/// its bottom alone would put them over the keys. A keyboard that leaves less room than
/// one toast needs — a short panel — has them come down over its top rows instead of being pushed
/// up past the content, under the status bar or off the screen.
fn toast_anchor(layout: &Layout, theme: &Theme) -> egui::Rect {
    let content = layout.content;
    layout.osk.map_or(content, |osk| {
        let floor = content.min.y + ToastQueue::room(theme);
        let bottom = osk.min.y.max(floor).min(content.max.y);
        egui::Rect::from_min_max(content.min, egui::pos2(content.max.x, bottom))
    })
}

/// The `Foreground` Area id shared by a `BarMode::Overlay` status bar, the shade panel's status row
/// and the peek (the layer ids are a fixed, finite set).
pub(crate) const STATUS_BAR_OVERLAY_AREA_ID: &str = "fairing.status_bar.overlay";

/// The most events to hold that [`Shell::poll_events`] has not taken. So that it does not grow
/// without bound when an integrator forgets to poll, the oldest are dropped first (with at least one
/// warning).
const MAX_PENDING_EVENTS: usize = 1024;

/// The floor used when a backend's `next_wake` is already in the past. Handing `Duration::ZERO` over
/// as it stands has egui read it as "repaint immediately" (`outstanding = 1`, two frames) and it
/// quietly becomes 60 fps (egui 0.36.1 `context.rs` `request_repaint_after`).
const MIN_WAKE: Duration = Duration::from_millis(1);

/// After `next_wake` has pointed into the past this many frames in a row, it warns which backend it
/// is. So that the only symptom of 100 % idle CPU is not "it is just slow".
const STALE_WAKE_FRAMES: u64 = 120;

/// How many more times stage 14 handles the requests that `on_lifecycle` handlers made while it
/// was flushing, before leaving the rest to the next frame.
const LIFECYCLE_ROUNDS: usize = 4;

/// One sent target and the latest intent waiting behind it.
#[derive(Clone, Copy)]
struct PendingBool {
    sent: bool,
    desired: bool,
    /// A pre-existing failed connection is not a rejection of this later radio command.
    was_failed: bool,
}

/// Stage 7: draw the status bar into a panel (or, under `BarMode::Overlay`, a `Foreground` Area).
///
/// Where there is an integrator painter, the item list is filled and handed over and the built-in
/// rendering ([`StatusBar::ui`]) is not called. Gathered into one function so that both
/// placement paths take the same branch.
#[allow(clippy::too_many_arguments)] // The two layout paths need the same values.
fn draw_status_bar(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    rect: egui::Rect,
    overlay: bool,
    opacity: f32,
    status_bar: &mut StatusBar,
    parts: &mut CxParts<'_>,
    registry: &mut Registry,
    rows: &mut Vec<BarItemRow>,
    painter: Option<&mut BarPainter>,
) -> Option<StatusBarAction> {
    if overlay {
        // `BarMode::Overlay`: it overlaps the content. Drawn as a panel, the screen (an
        // `Area(Background)`) would be drawn over it, so it is raised to a `Foreground` Area.
        return draw_status_bar_area(
            ctx, rect, rect, opacity, status_bar, parts, registry, rows, painter,
        );
    }
    // Where there is a painter, the item list (the slots and the gate decisions) is filled in and handed over.
    if painter.is_some() {
        status_bar.collect_items(registry.status_items(), parts.access, rows);
    }
    let mut action = None;
    egui::Panel::top(egui::Id::unique("fairing.status_bar.panel"))
        .exact_size(rect.height())
        .resizable(false)
        .show_separator_line(false)
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            // A1: the status bar icons' opacity while the shade is pulled, `1 − clamp(y/64)`.
            ui.set_opacity(opacity);
            action = match painter {
                Some(painter) => status_bar.ui_with(ui, parts, rows, painter),
                None => status_bar.ui(ui, parts, registry.status_items_mut()),
            };
        });
    action
}

/// Draw the status bar as a `Foreground` Area — `BarMode::Overlay`, the shade panel's status row and
/// the peek all take this path (and the same Area id: only one of them is used per frame).
/// `fade_in(false)`: an egui 0.36 `Area` applies an automatic 80 ms fade on the first frame it is
/// visible and requests a repaint — turned off for idling at 0 fps and for an exact opacity.
#[allow(clippy::too_many_arguments)]
fn draw_status_bar_area(
    ctx: &egui::Context,
    rect: egui::Rect,
    clip: egui::Rect,
    opacity: f32,
    status_bar: &mut StatusBar,
    parts: &mut CxParts<'_>,
    registry: &mut Registry,
    rows: &mut Vec<BarItemRow>,
    painter: Option<&mut BarPainter>,
) -> Option<StatusBarAction> {
    if painter.is_some() {
        status_bar.collect_items(registry.status_items(), parts.access, rows);
    }
    // The layout is `rect` (the whole row) with only what is visible clipped — the status row is
    // revealed from the top as the shade comes down like a curtain (the A1 render mapping). For the
    // peek the two are the same.
    egui::Area::new(egui::Id::unique(STATUS_BAR_OVERLAY_AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(clip.size())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(clip);
            ui.set_min_size(clip.size());
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            child.set_opacity(opacity);
            match painter {
                Some(painter) => status_bar.ui_with(&mut child, parts, rows, painter),
                None => status_bar.ui(&mut child, parts, registry.status_items_mut()),
            }
        })
        .inner
}

/// The screen Rect the shade uses (A1).
///
/// **Where a visible status bar has a band of its own, the shade comes down below it** — that is how
/// iOS's Control Centre and One UI's quick panel behave, and the clock, the signal and the battery
/// not disappearing while the shade opens is right on a device too. In overlay mode
/// (`BarMode::Overlay`) and when hidden, the content starts at the very top of the screen, so this
/// expression hands the whole screen back as it stands and the panel covers the status bar as before
/// (and when hidden, the panel has a status row of **its own**).
fn overlay_screen(layout: &Layout) -> egui::Rect {
    let full = layout
        .status
        .map_or(layout.content, |s| s.union(layout.content))
        .union(layout.nav.unwrap_or(layout.content));
    egui::Rect::from_min_max(
        egui::pos2(full.min.x, layout.content.min.y.max(full.min.y)),
        full.max,
    )
}

/// A real clamped to 0..=100 → an integer percentage (a config value → a backend).
fn percent_from_f64(v: f64) -> u8 {
    // Already clamped, so it is within 0..=100 — no truncation.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let p = v.round().clamp(0.0, 100.0) as u8;
    p
}

/// What stage 14 has to act on — the three taps the chrome and the desktop reported this frame.
/// Bundled so [`Shell::finish_frame`] keeps to the seven-argument lint now that the app's state
/// travels with it too.
#[derive(Debug)]
struct FrameActions {
    desktop: Option<DesktopAction>,
    nav: Option<NavAction>,
    status: Option<StatusBarAction>,
}

/// This frame's values, as the stage 15 repaint decision needs them.
#[derive(Debug, Clone, Copy)]
struct FrameWake {
    /// The earliest wake time a backend asked for.
    next_wake: Option<Instant>,
    /// This frame's shell time.
    now: Instant,
    /// Whether the command queue hit the cap and left some (they are carried on next frame).
    commands_pending: bool,
}

/// The stage 10–13 output (stage 14 handles it).
#[derive(Debug, Default)]
struct LateOutput {
    /// The OSK's hide key.
    osk_hide: bool,
    /// The overlay's action.
    overlay: Option<OverlaySlotAction>,
    /// A status bar tap on the panel's status row or peek (ignored in M2 — the shade is already in view).
    status_row_tapped: bool,
    /// The heads-up's.
    heads_up: Option<HeadsUpAction>,
    /// The toast's.
    toast: Option<ToastAction>,
    /// The prompt's.
    prompt: Option<PromptAction>,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("frame_no", &self.frame_no)
            .field("workspace", &self.workspace)
            .finish_non_exhaustive()
    }
}

impl Shell {
    /// The shell builder. It lays a theme injection, bar painters and a cell painter on from
    /// code.
    ///
    /// ```no_run
    /// # let ctx = egui::Context::default();
    /// let shell = fairing::Shell::builder(fairing::ShellConfig::default())
    ///     .theme(fairing::Theme::light())
    ///     .services(fairing::Services::null())
    ///     .build(&ctx)?;
    /// # let _ = shell;
    /// # Ok::<(), fairing::Error>(())
    /// ```
    #[must_use]
    pub fn builder(config: ShellConfig) -> ShellBuilder {
        ShellBuilder::new(config)
    }

    /// Build the shell. `ctx` is the context [`Waker`](crate::services::Waker) will wrap — to hand the
    /// handle to any thread without a lock, the context has to exist at construction. It
    /// applies the theme to `ctx` and gives the backends a `Waker`.
    ///
    /// A thin wrapper over [`Shell::builder`] — where customisation is needed, use the builder.
    ///
    /// # Errors
    /// Config validation failure ([`crate::Error::Config`]) — above all, two or more level tables with
    /// no `default_gate`, or a bad role name in `[theme.palette]`.
    pub fn new(config: ShellConfig, services: Services, ctx: &egui::Context) -> Result<Self> {
        Self::builder(config).services(services).build(ctx)
    }

    /// Add a declaration. It takes effect from the next frame. The same id is replaced — any open
    /// instance is closed and each one raises a [`ShellEvent::ScreenClosed`] (so that an
    /// integrator can tidy up the replaced screen's external state).
    ///
    /// A nav item the bar will not draw on that frame — one `[nav_bar] items` does not name, or
    /// any on a bar that is off or in the gesture style — is warned about in the log.
    pub fn add(&mut self, decl: impl Into<Decl>) {
        let decl = decl.into();
        let id = decl.id().to_owned();
        if matches!(decl, Decl::NavItem(_)) && !self.nav_item_checks.contains(&id) {
            self.nav_item_checks.push(id.clone());
        }
        if let Some(replaced) = self.registry.add(decl) {
            self.retire_decl(replaced);
            self.close_decl_instances(&id);
        }
    }

    /// A declaration replaced or removed: its resident screen goes to its open instance, which is
    /// closing, so that `Destroyed` reaches the screen that instance ran.
    fn retire_decl(&mut self, decl: Decl) {
        if let Decl::Screen(decl) = decl {
            let id = decl.id.clone();
            if let Some(screen) = decl.into_resident() {
                self.workspace.adopt_resident(&id, screen);
            }
        }
    }

    /// A nav item is drawn only where `[nav_bar] items` names it — unlike a status item it is not
    /// appended — and never on a bar that is off or in the gesture style. Each of those used to
    /// leave the declaration doing nothing without a word.
    ///
    /// Checked on the frame after the `add`, not inside it: an integrator may declare the item
    /// first and list it in code afterwards (`nav_bar_mut().style = NavStyle::Buttons { .. }`),
    /// and a warning at the `add` would then be wrong.
    fn check_added_nav_items(&mut self) {
        if self.nav_item_checks.is_empty() {
            return;
        }
        for id in std::mem::take(&mut self.nav_item_checks) {
            // Removed, or replaced by a declaration of another kind, before this frame.
            if self.registry.kind(&id) == Some(DeclKind::NavItem) {
                self.warn_if_nav_item_hidden(&id);
            }
        }
    }

    fn warn_if_nav_item_hidden(&self, id: &str) {
        let why = match &self.nav_bar.style {
            _ if !self.nav_bar.enabled => "the nav bar is off ([nav_bar] enabled = false)",
            NavStyle::Gesture { .. } => "the gesture-style nav bar draws no items",
            NavStyle::Buttons { items } if !items.iter().any(|item| item.id() == id) => {
                "it is not in [nav_bar] items - a nav item is drawn only where that list names it"
            }
            NavStyle::Buttons { .. } => return,
        };
        log::warn!("nav_item `{id}` will not be drawn: {why}");
    }

    /// Remove by id alone (of any kind, the built-in status items included). The icon or item goes
    /// with it and any open screen is closed. `true` if it was there.
    ///
    /// A built-in status item (`status.*`) raises a [`ShellEvent::DeclRemoved`] in **the same shape**
    /// as a registry declaration — an integrator has to be able to tidy up external state from one
    /// event ("the built-in items share the id scheme").
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(removed) = self.registry.remove(id) else {
            // The registry → the status bar → **the quick-settings tiles**, in that order. A built-in
            // tile comes from the `[overlay] tiles` list rather than from a declaration, so the first
            // two could never catch it (once a defect: `shell.remove("tile.wifi")` quietly returned
            // `false`).
            if !self.status_bar.remove(id) && !self.remove_tile(id) {
                return false;
            }
            self.events.push(ShellEvent::DeclRemoved(id.to_owned()));
            return true;
        };
        self.retire_decl(removed);
        self.close_decl_instances(id);
        self.events.push(ShellEvent::DeclRemoved(id.to_owned()));
        true
    }

    /// Remove a quick-settings tile (there is only something there under feature `overlay`).
    #[cfg(feature = "overlay")]
    fn remove_tile(&mut self, id: &str) -> bool {
        self.overlay.0.remove_tile(id)
    }

    /// With feature `overlay` off there are no tiles at all.
    #[cfg(not(feature = "overlay"))]
    #[expect(clippy::unused_self, reason = "it matches the feature-on signature")]
    fn remove_tile(&mut self, _id: &str) -> bool {
        false
    }

    /// Close every open instance of declaration `id` (with no animation), raising a `ScreenClosed` per
    /// instance and a `WentHome` where that leaves it at home. Shared by `add` (a replacement) and
    /// `remove`.
    fn close_decl_instances(&mut self, id: &str) {
        let closed: Vec<InstanceId> = self
            .workspace
            .tasks()
            .iter()
            .flat_map(|t| t.iter().filter(|i| i.decl_id() == id).map(Instance::id))
            .collect();
        if closed.is_empty() {
            return;
        }
        let was_home = self.workspace.is_home();
        self.workspace.close_decl(id, &self.theme.motion);
        for instance in closed {
            self.events.push(ShellEvent::ScreenClosed {
                id: id.to_owned(),
                instance,
            });
        }
        self.note_went_home(was_home);
    }

    /// Shared by the closing paths: raise a [`ShellEvent::WentHome`] **only where it was not home and
    /// has become home**. `was_home` is the value taken **before** the close — one background task's
    /// instance closing while already at home is not a transition.
    fn note_went_home(&mut self, was_home: bool) {
        if !was_home && self.workspace.is_home() {
            self.events.push(ShellEvent::WentHome);
        }
    }

    /// The handle (`Clone + Send`).
    #[must_use]
    pub fn handle(&self) -> ShellHandle {
        self.handle.clone()
    }

    /// Whether this shell is quarantined, after an unwind or an explicit host request.
    /// Normal dispatch stays blocked until the host replaces it, for example with
    /// [`Self::restart_with`]. Business mutations are not rolled back.
    #[must_use]
    pub const fn is_faulted(&self) -> bool {
        self.faulted
    }

    /// Stop normal shell dispatch after an application/host failure.
    /// Queued work is retained but will never execute on this shell. New sends are refused.
    /// The first transition requests a host repaint for recovery handling/guidance.
    pub fn quarantine(&mut self) {
        let was_faulted = self.faulted;
        self.faulted = true;
        self.auth_request = None;
        self.routed_auth = None;
        self.access.quarantine();
        self.rx.close_admission();
        if !was_faulted {
            let _ = crate::runtime::contain(|| self.handle.waker().wake());
        }
    }

    /// Replace this shell with one constructed by the application.
    ///
    /// The old inbox closes before `build` runs; queued work is never replayed and old
    /// handles remain disconnected. Obtain a new handle after success. Configuration,
    /// authentication, registrations and recovery UI come from the new shell. External
    /// business state and egui context state are not reset or rolled back.
    ///
    /// The host chooses when to call this, automatically or in response to user input.
    /// Use the host's existing egui context when building the replacement.
    ///
    /// # Errors
    /// Returns [`crate::recovery::RestartError`] on construction error, an already faulted
    /// replacement, or an unwind during construction/disposal. The current shell remains
    /// quarantined on any failure (it may be the new shell if old-shell disposal failed).
    /// Requires unwinding; aborts and double panics cannot be contained.
    pub fn restart_with(
        &mut self,
        build: impl FnOnce() -> crate::Result<Self>,
    ) -> std::result::Result<(), crate::recovery::RestartError> {
        use crate::recovery::RestartError;
        self.quarantine();
        let result = crate::runtime::attempt(|| {
            let replacement = build().map_err(RestartError::Build)?;
            if replacement.is_faulted() {
                return Err(RestartError::FaultedReplacement);
            }
            let old = std::mem::replace(self, replacement);
            drop(old);
            self.handle.waker().wake();
            Ok(())
        })
        .unwrap_or(Err(RestartError::Panicked));
        if result.is_err() {
            self.quarantine();
        }
        result
    }

    /// Enable, replace or hide the optional fault view. Clears any unconsumed button request.
    pub fn set_recovery_ui(&mut self, view: Option<crate::recovery::RecoveryUi>) {
        self.recovery_ui = view;
        self.restart_requested = false;
        if self.faulted {
            let _ = crate::runtime::contain(|| self.handle.waker().wake());
        }
    }

    /// Consume the optional fault-view button's request. Multiple clicks coalesce.
    /// This does not restart anything; the application decides what to do with it.
    pub fn take_restart_request(&mut self) -> bool {
        std::mem::take(&mut self.restart_requested)
    }

    fn show_recovery_ui(&mut self, ui: &mut egui::Ui) {
        if !crate::runtime::contain(|| {
            if self
                .recovery_ui
                .as_ref()
                .and_then(|view| view.show(ui))
                .is_some_and(|button| button.clicked())
            {
                self.restart_requested = true;
                ui.ctx().request_repaint();
            }
        }) {
            self.recovery_ui = None;
            crate::runtime::blank_faulted_frame(ui.ctx());
        }
    }

    fn contain_fault(&mut self, work: impl FnOnce(&mut Self)) {
        if self.faulted {
            return;
        }
        // Inner public APIs must unwind to the outer transaction boundary; swallowing here
        // would let the rest of an interrupted poll/frame continue.
        if self.boundary_active {
            work(self);
            return;
        }
        self.boundary_active = true;
        let completed = crate::runtime::contain(|| work(self));
        self.boundary_active = false;
        if !completed {
            self.quarantine();
        }
    }

    /// Take the events that have built up.
    pub fn poll_events(&mut self) -> Vec<ShellEvent> {
        if self.faulted {
            return Vec::new();
        }
        std::mem::take(&mut self.events)
    }

    /// Last frame's layout.
    #[must_use]
    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    /// The workspace.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The desktop.
    #[must_use]
    pub fn desktop(&self) -> &DesktopView {
        &self.desktop
    }

    /// The desktop (mutable — for swapping the wallpaper, and so on).
    pub fn desktop_mut(&mut self) -> &mut DesktopView {
        &mut self.desktop
    }

    /// The status bar (for looking last frame's item Rects up with `item_rect`, and so on).
    #[must_use]
    pub fn status_bar(&self) -> &StatusBar {
        &self.status_bar
    }

    /// The status bar (mutable — `tap_action`, editing the slots).
    pub fn status_bar_mut(&mut self) -> &mut StatusBar {
        &mut self.status_bar
    }

    /// The nav bar (mutable — `enabled`, `style`, replacing the items).
    pub fn nav_bar_mut(&mut self) -> &mut NavBar {
        &mut self.nav_bar
    }

    /// The nav bar (for looking last frame's item Rects up with `item_rect`, and so on).
    #[must_use]
    pub fn nav_bar(&self) -> &NavBar {
        &self.nav_bar
    }

    /// The theme.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// **The metrics, to change at run time** — the text sizes
    /// ([`MetricsSpec::type_scale`](crate::theme::MetricsSpec::type_scale)) included. The next frame
    /// resolves them at the current scale and hands them to egui's style.
    ///
    /// It is the **spec** and not [`Theme::metrics`] because the resolved metrics are a derived
    /// value: with a spec in play the shell rebuilds `theme.metrics` from it every frame, so a size
    /// written onto the theme was thrown away before it could reach egui. That is what made the
    /// type scale a token no rung of the override ladder could hold after start-up.
    ///
    /// Where the shell was built without one, the spec is seeded from the metrics it has, so this
    /// always hands back something to write on.
    ///
    /// ```no_run
    /// # use fairing::Shell;
    /// # fn bigger(shell: &mut Shell) {
    /// # let step = |s: &mut fairing::unit::Span<fairing::unit::Eye>, by: f32| { let _ = (s, by); };
    /// // Standing 70 cm away rather than 30 — the body text goes up.
    /// let spec = shell.metrics_spec_mut();
    /// step(&mut spec.type_scale[1], 4.0);
    /// # }
    /// ```
    ///
    /// The palette and the motion tokens have their own paths: [`Shell::set_palettes`] and
    /// [`Shell::set_theme_dark`] so the crossfade (10) runs, and [`Shell::set_motion`].
    pub fn metrics_spec_mut(&mut self) -> &mut crate::theme::MetricsSpec {
        self.theme_dirty = true;
        self.metrics_spec
            .get_or_insert_with(|| crate::theme::MetricsSpec::from_metrics(&self.theme.metrics))
    }

    /// The backends.
    #[must_use]
    pub fn services(&self) -> &Services {
        &self.services
    }

    /// The backends (mutable).
    pub fn services_mut(&mut self) -> &mut Services {
        &mut self.services
    }

    /// **Connect the time source, while the shell is running**.
    ///
    /// The clock is a backend like any other, so
    /// [`Services::builder().clock(..)`](crate::services::ServicesBuilder::clock) sets it before
    /// the shell is built. This is the same thing afterwards — the status bar clock, the shade and
    /// `settings.datetime` all read `services.clock`, so one call moves them together.
    ///
    /// The crate ships two: [`SystemClock`](crate::services::clock::SystemClock), which is
    /// `SystemTime` plus **an offset you give it**, and — behind feature `chrono` —
    /// [`ChronoClock`](crate::services::clock::ChronoClock), which follows the system time zone and
    /// its summer time. Anything else is a [`ClockSource`](crate::services::ClockSource)
    /// implementation of your own: a device that gets its time from a PLC or a GPS receiver hands
    /// that over here and the shell asks it, rather than the shell deciding what the time is.
    ///
    /// ```no_run
    /// # fn main() {
    /// # let shell: &mut fairing::Shell = todo!();
    /// use fairing::services::clock::SystemClock;
    ///
    /// // The device was told it is in KST — the offset is the integrator's to keep, not the shell's.
    /// shell.set_clock(SystemClock::with_offset(540));
    /// # }
    /// ```
    ///
    /// Whether the clock reads 24- or 12-hour is a separate question, and a **setting** rather than
    /// a backend: [`crate::settings::keys::UI_CLOCK_12H`], which
    /// `settings.datetime` writes and the status bar reads.
    pub fn set_clock(&mut self, clock: impl crate::services::ClockSource + 'static) {
        let mut clock = Box::new(clock);
        // The `Waker` goes to every backend at startup; one arriving later needs its own.
        clock.attach(self.handle.waker().clone());
        self.services.clock = clock;
    }

    /// Access control.
    #[must_use]
    pub fn access(&self) -> &Access {
        &self.access
    }

    /// Access control (mutable — for the integrator to replace the `AccessPolicy`, `access_mut().set_policy(..)`).
    pub fn access_mut(&mut self) -> &mut Access {
        &mut self.access
    }

    /// The shell's monotonic time at its last UI or logic update. `Cx::now`,
    /// `evict_after` and the backends' `poll_at` all see the same timeline.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.mono
    }

    /// The config.
    #[must_use]
    pub fn config(&self) -> &ShellConfig {
        &self.config
    }

    /// The declaration registry.
    #[doc(hidden)]
    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Whether the shell's prompt is up — an unlock prompt, or the lock screen with its prompt.
    /// Never in `routing` mode, with one level, or without an authenticator.
    #[must_use]
    pub fn unlock_prompt_visible(&self) -> bool {
        self.prompt.is_open()
    }

    /// Whether what is up is the lock screen.
    #[must_use]
    pub fn lock_screen_visible(&self) -> bool {
        self.prompt.is_lock_screen()
    }

    /// Where the prompt's key for `digit` was drawn last frame — for a tour or a test pressing it
    /// the way a finger would, wherever a shuffle put it.
    #[doc(hidden)]
    #[must_use]
    pub fn prompt_digit_rect(&self, digit: u8) -> Option<egui::Rect> {
        self.prompt.digit_rect(digit)
    }

    /// Where the prompt's pattern dot `dot` (counted from 0, row by row) was drawn last frame —
    /// for a tour or a test drawing a pattern the way a finger would.
    #[doc(hidden)]
    #[must_use]
    pub fn prompt_dot_center(&self, dot: u8) -> Option<egui::Pos2> {
        self.prompt.dot_center(dot)
    }

    /// Where the prompt's tab for method `index` was drawn last frame, while it has tabs.
    #[doc(hidden)]
    #[must_use]
    pub fn prompt_tab_rect(&self, index: usize) -> Option<egui::Rect> {
        self.prompt.tab_rect(index)
    }

    /// The shell-owned animation store (used by `Cx::animate` and the A7 presses). For verifying the entry count and the pruning.
    #[doc(hidden)]
    #[must_use]
    pub fn animations(&self) -> &AnimationStore {
        &self.animations
    }

    /// The gesture engine (for reading last frame's state).
    #[doc(hidden)]
    #[must_use]
    pub fn gestures(&self) -> &GestureEngine {
        &self.gestures
    }

    /// The shade (feature `overlay`).
    #[cfg(feature = "overlay")]
    #[must_use]
    pub fn overlay(&self) -> &crate::overlay::Overlay {
        &self.overlay.0
    }

    /// OSK (feature `osk`).
    #[cfg(feature = "osk")]
    #[must_use]
    pub fn osk(&self) -> &crate::osk::Osk {
        &self.osk.0
    }

    /// The OSK (mutable — replacing the layout, the manual toggle).
    #[cfg(feature = "osk")]
    pub fn osk_mut(&mut self) -> &mut crate::osk::Osk {
        &mut self.osk.0
    }

    /// The notification centre.
    #[must_use]
    pub fn notifications(&self) -> &NotificationCenter {
        &self.notifications
    }

    /// The toast queue.
    #[doc(hidden)]
    #[must_use]
    pub fn toasts(&self) -> &ToastQueue {
        &self.toasts
    }

    /// The heads-up banner.
    #[doc(hidden)]
    #[must_use]
    pub fn heads_up(&self) -> &HeadsUp {
        &self.heads_up
    }

    /// The chrome policy driver's state (the `keep_awake` value handed over, and so on).
    #[doc(hidden)]
    #[must_use]
    pub fn policy_driver(&self) -> &PolicyDriver {
        &self.guard
    }

    /// Notify (directly, from the UI thread). From another thread, [`ShellHandle::notify`].
    pub fn notify(&mut self, notification: Notification) {
        let _ = self.try_notify(notification);
    }

    /// Admit notification storage fallibly; optional banner allocation may be omitted.
    ///
    /// # Errors
    /// Hard retention cap, oversized payload or failed storage growth. Ownership is returned.
    #[allow(
        clippy::result_large_err,
        reason = "return rejected ownership inline; boxing an allocation failure would allocate again"
    )]
    pub fn try_notify(
        &mut self,
        mut notification: Notification,
    ) -> std::result::Result<crate::notify::CenterChange, crate::notify::Rejected<Notification>>
    {
        if notification.at == crate::time::WallTime::default() {
            notification.at = self.services.clock.now();
        }
        let id = notification.id;
        let change = match self.notifications.try_push(notification) {
            Ok(change) => change,
            Err(error) => {
                self.display_status.discarded = self.display_status.discarded.saturating_add(1);
                self.display_status.allocation_failed |=
                    error.kind() == crate::notify::AdmissionError::AllocationFailed;
                return Err(error);
            }
        };
        let Some(notification) = self.notifications.get(id) else {
            return Ok(change);
        };
        if change != crate::notify::CenterChange::Added {
            // No new banner for an update, but one already up for it shows what it now says.
            self.heads_up.update(notification);
            return Ok(change);
        }
        // The cue is the audio backend's to sound; `ui.silent` keeps it quiet and leaves the banner.
        let silent = matches!(
            self.settings.get(&keys::UI_SILENT.into()),
            Some(SettingValue::Bool(true))
        );
        if !silent {
            self.services.audio.play_cue(crate::services::Cue::Notify);
        }
        if self.config.notify.heads_up && self.overlay.is_closed() {
            self.heads_up
                .show(notification, self.mono, &self.theme.motion);
        }
        Ok(change)
    }

    /// A toast (directly, from the UI thread).
    pub fn toast<'a>(&mut self, toast: impl Into<ToastInput<'a>>) {
        match toast.into() {
            ToastInput::Text(text) => {
                let _ = self.toasts.try_push_text(text);
            }
            ToastInput::Value(toast) => self.toasts.push(toast),
        }
    }

    /// Fallibly construct/admit a bounded text toast; refuse it without stopping other work.
    ///
    /// # Errors
    /// Full display queue, oversized text or failed fallible allocation.
    pub fn try_toast(
        &mut self,
        text: &str,
    ) -> std::result::Result<(), crate::notify::AdmissionError> {
        self.toasts.try_push_text(text)
    }

    /// Read and acknowledge display omissions caused by toast retention/reclamation.
    /// Reporting does not enqueue another toast or allocate warning text.
    pub fn take_display_status(&mut self) -> crate::memory::DisplayStatus {
        self.display_status.merge(self.handle.display_status(true));
        self.display_status
            .merge(self.icons.cache.take_display_status());
        #[cfg(feature = "osk")]
        self.display_status.merge(self.osk.0.take_display_status());
        self.display_status.merge(self.toasts.take_display_status());
        self.display_status
            .merge(self.heads_up.take_display_status());
        std::mem::take(&mut self.display_status)
    }
    /// Record application display clipping; retain the warning until `take_display_status` acknowledges it.
    pub fn report_display_status(&mut self, status: crate::memory::DisplayStatus) {
        self.display_status.merge(status);
    }

    /// Dismiss a notification. `false` for a persistent/missing item or unavailable event storage.
    /// If storage is unavailable the notification is retained; use [`Self::try_dismiss_notification`]
    /// to distinguish that case and retry later.
    pub fn dismiss_notification(&mut self, id: NotificationId) -> bool {
        self.try_dismiss_notification(id).unwrap_or(false)
    }

    /// Reserve the dismissal event before removing a notification. Failure leaves both the
    /// retained notification and its unread state unchanged; nothing is reported as completed.
    ///
    /// # Errors
    /// Event storage cannot grow. No logging, rendering or warning allocation is performed.
    pub fn try_dismiss_notification(
        &mut self,
        id: NotificationId,
    ) -> std::result::Result<bool, crate::notify::AdmissionError> {
        if self
            .notifications
            .get(id)
            .is_none_or(|note| note.persistent)
        {
            return Ok(false);
        }
        if self.events.try_reserve(1).is_err() {
            self.display_status.allocation_failed = true;
            return Err(crate::notify::AdmissionError::AllocationFailed);
        }
        let dismissed = self.notifications.dismiss(id);
        if dismissed {
            self.events.push(ShellEvent::NotificationDismissed(id));
        }
        Ok(dismissed)
    }

    /// Replace the motion tokens (the tuning tool, `examples/motion_lab.rs`). The gesture engine's
    /// tuning (the slop, taps, long presses, flings) follows too. Animations in progress are left
    /// alone.
    pub fn set_motion(&mut self, tokens: crate::theme::MotionTokens) {
        self.theme.motion = tokens;
        let mut tuning = *self.gestures.tuning();
        tuning.slop_px = tokens.slop_px;
        tuning.tap = tokens.tap;
        tuning.long_press = tokens.long_press;
        tuning.fling_px_s = tokens.fling_px_s;
        self.gestures.set_tuning(tuning);
    }

    /// The dark/light switch — it cross-fades the palette over 200 ms (A7). The `theme.dark` setting
    /// and `tile.theme` come here.
    ///
    /// The target palette is chosen from the pair [`Shell::set_palettes`] settled. Where both sides of
    /// the pair are the same (the default when the theme was injected from code) the colours stay and
    /// only the `dark` flag and egui's `Visuals` change — **an injected palette does not vanish at one
    /// toggle**.
    pub fn set_theme_dark(&mut self, dark: bool) {
        if self.theme.dark == dark {
            return;
        }
        self.theme.dark = dark;
        // egui's `Visuals` follow the flag even where the palette does not move (equal sides
        // of the pair give the crossfade nothing to yield).
        self.theme_dirty = true;
        let target = if dark {
            self.palettes.0
        } else {
            self.palettes.1
        };
        self.theme_fade.start(target, self.theme.motion.theme_fade);
    }

    /// **Register a hidden entry point**. The invisible door into the service menu.
    ///
    /// An id already there is swapped out (its progress is reset).
    ///
    /// # This is not a security boundary
    ///
    /// The knocking is **concealment** and what stops anyone is the [`Gate`]. Leave `gate` empty and
    /// whoever finds the door walks in — anything irreversible, like a factory reset or a calibration
    /// value, takes one without exception.
    ///
    /// ```no_run
    /// # fn main() -> fairing::Result<()> {
    /// use fairing::access::{Corner, HiddenEntry, TapKnock, ZoneKnock};
    /// use fairing::LaunchAction;
    ///
    /// # let mut shell: fairing::Shell = todo!();
    /// // The Android way — a screen knocks with `cx.knock("service")`.
    /// shell.add_hidden_entry(
    ///     HiddenEntry::new("service", TapKnock::new(7), LaunchAction::open("service_menu"))
    ///         .gate("service")
    ///         .hint_from(3),
    /// );
    /// // The corner way — it needs no widget, so it works on a completely locked screen.
    /// shell.add_hidden_entry(
    ///     HiddenEntry::new(
    ///         "factory",
    ///         ZoneKnock::corners([
    ///             Corner::TopLeft,
    ///             Corner::TopRight,
    ///             Corner::BottomRight,
    ///             Corner::BottomLeft,
    ///         ]),
    ///         LaunchAction::open("factory"),
    ///     )
    ///     .gate("factory"),
    /// );
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_hidden_entry(&mut self, mut entry: crate::access::HiddenEntry) {
        entry.trigger.reset();
        if let Some(slot) = self.hidden.iter_mut().find(|e| e.id == entry.id) {
            *slot = entry;
        } else {
            self.hidden.push(entry);
        }
        self.refresh_hidden_remaining();
    }

    /// Take a hidden entry point off. `false` for an id that was not there.
    pub fn remove_hidden_entry(&mut self, id: &str) -> bool {
        let before = self.hidden.len();
        self.hidden.retain(|e| e.id != id);
        let removed = self.hidden.len() != before;
        if removed {
            self.refresh_hidden_remaining();
        }
        removed
    }

    /// The registered entry points' ids (in registration order).
    pub fn hidden_entries(&self) -> impl Iterator<Item = &str> {
        self.hidden.iter().map(|e| e.id.as_str())
    }

    /// **Poke** an entry point once — it reaches the trigger as
    /// [`KnockInput::pokes`](crate::access::KnockInput::pokes) `= 1`.
    /// [`Cx::knock`](crate::Cx::knock) comes here.
    ///
    /// An unknown id does nothing — a typo does not stop the shell (the same policy as a config id the code never declares).
    pub fn knock(&mut self, id: &str) {
        self.contain_fault(|shell| shell.knock_in(id, &mut None));
    }

    /// [`Shell::knock`] **with the app's state in hand** — a hidden entry point whose action is a
    /// `Run` closure that reads it. See [`Shell::launch_with`].
    pub fn knock_with(&mut self, id: &str, app: &mut dyn std::any::Any) {
        if self.faulted {
            return;
        }
        self.contain_fault(|shell| shell.knock_in(id, &mut Some(app)));
    }

    fn knock_in(&mut self, id: &str, app: &mut AppRef<'_>) {
        let Some(idx) = self.hidden.iter().position(|e| e.id == id) else {
            log::warn!("no hidden entry point \"{id}\" - ignoring the knock");
            return;
        };
        let input = crate::access::KnockInput {
            now: self.mono,
            screen: self.last_screen,
            pokes: 1,
            taps: &[],
            keys: &[],
            slop: self.theme.motion.slop_px,
        };
        let step = self.hidden.get_mut(idx).map(|e| e.trigger.feed(&input));
        if let Some(step) = step {
            self.after_knock(idx, step, app);
        }
    }

    /// Feed this frame's input to every trigger, once a frame.
    ///
    /// It is called on frames with no input too — a trigger with a time limit has to be able to expire
    /// by itself. It **reads the pointer events directly**: `PointerState` sometimes clears the
    /// position with `PointerGone` on the release frame (both the test harness and real touch do), so
    /// watching the state alone loses where the finger came up.
    ///
    /// A dragged tap is handed over as it is — whether to count a swipe is the trigger's decision.
    fn poll_knocks(
        &mut self,
        ctx: &egui::Context,
        screen: egui::Rect,
        now: Instant,
        app: &mut AppRef<'_>,
    ) {
        self.last_screen = screen;
        if self.hidden.is_empty() {
            self.tap_press = None;
            return;
        }
        let mut taps: Vec<crate::access::Tap> = Vec::new();
        let mut keys: Vec<egui::Key> = Vec::new();
        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        ..
                    } => {
                        if *pressed {
                            self.tap_press =
                                self.region_zones.at(*pos).is_none().then_some((*pos, 0.0));
                        } else if let Some((press, travel)) = self.tap_press.take() {
                            taps.push(crate::access::Tap {
                                press,
                                release: *pos,
                                travel: travel.max((*pos - press).length()),
                            });
                        }
                    }
                    egui::Event::PointerMoved(pos) => {
                        if let Some((origin, travel)) = self.tap_press.as_mut() {
                            *travel = travel.max((*pos - *origin).length());
                        }
                    }
                    egui::Event::PointerGone | egui::Event::WindowFocused(false) => {
                        self.tap_press = None;
                    }
                    egui::Event::Key {
                        key,
                        pressed: true,
                        repeat: false,
                        ..
                    } => keys.push(*key),
                    _ => {}
                }
            }
        });
        let input = crate::access::KnockInput {
            now,
            screen,
            pokes: 0,
            taps: &taps,
            keys: &keys,
            slop: self.theme.motion.slop_px,
        };
        for idx in 0..self.hidden.len() {
            let step = self.hidden.get_mut(idx).map(|e| e.trigger.feed(&input));
            if let Some(step) = step {
                self.after_knock(idx, step, app);
            }
        }
    }

    /// So that the event list does not grow without bound where an integrator never calls
    /// [`Shell::poll_events`], everything over the cap is dropped **oldest first**.
    fn trim_events(&mut self) {
        let len = self.events.len();
        if len <= MAX_PENDING_EVENTS {
            return;
        }
        let drop = len - MAX_PENDING_EVENTS;
        self.events.drain(..drop);
        log::warn!(
            "more than {MAX_PENDING_EVENTS} events queued, dropped the {drop} oldest - check that poll_events() runs every frame"
        );
    }

    /// Once a language: can the loaded fonts write it? Without a font that has its letters the text
    /// comes out as boxes, and nothing on the screen says why — a Korean panel on egui's default
    /// fonts is exactly that.
    fn check_script(&mut self, ctx: &egui::Context) {
        if self.script_checked.as_deref() == Some(self.strings.locale()) {
            return;
        }
        self.script_checked = Some(self.strings.locale().to_owned());
        let letters = self.strings.letters();
        let font = egui::FontId::proportional(14.0);
        let missing: Vec<char> = ctx.fonts_mut(|fonts| {
            letters
                .into_iter()
                .filter(|&c| !fonts.has_glyph(&font, c))
                .collect()
        });
        if let Some(first) = missing.first() {
            log::warn!(
                "locale \"{}\": no loaded font has {} of its letters ('{first}' among them) - they will show as boxes. Load one with ShellBuilder::fonts (fairing::fonts::korean_font finds a Korean one)",
                self.strings.locale(),
                missing.len()
            );
        }
    }

    fn flush_lifecycle(&mut self, app: &mut AppRef<'_>) {
        self.with_parts(app, |workspace, registry, parts| {
            workspace.flush_lifecycle(parts, registry);
            Some(())
        });
    }

    /// Borrow the workspace and the rest of the pieces at the same time.
    fn with_parts<R>(
        &mut self,
        app: &mut AppRef<'_>,
        f: impl FnOnce(&mut Workspace, &mut Registry, &mut CxParts<'_>) -> Option<R>,
    ) -> Option<R> {
        let now = self.mono;
        let frame = self.frame_no;
        let Self {
            workspace,
            registry,
            handle,
            access,
            services,
            settings,
            strings,
            theme,
            icons,
            widget_painters,
            animations,
            requests,
            hidden_remaining,
            ..
        } = self;
        let mut parts = CxParts {
            shell: handle,
            access,
            services,
            settings,
            app: app.as_deref_mut(),
            strings,
            theme,
            icons,
            animations,
            widget_painters,
            requests,
            hidden: hidden_remaining,
            now,
            frame,
        };
        f(workspace, registry, &mut parts)
    }
}

/// Bind this crate's strong and display families onto the font definitions egui has now, each
/// seeded with the regular faces where it is missing — for definitions that replaced the ones
/// [`ShellBuilder::build`] installed. What is already there is kept.
fn rebind_font_families(ctx: &egui::Context) {
    let mut defs = ctx.fonts(|f| f.definitions().clone());
    let regular = defs
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let strong = defs
        .families
        .entry(crate::fonts::strong_family())
        .or_insert(regular)
        .clone();
    defs.families
        .entry(crate::fonts::display_family())
        .or_insert(strong);
    ctx.set_fonts(defs);
}

/// Whether an input event is a person — the idle timers start again on these. A
/// screenshot arriving or the window gaining focus is not someone at the panel.
fn is_user_input(event: &egui::Event) -> bool {
    matches!(
        event,
        egui::Event::Key { .. }
            | egui::Event::Text(_)
            | egui::Event::PointerButton { .. }
            | egui::Event::PointerMoved(_)
            | egui::Event::MouseMoved(_)
            | egui::Event::MouseWheel { .. }
            | egui::Event::Touch { .. }
            | egui::Event::Zoom(_)
            | egui::Event::Paste(_)
            | egui::Event::Copy
            | egui::Event::Cut
            | egui::Event::Ime(_)
    )
}

/// Stage 12: the prompt, with the widget context lent from the frame's parts. `root` is the whole
/// screen — the modal covers the bars too.
fn draw_prompt(
    ctx: &egui::Context,
    prompt: &mut Prompt,
    parts: &mut CxParts<'_>,
    layout: &Layout,
    root: egui::Rect,
) -> Option<PromptAction> {
    if !prompt.is_drawn() {
        return None;
    }
    let clock = if prompt.wants_clock() {
        let now = parts.services.clock.now();
        let hour12 = matches!(
            parts.settings.get(&keys::UI_CLOCK_12H.into()),
            Some(SettingValue::Bool(true))
        );
        let format = crate::time::ClockFormat::Hm.with_hour12(hour12);
        let day = now.civil();
        // The date's order is the locale's: a template through the string table.
        let date = parts
            .strings
            .get(labels::DATE)
            .replace("{y}", &format!("{:04}", day.year))
            .replace("{mm}", &format!("{:02}", day.month))
            .replace("{dd}", &format!("{:02}", day.day))
            .replace("{m}", &day.month.to_string())
            .replace("{d}", &day.day.to_string());
        (
            now.format_with(format, crate::time::Meridiem::of(parts.strings)),
            date,
        )
    } else {
        (String::new(), String::new())
    };
    // The card keeps clear of the keyboard, which sits on top of the nav bar's band.
    let bottom = layout.osk.map_or(root.max.y, |osk| osk.min.y);
    let mut cx = PromptCx {
        widgets: fairing_widgets::WidgetCx {
            theme: parts.theme,
            icons: &mut *parts.icons,
            anims: &mut *parts.animations,
            anim_scope: egui::Id::unique("fairing.prompt"),
            frame: parts.frame,
            inset_bottom: 0.0,
            painters: Some(&mut *parts.widget_painters),
        },
        now: parts.now,
        clock,
        allow_continue: parts.access.lock_screen_continue(),
        strings: parts.strings,
    };
    prompt.ui(ctx, root, bottom, &mut cx)
}

/// The app's state as it travels down the frame: an `Option` because
/// [`Shell::frame`] lends nothing, and a `&mut` to the `Option` so each stage can reborrow it for
/// the next without moving it.
type AppRef<'a> = Option<&'a mut dyn std::any::Any>;

/// Whether this action **takes you out of the shade.**
///
/// Where it does, the shade goes up first. Opening a screen while the shade still covers it leaves no
/// telling that it opened — which is exactly what the footer's settings button did.
///
/// Operating a value (`Set` · `Toggle`) is **not** one of them. Adjusting the brightness and then
/// turning Wi-Fi off is normal, and closing on every touch would make it unusable. `OpenOverlay` is
/// the shade itself, so closing would make it blink.
fn leaves_overlay(action: &LaunchAction) -> bool {
    match action {
        LaunchAction::Open { .. }
        | LaunchAction::Run(_)
        | LaunchAction::Lock
        | LaunchAction::Logout
        | LaunchAction::OpenOverview
        | LaunchAction::ToggleSplit => true,
        // Being `#[non_exhaustive]`, new variants may appear. **Not closing is the default** — closing
        // the shade on an unknown action would close things that should not close.
        _ => false,
    }
}

/// The density from the resolution and the physical size (physical px / mm). It is reckoned **by
/// area** rather than as the mean of the short and long axes, so the area error is smallest even with
/// non-square pixels. Where the values are odd it hands back `NaN` and sends it to the fallback.
fn density(size_px: (u32, u32), mm: (f32, f32)) -> f32 {
    let (w, h) = (f64::from(size_px.0), f64::from(size_px.1));
    let (mw, mh) = (f64::from(mm.0), f64::from(mm.1));
    if w <= 0.0 || h <= 0.0 || mw <= 0.0 || mh <= 0.0 {
        return f32::NAN;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the density is a single-digit magnitude, so an f32 is enough"
    )]
    {
        ((w * h) / (mw * mh)).sqrt() as f32
    }
}

/// The icon rail's one column. Being outside the workspace, a screen does not cover it.
/// A desktop icon's info popover, over everything the desktop and the bars drew.
/// It keeps to the space between the bars, the rail's included.
fn draw_icon_info(
    ctx: &egui::Context,
    root: egui::Rect,
    layout: &Layout,
    desktop: &mut crate::desktop::DesktopView,
    parts: &mut CxParts<'_>,
) {
    let bounds = layout
        .rail
        .map_or(layout.content, |rail| rail.union(layout.content));
    let mut dcx = crate::desktop::DesktopCtx {
        theme: parts.theme,
        access: parts.access,
        icons: &mut *parts.icons,
        now: parts.now,
        legibility: desktop.legibility(),
        strings: parts.strings,
    };
    desktop.info_ui(ctx, root, bounds, &mut dcx);
}

fn draw_rail(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    desktop: &mut crate::desktop::DesktopView,
    workspace: &Workspace,
    parts: &mut CxParts<'_>,
) -> Option<DesktopAction> {
    let open = workspace
        .focused()
        .map(Instance::decl_id)
        .map(str::to_owned);
    let mut dcx = crate::desktop::DesktopCtx {
        theme: parts.theme,
        access: parts.access,
        icons: &mut *parts.icons,
        now: parts.now,
        legibility: desktop.legibility(),
        strings: parts.strings,
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect);
    desktop.rail_ui(&mut child, rect, open.as_deref(), &mut dcx)
}
