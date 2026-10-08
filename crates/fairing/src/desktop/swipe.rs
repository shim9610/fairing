//! The desktop page-swipe driving value (A4). A pure state machine — the rendering is in
//! `desktop/mod.rs`.
//!
//! - The driving value is a real page position `pos ∈ [0, N−1]`. `pos = pos_start − dx / W`.
//! - Beyond either end, the excess × 0.3, up to 0.15 of a page (the rubber band).
//! - On release: `|v_x| > 600` moves one page that way (one at most), otherwise `round(pos)`.
//!   Spring k 300 · c 35, with an initial velocity of `−v_x / W`.
//! - Rendering: only the two pages `floor(pos)` and `ceil(pos)`, each at x = `(i − pos) × W`,
//!   clipped by a Pane. The indicator dots cross-fade by `1 − |i − pos|`. A pressed icon is
//!   cancelled once the slop is exceeded.
//!
//! The wiring is [`DesktopView::ui`](crate::desktop::DesktopView::ui)'s: a horizontal drag over
//! the grid (past the slop, and more horizontal than vertical) → [`PageSwipe::drag`], the release
//! → [`PageSwipe::release`] (or straight to [`PageSwipe::snap`] under `reduce`), the two pages
//! drawn at [`PageSwipe::visible`]'s x offsets, and the indicator at [`PageSwipe::dot_weight`].
//! The width `W` is the grid's width.

use crate::motion::{DragSpring, RubberBand, Spring};
use crate::theme::PageTokens;

/// The page-swipe state.
///
/// **Inside, the driving value is in px.** [`crate::motion::Animated`]'s spring settles on
/// absolute figures (`SETTLE_DISTANCE` 0.5 · `SETTLE_VELOCITY` 10), so running it in 0..N−1 page
/// units settles it on **the first tick after the release** and the spring is never seen. What it
/// hands out, [`PageSwipe::pos`], is still in page units (the A4 contract).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageSwipe {
    /// The driving value in px. `value() = pos × width`.
    pos: DragSpring,
    pages: usize,
    width: f32,
    /// The rubber band (in page units — `max × width` to use it in px).
    rubber: RubberBand,
    /// The spring of the last release or move, for one under way to carry on across a width
    /// change.
    spring: Spring,
}

/// A page-unit rubber band in px (the excess factor is dimensionless; only the cap is multiplied by the width).
fn band_px(rubber: RubberBand, width: f32) -> RubberBand {
    RubberBand::new(rubber.factor, rubber.max * width)
}

impl PageSwipe {
    /// `pages` pages, starting at page `page`.
    ///
    /// The rubber band taken here is a **bootstrap** — while the shell is running,
    /// `Self::set_rubber` swaps in the current theme's value every frame.
    #[must_use]
    pub fn new(pages: usize, page: usize, tokens: &PageTokens) -> Self {
        let max = pages.saturating_sub(1) as f32;
        Self {
            pos: DragSpring::new((page as f32).min(max), 0.0, max, tokens.rubber),
            pages: pages.max(1),
            width: 1.0,
            rubber: tokens.rubber,
            spring: tokens.spring,
        }
    }

    /// Swap the rubber band for the current theme's value. **Call it every frame** — holding on to
    /// what was handed in at construction leaves the pull-back resistance at the end on its
    /// default however the manufacturer changes `[motion] page.rubber`. The spring and the fling
    /// take their tokens as they go, in [`Self::release`] and [`Self::go_to`]; this axis was the
    /// one with no refresh.
    ///
    /// An unchanged value does nothing — so that a drag in progress is not disturbed.
    pub(crate) fn set_rubber(&mut self, rubber: RubberBand) {
        if self.rubber == rubber {
            return;
        }
        self.rubber = rubber;
        self.pos.set_rubber(band_px(rubber, self.width));
    }

    /// Refresh the page count and the width (after a rebuild or a layout). Calling it every frame
    /// does not shake the value — the px driving value is re-anchored **only on a frame where the
    /// width actually changed** (a rare event, like a screen rotation or a split). A drag in
    /// progress then stops where it is; a spring in progress carries on to its page at the new
    /// width, rather than leaving the desktop parked between two pages.
    pub fn configure(&mut self, pages: usize, width: f32) {
        let width = width.max(1.0);
        self.pages = pages.max(1);
        let max = self.pages.saturating_sub(1) as f32;
        if (width - self.width).abs() > 0.5 {
            let page_pos = self.pos.value() / self.width;
            let heading = (self.pos.is_animating() && !self.pos.is_dragging()).then(|| {
                (
                    (self.pos.target() / self.width).clamp(0.0, max),
                    self.pos.velocity() / self.width,
                )
            });
            self.pos = DragSpring::new(
                page_pos * width,
                0.0,
                max * width,
                band_px(self.rubber, width),
            );
            self.width = width;
            if let Some((page, velocity)) = heading {
                // Through a zero-length drag, so the spring starts with the velocity it had.
                self.pos.begin();
                self.pos.drag(0.0, velocity * width);
                self.pos.release(page * width, self.spring);
            }
        } else {
            self.pos.set_range(0.0, max * self.width);
        }
    }

    /// A drag begins.
    pub fn begin(&mut self) {
        self.pos.begin();
    }

    /// Mid-drag: `dx` = the horizontal movement from the press point (px), `vx` = the finger's horizontal velocity (px/s).
    pub fn drag(&mut self, dx: f32, vx: f32) {
        self.pos.drag(-dx, -vx);
    }

    /// The release (the A4 release rules). It hands back the target page.
    pub fn release(&mut self, vx: f32, tokens: &PageTokens) -> usize {
        let max = self.pages.saturating_sub(1) as f32;
        let pos = self.pos();
        let target = if vx.abs() > tokens.fling_px_s {
            let from = if vx < 0.0 { pos.floor() } else { pos.ceil() };
            if vx < 0.0 {
                from + 1.0
            } else {
                from - 1.0
            }
        } else {
            pos.round()
        }
        .clamp(0.0, max);
        self.spring = tokens.spring;
        self.pos.release(target * self.width, tokens.spring);
        target as usize
    }

    /// An imperative move, such as an indicator tap.
    pub fn go_to(&mut self, page: usize, tokens: &PageTokens) {
        let max = self.pages.saturating_sub(1) as f32;
        self.spring = tokens.spring;
        self.pos
            .release((page as f32).min(max) * self.width, tokens.spring);
    }

    /// Settle at once.
    pub fn snap(&mut self, page: usize) {
        let max = self.pages.saturating_sub(1) as f32;
        self.pos.snap((page as f32).min(max) * self.width);
    }

    /// Advance. `true` while it is moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.pos.tick(dt)
    }

    /// The real page position (the A4 driving value `pos ∈ [0, N−1]`, a little outside it under the rubber band).
    #[must_use]
    pub fn pos(&self) -> f32 {
        self.pos.value() / self.width
    }

    /// The settled page (the spring's target, or the rounded position).
    #[must_use]
    pub fn page(&self) -> usize {
        (self.pos.target() / self.width).round().max(0.0) as usize
    }

    /// Whether a drag is in progress.
    #[doc(hidden)]
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.pos.is_dragging()
    }

    /// Whether the spring is running.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.pos.is_animating()
    }

    /// The two pages to draw right now and each one's x offset (px): `(index, x)`. On the same page, just the one.
    #[must_use]
    pub fn visible(&self) -> [(usize, f32); 2] {
        let pos = self.pos();
        let lo = pos.floor().max(0.0);
        let hi = pos.ceil().max(0.0);
        [
            (lo as usize, (lo - pos) * self.width),
            (hi as usize, (hi - pos) * self.width),
        ]
    }

    /// Indicator dot `i`'s strength, `1 − |i − pos|` (0..=1).
    #[must_use]
    pub fn dot_weight(&self, i: usize) -> f32 {
        (1.0 - (i as f32 - self.pos()).abs()).clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::PageSwipe;
    use crate::theme::MotionTokens;

    fn tokens() -> crate::theme::PageTokens {
        MotionTokens::default().page
    }

    /// A4: N = 3, W = 1024. From page 0, dx = −300 with v = −900 → 1. dx = −300 with v = 0 → 0.
    /// From page 2, dx = −400 → pos = 2.117, and the release lands on 2.
    #[test]
    fn a4_release_rules() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        s.begin();
        s.drag(-300.0, -900.0);
        assert_eq!(s.release(-900.0, &t), 1);
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        s.begin();
        s.drag(-300.0, 0.0);
        assert_eq!(s.release(0.0, &t), 0);
        let mut s = PageSwipe::new(3, 2, &t);
        s.configure(3, 1024.0);
        s.begin();
        s.drag(-400.0, 0.0);
        assert!((s.pos() - 2.117).abs() < 1e-3, "{}", s.pos());
        assert_eq!(s.release(0.0, &t), 2);
    }

    /// The A4 render mapping: two pages only, at x = `(i − pos) × W`. At an integer position
    /// the same page comes back twice (the caller draws just the one) with a zero offset.
    #[test]
    fn a4_visible_pair_and_offsets() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        assert_eq!(s.visible(), [(0, 0.0), (0, 0.0)]);
        s.begin();
        s.drag(-512.0, 0.0);
        let [(lo, x_lo), (hi, x_hi)] = s.visible();
        assert_eq!((lo, hi), (0, 1));
        assert!((x_lo + 512.0).abs() < 1e-3, "{x_lo}");
        assert!((x_hi - 512.0).abs() < 1e-3, "{x_hi}");
    }

    /// The indicator cross-fade weight `1 − |i − pos|`: at an integer, one dot at 1; halfway, two dots at 0.5.
    #[test]
    fn a4_dot_weight_crossfades() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        assert!((s.dot_weight(0) - 1.0).abs() < 1e-6);
        assert!(s.dot_weight(1).abs() < 1e-6 && s.dot_weight(2).abs() < 1e-6);
        s.begin();
        s.drag(-512.0, 0.0);
        assert!((s.dot_weight(0) - 0.5).abs() < 1e-3, "{}", s.dot_weight(0));
        assert!((s.dot_weight(1) - 0.5).abs() < 1e-3, "{}", s.dot_weight(1));
        assert!(s.dot_weight(2).abs() < 1e-6);
    }

    /// The release spring **actually runs**: the driving value being in px is what gives the
    /// settle test (0.5 px) its meaning. Run in page units, it settled on the first tick after the
    /// release and the spring was never seen.
    #[test]
    fn a4_release_spring_takes_frames() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        s.begin();
        s.drag(-600.0, 0.0);
        assert_eq!(s.release(0.0, &t), 1);
        let mut frames = 0;
        while s.tick(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 120, "it settles inside 2 s");
        }
        assert!(frames > 6, "the spring runs for several frames: {frames}");
        assert!((s.pos() - 1.0).abs() < 1e-3, "{}", s.pos());
        assert_eq!(s.page(), 1);
    }

    /// A width change re-anchors the px driving value while keeping the page position (a screen rotation).
    #[test]
    fn a4_configure_rescales_on_width_change() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 1, &t);
        s.configure(3, 1024.0);
        assert!((s.pos() - 1.0).abs() < 1e-6);
        s.begin();
        s.drag(-512.0, 0.0);
        assert!((s.pos() - 1.5).abs() < 1e-3);
        s.configure(3, 800.0);
        assert!(
            (s.pos() - 1.5).abs() < 1e-3,
            "the page position holds even as the width changes: {}",
            s.pos()
        );
        assert_eq!(s.visible(), [(1, -400.0), (2, 400.0)]);
    }

    /// The rubber band is only at the two ends (A4, "the excess × 0.3, up to 0.15 of a page"). Symmetrically below 0 as well.
    #[test]
    fn a4_rubber_band_at_both_ends() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        s.begin();
        s.drag(400.0, 0.0);
        assert!((s.pos() + 0.117).abs() < 1e-3, "{}", s.pos());
        s.drag(4000.0, 0.0);
        assert!(
            (s.pos() + 0.15).abs() < 1e-3,
            "the ceiling is 0.15: {}",
            s.pos()
        );
        assert_eq!(s.release(0.0, &t), 0);
    }

    /// **The rubber band follows the theme.** Holding on to what was handed in at construction
    /// leaves the pull-back resistance at the end on its default however the manufacturer changes
    /// `[motion] page.rubber` — which is why the shell calls [`PageSwipe::set_rubber`] every frame.
    #[test]
    fn the_rubber_band_follows_the_theme() {
        let t = tokens();
        let mut s = PageSwipe::new(3, 0, &t);
        s.configure(3, 1024.0);
        s.begin();
        s.drag(4000.0, 0.0);
        let stock = s.pos();

        // A theme with the cap halved.
        let mut loose = PageSwipe::new(3, 0, &t);
        loose.set_rubber(crate::motion::RubberBand::new(
            t.rubber.factor,
            t.rubber.max / 2.0,
        ));
        loose.configure(3, 1024.0);
        loose.begin();
        loose.drag(4000.0, 0.0);

        assert!(
            (loose.pos() - stock / 2.0).abs() < 1e-3,
            "the rubber-band ceiling did not follow: {} vs {}",
            loose.pos(),
            stock / 2.0
        );
    }
}
