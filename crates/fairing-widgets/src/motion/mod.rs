//! The motion system — no dependencies.
//!
//! Three principles: 1:1 while following a finger, a spring that inherits the velocity on
//! release, and short easing for every other transition. All of it is time-based, so it is
//! independent of the frame rate, and the spring evaluates **a closed-form solution** each frame
//! rather than integrating numerically, so no `dt` makes it diverge.

mod drag;

pub use drag::{DragSpring, ReleaseRule, RubberBand};

use std::collections::HashMap;
use std::time::Duration;

/// An easing curve. Uses `emath::easing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Easing {
    /// Linear.
    Linear,
    /// Fast to start, slow to finish (the default).
    #[default]
    CubicOut,
    /// Gentle at both ends.
    CubicInOut,
    /// A sharper start than `CubicOut`.
    QuartOut,
    /// Accelerating in (a heads-up leaving, A6).
    CubicIn,
    /// Past the end and back — a page flying in that lands a little beyond where it settles.
    /// The one curve here that leaves `0..=1`: it reaches about `1.1` before it comes
    /// back, so what it drives has to take a value past its rest.
    BackOut,
}

/// How far [`Easing::BackOut`] overshoots — Penner's `c1`, the value every toolkit shares.
const BACK: f32 = 1.701_58;

impl Easing {
    /// Put `t ∈ [0, 1]` through the curve. Every curve but [`Self::BackOut`] stays in `0..=1`.
    #[must_use]
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::CubicOut => egui::emath::easing::cubic_out(t),
            Self::CubicIn => t * t * t,
            Self::CubicInOut => egui::emath::easing::cubic_in_out(t),
            Self::QuartOut => {
                let u = 1.0 - t;
                1.0 - u * u * u * u
            }
            Self::BackOut => {
                let u = t - 1.0;
                (BACK + 1.0).mul_add(u * u * u, BACK.mul_add(u * u, 1.0))
            }
        }
    }
}

/// An eased transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tween {
    /// The length. 0 is instant.
    pub duration: Duration,
    /// The curve.
    pub easing: Easing,
}

impl Tween {
    /// A `CubicOut` of the given length.
    #[must_use]
    pub fn cubic_out(duration: Duration) -> Self {
        Self {
            duration,
            easing: Easing::CubicOut,
        }
    }

    /// A [`Easing::BackOut`] of the given length: it lands a little past the end and settles.
    #[must_use]
    pub fn back_out(duration: Duration) -> Self {
        Self {
            duration,
            easing: Easing::BackOut,
        }
    }

    /// Instant (`reduce` mode).
    #[must_use]
    pub fn instant() -> Self {
        Self {
            duration: Duration::ZERO,
            easing: Easing::Linear,
        }
    }

    /// The length in seconds.
    #[must_use]
    pub fn secs(&self) -> f32 {
        self.duration.as_secs_f32()
    }
}

impl Default for Tween {
    /// 220 ms `CubicOut` (push).
    fn default() -> Self {
        Self::cubic_out(Duration::from_millis(220))
    }
}

/// A damped spring. Critically damped by default: `k = 400, c = 2√k = 40`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// The stiffness, k.
    pub stiffness: f32,
    /// The damping, c.
    pub damping: f32,
}

impl Spring {
    /// A new spring. A `damping` below `2√k` bounces slightly (ζ < 1; 0.85 is as low as is recommended).
    #[must_use]
    pub fn new(stiffness: f32, damping: f32) -> Self {
        Self {
            stiffness: stiffness.max(f32::EPSILON),
            damping: damping.max(0.0),
        }
    }

    /// The damping ratio ζ = c / (2√k).
    #[must_use]
    pub fn damping_ratio(&self) -> f32 {
        self.damping / (2.0 * self.stiffness.sqrt())
    }

    /// Evaluate the closed form. Initial offset `x0`, initial velocity `v0`, elapsed `t` seconds
    /// → (offset, velocity).
    ///
    /// The closed solution of the damped harmonic oscillator `x'' + 2ζω₀x' + ω₀²x = 0`, split
    /// three ways by the damping ratio `ζ = c/(2√k)`:
    /// - `ζ = 1` (critical): `x(t) = (x0 + (v0 + ω₀x0)t) e^{-ω₀t}`.
    /// - `ζ < 1` (underdamped, oscillating): with `ωd = ω₀√(1−ζ²)`, `cos`/`sin` oscillation
    ///   inside the damping envelope `e^{-ζω₀t}`. To keep the overshoot from getting excessive,
    ///   `ζ` is floored here at [`MIN_DAMPING_RATIO`] (0.85) — `Spring`'s own `damping` is left
    ///   alone and the floor applies only to the evaluation.
    /// - `ζ > 1` (overdamped): `cosh`/`sinh` decay with `ωh = √(ζ²ω₀² − ω₀²)` (no oscillation).
    ///
    /// All three were derived to converge on the critical formula in the `ζ → 1` limit (the
    /// algebra, uncommented, is in the commit log). It returns four scalar coefficients, so it
    /// applies to vector values unchanged: `x = x0·cx + v0·cv`, `v = x0·dx + v0·dv`.
    #[must_use]
    pub fn coefficients(&self, t: f32) -> SpringCoefficients {
        let omega0 = self.stiffness.sqrt();
        let zeta = self.damping_ratio();

        if (zeta - 1.0).abs() < CRITICAL_DAMPING_EPS {
            // ζ = 1: critically damped. The under- and over-damped formulas go 0/0 at ζ = 1, so the limit is used directly.
            let e = (-omega0 * t).exp();
            return SpringCoefficients {
                x_from_x0: (1.0 + omega0 * t) * e,
                x_from_v0: t * e,
                v_from_x0: -omega0 * omega0 * t * e,
                v_from_v0: (1.0 - omega0 * t) * e,
            };
        }

        if zeta < 1.0 {
            // ζ < 1: under-damped (oscillating). Below 0.85 it bounces too much, so a floor is put on the ζ used for evaluation.
            let zeta_eff = zeta.max(MIN_DAMPING_RATIO);
            let lambda = zeta_eff * omega0;
            let omega_d = omega0 * (1.0 - zeta_eff * zeta_eff).sqrt();
            let e = (-lambda * t).exp();
            let (s, c) = (omega_d * t).sin_cos();
            SpringCoefficients {
                x_from_x0: e * (c + (lambda / omega_d) * s),
                x_from_v0: e * s / omega_d,
                v_from_x0: -e * (omega0 * omega0 / omega_d) * s,
                v_from_v0: e * (c - (lambda / omega_d) * s),
            }
        } else {
            // ζ > 1: over-damped. It settles without oscillating, more slowly than critical damping.
            //
            // Working `e^{-λt}·cosh(ωh·t)` / `sinh(ωh·t)` out separately has `cosh`/`sinh` diverge as t
            // grows under strong over-damping (a large ζ) while `e^{-λt}` converges to 0, giving
            // `inf * 0 = NaN`. So it is worked out as the sum and difference of two exponentials (both
            // decaying, `r2 ≥ r1 > 0`): `e^{-λt}cosh(ωh t) = (e^{-r1 t} + e^{-r2 t}) / 2`,
            // `e^{-λt}sinh(ωh t) = (e^{-r1 t} − e^{-r2 t}) / 2`.
            let lambda = zeta * omega0;
            let omega_h = (lambda * lambda - omega0 * omega0).sqrt();
            let r1 = lambda - omega_h; // the slow decay rate, ≥ 0
            let r2 = lambda + omega_h; // the fast decay rate
            let e1 = (-r1 * t).exp();
            let e2 = (-r2 * t).exp();
            #[expect(
                clippy::manual_midpoint,
                reason = "the textbook pair with e_sinh below; e1 and e2 are both at most 1, so the sum cannot overflow"
            )]
            let e_cosh = 0.5 * (e1 + e2); // e^{-λt} cosh(ωh t)
            let e_sinh = 0.5 * (e1 - e2); // e^{-λt} sinh(ωh t)
            SpringCoefficients {
                x_from_x0: e_cosh + (lambda / omega_h) * e_sinh,
                x_from_v0: e_sinh / omega_h,
                v_from_x0: -(omega0 * omega0 / omega_h) * e_sinh,
                v_from_v0: e_cosh - (lambda / omega_h) * e_sinh,
            }
        }
    }
}

/// Treat a ζ within this error as critically damped (ζ = 1) — it avoids the `sin(ωd·t)/ωd` and
/// `sinh(ωh·t)/ωh` terms of the under- and overdamped formulas becoming 0/0 as `ωd` (or `ωh`)
/// → 0.
const CRITICAL_DAMPING_EPS: f32 = 1e-3;

/// The floor on the damping ratio when evaluating underdamped (ζ < 1) ("0.85 or
/// so is as far as it goes"). Even if `Spring::damping` asks for a lower ζ, the evaluation
/// raises it so the overshoot does not get too large.
pub const MIN_DAMPING_RATIO: f32 = 0.85;

impl Default for Spring {
    fn default() -> Self {
        Self::new(400.0, 40.0)
    }
}

/// The result of [`Spring::coefficients`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringCoefficients {
    /// Multiply the offset by this.
    pub x_from_x0: f32,
    /// Multiply the velocity by this and add to the offset.
    pub x_from_v0: f32,
    /// Multiply the offset by this to get a velocity.
    pub v_from_x0: f32,
    /// Multiply the velocity by this to get a velocity.
    pub v_from_v0: f32,
}

/// An interpolatable value. Implemented for `f32` and `egui::Vec2`.
pub trait Lerp: Copy + PartialEq + std::fmt::Debug {
    /// 0.
    const ZERO: Self;
    /// `a + (b − a) t`.
    fn lerp(a: Self, b: Self, t: f32) -> Self;
    /// Addition.
    #[must_use]
    fn add(self, other: Self) -> Self;
    /// Subtraction.
    #[must_use]
    fn sub(self, other: Self) -> Self;
    /// Scalar multiplication.
    #[must_use]
    fn scale(self, k: f32) -> Self;
    /// The magnitude (for the settle decision).
    fn magnitude(self) -> f32;
}

impl Lerp for f32 {
    const ZERO: Self = 0.0;
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    fn add(self, other: Self) -> Self {
        self + other
    }
    fn sub(self, other: Self) -> Self {
        self - other
    }
    fn scale(self, k: f32) -> Self {
        self * k
    }
    fn magnitude(self) -> f32 {
        self.abs()
    }
}

impl Lerp for egui::Vec2 {
    const ZERO: Self = Self::ZERO;
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        a + (b - a) * t
    }
    fn add(self, other: Self) -> Self {
        self + other
    }
    fn sub(self, other: Self) -> Self {
        self - other
    }
    fn scale(self, k: f32) -> Self {
        self * k
    }
    fn magnitude(self) -> f32 {
        self.length()
    }
}

/// An [`Animated`]'s current mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode<T: Lerp> {
    /// At rest.
    Idle,
    /// An eased transition is running.
    Tween {
        /// The starting value.
        from: T,
        /// The elapsed seconds.
        elapsed: f32,
        /// The curve.
        tween: Tween,
    },
    /// A spring is settling.
    Spring {
        /// The initial offset (`current − target`).
        x0: T,
        /// The initial velocity.
        v0: T,
        /// The elapsed seconds.
        elapsed: f32,
        /// The spring.
        spring: Spring,
    },
    /// Following a finger (1:1).
    Dragging,
}

/// One animated value. Used for the shade offset, the page position, transition progress and so on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Animated<T: Lerp = f32> {
    current: T,
    target: T,
    velocity: T,
    mode: Mode<T>,
    /// The scale on the spring's settle decision — [`SETTLE_DISTANCE`] and [`SETTLE_VELOCITY`]
    /// are **in px**, so a normalised driving value (A3's `p ∈ [0, 1]` = `dx / W`) has to be
    /// multiplied by `1 / W` or it settles on the first tick ([`Animated::release_scaled`]).
    /// [`Animated::release`] uses 1.
    settle_scale: f32,
}

/// The dt cap. One late frame does not make it jump.
pub const MAX_DT: f32 = 0.05;

/// The spring's settle decision — the offset (px).
pub const SETTLE_DISTANCE: f32 = 0.5;
/// The spring's settle decision — the velocity (px/s).
pub const SETTLE_VELOCITY: f32 = 10.0;

impl<T: Lerp> Animated<T> {
    /// Build one at rest.
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            current: value,
            target: value,
            velocity: T::ZERO,
            mode: Mode::Idle,
            settle_scale: 1.0,
        }
    }

    /// An eased transition. Mid-flight it continues **from the current value** to the new target.
    pub fn to(&mut self, target: T, tween: Tween) {
        self.target = target;
        if tween.duration.is_zero() {
            self.snap(target);
            return;
        }
        self.mode = Mode::Tween {
            from: self.current,
            elapsed: 0.0,
            tween,
        };
    }

    /// Follow a finger. The value is used as-is and the velocity is recorded.
    pub fn drag(&mut self, value: T, velocity: T) {
        self.current = value;
        self.target = value;
        self.velocity = velocity;
        self.mode = Mode::Dragging;
    }

    /// Release → inherit the current value and velocity and settle to the target with a spring.
    /// The settle decision is in px ([`SETTLE_DISTANCE`] 0.5, [`SETTLE_VELOCITY`] 10) — for a
    /// value that is not px, use [`Animated::release_scaled`].
    pub fn release(&mut self, target: T, spring: Spring) {
        self.release_scaled(target, spring, 1.0);
    }

    /// The same as [`Animated::release`] but with the settle decision scaled by `settle_scale`.
    /// **A normalised driving value** (A3's `p = dx / W`, A4's page units) hits the
    /// absolute px decision (0.5 px, 10 px/s) on the very first tick, making the spring
    /// effectively instant — passing `1 / W` converts "0.5 px" into the value's own units. Zero
    /// or below is treated as 1.
    pub fn release_scaled(&mut self, target: T, spring: Spring, settle_scale: f32) {
        self.target = target;
        self.settle_scale = if settle_scale > 0.0 && settle_scale.is_finite() {
            settle_scale
        } else {
            1.0
        };
        self.mode = Mode::Spring {
            x0: self.current.sub(target),
            v0: self.velocity,
            elapsed: 0.0,
            spring,
        };
    }

    /// Straight to the target (reduce mode, or forcing a transition to finish).
    pub fn snap(&mut self, target: T) {
        self.current = target;
        self.target = target;
        self.velocity = T::ZERO;
        self.mode = Mode::Idle;
    }

    /// Advance. `true` while it is still moving → a repaint is needed. `dt` is clamped to [`MAX_DT`].
    pub fn tick(&mut self, dt: f32) -> bool {
        let dt = dt.clamp(0.0, MAX_DT);
        match self.mode {
            Mode::Idle | Mode::Dragging => false,
            Mode::Tween {
                from,
                elapsed,
                tween,
            } => {
                let elapsed = elapsed + dt;
                let secs = tween.secs();
                if elapsed >= secs {
                    let before = self.current;
                    self.snap(self.target);
                    self.velocity = self.target.sub(before).scale(1.0 / dt.max(1e-4));
                    false
                } else {
                    let s = tween.easing.apply(elapsed / secs);
                    let before = self.current;
                    self.current = T::lerp(from, self.target, s);
                    self.velocity = self.current.sub(before).scale(1.0 / dt.max(1e-4));
                    self.mode = Mode::Tween {
                        from,
                        elapsed,
                        tween,
                    };
                    true
                }
            }
            Mode::Spring {
                x0,
                v0,
                elapsed,
                spring,
            } => {
                let elapsed = elapsed + dt;
                let c = spring.coefficients(elapsed);
                let x = x0.scale(c.x_from_x0).add(v0.scale(c.x_from_v0));
                let v = x0.scale(c.v_from_x0).add(v0.scale(c.v_from_v0));
                self.current = self.target.add(x);
                self.velocity = v;
                if x.magnitude() < SETTLE_DISTANCE * self.settle_scale
                    && v.magnitude() < SETTLE_VELOCITY * self.settle_scale
                {
                    self.snap(self.target);
                    false
                } else {
                    self.mode = Mode::Spring {
                        x0,
                        v0,
                        elapsed,
                        spring,
                    };
                    true
                }
            }
        }
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> T {
        self.current
    }

    /// The target value.
    #[must_use]
    pub fn target(&self) -> T {
        self.target
    }

    /// The current velocity (units per second).
    #[must_use]
    pub fn velocity(&self) -> T {
        self.velocity
    }

    /// The mode.
    #[must_use]
    pub fn mode(&self) -> Mode<T> {
        self.mode
    }

    /// Whether a tween or spring is running (a drag is input, not an animation).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        matches!(self.mode, Mode::Tween { .. } | Mode::Spring { .. })
    }

    /// The tween's progress `t ∈ [0, 1]` (before easing). `1` when it is not a tween.
    #[must_use]
    pub fn progress(&self) -> f32 {
        match self.mode {
            Mode::Tween { elapsed, tween, .. } => {
                (elapsed / tween.secs().max(1e-6)).clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }
}

impl<T: Lerp + Default> Default for Animated<T> {
    fn default() -> Self {
        Self::new(T::default())
    }
}

/// The per-id animation store the shell owns ("for integrators"). A stateless
/// closure screen gets values from it through `cx.animate(id, target, tween)`. The same idea as
/// egui's animation manager.
#[derive(Debug, Default)]
pub struct AnimationStore {
    entries: HashMap<egui::Id, Entry>,
    animating: bool,
}

#[derive(Debug)]
struct Entry {
    anim: Animated<f32>,
    /// The frame number it was last queried on. Left unqueried for long enough, it is dropped.
    last_seen: u64,
}

impl AnimationStore {
    /// How many frames an unqueried entry survives. It is dropped once **at least this many**
    /// have passed since the last query (that is, on the `RETAIN_FRAMES + 1`-th tick) — the
    /// tests write the boundary in terms of this constant.
    pub const RETAIN_FRAMES: u64 = 600;

    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Frame start: advance every entry by `dt` and drop the stale ones.
    pub fn tick(&mut self, dt: f32, frame: u64) {
        let mut animating = false;
        self.entries
            .retain(|_, entry| frame.saturating_sub(entry.last_seen) < Self::RETAIN_FRAMES);
        for entry in self.entries.values_mut() {
            animating |= entry.anim.tick(dt);
        }
        self.animating = animating;
    }

    /// Tween `id`'s value towards `target` and return the current one. An id seen for the first time lands on `target`.
    /// So does one whose value is not a number (a NaN target asked for earlier): there is
    /// nothing to tween from.
    pub fn animate(&mut self, id: egui::Id, target: f32, tween: Tween, frame: u64) -> f32 {
        let entry = self.entries.entry(id).or_insert_with(|| Entry {
            anim: Animated::new(target),
            last_seen: frame,
        });
        entry.last_seen = frame;
        // Written so a NaN on either side counts as a new target, not as the same one.
        let same = (entry.anim.target() - target).abs() <= f32::EPSILON;
        if !entry.anim.value().is_finite() {
            entry.anim.snap(target);
        } else if !same {
            entry.anim.to(target, tween);
            self.animating = true;
        }
        entry.anim.value()
    }

    /// **Something outside the store is mid-motion this frame** — a page transit timed off
    /// egui's clock, a list refilling over several frames. It counts as animating until the next
    /// `tick`, so the shell's repaint policy and a tour's wait for rest see it as they see a
    /// tween here; without it a motion the store did not own was invisible to both, and a
    /// picture taken "once everything had settled" caught a page half faded in.
    pub fn keep_animating(&mut self) {
        self.animating = true;
    }

    /// Whether anything moved on the last `tick` (for the repaint policy).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.animating || self.entries.values().any(|e| e.anim.is_animating())
    }

    /// The entry count (for tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::{Animated, Easing, Spring, Tween};
    use std::time::Duration;

    /// `BackOut` is the one curve that leaves `0..=1`: it goes past the end and comes back,
    /// by about a tenth, and lands exactly.
    #[test]
    fn back_out_overshoots_and_lands() {
        assert!(Easing::BackOut.apply(0.0).abs() < 1e-6);
        assert!((Easing::BackOut.apply(1.0) - 1.0).abs() < 1e-5);
        let peak = (1..100u8)
            .map(|i| Easing::BackOut.apply(f32::from(i) / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(
            peak > 1.05 && peak < 1.15,
            "it overshoots by about a tenth: {peak}"
        );
    }

    #[test]
    fn tween_reaches_target_and_stops() {
        let mut a = Animated::new(0.0_f32);
        a.to(100.0, Tween::cubic_out(Duration::from_millis(100)));
        let mut frames = 0;
        while a.tick(1.0 / 60.0) {
            frames += 1;
            assert!(frames < 20);
        }
        assert!((a.value() - 100.0).abs() < f32::EPSILON);
        assert!(!a.is_animating());
    }

    #[test]
    fn critical_spring_never_overshoots() {
        let mut a = Animated::new(200.0_f32);
        a.drag(200.0, 0.0);
        a.release(0.0, Spring::default());
        let mut steps = 0;
        while a.tick(1.0 / 60.0) {
            assert!(a.value() >= -0.5, "overshoot: {}", a.value());
            steps += 1;
            assert!(steps < 240);
        }
        assert!((a.value()).abs() < f32::EPSILON);
    }

    #[test]
    fn easing_endpoints() {
        for e in [
            Easing::Linear,
            Easing::CubicOut,
            Easing::CubicInOut,
            Easing::QuartOut,
        ] {
            assert!(e.apply(0.0).abs() < 1e-6);
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6);
        }
    }

    /// ζ = 1 (critical) has to come out of `damping_ratio` as exactly 1 for the branch to be taken correctly.
    #[test]
    fn default_spring_is_critically_damped() {
        let s = Spring::default();
        assert!((s.damping_ratio() - 1.0).abs() < 1e-6);
    }

    /// Setting ζ < 1 does not take the evaluation below [`super::MIN_DAMPING_RATIO`] (0.85) —
    /// even a very low damping (ζ ≈ 0.1) has to trace the same path as a ζ = 0.85 spring.
    #[test]
    fn underdamped_evaluation_is_floored_at_0_85() {
        let k = 400.0_f32;
        let very_low = Spring::new(k, 0.1 * 2.0 * k.sqrt()); // ζ ≈ 0.1
        let floor = Spring::new(k, super::MIN_DAMPING_RATIO * 2.0 * k.sqrt()); // ζ = 0.85
        assert!(very_low.damping_ratio() < 0.2);
        for i in 0..30_u16 {
            let t = f32::from(i) * (1.0 / 60.0);
            let a = very_low.coefficients(t);
            let b = floor.coefficients(t);
            assert!((a.x_from_x0 - b.x_from_x0).abs() < 1e-3, "t={t} x_from_x0");
            assert!((a.x_from_v0 - b.x_from_v0).abs() < 1e-3, "t={t} x_from_v0");
            assert!((a.v_from_x0 - b.v_from_x0).abs() < 1e-2, "t={t} v_from_x0");
            assert!((a.v_from_v0 - b.v_from_v0).abs() < 1e-2, "t={t} v_from_v0");
        }
    }

    /// ζ < 1 (oscillating) overshoots the target and comes back — that is precisely what
    /// "underdamped" means, so the overshoot itself is correct. It does have to settle in the
    /// end, though.
    #[test]
    fn underdamped_spring_overshoots_then_settles() {
        let spring = Spring::new(400.0, 0.5 * 2.0 * 400.0_f32.sqrt()); // ζ = 0.5 → the 0.85 floor applies
        let mut a = Animated::new(100.0_f32);
        a.drag(100.0, 0.0);
        a.release(0.0, spring);
        let mut steps = 0;
        while a.tick(1.0 / 60.0) {
            steps += 1;
            assert!(steps < 600, "never settles");
        }
        assert!((a.value()).abs() < 1e-2);
    }

    /// ζ > 1 (overdamped): settles monotonically with no oscillation — it has to be calmer even than critical.
    #[test]
    fn overdamped_spring_never_overshoots_and_is_monotonic() {
        let spring = Spring::new(400.0, 8.0 * 40.0); // ζ = 8 (overdamped)
        assert!(spring.damping_ratio() > 1.0);
        let mut a = Animated::new(200.0_f32);
        a.drag(200.0, 0.0);
        a.release(0.0, spring);
        let mut last = a.value();
        let mut steps = 0;
        while a.tick(1.0 / 60.0) {
            let v = a.value();
            // Going past the target (0) into the negative is an overshoot — over-damped there should be none.
            assert!(v >= -0.5, "overshoot: {v}");
            // Monotonically decreasing (the sign does not change and it only approaches the target).
            assert!(v <= last + 1e-3, "not monotonic: {last} -> {v}");
            last = v;
            steps += 1;
            assert!(steps < 600, "never settles");
        }
        assert!(a.value().abs() < 1e-2);
    }

    /// The 50 ms dt cap: however late a real frame arrives (a large stall,
    /// say), one `tick` advances by at most 50 ms, so it does not diverge.
    #[test]
    fn dt_is_capped_and_never_diverges() {
        let mut a = Animated::new(0.0_f32);
        a.to(1000.0, Tween::cubic_out(Duration::from_millis(200)));
        // Given a very large dt over and over, the value has to stay finite and not pass the target.
        for _ in 0..50 {
            a.tick(5.0);
            assert!(a.value().is_finite());
            assert!(
                a.value() <= 1000.0 + 1e-3,
                "overshoot past target: {}",
                a.value()
            );
        }
        assert!((a.value() - 1000.0).abs() < 1e-2);

        // The spring likewise: finite through repeated large dt, and it settles in the end.
        let mut b = Animated::new(0.0_f32);
        b.drag(0.0, 2000.0);
        b.release(500.0, Spring::default());
        for _ in 0..200 {
            b.tick(5.0);
            assert!(b.value().is_finite());
        }
        assert!((b.value() - 500.0).abs() < 1.0);
    }

    /// Running to the same elapsed time at 60 fps and at 30 fps has to give the same value
    /// (±1 px) — both the spring and the tween evaluate a closed form of the accumulated elapsed
    /// time, so they are frame-rate independent.
    #[test]
    fn spring_value_is_frame_rate_independent() {
        let spring = Spring::default();
        let target = 0.0_f32;

        let mut at_60 = Animated::new(300.0_f32);
        at_60.drag(300.0, -150.0);
        at_60.release(target, spring);
        for _ in 0..12 {
            at_60.tick(1.0 / 60.0);
        } // 0.2 s elapsed

        let mut at_30 = Animated::new(300.0_f32);
        at_30.drag(300.0, -150.0);
        at_30.release(target, spring);
        for _ in 0..6 {
            at_30.tick(1.0 / 30.0);
        } // 0.2 s elapsed

        assert!(
            (at_60.value() - at_30.value()).abs() < 1.0,
            "60fps={} 30fps={}",
            at_60.value(),
            at_30.value()
        );
    }

    /// The tween on the same principle: its progress is a pure function of the elapsed time, so
    /// a difference in frame rate does not split the value.
    #[test]
    fn tween_value_is_frame_rate_independent() {
        let mut at_60 = Animated::new(0.0_f32);
        at_60.to(100.0, Tween::cubic_out(Duration::from_millis(240)));
        for _ in 0..8 {
            at_60.tick(1.0 / 60.0);
        } // 2/15 s

        let mut at_30 = Animated::new(0.0_f32);
        at_30.to(100.0, Tween::cubic_out(Duration::from_millis(240)));
        for _ in 0..4 {
            at_30.tick(1.0 / 30.0);
        } // 2/15 s

        assert!(
            (at_60.value() - at_30.value()).abs() < 1.0,
            "60fps={} 30fps={}",
            at_60.value(),
            at_30.value()
        );
    }

    /// Drag → release velocity continuity: the velocity the spring inherits the moment the
    /// finger lifts has to equal the drag's last velocity (no "start over").
    #[test]
    fn drag_then_release_velocity_is_continuous() {
        let mut a = Animated::new(0.0_f32);
        a.drag(50.0, -300.0);
        let v_before = a.velocity();
        a.release(0.0, Spring::default());
        // The velocity of the first evaluation at dt = 0 (effectively the moment of release) is exactly the drag's.
        a.tick(0.0);
        assert!(
            (a.velocity() - v_before).abs() < 1e-2,
            "jump at release: before={} after={}",
            v_before,
            a.velocity()
        );
    }

    /// The `AnimationStore::RETAIN_FRAMES` boundary: it is removed on the tick exactly that many
    /// frames after the last query (and survives until the tick before).
    #[test]
    fn animation_store_prunes_exactly_at_retain_boundary() {
        use super::AnimationStore;
        let mut store = AnimationStore::new();
        let id = egui::Id::new("m1-retain-test");
        store.animate(id, 1.0, Tween::default(), 0);
        for frame in 1..AnimationStore::RETAIN_FRAMES {
            store.tick(1.0 / 60.0, frame);
            assert_eq!(store.len(), 1, "pruned too early at frame {frame}");
        }
        store.tick(1.0 / 60.0, AnimationStore::RETAIN_FRAMES);
        assert_eq!(store.len(), 0, "not pruned at the retain boundary");
    }

    /// Using the absolute px settle decision on a normalised driving value (0..1) finishes it on
    /// the first tick — `release_scaled(.., 1 / W)` takes the same number of frames as the same
    /// spring seen in px (A3's release spring).
    #[test]
    fn release_scaled_makes_a_normalized_spring_take_real_frames() {
        let width = 1024.0_f32;
        let mut unscaled = Animated::new(0.4_f32);
        unscaled.release(0.0, Spring::default());
        assert!(
            !unscaled.tick(1.0 / 60.0),
            "0.4 < 0.5 px, so it settles on the first tick"
        );

        let mut scaled = Animated::new(0.4_f32);
        scaled.release_scaled(0.0, Spring::default(), 1.0 / width);
        let mut px = Animated::new(0.4_f32 * width);
        px.release(0.0, Spring::default());
        let mut frames_scaled = 0;
        while scaled.tick(1.0 / 60.0) {
            frames_scaled += 1;
            assert!(frames_scaled < 600, "it does not settle");
        }
        let mut frames_px = 0;
        while px.tick(1.0 / 60.0) {
            frames_px += 1;
        }
        assert!(
            frames_scaled > 5,
            "it really does run for several frames: {frames_scaled}"
        );
        assert_eq!(
            frames_scaled, frames_px,
            "the same frame count as seen in px"
        );
        assert!(scaled.value().abs() < 1e-6 && !scaled.is_animating());
        // The next `release` puts the factor back to 1.
        scaled.drag(0.3, 0.0);
        scaled.release(0.0, Spring::default());
        assert!(!scaled.tick(1.0 / 60.0));
    }
}
