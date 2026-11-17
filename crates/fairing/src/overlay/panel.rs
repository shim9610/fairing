//! The shade panel's content (unified): the quick-settings tiles at the top (one row plus a
//! Slider expansion row), the notification list below (a `ScrollArea`), and the user / lock /
//! settings footer at the bottom. Plus the panel's bottom handle.
//!
//! Cached rendering inputs: text is drawn from the galley cache
//! ([`PanelText`]) that [`Overlay`](super::Overlay) fills at the front of the frame, and names and
//! labels are borrowed as `&str`. A `LaunchAction` or an id is cloned only on the frame a tap
//! happens.
//!
//! Interaction ownership: the panel handles only widget-level taps (tiles, notification rows, ×,
//! the footer) and slider drags. **Who owns** a shade drag, a list scroll or a notification row
//! swipe is decided by [`super::Overlay`] from the raw pointer, and the result is handed to
//! [`TileRow`] (`drag_scroll` · `swiping` · `scroll_to`). The pressed notification row
//! (`pressed_row`), the list Rect and the slider Rect come back through [`PanelOutput`].
// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of clippy's pedantic set stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod tiles_render;
use tiles_render::draw_tiles;
mod notifications;
use notifications::draw_list;
mod footer;
use footer::draw_footer;

use super::hooks::{ShadeTileCx, ShadeTileLayout, ShadeTileLayoutCx, ShadeTilePainter};
use super::tiles::{Gauge, QuickTile, TileDecl, TileKind, TileState};
use super::{FooterButton, OverlayPanel};
use crate::icons::{builtin, IconColor, IconRef, IconStyle};
use crate::notify::{Notification, NotificationCenter, NotificationId};
use crate::screen::{CxParts, LaunchAction, PaneInfo};
use crate::settings::{SettingKey, SettingValue};
use crate::theme::{ColorRole, Theme};
use crate::workspace::InstanceId;
use egui::containers::scroll_area::{DragScroll, ScrollSource};
use egui::epaint::text::FontsView;
use egui::{Color32, FontId, Galley, Rect, Sense, Vec2};
use std::sync::Arc;

/// The panel's string keys: the English wording, looked up in the active language where drawn.
pub(super) mod labels {
    /// When there are no notifications.
    pub(crate) const NO_NOTIFICATIONS: &str = "No notifications";
    /// The condensed row for a notification that fails its gate (the title and body are hidden).
    pub(crate) const HIDDEN_NOTIFICATION: &str = "1 notification";
    /// The footer's "clear all".
    pub(crate) const CLEAR_ALL: &str = "Clear all";
}

/// What the panel hands back to the shell.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelAction {
    /// A tile tap → the shell `launch`es it (re-checking the gate).
    Launch(LaunchAction),
    /// A tile was **long-pressed** — the shell raises an event, and opens whatever the tile leads to, if anything.
    TileLongPressed {
        /// The tile id.
        id: String,
    },
    /// A locked tile was tapped → an unlock request for its gate.
    TileLocked {
        /// The tile id.
        id: String,
        /// The gate the tile sits behind — what the unlock asks for.
        gate: crate::access::Gate,
    },
    /// A notification was tapped.
    NotificationTapped(NotificationId),
    /// Dismiss a notification.
    Dismiss(NotificationId),
    /// "Clear all".
    ClearAll,
}

/// One frame's panel output.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelOutput {
    /// The action.
    pub action: Option<PanelAction>,
    /// The notification list's scroll offset (for the hand-off decision).
    pub list_offset: f32,
    /// How many tile Rects were recorded.
    pub tiles_drawn: usize,
    /// The notification list's area (for the hand-off decision — a press inside it is a candidate for the list to own).
    pub list_rect: Rect,
    /// The expanded Slider row's track Rect (`Rect::NOTHING` where there is none). A drag started here is the slider's.
    pub slider_rect: Rect,
    /// The expansion state after this frame (a Slider tile tap toggles it).
    pub expanded: Option<usize>,
    /// The slider's value, 0..=1, while it is held (following 1:1).
    pub slider_value: Option<f32>,
    /// The (non-persistent) notification row pressed this frame — a candidate for a swipe dismissal.
    pub pressed_row: Option<NotificationId>,
    /// The top notification's close **hit** Rect (wider than the × drawn). `Rect::NOTHING` where there is none.
    pub close_hit: Rect,
}

/// One panel string's galley cache. [`super::Overlay`] refreshes it at the front of the
/// frame and the panel draws it borrowed through [`TileRow::texts`]. Text buffers
/// are retained, but current text is submitted to egui layout on every refresh.
#[derive(Debug, Clone)]
pub struct PanelText {
    key: u64,
    text: String,
    galley: Option<Arc<Galley>>,
    seen: u64,
}

impl PanelText {
    /// An empty entry.
    pub(super) fn new(key: TextKey) -> Self {
        Self {
            key: key.code(),
            text: String::new(),
            galley: None,
            seen: 0,
        }
    }

    /// The key.
    #[must_use]
    pub fn key(&self) -> u64 {
        self.key
    }

    /// The laid-out galley.
    #[must_use]
    pub fn galley(&self) -> Option<&Arc<Galley>> {
        self.galley.as_ref()
    }

    /// The frame it was last used on.
    pub(super) fn seen(&self) -> u64 {
        self.seen
    }

    /// Bring the content up to date. The string buffer is overwritten only when the content changes
    /// (with no allocation while the capacity holds), and the layout is redone every frame — see the
    /// comment below.
    /// `max_w` bounds the layout: past it the text is cut with an ellipsis instead of running on.
    /// `None` leaves it unbounded, which is right for text the panel has already made room for.
    pub(super) fn refresh(
        &mut self,
        text: &str,
        font: &FontId,
        fonts: &mut FontsView<'_>,
        frame: u64,
        max_w: Option<f32>,
    ) {
        self.seen = frame;
        if self.text != text {
            self.text.clear();
            self.text.push_str(text);
        }
        // **It is laid out again every frame even where the content is unchanged.** A `Galley` carries
        // font atlas UVs, so when a new glyph arrives and the atlas grows, a galley being held draws the
        // wrong characters (reproducible straight away with a CJK font loaded). epaint's `GalleyCache`
        // memoises the same input, so the layout cost itself is not paid twice.
        self.galley = if text.is_empty() {
            None
        } else if let Some(max_w) = max_w {
            // **Cut with an ellipsis rather than run on.** `layout_no_wrap` makes a galley as wide
            // as the text, and a quick tile centres it and clips — so "Bluetooth" drew as
            // "luetoot", losing a letter at *each* end, which reads as a rendering fault rather
            // than as a name too long for its tile.
            let mut job = egui::text::LayoutJob::simple_singleline(
                self.text.clone(),
                font.clone(),
                Color32::PLACEHOLDER,
            );
            job.wrap.max_width = max_w;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            job.wrap.overflow_character = Some('…');
            Some(fonts.layout_job(job))
        } else {
            Some(fonts.layout_no_wrap(self.text.clone(), font.clone(), Color32::PLACEHOLDER))
        };
    }
}

/// The gap between two quick tiles, and the panel's own side inset either side of the row —
/// `screen_inset`, which follows the finger (it was a fixed 12 du).
pub(crate) fn tile_gap(metrics: &crate::theme::Metrics) -> f32 {
    metrics.screen_inset
}

/// **One quick tile's width** for a panel this wide.
///
/// Shared rather than written twice: the labels are laid out at the front of the frame, before the
/// row is drawn, and a label laid out against a different width than the tile it lands in is how
/// "Bluetooth" came to draw as "luetoot" — wider than its tile, centred, and clipped at both ends.
#[expect(
    clippy::cast_precision_loss,
    reason = "a shade holds a handful of columns, not 2^24"
)]
pub(super) fn tile_width(panel_w: f32, columns: usize, metrics: &crate::theme::Metrics) -> f32 {
    let cols = columns.max(1) as f32;
    let gap = tile_gap(metrics);
    ((panel_w - gap * 2.0 - gap * (cols - 1.0)) / cols)
        .min(metrics.tile_size)
        .max(metrics.touch_target * 0.5)
}

/// The galley cache's key. The top 8 bits are the kind, the rest the entry (a tile index, or a notification id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKey {
    /// A tile label (the index in the tile list).
    TileLabel(usize),
    /// "No notifications".
    NoNotifications,
    /// "Clear all".
    ClearAll,
    /// "1 notification", in place of a hidden notification's content.
    Hidden,
    /// The footer's subject name.
    FooterName,
    /// A notification's title.
    Title(NotificationId),
    /// A notification's body.
    Body(NotificationId),
    /// A gauge row's name (the tile index, the row index).
    GaugeLabel(usize, usize),
    /// A gauge row's value display (the tile index, the row index).
    GaugeValue(usize, usize),
}

impl TextKey {
    const ITEM_MASK: u64 = (1 << 56) - 1;

    /// The integer key.
    #[must_use]
    pub fn code(self) -> u64 {
        let (kind, item) = match self {
            Self::TileLabel(i) => (1_u64, i as u64),
            Self::NoNotifications => (2, 0),
            Self::ClearAll => (3, 0),
            Self::Hidden => (4, 0),
            Self::FooterName => (5, 0),
            Self::Title(id) => (6, id.0),
            Self::Body(id) => (7, id.0),
            // The tile and row indices go into one integer (256 rows to a tile is generous).
            Self::GaugeLabel(tile, row) => (8, ((tile as u64) << 8) | (row as u64 & 0xff)),
            Self::GaugeValue(tile, row) => (9, ((tile as u64) << 8) | (row as u64 & 0xff)),
        };
        (kind << 56) | (item & Self::ITEM_MASK)
    }
}

/// Find a galley in the cache (linearly — the entry count is the tiles + the notifications × 2 + 4).
fn galley(texts: &[PanelText], key: TextKey) -> Option<&Arc<Galley>> {
    let code = key.code();
    texts
        .iter()
        .find(|t| t.key == code)
        .and_then(PanelText::galley)
}

/// The panel's render input (the tiles plus this frame's interaction state).
pub struct TileRow<'a> {
    /// The tiles with this frame's state and whether they are allowed.
    pub tiles: &'a [(QuickTile, TileState, bool)],
    /// How many tiles to a row.
    pub columns: usize,
    /// The galley cache ([`super::Overlay`] fills it at the front of the frame).
    pub texts: &'a [PanelText],
    /// The expanded Slider / Panel tile's index.
    pub expanded: Option<usize>,
    /// The integrator tile declarations — a [`TileKind::Panel`]'s expansion-row body is in here.
    pub panels: &'a [TileDecl],
    /// Whether a close × is drawn on a notification row (`[notify] dismiss_button`). Turned off, the
    /// only ways to dismiss are a left swipe and "clear all".
    pub dismiss_button: bool,
    /// The footer buttons to draw, left to right (`[overlay] footer`). Empty draws none of them.
    pub footer: &'a [FooterButton],
    /// The notification row being swiped, and its x offset (px).
    pub swiping: Option<(NotificationId, f32)>,
    /// This press was handled as a long press — no tile tap is raised on release.
    pub swallow_tap: bool,
    /// The notification row collapsing, and the fraction of its height left (1 = full height, 0 =
    /// gone).
    ///
    /// **Removing a row the instant it has flown out sideways jerks the rows below up** — leaving no
    /// telling what went. Having them follow up while its place shrinks to 0 makes it visible.
    pub collapsing: Option<(NotificationId, f32)>,
    /// Whether the list may drag-scroll — `false` while the shade or a row swipe owns the gesture
    /// (the A1 nested-scrolling rule). egui hit-tests against last frame's widgets, so on every
    /// other frame it has to be `true` for the next press to be caught by the list.
    pub drag_scroll: bool,
    /// Set the list's offset to this on this frame (the slop's worth of catch-up after the hand-off decision).
    pub scroll_to: Option<f32>,
    /// **Which panel this is**: the unified shade draws the tiles, the list and the
    /// footer; a split panel draws only its own half — the notifications' list and "clear all",
    /// or the controls' tiles and the subject, lock and settings.
    pub panel: OverlayPanel,
}

/// Where the tiles go: the rects drawn this frame (what the long press, the two-step stop and
/// `Overlay::tile_rect` read), the integrator's layout where there is one, and the
/// buffer it places the tiles in.
pub(super) struct TileSpots<'a> {
    pub(super) drawn: &'a mut Vec<Rect>,
    pub(super) layout: Option<&'a mut ShadeTileLayout>,
    pub(super) homes: &'a mut Vec<Rect>,
    /// Who draws the tiles, and on what.
    pub(super) brush: TileBrush<'a>,
}

/// Who draws the tiles and the colour they sit on: the integrator's tile painter where
/// there is one, and the panel's ground — the surface colour under the built-in plate, or what a
/// panel painter said. The list fades into that colour where it overflows, and a padlock's disc is
/// cut from it.
pub(super) struct TileBrush<'a> {
    pub(super) painter: Option<&'a mut ShadeTilePainter>,
    pub(super) ground: Color32,
}

/// The tile row's output.
struct TilesOut {
    action: Option<PanelAction>,
    bottom: f32,
    expanded: Option<usize>,
    slider_rect: Rect,
    slider_value: Option<f32>,
}

/// The slider expansion row's output.
struct SliderOut {
    action: Option<PanelAction>,
    value: Option<f32>,
    rect: Rect,
}

/// The notification list's output.
struct ListOut {
    action: Option<PanelAction>,
    pressed: Option<NotificationId>,
    /// The top notification's **close hit Rect** — where it is actually pressed, not the × drawn.
    /// Exposed here so that tests need not duplicate the layout arithmetic (the same
    /// convention as `tile_rects`).
    close_hit: Rect,
}

/// Draw the panel's content into `rect` (the panel's **full** height, pinned to the top of the
/// screen). It being a curtain, what is actually seen is inside `ui`'s clip Rect, and the bottom
/// handle is drawn at **the curtain's end** (the A1 render mapping). `spots.drawn` records the
/// Rects in tile-id order.
pub(super) fn show(
    ui: &mut egui::Ui,
    rect: Rect,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    center: &NotificationCenter,
    mut spots: TileSpots<'_>,
    status_row: Option<f32>,
) -> PanelOutput {
    let theme = parts.theme;
    let metrics = theme.metrics;
    // The panel's ground takes the pointer: egui's hit test stops at the layer with a widget covering
    // the pointer, so a tap on the empty space between tiles does not leak into the scrim below (closing it).
    // Under the panel's own `Ui`, not a fixed id: while the split shade crosses from one panel to the
    // other both are drawn in the same frame, each in its own `Area`.
    let _ = ui.interact(
        rect,
        ui.scope_id().with("fairing.overlay.panel.bg"),
        Sense::click(),
    );
    let style = PanelStyle::new(&metrics, theme.components.shade);
    let top = rect.min.y + status_row.unwrap_or(0.0) + style.pad;
    let footer_h = metrics.widget_height;
    // What sits between the head and the footer: the tiles, the list, or the tiles and then the list.
    let tiles = if row.panel == OverlayPanel::Notifications {
        // No tiles were laid out this frame, so none can be pressed or long-pressed.
        spots.drawn.clear();
        None
    } else {
        Some(draw_tiles(ui, rect, top, parts, row, &mut spots))
    };
    let list = match (row.panel, &tiles) {
        (OverlayPanel::ControlCenter, _) => Rect::NOTHING,
        (_, Some(t)) => Rect::from_min_max(
            egui::pos2(rect.min.x, t.bottom + style.gap * 0.5),
            egui::pos2(rect.max.x, rect.max.y - footer_h - style.gap * 2.0),
        ),
        (_, None) => Rect::from_min_max(
            egui::pos2(rect.min.x, top),
            egui::pos2(rect.max.x, rect.max.y - footer_h - style.gap * 2.0),
        ),
    };
    let mut action = tiles.as_ref().and_then(|t| t.action.clone());
    let mut list_offset = 0.0;
    let mut pressed_row = None;
    let mut close_hit = Rect::NOTHING;
    if list.height() > 8.0 {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list));
        let mut area = egui::ScrollArea::vertical()
            .id_salt("fairing.overlay.list")
            .auto_shrink([false, false])
            .scroll_source(ScrollSource {
                scroll_bar: true,
                drag: if row.drag_scroll {
                    DragScroll::Always
                } else {
                    DragScroll::Never
                },
                mouse_wheel: true,
            });
        if let Some(offset) = row.scroll_to {
            area = area.vertical_scroll_offset(offset.max(0.0));
        }
        let out = area.show(&mut child, |ui| draw_list(ui, parts, center, row));
        if let Some(picked) = out.inner.action {
            action.get_or_insert(picked);
        }
        pressed_row = out.inner.pressed;
        close_hit = out.inner.close_hit;
        list_offset = out.state.clamped_offset().y;
        // Where the list overflows, its top and bottom ends fade into the panel's colour. A card simply
        // cut at a scroll boundary reads as "broken", and a fade reads as "there is more" — iOS and One
        // UI use the same device. The fade adds two triangles.
        let overflow = out.content_size.y > list.height() + 0.5;
        if overflow {
            let fade = style.gap * 1.6;
            let surface = spots.brush.ground;
            if list_offset > 0.5 {
                edge_fade(ui.painter(), list, fade, surface, true);
            }
            if list_offset + list.height() < out.content_size.y - 0.5 {
                edge_fade(ui.painter(), list, fade, surface, false);
            }
        }
    }
    if let Some(picked) = draw_footer(ui, rect, parts, row, center, footer_h) {
        action.get_or_insert(picked);
    }
    PanelOutput {
        action,
        list_offset,
        tiles_drawn: spots.drawn.len(),
        list_rect: list,
        slider_rect: tiles.as_ref().map_or(Rect::NOTHING, |t| t.slider_rect),
        expanded: tiles.as_ref().and_then(|t| t.expanded),
        slider_value: tiles.as_ref().and_then(|t| t.slider_value),
        pressed_row,
        close_hit,
    }
}

/// The shade panel's spacing and size conventions.
///
/// Numbers like `12.0` · `10.0` · `34.0` used to be scattered across the render functions. That
/// leaves the padding behind when the metrics move to physical units and breaks the rhythm — the
/// icons grow on a 7-inch panel while the card's inner padding stays at 12 du. They are derived on
/// two axes:
///
/// - **The visual axis** — padding and corners are multiples of `corner_radius` (du). Nothing to do
///   with fingers.
/// - **The finger axis** — the toggle puck and the close button are multiples of `touch_target`
///   (physical).
#[derive(Debug, Clone, Copy)]
struct PanelStyle {
    /// The panel's edge padding.
    pad: f32,
    /// Between blocks (the tile row · the list · the footer).
    gap: f32,
    /// A card's inner side padding.
    card_pad: f32,
    /// The vertical gap between cards.
    card_gap: f32,
    /// The corner radius of cards and pills.
    radius: f32,
    /// The diameter of a quick-settings tile's round toggle.
    puck: f32,
    /// The diameter of a notification row's icon disc.
    note_icon: f32,
    /// The diameter of a notification's close button.
    close: f32,
    /// The height of a footer button.
    footer_button: f32,
}

/// One notification row's height: **a full touch target for the card, plus the gap between cards**.
///
/// `notification_row_height` alone is not enough. The gap is carved out of the row
/// (`slot.shrink2(.., card_gap * 0.5)`), and `card_gap` is a multiple of `corner_radius` — which
/// started resolving with the finger at the vocabulary's adoption step 4. At a gloved 13 mm finger the
/// gap grew to 13.7 du and took the card down to 78.2, under the 81.89 touch target, so the close
/// button could no longer reach full size however wide its hit Rect asked to be: it is clipped to
/// the card. Stating the rule here rather than hoping the token is generous enough is the fix —
/// a card you can tap is one target tall, and the gap is on top of it, not out of it.
fn notification_row_h(metrics: &crate::theme::Metrics, style: &PanelStyle) -> f32 {
    metrics
        .notification_row_height
        .max(metrics.touch_target + style.card_gap)
}

impl PanelStyle {
    /// The multipliers are all `theme.components.shade`'s — the point being that there is
    /// no bare number in this function. The two axes (visual = `corner_radius`, finger =
    /// `touch_target`) are unchanged.
    ///
    /// These values describe visual dimensions. Tile cells, notification-close hits
    /// and footer slots separately reserve touch-target geometry in their renderers;
    /// the close/footer drawing can be smaller than its hit area.
    fn new(m: &crate::theme::Metrics, s: crate::theme::ShadeMetrics) -> Self {
        let u = m.corner_radius;
        let t = m.touch_target;
        Self {
            pad: u * s.pad,
            gap: u * s.gap,
            card_pad: u * s.card_pad,
            card_gap: u * s.card_gap,
            radius: u * s.radius,
            puck: t * s.tile_puck,
            note_icon: t * s.note_icon,
            close: t * s.close,
            footer_button: t * s.footer_button,
        }
    }
}

/// The shape with only its two bottom corners rounded (the shade comes down from above).
#[must_use]
pub(super) fn bottom_corners(radius: f32) -> egui::CornerRadius {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the radius is positive and a u8 is enough for it"
    )]
    let r = radius.round().clamp(0.0, 255.0) as u8;
    egui::CornerRadius {
        nw: 0,
        ne: 0,
        sw: r,
        se: r,
    }
}

/// How many pieces the progress arc is divided into.
const STEPS: usize = 32;

/// The fade at the end of the list. With `top`, the upper end dissolves into the panel colour; otherwise the lower.
fn edge_fade(painter: &egui::Painter, list: Rect, height: f32, surface: egui::Color32, top: bool) {
    let clear = surface.gamma_multiply(0.0);
    let band = if top {
        Rect::from_min_max(list.min, egui::pos2(list.max.x, list.min.y + height))
    } else {
        Rect::from_min_max(egui::pos2(list.min.x, list.max.y - height), list.max)
    };
    let (near, far) = if top {
        (surface, clear)
    } else {
        (clear, surface)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(band.left_top(), near);
    mesh.colored_vertex(band.right_top(), near);
    mesh.colored_vertex(band.right_bottom(), far);
    mesh.colored_vertex(band.left_bottom(), far);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

/// [`draw_tile_cell`]'s two pieces of state.
#[derive(Clone, Copy)]
struct CellPaint {
    /// It is held down right now.
    pressed: bool,
    /// The label's opacity (1 = as it is). It dims while the tile descends into the Slider row.
    fade: f32,
}

/// What drawing a [`TileKind::Gauges`] row needs.
#[derive(Clone, Copy)]
struct GaugeRows<'a> {
    /// The whole row, the icon's place included.
    rect: Rect,
    /// The tile index (the animation and galley-cache id).
    tile: usize,
    /// The row declarations.
    rows: &'a [Gauge],
    /// The name column's width (du). At 0 the names are not drawn.
    label_width: f32,
    /// The galley cache.
    texts: &'a [PanelText],
}

/// How many tiles can be in motion on one frame. A swap of places is two (one leaving, one
/// arriving), and counting the residual tween just after it, it does not exceed three.
const MOVING_MAX: usize = 4;
/// Anything that has moved less than this counts as being in place.
const MOVING_EPS: f32 = 0.001;

/// What [`draw_slider_row`] takes — hand it seven arguments and the order goes wrong.
#[derive(Clone, Copy)]
struct SliderSlot<'a> {
    /// The whole row, the icon's place plus the track. **The icon is filled by the tile coming down**
    /// — it is not drawn separately here (that would show both the tile and the icon).
    rect: Rect,
    /// Its place in the tile list (the animation id).
    index: usize,
    /// The config key to write the value to.
    key: &'a SettingKey,
    /// The current value.
    state: TileState,
}

/// Between the icon and the track in a Slider row (× `row_height`).
const SLIDER_ICON_GAP: f32 = 0.22;

/// How solid the swipe hint's ground is. Enough for the icon to read while what is under the card stays like a background.
const HINT_GROUND: f32 = 0.18;

/// One notification card's output.
struct CardOut {
    /// The close was tapped.
    closed: bool,
    /// The close's **hit** Rect (wider than the × drawn). `Rect::NOTHING` where there is no close.
    close_hit: Rect,
}

#[cfg(test)]
mod tests {
    use super::tiles_render::tile_is_filled;
    use super::{QuickTile, TileKind, TileState};
    use crate::icons::builtin;
    use crate::screen::LaunchAction;
    use crate::settings::SettingKey;

    /// A tile as the shade holds it, with or without a `lit_by` key.
    fn quick(kind: TileKind, lit_by: Option<&'static str>) -> QuickTile {
        QuickTile {
            id: "tile.t".to_owned(),
            icon: builtin::SETTINGS,
            label: "t".to_owned(),
            kind,
            gate: "tile.t".into(),
            enabled: true,
            long_press: None,
            lit_by: lit_by.map(SettingKey::from),
        }
    }

    /// The four kinds with no on/off of their own.
    fn stateless() -> [TileKind; 4] {
        [
            TileKind::Action(LaunchAction::Lock),
            TileKind::Status,
            TileKind::Panel { height: 120.0 },
            TileKind::Gauges {
                rows: Vec::new(),
                label_width: 96.0,
            },
        ]
    }

    /// **Only a toggle that is on wears the accent.** A slider's value is never `Off`, so the old
    /// `On | Value(_)` rule left Brightness and Volume permanently filled and the shade came out as
    /// five accent pucks out of six. Accent that is always on carries no information.
    #[test]
    fn only_a_toggle_that_is_on_is_filled() {
        let toggle = quick(TileKind::Toggle(SettingKey::from("wifi.enabled")), None);
        assert!(tile_is_filled(&toggle, TileState::On));
        assert!(!tile_is_filled(&toggle, TileState::Off));
        assert!(!tile_is_filled(&toggle, TileState::Unavailable));

        let slider = quick(
            TileKind::Slider(SettingKey::from("display.brightness")),
            None,
        );
        for v in [0.0, 0.4, 1.0] {
            assert!(
                !tile_is_filled(&slider, TileState::Value(v)),
                "a slider at {v} fills the puck — its readout is the arc"
            );
        }
    }

    /// A tile with no on/off is a button, and a button that looks latched is a lie.
    #[test]
    fn the_stateless_kinds_never_fill() {
        for kind in stateless() {
            let tile = quick(kind, None);
            for state in [
                TileState::Off,
                TileState::On,
                TileState::Value(1.0),
                TileState::Unavailable,
            ] {
                assert!(
                    !tile_is_filled(&tile, state),
                    "{:?} fills the puck at {state:?}",
                    tile.kind
                );
            }
        }
    }

    /// **A tile lit from a setting wears the accent while it is lit**. The kind
    /// has no on/off, but the device does — a cable is in — and `lit_by` promises the tile is drawn
    /// lit. With the toggle-only rule `On` and `Off` drew the same unfilled puck.
    #[test]
    fn a_tile_lit_from_a_setting_is_filled_while_it_is_lit() {
        for kind in stateless() {
            let tile = quick(kind, Some("app.net.wired.on"));
            assert!(
                tile_is_filled(&tile, TileState::On),
                "{:?} lit from a setting stays unfilled",
                tile.kind
            );
            for state in [TileState::Off, TileState::Unavailable] {
                assert!(
                    !tile_is_filled(&tile, state),
                    "{:?} fills the puck at {state:?}",
                    tile.kind
                );
            }
        }
        // `lit_by` on a slider is ignored — its readout stays the arc.
        let slider = quick(
            TileKind::Slider(SettingKey::from("display.brightness")),
            Some("app.x"),
        );
        assert!(!tile_is_filled(&slider, TileState::Value(1.0)));
    }
}
