//! The finger-coupling helpers — the shade (A1), gesture
//! back (A3), the page swipe (A4) and heads-up (A6) all share these rules.
//!
//! - **1:1 following**: mid-drag the value is `the starting value + the finger's movement`. Only
//!   the ends resist, through a [`RubberBand`].
//! - **The release decision**, [`ReleaseRule`]: either the distance threshold **or** the
//!   velocity threshold commits it. A fling the other way cancels.
//! - **Carrying the velocity over**: [`DragSpring::release`] hands the current value and
//!   velocity to the spring as its `x0` and `v0`.
//!
//! All of it is pure functions and values, so the numbers are pinned by headless unit tests.

use super::{Animated, Mode, Spring, Tween};

/// The rubber band at the ends: the overshoot × `factor`, up to `max`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RubberBand {
    /// The overshoot factor (0.25–0.3).
    pub factor: f32,
    /// The limit on how far the overshoot shows (px, or pages).
    pub max: f32,
}

impl RubberBand {
    /// A new band. Negatives clamp to 0.
    #[must_use]
    pub fn new(factor: f32, max: f32) -> Self {
        Self {
            factor: factor.max(0.0),
            max: max.max(0.0),
        }
    }

    /// `value` unchanged inside `[min, max]`; outside, the boundary plus the overshoot × factor (capped at `self.max`).
    #[must_use]
    pub fn apply(&self, value: f32, min: f32, max: f32) -> f32 {
        if value > max {
            max + ((value - max) * self.factor).min(self.max)
        } else if value < min {
            min - ((min - value) * self.factor).min(self.max)
        } else {
            value
        }
    }
}

impl Default for RubberBand {
    fn default() -> Self {
        Self::new(0.25, 40.0)
    }
}

/// The release decision: progress ≥ `snap_ratio` **or** velocity ≥ `fling` commits; a fling the other way cancels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReleaseRule {
    /// The distance threshold (0..=1, as progress).
    pub snap_ratio: f32,
    /// The velocity threshold (positive in the direction of travel).
    pub fling: f32,
}

impl ReleaseRule {
    /// `progress` is 0..=1, and `velocity` is positive in the direction of travel.
    #[must_use]
    pub fn confirm(&self, progress: f32, velocity: f32) -> bool {
        if velocity <= -self.fling {
            return false;
        }
        if velocity >= self.fling {
            return true;
        }
        progress >= self.snap_ratio
    }
}

impl Default for ReleaseRule {
    fn default() -> Self {
        Self {
            snap_ratio: 0.33,
            fling: 800.0,
        }
    }
}

/// One drag-to-spring value. The container for a value an animation is driven from by a finger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragSpring {
    anim: Animated<f32>,
    start: f32,
    min: f32,
    max: f32,
    band: RubberBand,
}

impl DragSpring {
    /// Value `value`, range `[min, max]`, rubber band `band`.
    #[must_use]
    pub fn new(value: f32, min: f32, max: f32, band: RubberBand) -> Self {
        Self {
            anim: Animated::new(value),
            start: value,
            min,
            max,
            band,
        }
    }

    /// Update the range (when the layout changes). The current value is left alone.
    pub fn set_range(&mut self, min: f32, max: f32) {
        self.min = min;
        self.max = max;
    }

    /// Update the rubber band (when the theme changes). The current value and any drag in
    /// progress are left alone — the new resistance applies from the next [`Self::drag`].
    pub fn set_rubber(&mut self, band: RubberBand) {
        self.band = band;
    }

    /// Begin a drag: take the current value as the starting point (mid-spring, from right there — every animation can be interrupted).
    pub fn begin(&mut self) {
        self.start = self.anim.value();
        self.anim.drag(self.start, 0.0);
    }

    /// Mid-drag: the value is rubber-banded(start + `delta`), and the velocity is the finger's.
    pub fn drag(&mut self, delta: f32, velocity: f32) {
        let value = self.band.apply(self.start + delta, self.min, self.max);
        self.anim.drag(value, velocity);
    }

    /// Release: spring to `target` from the current value and velocity.
    pub fn release(&mut self, target: f32, spring: Spring) {
        self.anim.release(target, spring);
    }

    /// Move with a tween (an imperative open or close).
    pub fn to(&mut self, target: f32, tween: Tween) {
        self.anim.to(target, tween);
    }

    /// Settle immediately.
    pub fn snap(&mut self, target: f32) {
        self.anim.snap(target);
    }

    /// Advance. `true` while it is still moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.anim.tick(dt)
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> f32 {
        self.anim.value()
    }

    /// The target.
    #[must_use]
    pub fn target(&self) -> f32 {
        self.anim.target()
    }

    /// The current velocity.
    #[must_use]
    pub fn velocity(&self) -> f32 {
        self.anim.velocity()
    }

    /// The progress within the range (0..=1).
    #[must_use]
    pub fn progress(&self) -> f32 {
        let span = (self.max - self.min).max(f32::EPSILON);
        ((self.anim.value() - self.min) / span).clamp(0.0, 1.0)
    }

    /// Whether it is being dragged.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        matches!(self.anim.mode(), Mode::Dragging)
    }

    /// Whether a tween or spring is running (a drag is not an animation).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.anim.is_animating()
    }

    /// The range's lower bound.
    #[must_use]
    pub fn min(&self) -> f32 {
        self.min
    }

    /// The range's upper bound.
    #[must_use]
    pub fn max(&self) -> f32 {
        self.max
    }
}

#[cfg(test)]
mod tests {
    use super::{DragSpring, ReleaseRule, RubberBand};
    use crate::motion::Spring;

    #[test]
    fn rubber_band_resists_beyond_range_with_cap() {
        let band = RubberBand::new(0.25, 40.0);
        assert!((band.apply(100.0, 0.0, 480.0) - 100.0).abs() < 1e-6);
        assert!(
            (band.apply(520.0, 0.0, 480.0) - 490.0).abs() < 1e-6,
            "the overshoot, 40 × 0.25"
        );
        assert!(
            (band.apply(2000.0, 0.0, 480.0) - 520.0).abs() < 1e-6,
            "the ceiling, 40"
        );
        assert!((band.apply(-40.0, 0.0, 480.0) + 10.0).abs() < 1e-6);
    }

    /// A4: on page 2 with dx = −400 and W = 1024, pos = 2 + 0.117.
    #[test]
    fn page_rubber_matches_a4_number() {
        let band = RubberBand::new(0.3, 0.15);
        let pos = band.apply(2.0 + 400.0 / 1024.0, 0.0, 2.0);
        assert!((pos - 2.117).abs() < 1e-3, "{pos}");
    }

    #[test]
    fn release_rule_distance_or_velocity() {
        let rule = ReleaseRule::default();
        assert!(
            rule.confirm(0.2, 900.0),
            "confirmed by velocity (A3 dx=0.2W, v=900)"
        );
        assert!(!rule.confirm(0.2, 0.0), "short of both, so cancelled");
        assert!(rule.confirm(0.5, 0.0), "confirmed by distance");
        assert!(
            !rule.confirm(0.9, -1000.0),
            "a fling the other way cancels (A1 v = −1000 → Closed)"
        );
    }

    #[test]
    fn drag_spring_follows_finger_then_settles() {
        let mut d = DragSpring::new(0.0, 0.0, 480.0, RubberBand::default());
        d.begin();
        d.drag(146.0, 600.0);
        assert!((d.value() - 146.0).abs() < 1e-6 && d.is_dragging());
        d.release(480.0, Spring::default());
        let mut frames = 0;
        while d.tick(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 120, "it settles inside 2 s");
        }
        assert!((d.value() - 480.0).abs() < 1e-3);
        // x0 = −334, v0 = 600, ω = 20: settling to 0.5 px takes ≈ 450 ms (A1's "Open within 300 ms"
        // was corrected to match the physical settle).
        assert!(
            frames < 40,
            "k=400 critical damping settles inside 670 ms ({frames} frames)"
        );
    }
}
