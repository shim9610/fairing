//! The parametric status icon crossfade (A7): when the Wi-Fi strength or the battery
//! step changes, the previous and next icons are both drawn for 120 ms
//! (`ParamStyle::with_alpha`).
//!
//! The status bar holds one [`ParamFade`] per item, and `set` records the start time on the
//! frame the value changed. Rendering hands a drawing closure to [`ParamFade::paint`] — the
//! closure receives a style with the value and the alpha already applied and draws once. Zero
//! heap allocations per frame.
//!
//! **The curve**: the time progress ([`ParamFade::progress`]) is linear and only the alpha goes
//! through `CubicOut` ("tweens are `CubicOut`"). The incoming value gets
//! `CubicOut(t)` and the outgoing one `1 − CubicOut(t)`, so the two alphas always sum to 1.
//!
//! **On consecutive changes** (a Wi-Fi strength going 0 → 2 → 3 within 120 ms): the fade is not
//! restarted. **The outgoing value is left as it is** and only the
//! incoming one is swapped for the new value, finishing on the original schedule — the
//! intermediate value was never on screen, so dropping it does not show, and a rapidly changing
//! value never leaves the icon stuck half-faded. Returning to where it started (0 → 1 → 0)
//! cancels the fade outright.

use super::parametric::ParamStyle;
use crate::motion::Easing;
use std::time::{Duration, Instant};

/// One value's crossfade state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamFade<T: Copy + PartialEq> {
    prev: Option<T>,
    cur: T,
    since: Option<Instant>,
    duration: Duration,
}

impl<T: Copy + PartialEq> ParamFade<T> {
    /// The initial value (no fade).
    #[must_use]
    pub fn new(value: T, duration: Duration) -> Self {
        Self {
            prev: None,
            cur: value,
            since: None,
            duration,
        }
    }

    /// A new value. If it changed, the fade starts at `now`. Mid-fade it leaves the outgoing
    /// value and the start time alone and only changes the incoming one (see "on consecutive
    /// changes" in the module docs).
    pub fn set(&mut self, value: T, now: Instant) {
        if value == self.cur {
            return;
        }
        if self.is_animating(now) {
            if self.prev == Some(value) {
                // It came back — there is nothing to fade.
                self.cur = value;
                self.prev = None;
                self.since = None;
                return;
            }
            self.cur = value;
            return;
        }
        self.prev = Some(self.cur);
        self.cur = value;
        self.since = Some(now);
    }

    /// The current value.
    #[must_use]
    pub fn value(&self) -> T {
        self.cur
    }

    /// The progress, 0..=1 (1 is finished). It is linear — [`ParamFade::paint`] puts the alpha curve on it.
    #[must_use]
    pub fn progress(&self, now: Instant) -> f32 {
        match self.since {
            Some(since) if !self.duration.is_zero() => {
                (now.saturating_duration_since(since).as_secs_f32() / self.duration.as_secs_f32())
                    .clamp(0.0, 1.0)
            }
            _ => 1.0,
        }
    }

    /// Whether it is fading (the reason to repaint).
    #[must_use]
    pub fn is_animating(&self, now: Instant) -> bool {
        self.prev.is_some() && self.progress(now) < 1.0
    }

    /// Draw: mid-fade, the previous value at `1 − CubicOut(t)` and the current one at `CubicOut(t)`, twice over.
    pub fn paint(
        &mut self,
        now: Instant,
        style: &ParamStyle,
        mut draw: impl FnMut(T, &ParamStyle),
    ) {
        let t = self.progress(now);
        if let Some(prev) = self.prev {
            if t < 1.0 {
                let alpha = Easing::CubicOut.apply(t);
                draw(prev, &style.with_alpha(1.0 - alpha));
                draw(self.cur, &style.with_alpha(alpha));
                return;
            }
            self.prev = None;
            self.since = None;
        }
        draw(self.cur, style);
    }
}

#[cfg(test)]
mod tests {
    use super::ParamFade;
    use crate::icons::parametric::ParamStyle;
    use egui::Color32;
    use std::time::{Duration, Instant};

    const D: Duration = Duration::from_millis(120);

    fn style() -> ParamStyle {
        ParamStyle {
            color: Color32::WHITE,
            muted: Color32::GRAY,
            danger: Color32::RED,
            stroke_px: 2.0,
        }
    }

    /// The (value, main colour alpha) pairs that were drawn.
    fn painted(fade: &mut ParamFade<u8>, now: Instant) -> Vec<(u8, u8)> {
        let mut out = Vec::new();
        fade.paint(now, &style(), |value, s| out.push((value, s.color.a())));
        out
    }

    #[test]
    fn steady_value_paints_once() {
        let t0 = Instant::now();
        let mut fade = ParamFade::new(1u8, D);
        assert_eq!(painted(&mut fade, t0), vec![(1, 255)]);
        assert!(!fade.is_animating(t0));
    }

    /// A change draws twice, and after 120 ms it is back to drawing once.
    #[test]
    fn change_crossfades_for_the_duration() {
        let t0 = Instant::now();
        let mut fade = ParamFade::new(1u8, D);
        fade.set(2, t0);
        assert!(fade.is_animating(t0));
        let mid = painted(&mut fade, t0 + Duration::from_millis(60));
        assert_eq!(mid.len(), 2, "the previous one plus the current one");
        assert_eq!(mid.first().map(|v| v.0), Some(1));
        assert_eq!(mid.get(1).map(|v| v.0), Some(2));
        assert!(!fade.is_animating(t0 + D));
        assert_eq!(painted(&mut fade, t0 + D), vec![(2, 255)]);
    }

    /// The alphas always sum to about 1 (`CubicOut` and its complement).
    #[test]
    fn alphas_complement_each_other() {
        let t0 = Instant::now();
        let mut fade = ParamFade::new(1u8, D);
        fade.set(2, t0);
        for ms in [10u64, 40, 80, 110] {
            let shot = painted(&mut fade, t0 + Duration::from_millis(ms));
            let sum = shot.iter().map(|v| u32::from(v.1)).sum::<u32>();
            assert!((250..=260).contains(&sum), "ms={ms} sum={sum}");
        }
    }

    /// Changing again mid-fade keeps the outgoing value and swaps only the incoming one — and finishes at the same time.
    #[test]
    fn consecutive_change_keeps_the_outgoing_value() {
        let t0 = Instant::now();
        let mut fade = ParamFade::new(0u8, D);
        fade.set(2, t0);
        fade.set(3, t0 + Duration::from_millis(40));
        let shot = painted(&mut fade, t0 + Duration::from_millis(60));
        assert_eq!(
            shot.first().map(|v| v.0),
            Some(0),
            "the outgoing value is the first one"
        );
        assert_eq!(
            shot.get(1).map(|v| v.0),
            Some(3),
            "only the incoming value is the latest"
        );
        assert_eq!(fade.value(), 3);
        assert!(
            !fade.is_animating(t0 + D),
            "the schedule ends from the first start time"
        );
    }

    /// Returning to the previous value cancels the fade (0 → 1 → 0).
    #[test]
    fn returning_to_the_previous_value_cancels_the_fade() {
        let t0 = Instant::now();
        let mut fade = ParamFade::new(0u8, D);
        fade.set(1, t0);
        fade.set(0, t0 + Duration::from_millis(30));
        assert!(!fade.is_animating(t0 + Duration::from_millis(30)));
        assert_eq!(
            painted(&mut fade, t0 + Duration::from_millis(30)),
            vec![(0, 255)]
        );
    }

    /// A zero duration (reduce) never animates.
    #[test]
    fn zero_duration_never_animates() {
        let t0 = Instant::now();
        let mut fade = ParamFade::new(0u8, Duration::ZERO);
        fade.set(1, t0);
        assert!(!fade.is_animating(t0));
        assert_eq!(painted(&mut fade, t0), vec![(1, 255)]);
    }
}
