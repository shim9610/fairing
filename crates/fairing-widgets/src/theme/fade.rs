//! The theme crossfade (A7, "the theme switch"): interpolate the palette with
//! [`Palette::lerp`] over 200 ms and apply it to egui's `Visuals` every frame. Dark and light
//! swap with no flicker.
//!
//! The shell calls [`ThemeFade::start`] when the `theme.dark` setting changes (the `tile.theme`
//! tile), and in frame stage 5 applies the palette [`ThemeFade::tick`] returned via
//! [`Theme::apply`](super::Theme::apply). While it runs, [`ThemeFade::is_animating`] is the
//! reason to repaint.
//!
//! **The cost**: `Theme::apply` builds a new egui `Style` each frame. So [`ThemeFade::tick`]
//! returns `Some` **only on a frame where the palette actually differs** — not after it has
//! settled, and not on a frame where the interpolated value is unchanged. Under `reduce` (a
//! 0 ms tween) it returns the target palette **once** and stops (the tween finishes instantly
//! so `is_animating` never rises, which is why a separate flag is needed).

use super::Palette;
use crate::motion::{Animated, Tween};

/// The palette interpolation state.
#[derive(Debug, Clone)]
pub struct ThemeFade {
    from: Palette,
    to: Palette,
    t: Animated<f32>,
    /// The palette last returned. When it is unchanged, `tick` is `None`.
    applied: Palette,
    /// Whether a change is still unsent (even a 0 ms tween goes out once).
    pending: bool,
}

impl ThemeFade {
    /// At rest (holding `palette`).
    #[must_use]
    pub fn idle(palette: Palette) -> Self {
        Self {
            from: palette,
            to: palette,
            t: Animated::new(1.0),
            applied: palette,
            pending: false,
        }
    }

    /// Start interpolating from `from` to `to` over `tween`. If one is running it continues
    /// **from the current interpolated value** to the new target.
    pub fn start(&mut self, to: Palette, tween: Tween) {
        self.from = self.current();
        self.to = to;
        self.t = Animated::new(0.0);
        self.t.to(1.0, tween);
        self.pending = true;
    }

    /// Advance. `Some` **only on a frame where the palette to apply changed**.
    pub fn tick(&mut self, dt: f32) -> Option<Palette> {
        if self.t.is_animating() {
            self.t.tick(dt);
        } else if !self.pending {
            return None;
        }
        self.pending = self.t.is_animating();
        let now = self.current();
        if now == self.applied {
            return None;
        }
        self.applied = now;
        Some(now)
    }

    /// The palette showing right now.
    #[must_use]
    pub fn current(&self) -> Palette {
        if self.t.value() >= 1.0 {
            self.to
        } else {
            self.from.lerp(&self.to, self.t.value())
        }
    }

    /// The target palette.
    #[must_use]
    pub fn target(&self) -> &Palette {
        &self.to
    }

    /// Whether it is interpolating (or still has an unapplied change).
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.t.is_animating() || self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::ThemeFade;
    use crate::motion::Tween;
    use crate::theme::Palette;
    use std::time::Duration;

    const DT: f32 = 1.0 / 60.0;

    /// A 200 ms interpolation: midway it is neither end, and at the end it is the target, at rest.
    #[test]
    fn interpolates_then_settles() {
        let mut fade = ThemeFade::idle(Palette::dark());
        fade.start(
            Palette::light(),
            Tween::cubic_out(Duration::from_millis(200)),
        );
        assert!(fade.is_animating());
        let mut last = None;
        for _ in 0..6 {
            last = fade.tick(DT);
        }
        let mid = last.unwrap_or_else(Palette::dark);
        assert_ne!(mid, Palette::dark());
        assert_ne!(mid, Palette::light());
        for _ in 0..20 {
            fade.tick(DT);
        }
        assert!(!fade.is_animating());
        assert_eq!(fade.current(), Palette::light());
        assert!(
            fade.tick(DT).is_none(),
            "after settling it is not applied again"
        );
    }

    /// Even under `reduce` (0 ms) the target palette goes out **once** — otherwise the shell cannot change the theme.
    #[test]
    fn instant_tween_still_emits_once() {
        let mut fade = ThemeFade::idle(Palette::dark());
        fade.start(Palette::light(), Tween::instant());
        assert!(
            fade.is_animating(),
            "a repaint is needed until it is applied"
        );
        assert_eq!(fade.tick(DT), Some(Palette::light()));
        assert!(!fade.is_animating());
        assert!(fade.tick(DT).is_none());
    }

    /// Asked to change to the same palette, nothing goes out (no `Style` recomputation).
    #[test]
    fn no_op_start_emits_nothing() {
        let mut fade = ThemeFade::idle(Palette::dark());
        fade.start(Palette::dark(), Tween::instant());
        assert!(fade.tick(DT).is_none());
        assert!(!fade.is_animating());
    }

    /// Reversing mid-flight continues from the current value.
    #[test]
    fn reversing_continues_from_the_current_value() {
        let mut fade = ThemeFade::idle(Palette::dark());
        fade.start(
            Palette::light(),
            Tween::cubic_out(Duration::from_millis(200)),
        );
        for _ in 0..6 {
            fade.tick(DT);
        }
        let mid = fade.current();
        fade.start(
            Palette::dark(),
            Tween::cubic_out(Duration::from_millis(200)),
        );
        assert_eq!(fade.current(), mid, "it does not break");
        for _ in 0..20 {
            fade.tick(DT);
        }
        assert_eq!(fade.current(), Palette::dark());
    }
}
