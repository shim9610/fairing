//! The shade's state machine (A1). The driving value `y ∈ [0, H]` is **the
//! revealed height** — the panel's content is pinned to the top of the screen and only
//! `[top, top + y]` shows (a curtain; [`super::Overlay::ui`]).
//!
//! ```text
//! Closed → Dragging{y, v} → Settling{opening} → Open
//! Open → Dragging → Settling → Closed
//! ```
//! Pressing during `Settling` returns to `Dragging` (at the current y, with v = 0 —
//! [`Shade::begin_drag`] freezes it where it is). On release: it commits to open at
//! `y ≥ H × snap_ratio` or `v ≥ +fling`, and to closed at `v ≤ −fling` or below the threshold.
//! The spring is k 400, c 40, with the finger's velocity as the initial one. Under `reduce` it
//! settles the instant it is released (still following the drag).
//!
//! **How `Open` is decided**: `Settling → Open/Closed` stays
//! at the spring's **physical settle** (`|x − target| < 0.5 px` and
//! `|v| < 10 px/s`, with [`crate::motion::Animated`] as the single source). In the
//! A1 test sequence ((10,8) → (10,300) over 12 frames, x0 = −188 px, v0 ≈ 1460 px/s),
//! critically damped k 400 still has 2.2 px and 38 px/s left at 300 ms, so no amount of
//! "finishing 1 px early" gets inside 300 ms (it is 350 ms). It settles at ≈ 390 ms (24 frames),
//! so the integration test asserts **`Open` within 450 ms**, and A1's
//! "300 ms" is corrected to these numbers. Finishing `Open` early was rejected because it would create an
//! `Open` with `y ≠ H`, which destabilises the `set_height` snap and the handoff decision.
//!
//! There are two paths by which a finger takes the shade: an edge swipe from the top (the engine
//! → [`super::Overlay::update`]) and a drag or re-grab on the open panel (the raw pointer in
//! [`super::Overlay::ui`]). Both call only this type's `begin_drag` / `drag` / `release`; who
//! owns the gesture (the nested-scroll handoff) is decided in `Overlay`.

use crate::motion::{DragSpring, ReleaseRule, Spring, Tween};
use crate::theme::MotionTokens;
use std::time::{Duration, Instant};

/// How close to either end a detent may sit, as a share of the height. Nearer than this and it is
/// dropped: two stops a few du apart is a shade that appears to stick rather than to have steps.
const DETENT_MARGIN: f32 = 0.12;

/// The shade's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayState {
    /// Closed (y = 0).
    Closed,
    /// Following the finger.
    Dragging {
        /// The driving value.
        y: f32,
        /// The finger's velocity.
        v: f32,
    },
    /// The spring is settling.
    Settling {
        /// Whether it is opening.
        opening: bool,
    },
    /// Open (y = H).
    Open,
}

/// The shade's driving value.
#[derive(Debug, Clone, Copy)]
pub struct Shade {
    y: DragSpring,
    state: OverlayState,
    height: f32,
    peek_until: Option<Instant>,
    /// **The intermediate stop**, where a two-step shade rests after the first pull — the tiles
    /// shown and the notification list still below the fold. `None` is the one-step shade, which
    /// is what every panel had before the two-step shade and still the default.
    detent: Option<f32>,
    /// Where "open" currently means. With a detent the shade has two open positions, and a layout
    /// change has to leave it at the one it is actually resting on.
    open_at: f32,
    /// **The way out, when it is not the spring**. A card leaves rather than being
    /// drawn back up — it fades and lifts inside a tenth of the time it took to arrive — so a
    /// card shade closes on this tween. `None` (the curtain) springs shut as it always has.
    close: Option<Tween>,
    /// The spring the shade last settled on, so a relayout mid-opening can re-aim it.
    spring: Spring,
}

impl Shade {
    /// A closed shade of height `height`.
    #[must_use]
    pub fn new(height: f32, tokens: &MotionTokens) -> Self {
        let h = height.max(1.0);
        Self {
            y: DragSpring::new(0.0, 0.0, h, tokens.shade.rubber),
            state: OverlayState::Closed,
            height: h,
            peek_until: None,
            detent: None,
            open_at: h,
            close: None,
            spring: tokens.shade.spring,
        }
    }

    /// **Close on a tween instead of the spring** — `Some` for a card, `None` for a curtain.
    pub(crate) fn set_close(&mut self, tween: Option<Tween>) {
        self.close = tween;
    }

    /// Update the height (when the layout changes). If it is open, or still settling open, the
    /// value follows.
    pub(crate) fn set_height(&mut self, height: f32) {
        let h = height.max(1.0);
        if (h - self.height).abs() < 0.5 {
            return;
        }
        // "Fully open" is judged against the height it was open at, before it changes.
        let fully_open = self.open_at >= self.height - 0.5;
        self.height = h;
        self.y.set_range(0.0, h);
        if fully_open || self.detent.is_none() {
            self.open_at = h;
        }
        self.open_at = self.open_at.min(h);
        let at = self.open_at;
        match self.state {
            // **To the stop it is resting on, not to the top.** With a detent the shade has two
            // open positions, and snapping a tile-height shade to the full height on a relayout
            // would look like it opened itself.
            OverlayState::Open => self.y.snap(at),
            // Still on its way: the spring is re-aimed from where it is, with the speed it has,
            // so it does not land `Open` at the old height.
            OverlayState::Settling { opening: true } if (self.y.target() - at).abs() > 0.5 => {
                self.y.release(at, self.spring);
            }
            _ => {}
        }
    }

    /// **Set the intermediate stop**, or `None` for a shade that only opens all the way.
    ///
    /// The value is a height from the shade's own top, and the caller is the one that knows it: it
    /// is where the tiles end, which is a fact about what the panel drew. Out of range, or within
    /// a hair of either end, it is ignored — two stops in the same place is a shade that appears to
    /// stick.
    pub(crate) fn set_detent(&mut self, detent: Option<f32>) {
        let margin = self.height * DETENT_MARGIN;
        self.detent = detent.filter(|d| *d > margin && *d < self.height - margin);
        if self.detent.is_none() {
            self.open_at = self.height;
        }
    }

    /// The intermediate stop, if this shade has one.
    #[must_use]
    pub(crate) fn detent(&self) -> Option<f32> {
        self.detent
    }

    /// **How far through the scrim's own travel** the shade is.
    ///
    /// The same as [`Self::progress`] on a one-step shade. On a two-step one it is zero until the
    /// stop and then sweeps to one over what is left, so the tiles come down over an undimmed
    /// screen and the dimming belongs to the notification list.
    ///
    /// That is the whole reason to have a stop. A shade opened to toggle Wi-Fi is worth stopping
    /// early only if what it stopped in front of is still readable, and a scrim that ramps with the
    /// total travel has already taken a quarter of the contrast by the time it gets there — which
    /// is what the console example looked like before the scrim waited for the stop: the run behind it went grey to show
    /// six tiles.
    #[must_use]
    pub(crate) fn scrim_progress(&self) -> f32 {
        let Some(detent) = self.detent else {
            return self.progress();
        };
        ((self.y.value() - detent) / (self.height - detent).max(1.0)).clamp(0.0, 1.0)
    }

    /// Whether it is resting at the intermediate stop rather than fully open.
    #[must_use]
    pub(crate) fn at_detent(&self) -> bool {
        self.detent
            .is_some_and(|d| self.is_open() && (self.open_at - d).abs() < 0.5)
    }

    /// The state.
    #[must_use]
    pub fn state(&self) -> OverlayState {
        self.state
    }

    /// The driving value y.
    #[must_use]
    pub fn y(&self) -> f32 {
        self.y.value()
    }

    /// The height H.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.height
    }

    /// `y / H`.
    #[must_use]
    pub fn progress(&self) -> f32 {
        self.y.progress()
    }

    /// Whether it is closed and still.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.state == OverlayState::Closed
    }

    /// Whether it is fully open.
    #[doc(hidden)]
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.state == OverlayState::Open
    }

    /// A spring or tween is running.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.y.is_animating()
    }

    /// Begin a drag (an edge swipe `Started`, or grabbing the open panel). It continues from the current y.
    pub(crate) fn begin_drag(&mut self) {
        self.y.begin();
        self.state = OverlayState::Dragging {
            y: self.y.value(),
            v: 0.0,
        };
    }

    /// Mid-drag: `dy` is the downward movement from the press point (px) and `v` the vertical velocity (px/s, positive down).
    pub fn drag(&mut self, dy: f32, v: f32) {
        if !matches!(self.state, OverlayState::Dragging { .. }) {
            self.begin_drag();
        }
        self.y.drag(dy, v);
        self.state = OverlayState::Dragging {
            y: self.y.value(),
            v,
        };
    }

    /// Release (the A1 release rules). `true` if it comes to rest open.
    ///
    /// With a detent the rules are applied **to the gap the finger is in** rather than to the whole
    /// travel: between the closed position and the stop, and between the stop and the top. So one
    /// pull lands on the tiles and a second carries on to the notifications, and a fling in either
    /// gap does what a fling has always done, which is to skip the snap ratio and commit. Written
    /// against the whole travel instead, a pull that stopped at the tiles would read as 40 % of the
    /// way open and fall straight back shut.
    pub fn release(&mut self, v: f32, tokens: &MotionTokens) -> bool {
        let target = self.stop_for(v, tokens);
        self.settle_to(target, tokens);
        target > 0.0
    }

    /// Which stop a release at velocity `v` commits to.
    fn stop_for(&self, v: f32, tokens: &MotionTokens) -> f32 {
        let rule = ReleaseRule {
            snap_ratio: tokens.shade.snap_ratio,
            fling: tokens.fling_px_s,
        };
        let Some(detent) = self.detent else {
            return if rule.confirm(self.y.progress(), v) {
                self.height
            } else {
                0.0
            };
        };
        let y = self.y.value();
        let (lo, hi) = if y < detent {
            (0.0, detent)
        } else {
            (detent, self.height)
        };
        let span = (hi - lo).max(1.0);
        if rule.confirm((y - lo) / span, v) {
            hi
        } else {
            lo
        }
    }

    /// Open it imperatively.
    pub fn open(&mut self, tokens: &MotionTokens) {
        if !self.is_open() {
            self.settle(true, tokens);
        }
    }

    /// Close it imperatively (back, a scrim tap, a session downgrade).
    pub fn close(&mut self, tokens: &MotionTokens) {
        if !self.is_closed() {
            self.settle(false, tokens);
        }
    }

    /// **Put it away at once**, with no motion — for a split shade crossing to its other panel,
    /// which then comes in from shut.
    pub(crate) fn reset_closed(&mut self) {
        self.y.snap(0.0);
        self.state = OverlayState::Closed;
    }

    fn settle(&mut self, opening: bool, tokens: &MotionTokens) {
        let target = if opening { self.height } else { 0.0 };
        self.settle_to(target, tokens);
    }

    /// Settle on an explicit stop.
    pub(crate) fn settle_to(&mut self, target: f32, tokens: &MotionTokens) {
        let opening = target > 0.0;
        if opening {
            self.open_at = target;
        }
        self.spring = tokens.shade.spring;
        if tokens.reduce {
            self.y.snap(target);
            self.state = if opening {
                OverlayState::Open
            } else {
                OverlayState::Closed
            };
            return;
        }
        match (opening, self.close) {
            // A card goes out on its tween; the finger's speed is not carried, there is nothing
            // left to carry it into.
            (false, Some(tween)) => self.y.to(target, tween),
            _ => self.y.release(target, tokens.shade.spring),
        }
        self.state = OverlayState::Settling { opening };
    }

    /// Advance. `true` while it is moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        let moving = self.y.tick(dt);
        if let OverlayState::Settling { opening } = self.state {
            if !moving {
                self.state = if opening {
                    OverlayState::Open
                } else {
                    OverlayState::Closed
                };
            }
        }
        moving
    }

    /// Start a peek: show the hidden status row until `until`.
    pub fn peek(&mut self, until: Instant) {
        self.peek_until = Some(until);
    }

    /// The peek time remaining. `None` once it is over (and it clears it).
    pub(crate) fn peek_remaining(&mut self, now: Instant) -> Option<Duration> {
        let until = self.peek_until?;
        if now >= until {
            self.peek_until = None;
            return None;
        }
        Some(until.saturating_duration_since(now))
    }

    /// Whether it is peeking.
    #[must_use]
    pub(crate) fn is_peeking(&self, now: Instant) -> bool {
        self.peek_until.is_some_and(|u| now < u)
    }
}

#[cfg(test)]
mod tests {
    use super::{OverlayState, Shade};
    use crate::theme::MotionTokens;

    /// A1: H = 480, released at y = 146 → below the threshold (0.304 < 0.33) so closed; v = 900 opens; v = −1000 closes.
    #[test]
    fn release_rules() {
        let t = MotionTokens::default();
        let mut s = Shade::new(480.0, &t);
        s.begin_drag();
        s.drag(146.0, 0.0);
        assert!(!s.release(0.0, &t));
        let mut s = Shade::new(480.0, &t);
        s.begin_drag();
        s.drag(146.0, 900.0);
        assert!(s.release(900.0, &t));
        assert_eq!(s.state(), OverlayState::Settling { opening: true });
        let mut n = 0;
        while s.tick(1.0 / 60.0) {
            n += 1;
            assert!(n < 60, "it settles inside 300 ms");
        }
        assert!(s.is_open() && (s.y() - 480.0).abs() < 1e-3);
        let mut s = Shade::new(480.0, &t);
        s.begin_drag();
        s.drag(400.0, -1000.0);
        assert!(!s.release(-1000.0, &t));
    }

    /// A1, interrupt and re-grab: grabbing mid-close freezes at that y (v = 0) and
    /// drags 1:1 from there, and dragging up from open starts at H and comes down 1:1 (the
    /// shade's half of the handoff).
    #[test]
    fn regrab_while_settling_freezes_at_current_y() {
        let t = MotionTokens::default();
        let mut s = Shade::new(480.0, &t);
        s.open(&t);
        while s.tick(1.0 / 60.0) {}
        assert!(s.is_open());
        s.close(&t);
        for _ in 0..6 {
            s.tick(1.0 / 60.0);
        }
        let mid = s.y();
        assert!(mid > 1.0 && mid < 479.0, "closing: {mid}");
        s.begin_drag();
        assert_eq!(
            s.state(),
            OverlayState::Dragging { y: mid, v: 0.0 },
            "it freezes where it is"
        );
        assert!(!s.tick(1.0 / 60.0) && (s.y() - mid).abs() < 1e-6);
        s.drag(-30.0, -200.0);
        assert!((s.y() - (mid - 30.0)).abs() < 1e-3, "1:1");
        // The hand-off while open: it starts at H.
        let mut s = Shade::new(480.0, &t);
        s.open(&t);
        while s.tick(1.0 / 60.0) {}
        s.begin_drag();
        s.drag(-60.0, -500.0);
        assert!((s.y() - 420.0).abs() < 1e-3);
        s.drag(20.0, 100.0);
        assert!(
            (s.y() - 485.0).abs() < 1e-3,
            "the 20 over H rubber-bands to × 0.25"
        );
    }
}
