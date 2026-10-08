//! `Shell` — the top-level object. One [`Shell::frame`] per frame draws everything.
//!
//! The frame order is a fixed contract.
//!
//! ```text
//! Shell::frame(ui)
//!  1. now / dt — now on the real time between frames, dt = stable_dt capped at 50 ms
//!  2. handle.drain()        — the command queue, through mpsc try_recv (+ M2 notify/toast/dismiss/toggle_overlay)
//!  3. services.poll()       — pin the backend snapshots, gather next_wake
//!  4. access.tick()         — a temporary unlock running out · the session timeout · the idle lock ·
//!                             polling the authenticator while the prompt is up
//!  5. gestures.update()     — edge swipes and long presses (M2)
//!     guard.update()        — the edge_guard mask · the emergency gesture (chrome.emergency) · keep_awake
//!     overlay.update()      — consume the top pull (priority: the shade > the back gesture)
//!     gesture back          — the left edge → Workspace::*_gesture_back (A3)
//!     osk.update()          — wants_keyboard_input + ChromePolicy::osk (A5)
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

mod builder;
mod env_scale;
mod event;
mod gesture_regions;
mod handle;
mod layout;
pub(crate) mod slots;

pub use builder::ShellBuilder;
pub use event::ShellEvent;
pub use handle::{Command, ShellHandle};
pub use layout::Layout;
pub(crate) use layout::{compute_layout, edge_zones, LayoutInput};

use crate::access::prompt::{Prompt, PromptAction, PromptCx, Purpose};
use crate::access::{
    Access, AccessEvent, AuthMethod, AuthOutcome, Authenticator, ChangeReason, Credential, Gate,
    SessionChange,
};
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
    HeadsUp, HeadsUpAction, Notification, NotificationCenter, NotificationId, Toast, ToastAction,
    ToastQueue,
};
use crate::screen::{
    ActionDecl, BackAction, BarMode, CxParts, CxRequest, Decl, DeclKind, LaunchAction, LaunchMode,
    Lifecycle, PaneInfo, Registry, ScreenDecl,
};
use crate::services::Services;
use crate::settings::{keys, SettingKey, SettingValue, SettingsView};
use crate::theme::{Theme, ThemeFade};
use crate::workspace::{Instance, InstanceId, OverviewMode, SplitBlocker, Task, Workspace};
use slots::{OskSlot, OverlaySlot, OverlaySlotAction};
use std::time::{Duration, Instant};

/// For this long after a touch is released, pointer movement is not counted as a mouse — because a
/// touch emits pointer events along with it (`CursorPolicy::Auto`).
const TOUCH_GRACE: Duration = Duration::from_millis(400);

/// The shell.
#[allow(clippy::struct_excessive_bools)] // Independent switches, not a state machine — one flag each.
pub struct Shell {
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
    strings: Strings,
    /// The locale whose letters were last looked for in the loaded fonts.
    script_checked: Option<String>,
    handle: ShellHandle,
    rx: crate::inbox::Inbox<Command>,
    events: Vec<ShellEvent>,
    requests: Vec<CxRequest>,
    layout: Layout,
    frame_no: u64,
    last_dt: f32,
    /// The shell's monotonic time. It starts from `Shell::new`'s `Instant::now()` and **accumulates**
    /// `dt` each frame (the M1 version of `now = clock.now()`). Headless,
    /// `RawInput.time` advances by 1/60 at a time, so this value is the virtual time as it stands —
    /// `run_for(0.2)` really does imitate 200 ms.
    mono: Instant,
    /// How many frames `next_wake` has pointed into the past in a row (for diagnosing a backend fault).
    stale_wake_frames: u64,
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
    tap_press: Option<egui::Pos2>,
    /// The gesture regions — the handles and the integrator's own — in the
    /// order they were added: a later one is above an earlier one.
    gesture_regions: Vec<gesture_regions::RegionSlot>,
    /// This frame's region zones — what the engine was handed, kept for the guards and the
    /// drawing.
    region_zones: crate::gesture::RegionZones,
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

/// The cap on how many commands frame stage 2 handles at once. The handle's channel is unbounded, so
/// a producer thread faster than the UI would make one frame grow in proportion to the queue — past
/// the cap the rest is left to the next frame and an immediate repaint is requested (a p95 of
/// 12 ms).
const MAX_COMMANDS_PER_FRAME: usize = 256;

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
    egui::Panel::top(egui::Id::new("fairing.status_bar.panel"))
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
    egui::Area::new(egui::Id::new(STATUS_BAR_OVERLAY_AREA_ID))
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
    /// This frame's `ctx.egui_wants_keyboard_input()` (for scheduling the OSK debounce).
    wants_keyboard: bool,
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
        if self.registry.add(decl).is_some() {
            self.close_decl_instances(&id);
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
        if self.registry.remove(id).is_none() {
            // The registry → the status bar → **the quick-settings tiles**, in that order. A built-in
            // tile comes from the `[overlay] tiles` list rather than from a declaration, so the first
            // two could never catch it (once a defect: `shell.remove("tile.wifi")` quietly returned
            // `false`).
            if !self.status_bar.remove(id) && !self.remove_tile(id) {
                return false;
            }
            self.events.push(ShellEvent::DeclRemoved(id.to_owned()));
            return true;
        }
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

    /// Take the events that have built up.
    pub fn poll_events(&mut self) -> Vec<ShellEvent> {
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

    /// The shell's monotonic time (as of last frame). `Cx::now`, `evict_after` and the backends' `poll_at` all see the same value.
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

    /// **Replace the authenticator** while the shell runs — the same as
    /// [`ShellBuilder::authenticator`] afterwards. It is given the waker first.
    ///
    /// A prompt up at the time — the lock screen included — opens again for the new one: its
    /// methods, and nothing the old one owed or locked. The old one is told to
    /// [`cancel`](Authenticator::cancel) first. Where the new one offers nothing, the prompt goes,
    /// as in `routing`.
    pub fn set_authenticator(&mut self, authenticator: impl Authenticator + 'static) {
        if self.prompt.is_open() {
            // The one going out stops what it was doing: a wait ends with an answer or with
            // `cancel`, never by being dropped mid-check.
            let _ = self.prompt.take_wait();
            if let Some(old) = self.access.authenticator_mut() {
                old.cancel();
            }
        }
        let mut authenticator = Box::new(authenticator);
        authenticator.attach(self.handle.waker().clone());
        self.access.install_authenticator(authenticator);
        self.prompt.forget_lockout();
        let now = self.mono;
        let lock = self.prompt.is_lock_screen();
        let Some(purpose) = self.prompt.close(now) else {
            return;
        };
        let Some(methods) = self.prompt_methods() else {
            if lock {
                self.events
                    .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
            }
            return;
        };
        let (gate, hint) = match &purpose {
            Purpose::Unlock { gate, .. } => (gate.clone(), self.access.hint(gate)),
            Purpose::Lock => (crate::access::prompt::LOCK_GATE, None),
        };
        if let Some(auth) = self.access.authenticator_mut() {
            auth.begin(&gate, now);
        }
        self.prompt.open(purpose, methods, hint, now);
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
    pub fn notify(&mut self, mut notification: Notification) {
        if notification.at == crate::time::WallTime::default() {
            notification.at = self.services.clock.now();
        }
        let change = self.notifications.push(notification.clone());
        if change != crate::notify::CenterChange::Added {
            return;
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
                .show(&notification, self.mono, &self.theme.motion);
        }
    }

    /// A toast (directly, from the UI thread).
    pub fn toast(&mut self, toast: impl Into<Toast>) {
        self.toasts.push(toast.into());
    }

    /// Dismiss a notification. `false` for a persistent one.
    pub fn dismiss_notification(&mut self, id: NotificationId) -> bool {
        let ok = self.notifications.dismiss(id);
        if ok {
            self.events.push(ShellEvent::NotificationDismissed(id));
        }
        ok
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
        self.knock_in(id, &mut None);
    }

    /// [`Shell::knock`] **with the app's state in hand** — a hidden entry point whose action is a
    /// `Run` closure that reads it. See [`Shell::launch_with`].
    pub fn knock_with(&mut self, id: &str, app: &mut dyn std::any::Any) {
        self.knock_in(id, &mut Some(app));
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
                            self.tap_press = Some(*pos);
                        } else if let Some(press) = self.tap_press.take() {
                            taps.push(crate::access::Tap {
                                press,
                                release: *pos,
                                travel: (*pos - press).length(),
                            });
                        }
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

    /// Handle a knock's result: on an opening, check the gate; otherwise only the hint.
    fn after_knock(&mut self, idx: usize, step: crate::access::KnockStep, app: &mut AppRef<'_>) {
        use crate::access::KnockStep;
        let Some(entry) = self.hidden.get(idx) else {
            return;
        };
        match step {
            KnockStep::Opened => {
                let (id, gate, action) =
                    (entry.id.clone(), entry.gate.clone(), entry.action.clone());
                // **It does not open quietly.** Entering a service menu is auditable.
                self.events.push(ShellEvent::HiddenEntry { id });
                match gate {
                    Some(gate) if !self.access.allows(&gate) => {
                        self.request_unlock(gate, Some(action));
                    }
                    _ => self.launch_in(action, app),
                }
            }
            KnockStep::Idle => return,
            KnockStep::Advanced | KnockStep::Reset => {}
        }
        self.refresh_hidden_remaining();
    }

    /// Rebuild the snapshot `Cx::knock_remaining` reads.
    fn refresh_hidden_remaining(&mut self) {
        self.hidden_remaining.clear();
        for entry in &self.hidden {
            // A trigger with nothing to count has no hint either — `Cx::knock_remaining` becomes `None`.
            if let Some(left) = entry.trigger.remaining() {
                self.hidden_remaining.insert(entry.id.clone(), left);
            }
        }
    }

    /// **Actually carry out** the power operation announced by [`ShellEvent::PowerRequest`].
    ///
    /// The point is that the crate does not call this by itself — a device often has to close a valve
    /// and flush its logs before shutting down, and the crate cannot do that tidying for it. Receiving
    /// the event and **not calling this is the refusal**.
    ///
    /// # Errors
    ///
    /// A [`ServiceError`](crate::services::ServiceError) where the backend does not support it or it
    /// fails.
    pub fn commit_power(
        &mut self,
        request: crate::services::PowerRequest,
    ) -> crate::services::ServiceResult {
        self.services.power.request(request)
    }

    /// Decide whether to show the mouse cursor (`[shell] cursor`).
    ///
    /// **An arrow must not sit on a touch device.** But every way of operating one is in scope (an
    /// industrial HMI with a trackball or a USB mouse), so it cannot simply be removed either; by
    /// default it follows the input last used.
    ///
    /// A touch emits pointer events **as well**, so for a few tens of milliseconds after the hand
    /// leaves it looks like a mouse. Ignoring [`TOUCH_GRACE`]'s worth removes that illusion.
    fn update_cursor(&mut self, ctx: &egui::Context, now: Instant) {
        use crate::config::CursorPolicy;
        let hide = match self.config.shell.cursor {
            CursorPolicy::Visible => false,
            CursorPolicy::Hidden => true,
            CursorPolicy::Auto => {
                let (touching, moved) = ctx.input(|i| {
                    (
                        i.any_touches(),
                        i.pointer.is_moving() || i.smooth_scroll_delta != egui::Vec2::ZERO,
                    )
                });
                if touching {
                    self.last_touch = Some(now);
                    self.mouse_active = false;
                } else if moved
                    && self
                        .last_touch
                        .is_none_or(|t| now.saturating_duration_since(t) > TOUCH_GRACE)
                {
                    self.mouse_active = true;
                }
                !self.mouse_active
            }
        };
        if hide {
            // It **overrides** the cursor a widget set — doing it here rather than at the end of the
            // frame works because egui does not take the frame's last value as the output cursor and we
            // set it again every frame. A widget changing it afterwards shows for that frame only.
            ctx.set_cursor_icon(egui::CursorIcon::None);
        }
    }

    /// In a rail layout, open the first entry when at home.
    ///
    /// There is no kiosk with only a menu and nothing beside it. The home button therefore becomes "to
    /// the first screen" — emptying the workspace has this function open the first entry again straight
    /// away.
    ///
    /// It leaves a transition in progress alone — reopening mid-close makes the A2 transition jump.
    fn open_rail_home(&mut self, app: &mut AppRef<'_>) {
        if self.layout.rail.is_none() || !self.workspace.is_home() || self.workspace.is_animating()
        {
            return;
        }
        let Some(id) = self
            .desktop
            .first_rail_entry(&self.access)
            .map(str::to_owned)
        else {
            return;
        };
        self.launch_in(LaunchAction::open(id), app);
    }

    /// `[desktop] rail` → the layout input.
    ///
    /// The rail **stands aside on a fullscreen screen** — with `ChromePolicy::fullscreen` there is no
    /// rail, just as there are no bars. Even a kiosk has moments that use a whole screen, such as a
    /// payment-complete screen.
    fn rail_input(&self) -> Option<crate::shell::layout::RailInput> {
        use crate::shell::layout::RailInput;
        /// The rail width's default fraction.
        const DEFAULT_FRACTION: f32 = 0.26;
        // A screen with both bars hidden means to use the whole screen — the rail stands aside too.
        // Even a kiosk has moments that use a whole screen, such as a payment-complete screen.
        let policy = self.workspace.chrome_policy();
        if policy.status_bar == crate::screen::BarMode::Hide
            && policy.nav_bar == crate::screen::BarMode::Hide
        {
            return None;
        }
        let left = match self
            .config
            .desktop
            .rail
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "left" => true,
            "right" => false,
            "none" | "" => return None,
            other => {
                log::warn!(
                    "[desktop] rail = \"{other}\" is not a known value (none | left | right)"
                );
                return None;
            }
        };
        let fraction = if self.config.desktop.rail_width > 0.0 {
            self.config.desktop.rail_width.clamp(0.1, 0.5)
        } else {
            DEFAULT_FRACTION
        };
        Some(RailInput {
            left,
            fraction,
            min_width: self.theme.metrics.icon_cell,
        })
    }

    /// The settings read view.
    #[must_use]
    pub fn settings(&self) -> &crate::settings::SettingsView {
        &self.settings
    }

    /// Swap the wallpaper from a file. This is the path by which a shopkeeper chooses a photo
    /// from the settings screen.
    ///
    /// The decode is done by **the integrator loader** registered through
    /// [`ShellBuilder::image_loader`](crate::shell::ShellBuilder::image_loader) — the crate does not
    /// decode images. The texture that comes back is held by
    /// [`Wallpaper::Owned`](crate::desktop::Wallpaper::Owned), so the caller need not keep it.
    ///
    /// **Call it outside the frame budget.** A large original takes tens of milliseconds to decode, and
    /// calling it mid-paint overruns the frame.
    ///
    /// # Errors
    ///
    /// [`crate::Error::Config`] where there is no loader; where the loader fails, its error as it
    /// stands. **The current wallpaper is unchanged on a failure** — it is never left half swapped.
    pub fn load_wallpaper(
        &mut self,
        ctx: &egui::Context,
        path: impl AsRef<std::path::Path>,
        fit: crate::desktop::Fit,
    ) -> crate::Result<()> {
        let Some(loader) = self.image_loader.as_deref() else {
            return Err(crate::Error::Config(
                "no `ShellBuilder::image_loader` - the crate does not decode images \
                 (guide 09 §1)"
                    .to_owned(),
            ));
        };
        let texture = loader(ctx, path.as_ref())?;
        self.desktop
            .set_wallpaper(crate::desktop::Wallpaper::owned(texture, fit));
        Ok(())
    }

    /// Swap the dark/light palette pair. The palette in view is left alone — it takes effect
    /// from the next [`Shell::set_theme_dark`].
    ///
    /// Where the old behaviour is wanted (a toggle resetting to the neutral palette), it is one line:
    /// `set_palettes(Palette::dark(), Palette::light())`.
    pub fn set_palettes(&mut self, dark: crate::theme::Palette, light: crate::theme::Palette) {
        self.palettes = (dark, light);
    }

    /// The current pair, `(dark, light)`.
    #[must_use]
    pub fn palettes(&self) -> (&crate::theme::Palette, &crate::theme::Palette) {
        (&self.palettes.0, &self.palettes.1)
    }

    /// Change the current palette straight away, by cross-fade (the pair is left alone).
    pub fn apply_palette(&mut self, palette: crate::theme::Palette) {
        self.theme_fade.start(palette, self.theme.motion.theme_fade);
    }

    /// Whether an animation or transition was running last frame (for verifying the repaint policy).
    /// It looks at all three of the shell's animation store (`Cx::animate`, the nav bar's A7), the
    /// workspace's transitions (A2/A3) and the desktop's icon presses (A7) — the repaint policy is
    /// settled by this one value.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.animations.is_animating()
            || self.workspace.is_animating()
            || self.desktop.is_animating()
            || self.overlay.is_animating()
            || self.osk.is_animating()
            || self.toasts.is_animating()
            || self.heads_up.is_animating()
            || self.theme_fade.is_animating()
            || self.status_bar.is_animating(self.mono)
            || self.guard.is_animating()
            || self
                .prompt
                .is_animating(self.mono, self.theme.motion.reduce)
    }

    /// The frame number.
    #[must_use]
    pub fn frame_no(&self) -> u64 {
        self.frame_no
    }

    /// Last frame's dt (in seconds).
    #[must_use]
    pub fn last_dt(&self) -> f32 {
        self.last_dt
    }

    /// Register a vector icon (integrator extension).
    pub fn register_icon(&mut self, def: IconDef) -> CustomIconId {
        self.icons.register(def)
    }

    /// Register an arbitrary drawing-callback icon (`register_icon_painter`). Used as `IconRef::Custom(id)`.
    pub fn register_icon_painter(&mut self, painter: IconPainter) -> CustomIconId {
        self.icons.register_painter(painter)
    }

    /// An icon badge.
    pub fn set_badge(&mut self, id: &str, badge: Option<&Badge>) -> bool {
        self.desktop.set_badge(id, badge)
    }

    /// This frame's scale. The density comes from, in order: a code pin > the backend > the
    /// fallback.
    ///
    /// `set_zoom_factor` is called only on a frame where `pixels_per_point` changed — that call throws
    /// the font atlas away whole, so calling it every frame breaks idling at 0 fps.
    fn resolve_scale(&mut self, ctx: &egui::Context, root: egui::Rect) {
        use crate::unit::{Scale, ScaleConfidence, ScaleSource};

        let info = self.services.display.info();
        // A pin is a declaration that "this window is that panel", so the pixel count is taken from
        // **the root Rect × the committed ppp** rather than from the backend. The backend's `size_px`
        // may differ from the window's (a mock backend, several outputs), and that mismatch becomes a
        // density error as it stands.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "the screen size is positive and inside u32's range"
        )]
        let root_px = (
            (root.width() * self.committed_ppp).max(1.0) as u32,
            (root.height() * self.committed_ppp).max(1.0) as u32,
        );
        // **The density chain**. The integrator's pin, then the environment, then the
        // backend, then the fallback. `FAIRING_PPI` is already a density and so needs no rect;
        // `FAIRING_PHYSICAL_MM` sits in the pin's slot and is told apart by `env_physical_mm`,
        // because a size in millimetres only becomes a density once the panel's pixels are known.
        let (px_per_mm, source, confidence) =
            match (self.physical_mm_pin, self.env_px_per_mm, info.physical_mm) {
                // Where the integrator pinned it, that wins — an integrator knows their own device's inches.
                (Some((w_mm, h_mm)), _, _) => (
                    density(root_px, (w_mm, h_mm)),
                    if self.env_physical_mm {
                        ScaleSource::Env
                    } else {
                        ScaleSource::Pin
                    },
                    ScaleConfidence::Declared,
                ),
                // A person at the device, working around a file they cannot edit. Declared,
                // not measured: a number someone typed is a claim, however carefully they measured it.
                (None, Some(px_per_mm), _) => {
                    (px_per_mm, ScaleSource::Env, ScaleConfidence::Declared)
                }
                (None, None, Some(mm)) => (
                    density(info.size_px, mm),
                    ScaleSource::Backend,
                    ScaleConfidence::Measured,
                ),
                // Nothing is known. `Scale::resolve` folds to the policy's assumption and changes the source to Fallback.
                (None, None, None) => (f32::NAN, ScaleSource::Fallback, ScaleConfidence::Assumed),
            };
        let scale = Scale::resolve(
            px_per_mm,
            &self.scale_policy,
            source,
            confidence,
            root.size(),
        );

        if (scale.pixels_per_point - self.committed_ppp).abs() > 1e-4 {
            ctx.set_zoom_factor(scale.pixels_per_point);
            self.committed_ppp = scale.pixels_per_point;
            log::debug!(
                "[scale] ppp {:.3} - {:.2} px/mm - finger {:.1} mm - source {:?}/{:?}",
                scale.pixels_per_point,
                scale.px_per_mm,
                scale.finger_mm,
                scale.source,
                scale.confidence
            );
        }
        if scale.confidence == ScaleConfidence::Assumed && self.frame_no == 0 {
            log::warn!(
                "[scale] the panel size in millimetres is unknown - assuming {:.3} px/mm. Pin it with `ShellBuilder::physical_mm(w, h)` and touch targets become real millimetres",
                self.scale_policy.assume_px_per_mm
            );
        }
        self.scale = scale;
        // `Metrics` stays f32 — the 71 places that read it do not change. Only the values are resolved again at this scale.
        if let Some(spec) = self.metrics_spec.as_ref() {
            let resolved = spec.resolve(&scale);
            // The text sizes go out as egui `TextStyle`s, so a change here has to reach the style too.
            if resolved.type_scale != self.theme.metrics.type_scale {
                self.theme_dirty = true;
            }
            self.theme.metrics = resolved;
            self.status_bar.height = self.theme.metrics.status_bar_height;
            self.nav_bar.height = self.theme.metrics.nav_bar_height;
        }
        // **Everything sized in `Dim::text` is resolved against the type scale that actually
        // landed, not the one the policy implies.** `Scale::text_du` comes from
        // `ScalePolicy::viewing_distance_mm`, which is the supported way to move the text; but an
        // integrator may also state `MetricsSpec::type_scale` outright, and when they do, the badge,
        // the lamp, the gaps and the control heights have to go with it. Leaving them behind is
        // precisely the defect this term was added to close.
        let scale = scale.with_text_du(self.theme.metrics.type_scale.body);
        if let Some(spec) = self.component_spec.as_ref() {
            self.theme.components = spec.resolve(&scale);
        }
        // **The control lengths are resolved here too, and were not.** `theme.control` was filled
        // once from `ControlMetrics::default()` — which resolves against `Scale::identity()`, a
        // gloved 13 mm finger at one pixel per du — and never again. Every drawn control therefore
        // came out at the gloved size on every panel: `mark_size` is `finger(0.6)`, so a switch
        // measured 7.8 mm tall whether the device declared a 9 mm bare finger or nothing at all,
        // while the row around it followed the real scale. The two disagreed by 1.45x, measured.
        if let Some(spec) = self.control_spec.as_ref() {
            self.theme.control = spec.resolve(&scale);
        }
        // Same reason again: both elevation lengths are millimetres with a pixel floor, so a spec
        // frozen at `Scale::identity()` would cast one panel's penumbra on every panel.
        if let Some(spec) = self.elevation_spec.as_ref() {
            self.theme.elevation = spec.resolve(&scale);
        }
    }

    /// This frame's scale. For an integrator who wants to measure in physical units.
    #[must_use]
    pub fn scale(&self) -> &crate::unit::Scale {
        &self.scale
    }

    /// Once a frame. `ui` is the root `Ui` the runner or the integrator handed over (in egui 0.36 a
    /// panel takes a `Ui`).
    ///
    /// The stage order is the table in the module docs.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.frame_with(ui, &mut ());
    }

    /// **The same, with the app's own state lent to the screens for this frame**.
    ///
    /// The app owns its state *and* owns the shell; the shell is a field of the app, not a
    /// container for it. Once a frame the app lends what its screens need, and
    /// [`Cx::app`](crate::Cx::app) / [`app_mut`](crate::Cx::app_mut) /
    /// [`with_app`](crate::Cx::with_app) hand it on:
    ///
    /// ```no_run
    /// # struct Console;
    /// struct App {
    ///     shell: fairing::Shell,
    ///     console: Console,
    /// }
    ///
    /// impl App {
    ///     fn frame(&mut self, ui: &mut egui::Ui) {
    ///         self.shell.frame_with(ui, &mut self.console);
    ///     }
    /// }
    /// ```
    ///
    /// One value goes over — an app with several things to share hands over one `struct` holding
    /// them. Nothing is captured by a screen and nothing is stored by the shell, so there is no
    /// `Rc<RefCell<_>>` between them and no `'static` bound on the state.
    ///
    /// [`Shell::frame`] is this with nothing lent; screens then see `cx.app::<T>() == None`.
    pub fn frame_with(&mut self, ui: &mut egui::Ui, app: &mut dyn std::any::Any) {
        self.frame_inner(ui, &mut Some(app));
    }

    fn frame_inner(&mut self, ui: &mut egui::Ui, app: &mut AppRef<'_>) {
        let started = Instant::now();
        let ctx = ui.ctx().clone();
        // **Nothing is drawn until this crate's font families are live.**
        //
        // `ShellBuilder::build` installs the fonts, but `Context::set_fonts` does not apply in the
        // pass it is called from — egui swaps the definitions in at the start of the *next* pass.
        // Build the shell from inside a frame (`App::update` does exactly that, and so does the
        // examples' tour) and for that one pass `fonts::STRONG_FAMILY` is not bound. Asking for an
        // unbound family is not a fallback in epaint, it is `panic!`, so the first `Theme::strong`
        // draw takes the process down — which is how the kiosk example lost every screenshot: its
        // first screen is a grid of `MediaCard`s and a card titles itself in the strong face.
        //
        // Skipping is allowed **only before the first frame**, which is the only moment the window
        // can legitimately be open. Later, an unbound family is the integrator's own `set_fonts`
        // clobbering ours, and a blank screen forever would hide it far worse than epaint saying so.
        if self.frame_no == 0 && !crate::fonts::families_are_live(&ctx) {
            ctx.request_repaint();
            return;
        }
        // 0. The scale. Density → `pixels_per_point` → `Metrics`. Fixed for the whole of this frame.
        self.resolve_scale(&ctx, ui.max_rect());
        self.check_added_nav_items();
        // 1. Two clocks. Animations step by `stable_dt`, capped at 50 ms
        // so a stall never jumps one. The shell's monotonic time — every deadline: a temporary
        // unlock, the session timeout, the idle lock, a lockout, a toast's hold — runs on the
        // **real** time between frames. On an idle panel egui reports `stable_dt` as its 1/60 s
        // prediction however long it slept, so deadlines counted in it ran many times slower than
        // the wall clock: a five-minute unlock outlived the operator by hours. Headless,
        // `RawInput::time` is the harness's virtual time, so tests stay deterministic.
        self.frame_no += 1;
        self.check_script(&ctx);
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, MAX_DT);
        self.last_dt = dt;
        let elapsed = ui.input(|i| i.unstable_dt);
        let elapsed = Duration::try_from_secs_f32(elapsed.max(0.0))
            .unwrap_or_else(|_| Duration::from_secs_f32(dt));
        self.mono = self.mono.checked_add(elapsed).unwrap_or(self.mono);
        let now = self.mono;
        let input_activity =
            ui.input(|i| i.pointer.any_down() || i.events.iter().any(is_user_input));
        self.update_cursor(&ctx, now);
        // 2.
        let commands_pending = self.drain_commands(app);
        // 3.
        let next_wake = self.services.poll(now);
        // 4.
        self.tick_access(now, input_activity, app);
        // 5.
        let screen = ui.max_rect();
        let wants_keyboard = ctx.egui_wants_keyboard_input();
        self.update_chrome(&ctx, screen, wants_keyboard, now, dt, app);
        // 6.
        let policy = self.workspace.chrome_policy();
        let inset = self.osk.inset_bottom();
        let was = self.layout.content;
        self.layout = compute_layout(&LayoutInput {
            screen,
            policy,
            status_height: self.status_bar.enabled.then_some(self.status_bar.height),
            nav_height: self.nav_bar.enabled.then_some(self.nav_bar.height),
            edge_px: self.theme.metrics.edge_px,
            osk_height: (inset > 0.5).then_some(inset),
            rail: self.rail_input(),
        });
        self.workspace.set_inset_bottom(inset);
        // **Say so when a Pane grows or shrinks**. A screen rotation, a window resize and
        // a transition to a screen with the chrome hidden all come in here. The first frame
        // (`Rect::NOTHING`) is excluded — the instance receives a `Created` first there, so the size can
        // be read from that.
        //
        // The OSK does not shrink `content` (the keyboard does not push the content and only
        // says so through `inset_bottom`). So raising and lowering the keypad does not leak this event.
        let now_content = self.layout.content;
        if was.is_positive() && was.size() != now_content.size() {
            // Each pane's instances hear their own size — the content's, or their share of it.
            self.workspace.content_resized(now_content);
        }
        self.open_rail_home(app);
        let layout = self.layout;
        // 7–9.
        let (out, nav_action, status_action) = self.draw_chrome(ui, &layout, now, app);
        // The screens the overview closed (a card thrown away, "Close all").
        for (id, instance) in &out.closed {
            self.events.push(ShellEvent::ScreenClosed {
                id: id.clone(),
                instance: *instance,
            });
        }
        // 10–13.
        let late = self.draw_late(&ctx, &layout, screen, now, app);
        self.guard_regions(&ctx);
        self.paint_regions(&ctx, now);
        self.finish_frame(
            &ctx,
            FrameActions {
                desktop: out.desktop_action,
                nav: nav_action,
                status: status_action,
            },
            late,
            FrameWake {
                next_wake,
                now,
                commands_pending,
                wants_keyboard,
            },
            app,
        );
        // 16. The events go out through poll_events.
        self.trim_events();
        let elapsed = started.elapsed();
        if elapsed > SLOW_FRAME {
            log::warn!(
                "slow frame #{}: {:.1} ms (the budget is 33 ms)",
                self.frame_no,
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }

    /// Stage 5: gestures → the policy guard → the overlay → the back gesture → the OSK → advancing the
    /// animations → evict · rebuild → the overlay's boundary lifecycle.
    /// The things that run every frame (animations, transitions, the notification queue). Split out
    /// from [`Self::update_chrome`] — inlined there, one function would go over the length cap.
    fn tick_animations(&mut self, dt: f32, now: Instant, tokens: &crate::theme::MotionTokens) {
        self.animations.tick(dt, self.frame_no);
        self.workspace.tick(dt);
        self.desktop.tick(dt);
        self.toasts
            .tick(dt, now, tokens, self.theme.components.toast);
        self.heads_up.tick(dt, now, tokens);
    }

    fn update_chrome(
        &mut self,
        ctx: &egui::Context,
        screen: egui::Rect,
        wants_keyboard: bool,
        now: Instant,
        dt: f32,
        app: &mut AppRef<'_>,
    ) {
        let tokens = self.theme.motion;
        let policy = self.workspace.chrome_policy();
        let gesture = self.read_gestures(ctx, screen, &policy, now);
        let modal = self.prompt.is_open();
        let gesture = self.behind_the_prompt(gesture);
        self.desktop_presses(ctx, gesture, &tokens);
        let guard = self
            .guard
            .update(&policy, screen, self.gestures.hold(), now);
        if guard.emergency && !modal {
            self.emergency();
        }
        if let Some(on) = self
            .guard
            .apply_keep_awake(&policy, self.services.display.as_mut())
        {
            log::debug!("keep_awake → DisplayBackend::set_idle_inhibit({on})");
        }
        // The hidden entry points' **corner knocks**. Caught from the raw pointer rather than
        // from a widget, so they work whatever a screen draws — on a kiosk screen with all the chrome
        // hidden too. They come **after** the gesture decision, so they do not intercept an edge swipe —
        // only a corner touched and released straight away is a knock. Behind the prompt the
        // corners are glass like everything else.
        if modal {
            self.last_screen = screen;
        } else {
            self.poll_knocks(ctx, screen, now, app);
        }
        // The overlay takes the top pull first (the priority: the shade > a screen transition > a page).
        let status_hidden = policy.status_bar == BarMode::Hide && self.status_bar.enabled;
        let allowed = self.access.allows(&OVERLAY_OPEN_GATE);
        let consumed = self.overlay.update(
            gesture,
            overlay_screen(&self.layout),
            self.layout.content,
            status_hidden,
            self.status_bar.height,
            allowed,
            now,
            dt,
            &tokens,
        );
        if consumed {
            // Once the shade starts, the lower priorities (a screen transition, a page
            // swipe) are finished at once — so that two do not run together.
            if matches!(
                gesture,
                Some(Gesture::EdgeSwipe {
                    phase: Phase::Started,
                    ..
                })
            ) {
                self.workspace.settle_transitions();
                self.desktop.settle_page();
            }
        } else {
            self.gesture_back_in_pane(ctx, gesture, screen);
            self.bottom_gestures(ctx, gesture);
        }
        // The touch in a region, every frame to its end — one the regions stopped being in play
        // under is told it was taken.
        self.region_gestures(ctx, gesture, now, app);
        // OSK (A5). The prompt is the shell's own, so the screen's keyboard policy does not
        // reach it — a screen that turned the keyboard off must not leave a password card mute.
        let osk_mode = if modal {
            crate::screen::OskMode::Auto
        } else {
            policy.osk
        };
        // The row cap is in millimetres, so it is this frame's resolved value.
        self.osk.set_max_key(self.theme.metrics.osk_max_key);
        self.osk
            .update(wants_keyboard, osk_mode, screen.height(), now, dt, &tokens);
        // Advance the animations and transitions.
        self.tick_animations(dt, now, &tokens);
        if let Some(palette) = self.theme_fade.tick(dt) {
            self.theme.palette = palette;
            self.theme_dirty = true;
        }
        // **Outside the crossfade branch.** It used to sit inside it, so the only thing that could
        // re-apply the theme was a palette animation — a metric or a text size changed at run time
        // never reached egui's style.
        if std::mem::take(&mut self.theme_dirty) {
            self.theme.apply(ctx);
        }
        for (id, instance) in self.workspace.evict_expired(now, &tokens) {
            self.events.push(ShellEvent::ScreenClosed { id, instance });
        }
        if self.registry.take_dirty() {
            self.desktop.rebuild(&self.registry, &self.config.desktop);
            self.overlay.mark_tiles_dirty();
        }
        self.note_cover();
        let shown = self.osk.is_shown();
        if shown != self.osk_was_shown {
            self.osk_was_shown = shown;
            self.events.push(ShellEvent::OskToggled(shown));
        }
        let split = self.workspace.is_split();
        if split != self.split_was {
            self.split_was = split;
            self.events.push(ShellEvent::SplitToggled(split));
        }
        self.status_bar
            .set_unread(u32::try_from(self.notifications.unread()).unwrap_or(u32::MAX));
    }

    /// Stage 5's first step: the gesture engine's reading of this frame. A run of quick switches
    /// ends here too, on a press anywhere but the indicator's edge.
    fn read_gestures(
        &mut self,
        ctx: &egui::Context,
        screen: egui::Rect,
        policy: &crate::screen::ChromePolicy,
        now: Instant,
    ) -> Option<Gesture> {
        // A1: a visible status bar is a top edge zone throughout (symmetrically, the bottom is
        // the nav bar). Last frame's layout is enough, and so is last frame's policy for
        // the policy guard's mask (it is decided within the same frame).
        let zones = edge_zones(
            screen,
            self.theme.metrics.edge_px,
            self.layout.status.map_or(0.0, |r| r.height()),
            self.layout.nav.map_or(0.0, |r| r.height()),
        );
        let blocked = PolicyDriver::blocked_edges(policy);
        self.region_zones = self.place_regions(screen, zones, blocked);
        let gesture = self.gestures.update(
            ctx,
            &GestureFrame {
                screen,
                edge_zones: zones,
                blocked,
                slide_edges: self.slide_edges(),
                regions: self.region_zones,
                now,
            },
        );
        if self.gesture_nav() {
            let elsewhere = ctx.input(|i| {
                i.pointer.any_pressed()
                    && crate::drag::press_point(i).is_some_and(|at| {
                        zones
                            .get(Edge::Bottom.index())
                            .is_some_and(|z| !z.contains(at))
                    })
            });
            if elsewhere {
                self.workspace.end_quick_switch();
            }
        }
        gesture
    }

    /// The desktop's share of this frame's input: a press over the info popover puts it
    /// away, a long press goes to the cells about to be drawn, and the popover goes the moment
    /// something takes the desktop's place — the shade, the prompt, a transition, the cards.
    fn desktop_presses(
        &mut self,
        ctx: &egui::Context,
        gesture: Option<Gesture>,
        tokens: &crate::theme::MotionTokens,
    ) {
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.desktop.note_press(tokens);
        }
        if let Some(Gesture::LongPress { pos }) = gesture {
            self.desktop.hold_at(pos);
        }
        let covered = !self.overlay.is_closed()
            || self.prompt.is_open()
            || self.workspace.is_animating()
            || self.workspace.is_overview_open();
        if covered {
            self.desktop.close_info(tokens);
        }
    }

    /// **Behind the prompt nothing new starts** (A9: the prompt is the top priority). A
    /// swipe that was already running when it opened still gets its end, so the shade or the back
    /// transition can settle instead of hanging mid-drag.
    fn behind_the_prompt(&mut self, gesture: Option<Gesture>) -> Option<Gesture> {
        match gesture {
            Some(
                Gesture::EdgeSwipe {
                    phase: Phase::Started,
                    ..
                }
                | Gesture::EdgeSlide {
                    phase: Phase::Started,
                    ..
                },
            ) if self.prompt.is_open() => {
                self.gestures.cancel();
                None
            }
            other => other,
        }
    }

    /// The overlay's `Closed` boundary, and the focused screen's cover: Paused while
    /// anything covers it — the shade leaving `Closed`, or the prompt — and Resumed once
    /// nothing does. One boundary for both, so a prompt over an open shade does not pause twice.
    fn note_cover(&mut self) {
        let closed = self.overlay.is_closed();
        if closed != self.overlay_was_closed {
            self.overlay_was_closed = closed;
            if !closed {
                self.notifications.mark_seen();
                self.heads_up.absorb();
            }
            self.events.push(ShellEvent::OverlayToggled(!closed));
        }
        let covered = !closed || self.prompt.is_open();
        if covered != self.focus_covered {
            self.focus_covered = covered;
            // Under the cards the screens stay paused when the shade or the prompt goes — the
            // overview resumes them itself, when it goes.
            if covered || !self.workspace.is_overview_open() {
                self.workspace.queue_focused(if covered {
                    Lifecycle::Paused
                } else {
                    Lifecycle::Resumed
                });
            }
        }
    }

    /// Whether the nav bar is gestures (`[nav_bar] style = "gesture"`). The gestures
    /// come with the bar: one turned off (`enabled = false`) takes them with it, as it does its
    /// buttons — a screen hiding the bar (`ChromePolicy`) does not.
    fn gesture_nav(&self) -> bool {
        self.nav_bar.enabled && matches!(self.nav_bar.style, NavStyle::Gesture { .. })
    }

    /// The edges a slide along them is recognised on: the bottom, where the nav bar is gestures —
    /// the home indicator's left and right.
    fn slide_edges(&self) -> EdgeMask {
        if self.gesture_nav() {
            EdgeMask::NONE.with(Edge::Bottom)
        } else {
            EdgeMask::NONE
        }
    }

    /// **The bottom edge's gestures**, where the nav bar is gestures: up is home — the
    /// screen on show following the finger — up and a pause is the recent screens, and along the
    /// home indicator is the task used before or after the one on show.
    fn bottom_gestures(&mut self, ctx: &egui::Context, gesture: Option<Gesture>) {
        if !self.gesture_nav() {
            return;
        }
        // Over an open shade the press is the shade's; on the on-screen keyboard's keys — where
        // the edge zone is deeper than the band the keyboard sits on — the keys'.
        let starting = matches!(
            gesture,
            Some(
                Gesture::EdgeSwipe {
                    edge: Edge::Bottom,
                    phase: Phase::Started,
                    ..
                } | Gesture::EdgeSlide {
                    edge: Edge::Bottom,
                    phase: Phase::Started,
                    ..
                }
            )
        );
        if starting && (!self.overlay.is_closed() || self.press_on_keys(ctx)) {
            self.gestures.cancel();
            return;
        }
        match gesture {
            Some(Gesture::EdgeSwipe {
                edge: Edge::Bottom,
                progress,
                velocity,
                phase,
            }) => self.bottom_swipe(ctx, progress, velocity, phase),
            Some(Gesture::SwipeHold { edge: Edge::Bottom }) => self.bottom_hold(),
            Some(Gesture::EdgeSlide {
                edge: Edge::Bottom,
                offset,
                velocity,
                phase,
            }) => self.indicator_slide(offset, velocity, phase),
            _ => {}
        }
    }

    /// Whether this press went down on the on-screen keyboard's keys.
    fn press_on_keys(&self, ctx: &egui::Context) -> bool {
        let press = ctx.input(|i| i.pointer.press_origin());
        self.osk.is_shown()
            && self
                .layout
                .osk
                .is_some_and(|keys| press.is_some_and(|at| keys.contains(at)))
    }

    /// Up from the bottom edge. Over a task the screen on show follows the finger (the lift); at
    /// home and over the cards nothing does, and the release alone says what it was. Let go far
    /// enough up, or flung up, it is home; short of that, the screen goes back down.
    fn bottom_swipe(&mut self, ctx: &egui::Context, progress: f32, velocity: f32, phase: Phase) {
        let tokens = self.theme.motion;
        match phase {
            Phase::Started => {
                self.bottom_held = false;
                if !self.workspace.is_home() && !self.workspace.is_overview_open() {
                    let press = ctx.input(|i| i.pointer.press_origin());
                    if let Some(press) = press {
                        let travel = self.lift_travel();
                        let _ = self.workspace.begin_lift(press, travel);
                    }
                }
                self.follow_lift(ctx, velocity);
            }
            Phase::Moved => self.follow_lift(ctx, velocity),
            Phase::Ended | Phase::Cancelled => {
                if std::mem::take(&mut self.bottom_held) {
                    return;
                }
                let rule = ReleaseRule {
                    snap_ratio: tokens.snap_ratio,
                    fling: tokens.fling_px_s,
                };
                let home =
                    phase == Phase::Ended && rule.confirm(progress / self.lift_travel(), velocity);
                if home {
                    self.home_from_swipe();
                } else {
                    self.workspace.lift_back(&tokens);
                }
            }
        }
    }

    /// The lift follows the finger: its offset from where it went down.
    fn follow_lift(&mut self, ctx: &egui::Context, velocity_up: f32) {
        if !self.workspace.is_lifting() {
            return;
        }
        let at = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.press_origin()));
        if let (Some(pos), Some(press)) = at {
            self.workspace.drag_lift(pos - press, velocity_up);
        }
    }

    /// A bottom swipe let go into home: a lifted screen carries on into its icon; anything else
    /// is the home button's.
    fn home_from_swipe(&mut self) {
        if !self.workspace.is_lifting() {
            self.home();
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
        }
        let icon_rect = self
            .workspace
            .active_task()
            .and_then(|t| t.root_id().map(str::to_owned))
            .and_then(|id| self.desktop.icon_rect(&id));
        if self.workspace.lift_home(icon_rect, &self.theme.motion) {
            self.events.push(ShellEvent::WentHome);
        }
    }

    /// Up and a pause: the recent screens, as the recents button — reported, gated, and only
    /// where `[workspace] overview` is on — the lifted screen carrying on into its card. A pause
    /// too low down is not one yet: the swipe may still pause again further up.
    fn bottom_hold(&mut self) {
        if self.workspace.is_lifting() && self.workspace.lift_progress() < LIFT_HOLD_MIN {
            return;
        }
        if self.bottom_held || self.workspace.is_overview_open() {
            return;
        }
        self.bottom_held = true;
        self.events.push(ShellEvent::OverviewRequested);
        let tokens = self.theme.motion;
        if !self.config.workspace.overview {
            self.workspace.lift_back(&tokens);
            return;
        }
        if !self.access.allows(&RECENTS_GATE) {
            self.workspace.lift_back(&tokens);
            self.request_unlock(RECENTS_GATE, Some(LaunchAction::OpenOverview));
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&tokens);
        }
        let mode = OverviewMode::Recents {
            beside: self.may_enter_split(),
        };
        if self.workspace.is_lifting() {
            self.workspace.lift_to_overview(mode, &self.theme);
        } else {
            self.workspace.open_overview(mode, &tokens);
        }
    }

    /// Along the home indicator: the screen on show slides away and the task used before it
    /// (to the right) or after it (to the left) slides in. At home, a slide brings back the task
    /// used last. With nothing to go to, the press is let go.
    fn indicator_slide(&mut self, offset: f32, velocity: f32, phase: Phase) {
        let tokens = self.theme.motion;
        let width = self.workspace.focused_pane_rect().width().max(1.0);
        match phase {
            Phase::Started => {
                if !self.workspace.is_home() && !self.workspace.begin_switch(width) {
                    self.gestures.cancel();
                    return;
                }
                self.workspace.drag_switch(offset, velocity);
            }
            Phase::Moved => self.workspace.drag_switch(offset, velocity),
            Phase::Ended | Phase::Cancelled => {
                let rule = ReleaseRule {
                    snap_ratio: tokens.snap_ratio,
                    fling: tokens.fling_px_s,
                };
                let through = phase == Phase::Ended
                    && rule.confirm(offset.abs() / width, velocity * offset.signum());
                if self.workspace.is_switching() {
                    self.workspace.release_switch(through, &tokens);
                } else if through && self.workspace.is_home() {
                    self.resume_most_recent();
                }
            }
        }
    }

    /// The task used last, back on show — from home, zooming out of its icon where it has one.
    fn resume_most_recent(&mut self) {
        let Some(index) = self.workspace.most_recent_task() else {
            return;
        };
        let icon_rect = self
            .workspace
            .tasks()
            .get(index)
            .and_then(Task::root_id)
            .and_then(|id| self.desktop.icon_rect(id));
        let _ = self
            .workspace
            .resume_task(index, icon_rect, &self.theme.motion);
    }

    /// How far up a lift reaches the card scale (A11's `D`): 35 % of the
    /// pane's height, between 15 and 60 mm, and no more than 60 % of the screen's short side.
    fn lift_travel(&self) -> f32 {
        let pane = self.workspace.focused_pane_rect();
        let mm = self.scale.du_per_mm.max(0.1);
        let short = self.last_screen.width().min(self.last_screen.height());
        (LIFT_TRAVEL_RATIO * pane.height())
            .clamp(LIFT_TRAVEL_MIN_MM * mm, LIFT_TRAVEL_MAX_MM * mm)
            .min(LIFT_TRAVEL_SHORT_SIDE * short)
            .max(1.0)
    }

    /// The back gesture, where the shade did not take it. Split, it is the pane's it starts on,
    /// measured across that pane.
    fn gesture_back_in_pane(
        &mut self,
        ctx: &egui::Context,
        gesture: Option<Gesture>,
        screen: egui::Rect,
    ) {
        // Over the cards a back swipe is back: they go down, and nothing under them is popped —
        // the screen behind a card is not on show to be swiped away.
        if self.workspace.is_overview_open() {
            if let Some(Gesture::EdgeSwipe {
                edge,
                phase: Phase::Started,
                ..
            }) = gesture
            {
                if self.back_edges.contains(edge) && !matches!(edge, Edge::Top | Edge::Bottom) {
                    self.gestures.cancel();
                    self.workspace.close_overview(&self.theme.motion);
                }
            }
            return;
        }
        let width = if self.workspace.is_split() {
            if matches!(
                gesture,
                Some(Gesture::EdgeSwipe {
                    phase: Phase::Started,
                    ..
                })
            ) {
                if let Some(at) = ctx.input(crate::drag::press_point) {
                    self.workspace.focus_at(ctx, at);
                }
            }
            self.workspace.focused_pane_rect().width()
        } else {
            screen.width()
        };
        self.gesture_back(gesture, width);
    }

    /// The back gesture (A3, M2): it hands an edge swipe on one of `back_edges` to the workspace.
    /// `p = dx / W`, confirmed at `p ≥ snap_ratio` or `v_x ≥ fling`.
    fn gesture_back(&mut self, gesture: Option<Gesture>, width: f32) {
        let Some(Gesture::EdgeSwipe {
            edge,
            progress,
            velocity,
            phase,
        }) = gesture
        else {
            return;
        };
        if !self.back_edges.contains(edge) || matches!(edge, Edge::Top | Edge::Bottom) {
            return;
        }
        let w = width.max(1.0);
        // A3 "taking hold again mid-cancel": the new press's `dx / W` is added to the `p` the
        // workspace caught — and the release decision goes by that sum (the real progress) too.
        let p = (self.workspace.gesture_back_grab() + progress / w).clamp(0.0, 1.0);
        match phase {
            Phase::Started => {
                if !self.workspace.begin_gesture_back() {
                    self.gestures.cancel();
                    return;
                }
                self.workspace.drag_gesture_back(p, velocity / w);
            }
            Phase::Moved => self.workspace.drag_gesture_back(p, velocity / w),
            Phase::Ended | Phase::Cancelled => {
                let tokens = self.theme.motion;
                let rule = ReleaseRule {
                    snap_ratio: tokens.snap_ratio,
                    fling: tokens.fling_px_s,
                };
                let confirm = phase == Phase::Ended && rule.confirm(p, velocity);
                let closing = self
                    .workspace
                    .focused()
                    .map(|i| (i.decl_id().to_owned(), i.id()));
                self.workspace.release_gesture_back(confirm, &tokens);
                if confirm {
                    if let Some((id, instance)) = closing {
                        self.events.push(ShellEvent::ScreenClosed { id, instance });
                    }
                }
            }
        }
    }

    /// The emergency gesture completing: where `chrome.emergency` passes it opens the shade,
    /// otherwise an unlock request — the prompt, in `prompt` mode.
    fn emergency(&mut self) {
        let gate = Gate::borrowed(EMERGENCY_GATE);
        self.events.push(ShellEvent::Emergency);
        if self.access.allows(&gate) {
            self.overlay.open(&self.theme.motion);
        } else {
            self.request_unlock(gate, Some(LaunchAction::OpenOverlay));
        }
    }

    /// Stages 7–9: the status bar (a panel, or an Overlay `Area`) · the nav bar · the workspace. It
    /// splits the fields up to make a `CxParts` (the panel closures borrow different fields).
    fn draw_chrome(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        now: Instant,
        app: &mut AppRef<'_>,
    ) -> (
        crate::workspace::WorkspaceOutput,
        Option<NavAction>,
        Option<StatusBarAction>,
    ) {
        let ctx = ui.ctx().clone();
        let root = ui.max_rect();
        let frame_no = self.frame_no;
        let status_opacity = self.status_opacity();
        let Self {
            status_bar,
            nav_bar,
            workspace,
            desktop,
            services,
            theme,
            access,
            registry,
            animations,
            icons,
            widget_painters,
            settings,
            strings,
            handle,
            requests,
            hidden_remaining,
            status_bar_painter,
            nav_bar_painter,
            nav_bar_layout,
            recents_painters,
            bar_rows,
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
            frame: frame_no,
        };
        let mut nav_action = None;
        let mut status_action = None;
        if let Some(rect) = layout.status {
            status_action = draw_status_bar(
                ui,
                &ctx,
                rect,
                layout.status_overlay,
                status_opacity,
                status_bar,
                &mut parts,
                registry,
                bar_rows,
                status_bar_painter.as_mut(),
            );
        } else {
            status_bar.mark_hidden();
        }
        if let Some(rect) = layout.nav {
            // Back is ignored on the desktop; this shows that as the back button's disabled look.
            nav_bar.back_enabled = !workspace.is_home();
            if nav_bar_painter.is_some() {
                nav_bar.collect_items(registry.nav_items(), parts.access, bar_rows);
            }
            egui::Panel::bottom(egui::Id::new("fairing.nav_bar.panel"))
                .exact_size(rect.height())
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    if let Some(painter) = nav_bar_painter.as_mut() {
                        nav_bar.ui_with(ui, &mut parts, bar_rows, painter);
                    } else {
                        let items = registry.nav_items_mut();
                        nav_action = nav_bar.ui(ui, &mut parts, items, nav_bar_layout.as_mut());
                    }
                });
        } else {
            nav_bar.mark_hidden();
        }
        // The rail is **outside** the workspace — it is drawn every frame and a screen does not cover
        // it. That is the only difference from the grid, and it is all there is to the kiosk layout.
        let rail_action = layout
            .rail
            .and_then(|rect| draw_rail(ui, rect, desktop, workspace, &mut parts));
        let mut out = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                workspace.ui(ui, &mut parts, registry, desktop, layout, recents_painters)
            })
            .inner;
        // A rail tap wins — where the grid was pressed on the same frame, it is hidden behind the rail and out of sight.
        if rail_action.is_some() {
            out.desktop_action = rail_action;
        }
        draw_icon_info(&ctx, root, layout, desktop, &mut parts);
        (out, nav_action, status_action)
    }

    /// The status bar's opacity this frame. The A1 fade means something only in the layout where
    /// the shade covers the status bar (`BarMode::Overlay`); a bar with a band of its own stays sharp
    /// to the end, since the shade comes down below it.
    fn status_opacity(&self) -> f32 {
        if self.layout.status_overlay {
            self.overlay.status_bar_opacity()
        } else {
            1.0
        }
    }

    /// Stages 10–13: the OSK → the overlay (the scrim, the panel, the status row/peek, the shield) →
    /// the prompt → the heads-up and the toasts. `root` is the whole screen.
    fn draw_late(
        &mut self,
        ctx: &egui::Context,
        layout: &Layout,
        root: egui::Rect,
        now: Instant,
        app: &mut AppRef<'_>,
    ) -> LateOutput {
        let frame_no = self.frame_no;
        let status_opacity = 1.0;
        let screen = overlay_screen(layout);
        let Self {
            status_bar,
            workspace: _,
            services,
            theme,
            access,
            registry,
            animations,
            icons,
            widget_painters,
            settings,
            strings,
            handle,
            requests,
            hidden_remaining,
            status_bar_painter,
            bar_rows,
            overlay,
            osk,
            notifications,
            toasts,
            heads_up,
            gestures,
            guard,
            prompt,
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
            frame: frame_no,
        };
        // 10. While the prompt is up the keyboard is drawn after it instead, above it.
        let modal = prompt.is_open();
        let mut late = LateOutput {
            osk_hide: !modal && osk.ui(ctx, layout.osk, parts.theme, parts.icons),
            ..LateOutput::default()
        };
        // 11.
        #[cfg(feature = "overlay")]
        let tiles = registry.tiles();
        #[cfg(not(feature = "overlay"))]
        let tiles: &[()] = &[];
        let out = overlay.ui(ctx, &mut parts, screen, notifications, tiles);
        late.overlay = out.action;
        if let Some(rect) = out.status_row.or(out.peek) {
            // The status row is panel content, so what is outside the curtain (the panel's visible area) is not revealed yet.
            let clip = out.panel.map_or(rect, |panel| rect.intersect(panel));
            let action = draw_status_bar_area(
                ctx,
                rect,
                clip,
                status_opacity,
                status_bar,
                &mut parts,
                registry,
                bar_rows,
                status_bar_painter.as_mut(),
            );
            if action.is_some() {
                late.status_row_tapped = true;
            }
            if out.status_row.is_some() {
                // egui 0.36.1's `Area::begin` raises a pressed Area to the top and `end_pass` sorts
                // stably — where `fairing.status_bar.overlay` has a history of being made before the
                // panel (a `BarMode::Overlay` screen → a `Hide` screen), the status row can end up below
                // the panel, so it is pinned just above the panel on every pass.
                OverlaySlot::pin_status_row_above_panel(ctx, STATUS_BAR_OVERLAY_AREA_ID);
            }
        }
        gestures.shield(ctx);
        // The emergency gesture's progress ring (the A7 long-press convention) — a draw-only layer that takes no input.
        guard.ui(ctx, parts.theme);
        // 12. The prompt, over everything but the heads-up and the toasts (z-order 5).
        late.prompt = draw_prompt(ctx, prompt, &mut parts, layout, root);
        if modal {
            // The password card's fields want the keyboard, and the keyboard has to be **above**
            // the modal that is taking every press — so it goes up a layer for as long as the
            // prompt is up.
            osk.raise(true);
            late.osk_hide = osk.ui(ctx, layout.osk, parts.theme, parts.icons);
            osk.raise(false);
        }
        // 13.
        let tokens = parts.theme.motion;
        late.heads_up = heads_up.ui(
            ctx,
            screen,
            parts.theme,
            &tokens,
            now,
            parts.icons,
            parts.strings,
        );
        let anchor = toast_anchor(layout, parts.theme);
        late.toast = toasts.ui(ctx, anchor, parts.theme, parts.icons, parts.strings);
        late
    }

    /// Stages 14–15: handle the outputs and requests, propagate the lifecycle, apply the repaint policy.
    fn finish_frame(
        &mut self,
        ctx: &egui::Context,
        actions: FrameActions,
        late: LateOutput,
        wake: FrameWake,
        app: &mut AppRef<'_>,
    ) {
        let FrameActions {
            desktop: desktop_action,
            nav: nav_action,
            status: status_action,
        } = actions;
        // 14.
        match status_action {
            Some(StatusBarAction::ItemTapped(id)) => {
                if let Some(action) = self.status_bar.tap_action(&id).cloned() {
                    self.launch_in(action, app);
                }
            }
            Some(StatusBarAction::Tapped) if self.status_bar.tap_opens_shade => {
                // An open split shade crosses to the panel on the tapped side; anything else
                // toggles, as it always did.
                if !self.overlay.bar_tapped(&self.theme.motion) {
                    self.toggle_overlay(app);
                }
            }
            Some(StatusBarAction::Tapped) | None => {}
        }
        match desktop_action {
            Some(DesktopAction::Tap(id)) => self.launch_decl(&id, app),
            Some(DesktopAction::TapLocked(id)) => {
                let then = self.action_for(&id);
                let gate = self.gate_of(&id);
                self.request_unlock(gate, then);
            }
            // The desktop has already answered it — its info popover is up unless `[desktop]
            // long_press = "none"` — and it goes out either way, for a menu of the
            // device's own.
            Some(DesktopAction::LongPress(id)) => {
                self.events.push(ShellEvent::IconLongPressed { id });
            }
            None => {}
        }
        match nav_action {
            Some(NavAction::Back) => self.back_in(app),
            Some(NavAction::Home) => self.home(),
            // The nav bar's own two unbuilt items are **reported** rather than greyed out — the
            // crate has no overview and no split pane yet, but the device may well have somewhere to
            // go, and a control the shell refuses to pass on is the shell deciding.
            Some(NavAction::Recents) => self.open_overview(),
            Some(NavAction::Split) => self.toggle_split(),
            Some(NavAction::Custom(id)) if id == "osk" => self.osk.toggle(),
            Some(NavAction::Custom(id)) => {
                // A `nav_item` closure takes the tap with its own widget. What arrives here is a
                // tap within the item's area **the closure did not consume**, so the shell has nothing to do.
                log::debug!("tap inside nav_item `{id}` - its closure did not consume it");
            }
            None => {}
        }
        if late.osk_hide {
            self.osk.hide();
        }
        match late.overlay {
            Some(OverlaySlotAction::Launch(action)) => {
                // **Where the action takes you elsewhere, the shade goes up first.** Press settings and
                // have the shade still covering it, and there is no telling the screen even opened.
                // Operating a value (`Set` · `Toggle`), conversely, normally means touching several in a
                // row, so it is left open.
                if leaves_overlay(&action) {
                    self.overlay.close(&self.theme.motion);
                }
                self.launch_in(action, app);
            }
            Some(OverlaySlotAction::TileLongPressed(id)) => {
                // The event is raised whether or not there is somewhere to go — an integrator who wants
                // to put a menu up, or do something else, need only take this.
                let action = self.tile_long_press(&id);
                self.events.push(ShellEvent::TileLongPressed { id });
                if let Some(action) = action {
                    if leaves_overlay(&action) {
                        self.overlay.close(&self.theme.motion);
                    }
                    self.launch_in(action, app);
                }
            }
            Some(OverlaySlotAction::TileLocked(id)) => {
                self.request_unlock(Gate::from(id), None);
            }
            Some(OverlaySlotAction::NotificationTapped(id)) => self.notification_tapped(id, app),
            Some(OverlaySlotAction::Dismiss(id)) => {
                self.dismiss_notification(id);
            }
            Some(OverlaySlotAction::ClearAll) => self.notifications.clear(),
            None => {}
        }
        match late.heads_up {
            // A banner over the lock screen is something to read, not a way past it.
            Some(HeadsUpAction::Tapped(id)) if !self.prompt.is_lock_screen() => {
                self.notification_tapped(id, app);
            }
            Some(HeadsUpAction::Tapped(_) | HeadsUpAction::Swiped(_)) | None => {}
        }
        if let Some(ToastAction::Tapped(index)) = late.toast {
            self.toasts.dismiss(index, &self.theme.motion);
        }
        match late.prompt {
            Some(PromptAction::Submit(credential)) => self.submit_credential(credential, app),
            Some(PromptAction::Cancel) => self.cancel_prompt(),
            Some(PromptAction::Continue) => self.continue_from_lock(app),
            None => {}
        }
        self.process_requests(app);
        self.flush_lifecycle(app);
        self.schedule_repaint(ctx, wake);
        // 16. The events go out through poll_events.
    }

    /// Stage 15: the repaint policy.
    fn schedule_repaint(&mut self, ctx: &egui::Context, wake: FrameWake) {
        // An immediate repaint only while an animation is running. Idle, it goes to the next
        //     scheduled moment: the earliest of a clock boundary (only where a clock was **actually
        //     drawn** this frame) · a backend's `next_wake` · a toast or heads-up expiring · a peek ·
        //     the OSK debounce · something being held (the long-press timer). With none of them, nothing
        //     is scheduled at all — idle at 0 fps.
        let animating = self.is_animating()
            || wake.commands_pending
            || self.config.shell.repaint == RepaintMode::Continuous;
        if animating {
            ctx.request_repaint();
            self.stale_wake_frames = 0;
            return;
        }
        let mut after = self.clock_wake();
        if let Some(at) = wake.next_wake {
            // Handing a `next_wake` already past over as `Duration::ZERO` has egui read it as an
            // immediate repaint and it quietly becomes 60 fps — a floor is put on it, and where it keeps
            // pointing into the past, which backend it is gets announced.
            let stale = at <= wake.now;
            self.note_stale_wake(stale, wake.now);
            let wake_in = at.saturating_duration_since(wake.now).max(MIN_WAKE);
            after = Some(after.map_or(wake_in, |a| a.min(wake_in)));
        } else {
            self.stale_wake_frames = 0;
        }
        let mut deadline = |at: Option<Duration>| {
            if let Some(d) = at {
                let d = d.max(MIN_WAKE);
                after = Some(after.map_or(d, |a| a.min(d)));
            }
        };
        deadline(
            self.toasts
                .next_deadline()
                .map(|t| t.saturating_duration_since(wake.now)),
        );
        deadline(
            self.heads_up
                .next_deadline()
                .map(|t| t.saturating_duration_since(wake.now)),
        );
        deadline(self.overlay.peek_remaining(wake.now));
        // A session timer running out on a panel nobody touches — a temporary unlock, the
        // timeout, the idle lock — and the lockout's countdown.
        let at = |t: Option<Instant>| t.map(|t| t.saturating_duration_since(wake.now));
        deadline(at(self.access.next_deadline()));
        deadline(at(self.prompt.next_deadline(wake.now)));
        if self.prompt.is_lock_screen() {
            // The lock screen's clock, at the next minute.
            deadline(Some(Duration::from_secs(
                self.services.clock.now().secs_to_next_minute(),
            )));
        }
        if self.osk.is_shown() && !wake.wants_keyboard {
            deadline(Some(self.theme.motion.osk_hide_debounce));
        }
        if self.gestures.hold().is_some() || self.gestures.is_active() || self.toasts.pending() > 0
        {
            // While something is held, the timers are re-evaluated every frame (long presses, the
            // emergency gesture) — and while an edge swipe or a touch in a region is under way: a
            // finger standing still on it sends nothing, and its pause (`SwipeHold`, the recent
            // screens in the gesture navigation, a gesture handle's long gesture,
            // a region's own) is only seen by frames that keep coming.
            deadline(Some(Duration::from_millis(16)));
        }
        if let Some(after) = after {
            ctx.request_repaint_after(after);
        }
    }

    /// A notification tap (in the shade's list, or a heads-up): run the action plus raise the event.
    fn notification_tapped(&mut self, id: NotificationId, app: &mut AppRef<'_>) {
        if let Some(action) = self.notifications.get(id).and_then(|n| n.action.clone()) {
            // A notification tap follows the same convention — opening a screen has the shade stand aside.
            if leaves_overlay(&action) {
                self.overlay.close(&self.theme.motion);
            }
            self.launch_in(action, app);
        }
        self.events.push(ShellEvent::NotificationTapped(id));
    }

    /// Toggle the shade. Opening it passes the `overlay.open` gate.
    fn toggle_overlay(&mut self, app: &mut AppRef<'_>) {
        if self.overlay.is_closed() {
            self.launch_in(LaunchAction::OpenOverlay, app);
        } else {
            self.overlay.close(&self.theme.motion);
        }
    }

    /// The clock boundary for the idle repaint. It has a value only where a clock item was **drawn on
    /// this frame** (waking at the clock's next minute boundary is a rule for when the clock is
    /// visible) — with the status bar off, the chrome policy `Hide`, or the clock invisible through a
    /// gate or a collapse, nothing is woken.
    fn clock_wake(&self) -> Option<Duration> {
        // `shows_seconds`, not `== Hms`: the 12-hour spelling of the same shape needs waking just as
        // often. The `ui.clock_12h` setting never changes which of the two it is.
        if self.status_bar.drawn_clock()?.shows_seconds() {
            // `WallTime` has no fractional seconds, so the phase cannot be matched to the second
            // boundary (M4). Scheduling at 500 ms guarantees each second is drawn at least once — fixed
            // at 1 s, frame delays accumulate and the seconds digit skips.
            return Some(Duration::from_millis(500));
        }
        Some(Duration::from_secs(
            self.services.clock.now().secs_to_next_minute(),
        ))
    }

    /// Count the frames `next_wake` points into the past. Every [`STALE_WAKE_FRAMES`] in a row it warns
    /// once which backend it is.
    fn note_stale_wake(&mut self, stale: bool, now: Instant) {
        if !stale {
            self.stale_wake_frames = 0;
            return;
        }
        self.stale_wake_frames += 1;
        if !self.stale_wake_frames.is_multiple_of(STALE_WAKE_FRAMES) {
            return;
        }
        let names = [
            ("clock", self.services.clock.next_wake()),
            ("power", self.services.power.next_wake()),
            ("wifi", self.services.wifi.next_wake()),
            ("bluetooth", self.services.bluetooth.next_wake()),
        ];
        for (name, at) in names {
            if at.is_some_and(|at| at <= now) {
                log::warn!(
                    "backend `{name}` has had next_wake in the past for {} frames - the shell cannot go idle (guide 06 §1)",
                    self.stale_wake_frames
                );
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

    /// The single enforcement point for running things. On a failed gate it does not open and
    /// raises an `UnlockRequested`. For `Set` / `Toggle` the key name is the gate ("writing a
    /// settings value").
    pub fn launch(&mut self, action: LaunchAction) {
        self.launch_in(action, &mut None);
    }

    /// [`Shell::launch`] **with the app's state in hand**.
    ///
    /// Inside a frame the shell already has it, so a tap that runs an `action(id, ..)` closure gives
    /// it `cx.app()`. Between frames it does not — the app is holding its own state, not the shell —
    /// so a direct `shell.launch(LaunchAction::run("calibrate"))` would run that closure with
    /// `cx.app() == None`. This is the same call with the state lent for its length:
    ///
    /// ```no_run
    /// # struct Console;
    /// # struct App { shell: fairing::Shell, console: Console }
    /// # impl App {
    /// fn calibrate(&mut self) {
    ///     self.shell
    ///         .launch_with(fairing::LaunchAction::run("calibrate"), &mut self.console);
    /// }
    /// # }
    /// ```
    ///
    /// Only `LaunchAction::Run` actually runs your code on the spot; `Open` puts a screen up and its
    /// `ui` runs in the next frame, which has the state anyway.
    pub fn launch_with(&mut self, action: LaunchAction, app: &mut dyn std::any::Any) {
        self.launch_in(action, &mut Some(app));
    }

    /// The frame's own launch — it carries the app's state, so an `action(id, ..)` closure reached
    /// by a tap sees `cx.app()`. The public [`Shell::launch`] is this with nothing lent,
    /// because between frames the shell is not holding the app's state; use
    /// [`ShellHandle::launch`](crate::ShellHandle::launch) instead and it lands inside the next
    /// frame with the state in hand.
    fn launch_in(&mut self, action: LaunchAction, app: &mut AppRef<'_>) {
        match action {
            LaunchAction::Open { ref id, .. } => {
                let id = id.clone();
                self.open_screen(&id, action);
            }
            LaunchAction::Run(id) => self.run_action(&id, app),
            LaunchAction::Set(key, value) => self.set_setting(key, value),
            LaunchAction::Toggle(key) => self.toggle_setting(key),
            LaunchAction::OpenOverlay => {
                let gate = OVERLAY_OPEN_GATE;
                if self.access.allows(&gate) {
                    self.overlay.open(&self.theme.motion);
                } else {
                    self.request_unlock(gate, Some(LaunchAction::OpenOverlay));
                }
            }
            // **Always reported**, and in `prompt` mode with an authenticator **also
            // carried out**: there the shell owns the session's way up — its prompt — so it
            // owns the way down too. Elsewhere what locking and logging out mean is the device's.
            // The arms are written out rather than left to a catch-all so that a new variant
            // cannot be swallowed here again the way these three once were.
            LaunchAction::Lock => self.lock_now(false),
            LaunchAction::Logout => self.end_session(),
            LaunchAction::OpenOverview => self.open_overview(),
            LaunchAction::ToggleSplit => self.toggle_split(),
        }
    }

    /// The overview control (the nav bar's `"recents"`, `LaunchAction::OpenOverview`).
    /// Always reported as [`ShellEvent::OverviewRequested`], and the shell's own
    /// overview comes up where `[workspace] overview` is on. Pressed with it up, it goes back.
    fn open_overview(&mut self) {
        self.events.push(ShellEvent::OverviewRequested);
        if !self.config.workspace.overview {
            return;
        }
        let tokens = self.theme.motion;
        if self.workspace.is_overview_open() {
            self.workspace.close_overview(&tokens);
        } else if self.access.allows(&RECENTS_GATE) {
            // The cards come up over everything but the shell's own chrome — the shade goes up
            // first, as it does for home.
            if !self.overlay.is_closed() {
                self.overlay.close(&tokens);
            }
            let beside = self.may_enter_split();
            self.workspace
                .open_overview(OverviewMode::Recents { beside }, &tokens);
        } else {
            self.request_unlock(RECENTS_GATE, Some(LaunchAction::OpenOverview));
        }
    }

    /// Whether a split may come up — `[workspace] split` on and the session through
    /// [`SPLIT_GATE`] — or is up already.
    fn may_enter_split(&self) -> bool {
        self.config.workspace.split
            && (self.workspace.is_split() || self.access.allows(&SPLIT_GATE))
    }

    /// The split control (the split tile, the nav bar's `"split"` item,
    /// `LaunchAction::ToggleSplit`). Split, back to one pane keeping the focused one; one pane in
    /// the Tasks view, the task used last beside it. Always reported as
    /// [`ShellEvent::SplitRequested`], and carried out where `[workspace] split` is on.
    fn toggle_split(&mut self) {
        self.events.push(ShellEvent::SplitRequested);
        if !self.config.workspace.split {
            return;
        }
        let tokens = self.theme.motion;
        // Pressed again over the cards, it goes back.
        if self.workspace.is_overview_open() {
            self.workspace.close_overview(&tokens);
            return;
        }
        if self.workspace.is_split() {
            let keep = self.workspace.focused_pane();
            self.workspace.unsplit(keep, &tokens);
            return;
        }
        if !self.access.allows(&SPLIT_GATE) {
            self.request_unlock(SPLIT_GATE, Some(LaunchAction::ToggleSplit));
            return;
        }
        // Where no split can come up, it says why rather than doing nothing.
        if let Some(blocker) = self.workspace.split_blocker() {
            let label = match blocker {
                SplitBlocker::NothingOnShow => labels::SPLIT_NOTHING_ON_SHOW,
                SplitBlocker::Refused => labels::SPLIT_REFUSED,
                SplitBlocker::NothingFits => labels::SPLIT_NOTHING_FITS,
            };
            let text = self.strings.get(label).to_owned();
            self.toast(text);
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&tokens);
        }
        // With the shell's overview, the cards say what can go beside — only what fits is offered;
        // without it, the task used last of those that fit.
        if self.config.workspace.overview {
            self.workspace.open_overview(OverviewMode::Picker, &tokens);
        } else if let Some(task) = self.workspace.most_recent_other_task() {
            let _ = self.workspace.show_in_other_pane(task, &tokens);
        }
    }

    /// Writing a settings value (the enforcement point). The gate = the key name. On a failure,
    /// `UnlockRequested { then: Set }`. It updates [`Shell::settings`], calls the backend of a
    /// built-in key and emits [`ShellEvent::SettingChanged`]. The shell writes no files: keeping
    /// the value past a restart is the integrator's, and [`Shell::restore_settings`] hands it back.
    pub fn set_setting(&mut self, key: SettingKey, value: SettingValue) {
        if !self.setting_allowed(&key, || LaunchAction::Set(key.clone(), value.clone())) {
            return;
        }
        self.write_setting(key, value);
    }

    /// **Put saved settings back** — at start, from wherever the device keeps them.
    ///
    /// The shell writes no files. A change reaches you as [`ShellEvent::SettingChanged`], to store
    /// where your device keeps such things, and this hands what you stored back. Each value goes
    /// into [`Shell::settings`], and a built-in key reaches its backend, the theme or the language
    /// as a change would — the theme at once, without the crossfade.
    ///
    /// Unlike [`Shell::set_setting`] it **checks no gate** — the values passed one when they were
    /// set, and it is the integrator restoring them, not the person at the panel — and it **emits
    /// no `SettingChanged`**, since they are stored already. `radio.airplane` is applied after the
    /// rest and only ever turns the radios off: airplane mode off leaves each radio to its own
    /// key.
    ///
    /// ```no_run
    /// # fn load() -> Vec<(String, fairing::SettingValue)> { Vec::new() }
    /// # fn start(shell: &mut fairing::Shell) {
    /// shell.restore_settings(load()); // what your `SettingChanged` handler stored
    /// # }
    /// ```
    pub fn restore_settings<K: Into<SettingKey>>(
        &mut self,
        values: impl IntoIterator<Item = (K, SettingValue)>,
    ) {
        let mut airplane = false;
        for (key, value) in values {
            let key = key.into();
            match (key.0.as_ref(), &value) {
                (keys::RADIO_AIRPLANE, on) => airplane = matches!(on, SettingValue::Bool(true)),
                (keys::THEME_DARK, SettingValue::Bool(dark)) => self.snap_theme_dark(*dark),
                _ => self.apply_setting_to_backend(&key, &value),
            }
            self.settings.set(key, value);
        }
        if airplane {
            let key = SettingKey::from(keys::RADIO_AIRPLANE);
            self.apply_setting_to_backend(&key, &SettingValue::Bool(true));
        }
    }

    /// Switch between dark and light **without** the crossfade — a restored theme is the one the
    /// panel starts in, not a change to watch.
    fn snap_theme_dark(&mut self, dark: bool) {
        self.theme.dark = dark;
        let palette = if dark {
            self.palettes.0
        } else {
            self.palettes.1
        };
        self.theme.palette = palette;
        self.theme_fade = ThemeFade::idle(palette);
        self.theme_dirty = true;
    }

    /// `Toggle(key)`: a built-in key (`wifi.enabled` · `bluetooth.enabled` · `theme.dark`) is written as
    /// the opposite of **the current value the backend reports**, and everything else as the opposite of
    /// the memory view. Going by the memory view alone would make a fresh shell go `None → true` and
    /// turn already-enabled Wi-Fi on again.
    fn toggle_setting(&mut self, key: SettingKey) {
        if !self.setting_allowed(&key, || LaunchAction::Toggle(key.clone())) {
            return;
        }
        let current = self
            .builtin_bool(&key)
            .or_else(|| match self.settings.get(&key) {
                None => Some(false),
                Some(SettingValue::Bool(b)) => Some(*b),
                Some(other) => {
                    log::warn!("Toggle: setting `{}` is not a Bool ({other:?})", key.0);
                    None
                }
            });
        let Some(current) = current else {
            return;
        };
        self.write_setting(key, SettingValue::Bool(!current));
    }

    /// A built-in Bool key's **current value** — only for keys the backend is the single source of (for the `Toggle` decision).
    fn builtin_bool(&self, key: &SettingKey) -> Option<bool> {
        match key.0.as_ref() {
            keys::WIFI_ENABLED => Some(self.services.wifi.enabled()),
            keys::BLUETOOTH_ENABLED => Some(self.services.bluetooth.enabled()),
            keys::AUDIO_MUTED => self.services.audio.volume().map(|audio| audio.muted),
            keys::THEME_DARK => Some(self.theme.dark),
            _ => None,
        }
    }

    fn setting_allowed(&mut self, key: &SettingKey, then: impl FnOnce() -> LaunchAction) -> bool {
        let gate = Gate::from(&*key.0);
        if self.access.allows(&gate) {
            return true;
        }
        self.request_unlock(gate, Some(then()));
        false
    }

    /// Refresh the memory view, plus the backend wiring for built-in keys, plus a
    /// `SettingChanged` — the integrator's cue to store the value.
    fn write_setting(&mut self, key: SettingKey, value: SettingValue) {
        self.settings.set(key.clone(), value.clone());
        self.apply_setting_to_backend(&key, &value);
        self.events.push(ShellEvent::SettingChanged { key, value });
    }

    /// A built-in key → the backend ("call the related backend"). A failure is only logged — the value is already in memory.
    fn apply_setting_to_backend(&mut self, key: &SettingKey, value: &SettingValue) {
        let as_bool = matches!(value, SettingValue::Bool(true));
        let as_u8 = match value {
            SettingValue::Int(v) => u8::try_from((*v).clamp(0, 100)).unwrap_or(100),
            SettingValue::Float(v) => percent_from_f64(*v),
            SettingValue::Bool(b) => u8::from(*b) * 100,
            SettingValue::Text(_) => 0,
        };
        let result = match key.0.as_ref() {
            keys::WIFI_ENABLED => self.services.wifi.set_enabled(as_bool),
            keys::BLUETOOTH_ENABLED => self.services.bluetooth.set_enabled(as_bool),
            keys::DISPLAY_BRIGHTNESS => self.services.display.set_brightness(as_u8),
            keys::AUDIO_VOLUME => self.services.audio.set_volume(as_u8),
            keys::AUDIO_MUTED => self.services.audio.set_muted(as_bool),
            keys::RADIO_AIRPLANE => {
                let wifi = self.services.wifi.set_enabled(!as_bool);
                self.services.bluetooth.set_enabled(!as_bool).and(wifi)
            }
            keys::THEME_DARK => {
                self.set_theme_dark(as_bool);
                Ok(())
            }
            keys::UI_LOCALE => {
                if let SettingValue::Text(locale) = value {
                    self.set_locale(locale);
                }
                Ok(())
            }
            _ => Ok(()),
        };
        // `Unsupported` is an absence, not a failure: with the `Null` audio backend the integrator
        // applies `audio.volume` from `SettingChanged` instead, and every slider step would warn.
        match result {
            Err(e) if e.kind != crate::services::ErrorKind::Unsupported => {
                log::warn!("setting `{}` did not reach the backend: {e}", key.0);
            }
            _ => {}
        }
    }

    /// Switch the language (the `ui.locale` setting): the next frame draws in it.
    fn set_locale(&mut self, locale: &str) {
        if !self.strings.set_locale(locale) {
            log::warn!(
                "ui.locale = \"{locale}\" has no table - staying with \"{}\" (add one with ShellBuilder::translations)",
                self.strings.locale()
            );
            return;
        }
        self.status_bar.forget_texts();
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

    /// Back (the priority: the prompt → close the overlay → close the OSK → `on_back` → pop →
    /// ignored at home). Back cancels an unlock prompt and does nothing on the lock screen — the
    /// way out of that is the prompt.
    pub fn back(&mut self) {
        self.back_in(&mut None);
    }

    /// [`Shell::back`] **with the app's state in hand** — for a screen whose
    /// [`Screen::on_back`](crate::Screen::on_back) reads it. See [`Shell::launch_with`].
    pub fn back_with(&mut self, app: &mut dyn std::any::Any) {
        self.back_in(&mut Some(app));
    }

    /// The frame's own back — see [`Shell::launch_in`] for why there are two.
    fn back_in(&mut self, app: &mut AppRef<'_>) {
        // Where stage 12's prompt is up, back is the prompt's.
        if self.prompt.is_open() {
            if !self.prompt.is_lock_screen() {
                self.cancel_prompt();
            }
            return;
        }
        // An icon's info popover goes before anything under it moves.
        if self.desktop.info_open() {
            self.desktop.close_info(&self.theme.motion);
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
            return;
        }
        if self.osk.is_shown() {
            self.osk.hide();
            return;
        }
        if self.workspace.is_overview_open() {
            let tokens = self.theme.motion;
            self.workspace.close_overview(&tokens);
            return;
        }
        if self.workspace.is_home() {
            return;
        }
        let consumed = self
            .with_parts(app, |workspace, registry, parts| {
                let rect = workspace.focused_pane_rect();
                let is_split = workspace.is_split();
                let task = workspace.active_task_mut()?;
                let top = task.top_mut()?;
                let pane = PaneInfo {
                    rect,
                    outer: rect,
                    is_split,
                    is_focused: true,
                    inset_bottom: 0.0,
                    instance: top.id(),
                };
                let mut cx = parts.cx_in(pane, None, Some(registry));
                Some(top.on_back(&mut cx) == BackAction::Consumed)
            })
            .unwrap_or(false);
        if consumed {
            return;
        }
        let closing = self
            .workspace
            .focused()
            .map(|i| (i.decl_id().to_owned(), i.id()));
        // A root pop (ending the task → home) hands the A2 close's origin as the root icon Rect
        // the desktop remembers **right now** — the last Rect the Workspace remembered can be stale
        // after an M2 page swipe.
        let at_root = self
            .workspace
            .active_task()
            .is_some_and(|task| task.len() <= 1);
        let popped = if at_root {
            let icon_rect = self
                .workspace
                .active_task()
                .and_then(Task::root_id)
                .and_then(|id| self.desktop.icon_rect(id));
            self.workspace
                .close_active_task_with(icon_rect, &self.theme.motion)
        } else {
            self.workspace.pop(&self.theme.motion)
        };
        if popped {
            if let Some((id, instance)) = closing {
                self.events.push(ShellEvent::ScreenClosed { id, instance });
            }
            if self.workspace.is_home() {
                self.events.push(ShellEvent::WentHome);
            }
        }
    }

    /// Home. The task stays alive. An open shade is closed with it — the A1 interruption rule,
    /// "home/back while opening → `Settling → Closed`" (the shade closes even when pressed on the home
    /// screen).
    pub fn home(&mut self) {
        self.desktop.close_info(&self.theme.motion);
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
        }
        if self.workspace.is_home() {
            // The overview over the desktop goes back down — home is where it already is.
            if self.workspace.is_overview_open() {
                self.workspace.close_overview(&self.theme.motion);
            }
            return;
        }
        let icon_rect = self
            .workspace
            .active_task()
            .and_then(|t| t.root_id().map(str::to_owned))
            .and_then(|id| self.desktop.icon_rect(&id));
        self.workspace.go_home(icon_rect, &self.theme.motion);
        self.events.push(ShellEvent::WentHome);
    }

    /// Stage 2. There is a **per-frame cap**: so that one frame does not grow in proportion to the
    /// queue when another thread pours commands in (the p95 budget of 12 ms), only
    /// [`MAX_COMMANDS_PER_FRAME`] are handled and the rest is left to the next frame. `true` if any
    /// were left — the shell then requests an immediate repaint to carry on.
    fn drain_commands(&mut self, app: &mut AppRef<'_>) -> bool {
        for _ in 0..MAX_COMMANDS_PER_FRAME {
            // `Inbox` has no blocking call to reach for, so this loop cannot become one.
            let Ok(command) = self.rx.try_recv() else {
                return false;
            };
            match command {
                Command::Launch(action) => self.launch_in(action, app),
                Command::CloseScreen(id) => self.close_instance(id, None, app),
                Command::Back => self.back_in(app),
                Command::Home => self.home(),
                Command::SetSubject(subject) => {
                    let to = subject.level;
                    let from = self.access.set_subject_at(subject, self.mono);
                    self.session_changed(SessionChange {
                        from,
                        to,
                        reason: ChangeReason::Integrator,
                    });
                    // The integrator vouched for someone above where a lock leaves the panel —
                    // a reader it handles itself, a remote unlock: the lock screen has done its
                    // work.
                    if self.prompt.is_lock_screen() && to > self.access.initial_subject().level {
                        self.cancel_prompt();
                        self.events
                            .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
                        // What waited behind it is asked now, not at some later lock screen.
                        self.ask_after_lock(app);
                    }
                }
                // The same as `LaunchAction::Logout`.
                Command::Logout => self.end_session(),
                Command::SetBadge { id, badge } => {
                    self.desktop.set_badge(&id, badge.as_ref());
                }
                Command::SetSetting { key, value } => self.set_setting(key, value),
                Command::RequestUnlock { gate, then } => self.request_unlock(gate, then),
                Command::Notify(notification) => self.notify(*notification),
                Command::Toast(toast) => self.toasts.push(*toast),
                Command::DismissNotification(id) => {
                    self.dismiss_notification(id);
                }
                Command::ToggleOverlay => self.toggle_overlay(app),
                Command::SetMotion(tokens) => self.set_motion(*tokens),
            }
        }
        log::warn!(
            "the command queue went over the per-frame limit ({MAX_COMMANDS_PER_FRAME}) - the rest run next frame"
        );
        true
    }

    /// After a session change: end the instances that can no longer pass their gate, and queue
    /// an `AccessChanged` on every instance left (notified at stage 14). A timeout, a lock, a logout,
    /// an unlock and the integrator's `set_subject` all take this path.
    fn propagate_access_change(&mut self) {
        let was_home = self.workspace.is_home();
        let Self {
            workspace,
            registry,
            access,
            theme,
            events,
            ..
        } = self;
        let closed = workspace.close_where(
            |instance| {
                let gate = registry
                    .screen(instance.decl_id())
                    .map_or_else(|| Gate::from(instance.decl_id()), ScreenDecl::gate_name);
                !access.allows(&gate)
            },
            &theme.motion,
        );
        for (id, instance) in closed {
            events.push(ShellEvent::ScreenClosed { id, instance });
        }
        workspace.queue_all(Lifecycle::AccessChanged);
        self.note_went_home(was_home);
    }

    /// **The one way a failed gate asks for an unlock**: the event always, and — when
    /// the shell can prompt — the prompt over the screen. A granted unlock runs `then` through
    /// [`Shell::launch`] again, gate and all.
    fn request_unlock(&mut self, gate: Gate, then: Option<LaunchAction>) {
        self.events
            .push(ShellEvent::Access(AccessEvent::UnlockRequested {
                gate: gate.clone(),
                then: then.clone(),
            }));
        // The lock screen is already a prompt; whatever asked behind it waits for it — and is
        // asked again once the lock screen is gone.
        if self.prompt.is_lock_screen() {
            self.after_lock = Some((gate, then));
            return;
        }
        let Some(methods) = self.prompt_methods() else {
            if self.access.can_prompt() {
                log::warn!(
                    "the authenticator offers no method - \"{}\" goes out as UnlockRequested only",
                    gate.as_str()
                );
            }
            return;
        };
        let hint = self.access.hint(&gate);
        if !self.prompt.is_open() {
            // Whatever was being dragged ends here (A9).
            self.gestures.cancel();
            let now = self.mono;
            if let Some(auth) = self.access.authenticator_mut() {
                auth.begin(&gate, now);
            }
        }
        self.prompt
            .open(Purpose::Unlock { gate, then }, methods, hint, self.mono);
    }

    /// The methods the shell's prompt would offer — `None` where it draws none: not `prompt` mode,
    /// one level, no authenticator, or an authenticator that offers nothing (which behaves as
    /// `routing` — the lock and the logout included).
    fn prompt_methods(&mut self) -> Option<Vec<AuthMethod>> {
        if !self.access.can_prompt() {
            return None;
        }
        let methods = self
            .access
            .authenticator_mut()
            .map(|auth| auth.methods())
            .unwrap_or_default();
        (!methods.is_empty()).then_some(methods)
    }

    /// What asked behind the lock screen, asked again now that it is gone: run where the session
    /// passes its gate now, the prompt where it does not.
    fn ask_after_lock(&mut self, app: &mut AppRef<'_>) {
        let Some((gate, then)) = self.after_lock.take() else {
            return;
        };
        if self.access.allows(&gate) {
            if let Some(action) = then {
                self.launch_in(action, app);
            }
        } else {
            self.request_unlock(gate, then);
        }
    }

    /// Stage 4: the session's own clock, and the authenticator's late answers.
    fn tick_access(&mut self, now: Instant, input_activity: bool, app: &mut AppRef<'_>) {
        #[cfg(feature = "settings")]
        self.access.refresh_credentials();
        if let Some(change) = self.access.tick(now, input_activity) {
            self.session_changed(change);
        }
        // Due is asked first, so the quiet stretch is spent even while the lock screen is
        // already up — and the device is not told to lock a panel that is locked.
        if self.access.idle_lock_due(now) && !self.prompt.is_lock_screen() {
            self.lock_now(true);
        }
        if self.prompt.is_open() {
            let outcome = self
                .access
                .authenticator_mut()
                .and_then(|auth| auth.poll(now));
            // What the authenticator reports is the state to follow: the shell makes the
            // transition and does not second-guess it.
            if let Some(outcome) = outcome {
                self.apply_outcome(outcome, app);
            }
        }
    }

    /// Announce a change of subject and pass it on: `SessionChanged`, the screens that no longer
    /// pass closed, and — on the way down — the shade and an unlock prompt closed too.
    fn session_changed(&mut self, change: SessionChange) {
        self.events
            .push(ShellEvent::Access(AccessEvent::SessionChanged {
                from: change.from,
                to: change.to,
                reason: change.reason,
            }));
        if change.to < change.from {
            if !self.overlay.is_closed() {
                self.overlay.close(&self.theme.motion);
            }
            if self.prompt.is_open() && !self.prompt.is_lock_screen() {
                self.cancel_prompt();
            }
        }
        // The cards follow the session too: short of the gate they came up behind they go, and
        // their split buttons come and go with `workspace.split`.
        if self.workspace.is_overview_open() {
            let gate = if self.workspace.is_overview_picking() {
                SPLIT_GATE
            } else {
                RECENTS_GATE
            };
            if self.access.allows(&gate) {
                let beside = self.may_enter_split();
                self.workspace.set_overview_beside(beside);
            } else {
                self.workspace.close_overview(&self.theme.motion);
            }
        }
        self.propagate_access_change();
    }

    /// Lock: [`LaunchAction::Lock`], or `idle_lock_secs` running out (`idle`).
    ///
    /// Always reported as [`ShellEvent::LockRequested`]. With a prompt to draw the
    /// shell carries it out: the session goes back to where it started and the lock screen
    /// comes up. Without one the idle lock still takes the session back — that is the timer's
    /// job — and the control is the integrator's.
    fn lock_now(&mut self, idle: bool) {
        self.events.push(ShellEvent::LockRequested);
        let methods = self.prompt_methods();
        let can_prompt = methods.is_some();
        if !can_prompt && !idle {
            return;
        }
        let now = self.mono;
        if self.access.away_from_start() {
            let from = self.access.reset(now);
            let to = self.access.initial_subject().level;
            self.session_changed(SessionChange {
                from,
                to,
                reason: ChangeReason::Lock,
            });
        }
        let Some(methods) = methods else {
            return;
        };
        if self.prompt.is_lock_screen() {
            return;
        }
        // An unlock under way gives way to the lock, and so does the shade.
        if self.prompt.is_open() {
            self.cancel_prompt();
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
        }
        self.gestures.cancel();
        if let Some(auth) = self.access.authenticator_mut() {
            auth.begin(&crate::access::prompt::LOCK_GATE, now);
        }
        // A new lock starts with nothing waiting behind it.
        self.after_lock = None;
        self.prompt.open(Purpose::Lock, methods, None, now);
        self.events
            .push(ShellEvent::Access(AccessEvent::LockScreenToggled(true)));
    }

    /// Log out: [`LaunchAction::Logout`] and [`ShellHandle::logout`]. Always reported as
    /// [`ShellEvent::LogoutRequested`]; with a prompt to draw, the shell also takes the
    /// session back to the starting subject.
    fn end_session(&mut self) {
        self.events.push(ShellEvent::LogoutRequested);
        if self.prompt_methods().is_none() || !self.access.away_from_start() {
            return;
        }
        let from = self.access.reset(self.mono);
        let to = self.access.initial_subject().level;
        self.session_changed(SessionChange {
            from,
            to,
            reason: ChangeReason::Logout,
        });
    }

    /// Hand what was entered to the authenticator, by value — it is not seen again.
    ///
    /// A new attempt calls off the check still under way: a wait ends with an answer or with
    /// `cancel`, so a slow answer never holds the keys — and the lock screen, which has no
    /// Cancel, always has a way past an answer that never comes.
    fn submit_credential(&mut self, credential: Credential, app: &mut AppRef<'_>) {
        let now = self.mono;
        let called_off = self.prompt.take_wait();
        let outcome = self.access.authenticator_mut().map(|auth| {
            if called_off {
                auth.cancel();
            }
            auth.submit(credential, now)
        });
        if let Some(outcome) = outcome {
            self.apply_outcome(outcome, app);
        }
    }

    /// Apply the authenticator's answer. A grant closes the prompt, applies
    /// `unlock_mode` and runs what the prompt was opened for — through the gate again.
    fn apply_outcome(&mut self, outcome: AuthOutcome, app: &mut AppRef<'_>) {
        let now = self.mono;
        let Some(gate) = self.prompt.gate() else {
            return;
        };
        match outcome {
            AuthOutcome::Granted(subject) => {
                let lock = self.prompt.is_lock_screen();
                let level = subject.level;
                // A level the table does not have would pass every gate; and on the unlock
                // prompt a grant at or below the session's own level opens nothing — it must not
                // lower the session (closing what it has open) either.
                let unknown = self.access.table().get(level).is_none();
                if unknown {
                    log::warn!(
                        "access: the authenticator granted level {}, which [access] levels does not have - refused",
                        level.0
                    );
                }
                if unknown || (!lock && level <= self.access.session().subject.level) {
                    self.events
                        .push(ShellEvent::Access(AccessEvent::Denied { gate }));
                    let text = crate::access::prompt::labels::NOT_ENOUGH.to_owned();
                    self.prompt.denied(text, now);
                    return;
                }
                let purpose = self.prompt.close(now);
                let mode = self.access.unlock_mode();
                let (subject_id, level) = (subject.id.clone(), subject.level);
                let from = self.access.unlock(subject, now);
                self.events.push(ShellEvent::Access(AccessEvent::Unlocked {
                    gate,
                    subject_id,
                    level,
                    mode,
                }));
                self.session_changed(SessionChange {
                    from,
                    to: level,
                    reason: ChangeReason::Unlock,
                });
                if lock {
                    self.events
                        .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
                    self.ask_after_lock(app);
                }
                if let Some(Purpose::Unlock {
                    then: Some(action), ..
                }) = purpose
                {
                    self.launch_in(action, app);
                }
            }
            AuthOutcome::Denied { message } => {
                self.events
                    .push(ShellEvent::Access(AccessEvent::Denied { gate }));
                self.prompt.denied(message, now);
            }
            AuthOutcome::Locked { until, message } => {
                self.events
                    .push(ShellEvent::Access(AccessEvent::Locked { gate, until }));
                self.prompt.locked(until, message);
            }
            AuthOutcome::Pending => self.prompt.wait(),
        }
    }

    /// Close the prompt without an answer — Cancel, back, a downgrade, the lock coming up, the
    /// lock screen's Continue, the integrator vouching for someone. The authenticator is always
    /// told: a wait ends with an answer or with `cancel`, and a reader stops listening.
    fn cancel_prompt(&mut self) {
        if let Some(auth) = self.access.authenticator_mut() {
            auth.cancel();
        }
        self.prompt.close(self.mono);
    }

    /// The lock screen's Continue: leave as the starting subject, which the lock already made the
    /// session.
    fn continue_from_lock(&mut self, app: &mut AppRef<'_>) {
        if !self.prompt.is_lock_screen() || !self.access.lock_screen_continue() {
            return;
        }
        self.cancel_prompt();
        self.events
            .push(ShellEvent::Access(AccessEvent::LockScreenToggled(false)));
        self.ask_after_lock(app);
    }

    fn gate_of(&self, id: &str) -> Gate {
        self.registry
            .screen(id)
            .map(ScreenDecl::gate_name)
            .or_else(|| {
                self.registry
                    .actions()
                    .iter()
                    .find(|a| a.id() == id)
                    .map(ActionDecl::gate_name)
            })
            .unwrap_or_else(|| Gate::from(id))
    }

    /// **Where a long press on a tile leads** ([`QuickTile::long_press`](crate::overlay::QuickTile::long_press)).
    /// `None` where there is none.
    ///
    /// What the declaration attached comes first, and without it it falls back to **the built-in tile's
    /// default pairing** — press and hold the Wi-Fi tile and the Wi-Fi settings open. It is used only
    /// where the paired screen is actually registered (turned off with `[settings] only`, or with the
    /// feature taken out, nothing happening is better).
    #[cfg(feature = "overlay")]
    fn tile_long_press(&self, id: &str) -> Option<LaunchAction> {
        let declared = self
            .registry
            .tiles()
            .iter()
            .find(|t| t.id() == id)
            .and_then(|t| t.tile().long_press.clone());
        declared.or_else(|| {
            let screen = crate::overlay::tiles::builtin_long_press(id)?;
            self.registry.screen(screen)?;
            Some(LaunchAction::open(screen))
        })
    }

    /// With no overlay there are no tiles — this branch is never reached in the first place.
    #[cfg(not(feature = "overlay"))]
    #[expect(clippy::unused_self, reason = "the same shape as feature-on")]
    fn tile_long_press(&self, _id: &str) -> Option<LaunchAction> {
        None
    }

    fn action_for(&self, id: &str) -> Option<LaunchAction> {
        match self.registry.kind(id)? {
            DeclKind::Screen => Some(LaunchAction::open(id)),
            DeclKind::Action => Some(LaunchAction::run(id)),
            DeclKind::StatusItem | DeclKind::NavItem | DeclKind::Tile => None,
        }
    }

    /// Run one declaration by id (an icon tap). `Open` for a screen, `Run` for an action.
    fn launch_decl(&mut self, id: &str, app: &mut AppRef<'_>) {
        match self.action_for(id) {
            Some(action) => self.launch_in(action, app),
            None => log::warn!("launch: no declaration `{id}`"),
        }
    }

    fn open_screen(&mut self, id: &str, action: LaunchAction) {
        // "open in the other pane" — where the shell's split is on and the session may
        // enter one; otherwise it opens where a split that cannot hold would, in the same pane.
        let in_other_pane = matches!(
            action,
            LaunchAction::Open {
                in_other_pane: true,
                ..
            }
        ) && self.may_enter_split();
        let Some(decl) = self.registry.screen(id) else {
            log::warn!("launch: screen `{id}` was never declared");
            return;
        };
        let gate = decl.gate_name();
        let mode = decl.launch_mode();
        if !self.access.allows(&gate) {
            self.request_unlock(gate, Some(action));
            return;
        }
        let tokens = self.theme.motion;
        // What opens is on show: the overview, if it is up, gets out of the way first.
        self.workspace.clear_overview();
        let icon_rect = self.desktop.icon_rect(id);
        if mode == LaunchMode::Single {
            // Single: ① inside the current task → clear-top, ② in another task → bring that task
            // forward, ③ neither → afresh (a new task at home, otherwise a push).
            if !self.workspace.is_home() {
                // The instances cleared away (everything above the lowest `id`) receive a `ScreenClosed`.
                let cleared: Vec<(String, InstanceId)> = self
                    .workspace
                    .active_task()
                    .and_then(|task| {
                        let pos = task.iter().position(|i| i.decl_id() == id)?;
                        Some(
                            task.iter()
                                .skip(pos + 1)
                                .map(|i| (i.decl_id().to_owned(), i.id()))
                                .collect(),
                        )
                    })
                    .unwrap_or_default();
                if self.workspace.clear_top_to(id) {
                    for (id, instance) in cleared {
                        self.events.push(ShellEvent::ScreenClosed { id, instance });
                    }
                    return;
                }
            }
            if let Some(index) = self.workspace.find_task(id) {
                // In a task of its own: beside the focused pane where asked, else brought forward.
                if in_other_pane && self.workspace.show_in_other_pane(index, &tokens) {
                    return;
                }
                if self.workspace.resume_task(index, icon_rect, &tokens) {
                    return;
                }
            }
        }
        let instance_id = self.workspace.alloc_id();
        let Some(decl) = self.registry.screen_mut(id) else {
            return;
        };
        let screen = decl.spawn();
        let instance = Instance::new(instance_id, decl, screen);
        if in_other_pane && self.workspace.can_split(instance.split_support()) {
            self.workspace.open_in_other_pane(instance, &tokens);
        } else {
            self.workspace.open(instance, icon_rect, &tokens);
        }
        // The entries are listed again whenever their screen opens — something outside the
        // shell may have changed them since.
        #[cfg(feature = "settings")]
        if id == crate::access::CREDENTIALS_SCREEN {
            self.access.mark_credentials_dirty();
        }
        self.events.push(ShellEvent::ScreenOpened {
            id: id.to_owned(),
            instance: instance_id,
        });
    }

    fn run_action(&mut self, id: &str, app: &mut AppRef<'_>) {
        let Some(gate) = self.registry.action_mut(id).map(|a| a.gate_name()) else {
            log::warn!("launch: action `{id}` was never declared");
            return;
        };
        if !self.access.allows(&gate) {
            self.request_unlock(gate, Some(LaunchAction::run(id)));
            return;
        }
        let pane = PaneInfo {
            rect: self.layout.content,
            outer: self.layout.content,
            is_split: false,
            is_focused: true,
            inset_bottom: 0.0,
            instance: InstanceId::NONE,
        };
        let now = self.mono;
        let frame = self.frame_no;
        let Self {
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
        if let Some(action) = registry.action_mut(id) {
            let mut cx = parts.cx(pane, None);
            action.run(&mut cx);
        }
    }

    fn close_instance(
        &mut self,
        id: InstanceId,
        value: Option<crate::screen::ScreenValue>,
        app: &mut AppRef<'_>,
    ) {
        // The screen closing, and the one under it in its own task — the parent its result goes
        // to. Split, that is not always the focused pane's top: a screen in the other pane can
        // finish on its own (an answer arriving), without a press there.
        let closing = self.workspace.tasks().iter().find_map(|t| {
            let mut below = None;
            for instance in t.iter() {
                if instance.id() == id {
                    return Some((instance.decl_id().to_owned(), below));
                }
                below = Some(instance.id());
            }
            None
        });
        let Some((decl_id, parent)) = closing else {
            return;
        };
        let was_home = self.workspace.is_home();
        if !self.workspace.close_instance(id, &self.theme.motion) {
            return;
        }
        self.events.push(ShellEvent::ScreenClosed {
            id: decl_id.clone(),
            instance: id,
        });
        if let (Some(value), Some(parent)) = (value, parent) {
            self.with_parts(app, |workspace, registry, parts| {
                let pane = workspace.pane_holding(parent);
                let rect = pane
                    .and_then(|p| workspace.pane_rect(p))
                    .unwrap_or_else(|| workspace.focused_pane_rect());
                let is_split = workspace.is_split();
                let is_focused = pane.is_some_and(|p| p == workspace.focused_pane());
                let parent = workspace.instance_mut(parent)?;
                let pane = PaneInfo {
                    rect,
                    outer: rect,
                    is_split,
                    is_focused,
                    inset_bottom: 0.0,
                    instance: parent.id(),
                };
                let mut cx = parts.cx_in(pane, None, Some(registry));
                parent.on_result(&decl_id, value, &mut cx);
                Some(())
            });
        }
        self.note_went_home(was_home);
    }

    fn process_requests(&mut self, app: &mut AppRef<'_>) {
        let requests = std::mem::take(&mut self.requests);
        for request in requests {
            match request {
                CxRequest::Open {
                    id,
                    in_other_pane,
                    from,
                } => {
                    // A screen opens from its own pane: split, the pane it is in takes the focus
                    // first, so "this pane" and "the other pane" are its own and the other.
                    let user = self.workspace.focused_pane();
                    let from_pane = self.workspace.pane_holding(from);
                    self.workspace.focus_pane_of(from);
                    self.launch_in(LaunchAction::Open { id, in_other_pane }, app);
                    // No press put the focus there (a press would have): the screen opened on
                    // its own, and what it opened landed in its pane — the user keeps theirs.
                    if from_pane.is_some_and(|p| p != user)
                        && Some(self.workspace.focused_pane()) == from_pane
                    {
                        self.workspace.return_focus(user);
                    }
                }
                CxRequest::Finish { instance, value } => self.close_instance(instance, value, app),
                CxRequest::SetChrome { instance, policy } => {
                    if let Some(i) = self.workspace.instance_mut(instance) {
                        i.set_chrome(policy);
                    }
                }
                CxRequest::Launch(action) => self.launch_in(action, app),
                CxRequest::Knock { id } => self.knock(&id),
                CxRequest::SetSetting { key, value } => self.set_setting(key, value),
                CxRequest::Power(request) => {
                    // **It is not carried out.** The integrator tidies up and then calls `commit_power`.
                    self.events.push(ShellEvent::PowerRequest(request));
                }
                #[cfg(feature = "settings")]
                CxRequest::Credentials(ops) => {
                    // The screen was drawn behind its gate, but the session can have dropped
                    // since — an earlier request this frame may have locked the panel.
                    let gate = self
                        .registry
                        .screen(crate::access::CREDENTIALS_SCREEN)
                        .map_or_else(
                            || Gate::borrowed(crate::access::CREDENTIALS_SCREEN),
                            ScreenDecl::gate_name,
                        );
                    let allowed = self.access.allows(&gate);
                    self.access.apply_admin(ops, allowed);
                }
                #[cfg(feature = "settings")]
                CxRequest::ListCredentials => self.access.mark_credentials_dirty(),
                #[cfg(feature = "settings")]
                CxRequest::WifiNetworkLongPressed { ssid, known } => {
                    // **It does not forget.** Where a hold leads is the integrator's.
                    self.events
                        .push(ShellEvent::WifiNetworkLongPressed { ssid, known });
                }
                #[cfg(feature = "settings")]
                CxRequest::WifiNetworkTapped {
                    ssid,
                    secured,
                    known,
                } => {
                    // **It does not connect.** The password, where the profile lives and whether a
                    // saved network should reconnect on a tap are the device's policy.
                    self.events.push(ShellEvent::WifiNetworkTapped {
                        ssid,
                        secured,
                        known,
                    });
                }
            }
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
            anim_scope: egui::Id::new("fairing.prompt"),
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
