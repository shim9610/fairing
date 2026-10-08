//! **Placing and drawing the shade yourself** — rungs 4 and 5 of the override ladder.
//!
//! A layout gets the quick-settings tiles **as the shell would place them**, at rest, and moves the
//! ones it wants. A tile painter draws each tile, and a panel painter draws the ground the panel's
//! content sits on. Either way the shell keeps the shade: the pull and the release, the scrim, the
//! card, the stop of a two-step shade (it follows the lowest tile), each tile's tap and long press,
//! what a tap does and who may do it, the row a slider, gauge or panel tile opens below the tiles —
//! which goes below the lowest tile, as the notification list does — the list, the footer and the
//! status row.
//!
//! ```no_run
//! # fn main() -> fairing::Result<()> {
//! # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
//! use fairing::overlay::{ShadePanelCx, ShadeTileCx, ShadeTileLayoutCx, TileState};
//! use fairing::ColorRole;
//!
//! let shell = fairing::Shell::builder(config)
//!     // The tiles in one column down the left of the panel.
//!     .shade_tile_layout(|tiles: &ShadeTileLayoutCx<'_>, rects: &mut [egui::Rect]| {
//!         let mut top = tiles.area.top();
//!         for rect in rects.iter_mut() {
//!             *rect = egui::Rect::from_min_size(
//!                 egui::pos2(tiles.area.left() + 16.0, top),
//!                 rect.size(),
//!             );
//!             top += rect.height() + 8.0;
//!         }
//!     })
//!     // Square tiles: the state is the fill, the label goes inside.
//!     .shade_tile_painter(|painter: &egui::Painter, tile: &mut ShadeTileCx<'_>| {
//!         let theme = tile.theme;
//!         let fill = if tile.lit { ColorRole::Primary } else { ColorRole::SurfaceVariant };
//!         painter.rect_filled(tile.rect, 4.0, theme.color(fill).gamma_multiply(tile.fade));
//!         painter.text(
//!             tile.rect.center(),
//!             egui::Align2::CENTER_CENTER,
//!             tile.label,
//!             egui::FontId::proportional(theme.metrics.type_scale.small),
//!             theme.color(ColorRole::OnSurface),
//!         );
//!         if let TileState::Value(v) = tile.state {
//!             let bar = egui::Rect::from_min_size(
//!                 tile.rect.left_bottom() - egui::vec2(0.0, 3.0),
//!                 egui::vec2(tile.rect.width() * v, 3.0),
//!             );
//!             painter.rect_filled(bar, 0.0, theme.color(ColorRole::Primary));
//!         }
//!     })
//!     // A flat panel in the theme's background colour, no corners.
//!     .shade_panel_painter(|painter: &egui::Painter, panel: &mut ShadePanelCx<'_>| {
//!         let ground = panel.theme.color(ColorRole::Background).gamma_multiply(panel.alpha);
//!         painter.rect_filled(panel.rect, 0.0, ground);
//!         panel.ground = ground; // the list fades into this where it overflows
//!     })
//!     .build(&ctx)?;
//! # let _ = shell;
//! # Ok(())
//! # }
//! ```

use super::tiles::{QuickTile, TileKind, TileState};
use super::{OverlayPanel, OverlayReveal};
use crate::icons::{IconRef, IconSet};
use crate::theme::Theme;
use egui::Rect;

/// What a shade tile layout places the tiles with (rung 4).
///
/// The rects come placed the built-in way — `columns` to a row, each row centred, the rows going
/// down from the top of `area` — **one per tile, in `[overlay] tiles` order** (the declared tiles
/// after them). They are the tiles at rest: an expanded tile still leaves its rect for the row
/// below, and comes back to it, but the tiles round it do not close up the gap as the built-in rows
/// do — the layout is told which tile is out ([`expanded`](Self::expanded)) and closes it up itself
/// if it wants. A rect the layout empties (`Rect::NOTHING`) leaves the tile out, neither drawn nor
/// pressed. The panel is clipped to what the shade has revealed, so a tile placed low shows when
/// the shade has come down that far.
pub struct ShadeTileLayoutCx<'a> {
    /// The panel the tiles are in, at its full height.
    pub panel: Rect,
    /// Where the built-in rows go: the panel's width, from under its head down as far as the rows
    /// reach.
    pub area: Rect,
    /// How many tiles to a row (`[overlay] tile_columns`).
    pub columns: usize,
    /// The tile that is out — its slider, gauges or panel open below the tiles — if one is.
    pub expanded: Option<usize>,
    /// The theme.
    pub theme: &'a Theme,
    tiles: &'a [(QuickTile, TileState, bool)],
}

impl<'a> ShadeTileLayoutCx<'a> {
    pub(super) fn new(
        panel: Rect,
        area: Rect,
        columns: usize,
        expanded: Option<usize>,
        theme: &'a Theme,
        tiles: &'a [(QuickTile, TileState, bool)],
    ) -> Self {
        Self {
            panel,
            area,
            columns,
            expanded,
            theme,
            tiles,
        }
    }

    /// How many tiles there are — one rect each.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    /// Whether there are no tiles.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Tile `index`'s id (`"tile.wifi"`, …).
    #[must_use]
    pub fn id(&self, index: usize) -> Option<&str> {
        self.tiles.get(index).map(|(tile, _, _)| tile.id.as_str())
    }

    /// The index of the tile with id `id`.
    #[must_use]
    pub fn index_of(&self, id: &str) -> Option<usize> {
        self.tiles.iter().position(|(tile, _, _)| tile.id == id)
    }

    /// Whether the session may use tile `index`. The shell draws one it may not with a padlock and
    /// asks to unlock when it is tapped — a layout may leave it out instead.
    #[must_use]
    pub fn is_allowed(&self, index: usize) -> bool {
        self.tiles
            .get(index)
            .is_some_and(|(_, _, allowed)| *allowed)
    }
}

impl std::fmt::Debug for ShadeTileLayoutCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShadeTileLayoutCx")
            .field("panel", &self.panel)
            .field("area", &self.area)
            .field("columns", &self.columns)
            .field("expanded", &self.expanded)
            .field("tiles", &self.tiles.len())
            .finish_non_exhaustive()
    }
}

/// The callback that places the shade's tiles. Given through
/// [`crate::shell::ShellBuilder::shade_tile_layout`].
pub type ShadeTileLayout = Box<dyn FnMut(&ShadeTileLayoutCx<'_>, &mut [Rect])>;

/// What a shade tile painter draws one quick-settings tile with (rung 5).
///
/// The painter draws the whole tile inside [`rect`](Self::rect) — what the built-in one draws as
/// a round puck, its icon, the label under it, a slider's value ring and a padlock. The shell
/// keeps the rest: where the tile goes (the built-in rows, or a [`ShadeTileLayout`]), its tap and
/// long press, what a tap does and who may do it, and the row a slider, gauge or panel tile opens.
/// It is called once for every tile drawn, in `[overlay] tiles` order.
#[allow(clippy::struct_excessive_bools)] // Four independent facts about one tile, not a state machine.
pub struct ShadeTileCx<'a> {
    /// The tile as drawn this frame. While a slider, gauge or panel tile goes out to the row it
    /// opens, the rect shrinks toward that row's icon and [`fade`](Self::fade) drops from 1 to 0;
    /// coming back, the other way.
    pub rect: Rect,
    /// Its id (`"tile.wifi"`, …).
    pub id: &'a str,
    /// Its label, in the language on screen.
    pub label: &'a str,
    /// Its icon.
    pub icon: &'a IconRef,
    /// What it is — a toggle, a slider, an action, a status, a panel or gauges.
    pub kind: &'a TileKind,
    /// Its state this frame.
    pub state: TileState,
    /// Whether the built-in tile would be drawn lit: a toggle that is on, or a tile lit by its
    /// `lit_by` key while that holds `true`.
    pub lit: bool,
    /// Whether it can be used — enabled, and its backend has the thing. The built-in tile dims one
    /// that cannot.
    pub live: bool,
    /// Whether the session may use it. The built-in tile draws a padlock on one it may not, and a
    /// tap on it asks to unlock.
    pub allowed: bool,
    /// Whether a finger is on it.
    pub pressed: bool,
    /// 1 at rest, toward 0 while the tile goes out to its row. The built-in tile fades its label
    /// with it.
    pub fade: f32,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl std::fmt::Debug for ShadeTileCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShadeTileCx")
            .field("rect", &self.rect)
            .field("id", &self.id)
            .field("state", &self.state)
            .field("lit", &self.lit)
            .field("live", &self.live)
            .field("allowed", &self.allowed)
            .field("pressed", &self.pressed)
            .field("fade", &self.fade)
            .finish_non_exhaustive()
    }
}

/// The callback that draws one quick-settings tile. Given through
/// [`crate::shell::ShellBuilder::shade_tile_painter`].
pub type ShadeTilePainter = Box<dyn FnMut(&egui::Painter, &mut ShadeTileCx<'_>)>;

/// What a shade panel painter draws the panel's ground with (rung 5).
///
/// The painter draws what the built-in panel draws under its content: the plate and its corners,
/// a card's shadow, frosted backdrop and relief, a curtain's closing line. The content — the status
/// row, the tiles (built in or a [`ShadeTilePainter`]'s), the notification list and the footer — is
/// drawn over it by the shell as before, clipped to [`rect`](Self::rect). With a panel painter a
/// card takes no screenshot of the page for its frost (`[overlay] card_glass`): the painter would
/// never see it.
pub struct ShadePanelCx<'a> {
    /// What shows of the panel this frame: the part a curtain has revealed, or the card where it
    /// is. The shell clips the content to it, and it is what a press inside the panel is.
    pub rect: Rect,
    /// The panel at its full height — where the content is laid out.
    pub content: Rect,
    /// The built-in plate's corners: a curtain's two bottom ones, a card's four.
    pub corner: egui::CornerRadius,
    /// Which panel: the one shade, or under `[overlay] layout = "split"` the notifications or the
    /// controls.
    pub panel: OverlayPanel,
    /// How it arrives: hung from the top edge, or a floating card.
    pub reveal: OverlayReveal,
    /// How much of it shows: 1 once it has arrived; less while a card arrives, and on the panel a
    /// split shade crosses away from, which fades out. Multiply the colours by it.
    pub alpha: f32,
    /// Whether this is the panel a split shade is crossing away from. It is drawn as it was, fading,
    /// and takes no presses.
    pub leaving: bool,
    /// The colour the notification list fades into at its ends where it overflows, and the disc
    /// behind a locked tile's padlock. It comes in as the theme's surface colour; set it to the
    /// colour the painter's ground is.
    pub ground: egui::Color32,
    /// The theme.
    pub theme: &'a Theme,
}

impl std::fmt::Debug for ShadePanelCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShadePanelCx")
            .field("rect", &self.rect)
            .field("content", &self.content)
            .field("panel", &self.panel)
            .field("reveal", &self.reveal)
            .field("alpha", &self.alpha)
            .field("leaving", &self.leaving)
            .field("ground", &self.ground)
            .finish_non_exhaustive()
    }
}

/// The callback that draws the shade panel's ground. Given through
/// [`crate::shell::ShellBuilder::shade_panel_painter`].
pub type ShadePanelPainter = Box<dyn FnMut(&egui::Painter, &mut ShadePanelCx<'_>)>;
