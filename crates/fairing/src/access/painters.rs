//! **Drawing the lock screen and the unlock prompt yourself** — rung 5 of the override ladder.
//!
//! A lock screen painter draws the lock screen's ground and its clock; an unlock prompt painter
//! draws the prompt's backdrop and the card the way in sits on. Either way the shell keeps the way
//! in — the top line, the method tabs, the keypad, the dots or the fields, the way out — and draws
//! it over what the painter drew. It keeps what is typed, the authenticator and what it answers,
//! the lockout, the shake, and when the modal comes and goes. The keys and the buttons are widgets
//! and follow the theme.
//!
//! ```no_run
//! # fn main() -> fairing::Result<()> {
//! # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
//! use fairing::access::{LockScreenCx, PromptPiece, UnlockPromptCx};
//! use fairing::ColorRole;
//!
//! let shell = fairing::Shell::builder(config)
//!     // A plain ground and the time, large, in the accent colour.
//!     .lock_screen_painter(|painter: &egui::Painter, lock: &mut LockScreenCx<'_>| {
//!         let theme = lock.theme;
//!         let ground = theme.color(ColorRole::Background).gamma_multiply(lock.alpha);
//!         painter.rect_filled(lock.screen, 0.0, ground);
//!         painter.text(
//!             lock.clock.center(),
//!             egui::Align2::CENTER_CENTER,
//!             lock.time,
//!             egui::FontId::proportional(theme.metrics.type_scale.heading * 4.0),
//!             theme.color(ColorRole::Primary).gamma_multiply(lock.alpha),
//!         );
//!     })
//!     // A dark veil, and a square card with a hairline round it.
//!     .unlock_prompt_painter(|painter: &egui::Painter, prompt: &mut UnlockPromptCx<'_>| {
//!         let theme = prompt.theme;
//!         match prompt.piece {
//!             PromptPiece::Backdrop => {
//!                 let veil = egui::Color32::from_black_alpha(200).gamma_multiply(prompt.alpha);
//!                 painter.rect_filled(prompt.rect, 0.0, veil);
//!             }
//!             PromptPiece::Card => {
//!                 let fill = theme.color(ColorRole::Surface).gamma_multiply(prompt.alpha);
//!                 painter.rect_filled(prompt.rect, 0.0, fill);
//!                 let line = theme.color(ColorRole::Outline).gamma_multiply(prompt.alpha);
//!                 painter.rect_stroke(
//!                     prompt.rect,
//!                     0.0,
//!                     egui::Stroke::new(1.0, line),
//!                     egui::StrokeKind::Inside,
//!                 );
//!             }
//!             _ => {}
//!         }
//!     })
//!     .build(&ctx)?;
//! # let _ = shell;
//! # Ok(())
//! # }
//! ```

use crate::icons::IconSet;
use crate::theme::Theme;
use egui::{CornerRadius, Rect};

/// What a lock screen painter draws the lock screen with (rung 5).
///
/// The painter draws what the built-in lock screen draws under the way in: the ground over the
/// whole screen, and the time and the date. The shell draws the way in over it — the top line,
/// the tabs, the keypad or the dots, Continue where it is allowed — centred in
/// [`room`](Self::room). As the lock screen leaves it grows a little and fades; the growing is
/// the shell's (the layer is scaled), the fading is [`alpha`](Self::alpha).
pub struct LockScreenCx<'a> {
    /// The whole screen: the ground. Every press that misses the way in is the shell's, and does
    /// nothing.
    pub screen: Rect,
    /// Where the built-in clock and date go: above the way in on a tall panel, beside it on a wide
    /// one.
    pub clock: Rect,
    /// Where the way in goes — the shell centres its column in here.
    pub room: Rect,
    /// The time, as the status bar's clock writes it (the owner's 12- or 24-hour switch
    /// included).
    pub time: &'a str,
    /// The date, in the language's own order.
    pub date: &'a str,
    /// How much of it shows: 0 to 1 as it comes in, back to 0 as it leaves. Multiply the colours
    /// by it.
    pub alpha: f32,
    /// Whether it is leaving — unlocked, or continued past.
    pub leaving: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

impl std::fmt::Debug for LockScreenCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockScreenCx")
            .field("screen", &self.screen)
            .field("clock", &self.clock)
            .field("room", &self.room)
            .field("time", &self.time)
            .field("date", &self.date)
            .field("alpha", &self.alpha)
            .field("leaving", &self.leaving)
            .finish_non_exhaustive()
    }
}

/// The callback that draws the lock screen. Given through
/// [`crate::shell::ShellBuilder::lock_screen_painter`].
pub type LockScreenPainter = Box<dyn FnMut(&egui::Painter, &mut LockScreenCx<'_>)>;

/// Which piece of the unlock prompt a painter is drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PromptPiece {
    /// The backdrop over the whole screen — the built-in one is the theme's scrim.
    Backdrop,
    /// The card the way in sits on — the built-in one is a floating plate in the surface colour.
    Card,
}

/// What an unlock prompt painter draws one piece of the prompt with (rung 5).
///
/// The painter is called twice a frame: for the [`Backdrop`](PromptPiece::Backdrop) over the
/// screen, and for the [`Card`](PromptPiece::Card) under the way in. The card is drawn on a layer of
/// its own, which the shell scales — in from 0.98, and down whole on a screen too short for the
/// keypad — and shakes at a wrong PIN. A card painter draws at [`rect`](Self::rect), and all of
/// that happens to what it drew, as it does to the way in.
pub struct UnlockPromptCx<'a> {
    /// Which piece.
    pub piece: PromptPiece,
    /// The piece's rect: the screen for the backdrop; for the card, the card in its layer — the
    /// layer's scale and shake carry it across the glass with the way in.
    pub rect: Rect,
    /// The built-in card's corners (none for the backdrop).
    pub corner: CornerRadius,
    /// How much of the piece shows: the backdrop's and the card's own fade in and out. Multiply
    /// the colours by it.
    pub alpha: f32,
    /// Whether the prompt is leaving — answered, or cancelled.
    pub leaving: bool,
    /// The theme.
    pub theme: &'a Theme,
}

impl std::fmt::Debug for UnlockPromptCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockPromptCx")
            .field("piece", &self.piece)
            .field("rect", &self.rect)
            .field("alpha", &self.alpha)
            .field("leaving", &self.leaving)
            .finish_non_exhaustive()
    }
}

/// The callback that draws the unlock prompt's backdrop and card. Given through
/// [`crate::shell::ShellBuilder::unlock_prompt_painter`].
pub type UnlockPromptPainter = Box<dyn FnMut(&egui::Painter, &mut UnlockPromptCx<'_>)>;
