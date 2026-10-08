//! **Placing and drawing the toasts and the heads-up banner yourself** — rungs 4 and 5 of the
//! override ladder.
//!
//! A layout gets the rects **as the shell would place them** and changes the ones it wants; a
//! painter draws one card in the rect it is given. Either way the shell keeps what makes them
//! notifications: the queue and the dedup, the hold and its timer, coming in and going out, the
//! tap that dismisses a toast or opens a notification, and the swipe that puts a banner away.
//!
//! ```no_run
//! # fn main() -> fairing::Result<()> {
//! # let (ctx, config): (egui::Context, fairing::ShellConfig) = todo!();
//! use fairing::notify::{ToastCx, ToastLayoutCx};
//!
//! let shell = fairing::Shell::builder(config)
//!     // The stack in the top right corner rather than at the bottom centre.
//!     .toast_layout(|area: &ToastLayoutCx<'_>, rects: &mut [egui::Rect]| {
//!         let mut top = area.area.top() + 16.0;
//!         for rect in rects.iter_mut() {
//!             *rect = egui::Rect::from_min_size(
//!                 egui::pos2(area.area.right() - 16.0 - rect.width(), top),
//!                 rect.size(),
//!             );
//!             top += rect.height() + 8.0;
//!         }
//!     })
//!     // A plain card in the panel's own colours.
//!     .toast_painter(|ui: &mut egui::Ui, toast: &mut ToastCx<'_>| {
//!         let theme = toast.theme;
//!         ui.painter().rect_filled(toast.rect, 4.0, theme.color(fairing::ColorRole::Primary));
//!         ui.painter().text(
//!             toast.rect.center(),
//!             egui::Align2::CENTER_CENTER,
//!             &toast.toast.text,
//!             egui::FontId::proportional(theme.metrics.type_scale.body),
//!             theme.color(fairing::ColorRole::OnPrimary),
//!         );
//!     })
//!     .build(&ctx)?;
//! # let _ = shell;
//! # Ok(())
//! # }
//! ```

use super::model::{Level, NotificationId, Toast};
use crate::i18n::Strings;
use crate::icons::{IconRef, IconSet};
use crate::theme::Theme;
use egui::Rect;

/// What a toast painter draws one toast with (rung 5).
///
/// The card is the painter's whole — the background included. The shell has already faded it in
/// or out (the `Ui`'s opacity) and moved it for its entry; it takes the tap on `rect` after the
/// painter has drawn.
pub struct ToastCx<'a> {
    /// Where the shell put the toast this frame. Fill it.
    pub rect: Rect,
    /// The toast: its text, level and icon.
    pub toast: &'a Toast,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
    /// The string table, for words of your own. The toast's text is shown as it was given.
    pub strings: &'a Strings,
    height: &'a mut Option<f32>,
}

impl<'a> ToastCx<'a> {
    pub(crate) fn new(
        rect: Rect,
        toast: &'a Toast,
        theme: &'a Theme,
        icons: &'a mut IconSet,
        strings: &'a Strings,
        height: &'a mut Option<f32>,
    ) -> Self {
        Self {
            rect,
            toast,
            theme,
            icons,
            strings,
            height,
        }
    }

    /// The height this card needs. The stack is laid out at it from the next frame — never below
    /// `metrics.widget_height`, the built-in card's floor (lower that token to go smaller). Until
    /// a painter says, a card is the floor's height.
    pub fn set_height(&mut self, height: f32) {
        *self.height = Some(height);
    }
}

impl std::fmt::Debug for ToastCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastCx")
            .field("rect", &self.rect)
            .field("toast", &self.toast)
            .finish_non_exhaustive()
    }
}

/// The callback that draws one toast. Given through
/// [`crate::shell::ShellBuilder::toast_painter`].
pub type ToastPainter = Box<dyn FnMut(&mut egui::Ui, &mut ToastCx<'_>)>;

/// What a toast layout places the stack with (rung 4).
///
/// The rects come placed the built-in way — one per toast on screen, the oldest first, stacked
/// up from the bottom centre of `area` — at rest: the shell adds the entry's rise and the small
/// lift a newer toast gives the older ones on top of where the layout puts them. A rect the layout
/// empties (`Rect::NOTHING`) keeps its toast off the screen, still timed.
pub struct ToastLayoutCx<'a> {
    /// The space the toasts go in: the content between the bars, and above the keyboard while it
    /// is up — though never shorter than one toast needs, so on a short panel it reaches over the
    /// keyboard's top rows.
    pub area: Rect,
    /// The theme.
    pub theme: &'a Theme,
}

impl std::fmt::Debug for ToastLayoutCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToastLayoutCx")
            .field("area", &self.area)
            .finish_non_exhaustive()
    }
}

/// The callback that places the toasts. Given through
/// [`crate::shell::ShellBuilder::toast_layout`].
pub type ToastLayout = Box<dyn FnMut(&ToastLayoutCx<'_>, &mut [Rect])>;

/// What a heads-up painter draws the banner with (rung 5).
///
/// The banner is the painter's whole — the background included. The shell slides it in and out,
/// holds it (the timer stopping while a finger rests on it), takes its tap — which opens the
/// notification — and its swipe up, which puts it away.
pub struct HeadsUpCx<'a> {
    /// Where the shell put the banner this frame. Fill it.
    pub rect: Rect,
    /// The notification's id.
    pub id: NotificationId,
    /// Its title.
    pub title: &'a str,
    /// Its body.
    pub body: &'a str,
    /// Its icon.
    pub icon: &'a IconRef,
    /// Its level.
    pub level: Level,
    /// Its progress, where it has one (0..=1).
    pub progress: Option<f32>,
    /// Whether a finger is resting on the banner.
    pub pressed: bool,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
    /// The string table, for words of your own.
    pub strings: &'a Strings,
    height: &'a mut f32,
}

/// The parts of one banner the shell hands a painter, gathered so its constructor stays short.
#[derive(Clone, Copy)]
pub(crate) struct BannerView<'a> {
    pub(crate) id: NotificationId,
    pub(crate) title: &'a str,
    pub(crate) body: &'a str,
    pub(crate) icon: &'a IconRef,
    pub(crate) level: Level,
    pub(crate) progress: Option<f32>,
    pub(crate) pressed: bool,
}

impl<'a> HeadsUpCx<'a> {
    pub(crate) fn new(
        rect: Rect,
        banner: BannerView<'a>,
        theme: &'a Theme,
        icons: &'a mut IconSet,
        strings: &'a Strings,
        height: &'a mut f32,
    ) -> Self {
        Self {
            rect,
            id: banner.id,
            title: banner.title,
            body: banner.body,
            icon: banner.icon,
            level: banner.level,
            progress: banner.progress,
            pressed: banner.pressed,
            theme,
            icons,
            strings,
            height,
        }
    }

    /// The height this banner needs. It is drawn at it, and travels it, from the next frame —
    /// never below `metrics.heads_up_height`, the built-in banner's floor.
    pub fn set_height(&mut self, height: f32) {
        *self.height = height;
    }
}

impl std::fmt::Debug for HeadsUpCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadsUpCx")
            .field("rect", &self.rect)
            .field("id", &self.id)
            .field("title", &self.title)
            .finish_non_exhaustive()
    }
}

/// The callback that draws the heads-up banner. Given through
/// [`crate::shell::ShellBuilder::heads_up_painter`].
pub type HeadsUpPainter = Box<dyn FnMut(&mut egui::Ui, &mut HeadsUpCx<'_>)>;

/// What a heads-up layout places the banner with (rung 4).
///
/// The rect comes placed the built-in way — centred at the top of `screen`, at most 560 wide —
/// at rest. The layout moves it and may change its width; its **height stays the banner's own**
/// (its content, never below the token), and it still comes down from above its rect and leaves
/// upwards. A rect the layout leaves no width (`Rect::NOTHING`) keeps the banner off the screen,
/// still timed.
pub struct HeadsUpLayoutCx<'a> {
    /// The space the banner goes in: the screen below a status bar with a band of its own.
    pub screen: Rect,
    /// The banner's height this frame.
    pub height: f32,
    /// The theme.
    pub theme: &'a Theme,
}

impl std::fmt::Debug for HeadsUpLayoutCx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeadsUpLayoutCx")
            .field("screen", &self.screen)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// The callback that places the heads-up banner. Given through
/// [`crate::shell::ShellBuilder::heads_up_layout`].
pub type HeadsUpLayout = Box<dyn FnMut(&HeadsUpLayoutCx<'_>, &mut Rect)>;

/// A rect a layout gave back, where it has an area to draw in — `None` leaves the card out.
pub(crate) fn kept(rect: Rect) -> Option<Rect> {
    rect.is_positive().then_some(rect)
}
