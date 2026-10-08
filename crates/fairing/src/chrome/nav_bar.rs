//! The navigation bar. Two styles: `Buttons` — back ◀ · home ● · recents ▮ (plus
//! integrator items) — and `Gesture`, a band with the home indicator, where the bottom edge's
//! swipes are home, recents and the next task (the gestures are the shell's).
//!
//! An item keeps the minimum touch target (48 px) and its press feedback follows A7 —
//! it answers on the first frame with the `pressed` tint, and the scale shrinks `1 →
//! press_scale` and comes back on release. The scale value lives in the shell-owned
//! [`crate::motion::AnimationStore`], so the repaint policy (`is_animating`) applies to it
//! unchanged.
//!
//! Integrator extension comes at three levels:
//! - One [`nav_item(id, |ui, cx| ..)`](nav_item) declaration = one item. That closure is called
//!   in the `NavItem::Custom(id)` slot (the same structure as
//!   [`status_item`](crate::status_item)).
//! - [`crate::shell::ShellBuilder::nav_bar_layout`] places the items (rung 4 of the override ladder):
//!   the cells are yours, and the drawing, the gates, the presses and
//!   [`NavBar::item_rect`] stay the shell's.
//! - [`crate::shell::ShellBuilder::nav_bar_painter`] replaces the bar whole. [`NavBar::ui`] is
//!   then never called and the shell keeps only the Rects and the gates.

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of clippy's pedantic set stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::bar::{BarCx, BarItemRow, BarKind, BarPainter, RowSink};
use crate::access::{Access, Gate};
use crate::config::NavBarConfig;
use crate::icons::{builtin, IconRef, IconStyle};
use crate::screen::{Cx, CxParts, PaneInfo};
use crate::theme::{ColorRole, Theme};
use crate::workspace::InstanceId;
use egui::{Rect, Sense};

/// A nav item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavItem {
    /// Back.
    Back,
    /// Home.
    Home,
    /// Recents (M5).
    Recents,
    /// Split (M5).
    Split,
    /// A [`nav_item(id, ..)`](nav_item) declaration (M2a).
    Custom(String),
}

impl NavItem {
    /// From a config string.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        match text {
            "back" => Self::Back,
            "home" => Self::Home,
            "recents" => Self::Recents,
            "split" => Self::Split,
            other => Self::Custom(other.to_owned()),
        }
    }

    /// The config string (the inverse of `NavItem::parse`). An integrator item keeps its declaration id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Back => "back",
            Self::Home => "home",
            Self::Recents => "recents",
            Self::Split => "split",
            Self::Custom(id) => id,
        }
    }

    /// The built-in icon and the font fallback glyph.
    fn art(&self) -> (Option<&'static IconRef>, &'static str) {
        match self {
            Self::Back => (Some(&builtin::BACK), "◀"),
            Self::Home => (Some(&builtin::HOME), "●"),
            Self::Recents => (Some(&builtin::RECENTS), "▮"),
            Self::Split => (Some(&builtin::SPLIT), "◫"),
            // An integrator item draws itself (its declaration's closure).
            Self::Custom(_) => (None, "•"),
        }
    }
}

/// The style (`NavStyle`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavStyle {
    /// Buttons.
    Buttons {
        /// The items.
        items: Vec<NavItem>,
    },
    /// Gestures: up from the bottom edge is home, up and hold is recents, along the
    /// home indicator is the previous or next task, and back is a `back_edges` swipe.
    Gesture {
        /// Show the home indicator.
        indicator: bool,
    },
}

/// What happened in the nav bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavAction {
    /// Back.
    Back,
    /// Home.
    Home,
    /// Recents.
    Recents,
    /// Split.
    Split,
    /// An integrator item.
    Custom(String),
}

/// The closure that draws a nav item.
pub(super) type NavItemUi = Box<dyn FnMut(&mut egui::Ui, &mut Cx<'_>)>;

/// What a nav bar layout places the items with (rung 4 of the override ladder).
///
/// The layout gets the cells **already laid out the built-in way** — even widths between one and
/// [`nav_item_max_span`](crate::theme::Metrics::nav_item_max_span) touch targets, centred — and
/// changes the ones it wants. So a layout that only pins back to the left edge is three lines,
/// and everything it does not touch stays where the shell put it.
///
/// ```no_run
/// # fn main() -> fairing::Result<()> {
/// # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
/// let shell = fairing::Shell::builder(config)
///     .nav_bar_layout(|bar: &fairing::NavLayoutCx<'_>, cells: &mut [egui::Rect]| {
///         // Back hard against the left edge, recents against the right; home stays centred.
///         if let Some(cell) = bar.index_of("back").and_then(|i| cells.get_mut(i)) {
///             *cell = cell.translate(egui::vec2(bar.rect.left() - cell.left(), 0.0));
///         }
///         if let Some(cell) = bar.index_of("recents").and_then(|i| cells.get_mut(i)) {
///             *cell = cell.translate(egui::vec2(bar.rect.right() - cell.right(), 0.0));
///         }
///     })
///     .build(&ctx)?;
/// # let _ = shell;
/// # Ok(())
/// # }
/// ```
///
/// A cell the layout empties (`Rect::NOTHING`, or any rect with no area) leaves its item out:
/// not drawn, not pressed, and [`NavBar::item_rect`] says `None`. A cell reaching outside the bar
/// is cut by the bar.
pub struct NavLayoutCx<'a> {
    /// The bar the items go in.
    pub rect: Rect,
    /// The theme — the touch target, the icon size, the type.
    pub theme: &'a Theme,
    items: &'a [NavItem],
    live: &'a [bool],
}

impl NavLayoutCx<'_> {
    /// How many items there are — one cell each, in `[nav_bar] items` order.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether there are none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Item `index`'s id: `"back"`, `"home"`, `"recents"`, `"split"`, or a
    /// [`nav_item`] declaration's id.
    #[must_use]
    pub fn id(&self, index: usize) -> Option<&str> {
        self.items.get(index).map(NavItem::id)
    }

    /// Where the item `id` is in the list.
    #[must_use]
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.items.iter().position(|item| item.id() == id)
    }

    /// Whether item `index` will be drawn live — enabled and past its gate. Back at home is not
    /// live but still drawn, dimmed; a [`nav_item`] that is off or fails its gate is not drawn at
    /// all, whatever its cell.
    #[must_use]
    pub fn is_live(&self, index: usize) -> bool {
        self.live.get(index).copied().unwrap_or(false)
    }
}

impl std::fmt::Debug for NavLayoutCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavLayoutCx")
            .field("rect", &self.rect)
            .field("items", &self.items)
            .field("live", &self.live)
            .finish_non_exhaustive()
    }
}

/// The callback that places the nav bar's items. Given through
/// [`crate::shell::ShellBuilder::nav_bar_layout`].
pub type NavLayout = Box<dyn FnMut(&NavLayoutCx<'_>, &mut [Rect])>;

/// An integrator nav-item declaration (`nav_item(id, |ui, cx| ..)`, guide 03 §2.5).
///
/// **The same structure** as [`status_item`](crate::status_item) on the top bar: it goes in with
/// `shell.add(decl)` and comes out with `shell.remove(id)`. Its place is that id's slot in the
/// `[nav_bar] items` list, and a declaration not in the list is not drawn (in a nav bar the number
/// of slots is the layout, so nothing is appended at the end).
pub struct NavItemDecl {
    pub(crate) id: String,
    pub(crate) ui: NavItemUi,
    pub(crate) gate: Option<Gate>,
    pub(crate) enabled: bool,
}

impl std::fmt::Debug for NavItemDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NavItemDecl")
            .field("id", &self.id)
            .field("enabled", &self.enabled)
            .finish_non_exhaustive()
    }
}

/// A nav-item declaration ("the other declarations have the same shape").
///
/// ```no_run
/// # use fairing::{nav_item, Cx};
/// # let mut shell: fairing::Shell = unimplemented!();
/// shell.add(nav_item("kbd", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
///     if ui.button("⌨").clicked() {
///         cx.launch(fairing::LaunchAction::open("settings.keyboard"));
///     }
/// }));
/// ```
pub fn nav_item(
    id: impl Into<String>,
    ui: impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static,
) -> NavItemDecl {
    let id = id.into();
    NavItemDecl {
        gate: Some(Gate::from(&id)),
        id,
        ui: Box::new(ui),
        enabled: true,
    }
}

impl NavItemDecl {
    /// The gate name (the id by default).
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }

    /// On or off. An item that is off is not drawn and takes no taps (its slot stays).
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

    /// The gate name.
    #[must_use]
    pub fn gate_name(&self) -> Gate {
        self.gate.clone().unwrap_or_else(|| Gate::from(&self.id))
    }
}

/// The nav bar.
#[derive(Debug, Clone, PartialEq)]
pub struct NavBar {
    /// The style.
    pub style: NavStyle,
    /// The height.
    pub height: f32,
    /// The background role.
    pub background: ColorRole,
    /// On or off (`[nav_bar] enabled`). With `false` the bar is not drawn and the content grows.
    pub enabled: bool,
    /// Whether the back button has anything to do ("ignored on the desktop"). With `false`
    /// it is drawn dimmed and a tap produces no [`NavAction::Back`]. The shell refreshes it every
    /// frame from `!workspace.is_home()` — without that refresh it stays enabled (the old behaviour).
    pub back_enabled: bool,
    /// The item Rects drawn last frame (in `items` order). Exposed as [`NavBar::item_rect`] so
    /// that tests need not duplicate the layout arithmetic.
    rects: Vec<Rect>,
    /// The glyph fallback galley cache (in `items` order).
    glyphs: Vec<GlyphCache>,
    /// This frame's cells, laid out the built-in way and then by the integrator's layout.
    /// A buffer whose values are overwritten each frame.
    cells: Vec<Rect>,
    /// Whether each item is live this frame — what [`NavLayoutCx::is_live`] answers.
    live: Vec<bool>,
}

/// The font fallback galley cache for items with no built-in icon (`split` · `Custom`).
/// `egui::Painter::text` **allocates a `String` on every call** (0.36.1 `painter.rs`:
/// `layout_no_wrap(text.to_string())`), so even an idle frame allocates once per item — while the
/// size and the colour are unchanged, the previous galley is reused (the same pattern as
/// the status bar's `TextCache`).
#[derive(Debug, Clone, Default)]
struct GlyphCache {
    /// (the text size ×4 rounded, the colour's RGBA).
    key: (u32, u32),
}

impl PartialEq for GlyphCache {
    fn eq(&self, other: &Self) -> bool {
        // The galleys are only a cache, so they are left out of the equality.
        self.key == other.key
    }
}

impl NavBar {
    /// From the config.
    #[must_use]
    pub fn from_config(cfg: &NavBarConfig) -> Self {
        if cfg.style != "gesture" && cfg.style != "buttons" {
            log::warn!(
                "[nav_bar] style = \"{}\" is neither buttons nor gesture - using buttons",
                cfg.style
            );
        }
        let style = if cfg.style == "gesture" {
            // There is no back button in this style: back is a `back_edges` swipe, and a config
            // with none never gets this far — `ShellConfig::validate` stops it.
            NavStyle::Gesture { indicator: true }
        } else {
            NavStyle::Buttons {
                items: cfg.items.iter().map(|s| NavItem::parse(s)).collect(),
            }
        };
        Self {
            style,
            // `build` sets the resolved height straight after; this is only the start.
            height: cfg
                .height
                .unwrap_or(crate::theme::Metrics::default().nav_bar_height),
            background: ColorRole::Surface,
            enabled: cfg.enabled,
            back_enabled: true,
            rects: Vec::new(),
            glyphs: Vec::new(),
            cells: Vec::new(),
            live: Vec::new(),
        }
    }

    /// The Rect of an item drawn last frame (the first, where several match). `None` if it was not drawn.
    #[must_use]
    pub fn item_rect(&self, item: &NavItem) -> Option<Rect> {
        let NavStyle::Buttons { items } = &self.style else {
            return None;
        };
        let index = items.iter().position(|i| i == item)?;
        self.rects.get(index).copied().filter(Rect::is_positive)
    }

    /// The same question by **id string**.
    ///
    /// "Give me last frame's Rect" is one question, and the three took different arguments — only
    /// the nav bar took a `&NavItem`, while
    /// [`StatusBar::item_rect`](crate::chrome::StatusBar::item_rect) and
    /// [`DesktopView::icon_rect`](crate::desktop::DesktopView::icon_rect) take a `&str`. There is
    /// no reason for tests and integrators to memorise different arguments for the same thing.
    #[must_use]
    pub fn item_rect_by_id(&self, id: &str) -> Option<Rect> {
        self.item_rect(&NavItem::parse(id))
    }

    /// Whether an item is enabled. Only Back is ever dim — it follows `back_enabled`;
    /// `Recents` and `Split` are always live.
    #[must_use]
    pub fn item_enabled(&self, item: &NavItem) -> bool {
        item_enabled(item, self.back_enabled)
    }

    /// Called by the shell instead when the bar is not drawn this frame (chrome policy `Hide`) — it
    /// clears the record so [`NavBar::item_rect`] does not hand back a stale Rect from last frame.
    /// No allocation.
    pub(crate) fn mark_hidden(&mut self) {
        self.rects.fill(Rect::NOTHING);
    }

    /// Draw. `ui` is inside the bottom panel. `custom` is the registry's [`nav_item`] declarations,
    /// and `layout` the integrator's placement of the items, if there is one.
    ///
    /// The item slots (their widths and order) are decided by `[nav_bar] items` — and by `layout`
    /// where it moves them — and where a slot's id has an integrator declaration, **that closure
    /// draws the item** (in place of the built-in icon). There is no per-frame allocation beyond the
    /// one child `Ui` handed to the closure — the same cost as an integrator item on the top bar.
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        custom: &mut [NavItemDecl],
        layout: Option<&mut NavLayout>,
    ) -> Option<NavAction> {
        let rect = ui.max_rect();
        ui.painter()
            .rect_filled(rect, 0.0, parts.theme.color(self.background));
        let back_enabled = self.back_enabled;
        let NavStyle::Buttons { items } = &self.style else {
            // Gestures: no items and no item Rects — the home indicator, where it is wanted.
            self.rects.clear();
            if matches!(self.style, NavStyle::Gesture { indicator: true }) {
                paint_indicator(ui.painter(), rect, parts.theme);
            }
            return None;
        };
        self.rects.resize(items.len(), Rect::NOTHING);
        self.glyphs.resize(items.len(), GlyphCache::default());
        self.live.clear();
        for item in items {
            let declared = match item {
                NavItem::Custom(id) => custom.iter().find(|d| d.id == *id),
                _ => None,
            };
            self.live.push(
                item_enabled(item, back_enabled)
                    && declared.is_none_or(|d| d.enabled && allowed(parts, d.gate.as_ref(), &d.id)),
            );
        }
        even_cells(rect, items.len(), &parts.theme.metrics, &mut self.cells);
        if let Some(layout) = layout {
            let bar = NavLayoutCx {
                rect,
                theme: parts.theme,
                items,
                live: &self.live,
            };
            layout(&bar, &mut self.cells);
        }
        let raw = raw_press(ui);
        let mut action = None;
        for (index, item) in items.iter().enumerate() {
            // A cell the layout emptied leaves its item out.
            let cell = self
                .cells
                .get(index)
                .copied()
                .filter(Rect::is_positive)
                .map(|cell| cell.intersect(rect))
                .filter(Rect::is_positive);
            let Some(cell) = cell else {
                if let Some(slot) = self.rects.get_mut(index) {
                    *slot = Rect::NOTHING;
                }
                continue;
            };
            let declared = match item {
                NavItem::Custom(id) => custom.iter().position(|d| d.id == *id),
                _ => None,
            };
            let live = self.live.get(index).copied().unwrap_or(false);
            // Where an integrator declaration drops out through its gate or `enabled`, its slot is left
            // empty and the Rect record cleared (so `item_rect` does not hand back a place that was not drawn).
            let drawn = declared.is_none() || live;
            if let Some(slot) = self.rects.get_mut(index) {
                *slot = if drawn { cell } else { Rect::NOTHING };
            }
            if let Some(custom_index) = declared {
                if live {
                    if let Some(hit) = draw_custom(ui, cell, index, custom_index, custom, parts) {
                        action = Some(hit);
                    }
                }
                continue;
            }
            let down = live
                && raw.is_some_and(|(origin, now)| cell.contains(origin) && cell.contains(now));
            let Some(glyph) = self.glyphs.get_mut(index) else {
                continue;
            };
            if let Some(hit) = draw_item(ui, cell, item, index, live, down, glyph, parts) {
                action = Some(hit);
            }
        }
        action
    }

    /// Hand the whole bar to an integrator painter. The shell keeps only the Rect, the
    /// gate decisions and the item list, and the built-in rendering ([`NavBar::ui`]) is never
    /// called — the background is the painter's too. Item taps are taken by the painter's own
    /// widgets and asked for through `cx.shell`.
    pub(crate) fn ui_with(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        rows: &[BarItemRow],
        painter: &mut BarPainter,
    ) {
        // No built-in item was drawn, so last frame's Rects are not left behind.
        self.mark_hidden();
        let rect = ui.max_rect();
        let pane = PaneInfo {
            rect,
            outer: rect,
            is_split: false,
            is_focused: false,
            inset_bottom: 0.0,
            instance: InstanceId::NONE,
        };
        let mut bar = BarCx::new(
            BarKind::Nav,
            rect,
            self.back_enabled,
            rows,
            parts.cx(pane, None),
        );
        painter(ui, &mut bar);
    }

    /// Fill a buffer with the item list to hand to the painter (gate decisions included). No allocation.
    pub(crate) fn collect_items(
        &self,
        custom: &[NavItemDecl],
        access: &Access,
        rows: &mut Vec<BarItemRow>,
    ) {
        let mut sink = RowSink::new(rows);
        let NavStyle::Buttons { items } = &self.style else {
            return;
        };
        for item in items {
            let declared = match item {
                NavItem::Custom(id) => custom.iter().find(|d| d.id == *id),
                _ => None,
            };
            let enabled =
                item_enabled(item, self.back_enabled) && declared.is_none_or(|d| d.enabled);
            // The shell's two controls carry its gates — a painter can draw the padlock.
            let allowed = match item {
                NavItem::Recents => access.allows(&crate::shell::RECENTS_GATE),
                NavItem::Split => access.allows(&crate::shell::SPLIT_GATE),
                _ => declared.is_none_or(|d| match d.gate.as_ref() {
                    Some(gate) => access.allows(gate),
                    None => access.allows_name(&d.id),
                }),
            };
            sink.push(item.id(), None, enabled, allowed);
        }
    }
}

/// The built-in cells: `count` even widths, each between one touch target (48 px)
/// and `nav_item_max_span` of them, centred in `bar`. Written over `cells` (no allocation once the
/// buffer has grown).
fn even_cells(bar: Rect, count: usize, metrics: &crate::theme::Metrics, cells: &mut Vec<Rect>) {
    cells.clear();
    let width = (bar.width() / count.max(1) as f32).clamp(
        metrics.touch_target.min(bar.width()),
        metrics.touch_target * metrics.nav_item_max_span,
    );
    let start_x = bar.center().x - width * count as f32 / 2.0;
    cells.extend((0..count).map(|index| {
        Rect::from_min_size(
            egui::pos2(start_x + width * index as f32, bar.min.y),
            egui::vec2(width, bar.height()),
        )
    }));
}

/// The home indicator: a pill in the middle of the band, `metrics.nav_indicator_length`
/// long — never more than half the band — and `metrics.nav_indicator_thickness` thick.
fn paint_indicator(painter: &egui::Painter, band: Rect, theme: &crate::theme::Theme) {
    let m = &theme.metrics;
    let length = m.nav_indicator_length.min(band.width() * 0.5).max(0.0);
    let thickness = m.nav_indicator_thickness.min(band.height()).max(0.0);
    let pill = Rect::from_center_size(band.center(), egui::vec2(length, thickness));
    painter.rect_filled(pill, thickness / 2.0, theme.color(ColorRole::OnSurface));
}

/// Whether an item is live. **Only `Back` is ever dim**, and only because "is there a stack
/// behind you" is a fact the shell owns.
///
/// `Recents` and `Split` used to be dim as well, while the crate had no overview or split panes of
/// its own. That was the shell deciding a control could not work when the
/// **device** might have somewhere to send it, so the items went live with the tap leaving as
/// [`ShellEvent::OverviewRequested`](crate::ShellEvent::OverviewRequested) /
/// [`SplitRequested`](crate::ShellEvent::SplitRequested). Since M5 the shell acts on them
/// as well, under `[workspace]` and the `nav.recents` / `workspace.split` gates.
fn item_enabled(item: &NavItem, back_enabled: bool) -> bool {
    match item {
        NavItem::Back => back_enabled,
        NavItem::Home | NavItem::Recents | NavItem::Split | NavItem::Custom(_) => true,
    }
}

/// The gate decision. The `None` fallback is [`Access::allows_name`], so no `Gate` is built each frame.
fn allowed(parts: &CxParts<'_>, gate: Option<&Gate>, id: &str) -> bool {
    match gate {
        Some(gate) => parts.access.allows(gate),
        None => parts.access.allows_name(id),
    }
}

/// One integrator item (a `nav_item` declaration). The shell takes only the Rect and the tap and
/// the closure does the drawing — the same structure as an integrator item on the top bar, so the
/// press tint and scale are the closure's too (`cx.animate`).
fn draw_custom(
    ui: &mut egui::Ui,
    cell: Rect,
    index: usize,
    custom_index: usize,
    custom: &mut [NavItemDecl],
    parts: &mut CxParts<'_>,
) -> Option<NavAction> {
    // It has to be registered first for the widget the closure makes to win the hit test (the same order as the top bar).
    let response = ui.interact(cell, egui::Id::new(("fairing.nav", index)), Sense::click());
    let decl = custom.get_mut(custom_index)?;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell).layout(
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
    ));
    let pane = PaneInfo {
        rect: cell,
        outer: cell,
        is_split: false,
        is_focused: false,
        inset_bottom: 0.0,
        instance: InstanceId::NONE,
    };
    let mut cx = parts.cx(pane, None);
    (decl.ui)(&mut child, &mut cx);
    response
        .clicked()
        .then(|| NavAction::Custom(decl.id.clone()))
}

/// This frame's raw press: (where it went down, where it is now). The nav bar is a panel (the
/// background layer), so it is valid only where no `Area` is registered over that spot — under M2's
/// shade, the press tint does not come up.
fn raw_press(ui: &egui::Ui) -> Option<(egui::Pos2, egui::Pos2)> {
    let (down, origin, current) = ui.input(|i| {
        (
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.interact_pos(),
        )
    });
    if !down {
        return None;
    }
    let (origin, current) = (origin?, current?);
    ui.ctx()
        .layer_id_at(origin)
        .is_none()
        .then_some((origin, current))
}

/// One item. It does the press feedback (A7) and the hit test together. `raw_down` is the raw
/// input decision that makes up for an egui `Response` being one pass late.
#[allow(clippy::too_many_arguments)] // These are the values drawing one item needs (grouping them buys nothing).
fn draw_item(
    ui: &mut egui::Ui,
    cell: Rect,
    item: &NavItem,
    index: usize,
    live: bool,
    raw_down: bool,
    cache: &mut GlyphCache,
    parts: &mut CxParts<'_>,
) -> Option<NavAction> {
    let theme = parts.theme;
    let tokens = theme.motion;
    let response = ui.interact(
        cell,
        egui::Id::new(("fairing.nav", index)),
        if live { Sense::click() } else { Sense::hover() },
    );
    let down = raw_down || (live && response.is_pointer_button_down_on());
    // It answers on the first frame with the tint.
    if down {
        ui.painter().rect_filled(
            cell.shrink(6.0),
            theme.metrics.corner_radius,
            theme.color(ColorRole::Pressed),
        );
    }
    let target = if down { tokens.press_scale } else { 1.0 };
    let tween = if down {
        tokens.press
    } else {
        tokens.press_release
    };
    let scale = parts.animations.animate(
        egui::Id::new(("fairing.nav.press", index)),
        target,
        tween,
        parts.frame,
    );

    let size = theme.metrics.nav_icon_size * scale;
    let icon_rect = Rect::from_center_size(cell.center(), egui::vec2(size, size));
    let (icon, glyph) = item.art();
    let style = IconStyle::sized(size).enabled(live);
    let painter = ui.painter().clone();
    let drawn = icon.is_some_and(|icon| {
        parts
            .icons
            .paint(&painter, icon_rect, icon, &style, parts.theme)
    });
    if !drawn {
        let color = style.resolve_color(parts.theme);
        let font = egui::FontId::proportional(18.0 * scale);
        let key = (
            (font.size * 4.0).round() as u32,
            u32::from_le_bytes(color.to_array()),
        );
        cache.key = key;
        // The galleys are not held across frames — a growing atlas throws the UVs out.
        let galley = painter.layout_no_wrap(glyph.to_owned(), font, color);
        let pos = egui::Align2::CENTER_CENTER
            .anchor_size(cell.center(), galley.size())
            .min;
        painter.galley(pos, galley, color);
    }
    if live && response.clicked() {
        return Some(match item {
            NavItem::Back => NavAction::Back,
            NavItem::Home => NavAction::Home,
            NavItem::Recents => NavAction::Recents,
            NavItem::Split => NavAction::Split,
            NavItem::Custom(id) => NavAction::Custom(id.clone()),
        });
    }
    None
}
