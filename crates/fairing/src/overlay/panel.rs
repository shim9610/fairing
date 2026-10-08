//! The shade panel's content (unified): the quick-settings tiles at the top (one row plus a
//! Slider expansion row), the notification list below (a `ScrollArea`), and the user / lock /
//! settings footer at the bottom. Plus the panel's bottom handle.
//!
//! Zero heap allocation per frame: text is drawn from the galley cache
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
    /// A locked tile was tapped → an unlock request.
    TileLocked {
        /// The tile id.
        id: String,
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
/// frame and the panel draws it borrowed through [`TileRow::texts`]. Only entries whose content
/// changed are laid out again.
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

/// The gap between two quick tiles, and the panel's own side inset either side of the row.
pub(crate) const TILE_GAP: f32 = 12.0;

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
    ((panel_w - TILE_GAP * 2.0 - TILE_GAP * (cols - 1.0)) / cols)
        .min(metrics.tile_size)
        .max(24.0)
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
        ui.id().with("fairing.overlay.panel.bg"),
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
        list_offset = out.state.offset.y;
        // Where the list overflows, its top and bottom ends fade into the panel's colour. A card simply
        // cut at a scroll boundary reads as "broken", and a fade reads as "there is more" — iOS and One
        // UI use the same device. Two triangles, so it costs nothing.
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
/// started resolving with the finger at the vocabulary's adoption step 4. At the gloved default the
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
    /// **Not all four on the finger axis are "the size drawn".** Only the tile puck (`puck`) has a
    /// hit area of the whole cell and so is bigger than its drawing; the close button and the footer
    /// buttons **hit exactly as they draw** — which comes to only 0.72 and 0.82 of `touch_target`.
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

/// The progress arc around the puck. `progress` is 0..=1 and grows clockwise from twelve o'clock.
///
/// Drawing the value **inside** the puck overlaps the icon, and drawing it as a thin band along the
/// bottom of the cell collides with the label. The circumference is empty space and fights with
/// neither.
fn value_ring(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    progress: f32,
    stroke: egui::Stroke,
) {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return;
    }
    let last = (STEPS as f32 * progress).ceil().max(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "STEPS is 32"
    )]
    let count = last as usize;
    let mut points = Vec::with_capacity(count + 1);
    for i in 0..=count {
        #[expect(clippy::cast_precision_loss, reason = "i <= 32")]
        let t = (i as f32 / STEPS as f32).min(progress);
        let angle = std::f32::consts::TAU * t - std::f32::consts::FRAC_PI_2;
        points.push(egui::pos2(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }
    painter.add(egui::Shape::Path(egui::epaint::PathShape {
        points,
        closed: false,
        fill: egui::Color32::TRANSPARENT,
        stroke: stroke.into(),
    }));
}

/// [`draw_tile_cell`]'s two pieces of state.
#[derive(Clone, Copy)]
struct CellPaint {
    /// It is held down right now.
    pressed: bool,
    /// The label's opacity (1 = as it is). It dims while the tile descends into the Slider row.
    fade: f32,
}

/// One tile's tap → an action (or a Slider expansion toggle). A `Toggle` tile sends **the opposite
/// of the state shown** as `Set(key, Bool)` — for a key the backend owns the truth of (`wifi.enabled`
/// and the like) the settings memory may hold no value at all, so a `LaunchAction::Toggle` (which
/// goes by that memory) would turn on something already on. The gate is the same for both, being the
/// key name.
fn tile_tap(
    quick: &QuickTile,
    state: TileState,
    allowed: bool,
    live: bool,
    index: usize,
    expanded: &mut Option<usize>,
) -> Option<PanelAction> {
    if !allowed {
        return Some(PanelAction::TileLocked {
            id: quick.id.clone(),
        });
    }
    if !live {
        return None;
    }
    match &quick.kind {
        TileKind::Toggle(key) => Some(PanelAction::Launch(LaunchAction::Set(
            key.clone(),
            SettingValue::Bool(state != TileState::On),
        ))),
        // The kinds that open an expansion row. They share the same place, so one opening closes another.
        TileKind::Slider(_) | TileKind::Panel { .. } | TileKind::Gauges { .. } => {
            *expanded = if *expanded == Some(index) {
                None
            } else {
                Some(index)
            };
            None
        }
        TileKind::Action(launch) => Some(PanelAction::Launch(launch.clone())),
        TileKind::Status => None,
    }
}

/// **A slider tile's value, as an arc around its puck.**
///
/// The puck no longer fills with the accent, so this ring is the only thing that says 40 % rather
/// than 90 %. It is drawn at `stroke_mark` — the weight of a checkbox's tick — and stands off the
/// puck by `focus_gap`, clear of both the icon inside and the label below.
///
/// **The track is the same path as the value.** It used to be a `circle_stroke`, which epaint
/// tessellates with the stroke *outside* the radius (`PathStroke::outside`, `tessellator.rs`),
/// while `value_ring` is a path and is stroked down its middle — so the two rings sat half a stroke
/// apart and read as a double outline instead of one filling arc. Drawing the track through
/// `value_ring` at 1.0 puts both on one geometry.
fn tile_value_arc(
    painter: &egui::Painter,
    theme: &Theme,
    puck: Rect,
    (value, live, fade): (f32, bool, f32),
) {
    let w = theme.control.stroke_mark;
    let r = puck.width() * 0.5 + theme.control.focus_gap + w * 0.5;
    let dim = theme.color(ColorRole::Outline).gamma_multiply(fade);
    value_ring(painter, puck.center(), r, 1.0, egui::Stroke::new(w, dim));
    if live {
        let ink = theme.color(ColorRole::Primary).gamma_multiply(fade);
        value_ring(painter, puck.center(), r, value, egui::Stroke::new(w, ink));
    }
}

/// **Whether a tile's puck is filled with the accent.**
///
/// A [`TileKind::Toggle`] that is on, and a tile **lit from [`QuickTile::lit_by`]** while its key
/// holds `Bool(true)`. It used to be `TileState::On | TileState::Value(_)`, which
/// made every slider tile permanently accent-filled — a brightness of 40 % is still a `Value`,
/// never an `Off` — so Brightness and Volume sat in the shade looking exactly as "on" as Wi-Fi,
/// and five of six tiles were a block of accent. Accent that is always on says nothing.
///
/// The kinds without an on/off (`Action`, `Status`, `Panel`, `Gauges`) do not fill **on their
/// own**; they are buttons, and a button that looks latched is a lie. But when the declaration
/// names a `lit_by` key, the device is saying the thing behind the tile is on (a cable is in, a
/// heater is at temperature) — that is a real state, and `lit_by` promises the tile is drawn lit
/// while it holds. An unfilled puck for it drew `On` and `Off` identically. A slider's readout is
/// its value arc, which is a shape and reads at a glance; `lit_by` on a slider is ignored.
fn tile_is_filled(tile: &QuickTile, state: TileState) -> bool {
    state == TileState::On
        && match tile.kind {
            TileKind::Toggle(_) => true,
            TileKind::Slider(_) => false,
            TileKind::Action(_)
            | TileKind::Status
            | TileKind::Panel { .. }
            | TileKind::Gauges { .. } => tile.lit_by.is_some(),
        }
}

/// One tile: **the integrator's painter draws it where there is one** (rung 5), the
/// built-in drawing otherwise. The tap, the long press and the row a tile opens stay the shell's,
/// in `draw_tiles`.
#[allow(clippy::too_many_arguments)] // One tile: where, what, its state, and who draws it.
fn draw_tile_cell(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    index: usize,
    tile: (&QuickTile, TileState, bool),
    cell: Rect,
    paint: CellPaint,
    brush: &mut TileBrush<'_>,
) {
    if let Some(painter) = brush.painter.as_deref_mut() {
        let label = galley(row.texts, TextKey::TileLabel(index)).map_or("", |g| g.text());
        paint_tile_with(painter, ui, parts, label, tile, cell, paint);
    } else {
        draw_builtin_tile(ui, parts, row, index, tile, cell, (paint, brush.ground));
    }
}

/// Hand one tile to the integrator's painter, whole (rung 5).
fn paint_tile_with(
    painter: &mut ShadeTilePainter,
    ui: &egui::Ui,
    parts: &mut CxParts<'_>,
    label: &str,
    (quick, state, allowed): (&QuickTile, TileState, bool),
    cell: Rect,
    paint: CellPaint,
) {
    let mut tile = ShadeTileCx {
        rect: cell,
        id: &quick.id,
        label,
        icon: &quick.icon,
        kind: &quick.kind,
        state,
        lit: tile_is_filled(quick, state),
        live: quick.enabled && state != TileState::Unavailable,
        allowed,
        pressed: paint.pressed,
        fade: paint.fade,
        theme: parts.theme,
        icons: &mut *parts.icons,
    };
    painter(ui.painter(), &mut tile);
}

/// The built-in tile: **a round toggle with a label below it**, on the panel's `ground`.
///
/// It used to paint the whole cell in the accent colour and put the label on top. That meant (1) an
/// enabled tile stood out on screen as a block of accent colour, (2) the label sat on the fill and
/// needed its contrast matched separately, and (3) the slider's value band fought the label for the
/// same place. Making **only the icon a disc**, as One UI and iOS do, and leaving the label on the
/// panel's ground, makes all three go away.
fn draw_builtin_tile(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    index: usize,
    (quick, state, allowed): (&QuickTile, TileState, bool),
    cell: Rect,
    (paint, ground): (CellPaint, Color32),
) {
    let CellPaint { pressed, fade } = paint;
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let live = quick.enabled && state != TileState::Unavailable;
    let on = tile_is_filled(quick, state);

    let label = galley(row.texts, TextKey::TileLabel(index));
    // **A label on its way out takes no place either.** The height is reduced by `fade` — otherwise, as
    // the tile shrinks into the Slider row, the puck is squeezed into the label's place and the icon
    // disappears (measured).
    let label_h = label.map_or(0.0, |g| g.size().y) * fade;
    let gap = style.gap * 0.5 * fade;
    let puck_d = style
        .puck
        .min(cell.width())
        .min(cell.height() - label_h - gap);
    let content_h = puck_d + gap + label_h;
    let top = cell.center().y - content_h * 0.5;
    let puck = Rect::from_center_size(
        egui::pos2(cell.center().x, top + puck_d * 0.5),
        Vec2::splat(puck_d),
    );

    let mut fill = theme.color(if on {
        ColorRole::Primary
    } else {
        ColorRole::SurfaceVariant
    });
    let mut fg = if on {
        ColorRole::OnPrimary
    } else {
        ColorRole::OnSurface
    };
    if !live {
        fill = fill.gamma_multiply(0.45);
        fg = ColorRole::Muted;
    }
    if pressed {
        fill = fill.gamma_multiply(0.82);
    }
    let painter = ui.painter();
    painter.circle_filled(puck.center(), puck_d * 0.5, fill);
    // An unfilled puck gets one line of border to bound it — so the surfaces do not merge on a dark
    // background. Most tiles are unfilled now, so the line comes off `stroke_hairline` rather than a
    // literal 1.0 and holds at any scale.
    if !on && live {
        // epaint strokes a circle *outside* its radius, so `r` is the inner edge and `r + w` the
        // outer one — the ring is inset by the full width, not half of it.
        let w = theme.control.stroke_hairline;
        painter.circle_stroke(
            puck.center(),
            puck_d * 0.5 - w,
            egui::Stroke::new(w, theme.color(ColorRole::Outline)),
        );
    }

    if let (TileKind::Slider(_), TileState::Value(v)) = (&quick.kind, state) {
        tile_value_arc(painter, theme, puck, (v, live, fade));
    }

    let icon_px = puck_d * 0.46;
    let icon_style = IconStyle {
        color: IconColor::Role(fg),
        ..IconStyle::sized(icon_px)
    };
    parts.icons.paint(
        ui.painter(),
        Rect::from_center_size(puck.center(), Vec2::splat(icon_px)),
        &quick.icon,
        &icon_style,
        theme,
    );

    if let Some(label) = label {
        let size = label.size();
        let color = if live {
            ColorRole::OnSurface
        } else {
            ColorRole::Muted
        };
        ui.painter().with_clip_rect(cell).galley(
            egui::pos2(cell.center().x - size.x * 0.5, puck.max.y + gap),
            Arc::clone(label),
            theme.color(color).gamma_multiply(fade),
        );
    }

    if !allowed {
        let lock = theme.metrics.desktop_lock_size;
        let badge = Rect::from_center_size(
            egui::pos2(puck.max.x - lock * 0.25, puck.min.y + lock * 0.25),
            Vec2::splat(lock),
        );
        // A panel-coloured disc behind the padlock keeps it readable even where it overlaps the puck's border.
        ui.painter()
            .circle_filled(badge.center(), lock * 0.62, ground);
        let lock_style = IconStyle {
            color: IconColor::Role(ColorRole::Warning),
            ..IconStyle::sized(lock)
        };
        parts
            .icons
            .paint(ui.painter(), badge, &builtin::LOCK, &lock_style, theme);
    }
}

/// Where each tile rests, into `spots.homes`, when an integrator's layout places them:
/// the built-in rows at rest, then wherever the layout moves them. It returns where the tile block
/// ends — under the lowest tile placed — or `None` without a layout.
fn place_tiles(
    rect: Rect,
    top: f32,
    theme: &Theme,
    row: &TileRow<'_>,
    spots: &mut TileSpots<'_>,
) -> Option<f32> {
    let layout = spots.layout.as_mut()?;
    let cols = row.columns.max(1);
    let tile_px = tile_width(rect.width(), cols, &theme.metrics);
    let count = row.tiles.len();
    let homes = &mut *spots.homes;
    homes.clear();
    for index in 0..count {
        let (line, col) = (index / cols, index % cols);
        let in_line = (count - line * cols).min(cols);
        let line_w = in_line as f32 * (tile_px + TILE_GAP) - TILE_GAP;
        let x = rect.center().x - line_w / 2.0 + col as f32 * (tile_px + TILE_GAP);
        let y = top + line as f32 * (tile_px + TILE_GAP);
        homes.push(Rect::from_min_size(egui::pos2(x, y), Vec2::splat(tile_px)));
    }
    let rows_n = count.div_ceil(cols).max(1);
    let area = Rect::from_min_max(
        egui::pos2(rect.min.x, top),
        egui::pos2(
            rect.max.x,
            top + rows_n as f32 * (tile_px + TILE_GAP) - TILE_GAP,
        ),
    );
    let expanded = row.expanded.filter(|i| *i < count);
    layout(
        &ShadeTileLayoutCx::new(rect, area, cols, expanded, theme, row.tiles),
        homes,
    );
    Some(
        homes
            .iter()
            .filter(|r| r.is_positive())
            .map(|r| r.max.y)
            .fold(top, f32::max),
    )
}

/// Where tile `index` rests and how big it is there: `in_row`, a `tile_px` square — or, with an
/// integrator's layout, the place it gave the tile in `homes`, and `None` where it emptied it.
fn tile_home(
    homes: Option<&[Rect]>,
    index: usize,
    in_row: egui::Pos2,
    tile_px: f32,
) -> Option<(egui::Pos2, Vec2)> {
    let Some(homes) = homes else {
        return Some((in_row, Vec2::splat(tile_px)));
    };
    let place = homes.get(index).copied().filter(Rect::is_positive)?;
    Some((place.center(), place.size()))
}

/// How wide the tiles `range` take in their row, each less by how far it has left the row.
/// The gap after the last cell does not count towards the row's width.
fn row_width(
    parts: &mut CxParts<'_>,
    range: std::ops::Range<usize>,
    expanded: Option<usize>,
    tween: crate::motion::Tween,
    tile_px: f32,
) -> f32 {
    let mut total = 0.0;
    for i in range.clone() {
        total += (tile_px + TILE_GAP) * (1.0 - tile_out(parts, i, expanded, tween));
    }
    if let Some(last) = range.last() {
        total -= TILE_GAP * (1.0 - tile_out(parts, last, expanded, tween));
    }
    total
}

/// The tile row (plus the Slider expansion row).
fn draw_tiles(
    ui: &mut egui::Ui,
    rect: Rect,
    top: f32,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    spots: &mut TileSpots<'_>,
) -> TilesOut {
    let metrics = parts.theme.metrics;
    let tween = parts.theme.motion.switch;
    let mut action = None;
    let mut expanded = row.expanded.filter(|i| *i < row.tiles.len());
    let cols = row.columns.max(1);
    let gap = TILE_GAP;
    let tile_px = tile_width(rect.width(), cols, &metrics);
    let n = row.tiles.len();
    let rows_n = n.div_ceil(cols).max(1);
    // Where the block of tiles ends: under the last row, or under the lowest tile an integrator's
    // layout placed.
    let placed = place_tiles(rect, top, parts.theme, row, spots);
    let block_bottom = placed.unwrap_or(top + (rows_n as f32 - 1.0) * (tile_px + gap) + tile_px);
    let homes = placed.is_some().then_some(spots.homes.as_slice());
    let tile_rects = &mut *spots.drawn;
    tile_rects.clear();
    // The **arrival point** of a tile on its way out = the Slider expansion row's icon place on the left.
    // The row count being fixed, it is known before the tiles are drawn.
    let icon_d = metrics.touch_target * parts.theme.components.shade.note_icon;
    let slider_top = block_bottom + 12.0;
    let slot = egui::pos2(
        rect.min.x + 24.0 + icon_d / 2.0,
        slider_top + metrics.widget_height / 2.0,
    );

    // An expanded tile **leaves the row and the tiles left fill its place.** The progress is held per
    // tile, so pressing another Slider tile has one go up and one come down, swapping places.
    //
    // Measuring the width needs the progress first, and the progress needs `parts` borrowed, so it is
    // walked **twice per row**. `AnimationStore::animate` only moves its value at the per-frame `tick`,
    // so calling it twice in the same frame gives the same value (this file is allocation-free per frame,
    // so an array cannot be used).
    let mut cursor_y = top;
    // A tile in motion is drawn **behind** the Slider row — otherwise the row's ground covers it.
    let mut moving: [(usize, Rect, f32); MOVING_MAX] = [(0, Rect::NOTHING, 0.0); MOVING_MAX];
    let mut moving_n = 0_usize;

    for r in 0..rows_n {
        let first = r * cols;
        let last = ((r + 1) * cols).min(n);
        let total = row_width(parts, first..last, expanded, tween, tile_px);
        let mut cursor_x = rect.center().x - total.max(0.0) / 2.0;
        for (index, (quick, state, allowed)) in row.tiles.iter().enumerate().take(last).skip(first)
        {
            let t = tile_out(parts, index, expanded, tween);
            // Where it rests and how big it is there: the row's, or the layout's — whose
            // tiles do not close up behind one on its way out. A place it emptied leaves the tile out.
            let in_row = egui::pos2(
                cursor_x + tile_px * (1.0 - t) / 2.0,
                cursor_y + tile_px / 2.0,
            );
            let Some((home, home_size)) = tile_home(homes, index, in_row, tile_px) else {
                tile_rects.push(Rect::NOTHING);
                continue;
            };
            let size = home_size + (Vec2::splat(icon_d) - home_size) * t;
            let cell = Rect::from_center_size(home + (slot - home) * t, size);
            cursor_x += (tile_px + gap) * (1.0 - t);
            tile_rects.push(cell);
            let resp = ui.interact(cell, egui::Id::new(("fairing.tile", index)), Sense::click());
            let live = quick.enabled && *state != TileState::Unavailable;
            if t > MOVING_EPS && moving_n < MOVING_MAX {
                if let Some(hold) = moving.get_mut(moving_n) {
                    *hold = (index, cell, t);
                }
                moving_n += 1;
            } else {
                let tile = (quick, *state, *allowed);
                let paint = CellPaint {
                    pressed: live && resp.is_pointer_button_down_on(),
                    fade: 1.0,
                };
                draw_tile_cell(ui, parts, row, index, tile, cell, paint, &mut spots.brush);
            }
            // On a press already handled as a long press, the release's tap is swallowed — otherwise
            // Wi-Fi turns off and the settings open with it.
            if resp.clicked() && action.is_none() && !row.swallow_tap {
                action = tile_tap(quick, *state, *allowed, live, index, &mut expanded);
            }
        }
        if r + 1 < rows_n {
            cursor_y += tile_px + gap;
        }
    }

    let mut bottom = block_bottom + 16.0;
    let mut slider_rect = Rect::NOTHING;
    let mut slider_value = None;
    // The Slider expansion row ("2–3 rows when expanded").
    let row_rect = Rect::from_min_size(
        egui::pos2(rect.min.x + 24.0, slider_top),
        Vec2::new(rect.width() - 48.0, expanded_height(row, expanded, metrics)),
    );
    match expanded_row(ui, parts, row, expanded, row_rect) {
        Some(out) => {
            if out.action.is_some() && action.is_none() {
                action = out.action;
            }
            slider_rect = out.rect;
            slider_value = out.value;
            bottom = row_rect.max.y + 12.0;
        }
        None => expanded = None,
    }

    let moving = moving.get(..moving_n).unwrap_or(&moving);
    draw_moving_tiles(ui, parts, row, moving, &mut spots.brush);

    TilesOut {
        action,
        bottom,
        expanded,
        slider_rect,
        slider_value,
    }
}

/// The expansion row's height. A row an integrator draws has a height of its own — three or four
/// gauges will not sit in one widget's worth of line.
fn expanded_height(
    row: &TileRow<'_>,
    expanded: Option<usize>,
    metrics: crate::theme::Metrics,
) -> f32 {
    expanded
        .and_then(|i| row.tiles.get(i))
        .map_or(metrics.widget_height, |(quick, _, _)| match &quick.kind {
            TileKind::Panel { height } => height.max(metrics.widget_height),
            // The row count × a finger's height. The declaration settles the row count, so the height comes from it.
            TileKind::Gauges { rows, .. } => (rows.len().max(1) as f32) * gauge_row_height(metrics),
            _ => metrics.widget_height,
        })
}

fn expanded_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    expanded: Option<usize>,
    row_rect: Rect,
) -> Option<SliderOut> {
    let index = expanded?;
    let (quick, state, allowed) = row.tiles.get(index)?;
    if !(*allowed && quick.enabled) {
        return None;
    }
    match &quick.kind {
        TileKind::Slider(key) => Some(draw_slider_row(
            ui,
            parts,
            SliderSlot {
                rect: row_rect,
                index,
                key,
                state: *state,
            },
        )),
        TileKind::Panel { .. } => Some(SliderOut {
            action: None,
            value: None,
            // A drag started in this row is not taken by the shade — a slider can be put inside it.
            rect: draw_panel_row(ui, parts, row, &quick.id, row_rect),
        }),
        TileKind::Gauges { rows, label_width } => Some(draw_gauges_row(
            ui,
            parts,
            GaugeRows {
                rect: row_rect,
                tile: index,
                rows,
                label_width: *label_width,
                texts: row.texts,
            },
        )),
        TileKind::Toggle(_) | TileKind::Action(_) | TileKind::Status => None,
    }
}

/// One gauge row's height — one finger. The row count × this is the expansion row's height.
fn gauge_row_height(metrics: crate::theme::Metrics) -> f32 {
    metrics.touch_target
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

/// [`TileKind::Gauges`]'s expansion row — **the crate draws it.**
///
/// A row is `name | value | track`, and the declaration sets the name and value columns' widths.
/// The track uses all the width left and its colour differs by row. The icon's place on the left is
/// left empty as it is for [`TileKind::Slider`] — the tile descends into it.
fn draw_gauges_row(ui: &mut egui::Ui, parts: &mut CxParts<'_>, spec: GaugeRows<'_>) -> SliderOut {
    let theme: &Theme = parts.theme;
    let metrics = theme.metrics;
    let icon_d = metrics.touch_target * theme.components.shade.note_icon;
    let gap = metrics.row_height * SLIDER_ICON_GAP;
    let body = Rect::from_min_max(
        egui::pos2(spec.rect.min.x + icon_d + gap, spec.rect.min.y),
        spec.rect.max,
    );
    let mut out = SliderOut {
        action: None,
        value: None,
        rect: body,
    };
    let row_h = gauge_row_height(metrics);
    let base_track = crate::widgets::base_thickness(theme);
    for (i, gauge) in spec.rows.iter().enumerate() {
        let top = row_h.mul_add(i as f32, body.min.y);
        let line = Rect::from_min_max(
            egui::pos2(body.min.x, top),
            egui::pos2(body.max.x, top + row_h),
        );
        if let Some(action) = draw_gauge(ui, parts, &spec, (i, gauge), (line, base_track)) {
            if out.action.is_none() {
                out.action = Some(action);
            }
        }
    }
    out
}

/// One gauge row. On a frame the value changed, it hands back a write action.
fn draw_gauge(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    spec: &GaugeRows<'_>,
    (index, gauge): (usize, &Gauge),
    (line, base_track): (Rect, f32),
) -> Option<PanelAction> {
    let theme: &Theme = parts.theme;
    let mid = line.center().y;
    let mut left = line.min.x;
    if spec.label_width > 0.5 {
        if let Some(g) = galley(spec.texts, TextKey::GaugeLabel(spec.tile, index)) {
            ui.painter().galley(
                egui::pos2(left, mid - g.size().y / 2.0),
                Arc::clone(g),
                theme.color(ColorRole::OnSurface),
            );
        }
        left += spec.label_width;
    }
    if !gauge.unit.is_empty() {
        if let Some(g) = galley(spec.texts, TextKey::GaugeValue(spec.tile, index)) {
            // The value is **right-aligned** — the track's starting point does not wobble as the digit count changes.
            let x = left + gauge.value_width - g.size().x - base_track * 0.5;
            ui.painter().galley(
                egui::pos2(x, mid - g.size().y / 2.0),
                Arc::clone(g),
                theme.color(ColorRole::Muted),
            );
        }
        left += gauge.value_width;
    }
    let track = Rect::from_min_max(egui::pos2(left, line.min.y), line.max);
    if track.width() < base_track * 2.0 {
        return None;
    }
    let current = gauge_value(parts, gauge);
    let resp = ui.interact(
        track,
        egui::Id::new(("fairing.tile.gauge", spec.tile, index)),
        if gauge.read_only {
            Sense::hover()
        } else {
            Sense::click_and_drag()
        },
    );
    // It follows the finger, and says so: the page under the shade reads no swipe off this drag.
    crate::drag::claim_if_held(&resp);
    let axis = track.shrink2(Vec2::new(base_track / 2.0, 0.0));
    let pressed = !gauge.read_only && resp.is_pointer_button_down_on();
    let mut value = current;
    let mut action = None;
    if pressed {
        if let Some(pos) = resp.interact_pointer_pos() {
            value = ((pos.x - axis.min.x) / axis.width().max(1.0)).clamp(0.0, 1.0);
            let pct = round_pct(value);
            if pct != round_pct(current) {
                action = Some(PanelAction::Launch(LaunchAction::Set(
                    gauge.key.clone(),
                    SettingValue::Int(pct),
                )));
            }
        }
    }
    // The press swell is the same tween as the built-in slider's (A7).
    let tween = theme.motion.press;
    let grow = parts.animations.animate(
        egui::Id::new(("fairing.tile.gauge.press", spec.tile, index)),
        f32::from(u8::from(pressed)),
        tween,
        parts.frame,
    );
    let theme: &Theme = parts.theme;
    let dim = if gauge.read_only { 0.55 } else { 1.0 };
    crate::widgets::paint_track(
        ui.painter(),
        track,
        value,
        // `from_theme` first, so the shade inherits the track's edge and its mover from the control
        // vocabulary and only overrides the two colours a gauge actually decides for itself.
        crate::widgets::TrackPaint {
            done: theme.color(gauge.color).gamma_multiply(dim),
            handle: theme.color(ColorRole::OnSurface).gamma_multiply(dim),
            ..crate::widgets::TrackPaint::from_theme(
                theme,
                base_track,
                crate::widgets::swollen(theme, base_track, grow),
            )
        },
    );
    action
}

/// A gauge's value, 0..=1 — it lives in the config as an integer `0..=100`.
fn gauge_value(parts: &CxParts<'_>, gauge: &Gauge) -> f32 {
    match parts.settings.get(&gauge.key) {
        Some(SettingValue::Int(v)) => (*v as f32 / 100.0).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// [`TileKind::Panel`]'s expansion row — an integrator draws the inside.
///
/// The icon's place on the left is left empty as it is for [`TileKind::Slider`]. That is because
/// **the tile descends into it** (`draw_tiles`'s shared-element transition) — a body covering it
/// would hide the icon.
fn draw_panel_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    id: &str,
    rect: Rect,
) -> Rect {
    let metrics = parts.theme.metrics;
    let icon_d = metrics.touch_target * parts.theme.components.shade.note_icon;
    let gap = metrics.row_height * SLIDER_ICON_GAP;
    let body = Rect::from_min_max(egui::pos2(rect.min.x + icon_d + gap, rect.min.y), rect.max);
    let Some(decl) = row.panels.iter().find(|d| d.id() == id) else {
        return body;
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
    child.set_clip_rect(child.clip_rect().intersect(body));
    let pane = PaneInfo {
        rect: body,
        outer: body,
        is_split: false,
        is_focused: false,
        inset_bottom: 0.0,
        instance: InstanceId::NONE,
    };
    let mut cx = parts.cx(pane, None);
    decl.draw_panel(&mut child, &mut cx);
    body
}

/// A tile in motion is drawn **behind the Slider row** — otherwise the row's ground covers it. Its
/// label dims as it goes.
fn draw_moving_tiles(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    moving: &[(usize, Rect, f32)],
    brush: &mut TileBrush<'_>,
) {
    for &(index, cell, t) in moving {
        let Some((quick, state, allowed)) = row.tiles.get(index) else {
            continue;
        };
        draw_tile_cell(
            ui,
            parts,
            row,
            index,
            (quick, *state, *allowed),
            cell,
            CellPaint {
                pressed: false,
                fade: 1.0 - t,
            },
            brush,
        );
    }
}

/// How many tiles can be in motion on one frame. A swap of places is two (one leaving, one
/// arriving), and counting the residual tween just after it, it does not exceed three.
const MOVING_MAX: usize = 4;
/// Anything that has moved less than this counts as being in place.
const MOVING_EPS: f32 = 0.001;

/// How far tile `index` has **left the row** (0 = in place, 1 = at the Slider row's icon place).
fn tile_out(
    parts: &mut CxParts<'_>,
    index: usize,
    expanded: Option<usize>,
    tween: crate::motion::Tween,
) -> f32 {
    parts.animations.animate(
        egui::Id::new(("fairing.tile.out", index)),
        f32::from(u8::from(expanded == Some(index))),
        tween,
        parts.frame,
    )
}

/// The Slider expansion row: a track that **shows whose value it is**. The value follows the finger
/// 1:1 (A7). It raises `Set(key, Int(pct))` only on a frame the integer percentage changes — once
/// the backend or the config takes it, the next frame's `TileState` follows.
///
/// **Its drawing is one with [`TouchSlider`](crate::widgets::TouchSlider).** It used to draw two
/// line segments and a puck here itself with a thickness of `4.0` written in, so changing the
/// `slider.track_ratio` token left the shade alone. Now they share the one
/// `widgets::slider::paint_track`.
///
/// **The icon on the left comes down from the tile.** With only a track there is no telling what is
/// being operated — "what is this bar for?" was actually asked. Sliding the icon from the tile's
/// place to the left of the row joins the two.
fn draw_slider_row(ui: &mut egui::Ui, parts: &mut CxParts<'_>, slot: SliderSlot<'_>) -> SliderOut {
    let theme: &Theme = parts.theme;
    let metrics = theme.metrics;
    let full_rect = slot.rect;
    let current = match slot.state {
        TileState::Value(v) => Some(v.clamp(0.0, 1.0)),
        _ => None,
    };
    // What is left after taking the icon's place off the left is the track.
    let icon_d = metrics.touch_target * theme.components.shade.note_icon;
    let gap = metrics.row_height * SLIDER_ICON_GAP;
    let track_rect = Rect::from_min_max(
        egui::pos2(full_rect.min.x + icon_d + gap, full_rect.min.y),
        full_rect.max,
    );
    let resp = ui.interact(
        track_rect,
        egui::Id::new(("fairing.tile.slider", slot.index)),
        Sense::click_and_drag(),
    );
    // It follows the finger, and says so: the page under the shade reads no swipe off this drag.
    crate::drag::claim_if_held(&resp);
    let base_track = crate::widgets::base_thickness(theme);
    let axis = track_rect.shrink2(Vec2::new(base_track / 2.0, 0.0));
    let pressed = current.is_some() && resp.is_pointer_button_down_on();
    let mut value = current.unwrap_or(0.0);
    let mut out = SliderOut {
        action: None,
        value: None,
        rect: track_rect,
    };
    if pressed {
        if let Some(pos) = resp.interact_pointer_pos() {
            value = ((pos.x - axis.min.x) / axis.width().max(1.0)).clamp(0.0, 1.0);
            out.value = Some(value);
            let pct = round_pct(value);
            if pct != round_pct(current.unwrap_or(0.0)) {
                out.action = Some(PanelAction::Launch(LaunchAction::Set(
                    slot.key.clone(),
                    SettingValue::Int(pct),
                )));
            }
        }
    }

    let live = current.is_some();
    let (done, todo, handle) = if live {
        (
            theme.color(ColorRole::Primary),
            theme.color(ColorRole::SurfaceVariant),
            theme.color(ColorRole::OnSurface),
        )
    } else {
        let muted = theme.color(ColorRole::Muted);
        (
            muted.gamma_multiply(0.55),
            muted.gamma_multiply(0.22),
            theme.color(ColorRole::ShadeSurface),
        )
    };
    let grow = parts.animations.animate(
        egui::Id::new(("fairing.tile.slider.press", slot.index)),
        f32::from(u8::from(pressed)),
        theme.motion.press,
        parts.frame,
    );
    let track_h = crate::widgets::swollen(theme, base_track, grow);
    crate::widgets::paint_track(
        ui.painter(),
        track_rect,
        value,
        // As above: the vocabulary's track, with only the three colours this tile chooses replaced.
        crate::widgets::TrackPaint {
            done,
            todo,
            handle,
            ..crate::widgets::TrackPaint::from_theme(theme, base_track, track_h)
        },
    );

    // The value goes above the track, only while it is held. The same convention as `TouchSlider`
    // — and measured off the track's own top, not the row's: at the row's top edge the label
    // stood entirely outside the row and the row's clip cut it in half.
    if grow > 0.01 && live {
        let x = axis.min.x + axis.width() * value;
        let top = track_rect.center().y - track_h * 0.5 - theme.components.slider.label_gap;
        ui.painter().text(
            egui::pos2(x, top),
            egui::Align2::CENTER_BOTTOM,
            format!("{}%", round_pct(value)),
            egui::TextStyle::Small.resolve(ui.style()),
            theme.color(ColorRole::OnSurface).gamma_multiply(grow),
        );
    }
    out
}

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

/// 0…1 as an integer percentage.
#[expect(
    clippy::cast_possible_truncation,
    reason = "it is clamp(0,1) × 100, so it is inside i64's range"
)]
fn round_pct(v: f32) -> i64 {
    (v.clamp(0.0, 1.0) * 100.0).round() as i64
}

/// Between the icon and the track in a Slider row (× `row_height`).
const SLIDER_ICON_GAP: f32 = 0.22;

/// The notification list (inside the scroll area).
fn draw_list(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    center: &NotificationCenter,
    row: &TileRow<'_>,
) -> ListOut {
    let theme: &Theme = parts.theme;
    let metrics = theme.metrics;
    let mut out = ListOut {
        action: None,
        pressed: None,
        close_hit: Rect::NOTHING,
    };
    let pressed_now = ui.input(|i| i.pointer.primary_pressed());
    if center.is_empty() {
        let (empty, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), metrics.notification_row_height),
            Sense::hover(),
        );
        if let Some(g) = galley(row.texts, TextKey::NoNotifications) {
            let size = g.size();
            ui.painter().galley(
                empty.center() - size * 0.5,
                Arc::clone(g),
                theme.color(ColorRole::Muted),
            );
        }
    }
    for item in center.iter() {
        // A row mid-collapse: the card is already off-screen, so only **the space revealed** (the
        // throw-away marking) is left. It has to collapse together with the place for it to read as "the
        // discard slot closing" — the marking vanishing first and the place shrinking after looks like two
        // separate actions. It takes no input: a 1 px row taking a tap would open the notification just
        // removed.
        if let Some((_, scale)) = row.collapsing.filter(|(id, _)| *id == item.id) {
            let h = metrics.notification_row_height * scale;
            if h <= 0.5 {
                continue;
            }
            let (slot, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
            if ui.is_rect_visible(slot) {
                let style = PanelStyle::new(&metrics, theme.components.shade);
                let home = slot.shrink2(Vec2::new(style.pad, style.card_gap * 0.5));
                swipe_hint(ui, parts, item, home, -slot.width(), &style);
            }
            continue;
        }
        // The row is a touch target and a gap — and, where the title and body ask for more,
        // their two lines with the card's padding round them. The tokens are hand-sized and the
        // text is eye-sized, and at a coarse finger the finger's row was shorter than the
        // eye's two lines: the body sat on the card's bottom edge, half outside it.
        let style = PanelStyle::new(&metrics, theme.components.shade);
        let text_h = {
            let title = galley(row.texts, TextKey::Title(item.id)).map_or(0.0, |g| g.size().y);
            let body = galley(row.texts, TextKey::Body(item.id)).map_or(0.0, |g| g.size().y);
            title
                + if body > 0.0 {
                    style.card_gap * 0.5 + body
                } else {
                    0.0
                }
        };
        let row_h = notification_row_h(&metrics, &style)
            .max(text_h + style.card_gap + 2.0 * style.card_pad);
        let (row_rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
        if !ui.is_rect_visible(row_rect) {
            continue;
        }
        // Gate condensation is enforced at the notification centre alone — a row whose gate fails
        // hides its title, body and progress (the `allowed` branch of `draw_notification_row`).
        let allowed = crate::notify::shows_content(item, parts.access);
        let dx = row
            .swiping
            .filter(|(id, _)| *id == item.id)
            .map_or(0.0, |(_, x)| x);
        if resp.clicked() && out.action.is_none() {
            out.action = Some(PanelAction::NotificationTapped(item.id));
        }
        if !item.persistent && pressed_now && resp.contains_pointer() {
            out.pressed = Some(item.id);
        }
        let card = draw_notification_row(ui, parts, row, item, row_rect, dx, allowed);
        if out.close_hit == Rect::NOTHING {
            out.close_hit = card.close_hit;
        }
        if card.closed && allowed {
            out.action = Some(PanelAction::Dismiss(item.id));
        }
    }
    out
}

/// Draw **the outcome** in the space a swipe reveals — throwing it away on the left, opening it on
/// the right.
///
/// The direction decides the meaning, so the drawing has to split too. Were both the same grey,
/// which way the finger is pushing would not connect to what happens.
///
/// The right side appears **only on a notification with somewhere to go** (`action`). Without one,
/// `Overlay` does not pull to the right in the first place.
fn swipe_hint(
    ui: &egui::Ui,
    parts: &mut CxParts<'_>,
    item: &Notification,
    home: Rect,
    dx: f32,
    style: &PanelStyle,
) {
    if dx.abs() < 0.5 {
        return;
    }
    let theme: &Theme = parts.theme;
    let to_open = dx > 0.0;
    if to_open && item.action.is_none() {
        return;
    }
    let (role, icon) = if to_open {
        (ColorRole::Primary, &builtin::CHEVRON_RIGHT)
    } else {
        (ColorRole::Danger, &builtin::TRASH)
    };
    let tint = theme.color(role);
    let painter = ui.painter().with_clip_rect(home);
    painter.rect_filled(home, style.radius, tint.gamma_multiply(HINT_GROUND));
    // The icon goes at **the end being revealed** — pushed left it is the right end, and the same the other way.
    let d = theme.metrics.touch_target * theme.components.shade.note_icon;
    let inset = style.card_pad + d / 2.0;
    let cx_ = if to_open {
        home.left() + inset
    } else {
        home.right() - inset
    };
    let icon_style = IconStyle {
        color: IconColor::Role(role),
        ..IconStyle::sized(d)
    };
    parts.icons.paint(
        &painter,
        Rect::from_center_size(egui::pos2(cx_, home.center().y), Vec2::splat(d)),
        icon,
        &icon_style,
        theme,
    );
}

/// How solid the swipe hint's ground is. Enough for the icon to read while what is under the card stays like a background.
const HINT_GROUND: f32 = 0.18;

/// One notification card's output.
struct CardOut {
    /// The close was tapped.
    closed: bool,
    /// The close's **hit** Rect (wider than the × drawn). `Rect::NOTHING` where there is no close.
    close_hit: Rect,
}

/// One notification card (the icon disc · the title · the body · the progress · the close).
///
/// The structure divides into three — the icon column on the left, the text column in the middle,
/// the close column on the right. The text starts on **one vertical line** and the icon's optical
/// centre is matched to the title's line (not to the card's centre — with a long body the icon would
/// drift away from the title, blurring which text it belongs to).
#[expect(
    clippy::too_many_lines,
    reason = "it lays out one card's three columns in one place"
)]
fn draw_notification_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    item: &Notification,
    row_rect: Rect,
    dx: f32,
    allowed: bool,
) -> CardOut {
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let home = row_rect.shrink2(Vec2::new(style.pad, style.card_gap * 0.5));
    let card = home.translate(Vec2::new(dx, 0.0));
    let painter = ui.painter().with_clip_rect(row_rect);
    // **What is about to happen** is laid down in the place the card has moved off first, and the card goes over it.
    swipe_hint(ui, parts, item, home, dx, &style);
    painter.rect_filled(card, style.radius, theme.color(ColorRole::SurfaceVariant));

    let heading = if allowed {
        galley(row.texts, TextKey::Title(item.id))
    } else {
        galley(row.texts, TextKey::Hidden)
    };
    let body = allowed
        .then(|| galley(row.texts, TextKey::Body(item.id)))
        .flatten();
    let title_h = heading.map_or(0.0, |g| g.size().y);
    let body_h = body.map_or(0.0, |g| g.size().y);
    let line_gap = style.card_gap * 0.5;
    let text_h = title_h + if body_h > 0.0 { line_gap + body_h } else { 0.0 };
    let text_top = card.center().y - text_h * 0.5;

    // The icon: a disc centred on **the whole text block**, not on the title's line.
    //
    // It was `text_top + title_h * 0.5` — the title's own centre — which is right for a one-line
    // notification and visibly high on a two-line one, where it sits above the optical centre of
    // the title-and-body pair. `text_h` was already computed three lines up for exactly this block;
    // the badge simply was not using it.
    let puck_d = style.note_icon;
    let puck = Rect::from_center_size(
        egui::pos2(
            card.min.x + style.card_pad + puck_d * 0.5,
            text_top + text_h * 0.5,
        ),
        Vec2::splat(puck_d),
    );
    painter.circle_filled(
        puck.center(),
        puck_d * 0.5,
        theme.color(ColorRole::ShadeSurface),
    );
    let icon_px = puck_d * 0.52;
    let icon_style = IconStyle {
        color: IconColor::Role(if allowed {
            item.level.role()
        } else {
            ColorRole::Muted
        }),
        ..IconStyle::sized(icon_px)
    };
    // A gated row shows the bell and nothing else — the shape is part of the content it is hiding.
    // An icon the caller set is borrowed, never cloned per frame; the fallbacks are both
    // `Builtin`, so neither allocates.
    let fallback: IconRef = if allowed {
        item.level.icon()
    } else {
        builtin::BELL
    };
    let icon: &IconRef = match (&item.icon, allowed) {
        (Some(icon), true) => icon,
        _ => &fallback,
    };
    parts.icons.paint(
        &painter,
        Rect::from_center_size(puck.center(), Vec2::splat(icon_px)),
        icon,
        &icon_style,
        theme,
    );

    // With `[notify] dismiss_button = false` the × is not drawn at all — the only ways to dismiss are a
    // left swipe and "clear all". Not drawing it widens the text column by that much.
    let closable = row.dismiss_button && !item.persistent && allowed;
    let close_d = style.close;
    let text_x = puck.max.x + style.card_pad;
    let text_right = if closable {
        card.max.x - style.card_pad - close_d
    } else {
        card.max.x - style.card_pad
    };
    let text_painter = painter.with_clip_rect(Rect::from_min_max(
        egui::pos2(text_x, card.min.y),
        egui::pos2(text_right, card.max.y),
    ));
    let mut y = text_top;
    if let Some(g) = heading {
        text_painter.galley(
            egui::pos2(text_x, y),
            Arc::clone(g),
            theme.color(ColorRole::OnSurface),
        );
        y += title_h + line_gap;
    }
    if let Some(g) = body {
        text_painter.galley(
            egui::pos2(text_x, y),
            Arc::clone(g),
            theme.color(ColorRole::Muted),
        );
    }
    if allowed {
        if let Some(p) = item.progress {
            let h = 3.0;
            let bar = Rect::from_min_size(
                egui::pos2(text_x, card.max.y - style.card_pad * 0.6 - h),
                Vec2::new((text_right - text_x).max(0.0), h),
            );
            painter.rect_filled(bar, h * 0.5, theme.color(ColorRole::Outline));
            let done = Rect::from_min_size(bar.min, Vec2::new(bar.width() * p.clamp(0.0, 1.0), h));
            painter.rect_filled(done, h * 0.5, theme.color(ColorRole::Primary));
        }
    }
    if !closable {
        return CardOut {
            closed: false,
            close_hit: Rect::NOTHING,
        };
    }
    let close = Rect::from_center_size(
        egui::pos2(card.max.x - style.card_pad - close_d * 0.5, card.center().y),
        Vec2::splat(close_d),
    );
    // **The size drawn and the place pressed are separated** — the same prescription as a grid
    // cell's `rect` and `visual`.
    //
    // The close × is drawn at a multiple of `shade.close` (0.72), which comes to 9.4 mm under the gloved
    // default — short of `finger_hard_mm` (10.1 mm). Growing the drawing would bury the notification card
    // in ×s, so **the drawing is left as it is and only the hit Rect** widens to the touch target. It
    // widens leftwards and covers the body text, but the text is not pressable, and between the two the
    // close is the smaller than a card tap (opening the notification), so it is right that it takes the place.
    let hit_d = close_d
        .max(parts.theme.metrics.touch_target)
        .min(card.height());
    let close_hit = Rect::from_center_size(close.center(), Vec2::splat(hit_d))
        .intersect(card)
        .translate(Vec2::new(-dx, 0.0));
    let close_resp = ui.interact(
        close_hit,
        egui::Id::new(("fairing.notif.x", item.id.0)),
        Sense::click(),
    );
    if close_resp.hovered() || close_resp.is_pointer_button_down_on() {
        painter.circle_filled(
            close.center(),
            close_d * 0.5,
            theme.color(ColorRole::Pressed),
        );
    }
    let x_px = close_d * 0.42;
    let x_style = IconStyle {
        color: IconColor::Role(ColorRole::Muted),
        ..IconStyle::sized(x_px)
    };
    parts.icons.paint(
        &painter,
        Rect::from_center_size(close.center(), Vec2::splat(x_px)),
        &builtin::CLOSE,
        &x_style,
        theme,
    );
    CardOut {
        closed: close_resp.clicked(),
        close_hit,
    }
}

/// The footer's icon buttons (filled from the right). `true` if it was tapped.
fn footer_icon_button(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    right: &mut f32,
    footer: Rect,
    id: &'static str,
    icon: &IconRef,
) -> bool {
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let d = style.footer_button;
    // **Here the place widens too**. Unlike the close ×, the footer buttons are separated
    // from their neighbours by only `card_gap` (≈ 8 du), so widening the hit Rect alone would overlap the
    // next button and press the wrong thing. So **the slot** is taken at the touch target and a disc of
    // diameter `d` drawn inside it — the finger settles the place and the token the drawing. The buttons
    // spreading apart by that much is the intended result.
    let slot = d
        .max(parts.theme.metrics.touch_target)
        .min(footer.height().max(d));
    let cell = Rect::from_center_size(
        egui::pos2(*right - slot * 0.5, footer.center().y),
        Vec2::splat(slot),
    );
    *right = cell.min.x - style.card_gap;
    let resp = ui.interact(cell, egui::Id::new(id), Sense::click());
    let down = resp.is_pointer_button_down_on();
    // An icon floating alone does not show where the pressable place is — a disc beneath it declares it a button.
    ui.painter().circle_filled(
        cell.center(),
        d * 0.5,
        theme.color(if down {
            ColorRole::Primary
        } else {
            ColorRole::SurfaceVariant
        }),
    );
    let icon_px = d * 0.46;
    let icon_style = IconStyle {
        color: IconColor::Role(if down {
            ColorRole::OnPrimary
        } else {
            ColorRole::OnSurface
        }),
        ..IconStyle::sized(icon_px)
    };
    parts.icons.paint(
        ui.painter(),
        Rect::from_center_size(cell.center(), Vec2::splat(icon_px)),
        icon,
        &icon_style,
        theme,
    );
    resp.clicked()
}

/// The footer (the subject's name · the level dot · clear all · lock · settings) plus the bottom handle.
fn draw_footer(
    ui: &mut egui::Ui,
    rect: Rect,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    center: &NotificationCenter,
    footer_h: f32,
) -> Option<PanelAction> {
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let mut action = None;
    let footer = Rect::from_min_size(
        egui::pos2(rect.min.x + style.pad, rect.max.y - footer_h - style.gap),
        Vec2::new(rect.width() - style.pad * 2.0, footer_h),
    );
    // The separator between the list and the footer. It keeps the scroll from looking as though it flows under the footer.
    ui.painter().hline(
        footer.min.x..=footer.max.x,
        footer.min.y - style.gap * 0.5,
        egui::Stroke::new(1.0, theme.color(ColorRole::Outline)),
    );

    // The level's colour dot: Primary at the top level, Muted otherwise. Who is signed in goes with
    // the lock and the settings, so a split shade shows it on the controls and not under the list.
    if row.panel != OverlayPanel::Notifications {
        let session = parts.access.session();
        let top = parts.access.table().top();
        let dot_color = theme.color(if session.subject.level == top {
            ColorRole::Primary
        } else {
            ColorRole::Muted
        });
        let dot_r = style.gap * 0.3;
        ui.painter().circle_filled(
            egui::pos2(footer.min.x + dot_r, footer.center().y),
            dot_r,
            dot_color,
        );
        if let Some(g) = galley(row.texts, TextKey::FooterName) {
            let size = g.size();
            ui.painter().galley(
                egui::pos2(
                    footer.min.x + dot_r * 2.0 + style.card_gap,
                    footer.center().y - size.y * 0.5,
                ),
                Arc::clone(g),
                theme.color(ColorRole::OnSurface),
            );
        }
    }

    // `[overlay] footer` reads left to right as it is drawn, so it is laid out from the right in
    // reverse. **Nothing here is unconditional** — a device with no lock concept, or one that never
    // registered `settings.home`, empties the list and the buttons are gone.
    let mut right = footer.max.x;
    for button in row.footer.iter().rev() {
        match button {
            FooterButton::Settings => {
                if footer_icon_button(
                    ui,
                    parts,
                    &mut right,
                    footer,
                    "fairing.overlay.footer.settings",
                    &builtin::SETTINGS,
                ) {
                    action = Some(PanelAction::Launch(LaunchAction::open("settings.home")));
                }
            }
            FooterButton::Lock => {
                if footer_icon_button(
                    ui,
                    parts,
                    &mut right,
                    footer,
                    "fairing.overlay.footer.lock",
                    &builtin::LOCK,
                ) {
                    action.get_or_insert(PanelAction::Launch(LaunchAction::Lock));
                }
            }
            FooterButton::ClearAll => {
                clear_all_button(ui, parts, &mut right, footer, row, center, &mut action);
            }
        }
    }

    // The handle is drawn at **the curtain's end** (the bottom of what is revealed) — towards the finger
    // while it is being pulled, and at the panel's bottom once open (the clip is the whole panel). A
    // caller that has set no clip draws it at the panel's bottom.
    let edge = ui.clip_rect().max.y.min(rect.max.y);
    let handle_h = 4.0;
    let handle = Rect::from_center_size(
        egui::pos2(rect.center().x, edge - handle_h * 1.6),
        Vec2::new(theme.metrics.shade_handle_width, handle_h),
    );
    ui.painter()
        .rect_filled(handle, handle_h * 0.5, theme.color(ColorRole::Outline));
    action
}

/// "Clear all", as a pill rather than a bare text link — a link does not show where the pressable
/// place is. Drawn only where there is something to clear.
fn clear_all_button(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    right: &mut f32,
    footer: Rect,
    row: &TileRow<'_>,
    center: &NotificationCenter,
    action: &mut Option<PanelAction>,
) {
    if center.is_empty() {
        return;
    }
    let Some(g) = galley(row.texts, TextKey::ClearAll) else {
        return;
    };
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let size = g.size();
    let h = style.footer_button;
    let w = size.x + style.card_pad * 2.0;
    let clear = Rect::from_center_size(
        egui::pos2(*right - w * 0.5, footer.center().y),
        Vec2::new(w, h),
    );
    *right = clear.min.x - style.card_gap;
    let resp = ui.interact(
        clear,
        egui::Id::new("fairing.overlay.clear"),
        Sense::click(),
    );
    let down = resp.is_pointer_button_down_on();
    ui.painter().rect_filled(
        clear,
        h * 0.5,
        theme.color(if down {
            ColorRole::Primary
        } else {
            ColorRole::SurfaceVariant
        }),
    );
    ui.painter().galley(
        clear.center() - size * 0.5,
        Arc::clone(g),
        theme.color(if down {
            ColorRole::OnPrimary
        } else {
            ColorRole::Primary
        }),
    );
    if resp.clicked() {
        action.get_or_insert(PanelAction::ClearAll);
    }
}

#[cfg(test)]
mod tests {
    use super::{tile_is_filled, QuickTile, TileKind, TileState};
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
