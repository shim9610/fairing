//! The status bar. Left / centre / right slots, the built-in items (`status.*`) and
//! integrator `status_item` declarations.
//!
//! The layout is **worked out directly**: each item is measured, the left ones go from the left,
//! the right ones from the right end, and the centre ones sit in the middle of what is left. Which
//! is why [`StatusBar::item_rect`] is the real Rect rather than an approximation (so tests need not
//! duplicate the layout arithmetic), and why, when the width runs short, items can be collapsed by
//! one rule — the lowest `priority` first, and on a tie the rightmost.
//!
//! **Zero heap allocation per frame** is the goal: [`StatusBar`] holds the layout buffer
//! and the text galleys and rebuilds them only when a value changes. The clock builds a string only
//! when the displayed value (the minute, or the second) changes.
//!
//! [`crate::shell::ShellBuilder::status_bar_layout`] places the items (rung 4 of the
//! override ladder): it is handed the built-in placement and moves what it wants, and the bar still
//! measures, collapses, draws and presses the items where it says.

// UI geometry: pixel values and small integers crossing to f32. The loss is meaningless in this range,
// so the cast lints are lifted for the whole file (the rest of pedantic stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::bar::{BarCx, BarItemRow, BarKind, BarPainter, RowSink};
use crate::access::{Access, Gate};
use crate::config::StatusBarConfig;
use crate::icons::parametric::{self, BtIconState};
use crate::icons::ParamFade;
use crate::icons::{IconColor, IconStyle};
use crate::screen::{Cx, CxParts, LaunchAction, PaneInfo};
use crate::services::Capabilities;
use crate::theme::{ColorRole, Theme};
use crate::time::ClockFormat;
use crate::workspace::InstanceId;
use egui::text::Galley;
use egui::{Color32, Rect, Sense};
use std::sync::Arc;

/// The first frame's width estimate for an integrator item (the real width is used from the next frame on).
const CUSTOM_WIDTH_GUESS: f32 = 72.0;

/// A slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Left.
    Left,
    /// Centre.
    Center,
    /// Right.
    Right,
}

impl Slot {
    /// The right-first order used when collapsing (the higher one collapses first).
    fn rank(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Center => 1,
            Self::Right => 2,
        }
    }
}

/// The kinds of built-in item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusItem {
    /// The clock.
    Clock(ClockFormat),
    /// The Wi-Fi strength.
    Wifi,
    /// Bluetooth.
    Bluetooth,
    /// The battery.
    Battery,
    /// Ethernet (M4).
    Ethernet,
    /// The volume (M4).
    Volume,
    /// The brightness (M4).
    Brightness,
    /// The unread notification count (M2).
    Notifications,
    /// The current subject's name plus the level dot.
    User,
    /// An open padlock while the session is above where it started — unlocked by the prompt,
    /// or set higher by the integrator. A tap is [`LaunchAction::Logout`]: back to the start.
    /// Not drawn otherwise.
    Lock,
    /// A fixed string.
    Text(String),
}

impl StatusItem {
    /// The built-in id (`status.<kind>`).
    #[must_use]
    pub fn builtin_id(&self) -> &'static str {
        match self {
            Self::Clock(_) => "status.clock",
            Self::Wifi => "status.wifi",
            Self::Bluetooth => "status.bluetooth",
            Self::Battery => "status.battery",
            Self::Ethernet => "status.ethernet",
            Self::Volume => "status.volume",
            Self::Brightness => "status.brightness",
            Self::Notifications => "status.notifications",
            Self::User => "status.user",
            Self::Lock => "status.lock",
            Self::Text(_) => "status.text",
        }
    }

    /// id → the built-in item.
    #[must_use]
    pub fn from_id(id: &str, clock: ClockFormat) -> Option<Self> {
        Some(match id {
            "status.clock" => Self::Clock(clock),
            "status.wifi" => Self::Wifi,
            "status.bluetooth" => Self::Bluetooth,
            "status.battery" => Self::Battery,
            "status.ethernet" => Self::Ethernet,
            "status.volume" => Self::Volume,
            "status.brightness" => Self::Brightness,
            "status.notifications" => Self::Notifications,
            "status.user" => Self::User,
            "status.lock" => Self::Lock,
            _ => return None,
        })
    }

    /// Whether it is drawn as text (a candidate for the galley cache).
    fn is_text(&self) -> bool {
        matches!(self, Self::Clock(_) | Self::Text(_) | Self::User)
    }
}

/// A built-in item's spec (`StatusItemSpec`).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusItemSpec {
    /// id (`status.clock` …).
    pub id: String,
    /// The kind.
    pub item: StatusItem,
    /// on/off.
    pub enabled: bool,
    /// The gate (the id by default). Not drawn if it fails.
    pub gate: Option<Gate>,
    /// An icon size / colour override.
    pub style: Option<IconStyle>,
    /// The collapse priority when the width runs short (the lower, the sooner it collapses).
    pub priority: i8,
    /// An item tap → the related settings screen, and so on. With `None`, a tap does nothing.
    pub tap_action: Option<LaunchAction>,
    /// A caption shown **alongside** the icon.
    ///
    /// On a device's status bar an icon alone often says nothing about what the value is — an
    /// instrument needs words like "flow" or "kPa" to be read. Given a caption, the item becomes
    /// not one icon but an **icon-and-text pair**.
    pub label: Option<String>,
    /// Where the caption goes.
    pub label_pos: LabelPos,
    /// The caption's text size (du). `None` means [`Metrics::status_label_size`](crate::theme::Metrics).
    pub label_size: Option<f32>,
    /// The value's text size (du). Used by the clock, `Text` and `User`. `None` means
    /// [`Metrics::status_text_size`](crate::theme::Metrics).
    pub text_size: Option<f32>,
    /// The gap between the icon and the caption. `None` means 0.3 × the text size.
    pub gap: Option<f32>,
    /// The item's side padding. `None` means 0 (the slot gap only).
    pub pad_x: Option<f32>,
    /// The minimum width. For keeping a slot from shifting as its value wobbles (a clock, a number).
    pub min_width: Option<f32>,
}

impl Default for StatusItemSpec {
    fn default() -> Self {
        Self {
            id: String::new(),
            item: StatusItem::Text(String::new()),
            enabled: true,
            gate: None,
            style: None,
            priority: 0,
            tap_action: None,
            label: None,
            label_pos: LabelPos::default(),
            label_size: None,
            text_size: None,
            gap: None,
            pad_x: None,
            min_width: None,
        }
    }
}

/// Which side of the icon the caption goes on, within an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelPos {
    /// Below the icon (a vertical pair). The commonest arrangement on real instrumentation.
    #[default]
    Below,
    /// Above the icon.
    Above,
    /// To the right of the icon (a horizontal pair).
    Right,
    /// To the left of the icon.
    Left,
}

impl LabelPos {
    /// Whether it is a vertical pair.
    #[must_use]
    pub fn is_vertical(self) -> bool {
        matches!(self, Self::Below | Self::Above)
    }
}

/// The closure that draws a top-bar item.
pub(super) type StatusItemUi = Box<dyn FnMut(&mut egui::Ui, &mut Cx<'_>)>;

/// An integrator top-bar item declaration (`status_item(id, Slot, |ui, cx| ..)`).
pub struct StatusItemDecl {
    pub(crate) id: String,
    pub(crate) slot: Slot,
    pub(crate) ui: StatusItemUi,
    pub(crate) gate: Option<Gate>,
    pub(crate) priority: i8,
    pub(crate) enabled: bool,
}

impl std::fmt::Debug for StatusItemDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusItemDecl")
            .field("id", &self.id)
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

/// A top-bar item declaration.
pub fn status_item(
    id: impl Into<String>,
    slot: Slot,
    ui: impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static,
) -> StatusItemDecl {
    let id = id.into();
    StatusItemDecl {
        gate: Some(Gate::from(&id)),
        id,
        slot,
        ui: Box::new(ui),
        priority: 0,
        enabled: true,
    }
}

impl StatusItemDecl {
    /// The gate name (the id by default).
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }

    /// The collapse priority.
    #[must_use]
    pub fn priority(mut self, priority: i8) -> Self {
        self.priority = priority;
        self
    }

    /// on/off.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The slot.
    #[must_use]
    pub fn slot(&self) -> Slot {
        self.slot
    }

    /// The gate name.
    #[must_use]
    pub fn gate_name(&self) -> Gate {
        self.gate.clone().unwrap_or_else(|| Gate::from(&self.id))
    }
}

/// What happened in the status bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusBarAction {
    /// A tap on the bar itself (`tap_opens_shade`, M2).
    Tapped,
    /// An item tap.
    ItemTapped(String),
}

/// One item's place this frame. The layout buffer is reused by [`StatusBar`].
#[derive(Debug, Clone, Copy)]
struct Placed {
    slot: Slot,
    /// The index within the slot list (`left` / `center` / `right`). `None` for an integrator declaration outside the list.
    spec: Option<usize>,
    /// The index within the `custom` array.
    custom: Option<usize>,
    width: f32,
    priority: i8,
    rect: Rect,
    /// Whether it is drawn. The collapse leaves an item out when the width runs short, and
    /// a layout leaves one out by emptying its rect — or brings a collapsed one back.
    shown: bool,
}

/// What a status bar layout places the items with (rung 4).
///
/// The layout gets the rects **already laid out the built-in way** — measured, the lowest
/// priority collapsed where the width runs short, the left items from the left edge, the right ones
/// from the right and the centre ones in the middle of what is left — **one per item the bar would
/// draw**, in left, centre, right order (the declarations a slot list does not name at the end of
/// their slot). It changes the ones it wants:
///
/// ```no_run
/// # fn main() -> fairing::Result<()> {
/// # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
/// let shell = fairing::Shell::builder(config)
///     .status_bar_layout(|bar: &fairing::StatusLayoutCx<'_>, rects: &mut [egui::Rect]| {
///         // The clock in the middle of the bar, whatever slot it is in.
///         if let Some(clock) = bar.index_of("status.clock").and_then(|i| rects.get_mut(i)) {
///             *clock = clock.translate(egui::vec2(bar.rect.center().x - clock.center().x, 0.0));
///         }
///     })
///     .build(&ctx)?;
/// # let _ = shell;
/// # Ok(())
/// # }
/// ```
///
/// An item the built-in collapse left out comes as `Rect::NOTHING` ([`is_collapsed`](Self::is_collapsed)
/// says which); give it a rect and it is drawn there. A rect the layout empties leaves its item
/// out: not drawn, not pressed, and [`StatusBar::item_rect`] says `None`. A rect reaching outside
/// the bar is cut by the bar. An integrator item's width is the one it drew last frame.
pub struct StatusLayoutCx<'a> {
    /// The bar.
    pub rect: Rect,
    /// The bar less its side padding (`status_edge_pad`) — where the built-in placement goes.
    pub inner: Rect,
    /// The theme.
    pub theme: &'a Theme,
    placed: &'a [Placed],
    slots: [&'a [StatusItemSpec]; 3],
    custom: &'a [StatusItemDecl],
}

impl StatusLayoutCx<'_> {
    /// How many items there are — one rect each.
    #[must_use]
    pub fn len(&self) -> usize {
        self.placed.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.placed.is_empty()
    }

    /// Item `index`'s id (`"status.clock"`, or a [`status_item`] declaration's).
    #[must_use]
    pub fn id(&self, index: usize) -> Option<&str> {
        let placed = self.placed.get(index)?;
        let slot = self.slots.get(usize::from(placed.slot.rank()))?;
        match (placed.spec.and_then(|i| slot.get(i)), placed.custom) {
            (Some(spec), _) => Some(spec.id.as_str()),
            (None, Some(custom)) => self.custom.get(custom).map(|decl| decl.id.as_str()),
            (None, None) => None,
        }
    }

    /// The index of the item with id `id`.
    #[must_use]
    pub fn index_of(&self, id: &str) -> Option<usize> {
        (0..self.placed.len()).find(|&index| self.id(index) == Some(id))
    }

    /// Item `index`'s slot.
    #[must_use]
    pub fn slot(&self, index: usize) -> Option<Slot> {
        self.placed.get(index).map(|placed| placed.slot)
    }

    /// Whether the built-in collapse left item `index` out for want of width — its rect comes
    /// empty.
    #[must_use]
    pub fn is_collapsed(&self, index: usize) -> bool {
        self.placed.get(index).is_some_and(|placed| !placed.shown)
    }
}

impl std::fmt::Debug for StatusLayoutCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatusLayoutCx")
            .field("rect", &self.rect)
            .field("inner", &self.inner)
            .field("items", &self.placed.len())
            .finish_non_exhaustive()
    }
}

/// The callback that places the status bar's items. Given through
/// [`crate::shell::ShellBuilder::status_bar_layout`].
pub type StatusLayout = Box<dyn FnMut(&StatusLayoutCx<'_>, &mut [Rect])>;

/// The integrator's layout, where there is one — a box of its own so the bar stays `Debug`.
#[derive(Default)]
struct LayoutHook(Option<StatusLayout>);

impl std::fmt::Debug for LayoutHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "Some(..)" } else { "None" })
    }
}

/// A text item's galley cache. Laid out again only when the displayed value changes.
#[derive(Debug)]
struct TextCache {
    id: String,
    text: String,
    /// The clock's display-unit key (the minute, or the second). 0 for every other item.
    key: u64,
    /// **The clock format the string was written in** — the configured shape with the `ui.clock_12h`
    /// setting applied. `None` for every other item. It is what [`StatusBar::drawn_clock`]
    /// answers with, so that the accessor keeps its word: the format *drawn*, not the one configured.
    clock: Option<ClockFormat>,
}

/// The status bar.
#[derive(Debug)]
pub struct StatusBar {
    /// On or off.
    pub enabled: bool,
    /// The height.
    pub height: f32,
    /// The gap between two items this frame (`components.status_bar.item_gap`), read from the
    /// theme at the start of the frame for the stages that lay the bar out without it.
    item_gap: f32,
    /// Left.
    pub left: Vec<StatusItemSpec>,
    /// Centre.
    pub center: Vec<StatusItemSpec>,
    /// Right.
    pub right: Vec<StatusItemSpec>,
    /// The default icon style.
    pub icon: IconStyle,
    /// The background role.
    pub background: ColorRole,
    /// A tap → the shade (M2).
    pub tap_opens_shade: bool,
    /// The item Rects drawn last frame (by id). Only the values are overwritten each frame — a `String` is made once per id.
    item_rects: Vec<(String, Rect)>,
    /// The content width each integrator item last measured to, kept across frames (unlike
    /// `item_rects`, which a collapsed item leaves empty). The next frame's layout uses it.
    measured: Vec<(String, f32)>,
    /// The layout buffer (`clear` + `push` each frame, no reallocation).
    layout: Vec<Placed>,
    /// The text galley cache.
    texts: Vec<TextCache>,
    /// The parametric cross-fade state plus the notification badge (M2).
    fades: StatusFades,
    /// The integrator's placement of the items (rung 4).
    layout_hook: LayoutHook,
    /// The rects handed to that placement — a buffer overwritten each frame.
    cells: Vec<Rect>,
}

/// The parametric icon cross-fades (A7) and the `status.notifications` badge value (M2).
#[derive(Debug, Clone, Copy)]
pub(super) struct StatusFades {
    /// Wi-Fi (the strength, and off).
    pub wifi: ParamFade<(u8, bool)>,
    /// The battery (the level, and charging).
    pub battery: ParamFade<(u8, bool)>,
    /// Bluetooth.
    pub bluetooth: ParamFade<BtIconState>,
    /// The volume (the step, and muted).
    pub volume: ParamFade<(u8, bool)>,
    /// The unread notification count (the shell hands it over every frame with `set_unread`).
    pub unread: u32,
}

impl StatusFades {
    /// Give every fade the theme's crossfade — called each frame before a state is set, since
    /// the fades are made from the config before the theme is, and the token can change with it.
    fn follow(&mut self, crossfade: std::time::Duration) {
        self.wifi.set_duration(crossfade);
        self.battery.set_duration(crossfade);
        self.bluetooth.set_duration(crossfade);
        self.volume.set_duration(crossfade);
    }
}

impl Default for StatusFades {
    fn default() -> Self {
        // A placeholder until `follow` runs with the theme's `motion.crossfade`.
        let d = std::time::Duration::ZERO;
        Self {
            wifi: ParamFade::new((0, true), d),
            battery: ParamFade::new((0, false), d),
            bluetooth: ParamFade::new(BtIconState::Off, d),
            volume: ParamFade::new((0, false), d),
            unread: 0,
        }
    }
}

/// Every built-in item id. `status.ethernet` · `status.volume` · `status.brightness` show only
/// where the backend behind them reports the capability (`NetworkBackend` / `AudioBackend` /
/// `DisplayBackend`, "a capability not reported is not drawn") — with the `Null` defaults
/// they stay hidden, and an integrator's own backend brings them up.
/// `status.lock` shows only while the session is unlocked.
pub const BUILTIN_IDS: &[&str] = &[
    "status.clock",
    "status.wifi",
    "status.bluetooth",
    "status.battery",
    "status.ethernet",
    "status.volume",
    "status.brightness",
    "status.notifications",
    "status.user",
    "status.lock",
];

impl StatusBar {
    /// Build from the config. Built-in ids in a slot list become specs; integrator ids have only
    /// their place remembered. The gates are always filled in (the id by default) so that no `Gate`
    /// is built on the render path.
    #[must_use]
    pub fn from_config(cfg: &StatusBarConfig) -> Self {
        let clock = match cfg.clock_format.as_str() {
            "hms" => ClockFormat::Hms,
            "hm12" => ClockFormat::Hm12,
            "date_hm" => ClockFormat::DateHm,
            "hms12" => ClockFormat::Hms12,
            "date_hm12" => ClockFormat::DateHm12,
            "hm" => ClockFormat::Hm,
            other => {
                // Falling back quietly would leave a typo as nothing but "why are the seconds not showing".
                log::warn!(
                    "[status_bar] clock_format = \"{other}\" is none of hm, hms, hm12, date_hm, hms12 or date_hm12 - using hm"
                );
                ClockFormat::Hm
            }
        };
        let specs = |ids: &[String]| -> Vec<StatusItemSpec> {
            ids.iter()
                .map(|id| StatusItemSpec {
                    id: id.clone(),
                    item: StatusItem::from_id(id, clock)
                        .unwrap_or_else(|| StatusItem::Text(String::new())),
                    gate: Some(Gate::from(id)),
                    tap_action: builtin_tap_action(id),
                    ..StatusItemSpec::default()
                })
                .collect()
        };
        Self {
            enabled: cfg.enabled,
            // `build` sets the resolved sizes straight after; these are only the start.
            height: cfg
                .height
                .unwrap_or(crate::theme::Metrics::default().status_bar_height),
            left: specs(&cfg.left),
            center: specs(&cfg.center),
            right: specs(&cfg.right),
            icon: IconStyle::sized(
                cfg.icon_size
                    .unwrap_or(crate::theme::Metrics::default().status_icon_size),
            )
            .color(
                IconColor::parse(&cfg.icon_color).unwrap_or(IconColor::Role(ColorRole::OnSurface)),
            ),
            background: ColorRole::Surface,
            tap_opens_shade: cfg.tap_opens_shade,
            item_rects: Vec::new(),
            measured: Vec::new(),
            layout: Vec::new(),
            texts: Vec::new(),
            fades: StatusFades::default(),
            item_gap: 0.0,
            layout_hook: LayoutHook::default(),
            cells: Vec::new(),
        }
    }

    /// Place the items with `layout` (rung 4).
    pub(crate) fn set_layout(&mut self, layout: StatusLayout) {
        self.layout_hook = LayoutHook(Some(layout));
    }

    /// The unread notification count (the `status.notifications` badge). The shell hands it over from the notification centre every frame (M2).
    pub(crate) fn set_unread(&mut self, unread: u32) {
        self.fades.unread = unread;
    }

    /// The unread count the `status.notifications` badge is holding (the last value the shell handed over).
    #[must_use]
    pub fn unread(&self) -> u32 {
        self.fades.unread
    }

    /// Whether a parametric cross-fade is running (grounds for a repaint, A7).
    #[must_use]
    pub fn is_animating(&self, now: std::time::Instant) -> bool {
        self.fades.wifi.is_animating(now)
            || self.fades.battery.is_animating(now)
            || self.fades.bluetooth.is_animating(now)
            || self.fades.volume.is_animating(now)
    }

    /// Remove a built-in item (`shell.remove("status.clock")`). `true` if it was there.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.left.len() + self.center.len() + self.right.len();
        self.left.retain(|s| s.id != id);
        self.center.retain(|s| s.id != id);
        self.right.retain(|s| s.id != id);
        self.item_rects.retain(|(i, _)| i != id);
        self.measured.retain(|(i, _)| i != id);
        self.texts.retain(|t| t.id != id);
        before != self.left.len() + self.center.len() + self.right.len()
    }

    /// The spec in the slot list (both built-in and integrator places).
    #[must_use]
    pub fn spec(&self, id: &str) -> Option<&StatusItemSpec> {
        self.left
            .iter()
            .chain(&self.center)
            .chain(&self.right)
            .find(|s| s.id == id)
    }

    /// Adjust an item's spec. The caption, text size, padding and minimum width can be
    /// different for each item.
    ///
    /// ```no_run
    /// # let shell: &mut fairing::Shell = todo!();
    /// use fairing::chrome::LabelPos;
    ///
    /// if let Some(spec) = shell.status_bar_mut().spec_mut("status.wifi") {
    ///     spec.label = Some("Network".to_owned());
    ///     spec.label_pos = LabelPos::Below;
    ///     spec.label_size = Some(11.0);
    /// }
    /// ```
    pub fn spec_mut(&mut self, id: &str) -> Option<&mut StatusItemSpec> {
        self.left
            .iter_mut()
            .chain(self.center.iter_mut())
            .chain(self.right.iter_mut())
            .find(|s| s.id == id)
    }

    /// The action to run when an item is tapped (`tap_action`). The shell looks it up on receiving `ItemTapped(id)`.
    #[must_use]
    pub fn tap_action(&self, id: &str) -> Option<&LaunchAction> {
        self.spec(id).and_then(|s| s.tap_action.as_ref())
    }

    /// The format of the clock item **actually drawn last frame**. The shell asks when choosing its
    /// idle repaint interval — with the status bar off, the chrome policy `Hide`
    /// (`StatusBar::mark_hidden`), or the clock invisible through a failed gate or a collapse, it
    /// is `None` and the shell does not wake for the clock (repainting "the clock at the next
    /// minute boundary" is a rule for when the clock is visible). No allocation.
    ///
    /// It is the **resolved** format, `ui.clock_12h` included — the configured shape is
    /// [`StatusBar::configured_clock`]. Between the two, this is the one that was on screen.
    #[must_use]
    pub fn drawn_clock(&self) -> Option<ClockFormat> {
        if !self.enabled {
            return None;
        }
        [&self.left, &self.center, &self.right]
            .into_iter()
            .flatten()
            .filter(|spec| spec.enabled)
            .find_map(|spec| match spec.item {
                StatusItem::Clock(configured) if self.item_rect(&spec.id).is_some() => Some(
                    self.texts
                        .iter()
                        .find(|cache| cache.id == spec.id)
                        .and_then(|cache| cache.clock)
                        .unwrap_or(configured),
                ),
                _ => None,
            })
    }

    /// The clock **shape as configured**, whether or not it was drawn — the seed for the
    /// `ui.clock_12h` setting at startup. `None` where there is no clock item at all.
    #[must_use]
    pub fn configured_clock(&self) -> Option<ClockFormat> {
        [&self.left, &self.center, &self.right]
            .into_iter()
            .flatten()
            .find_map(|spec| match spec.item {
                StatusItem::Clock(format) => Some(format),
                _ => None,
            })
    }

    /// Whether a clock showing seconds (`hms`) was drawn last frame — the convenience form of
    /// [`StatusBar::drawn_clock`]. No allocation.
    #[must_use]
    pub fn shows_seconds(&self) -> bool {
        self.drawn_clock().is_some_and(ClockFormat::shows_seconds)
    }

    /// The Rect of an item drawn last frame. `None` if it was not drawn (collapsed, gated, or the
    /// capability not reported). Exposed so that tests need not duplicate the layout arithmetic.
    #[must_use]
    pub fn item_rect(&self, id: &str) -> Option<Rect> {
        self.item_rects
            .iter()
            .find(|(i, _)| i == id)
            .map(|(_, r)| *r)
            .filter(Rect::is_positive)
    }

    /// Called by the shell instead when the bar is not drawn this frame (chrome policy `Hide`,
    /// `enabled = false`) — it clears the record so [`StatusBar::item_rect`] does not hand back a
    /// stale Rect from last frame. No allocation (the `String`s are kept).
    pub(crate) fn mark_hidden(&mut self) {
        for (_, rect) in &mut self.item_rects {
            *rect = Rect::NOTHING;
        }
    }

    /// Draw. `ui` is inside the top panel (or an Overlay `Area`). `custom` is the registry's
    /// integrator items. Zero heap allocation per frame is the goal — the slot lists are borrowed,
    /// not cloned.
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        custom: &mut [StatusItemDecl],
    ) -> Option<StatusBarAction> {
        let rect = ui.max_rect();
        // `BarMode::Overlay` is put up by the shell as an `Order::Foreground` Area (z-order).
        // The panel path is a background layer, so they can be told apart by order — the translucent
        // background is this one's job.
        let overlay = ui.layer_id().order != egui::Order::Background;
        let background = parts.theme.color(self.background);
        ui.painter().rect_filled(
            rect,
            0.0,
            if overlay {
                background.gamma_multiply(parts.theme.metrics.status_overlay_alpha)
            } else {
                background
            },
        );
        // The bar's own tap is registered first — an item registered later wins the hit test.
        let bar = ui.interact(rect, egui::Id::new("fairing.status_bar"), Sense::click());
        let inner = rect.shrink2(egui::vec2(parts.theme.metrics.status_edge_pad, 0.0));

        self.measure(ui, parts, custom);
        self.item_gap = parts.theme.components.status_bar.item_gap;
        self.collapse(inner.width());
        self.position(inner);
        self.place_by_layout(rect, inner, parts.theme, custom);
        let action = self.render(ui, parts, custom);
        action.or_else(|| bar.clicked().then_some(StatusBarAction::Tapped))
    }

    /// Forget the text drawn last: the next frame writes it again — a new language spells the
    /// clock differently, and the cache is keyed by the minute alone.
    pub(crate) fn forget_texts(&mut self) {
        for cache in &mut self.texts {
            cache.text.clear();
        }
    }

    /// Hand the whole bar to an integrator painter. The shell keeps only the Rect, the gate
    /// decisions, the item list and **taps on the bar itself** ([`StatusBarAction::Tapped`] → the
    /// shade), and does not call the built-in rendering ([`StatusBar::ui`]) — the background is the
    /// painter's too. Item taps are taken by the painter's own widgets and asked for through
    /// `cx.shell` (registered after the bar tap, so they win).
    pub(crate) fn ui_with(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        rows: &[BarItemRow],
        painter: &mut BarPainter,
    ) -> Option<StatusBarAction> {
        // No built-in item was drawn, so last frame's Rects are not left behind.
        self.mark_hidden();
        let rect = ui.max_rect();
        let bar = ui.interact(rect, egui::Id::new("fairing.status_bar"), Sense::click());
        let pane = PaneInfo {
            rect,
            outer: rect,
            is_split: false,
            is_focused: false,
            inset_bottom: 0.0,
            instance: InstanceId::NONE,
        };
        {
            let mut cx = BarCx::new(BarKind::Status, rect, false, rows, parts.cx(pane, None));
            painter(ui, &mut cx);
        }
        bar.clicked().then_some(StatusBarAction::Tapped)
    }

    /// Fill a buffer with the item list to hand to the painter (slots and gate decisions included).
    /// The order is the built-in rendering's layout order (left → centre → right, with the
    /// declarations outside the list after the slot list). No allocation.
    pub(crate) fn collect_items(
        &self,
        custom: &[StatusItemDecl],
        access: &Access,
        rows: &mut Vec<BarItemRow>,
    ) {
        let mut sink = RowSink::new(rows);
        for (slot, specs) in [
            (Slot::Left, &self.left),
            (Slot::Center, &self.center),
            (Slot::Right, &self.right),
        ] {
            for spec in specs {
                let decl = custom.iter().find(|d| d.id == spec.id);
                sink.push(
                    &spec.id,
                    Some(slot),
                    spec.enabled && decl.is_none_or(|d| d.enabled),
                    gate_allows(access, spec.gate.as_ref(), &spec.id),
                );
            }
            // An integrator declaration not in the list goes at the end of the slot.
            for decl in custom {
                if decl.slot != slot || specs.iter().any(|s| s.id == decl.id) {
                    continue;
                }
                sink.push(
                    &decl.id,
                    Some(slot),
                    decl.enabled,
                    gate_allows(access, decl.gate.as_ref(), &decl.id),
                );
            }
        }
    }

    /// Stage 1: gather the items to draw, and their widths, into [`Self::layout`].
    fn measure(&mut self, ui: &egui::Ui, parts: &mut CxParts<'_>, custom: &[StatusItemDecl]) {
        // **The single source of the icon size is the theme too**. When `[status_bar]
        // icon_size` was made, it was read once and cached — and on a device that injected a theme whole
        // through `ShellBuilder::theme`, the bar height followed while the icons alone did not.
        // `Theme::from_config` moves the config value into `metrics.status_icon_size`, so the config path
        // stays alive as it was.
        self.icon.size = parts.theme.metrics.status_icon_size;
        let Self {
            left,
            center,
            right,
            icon,
            layout,
            texts,
            measured,
            fades,
            ..
        } = self;
        layout.clear();
        let painter = ui.painter();
        for (slot, specs) in [
            (Slot::Left, &*left),
            (Slot::Center, &*center),
            (Slot::Right, &*right),
        ] {
            for (index, spec) in specs.iter().enumerate() {
                if !spec.enabled || !allowed(parts, spec.gate.as_ref(), &spec.id) {
                    continue;
                }
                if let Some(custom_index) = custom.iter().position(|d| d.id == spec.id) {
                    let Some(decl) = custom.get(custom_index) else {
                        continue;
                    };
                    if !decl.enabled {
                        continue;
                    }
                    layout.push(Placed {
                        slot,
                        spec: Some(index),
                        custom: Some(custom_index),
                        width: recorded_width(measured, &spec.id),
                        priority: spec.priority,
                        rect: Rect::NOTHING,
                        shown: true,
                    });
                    continue;
                }
                let style = spec.style.unwrap_or(*icon);
                let Some(width) = measure_builtin(spec, style, painter, parts, texts, fades.unread)
                else {
                    continue;
                };
                layout.push(Placed {
                    slot,
                    spec: Some(index),
                    custom: None,
                    width,
                    priority: spec.priority,
                    rect: Rect::NOTHING,
                    shown: true,
                });
            }
            // An integrator declaration not in the list goes at the end of the slot.
            for (custom_index, decl) in custom.iter().enumerate() {
                if decl.slot != slot || !decl.enabled || specs.iter().any(|s| s.id == decl.id) {
                    continue;
                }
                if !allowed(parts, decl.gate.as_ref(), &decl.id) {
                    continue;
                }
                layout.push(Placed {
                    slot,
                    spec: None,
                    custom: Some(custom_index),
                    width: recorded_width(measured, &decl.id),
                    priority: decl.priority,
                    rect: Rect::NOTHING,
                    shown: true,
                });
            }
        }
    }

    /// Stage 2: when the width runs short, collapse the lowest `priority` first and on a tie the
    /// rightmost. The last one stays — overflowing and clipped still beats an empty bar. A
    /// collapsed item stays in the buffer, not shown, so a layout can still be told of it.
    fn collapse(&mut self, available: f32) {
        while total_width(&self.layout, self.item_gap) > available
            && self.layout.iter().filter(|placed| placed.shown).count() > 1
        {
            let Some(victim) = self
                .layout
                .iter_mut()
                .enumerate()
                .filter(|(_, placed)| placed.shown)
                .min_by_key(|(index, placed)| {
                    (
                        placed.priority,
                        std::cmp::Reverse((placed.slot.rank(), *index)),
                    )
                })
                .map(|(_, placed)| placed)
            else {
                break;
            };
            victim.shown = false;
        }
    }

    /// Stage 3: the left ones from the left, the right ones from the right end, and the centre ones in the middle of what is left.
    fn position(&mut self, inner: Rect) {
        let gap = self.item_gap;
        let left_span = slot_span(&self.layout, Slot::Left, gap);
        let right_span = slot_span(&self.layout, Slot::Right, gap);
        let center_span = slot_span(&self.layout, Slot::Center, gap);
        let center_start = (inner.center().x - center_span / 2.0)
            .max(inner.min.x + left_span + if left_span > 0.0 { gap } else { 0.0 })
            .min(inner.max.x - right_span - center_span - if right_span > 0.0 { gap } else { 0.0 })
            .max(inner.min.x);
        let mut cursor = [inner.min.x, center_start, inner.max.x - right_span];
        for placed in &mut self.layout {
            if !placed.shown {
                placed.rect = Rect::NOTHING;
                continue;
            }
            let bucket = usize::from(placed.slot.rank());
            let start = cursor.get(bucket).copied().unwrap_or(inner.min.x);
            placed.rect = Rect::from_min_size(
                egui::pos2(start, inner.min.y),
                egui::vec2(placed.width, inner.height()),
            );
            if let Some(slot) = cursor.get_mut(bucket) {
                *slot = start + placed.width + gap;
            }
        }
    }

    /// Stage 3½: the integrator's layout, where there is one, moves what it wants. An
    /// emptied rect leaves its item out, a rect given to a collapsed item brings it back, and every
    /// rect is cut by the bar.
    fn place_by_layout(
        &mut self,
        rect: Rect,
        inner: Rect,
        theme: &Theme,
        custom: &[StatusItemDecl],
    ) {
        let Self {
            left,
            center,
            right,
            layout,
            layout_hook,
            cells,
            ..
        } = self;
        let Some(hook) = layout_hook.0.as_mut() else {
            return;
        };
        cells.clear();
        cells.extend(layout.iter().map(|placed| placed.rect));
        let bar = StatusLayoutCx {
            rect,
            inner,
            theme,
            placed: layout,
            slots: [left, center, right],
            custom,
        };
        hook(&bar, cells);
        for (placed, cell) in layout.iter_mut().zip(cells.iter()) {
            let cell = Some(*cell)
                .filter(Rect::is_positive)
                .map(|cell| cell.intersect(rect))
                .filter(Rect::is_positive);
            placed.rect = cell.unwrap_or(Rect::NOTHING);
            placed.shown = cell.is_some();
        }
    }

    /// Stage 4: draw, and take the taps.
    fn render(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        custom: &mut [StatusItemDecl],
    ) -> Option<StatusBarAction> {
        let pane = PaneInfo {
            rect: ui.max_rect(),
            outer: ui.max_rect(),
            is_split: false,
            is_focused: false,
            inset_bottom: 0.0,
            instance: InstanceId::NONE,
        };
        let Self {
            left,
            center,
            right,
            icon,
            layout,
            texts,
            item_rects,
            measured,
            fades,
            ..
        } = self;
        for (_, r) in item_rects.iter_mut() {
            *r = Rect::NOTHING;
        }
        let mut action = None;
        for placed in layout.iter().filter(|placed| placed.shown) {
            let specs: &[StatusItemSpec] = match placed.slot {
                Slot::Left => left,
                Slot::Center => center,
                Slot::Right => right,
            };
            let spec = placed.spec.and_then(|i| specs.get(i));
            let widget_id = match (spec, placed.custom.and_then(|i| custom.get(i))) {
                (Some(spec), _) => egui::Id::new(("fairing.status_bar.item", &spec.id)),
                (None, Some(decl)) => egui::Id::new(("fairing.status_bar.item", &decl.id)),
                (None, None) => continue,
            };
            let response = ui.interact(placed.rect, widget_id, Sense::click());
            let mut drawn = placed.rect;
            if let Some(index) = placed.custom {
                if let Some(decl) = custom.get_mut(index) {
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(placed.rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    let mut cx = parts.cx(pane, None);
                    (decl.ui)(&mut child, &mut cx);
                    // The real content width is recorded so the next frame's estimate converges.
                    let content = child.min_rect();
                    if content.is_positive() {
                        drawn = Rect::from_min_size(
                            placed.rect.min,
                            egui::vec2(content.width(), placed.rect.height()),
                        );
                        record_width(measured, &decl.id, content.width());
                    }
                }
            } else if let Some(spec) = spec {
                let style = spec.style.unwrap_or(*icon);
                draw_builtin(ui, placed.rect, spec, style, parts, texts, fades);
            }
            // The id is borrowed again after the mutable borrow of `custom` ends.
            let id: &str = match (spec, placed.custom.and_then(|i| custom.get(i))) {
                (Some(spec), _) => &spec.id,
                (None, Some(decl)) => &decl.id,
                (None, None) => continue,
            };
            record(item_rects, id, drawn);
            if response.clicked() {
                action = Some(StatusBarAction::ItemTapped(id.to_owned()));
            }
        }
        // An integrator item that collapsed before it was ever drawn is run once, out of sight,
        // to measure it: laid out on the estimate alone it could stay collapsed for good where
        // its real width fits. Measured, a collapsed item is not run again.
        for placed in layout.iter().filter(|placed| !placed.shown) {
            let Some(decl) = placed.custom.and_then(|i| custom.get_mut(i)) else {
                continue;
            };
            if measured.iter().any(|(id, _)| *id == decl.id) {
                continue;
            }
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(ui.max_rect())
                    .layout(egui::Layout::left_to_right(egui::Align::Center))
                    .invisible(),
            );
            let mut cx = parts.cx(pane, None);
            (decl.ui)(&mut child, &mut cx);
            // Something that draws nothing is recorded too, so it is not run again.
            let content = child.min_rect();
            let width = if content.is_positive() {
                content.width()
            } else {
                CUSTOM_WIDTH_GUESS
            };
            record_width(measured, &decl.id, width);
        }
        action
    }
}

/// The total width the shown items take up (`gap`s included).
fn total_width(layout: &[Placed], gap: f32) -> f32 {
    let shown = layout.iter().filter(|p| p.shown).count();
    if shown == 0 {
        return 0.0;
    }
    layout
        .iter()
        .filter(|p| p.shown)
        .map(|p| p.width)
        .sum::<f32>()
        + gap * (shown - 1) as f32
}

/// The width one slot's shown items take up (`gap`s included).
fn slot_span(layout: &[Placed], slot: Slot, gap: f32) -> f32 {
    let mut total = 0.0;
    let mut count = 0usize;
    for placed in layout.iter().filter(|p| p.shown && p.slot == slot) {
        total += placed.width;
        count += 1;
    }
    if count == 0 {
        0.0
    } else {
        total + gap * (count - 1) as f32
    }
}

/// The width an integrator item last measured to, or the estimate before it has been measured.
fn recorded_width(measured: &[(String, f32)], id: &str) -> f32 {
    measured
        .iter()
        .find(|(i, _)| i == id)
        .map_or(CUSTOM_WIDTH_GUESS, |(_, w)| *w)
}

/// Record an integrator item's measured width. A `String` is made only for an id seen for the
/// first time.
fn record_width(measured: &mut Vec<(String, f32)>, id: &str, width: f32) {
    match measured.iter_mut().find(|(i, _)| i == id) {
        Some((_, w)) => *w = width,
        None => measured.push((id.to_owned(), width)),
    }
}

/// The gate decision. The `None` fallback (a `StatusItemSpec` an integrator built themselves) is
/// [`Access::allows_name`], so no `Gate` is built each frame.
fn allowed(parts: &CxParts<'_>, gate: Option<&Gate>, id: &str) -> bool {
    gate_allows(parts.access, gate, id)
}

/// The gate decision (the path with only an [`Access`] — the item list for a painter).
fn gate_allows(access: &Access, gate: Option<&Gate>, id: &str) -> bool {
    match gate {
        Some(gate) => access.allows(gate),
        None => access.allows_name(id),
    }
}

/// Record last frame's Rect. An id already there has only its value overwritten; a `String` is made only for an id seen for the first time.
fn record(rects: &mut Vec<(String, Rect)>, id: &str, rect: Rect) {
    match rects.iter_mut().find(|(i, _)| i == id) {
        Some((_, r)) => *r = rect,
        None => rects.push((id.to_owned(), rect)),
    }
}

/// The text galley cache's slot. A `String` is made only for an id seen for the first time.
fn cache_index(texts: &mut Vec<TextCache>, id: &str) -> usize {
    if let Some(index) = texts.iter().position(|t| t.id == id) {
        return index;
    }
    texts.push(TextCache {
        id: id.to_owned(),
        text: String::new(),
        key: 0,
        clock: None,
    });
    texts.len() - 1
}

/// Unchanged content is left alone. Otherwise the buffer is overwritten (with no allocation while the capacity holds) and `true`.
fn set_text(cache: &mut TextCache, text: &str) -> bool {
    if cache.text == text {
        return false;
    }
    cache.text.clear();
    cache.text.push_str(text);
    true
}

/// Whether the device is set to a 12-hour clock — [`keys::UI_CLOCK_12H`], written by the built-in
/// `settings.datetime` screen. Absent means 24-hour.
fn wants_hour12(settings: &crate::settings::SettingsView) -> bool {
    matches!(
        settings.get(&crate::settings::keys::UI_CLOCK_12H.into()),
        Some(crate::settings::SettingValue::Bool(true))
    )
}

/// A small stable number per format, so the galley cache key changes with the spelling.
const fn format_bits(format: ClockFormat) -> u64 {
    match format {
        ClockFormat::Hm => 0,
        ClockFormat::Hms => 1,
        ClockFormat::Hm12 => 2,
        ClockFormat::DateHm => 3,
        ClockFormat::Hms12 => 4,
        ClockFormat::DateHm12 => 5,
    }
}

/// Bring a text item's displayed value up to date and hand back the galley.
fn text_galley(
    spec: &StatusItemSpec,
    painter: &egui::Painter,
    parts: &CxParts<'_>,
    texts: &mut Vec<TextCache>,
) -> Option<Arc<Galley>> {
    let clock_size = parts.theme.metrics.status_text_size;
    let text_size = parts.theme.metrics.status_text_size;
    let index = cache_index(texts, &spec.id);
    let (changed, size) = match &spec.item {
        StatusItem::Clock(format) => {
            // The shape is the integrator's (`[status_bar] clock_format`); **the hour convention is
            // the device owner's** and can change while the shell runs, so it is resolved here from
            // the setting rather than baked in at build. It is part of the cache key too,
            // or the clock would keep the old spelling until the next minute rolled over.
            let format = format.with_hour12(wants_hour12(parts.settings));
            let now = parts.services.clock.now();
            let unit = if format.shows_seconds() {
                now.utc_secs
            } else {
                now.utc_secs / 60
            };
            let key =
                unit ^ ((i64::from(now.offset_min) as u64) << 40) ^ (format_bits(format) << 60);
            let cache = texts.get_mut(index)?;
            cache.clock = Some(format);
            // `cache.text.is_empty()` stands for "nothing has been written yet" — that used to be
            // `galley.is_none()`'s job, and the galley is no longer held.
            let changed = cache.key != key || cache.text.is_empty();
            if changed {
                cache.key = key;
                // Rewritten only when the displayed value changes (once a minute) — the buffer is reused, so there is no allocation either.
                now.format_into_with(
                    &mut cache.text,
                    format,
                    crate::time::Meridiem::of(parts.strings),
                );
            }
            (changed, spec.text_size.unwrap_or(clock_size))
        }
        StatusItem::Text(text) => (
            set_text(texts.get_mut(index)?, text),
            spec.text_size.unwrap_or(text_size),
        ),
        StatusItem::User => (
            set_text(
                texts.get_mut(index)?,
                parts.access.subject_name(parts.strings),
            ),
            spec.text_size.unwrap_or(text_size),
        ),
        _ => return None,
    };
    let _ = changed;
    let cache = texts.get(index)?;
    if cache.text.is_empty() {
        return None;
    }
    // **The galleys are not held across frames** — the font atlas UVs a `Galley` carries shift wholesale
    // when a new glyph arrives and the atlas grows, and from then on a cached galley draws the wrong
    // characters. Only the text buffer (`cache.text`) is reused and the layout is left to epaint's
    // `GalleyCache` — for the same string, that memoises it.
    Some(painter.layout_no_wrap(
        cache.text.clone(),
        egui::FontId::proportional(size),
        Color32::PLACEHOLDER,
    ))
}

/// How far the badge pill overlaps the bell icon's left edge (as a fraction of the pill's height).
const BADGE_OVERLAP: f32 = 0.4;

/// The `status.notifications` badge pill's `(width, height)`. A single digit is a circle, several a pill.
fn badge_size(icon_size: f32, text_w: f32) -> (f32, f32) {
    let height = (icon_size * 0.62).max(11.0);
    ((text_w + height * 0.5).max(height), height)
}

/// How far the badge sticks out to the right of the bell icon (0 = no badge).
fn badge_overhang(icon_size: f32, text_w: f32) -> f32 {
    if text_w <= 0.0 {
        return 0.0;
    }
    let (pill_w, pill_h) = badge_size(icon_size, text_w);
    (pill_w - pill_h * BADGE_OVERLAP).max(0.0)
}

/// Write the badge number into a buffer — `99+` past 99. No allocation (the buffer is reused).
fn write_badge(buf: &mut String, unread: u32) {
    buf.clear();
    if unread > 99 {
        buf.push_str("99+");
        return;
    }
    if unread >= 10 {
        buf.push(char::from(b'0' + u8::try_from(unread / 10).unwrap_or(9)));
    }
    buf.push(char::from(b'0' + u8::try_from(unread % 10).unwrap_or(9)));
}

/// The badge number's galley. Laid out again only when the unread count (or the icon size) changes. `None` at 0.
fn badge_galley(
    id: &str,
    unread: u32,
    painter: &egui::Painter,
    icon_size: f32,
    texts: &mut Vec<TextCache>,
) -> Option<Arc<Galley>> {
    if unread == 0 {
        return None;
    }
    let index = cache_index(texts, id);
    let cache = texts.get_mut(index)?;
    // Unlike the clock's, this cache is used by one item only, so the key can be (the number shown, the size).
    let key = u64::from(unread.min(100)) | ((icon_size.round() as u64) << 32);
    if cache.key != key || cache.text.is_empty() {
        cache.key = key;
        write_badge(&mut cache.text, unread);
    }
    Some(painter.layout_no_wrap(
        cache.text.clone(),
        egui::FontId::proportional(icon_size * 0.44),
        Color32::PLACEHOLDER,
    ))
}

/// An item caption's galley. `None` with no caption, or an empty one.
fn label_galley(
    spec: &StatusItemSpec,
    painter: &egui::Painter,
    theme: &Theme,
) -> Option<Arc<Galley>> {
    let text = spec.label.as_ref()?;
    if text.is_empty() {
        return None;
    }
    let size = spec.label_size.unwrap_or(theme.metrics.status_label_size);
    Some(painter.layout_no_wrap(
        text.clone(),
        egui::FontId::proportional(size),
        Color32::PLACEHOLDER,
    ))
}

/// The gap between the icon and the caption.
fn label_gap(spec: &StatusItemSpec, theme: &Theme) -> f32 {
    spec.gap
        .unwrap_or_else(|| spec.label_size.unwrap_or(theme.metrics.status_label_size) * 0.3)
}

/// The item's width with the caption included. `base` is the width of the icon and value alone.
fn item_width(spec: &StatusItemSpec, base: f32, label: Option<&Arc<Galley>>, theme: &Theme) -> f32 {
    let inner = match label {
        None => base,
        Some(g) if spec.label_pos.is_vertical() => base.max(g.size().x),
        Some(g) => base + label_gap(spec, theme) + g.size().x,
    };
    (inner + spec.pad_x.unwrap_or(0.0) * 2.0).max(spec.min_width.unwrap_or(0.0))
}

/// The `(icon-and-value place, caption place)` of an item with a caption. With no caption there is
/// only the one place.
///
/// A vertical pair puts the two of them, at their combined height, in the middle of the row —
/// centring the icon alone and hanging the caption below it would leave the whole pair sitting high.
fn split_for_label(
    spec: &StatusItemSpec,
    rect: Rect,
    label: &Arc<Galley>,
    icon_h: f32,
    theme: &Theme,
) -> (Rect, egui::Pos2) {
    let gap = label_gap(spec, theme);
    let ls = label.size();
    let pad = spec.pad_x.unwrap_or(0.0);
    let inner = Rect::from_min_max(
        egui::pos2(rect.min.x + pad, rect.min.y),
        egui::pos2(rect.max.x - pad, rect.max.y),
    );
    match spec.label_pos {
        LabelPos::Below | LabelPos::Above => {
            let total = icon_h + gap + ls.y;
            let top = inner.center().y - total * 0.5;
            let (icon_y, label_y) = if spec.label_pos == LabelPos::Below {
                (top, top + icon_h + gap)
            } else {
                (top + ls.y + gap, top)
            };
            (
                Rect::from_min_size(
                    egui::pos2(inner.min.x, icon_y),
                    egui::vec2(inner.width(), icon_h),
                ),
                egui::pos2(inner.center().x - ls.x * 0.5, label_y),
            )
        }
        LabelPos::Right => {
            let icon_w = (inner.width() - gap - ls.x).max(0.0);
            (
                Rect::from_min_size(inner.min, egui::vec2(icon_w, inner.height())),
                egui::pos2(inner.min.x + icon_w + gap, inner.center().y - ls.y * 0.5),
            )
        }
        LabelPos::Left => {
            let icon_w = (inner.width() - gap - ls.x).max(0.0);
            (
                Rect::from_min_size(
                    egui::pos2(inner.max.x - icon_w, inner.min.y),
                    egui::vec2(icon_w, inner.height()),
                ),
                egui::pos2(inner.min.x, inner.center().y - ls.y * 0.5),
            )
        }
    }
}

/// A built-in item's default tap behaviour (`tap_action`). The notifications item opens the
/// shade — being a [`LaunchAction::OpenOverlay`], the shell's `launch` checks the `overlay.open`
/// gate again. An integrator can change it by overwriting `StatusItemSpec::tap_action`.
/// The volume as [`parametric::volume`]'s step: 0 silent, then three steps of a third each.
fn volume_step(level: u8) -> u8 {
    match level {
        0 => 0,
        1..=33 => 1,
        34..=66 => 2,
        _ => 3,
    }
}

fn builtin_tap_action(id: &str) -> Option<LaunchAction> {
    match id {
        "status.notifications" => Some(LaunchAction::OpenOverlay),
        // Closing the padlock is leaving the unlocked session — a logout, not the lock screen.
        "status.lock" => Some(LaunchAction::Logout),
        _ => None,
    }
}

/// A built-in item's width. `None` for one that is not drawn (the capability not reported, or
/// `status.lock` with nothing unlocked).
fn measure_builtin(
    spec: &StatusItemSpec,
    style: IconStyle,
    painter: &egui::Painter,
    parts: &CxParts<'_>,
    texts: &mut Vec<TextCache>,
    unread: u32,
) -> Option<f32> {
    let label = label_galley(spec, painter, parts.theme);
    if spec.item.is_text() {
        let galley = text_galley(spec, painter, parts, texts)?;
        let extra = if spec.item == StatusItem::User {
            parts.theme.components.status_bar.user_dot + 4.0
        } else {
            0.0
        };
        return Some(item_width(
            spec,
            galley.size().x + extra,
            label.as_ref(),
            parts.theme,
        ));
    }
    let base = match spec.item {
        StatusItem::Wifi => parts
            .services
            .wifi
            .capabilities()
            .contains(Capabilities::WIFI)
            .then_some(style.size),
        StatusItem::Bluetooth => parts
            .services
            .bluetooth
            .capabilities()
            .contains(Capabilities::BLUETOOTH)
            .then_some(style.size),
        StatusItem::Battery => parts.services.power.battery().map(|_| style.size * 1.4),
        // The notification badge (M2): the bell icon plus, where there are unread, a number pill sticking out to its upper right.
        StatusItem::Notifications => {
            let galley = badge_galley(&spec.id, unread, painter, style.size, texts);
            let text_w = galley.map_or(0.0, |g| g.size().x);
            Some(style.size + badge_overhang(style.size, text_w))
        }
        StatusItem::Volume => {
            let audio = &parts.services.audio;
            (audio.capabilities().contains(Capabilities::VOLUME) && audio.volume().is_some())
                .then_some(style.size)
        }
        StatusItem::Ethernet => {
            let network = &parts.services.network;
            (network.capabilities().contains(Capabilities::ETHERNET)
                && network.ethernet_up().is_some())
            .then_some(style.size)
        }
        StatusItem::Brightness => {
            let display = &parts.services.display;
            (display.capabilities().contains(Capabilities::BRIGHTNESS)
                && display.brightness().is_some())
            .then_some(style.size)
        }
        StatusItem::Lock => parts.access.is_unlocked().then_some(style.size),
        _ => None,
    };
    base.map(|w| item_width(spec, w, label.as_ref(), parts.theme))
}

/// Draw one built-in item inside its own Rect. `status.notifications` is the bell icon plus the
/// unread badge (the number's galley is cached, so it is laid out only when the value changes, and
/// past 99 it reads "99+"), and a tap opens the shade through [`builtin_tap_action`]'s
/// `OpenOverlay`.
#[expect(
    clippy::too_many_lines,
    reason = "each kind of built-in item is drawn its own way"
)]
fn draw_builtin(
    ui: &egui::Ui,
    rect: Rect,
    spec: &StatusItemSpec,
    style: IconStyle,
    parts: &mut CxParts<'_>,
    texts: &mut Vec<TextCache>,
    fades: &mut StatusFades,
) {
    let theme: &Theme = parts.theme;
    let color = style.resolve_color(theme);
    let painter = ui.painter();
    // With a caption, the item's place is divided into the icon-and-value place and the caption's.
    let label = label_galley(spec, painter, theme);
    let (rect, label_at) = match &label {
        Some(g) => {
            let icon_h = if spec.item.is_text() {
                spec.text_size.unwrap_or(theme.metrics.status_text_size) * 1.2
            } else {
                style.size
            };
            let (r, at) = split_for_label(spec, rect, g, icon_h, theme);
            (r, Some(at))
        }
        None => (rect, None),
    };
    if let (Some(g), Some(at)) = (&label, label_at) {
        painter.galley(at, Arc::clone(g), color);
    }
    if spec.item.is_text() {
        let Some(galley) = text_galley(spec, painter, parts, texts) else {
            return;
        };
        let size = galley.size();
        let mut x = rect.min.x;
        if spec.item == StatusItem::User {
            // The level's colour dot ("the current subject's name plus the level's colour dot").
            let dot = theme.components.status_bar.user_dot;
            let session = parts.access.session();
            let role = parts
                .access
                .table()
                .get(session.subject.level)
                .and_then(|def| def.color)
                .unwrap_or(ColorRole::Primary);
            painter.circle_filled(
                egui::pos2(x + dot / 2.0, rect.center().y),
                dot / 2.0,
                theme.color(role),
            );
            x += dot + 4.0;
        }
        painter.galley(egui::pos2(x, rect.center().y - size.y / 2.0), galley, color);
        return;
    }
    // The muted/danger/stroke/disabled rules are gathered in one place in icons
    // (`IconStyle::param_style`) — the colours come out the same as a static icon's.
    let pstyle = style.param_style(theme);
    let icon_rect = Rect::from_center_size(rect.center(), egui::vec2(rect.width(), style.size));
    let now = parts.now;
    fades.follow(theme.motion.crossfade.duration);
    match spec.item {
        StatusItem::Wifi => {
            // A narrow accessor rather than cloning the snapshot (a heap allocation) — zero heap allocation on the render path.
            let wifi = &parts.services.wifi;
            fades.wifi.set((wifi.strength(), !wifi.enabled()), now);
            fades.wifi.paint(now, &pstyle, |(level, off), s| {
                parametric::wifi(painter, icon_rect, level, off, s);
            });
        }
        StatusItem::Bluetooth => {
            let bt = &parts.services.bluetooth;
            let state = if !bt.enabled() {
                BtIconState::Off
            } else if bt.any_connected() {
                BtIconState::Connected
            } else {
                BtIconState::On
            };
            fades.bluetooth.set(state, now);
            fades.bluetooth.paint(now, &pstyle, |state, s| {
                parametric::bluetooth(painter, icon_rect, state, s);
            });
        }
        StatusItem::Battery => {
            if let Some(b) = parts.services.power.battery() {
                fades.battery.set((b.percent, b.charging), now);
                fades.battery.paint(now, &pstyle, |(percent, charging), s| {
                    parametric::battery(painter, icon_rect, percent, charging, s);
                });
            }
        }
        StatusItem::Volume => {
            if let Some(audio) = parts.services.audio.volume() {
                fades
                    .volume
                    .set((volume_step(audio.level), audio.muted), now);
                fades.volume.paint(now, &pstyle, |(step, muted), s| {
                    parametric::volume(painter, icon_rect, step, muted, s);
                });
            }
        }
        StatusItem::Ethernet => {
            if let Some(up) = parts.services.network.ethernet_up() {
                // A wired interface with no link is drawn, dimmed: the cable is the thing to check.
                let dimmed = IconStyle {
                    enabled: up,
                    ..style
                };
                parts.icons.paint(
                    painter,
                    icon_rect,
                    &crate::icons::builtin::ETHERNET,
                    &dimmed,
                    theme,
                );
            }
        }
        StatusItem::Brightness => {
            parts.icons.paint(
                painter,
                icon_rect,
                &crate::icons::builtin::BRIGHTNESS,
                &style,
                theme,
            );
        }
        StatusItem::Lock => {
            if parts.access.is_unlocked() {
                parts.icons.paint(
                    painter,
                    icon_rect,
                    &crate::icons::builtin::UNLOCK,
                    &style,
                    theme,
                );
            }
        }
        StatusItem::Notifications => {
            let bell = Rect::from_center_size(
                egui::pos2(rect.min.x + style.size / 2.0, rect.center().y),
                egui::vec2(style.size, style.size),
            );
            parts
                .icons
                .paint(painter, bell, &crate::icons::builtin::BELL, &style, theme);
            // The badge number's galley is cached, so it is laid out only when the unread count changes
            // (allocation-free). From 100 it reads "99+" — the status bar's width does not wobble as the
            // digit count grows.
            let Some(galley) = badge_galley(&spec.id, fades.unread, painter, style.size, texts)
            else {
                return;
            };
            let (pill_w, pill_h) = badge_size(style.size, galley.size().x);
            let pill = Rect::from_min_size(
                egui::pos2(
                    bell.max.x - pill_h * BADGE_OVERLAP,
                    bell.min.y - pill_h * 0.1,
                ),
                egui::vec2(pill_w, pill_h),
            );
            painter.rect_filled(pill, pill_h / 2.0, theme.color(ColorRole::Danger));
            let size = galley.size();
            painter.galley(
                pill.center() - size / 2.0,
                galley,
                theme.color(ColorRole::OnPrimary),
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{badge_overhang, badge_size, builtin_tap_action, write_badge};
    use crate::screen::LaunchAction;

    /// The badge number: one digit, two digits, and 100 or more. The buffer is reused, so no earlier value is left behind.
    #[test]
    fn badge_text_caps_at_99_plus() {
        let mut buf = String::new();
        write_badge(&mut buf, 1);
        assert_eq!(buf, "1");
        write_badge(&mut buf, 42);
        assert_eq!(buf, "42");
        write_badge(&mut buf, 99);
        assert_eq!(buf, "99");
        write_badge(&mut buf, 100);
        assert_eq!(buf, "99+");
        write_badge(&mut buf, 12_345);
        assert_eq!(buf, "99+");
    }

    /// One digit is a circle (width = height); more digits grow the pill sideways only.
    #[test]
    fn badge_pill_grows_sideways_only() {
        let (w1, h1) = badge_size(18.0, 6.0);
        let (w3, h3) = badge_size(18.0, 20.0);
        assert!((h1 - h3).abs() < f32::EPSILON, "the height stays as it is");
        assert!(w1 < w3, "more digits makes it wider");
        assert!(w1 >= h1, "it never gets narrower than a circle");
        assert!(
            (badge_size(18.0, 1.0).0 - h1).abs() < f32::EPSILON,
            "a narrow glyph gives a circle"
        );
        assert!(badge_overhang(18.0, 0.0) == 0.0, "0 unread means no badge");
        assert!(badge_overhang(18.0, 20.0) > 0.0);
    }

    /// A tap on the notifications item opens the shade (the gate is checked again by `Shell::launch`).
    #[test]
    fn notifications_item_taps_open_the_shade() {
        assert_eq!(
            builtin_tap_action("status.notifications"),
            Some(LaunchAction::OpenOverlay)
        );
        assert_eq!(builtin_tap_action("status.clock"), None);
    }
}
