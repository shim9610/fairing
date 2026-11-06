//! The overlay — the Shade / `ControlCenter` (A1). Feature `overlay`.
//!
//! Two layouts: `unified` (the Android way: one shade, pulled from anywhere along the top) and
//! `split` (One UI's and iOS's: two panels picked by where the pull starts).
//!
//! - The state machine, the driving value and the release rules are [`Shade`]'s. The scrim is
//!   `scrim`'s, the content [`PanelOutput`] and its kin, the tiles [`TileDecl`] and [`TileKind`].
//! - Frame stage 5, `Overlay::update`, consumes **the engine's top edge swipe** and advances,
//!   and stage 11, `Overlay::ui`, draws it as an `Area(Order::Foreground)` (scrim then panel, so
//!   the scrim is underneath). A finger on an open (or settling) panel is caught by `ui` from the
//!   raw pointer.
//! - **The render mapping is a curtain** (A1): the panel's
//!   **content** is pinned to the top of the screen and only the revealed part `[top, top + y]` is
//!   clipped — so what shows mid-pull is the panel's **head** (the status row and the tile rows)
//!   while the footer and the handle are still outside the curtain. The bottom handle is drawn at
//!   the curtain's end (`panel::show`). Only while the rubber band has `y > H` does the content
//!   come down with it by the excess. [`OverlayFrame::panel`] is therefore the **revealed** Rect.
//! - The moment it leaves `Closed` the focused screen is `Paused`, and on returning `Resumed` —
//!   the shell handles it from the change in [`Overlay::is_closed`].
//! - With the status bar hidden (`BarMode::Hide`, `allow_peek`) the panel's **head** includes the
//!   **status row**, so the status row is the first thing revealed in the first 32 px. The shell
//!   draws the status bar into [`OverlayFrame::status_row`]. **The peek rule**:
//!   if it was released without opening and the movement from the press point is at least
//!   half the status bar's height, the status row alone stays for `[overlay] peek_ms`
//!   ([`OverlayFrame::peek`], from the point the shade is fully closed). Pull again and the peek
//!   row carries on as the panel's status row.
//!
//! **Deciding who owns a gesture over the panel** (A1, "nested scrolling while open"; once per
//! gesture). It is decided on the frame of the press —
//! - A press mid-`Settling` → the shade is caught at that y immediately (re-entry, v = 0).
//! - A press in `Open` **outside** the notification list (on the tiles, the footer or the status
//!   row), or on the list with last frame's offset at 0 → **the shade owns it**: past the slop,
//!   `Dragging` from `H`, 1:1 upwards and a rubber band downwards. The release rules are the edge
//!   swipe's.
//! - A press on the list with the offset > 0 → **the list owns it**: a `ScrollArea` drag scroll.
//!   Even if the offset reaches 0 mid-scroll, that gesture ends as a scroll (no rubber band, no
//!   hand-off).
//! - A press on a non-persistent notification row in the list that passes the slop horizontally →
//!   **a row swipe dismissal**.
//! - If the engine recognises the same press as a top edge swipe (`Started`), the engine takes it —
//!   the origin being the same, `y = the starting value + the movement` carries on.
//! - A press over an expanded slider row is the slider's.
// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of pedantic stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

mod render;
mod transitions;

mod backdrop;
#[cfg(test)]
mod capture_tests;
mod card;
mod hooks;
mod panel;
mod relief;
mod scrim;
mod shade;
pub(crate) mod tiles;

pub use card::{CardAnchor, OverlayReveal};
pub use hooks::{
    ShadePanelCx, ShadePanelPainter, ShadeTileCx, ShadeTileLayout, ShadeTileLayoutCx,
    ShadeTilePainter,
};
pub use panel::{PanelAction, PanelOutput, PanelText, TextKey, TileRow};
pub use shade::{OverlayState, Shade};
pub use tiles::{
    builtin as builtin_tile, rotation_lock_available, tile, tile_panel, Gauge, QuickTile, TileDecl,
    TileKind, TileState, BUILTIN_IDS,
};

use crate::access::Access;
use crate::config::OverlayConfig;
use crate::gesture::{Edge, Gesture, Phase};
use crate::motion::{DragSpring, ReleaseRule, RubberBand};
use crate::notify::{NotificationCenter, NotificationId};
use crate::screen::CxParts;
use crate::services::Services;
use crate::settings::{keys, SettingValue, SettingsView};
use crate::theme::{ColorRole, MotionTokens, Theme};
use card::{CardSide, CardStyle};
use egui::epaint::text::FontsView;
use egui::epaint::Shadow;
use egui::{FontId, Pos2, Rect, Vec2};
use panel::{TileBrush, TileSpots};
use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// A button on the shade's footer row (`[overlay] footer`).
///
/// The subject's name and the level dot are not in here — they are state, not controls, and always
/// draw. These three are controls, so which of them exist is the device's to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FooterButton {
    /// Clear every notification. Drawn only where there is something to clear.
    ClearAll,
    /// Lock the session ([`LaunchAction::Lock`](crate::LaunchAction::Lock)).
    Lock,
    /// Open `settings.home` — which exists only if the integrator registered the built-in settings
    /// screens ([`settings::add_all`](crate::settings::add_all)).
    Settings,
}

/// The layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OverlayLayout {
    /// The Android way — one shade.
    #[default]
    Unified,
    /// The iOS way — notifications on the left, controls on the right (M6).
    Split,
}

/// The kinds of panel (two under `split`, one under `unified`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverlayPanel {
    /// Tiles plus notifications plus the footer.
    Shade,
    /// Notifications only (M6).
    Notifications,
    /// The tile grid (M6).
    ControlCenter,
}

/// The frame stage 5 input.
#[derive(Debug, Clone, Copy)]
pub struct OverlayInput {
    /// This frame's gesture.
    pub gesture: Option<Gesture>,
    /// The screen.
    pub screen: Rect,
    /// The content Rect (the cap on the shade's height).
    pub content: Rect,
    /// Whether the status bar is hidden (peek — the panel includes the status row).
    pub status_hidden: bool,
    /// The status bar's height.
    pub status_height: f32,
    /// Whether the `overlay.open` gate passes.
    pub allowed: bool,
    /// The shell's time.
    pub now: Instant,
    /// dt.
    pub dt: f32,
}

/// The stage 11 output — the shell puts the status row and the shield on top of it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OverlayFrame {
    /// The panel's **visible** Rect (while visible). For a curtain it is the part revealed, `[the
    /// top of the screen, the top + y]`, and below it is the scrim's; for a card it is
    /// the card itself, inset from the sides, wherever it is on its way down.
    pub panel: Option<Rect>,
    /// The status row's Rect within the panel (with the status bar hidden and the panel visible).
    /// Being in the panel's content coordinates (pinned to its head), only its intersection with
    /// [`OverlayFrame::panel`] is visible until the curtain has passed it.
    pub status_row: Option<Rect>,
    /// The status row briefly visible under peek (the shade is closed).
    pub peek: Option<Rect>,
}

/// An overlay action (shell stage 14).
pub type OverlayAction = PanelAction;

/// The panel `Area` id's name (it has a fixed place in the z-order).
pub(crate) const PANEL_AREA_ID: &str = "fairing.overlay.panel";

/// The `Area` the split shade's **outgoing** panel is drawn in while the header crosses to the
/// other one — beside the panel, above it, and taking no input.
pub(crate) const PANEL_LEAVING_AREA_ID: &str = "fairing.overlay.panel.leaving";

/// Who owns a press over the panel (see the module docs). `Pending` on the frame of the press,
/// settled on the frame it passes the slop, and `None` on release.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    /// None (or a press the engine, the scrim or a slider holds).
    None,
    /// Pressed, within the slop — the direction and the owner are undecided.
    Pending {
        /// The press point.
        origin: Pos2,
        /// Past the slop, the shade owns it (outside the list, or at offset 0).
        shade: bool,
        /// The non-persistent notification row pressed (past the slop horizontally, a swipe dismissal).
        row: Option<NotificationId>,
    },
    /// A shade drag (`y = the starting value + (pos.y − origin_y)`).
    Shade {
        /// The y pressed at.
        origin_y: f32,
    },
    /// A list scroll (the `ScrollArea` holds it).
    List,
    /// A notification row swipe.
    Row {
        /// The row.
        id: NotificationId,
        /// The x pressed at.
        origin_x: f32,
    },
}

/// One frame's pointer (one `ctx.input`).
#[derive(Debug, Clone, Copy)]
struct Pointer {
    pressed: bool,
    down: bool,
    pos: Option<Pos2>,
    /// Where the press went down ([`crate::drag::press_point`]) — which side a card is pulled
    /// from, which panel a split shade opens, and what a press on the open panel lands on. Not
    /// `pos`, which a fast finger has carried on by the frame that sees the press.
    origin: Option<Pos2>,
    velocity: Vec2,
    delta: Vec2,
}

/// What the panel is drawn as this frame, for either reveal: where the content is laid
/// out, what part of it shows, and how the plate under it is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Plate {
    /// Where the content is laid out — the panel's full height, for either reveal.
    content: Rect,
    /// What shows: the clip, the hit Rect, [`OverlayFrame::panel`].
    visible: Rect,
    /// The plate's corners: a curtain's two bottom ones, a card's four.
    corner: egui::CornerRadius,
    /// The plate's colour, its opacity included.
    fill: egui::Color32,
    /// A soft, feathered copy of the plate under it while a card is still a blob — the feather's
    /// width and the colour. `None` once it has landed, and always for a curtain.
    soft: Option<(f32, egui::Color32)>,
    /// The content's opacity — a card's content resolves after its plate.
    content_alpha: f32,
    /// How far the plate has arrived, 0..=1: a card's materialising, times the fade of a split
    /// panel being crossed away from. A curtain is always there. What a panel painter is told as
    /// its `alpha`.
    arrival: f32,
    /// A curtain closes with a line along its edge; a card has no edge to close.
    edge_line: bool,
    /// How far past `visible` the plate may paint — a card's soft edge and its shadow. A curtain
    /// paints inside what it has revealed and nowhere else.
    bleed: f32,
    /// The frosted backdrop under a card, once the runner has answered.
    glass: Option<Glass>,
    /// A card's relief, `[overlay] card_relief` — and with it the floating rim a dark
    /// palette draws. `None` for a curtain, which is a sheet hung from the edge, not a slab.
    relief: Option<f32>,
}

/// A card's frosted backdrop as one frame draws it: the texture, where the card's Rect falls in
/// it, and how much of it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glass {
    texture: egui::TextureId,
    uv: Rect,
    alpha: f32,
}

/// The frosted screenshot a card lies on, the viewport it was taken of, and the card's surface
/// colour then — the two things that, changed under an open card, leave the frost showing a page
/// that is no longer there.
struct Backdrop {
    texture: egui::TextureHandle,
    viewport: Rect,
    surface: egui::Color32,
}

/// A response belongs to the viewport and surface present when it was requested.
struct PendingShot {
    rx: crate::inbox::Inbox<Arc<egui::ColorImage>>,
    viewport: Rect,
    surface: egui::Color32,
}

/// How many frames a card waits for the runner to answer its screenshot before it gives up and
/// stays solid — a runner without the command never answers, and a frame late is all a real one
/// is.
const SHOT_PATIENCE: u8 = 12;

/// Shade / `ControlCenter`.
// The four flags (a declaration dirty · the status bar hidden · a dismissal · the engine owning it) are
// independent switches set and read by different frame stages, so bundling them into an enum would only
// read worse.
#[allow(clippy::struct_excessive_bools)]
pub struct Overlay {
    shade: Shade,
    layout: OverlayLayout,
    panels: Vec<OverlayPanel>,
    tile_ids: Vec<String>,
    /// The footer buttons to draw, left to right (`[overlay] footer`). Empty draws none.
    footer: Vec<FooterButton>,
    tile_columns: usize,
    max_height_ratio: f32,
    peek_ms: Duration,
    /// `[overlay] two_step` — whether the shade stops at the tiles on the way down. Always off
    /// under `split`.
    two_step: bool,
    /// `[overlay] split_ratio` — where the split shade's two panels divide the top edge.
    split_ratio: f32,
    /// **The panel shown**: always [`OverlayPanel::Shade`] when unified; under `split`
    /// the one the pull or tap that opened the shade picked, or the one the header crossed to.
    panel: OverlayPanel,
    /// Where the latest press landed — what decides a split shade's panel when it opens.
    last_press: Option<Pos2>,
    /// The overlay's screen as of the last `update`.
    screen: Rect,
    /// From the overlay's top to the bottom of the content area, as of the last `update` — the
    /// height a split panel fills.
    room: f32,
    /// The footer buttons each split panel draws: "clear all" goes with the list, the lock and the
    /// settings with the controls.
    footer_notes: Vec<FooterButton>,
    /// See [`Overlay::footer_notes`].
    footer_controls: Vec<FooterButton>,
    /// **The panel being crossed from**, and the plate it was showing, while the header's dissolve
    /// runs ([`Overlay::fade_t`] 0 → 1).
    fade: Option<(OverlayPanel, Plate)>,
    /// The crossing's progress.
    fade_t: DragSpring,
    /// The plate the panel was drawn on last frame — what a crossing dissolves from.
    last_plate: Option<Plate>,
    /// `[overlay] card_glass` — a card's opacity over its frosted backdrop.
    card_glass: f32,
    /// `[overlay] card_relief` — how raised a card looks.
    card_relief: f32,
    /// The relief meshes, one for the panel and one for the panel being crossed from, each kept
    /// until what it was built from changes.
    reliefs: [relief::Cache; 2],
    /// The frosted backdrop, from the screenshot taken as the card first showed; dropped when the
    /// shade shuts, so every open lies on the page as it then is, and put down early where it no
    /// longer matches (a theme switch, a resize).
    backdrop: Option<Backdrop>,
    /// Frames left to wait for that screenshot; 0 when not waiting.
    shot_wait: u8,
    /// Per-request inbox; dropping it invalidates any late callback.
    pending_shot: Option<PendingShot>,
    /// Where the outgoing panel's tiles were drawn — kept apart from `tile_rects`, which the panel
    /// being pressed owns.
    leaving_tile_rects: Vec<Rect>,
    /// `[overlay] reveal` — a curtain drawn down, or a card that materialises.
    reveal: OverlayReveal,
    /// `[overlay] card_width_ratio`.
    card_ratio: f32,
    /// `[overlay] card_anchor`.
    card_anchor: CardAnchor,
    /// The side the current pull started from, latched on the frame the card first shows and
    /// kept until it has gone — a card does not change sides mid-way.
    card_side: Option<CardSide>,
    /// The card's height as last drawn while not closing: the stop while it materialises, then
    /// the pull past it. A closing card keeps the height it closed from, so the whole of the way
    /// out is a fade and a lift rather than a shrink.
    card_h: f32,
    /// The air a card keeps under its tiles at the stop — the same as above them.
    card_air: f32,
    /// The gap between quick tiles this frame (`panel::tile_gap`), kept for `tiles_bottom`.
    tile_gap: f32,
    /// **How far the card has arrived, as drawn** — the pull's share trailed by
    /// [`card::LAG_S`] on the way in, so a flick still shows the arrival.
    card_p: f32,
    /// The card is still catching the pull up — the overlay asks for frames until it has.
    card_lagging: bool,
    /// The last frame's dt, which the card's trailing arrival advances by.
    dt: f32,
    /// **The last stop worked out from a drawn frame**, kept across closes.
    ///
    /// `tile_rects` is cleared when the shade shuts (`reset_panel_state`), so reading the stop
    /// straight off it would make every *first* pull a one-step pull and only the ones that
    /// followed a full open behave — a shade that has steps every other time. The block's height
    /// only changes when the tiles or the columns do, so the remembered value is right until the
    /// next frame replaces it.
    tiles_stop: Option<f32>,
    tiles: Vec<(QuickTile, TileState, bool)>,
    tile_rects: Vec<Rect>,
    /// The integrator's placement of the tiles (rung 4).
    tile_layout: Option<ShadeTileLayout>,
    /// The integrator's drawing of each tile (rung 5).
    tile_painter: Option<ShadeTilePainter>,
    /// The integrator's drawing of the panel's ground (rung 5).
    panel_painter: Option<ShadePanelPainter>,
    /// Where each tile rests this frame, by that layout — a buffer overwritten each frame.
    tile_homes: Vec<Rect>,
    tiles_dirty: bool,
    frame: OverlayFrame,
    list_offset: f32,
    status_hidden: bool,
    status_height: f32,
    /// The operator has moved on: the scrim was tapped, or, resting at the tiles stop where there
    /// is no scrim, something outside the panel was pressed. Closed on the next `update`.
    dismissed: bool,
    /// The panel's text galley cache.
    texts: Vec<PanelText>,
    /// The buffer the gauge value strings are built in (not rebuilt each frame).
    value_buf: String,
    /// Who owns a press over the panel.
    grab: Grab,
    /// The engine's top edge swipe is in progress (`Started` … `Ended/Cancelled`).
    edge_active: bool,
    /// Last frame's notification list Rect.
    list_rect: Rect,
    /// The top notification's close hit Rect (for the regression test on its size).
    close_hit: Rect,
    /// Last frame's slider track Rect.
    slider_rect: Rect,
    /// The expanded Slider tile.
    expanded: Option<usize>,
    /// Whether a close × is drawn on a notification row (`[notify] dismiss_button`).
    dismiss_button: bool,
    /// The notification row being swiped (or springing back).
    swiping: Option<NotificationId>,
    /// Whether the row being pushed **can be opened** (whether the notification has an `action`). Without one it does not go right.
    swipe_open: bool,
    /// A confirmed action **flying out**. It is raised once the spring settles — removing it at once
    /// makes the row vanish in an instant, leaving no sight of what went.
    flying: Option<PanelAction>,
    /// The row collapsing (= the notification [`Overlay::collapse`] points at).
    collapsing: Option<NotificationId>,
    /// Where a long press was caught in this press (the engine's [`Gesture::LongPress`]).
    ///
    /// **egui's `long_touched()` cannot be used** — it only fires where there are real touch events
    /// (`Event::Touch`), so on a driver that pipes touch through as a mouse it never fires at all.
    /// The engine's long press fires on a pointer alone and is tuned by `[gesture] long_press_ms`.
    long_press: Option<Pos2>,
    /// On a press handled as a long press, the tap is swallowed on release — otherwise Wi-Fi turns
    /// off and the settings open with it. It clears on the next press.
    swallow_tap: bool,
    /// The swiped row's x offset (1:1 while dragged, springing to 0 on release).
    swipe_x: DragSpring,
    /// The **fraction of height left** on a row being removed (1 → 0). Once it has flown out
    /// sideways, the rows below rise to fill the gap while this reaches 0. It is really removed on
    /// the frame it has finished collapsing.
    collapse: DragSpring,
    /// The offset catch-up on the frame the list takes ownership (once).
    scroll_to: Option<f32>,
    /// The action settled on the frame of the release (a swipe dismissal).
    pending: Option<PanelAction>,
}

impl std::fmt::Debug for Overlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Overlay")
            .field("state", &self.shade.state())
            .field("layout", &self.layout)
            .field("tiles", &self.tiles.len())
            .field("tile_layout", &self.tile_layout.is_some())
            .field("tile_painter", &self.tile_painter.is_some())
            .field("panel_painter", &self.panel_painter.is_some())
            .field("grab", &self.grab)
            .finish_non_exhaustive()
    }
}

impl Overlay {
    /// From `[overlay]`.
    #[must_use]
    pub(crate) fn from_config(
        cfg: &OverlayConfig,
        dismiss_button: bool,
        tokens: &MotionTokens,
    ) -> Self {
        let (layout, reveal, card_anchor) = parse_names(cfg);
        let footer = footer_buttons(&cfg.footer);
        let mut shade = Shade::new(1.0, tokens);
        if reveal == OverlayReveal::Card {
            shade.set_close(Some(tokens.shade.card_close));
        }
        let split = layout == OverlayLayout::Split;
        if split && cfg.two_step {
            log::warn!(
                "[overlay] two_step is ignored under layout = \"split\" - each panel already does one thing"
            );
        }
        let footer_notes: Vec<FooterButton> = footer
            .iter()
            .copied()
            .filter(|b| *b == FooterButton::ClearAll)
            .collect();
        let footer_controls: Vec<FooterButton> = footer
            .iter()
            .copied()
            .filter(|b| *b != FooterButton::ClearAll)
            .collect();
        Self {
            shade,
            layout,
            panels: if split {
                vec![OverlayPanel::Notifications, OverlayPanel::ControlCenter]
            } else {
                vec![OverlayPanel::Shade]
            },
            dismiss_button,
            tile_ids: cfg.tiles.clone(),
            footer,
            tile_columns: usize::from(cfg.tile_columns.max(1)),
            max_height_ratio: cfg.max_height_ratio,
            peek_ms: Duration::from_millis(cfg.peek_ms),
            two_step: cfg.two_step && !split,
            split_ratio: cfg.split_ratio,
            panel: if split {
                OverlayPanel::Notifications
            } else {
                OverlayPanel::Shade
            },
            last_press: None,
            screen: Rect::NOTHING,
            room: 0.0,
            footer_notes,
            footer_controls,
            fade: None,
            fade_t: DragSpring::new(1.0, 0.0, 1.0, RubberBand::new(0.0, 0.0)),
            last_plate: None,
            card_glass: cfg.card_glass,
            card_relief: cfg.card_relief,
            reliefs: Default::default(),
            backdrop: None,
            shot_wait: 0,
            pending_shot: None,
            leaving_tile_rects: Vec::new(),
            reveal,
            card_ratio: cfg.card_width_ratio,
            card_anchor,
            card_side: None,
            card_h: 1.0,
            card_air: 0.0,
            tile_gap: 0.0,
            card_p: 0.0,
            card_lagging: false,
            dt: 0.0,
            tiles_stop: None,
            tiles: Vec::new(),
            tile_rects: Vec::new(),
            tile_layout: None,
            tile_painter: None,
            panel_painter: None,
            tile_homes: Vec::new(),
            tiles_dirty: true,
            frame: OverlayFrame::default(),
            list_offset: 0.0,
            status_hidden: false,
            status_height: 0.0,
            dismissed: false,
            texts: Vec::new(),
            value_buf: String::new(),
            grab: Grab::None,
            edge_active: false,
            list_rect: Rect::NOTHING,
            close_hit: Rect::NOTHING,
            slider_rect: Rect::NOTHING,
            expanded: None,
            swiping: None,
            swipe_open: false,
            flying: None,
            collapsing: None,
            long_press: None,
            swallow_tap: false,
            // A row swipe stops at the list's width with no rubber band (the range is matched when the drag begins).
            swipe_x: DragSpring::new(0.0, -1.0, 1.0, RubberBand::new(0.0, 0.0)),
            collapse: DragSpring::new(1.0, 0.0, 1.0, RubberBand::new(0.0, 0.0)),
            scroll_to: None,
            pending: None,
        }
    }

    /// Place the tiles with `layout` (rung 4).
    pub(crate) fn set_tile_layout(&mut self, layout: ShadeTileLayout) {
        self.tile_layout = Some(layout);
    }

    /// Draw each tile with `painter` (rung 5).
    pub(crate) fn set_tile_painter(&mut self, painter: ShadeTilePainter) {
        self.tile_painter = Some(painter);
    }

    /// Draw the panel's ground with `painter` (rung 5).
    pub(crate) fn set_panel_painter(&mut self, painter: ShadePanelPainter) {
        self.panel_painter = Some(painter);
    }

    /// The state.
    #[must_use]
    pub fn state(&self) -> OverlayState {
        self.shade.state()
    }

    /// How the shade comes into view (`[overlay] reveal`).
    #[must_use]
    pub fn reveal(&self) -> OverlayReveal {
        self.reveal
    }

    /// **The panel the shade shows** — always [`OverlayPanel::Shade`] when unified; under
    /// `split` the one the pull or tap that opened it picked, or the one its header crossed to.
    #[must_use]
    pub fn panel(&self) -> OverlayPanel {
        self.panel
    }

    /// The split panel being **crossed from**, and how far its dissolve has got (0 → 1), while it
    /// runs.
    #[must_use]
    pub fn crossing(&self) -> Option<(OverlayPanel, f32)> {
        self.fade.map(|(from, _)| (from, self.fade_t.value()))
    }

    /// The layout.
    #[must_use]
    pub fn layout(&self) -> OverlayLayout {
        self.layout
    }

    /// The panel composition.
    #[must_use]
    pub fn panels(&self) -> &[OverlayPanel] {
        &self.panels
    }

    /// Drop the frost, and every plate still drawn with it: the texture is freed with the handle,
    /// and a dissolving panel's last plate (or the one a crossing would start from) must not
    /// point at it a frame later.
    fn put_down_glass(&mut self) {
        self.backdrop = None;
        self.shot_wait = 0;
        self.pending_shot = None;
        if let Some((_, plate)) = self.fade.as_mut() {
            plate.glass = None;
        }
        if let Some(plate) = self.last_plate.as_mut() {
            plate.glass = None;
        }
    }

    /// **What the panel is drawn as this frame** — the curtain (the A1 render mapping) or the
    /// card. Sets [`OverlayFrame::panel`].
    fn plate(&mut self, screen: Rect, pointer: &Pointer, theme: &Theme, first: bool) -> Plate {
        match self.reveal {
            OverlayReveal::Curtain => self.curtain_plate(screen, theme),
            OverlayReveal::Card => self.card_plate(screen, pointer, theme, first),
        }
    }

    /// The curtain's plate: hung from the top edge and revealed by the pull.
    fn curtain_plate(&mut self, screen: Rect, theme: &Theme) -> Plate {
        let y = self.shade.y();
        let h = self.shade.height();
        // Only while the rubber band has `y > H` does the content come down with it (by the excess).
        let over = (y - h).max(0.0);
        let surface = theme.color(ColorRole::ShadeSurface);
        // The curtain: the panel's **content** is pinned to the top of the screen and only
        // the visible area is clipped to `[top, top + y]` — so the tile row is revealed from
        // the top mid-pull too.
        let content = Rect::from_min_size(
            egui::pos2(screen.min.x, screen.min.y + over),
            egui::vec2(screen.width(), h),
        );
        let visible = Rect::from_min_max(
            content.min,
            egui::pos2(content.max.x, (screen.min.y + y).min(content.max.y)),
        );
        // The panel Rect the shell and the hand-off see is **the part revealed** (below the
        // curtain is the scrim's).
        self.frame.panel = Some(visible);
        // A shade is **a surface** — a plate laid over the desktop, so its two bottom corners
        // are rounded and a boundary line goes along its bottom. A square rectangle looks
        // like "the screen is half covered" rather than "a panel has come down". **The
        // sheet's edge is where it has been revealed to, not where it would end**:
        // the fill and the closing line were once laid on the full height, so mid-pull both
        // were below the fold and the revealed edge was a bare square cut.
        //
        // `card_radius`, not `corner_radius * 1.6` written out. The 1.6 here **was**
        // `control.card_radius_ratio` copied as a literal, so a theme that retuned the ratio
        // retuned every card and left the shade behind — and the ban on multiplying a token
        // by a bare number is the rule that exists to catch it.
        let corner = panel::bottom_corners(fairing_widgets::theme::card_radius(
            &theme.metrics,
            &theme.control,
        ));
        Plate {
            content,
            visible,
            corner,
            fill: surface,
            soft: None,
            content_alpha: 1.0,
            arrival: 1.0,
            edge_line: true,
            bleed: 0.0,
            glass: None,
            relief: None,
        }
    }

    /// The card's plate: already at its place and size, arriving as the pull goes.
    fn card_plate(&mut self, screen: Rect, pointer: &Pointer, theme: &Theme, first: bool) -> Plate {
        let y = self.shade.y();
        let h = self.shade.height();
        // Only while the rubber band has `y > H` does the card come down with it (by the excess).
        let over = (y - h).max(0.0);
        let surface = theme.color(ColorRole::ShadeSurface);
        let style = CardStyle::new(&theme.metrics, theme.components.shade);
        // The side is settled on the frame the card first shows and kept until it has gone.
        // Only a finger that is still down has a side: a shade opened by a tap or by
        // `open()` rests in the middle.
        // Under `split` the panel says which side: the notifications rest on the left and
        // the controls on the right, as One UI's do.
        if first {
            self.card_side = match self.panel {
                OverlayPanel::Notifications => Some(CardSide::Left),
                OverlayPanel::ControlCenter => Some(CardSide::Right),
                OverlayPanel::Shade => pointer.origin.map(|p| CardSide::of(p.x, screen)),
            };
        }
        let cols = self.tile_columns as f32;
        // Never narrower than the tile row at one touch target a tile.
        let gap = panel::tile_gap(&theme.metrics);
        let min_w = cols * theme.metrics.touch_target + gap * (cols + 1.0) + style.extra * 2.0;
        // A split card fills the height: from under the status bar down to its
        // inset above the bottom of the content area.
        let tall = if self.layout == OverlayLayout::Split {
            (h - style.gap - style.inset).max(1.0)
        } else {
            h
        };
        let rest = card::rest(
            screen,
            tall,
            self.card_ratio,
            min_w,
            self.card_anchor,
            self.card_side,
            &style,
        );
        // The card materialises over the first stop (the tiles, or the whole height) and
        // grows past it with the finger; closing, it keeps the height it closed from, so
        // the whole of the way out is a fade and a lift rather than a shrink.
        let stop = self.shade.detent().unwrap_or(tall).max(1.0);
        let closing = matches!(
            self.shade.state(),
            OverlayState::Settling { opening: false }
        );
        if !closing {
            self.card_h = stop.max(y).min(tall);
        }
        let span = if closing { self.card_h.max(1.0) } else { stop };
        // The pull says how far it has arrived; the card trails it on the way in, so a
        // flick that covers the stop in a frame still shows the whole arrival.
        let target = (y / span).clamp(0.0, 1.0);
        self.card_p = card::follow(self.card_p, target, self.dt, theme.motion.reduce);
        self.card_lagging = self.card_p < target;
        let arrival = card::arrival(self.card_p);
        let full = rest.translate(egui::vec2(0.0, over - style.drop * arrival.rise));
        let visible = Rect::from_min_size(
            full.min,
            egui::vec2(full.width(), self.card_h.min(full.height())),
        );
        self.card_air = style.extra + theme.metrics.corner_radius * theme.components.shade.pad;
        self.tile_gap = panel::tile_gap(&theme.metrics);
        self.frame.panel = Some(visible);
        // On its frosted backdrop the card is glass: the frost comes in with
        // the card, and the plate lets `1 − card_glass` of it through. With no backdrop
        // the plate stays solid — glass over the sharp page would muddle both.
        let glass = self.backdrop.as_ref().map(|b| Glass {
            texture: b.texture.id(),
            uv: uv_in(visible, b.viewport),
            alpha: arrival.plate,
        });
        let opacity = if glass.is_some() {
            self.card_glass
        } else {
            1.0
        };
        Plate {
            content: full.shrink(style.extra),
            visible,
            corner: style.corners(),
            fill: surface.gamma_multiply(arrival.plate * opacity),
            soft: (arrival.soft > 0.0).then(|| {
                (
                    style.corner * arrival.soft,
                    card::soft_fill(surface, arrival),
                )
            }),
            content_alpha: arrival.content,
            arrival: arrival.plate,
            edge_line: false,
            // The soft edge at its widest, or the floating shadow — whichever reaches
            // further. A card floats, and a shadow clipped to its own edge is no shadow.
            bleed: style.corner.max(shadow_reach(theme)),
            glass,
            relief: Some(self.card_relief),
        }
    }

    /// This frame's long press's action, if it was over a tile. A locked tile goes nowhere — the same
    /// reason as a tap. The unlock prompt comes up only on a tap: coming up on a mere press
    /// and hold would make it easy to call up by accident.
    fn take_tile_long_press(&mut self) -> Option<PanelAction> {
        let pos = self.long_press.take()?;
        let index = self.tile_rects.iter().position(|r| r.contains(pos))?;
        let (quick, _, allowed) = self.tiles.get(index)?;
        if !*allowed {
            return None;
        }
        self.swallow_tap = true;
        Some(PanelAction::TileLongPressed {
            id: quick.id.clone(),
        })
    }
}

/// Where `rect` falls in a screenshot of `viewport`, as texture coordinates.
fn uv_in(rect: Rect, viewport: Rect) -> Rect {
    let size = viewport.size().max(Vec2::splat(1.0));
    Rect::from_min_max(
        ((rect.min - viewport.min) / size).to_pos2(),
        ((rect.max - viewport.min) / size).to_pos2(),
    )
}

/// How far the floating elevation's shadow reaches past the shape it is cast by.
fn shadow_reach(theme: &Theme) -> f32 {
    let shadow = theme
        .elevate(fairing_widgets::theme::Elevation::Floating)
        .shadow;
    let [x, y] = shadow.offset;
    f32::from(shadow.blur)
        + f32::from(shadow.spread)
        + f32::from(x.unsigned_abs().max(y.unsigned_abs()))
}

/// `[overlay] layout`, `reveal` and `card_anchor` read from their names — an unknown one warns
/// and falls back to the default.
fn parse_names(cfg: &OverlayConfig) -> (OverlayLayout, OverlayReveal, CardAnchor) {
    let layout = match cfg.layout.as_str() {
        "unified" => OverlayLayout::Unified,
        "split" => OverlayLayout::Split,
        other => {
            log::warn!("[overlay] layout = \"{other}\" is not a known layout - using unified");
            OverlayLayout::Unified
        }
    };
    let reveal = OverlayReveal::parse(&cfg.reveal).unwrap_or_else(|| {
        log::warn!(
            "[overlay] reveal = \"{}\" is not a known reveal - drawing the curtain",
            cfg.reveal
        );
        OverlayReveal::Curtain
    });
    let anchor = CardAnchor::parse(&cfg.card_anchor).unwrap_or_else(|| {
        log::warn!(
            "[overlay] card_anchor = \"{}\" is not a known anchor - using press",
            cfg.card_anchor
        );
        CardAnchor::Press
    });
    (layout, reveal, anchor)
}

/// `[overlay] footer` read from its names — an unknown one warns at start-up and is skipped.
fn footer_buttons(names: &[String]) -> Vec<FooterButton> {
    names
        .iter()
        .filter_map(|name| match name.as_str() {
            "clear_all" => Some(FooterButton::ClearAll),
            "lock" => Some(FooterButton::Lock),
            "settings" => Some(FooterButton::Settings),
            other => {
                log::warn!("[overlay] footer = \"{other}\" is not a known button - skipped");
                None
            }
        })
        .collect()
}

/// Keep the outgoing split panel **under** the panel crossed to, so where the two stand
/// in one column the new one arrives over what is left of the old. egui keeps sublayers one level
/// deep, so both are the scrim's, and the panel is raised above its sibling every frame of the
/// dissolve.
fn pin_leaving_under_panel(ctx: &egui::Context) {
    ctx.set_sublayer(
        egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(scrim::AREA_ID)),
        egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique(PANEL_LEAVING_AREA_ID),
        ),
    );
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique(PANEL_AREA_ID),
    ));
}

/// The footer buttons each panel draws.
#[derive(Clone, Copy)]
struct Footers<'a> {
    /// The unified shade's: every button `[overlay] footer` names.
    all: &'a [FooterButton],
    /// The notifications': "clear all".
    notes: &'a [FooterButton],
    /// The controls': the lock and the settings.
    controls: &'a [FooterButton],
}

impl<'a> Footers<'a> {
    fn of(self, panel: OverlayPanel) -> &'a [FooterButton] {
        match panel {
            OverlayPanel::Shade => self.all,
            OverlayPanel::Notifications => self.notes,
            OverlayPanel::ControlCenter => self.controls,
        }
    }
}

/// Paint the panel's ground: the integrator's panel painter where there is one (rung 5),
/// the built-in plate otherwise. It returns the colour the content fades into — what the painter
/// left in `ground`, or the surface colour under the built-in plate.
fn paint_ground(
    ui: &egui::Ui,
    plate: &Plate,
    theme: &Theme,
    relief: &mut relief::Cache,
    painter: Option<&mut ShadePanelPainter>,
    (panel, reveal, leaving): (OverlayPanel, OverlayReveal, bool),
) -> egui::Color32 {
    let surface = theme.color(ColorRole::ShadeSurface);
    let Some(painter) = painter else {
        paint_plate(ui, plate, theme, relief);
        return surface;
    };
    let mut ground = ShadePanelCx {
        rect: plate.visible,
        content: plate.content,
        corner: plate.corner,
        panel,
        reveal,
        alpha: plate.arrival,
        leaving,
        ground: surface,
        theme,
    };
    painter(ui.painter(), &mut ground);
    ground.ground
}

/// Paint the plate under the content: a card's soft blob while it is one, the elevation's shadow,
/// the plate, a card's rim and relief, and a curtain's closing line.
fn paint_plate(ui: &egui::Ui, plate: &Plate, theme: &Theme, relief: &mut relief::Cache) {
    let bg = ui.painter();
    if let Some((feather, fill)) = plate.soft {
        // A feathered copy of the plate, as wide as the corner at the start and gone when it
        // lands — the blur that reads as something arriving at speed.
        let soft = Shadow {
            offset: [0, 0],
            blur: feather.round().clamp(0.0, 255.0) as u8,
            spread: 0,
            color: fill,
        };
        bg.add(soft.as_shape(plate.visible, plate.corner));
    }
    let ink = theme.elevate(fairing_widgets::theme::Elevation::Floating);
    if ink.shadow != Shadow::NONE {
        let shadow = Shadow {
            color: ink.shadow.color.gamma_multiply(plate.content_alpha),
            ..ink.shadow
        };
        bg.add(shadow.as_shape(plate.visible, plate.corner));
    }
    if let Some(g) = plate.glass {
        // The frost in the card's own shape: a texture-filled rounded rect, so the backdrop is
        // cut to the corners with no mask.
        bg.add(
            egui::epaint::RectShape::filled(
                plate.visible,
                plate.corner,
                egui::Color32::WHITE.gamma_multiply(g.alpha),
            )
            .with_texture(g.texture, g.uv),
        );
    }
    bg.rect_filled(plate.visible, plate.corner, plate.fill);
    if let Some(strength) = plate.relief {
        paint_relief(ui, plate, theme, strength, relief);
    }
    if plate.edge_line {
        bg.hline(
            plate.visible.min.x..=plate.visible.max.x,
            plate.visible.max.y - 0.5,
            egui::Stroke::new(
                theme.control.stroke_hairline,
                theme.color(ColorRole::Outline),
            ),
        );
    }
}

/// **A card stands off the page**: over its plate, the floating rim where the palette
/// draws one — the theme's boundary for a floating thing on a dark palette, which the shadow
/// alone left out — and inside it the relief, the light along the top edge, the shade along the
/// bottom and the sheen. Both fade in with the content, as the shadow does.
fn paint_relief(
    ui: &egui::Ui,
    plate: &Plate,
    theme: &Theme,
    strength: f32,
    cache: &mut relief::Cache,
) {
    let bg = ui.painter();
    let rim = theme
        .elevate(fairing_widgets::theme::Elevation::Floating)
        .rim;
    if rim != egui::Stroke::NONE {
        bg.rect_stroke(
            plate.visible,
            plate.corner,
            egui::Stroke {
                color: rim.color.gamma_multiply(plate.content_alpha),
                ..rim
            },
            egui::StrokeKind::Inside,
        );
    }
    let ppp = ui.ctx().pixels_per_point();
    let mesh = cache.mesh(&relief::Slab {
        // Where egui puts the plate: a filled rect is rounded to pixels as it is tessellated.
        rect: egui::emath::GuiRounding::round_to_pixels(plate.visible, ppp),
        corner: plate.corner,
        line: theme.control.stroke_hairline,
        feather: 1.0 / ppp,
        inset: rim.width,
        relief: relief::Relief::of(theme.dark, strength).times(plate.content_alpha),
    });
    if !mesh.vertices.is_empty() {
        bg.add(egui::Shape::Mesh(mesh));
    }
}

/// Keep the panel a sublayer of the scrim (every pass). egui raises a pressed Area to the top
/// (`Area::begin`), so one tap on the scrim would put the scrim above the panel and every panel tap
/// after it would leak into the scrim (closing it) — a sublayer is put back just above its parent on
/// every `end_pass`.
fn pin_panel_above_scrim(ctx: &egui::Context) {
    ctx.set_sublayer(
        egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(scrim::AREA_ID)),
        egui::LayerId::new(egui::Order::Foreground, egui::Id::unique(PANEL_AREA_ID)),
    );
}

fn on_off(on: bool) -> TileState {
    if on {
        TileState::On
    } else {
        TileState::Off
    }
}

/// A tile with no on/off of its own: lit from [`QuickTile::lit_by`] where the declaration named a
/// key, and otherwise whatever it has always been.
fn lit(tile: &QuickTile, settings: &SettingsView, without: TileState) -> TileState {
    match tile.lit_by.as_ref() {
        Some(key) => on_off(matches!(settings.get(key), Some(SettingValue::Bool(true)))),
        None => without,
    }
}

/// What [`gauge_texts`] borrows in one go (bundled so as not to line up seven arguments).
struct GaugeTexts<'a> {
    texts: &'a mut Vec<PanelText>,
    tile: usize,
    rows: &'a [crate::overlay::tiles::Gauge],
    settings: &'a SettingsView,
    strings: &'a crate::i18n::Strings,
    buf: &'a mut String,
}

/// Fill the cache with the name and value galleys of a [`TileKind::Gauges`] row (the panel draws from borrowed data).
fn gauge_texts(
    spec: GaugeTexts<'_>,
    (label_font, value_font): (&FontId, &FontId),
    fonts: &mut FontsView<'_>,
    frame: u64,
) {
    let GaugeTexts {
        texts,
        tile,
        rows,
        settings,
        strings,
        buf,
    } = spec;
    for (r, gauge) in rows.iter().enumerate() {
        touch(
            texts,
            TextKey::GaugeLabel(tile, r),
            strings.get(&gauge.label),
            label_font,
            fonts,
            frame,
            None,
        );
        if gauge.unit.is_empty() {
            continue;
        }
        let pct = match settings.get(&gauge.key) {
            Some(SettingValue::Int(v)) => (*v).clamp(0, 100),
            _ => 0,
        };
        buf.clear();
        let _ = write!(buf, "{pct} {}", gauge.unit);
        touch(
            texts,
            TextKey::GaugeValue(tile, r),
            buf,
            value_font,
            fonts,
            frame,
            None,
        );
    }
}

/// Find or make a cache entry (allocating only for a key seen for the first time) and bring its content up to date.
fn touch(
    texts: &mut Vec<PanelText>,
    key: TextKey,
    text: &str,
    font: &FontId,
    fonts: &mut FontsView<'_>,
    frame: u64,
    max_w: Option<f32>,
) {
    let code = key.code();
    let index = texts
        .iter()
        .position(|t| t.key() == code)
        .unwrap_or_else(|| {
            texts.push(PanelText::new(key));
            texts.len() - 1
        });
    if let Some(entry) = texts.get_mut(index) {
        entry.refresh(text, font, fonts, frame, max_w);
    }
}
