//! **Drawing the recent screens yourself** — rung 5 of the override ladder.
//!
//! A card painter draws each card; a ground painter draws what the cards stand on. Either way the
//! shell keeps the recent screens: where the cards go and the carousel's drag, a tap that brings a
//! task forward, the throw that closes one, the screen on show shrinking into its card, a card's
//! split button, "Close all", and the words over and under the cards. The buttons are widgets and
//! follow the theme.
//!
//! ```no_run
//! # fn main() -> fairing::Result<()> {
//! # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
//! use fairing::workspace::{RecentCardCx, RecentsGroundCx};
//! use fairing::ColorRole;
//!
//! let shell = fairing::Shell::builder(config)
//!     // Flat cards with a coloured top edge, the title under it.
//!     .recents_card_painter(|painter: &egui::Painter, card: &mut RecentCardCx<'_>| {
//!         let theme = card.theme;
//!         let fade = |role| theme.color(role).gamma_multiply(card.alpha);
//!         painter.rect_filled(card.rect, 0.0, fade(ColorRole::SurfaceVariant));
//!         let edge = egui::Rect::from_min_size(card.rect.min, egui::vec2(card.rect.width(), 6.0));
//!         painter.rect_filled(edge, 0.0, fade(ColorRole::Primary));
//!         painter.text(
//!             card.rect.left_top() + egui::vec2(12.0, 18.0),
//!             egui::Align2::LEFT_TOP,
//!             card.title,
//!             egui::FontId::proportional(theme.metrics.type_scale.body),
//!             fade(ColorRole::OnSurface),
//!         );
//!     })
//!     // A dark ground.
//!     .recents_ground_painter(|painter: &egui::Painter, ground: &mut RecentsGroundCx<'_>| {
//!         let dark = egui::Color32::from_gray(12).gamma_multiply(ground.alpha);
//!         painter.rect_filled(ground.rect, 0.0, dark);
//!     })
//!     .build(&ctx)?;
//! # let _ = shell;
//! # Ok(())
//! # }
//! ```

use crate::icons::{IconRef, IconSet};
use crate::theme::{ColorRole, Theme};
use egui::{CornerRadius, Rect};

/// What a card painter draws one recent screen's card with (rung 5).
///
/// The painter draws the whole card inside [`rect`](Self::rect) — what the built-in card draws as
/// its floating plate, the task's icon and title in the corner, when it was used, the level it
/// needs and the icon large in the middle. It is called once for every card drawn, the task on
/// show first, and the shell draws a card's split button over it at
/// [`split_button`](Self::split_button).
pub struct RecentCardCx<'a> {
    /// The card as drawn this frame — moved up while a finger throws it away.
    pub rect: Rect,
    /// The built-in card's corners.
    pub corner: CornerRadius,
    /// The task's title, in the language on screen.
    pub title: &'a str,
    /// When the task was last used, in words: "Just now", "3 min ago".
    pub when: &'a str,
    /// The level its screen needs, where that is more than everyone has — the built-in card writes
    /// it after [`when`](Self::when).
    pub level: Option<&'a str>,
    /// The task's icon.
    pub icon: Option<&'a IconRef>,
    /// Whether this is the task that was on show: its screen shrinks into this card as the recent
    /// screens come in, and the card takes over from it at the end.
    pub current: bool,
    /// Where the shell draws the card's split button — the task can go beside the pane on show —
    /// or `None` where it has none. Leave it clear.
    pub split_button: Option<Rect>,
    /// How much of it shows: the cards fade in, the task on show's as its screen hands over to
    /// it, and a card thrown away fades as it goes up. Multiply the colours by it.
    pub alpha: f32,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl std::fmt::Debug for RecentCardCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentCardCx")
            .field("rect", &self.rect)
            .field("title", &self.title)
            .field("when", &self.when)
            .field("level", &self.level)
            .field("current", &self.current)
            .field("split_button", &self.split_button)
            .field("alpha", &self.alpha)
            .finish_non_exhaustive()
    }
}

/// The callback that draws one recent screen's card. Given through
/// [`crate::shell::ShellBuilder::recents_card_painter`].
pub type RecentCardPainter = Box<dyn FnMut(&egui::Painter, &mut RecentCardCx<'_>)>;

/// What the recent screens stand on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecentsOver {
    /// The desktop: the ground fades in over it as the cards come.
    Desktop,
    /// A task: its screen shrinks into its card in front of the ground, which is there whole from
    /// the start. A screen lifted from the bottom edge (gesture navigation) stands on it too — the
    /// recent screens it may become.
    Task,
}

/// What a ground painter draws the ground under the recent screens with (rung 5).
///
/// The painter draws what the built-in recent screens draw under the cards: a plain ground in the
/// theme's background colour over the content area. Over a task it is drawn on the desktop's layer,
/// under the screen shrinking into its card; over the desktop, over it.
pub struct RecentsGroundCx<'a> {
    /// The content area — between the bars, where the cards go.
    pub rect: Rect,
    /// What it stands on.
    pub over: RecentsOver,
    /// How much of it shows: over the desktop it fades in with the cards and out on the way back;
    /// over a task it is 1. Multiply the colours by it.
    pub alpha: f32,
    /// The theme.
    pub theme: &'a Theme,
}

impl std::fmt::Debug for RecentsGroundCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentsGroundCx")
            .field("rect", &self.rect)
            .field("over", &self.over)
            .field("alpha", &self.alpha)
            .finish_non_exhaustive()
    }
}

/// The callback that draws the ground under the recent screens. Given through
/// [`crate::shell::ShellBuilder::recents_ground_painter`].
pub type RecentsGroundPainter = Box<dyn FnMut(&egui::Painter, &mut RecentsGroundCx<'_>)>;

/// The recent screens' painters, held by the shell and lent to the workspace each frame.
#[derive(Default)]
pub(crate) struct RecentsPainters {
    pub(crate) card: Option<RecentCardPainter>,
    pub(crate) ground: Option<RecentsGroundPainter>,
}

impl std::fmt::Debug for RecentsPainters {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecentsPainters")
            .field("card", &self.card.is_some())
            .field("ground", &self.ground.is_some())
            .finish()
    }
}

/// The ground under the recent screens: a painter's where one is given, the theme's
/// background colour at `alpha` where not. The painter gets `painter` as it is and is told `alpha`.
pub(crate) fn paint_ground(
    painter: &egui::Painter,
    theme: &Theme,
    ground: Option<&mut RecentsGroundPainter>,
    rect: Rect,
    over: RecentsOver,
    alpha: f32,
) {
    if let Some(paint) = ground {
        paint(
            painter,
            &mut RecentsGroundCx {
                rect,
                over,
                alpha,
                theme,
            },
        );
    } else {
        let fill = theme.color(ColorRole::Background).gamma_multiply(alpha);
        painter.rect_filled(rect, 0.0, fill);
    }
}
