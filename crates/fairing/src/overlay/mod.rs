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

mod backdrop;
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
            leaving_tile_rects: Vec::new(),
            reveal,
            card_ratio: cfg.card_width_ratio,
            card_anchor,
            card_side: None,
            card_h: 1.0,
            card_air: 0.0,
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

    /// The driving value y (px).
    #[must_use]
    pub fn y(&self) -> f32 {
        self.shade.y()
    }

    /// The height H.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.shade.height()
    }

    /// `y / H`.
    #[must_use]
    pub fn progress(&self) -> f32 {
        self.shade.progress()
    }

    /// The scrim alpha, `0.5 × y/H` — or, on a two-step shade, `0.5 ×` the travel **past the
    /// stop**, so the tiles come down over an undimmed screen. See `Shade::scrim_progress`.
    #[doc(hidden)]
    #[must_use]
    pub fn scrim_alpha(&self) -> f32 {
        scrim::alpha(self.shade.scrim_progress())
    }

    /// The status bar icons' opacity, `1 − clamp(y/64)` (A1).
    #[doc(hidden)]
    #[must_use]
    pub fn status_bar_opacity(&self) -> f32 {
        (1.0 - (self.shade.y() / 64.0).clamp(0.0, 1.0)).clamp(0.0, 1.0)
    }

    /// Whether it is fully closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shade.is_closed()
    }

    /// Whether it is fully open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.shade.is_open()
    }

    /// Whether it is moving (the shade spring, or a notification row springing back).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.shade.is_animating()
            || self.swipe_x.is_animating()
            || self.collapse.is_animating()
            || self.card_lagging
            || self.fade.is_some()
    }

    /// Open imperatively (`LaunchAction::OpenOverlay`, an emergency gesture). A split shade opens
    /// the panel on the side of the latest press — the status bar tap that asked for it.
    pub(crate) fn open(&mut self, tokens: &MotionTokens) {
        if self.shade.is_closed() {
            self.latch_panel();
        }
        self.shade.open(tokens);
    }

    /// Close (back, the scrim, or a session downgrade).
    pub(crate) fn close(&mut self, tokens: &MotionTokens) {
        self.shade.close(tokens);
    }

    /// **Which split panel this pull or tap opens** — by where the press that started it landed,
    /// left or right of `split_ratio`. Decided once, while the shade is shut, and
    /// kept until it shuts again; with no press to go by, the last panel shown.
    fn latch_panel(&mut self) {
        self.fade = None;
        if let Some(side) = self.side_of_press() {
            self.panel = side;
        }
    }

    /// Under `split`, the panel whose side of the divide the latest press landed on.
    fn side_of_press(&self) -> Option<OverlayPanel> {
        if self.layout != OverlayLayout::Split {
            return None;
        }
        let p = self.last_press?;
        let divide = self.screen.min.x + self.screen.width() * self.split_ratio;
        Some(if p.x < divide {
            OverlayPanel::Notifications
        } else {
            OverlayPanel::ControlCenter
        })
    }

    /// Under `split`, with a panel showing: the other panel, if the latest press landed on its
    /// side — a pull or a bar tap there asks for it.
    fn other_side(&self) -> Option<OverlayPanel> {
        self.side_of_press().filter(|side| *side != self.panel)
    }

    /// **A status bar tap while the shade is open**: under `split`, a tap on the other
    /// panel's side of the bar crosses to it, and `true` says it did; anywhere else it is the
    /// shell's to close, as it always was.
    pub(crate) fn bar_tapped(&mut self, tokens: &MotionTokens) -> bool {
        if self.shade.is_closed() {
            return false;
        }
        let Some(other) = self.other_side() else {
            return false;
        };
        self.cross_to(other, tokens);
        self.shade.open(tokens);
        true
    }

    /// **Cross to the other split panel** — a pull or a status bar tap on its side while this one
    /// shows (it replaces the old header segment, so each panel holds only its own).
    ///
    /// What is left of this panel dissolves where it stands, from the plate it was last drawn on,
    /// under the new one. The new one comes in from shut, as a first open does — under the finger
    /// for a pull, on the open spring for a tap — so crossing and opening look the same. What a
    /// press on the old panel had started is dropped with it.
    fn cross_to(&mut self, to: OverlayPanel, tokens: &MotionTokens) {
        let leaving = self.panel;
        self.panel = to;
        self.card_side = match to {
            OverlayPanel::Notifications => Some(CardSide::Left),
            OverlayPanel::ControlCenter => Some(CardSide::Right),
            OverlayPanel::Shade => self.card_side,
        };
        self.swiping = None;
        self.swipe_x.snap(0.0);
        self.scroll_to = None;
        self.grab = Grab::None;
        self.card_p = 0.0;
        self.card_lagging = false;
        self.shade.reset_closed();
        self.fade = match (self.last_plate, tokens.reduce) {
            (Some(plate), false) => {
                self.fade_t.snap(0.0);
                self.fade_t.to(1.0, tokens.split_crossfade);
                Some((leaving, plate))
            }
            _ => None,
        };
    }

    /// Last frame's output.
    #[must_use]
    pub fn frame(&self) -> OverlayFrame {
        self.frame
    }

    /// The tile ids, in order.
    #[must_use]
    pub fn tile_ids(&self) -> &[String] {
        &self.tile_ids
    }

    /// Remove one quick-settings tile. `false` if it was not there.
    ///
    /// The built-in tiles come from the `[overlay] tiles` list rather than the declaration registry
    /// — which is why, while [`Shell::remove`](crate::Shell::remove) looked only at the registry and
    /// the status bar, there was no way at all to remove `tile.wifi`. `Shell::remove`
    /// now looks this far.
    pub(crate) fn remove_tile(&mut self, id: &str) -> bool {
        let before = self.tile_ids.len();
        self.tile_ids.retain(|t| t != id);
        let removed = self.tile_ids.len() != before;
        if removed {
            self.mark_tiles_dirty();
        }
        removed
    }

    /// **The state a tile is drawn in** as of the last frame — on, off, a slider value, or
    /// unavailable.
    ///
    /// Read by the integrator to check that a [`TileDecl::lit_by`] key is reaching the tile, and by
    /// the tests instead of matching colours.
    #[must_use]
    pub fn tile_state(&self, id: &str) -> Option<TileState> {
        self.tiles
            .iter()
            .find(|(t, _, _)| t.id == id)
            .map(|(_, state, _)| *state)
    }

    /// **Where the tile block ended last frame**, as a height from the shade's own top — the stop
    /// a two-step shade rests on.
    ///
    /// `None` until the panel has drawn once, which makes the shade one-step on its first pull and
    /// two-step from then on. That is the safe way round: a stop guessed at the wrong height is a
    /// shade that sticks somewhere meaningless, and there is nothing to guess from before the
    /// tiles have been laid out.
    fn tiles_bottom(&self) -> Option<f32> {
        let top = self.frame.panel?.min.y;
        // An expanded row (a slider, a panel body) is part of the tile block: the stop moves
        // down to take it in, and back up when it folds. Left out, the row opened under the
        // fold and was cut off there.
        let expanded = Some(self.slider_rect)
            .filter(Rect::is_positive)
            .map_or(f32::NEG_INFINITY, |r| r.max.y);
        let bottom = self
            .tile_rects
            .iter()
            .filter(|r| r.is_positive())
            .map(|r| r.max.y)
            .fold(expanded, f32::max);
        // The gap below the tiles is the one that sits *between* them, so the stop does not cut the
        // block off flush against the fold. A card keeps the air it has above them.
        let air = match self.reveal {
            OverlayReveal::Curtain => panel::TILE_GAP * 2.0,
            OverlayReveal::Card => self.card_air,
        };
        (bottom.is_finite() && bottom > top).then_some(bottom - top + air)
    }

    /// The tile Rects drawn last frame (so tests need not duplicate the layout arithmetic).
    #[must_use]
    pub fn tile_rect(&self, id: &str) -> Option<Rect> {
        let i = self.tiles.iter().position(|(t, _, _)| t.id == id)?;
        self.tile_rects.get(i).copied().filter(Rect::is_positive)
    }

    /// The **top notification's close hit Rect** as drawn last frame.
    ///
    /// Wider than the × drawn — at a multiple of `shade.close` it comes to 9.4 mm under a gloved
    /// finger, short of `finger_hard_mm` (10.1 mm), so the drawing is left as it is and only the
    /// place pressed widens to `touch_target` (hit size and drawn size kept apart). Exposed here so
    /// that tests need not duplicate the layout arithmetic.
    #[doc(hidden)]
    #[must_use]
    pub fn close_hit_rect(&self) -> Option<Rect> {
        Some(self.close_hit).filter(Rect::is_positive)
    }

    /// The Rect of the expanded row that is open (a Slider track, or a [`TileKind::Panel`] body).
    /// `None` where there is none.
    ///
    /// **A drag started here is not taken by the shade** — which is what makes it possible to put a
    /// slider in a row. Exposed so that tests need not duplicate the layout arithmetic.
    #[doc(hidden)]
    #[must_use]
    pub fn expanded_rect(&self) -> Option<Rect> {
        Some(self.slider_rect).filter(Rect::is_positive)
    }

    /// The notification being removed right now and **the fraction of its place left** (1 = still
    /// at full height, 0 = fully collapsed).
    ///
    /// It has a value only while the place is collapsing, after the card has flown out sideways. On
    /// the frame it reaches 0 the notification really leaves the list — until then the rows below
    /// rise to fill the gap.
    #[doc(hidden)]
    #[must_use]
    pub fn dismissing(&self) -> Option<(NotificationId, f32)> {
        self.collapsing.map(|id| (id, self.collapse.value()))
    }

    /// Last frame's notification list Rect (while the panel was visible). Where the hand-off test presses.
    #[doc(hidden)]
    #[must_use]
    pub fn list_rect(&self) -> Option<Rect> {
        Some(self.list_rect).filter(Rect::is_positive)
    }

    /// Last frame's notification list scroll offset (px).
    #[doc(hidden)]
    #[must_use]
    pub fn list_offset(&self) -> f32 {
        self.list_offset
    }

    /// The expanded Slider tile's id.
    #[must_use]
    pub fn expanded_tile(&self) -> Option<&str> {
        self.expanded
            .and_then(|i| self.tiles.get(i))
            .map(|(t, _, _)| t.id.as_str())
    }

    /// This frame's tile list (id, state, allowed).
    #[must_use]
    pub fn tiles(&self) -> &[(QuickTile, TileState, bool)] {
        &self.tiles
    }

    /// A declaration changed — rebuild the tile list next frame.
    pub(crate) fn mark_tiles_dirty(&mut self) {
        self.tiles_dirty = true;
    }

    /// The time left on the peek (for scheduling the idle repaint).
    pub(crate) fn peek_remaining(&mut self, now: Instant) -> Option<Duration> {
        self.shade.peek_remaining(now)
    }

    /// Frame stage 5: consume the engine's top edge swipe and advance. `true` if it consumed it (a
    /// lower priority then does not start). It consumes a top pull while open as well (a rubber band
    /// downwards).
    pub(crate) fn update(&mut self, input: &OverlayInput, tokens: &MotionTokens) -> bool {
        self.status_hidden = input.status_hidden;
        self.dt = input.dt;
        self.screen = input.screen;
        // A split shade's panel is picked as the pull starts, before its height is worked out.
        if let Some(Gesture::EdgeSwipe {
            edge: Edge::Top,
            phase: Phase::Started,
            ..
        }) = input.gesture
        {
            if input.allowed && self.shade.is_closed() {
                self.latch_panel();
            } else if input.allowed {
                // A pull starting on the other panel's side while one is showing brings that one
                // in under the finger.
                if let Some(other) = self.other_side() {
                    self.cross_to(other, tokens);
                }
            }
        }
        self.status_height = input.status_height;
        // H = the smaller of the layout's content height (plus the status row where the status bar is hidden) and 0.85 × the screen.
        let extra = if input.status_hidden {
            input.status_height
        } else {
            0.0
        };
        let most =
            (input.content.height() + extra).min(input.screen.height() * self.max_height_ratio);
        // **A split panel fills the height**. Each half of a wide screen is a column of
        // its own, and a panel stopping where its content did read as a box floating in it; One
        // UI's run the whole way down. It runs to the bottom of the content area — never over the
        // navigation bar — and a card keeps its inset above that. The unified shade keeps its
        // limit, which leaves the scrim a band to be tapped; a split one has the other half.
        self.room = input.content.height() + extra;
        let h = if self.layout == OverlayLayout::Split {
            self.room
        } else {
            most
        };
        self.shade.set_height(h);
        // **The stop is where the tiles ended, measured off the frame that drew them.**
        //
        // It cannot be worked out here: how tall the tile block is depends on how many tiles the
        // gates left, how many columns they fell into and whether one is expanded, and all three
        // are decided while the panel draws. So the rects the panel recorded last frame are what
        // this reads — the same one-frame-late trick `CUSTOM_WIDTH_GUESS` uses in the status bar,
        // and stable for the same reason: it only changes when the layout does. Before the first
        // draw there are no rects and the shade is one-step, which is the safe way round.
        let resting = self.shade.at_detent();
        let moved = self.tiles_bottom().map(|stop| {
            let moved = self.tiles_stop.is_some_and(|old| (old - stop).abs() > 0.5);
            self.tiles_stop = Some(stop);
            moved
        });
        self.shade
            .set_detent(if self.two_step { self.tiles_stop } else { None });
        // A shade resting on the stop follows it when the stop moves — a row expanding under
        // it, or folding — rather than staying where the stop was. A stop that moved within a
        // hair of the bottom is no stop at all (`set_detent` drops it), and then the shade opens
        // the whole way: the row needs the room more than the screen behind needs the light.
        if self.two_step && resting && moved == Some(true) {
            let target = self.shade.detent().unwrap_or_else(|| self.shade.height());
            self.shade.settle_to(target, tokens);
        }
        let mut consumed = false;
        // A long press rides this frame only — the panel matches it against the tile places as it draws.
        if let Some(Gesture::LongPress { pos }) = input.gesture {
            self.long_press = Some(pos);
        }
        if let Some(Gesture::EdgeSwipe {
            edge: Edge::Top,
            progress,
            velocity,
            phase,
        }) = input.gesture
        {
            if input.allowed {
                consumed = true;
                match phase {
                    Phase::Started => {
                        // The engine takes it. Where the panel side already caught the same press (a
                        // re-entry mid-settle) the starting value is kept and it carries on — the origin
                        // being the same, y does not jump.
                        self.edge_active = true;
                        self.grab = Grab::None;
                        if !matches!(self.shade.state(), OverlayState::Dragging { .. }) {
                            self.shade.begin_drag();
                        }
                        self.shade.drag(progress, velocity);
                    }
                    Phase::Moved => {
                        self.edge_active = true;
                        self.shade.drag(progress, velocity);
                    }
                    // Taken by a higher priority (the prompt, the lock screen): the pull
                    // commits to nothing and the shade goes back up.
                    Phase::Cancelled => {
                        self.edge_active = false;
                        self.shade.close(tokens);
                    }
                    Phase::Ended => {
                        self.edge_active = false;
                        let opened = self.shade.release(velocity, tokens);
                        // The peek: not opened, and moved ≥ half the status bar's height, leaves the status row up for a moment.
                        if !opened && input.status_hidden && progress >= input.status_height * 0.5 {
                            self.shade.peek(input.now + self.peek_ms);
                        }
                    }
                }
            }
        }
        if self.dismissed {
            self.dismissed = false;
            self.shade.close(tokens);
        }
        self.shade.tick(input.dt);
        self.swipe_x.tick(input.dt);
        self.fade_t.tick(input.dt);
        if self.fade.is_some() && !self.fade_t.is_animating() {
            self.fade = None;
        }
        self.collapse.tick(input.dt);
        consumed
    }

    /// Rebuild the tile list from the declarations and the config (on a registry-dirty frame).
    pub(crate) fn rebuild_tiles(&mut self, custom: &[TileDecl]) {
        self.tiles.clear();
        self.expanded = None;
        for id in &self.tile_ids {
            if let Some(t) = custom
                .iter()
                .find(|d| d.id() == id)
                .map(|d| d.tile().clone())
                .or_else(|| tiles::builtin(id))
            {
                self.tiles.push((t, TileState::Off, true));
            } else {
                log::warn!(
                    "[overlay] tiles: `{id}` is neither built-in nor declared - ignoring it"
                );
            }
        }
        for d in custom {
            if !self.tiles.iter().any(|(t, _, _)| t.id == d.id()) {
                self.tiles.push((d.tile().clone(), TileState::Off, true));
            }
        }
        self.tiles_dirty = false;
    }

    /// Read the tile states from the backends and the config (every frame, allocation-free — the rules are in the [`tiles`] docs).
    fn refresh_tile_states(
        &mut self,
        services: &Services,
        settings: &SettingsView,
        access: &Access,
        dark: bool,
    ) {
        for (t, state, allowed) in &mut self.tiles {
            *allowed = access.allows(t.gate_name());
            *state = match &t.kind {
                TileKind::Toggle(key) => match key.0.as_ref() {
                    keys::WIFI_ENABLED => on_off(services.wifi.enabled()),
                    keys::BLUETOOTH_ENABLED => on_off(services.bluetooth.enabled()),
                    // **The theme's truth is what is drawn right now.** Going by the settings memory
                    // alone, there is no value just after boot and it reads `Off` while the shell is
                    // already dark — drawing something that is on as off. The same reason Wi-Fi and
                    // Bluetooth ask the services.
                    keys::THEME_DARK => on_off(dark),
                    keys::DISPLAY_ROTATION_LOCK
                        if !tiles::rotation_lock_available(services.display.capabilities()) =>
                    {
                        TileState::Unavailable
                    }
                    _ => on_off(matches!(settings.get(key), Some(SettingValue::Bool(true)))),
                },
                // Neither can be summarised in one tile: a `Panel` row is drawn by the integrator, so the
                // crate knows no value for it, and a `Gauges` expansion row holds several values at once.
                // Both are drawn as on while they are open (the same rule as the other expansion tiles).
                TileKind::Panel { .. } | TileKind::Gauges { .. } => {
                    lit(t, settings, TileState::Off)
                }
                TileKind::Slider(key) => match key.0.as_ref() {
                    keys::DISPLAY_BRIGHTNESS => services
                        .display
                        .brightness()
                        .map_or(TileState::Unavailable, |b| {
                            TileState::Value(f32::from(b) / 100.0)
                        }),
                    // The audio backend is the original where it reports a volume; with the `Null`
                    // default the setting is all there is.
                    keys::AUDIO_VOLUME
                        if services
                            .audio
                            .capabilities()
                            .contains(crate::services::Capabilities::VOLUME) =>
                    {
                        services
                            .audio
                            .volume()
                            .map_or(TileState::Unavailable, |audio| {
                                TileState::Value(f32::from(audio.level) / 100.0)
                            })
                    }
                    _ => match settings.get(key) {
                        Some(SettingValue::Int(v)) => {
                            TileState::Value((*v as f32 / 100.0).clamp(0.0, 1.0))
                        }
                        Some(SettingValue::Float(v)) => {
                            TileState::Value((*v as f32 / 100.0).clamp(0.0, 1.0))
                        }
                        _ => TileState::Value(0.5),
                    },
                },
                // A tile that opens a screen is not a switch, so it has no on/off of its own — but the
                // **device** may (a conveyor running, a door open). `lit_by` is where it says so; with
                // no `lit_by` these keep the state they always had.
                TileKind::Action(_) => lit(t, settings, TileState::Off),
                TileKind::Status => lit(t, settings, TileState::Unavailable),
            };
        }
    }

    /// **The width a tile label is laid out against**, so a name too long for its tile is cut with
    /// an ellipsis instead of overflowing and being clipped at both ends — which is how "Bluetooth"
    /// drew as "luetoot", a letter gone from each end.
    ///
    /// It comes from [`panel::tile_width`], the same function the row itself uses, because the
    /// labels are laid out at the front of the frame and the row is drawn later: two copies of that
    /// arithmetic is precisely how the two came to disagree.
    fn tile_label_width(&self, panel_w: f32, parts: &CxParts<'_>) -> f32 {
        panel::tile_width(panel_w, self.tile_columns, &parts.theme.metrics)
    }

    /// Bring the panel's text galley cache up to date (only the entries whose content changed are laid out again; unused ones are dropped).
    fn refresh_texts(
        &mut self,
        ctx: &egui::Context,
        parts: &CxParts<'_>,
        center: &NotificationCenter,
        panel_w: f32,
    ) {
        let frame = parts.frame;
        let style = ctx.global_style();
        let small = egui::TextStyle::Small.resolve(&style);
        let body = egui::TextStyle::Body.resolve(&style);
        let button = egui::TextStyle::Button.resolve(&style);
        let strings = parts.strings;
        let name = parts.access.subject_name(strings);
        let settings = parts.settings;
        // The buffer holding the gauge value strings. One cell is reused so it is not rebuilt each frame
        // (`touch` does not even touch the buffer where the content is unchanged).
        let mut value_buf = std::mem::take(&mut self.value_buf);
        let tile_label_w = Some(self.tile_label_width(panel_w, parts));
        let Self { texts, tiles, .. } = self;
        ctx.fonts_mut(|fonts| {
            for (i, (t, _, _)) in tiles.iter().enumerate() {
                touch(
                    texts,
                    TextKey::TileLabel(i),
                    strings.get(&t.label),
                    &small,
                    fonts,
                    frame,
                    tile_label_w,
                );
                if let TileKind::Gauges { rows, .. } = &t.kind {
                    gauge_texts(
                        GaugeTexts {
                            texts,
                            tile: i,
                            rows,
                            settings,
                            strings,
                            buf: &mut value_buf,
                        },
                        (&body, &small),
                        fonts,
                        frame,
                    );
                }
            }
            touch(
                texts,
                TextKey::NoNotifications,
                strings.get(panel::labels::NO_NOTIFICATIONS),
                &body,
                fonts,
                frame,
                None,
            );
            touch(
                texts,
                TextKey::ClearAll,
                strings.get(panel::labels::CLEAR_ALL),
                &body,
                fonts,
                frame,
                None,
            );
            touch(
                texts,
                TextKey::Hidden,
                strings.get(panel::labels::HIDDEN_NOTIFICATION),
                &button,
                fonts,
                frame,
                None,
            );
            touch(texts, TextKey::FooterName, name, &body, fonts, frame, None);
            for item in center.iter() {
                touch(
                    texts,
                    TextKey::Title(item.id),
                    &item.title,
                    &button,
                    fonts,
                    frame,
                    None,
                );
                touch(
                    texts,
                    TextKey::Body(item.id),
                    &item.body,
                    &small,
                    fonts,
                    frame,
                    None,
                );
            }
        });
        texts.retain(|t| t.seen() == frame);
        value_buf.clear();
        self.value_buf = value_buf;
    }

    /// Decide and drive who owns a press over the panel (see the module docs). It does nothing on a frame the engine holds.
    fn update_grab(&mut self, p: &Pointer, prev_panel: Option<Rect>, tokens: &MotionTokens) {
        if self.edge_active {
            self.grab = Grab::None;
            return;
        }
        if p.pressed {
            self.grab = Grab::None;
            let Some(pos) = p.origin.or(p.pos) else {
                return;
            };
            let on_panel = prev_panel.is_some_and(|r| r.contains(pos));
            if !on_panel || self.slider_rect.contains(pos) {
                return;
            }
            match self.shade.state() {
                OverlayState::Settling { .. } => {
                    // A re-entry: freeze it where it is (v = 0) and follow the finger.
                    self.shade.begin_drag();
                    self.grab = Grab::Shade { origin_y: pos.y };
                }
                OverlayState::Open => {
                    let in_list = self.list_rect.contains(pos);
                    self.grab = Grab::Pending {
                        origin: pos,
                        shade: !in_list || self.list_offset <= 0.5,
                        row: None,
                    };
                }
                OverlayState::Closed | OverlayState::Dragging { .. } => {}
            }
            return;
        }
        if !p.down {
            self.finish_grab(p, tokens);
            return;
        }
        let Some(pos) = p.pos else {
            return;
        };
        match self.grab {
            Grab::Pending { origin, shade, row } => {
                let d = pos - origin;
                if d.length() <= tokens.slop_px {
                    return;
                }
                // While an earlier removal is still flying out or collapsing, no new row is caught —
                // there is only one `swipe_x`, and catching one would cut that animation off.
                if let (Some(id), true) = (row, d.x.abs() > d.y.abs() && self.flying.is_none()) {
                    let w = self.list_rect.width().max(1.0);
                    // **The right only opens where there is something to open.** Pushed right on a
                    // notification with no routing it looks as though something will happen and nothing
                    // does — not following at all is the more honest of the two.
                    self.swipe_x
                        .set_range(-w, if self.swipe_open { w } else { 0.0 });
                    self.swipe_x.snap(0.0);
                    self.swipe_x.begin();
                    self.swipe_x.drag(d.x, p.velocity.x);
                    self.swiping = Some(id);
                    self.grab = Grab::Row {
                        id,
                        origin_x: origin.x,
                    };
                } else if shade {
                    self.shade.begin_drag();
                    self.shade.drag(d.y, p.velocity.y);
                    self.grab = Grab::Shade { origin_y: origin.y };
                } else {
                    // The list owns it: what could not be followed during the slop is caught up in one go
                    // (this frame's delta is added by the `ScrollArea`). The offset grows opposite to the finger.
                    self.scroll_to = Some(self.list_offset + (origin.y - (pos.y - p.delta.y)));
                    self.grab = Grab::List;
                }
            }
            Grab::Shade { origin_y } => self.shade.drag(pos.y - origin_y, p.velocity.y),
            Grab::Row { origin_x, .. } => self.swipe_x.drag(pos.x - origin_x, p.velocity.x),
            Grab::None | Grab::List => {}
        }
    }

    /// The release (or the pointer disappearing).
    fn finish_grab(&mut self, p: &Pointer, tokens: &MotionTokens) {
        match self.grab {
            Grab::Shade { .. } => {
                self.shade.release(p.velocity.y, tokens);
            }
            Grab::Row { id, .. } => {
                // Confirmed: at least a third of the width, or a fling in the same direction.
                // **The direction decides the meaning** — left throws it away, right opens the screen
                // the notification points at.
                let w = self.list_rect.width().max(1.0);
                let x = self.swipe_x.value();
                let rule = ReleaseRule {
                    snap_ratio: 1.0 / 3.0,
                    fling: tokens.fling_px_s,
                };
                let confirmed =
                    x.abs() > 0.0 && rule.confirm(x.abs() / w, p.velocity.x * x.signum());
                match (confirmed, x < 0.0) {
                    // Left = remove. It is removed **after being flown out** — that is what makes what went visible.
                    (true, true) => {
                        self.flying = Some(PanelAction::Dismiss(id));
                        if tokens.reduce {
                            self.pending = self.flying.take();
                            self.swiping = None;
                            self.swipe_x.snap(0.0);
                        } else {
                            self.swipe_x.release(-w, tokens.shade.spring);
                        }
                    }
                    // Right = open. The shade is about to go up, so the row is put back in place.
                    (true, false) if self.swipe_open => {
                        self.pending = Some(PanelAction::NotificationTapped(id));
                        self.swiping = None;
                        self.swipe_x.snap(0.0);
                    }
                    _ if tokens.reduce => self.swipe_x.snap(0.0),
                    _ => self.swipe_x.release(0.0, tokens.shade.spring),
                }
            }
            Grab::None | Grab::Pending { .. } | Grab::List => {}
        }
        self.grab = Grab::None;
    }

    /// Clear the swipe state on a row whose removal has taken effect (it is no longer in the list) or
    /// whose return spring has finished.
    ///
    /// Removal is **two steps**: it flies out sideways ([`Overlay::flying`]) and then collapses the
    /// place it left down to 0 ([`Overlay::collapse`]). Only on the frame the second step finishes is
    /// `Dismiss` raised and the notification really out of the list.
    fn settle_swipe(&mut self, center: &NotificationCenter, tokens: &MotionTokens) {
        // The row flying out has stopped — now it collapses vertically.
        if self.flying.is_some() && !self.swipe_x.is_animating() {
            if self.collapsing.is_none() && !tokens.reduce {
                self.collapsing = self.swiping;
                self.collapse.snap(1.0);
                self.collapse.to(0.0, tokens.toast_out);
                return;
            }
            if self.collapse.is_animating() {
                return;
            }
            self.pending = self.flying.take();
            self.swiping = None;
            self.collapsing = None;
            self.collapse.snap(1.0);
            self.swipe_x.snap(0.0);
            return;
        }
        if self.flying.is_some() {
            return;
        }
        let Some(id) = self.swiping else {
            return;
        };
        let settled = !matches!(self.grab, Grab::Row { .. })
            && !self.swipe_x.is_animating()
            && self.swipe_x.value().abs() < 0.5;
        if center.get(id).is_none() || settled {
            self.swiping = None;
            self.swipe_x.snap(0.0);
        }
    }

    /// A frame closed with no panel: clear the panel interaction state (a shade grab in progress is
    /// kept — its release is handled by [`Overlay::update_grab`]).
    fn reset_panel_state(&mut self) {
        self.tile_rects.clear();
        self.card_p = 0.0;
        self.card_lagging = false;
        self.fade = None;
        self.last_plate = None;
        self.backdrop = None;
        self.shot_wait = 0;
        self.expanded = None;
        self.swiping = None;
        self.swipe_x.snap(0.0);
        self.scroll_to = None;
        self.pending = None;
        self.flying = None;
        self.collapsing = None;
        self.collapse.snap(1.0);
        self.long_press = None;
        self.swallow_tap = false;
        self.list_rect = Rect::NOTHING;
        self.close_hit = Rect::NOTHING;
        self.slider_rect = Rect::NOTHING;
    }

    /// Stage 11: the scrim plus the panel. Closed, it draws nothing (it only reports the peek row).
    #[expect(
        clippy::too_many_lines,
        reason = "it ties the scrim, the panel and the layer order together inside one frame"
    )]
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        screen: Rect,
        center: &NotificationCenter,
        custom: &[TileDecl],
    ) -> Option<OverlayAction> {
        if self.tiles_dirty {
            self.rebuild_tiles(custom);
        }
        let tokens = parts.theme.motion;
        let now = parts.now;
        let pointer = ctx.input(|i| Pointer {
            pressed: i.pointer.primary_pressed(),
            down: i.pointer.primary_down(),
            pos: i.pointer.interact_pos(),
            origin: crate::drag::press_point(i),
            velocity: i.pointer.velocity(),
            delta: i.pointer.delta(),
        });
        if pointer.pressed {
            if let Some(p) = pointer.origin.or(pointer.pos) {
                self.last_press = Some(p);
            }
        }
        let prev = self.frame;
        self.frame = OverlayFrame::default();
        // The finger over the panel (after the engine's stage 5). The release of a drag caught while closed is handled here too.
        self.update_grab(&pointer, prev.panel, &tokens);
        // **At the stop, a press anywhere else closes it**. There is no scrim at the
        // stop — that is the point of stopping there — so nothing was there to take the tap
        // that closes a full shade, and a shade pulled down to toggle Wi-Fi stayed over the top of
        // the screen through everything the hand did next (user report). The press is not taken:
        // it lands on the page as it would have, and the shade folds away behind it.
        if self.two_step && self.shade.at_detent() && pointer.pressed && !self.edge_active {
            let on_panel = pointer
                .origin
                .or(pointer.pos)
                .is_some_and(|p| prev.panel.is_some_and(|r| r.contains(p)));
            if !on_panel {
                self.dismissed = true;
            }
        }
        if self.shade.is_peeking(now) && self.shade.y() <= 0.5 {
            self.frame.peek = Some(Rect::from_min_size(
                screen.min,
                egui::vec2(screen.width(), self.status_height),
            ));
        }
        if self.shade.y() <= 0.5 {
            // A removal the operator already confirmed is not lost to the shade closing over
            // its animation: what was flying out (or done and waiting) is raised now.
            let owed = self.pending.take().or_else(|| self.flying.take());
            self.reset_panel_state();
            return owed;
        }
        self.settle_swipe(center, &tokens);
        self.refresh_tile_states(
            parts.services,
            parts.settings,
            parts.access,
            parts.theme.dark,
        );
        self.refresh_texts(ctx, parts, center, screen.width());
        // The scrim — or, for a card, only its catch for the tap outside: a card lies on
        // an undimmed page, and a tap beside it closes it without a scrim to take it.
        let tapped_outside = match self.reveal {
            OverlayReveal::Curtain => scrim::show(ctx, screen, self.scrim_alpha(), parts.theme),
            OverlayReveal::Card => !self.shade.at_detent() && scrim::catch(ctx, screen),
        };
        if tapped_outside {
            self.dismissed = true;
        }
        self.frost_backdrop(
            ctx,
            prev.panel.is_none(),
            parts.theme.color(ColorRole::ShadeSurface),
        );
        let plate = self.plate(screen, &pointer, parts.theme, prev.panel.is_none());
        self.last_plate = Some(plate);
        // **Crossing between the split panels**: the panel crossed to comes in as a
        // first open does, and what is left of the other dissolves where it stood, under it.
        let fading = self
            .fade
            .map(|(from, from_plate)| (from, from_plate, self.fade_t.value()));
        let status_row = self.status_hidden.then_some(self.status_height);
        if let Some(sh) = status_row {
            self.frame.status_row = Some(Rect::from_min_size(
                plate.content.min,
                egui::vec2(plate.content.width(), sh),
            ));
        }
        // The list drag-scrolls only on a gesture the list owns (or with nothing pressed). While the
        // engine, the shade or a row owns it, it is `Never` and the `ScrollArea` adds no delta.
        let drag_scroll = !self.edge_active && matches!(self.grab, Grab::None | Grab::List);
        let scroll_to = self.scroll_to.take();
        let swiping = self.swiping.map(|id| (id, self.swipe_x.value()));
        let collapsing = self.collapsing.map(|id| (id, self.collapse.value()));
        let Self {
            tiles,
            tile_rects,
            leaving_tile_rects,
            tile_layout,
            tile_painter,
            panel_painter,
            reveal,
            tile_homes,
            tile_columns,
            texts,
            expanded,
            dismiss_button,
            swallow_tap,
            footer,
            footer_notes,
            footer_controls,
            panel: shown,
            reliefs,
            ..
        } = self;
        let [panel_relief, leaving_relief] = reliefs;
        let reveal = *reveal;
        let footers = Footers {
            all: footer,
            notes: footer_notes,
            controls: footer_controls,
        };
        let row = TileRow {
            tiles,
            columns: *tile_columns,
            texts,
            expanded: *expanded,
            panels: custom,
            dismiss_button: *dismiss_button,
            footer: footers.of(*shown),
            swiping,
            collapsing,
            swallow_tap: *swallow_tap,
            drag_scroll,
            scroll_to,
            panel: *shown,
        };
        let out = egui::Area::new(egui::Id::new(PANEL_AREA_ID))
            .order(egui::Order::Foreground)
            .fixed_pos(plate.visible.min)
            .default_size(plate.visible.size())
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                // The clip = the part shown, plus the room a card's soft edge feathers into. The
                // content is laid out into `plate.content` (the full height), so what is not yet
                // revealed below (the footer and the handle's places) is neither drawn nor
                // pressable (egui's hit Rect is the intersection with the clip).
                // Never above the overlay's own screen: a card on its way down comes out from
                // under a visible status bar rather than over it, so the clock and the alarms stay
                // readable through the whole arrival.
                ui.set_clip_rect(plate.visible.expand(plate.bleed).intersect(screen));
                // The Area's size = the part shown. Otherwise egui records this layer as small (or as
                // large as below the curtain) and `layer_id_at(the press point)` picks the scrim (a panel
                // tap leaking into the scrim) or the panel eats a tap below the curtain.
                ui.set_min_size(plate.visible.size());
                let ground = paint_ground(
                    ui,
                    &plate,
                    parts.theme,
                    panel_relief,
                    panel_painter.as_mut(),
                    (*shown, reveal, false),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(plate.content));
                child.set_clip_rect(plate.visible.intersect(screen));
                child.set_opacity(plate.content_alpha);
                panel::show(
                    &mut child,
                    plate.content,
                    parts,
                    &row,
                    center,
                    TileSpots {
                        drawn: tile_rects,
                        layout: tile_layout.as_mut(),
                        homes: tile_homes,
                        brush: TileBrush {
                            painter: tile_painter.as_mut(),
                            ground,
                        },
                    },
                    status_row,
                )
            })
            .inner;
        pin_panel_above_scrim(ctx);
        if let Some((from, from_plate, t)) = fading {
            // What is left of the panel being crossed from: drawn as it was, fading, and taking no
            // presses — the panel crossed to has them from the frame it was asked for.
            let leaving = Plate {
                fill: from_plate.fill.gamma_multiply(1.0 - t),
                soft: None,
                content_alpha: from_plate.content_alpha * (1.0 - t),
                arrival: from_plate.arrival * (1.0 - t),
                glass: from_plate.glass.map(|g| Glass {
                    alpha: g.alpha * (1.0 - t),
                    ..g
                }),
                ..from_plate
            };
            let old_row = TileRow {
                footer: footers.of(from),
                swiping: None,
                collapsing: None,
                drag_scroll: false,
                scroll_to: None,
                panel: from,
                ..row
            };
            egui::Area::new(egui::Id::new(PANEL_LEAVING_AREA_ID))
                .order(egui::Order::Foreground)
                .fixed_pos(leaving.visible.min)
                .default_size(leaving.visible.size())
                .constrain(false)
                .fade_in(false)
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_clip_rect(leaving.visible.expand(leaving.bleed).intersect(screen));
                    let ground = paint_ground(
                        ui,
                        &leaving,
                        parts.theme,
                        leaving_relief,
                        panel_painter.as_mut(),
                        (from, reveal, true),
                    );
                    let mut child =
                        ui.new_child(egui::UiBuilder::new().max_rect(from_plate.content));
                    child.set_clip_rect(leaving.visible.intersect(screen));
                    child.set_opacity(leaving.content_alpha);
                    let _ = panel::show(
                        &mut child,
                        from_plate.content,
                        parts,
                        &old_row,
                        center,
                        TileSpots {
                            drawn: leaving_tile_rects,
                            layout: tile_layout.as_mut(),
                            homes: tile_homes,
                            brush: TileBrush {
                                painter: tile_painter.as_mut(),
                                ground,
                            },
                        },
                        status_row,
                    );
                });
            pin_leaving_under_panel(ctx);
        } else if self.layout == OverlayLayout::Split {
            // Kept up, empty, while a split shade is open: egui lays a new `Area` out unseen on
            // its first frame, and the outgoing panel vanishing for that frame read as a blink
            // at the start of every crossing.
            egui::Area::new(egui::Id::new(PANEL_LEAVING_AREA_ID))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .constrain(false)
                .fade_in(false)
                .interactable(false)
                .show(ctx, |_| {});
        }
        self.list_offset = out.list_offset;
        self.list_rect = out.list_rect;
        self.close_hit = out.close_hit;
        self.slider_rect = out.slider_rect;
        self.expanded = out.expanded;
        // A long press is matched against **the tile places drawn this frame**. It comes while the finger
        // is still down, so the screen changes before the release and "what I pressed opened" is felt in
        // the hand.
        let long_press = self.take_tile_long_press();
        if pointer.pressed {
            self.swallow_tap = false;
            if let Grab::Pending { row, .. } = &mut self.grab {
                *row = out.pressed_row;
            }
            // **Whether it can be pushed right to open** — only where that notification carries somewhere
            // to go. Settled once on the frame of the press and used through that whole drag.
            self.swipe_open = out
                .pressed_row
                .and_then(|id| center.get(id))
                .is_some_and(|n| n.action.is_some());
        }
        // **What the × removes goes out the same way as a swipe.** A button has no finger velocity, so it
        // is pushed left by a tween rather than a spring (`toast_out` — the motion of a notification
        // leaving). On the frame it is all the way out, `settle_swipe` raises the `Dismiss`.
        //
        // **Only what the panel raised this frame** is intercepted. `self.pending` is the result the swipe
        // already flew out and handed over, and catching it again here would fly the same row out twice.
        if let Some(PanelAction::Dismiss(id)) = out.action {
            if self.flying.is_none() && !tokens.reduce {
                let w = self.list_rect.width().max(1.0);
                self.swiping = Some(id);
                self.swipe_x.set_range(-w, 0.0);
                self.swipe_x.snap(0.0);
                self.swipe_x.to(-w, tokens.toast_out);
                self.flying = Some(PanelAction::Dismiss(id));
                return None;
            }
        }
        long_press.or(out.action).or_else(|| self.pending.take())
    }

    /// **The card's frosted backdrop**: asked of the runner on a card's first visible
    /// frame, frosted and uploaded when it answers — a frame or two later, while the card is still
    /// arriving — and kept until the shade shuts. A runner that never answers is waited on for
    /// [`SHOT_PATIENCE`] frames, and the card stays solid.
    ///
    /// **A frost that no longer matches is put down**, and the card goes solid until it next
    /// opens: the theme switched under it (the dark mode tile is on the controls card, and a dark
    /// page behind a frost of the light one washes the card out) or the window resized (the frost
    /// no longer lines up). It is not taken again — the card is over the page by then, and a
    /// second screenshot would frost the card into its own backdrop. A theme switch repaints
    /// everything anyway, so the card going solid with it does not show as a change of its own.
    ///
    /// **A panel painter takes the frost's place**: it draws the ground and is handed no
    /// backdrop, so with one the screenshot is never asked for — frosting it would be work for
    /// nothing.
    fn frost_backdrop(&mut self, ctx: &egui::Context, first: bool, surface: egui::Color32) {
        if self.reveal != OverlayReveal::Card
            || self.card_glass >= 1.0
            || self.panel_painter.is_some()
        {
            return;
        }
        let viewport = ctx.viewport_rect();
        if self
            .backdrop
            .as_ref()
            .is_some_and(|b| b.surface != surface || b.viewport != viewport)
        {
            self.put_down_glass();
            return;
        }
        if first && self.backdrop.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            self.shot_wait = SHOT_PATIENCE;
            return;
        }
        if self.shot_wait == 0 || self.backdrop.is_some() {
            return;
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
        });
        match shot {
            Some(image) => {
                let frosted = backdrop::frost(&image);
                let texture = ctx.load_texture(
                    "fairing.overlay.backdrop",
                    frosted,
                    egui::TextureOptions::LINEAR,
                );
                self.backdrop = Some(Backdrop {
                    texture,
                    viewport,
                    surface,
                });
                self.shot_wait = 0;
            }
            None => self.shot_wait -= 1,
        }
    }

    /// Drop the frost, and every plate still drawn with it: the texture is freed with the handle,
    /// and a dissolving panel's last plate (or the one a crossing would start from) must not
    /// point at it a frame later.
    fn put_down_glass(&mut self) {
        self.backdrop = None;
        self.shot_wait = 0;
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
        let min_w =
            cols * theme.metrics.touch_target + panel::TILE_GAP * (cols + 1.0) + style.extra * 2.0;
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
        egui::LayerId::new(egui::Order::Foreground, egui::Id::new(scrim::AREA_ID)),
        egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new(PANEL_LEAVING_AREA_ID),
        ),
    );
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new(PANEL_AREA_ID),
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
            egui::Stroke::new(1.0, theme.color(ColorRole::Outline)),
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
        egui::LayerId::new(egui::Order::Foreground, egui::Id::new(scrim::AREA_ID)),
        egui::LayerId::new(egui::Order::Foreground, egui::Id::new(PANEL_AREA_ID)),
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
