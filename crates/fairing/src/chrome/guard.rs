//! Driving the chrome policy: the `edge_guard` block mask, the
//! emergency gesture (a 2 s long press in a top corner → `chrome.emergency`), and `keep_awake`
//! → `DisplayBackend::set_idle_inhibit`.
//!
//! The shell hands over the focused screen's [`ChromePolicy`] and the gesture engine's [`hold`]
//! in frame stage 5. This module only decides; the shell acts (events, `launch`). The one
//! exception is the progress ring, which [`PolicyDriver::ui`] draws on its own layer (so the
//! state need not be copied into the shell).
//!
//! **The blocking rules** — [`PolicyDriver::blocked_edges`]:
//!
//! | Policy | Blocked | What is left |
//! |---|---|---|
//! | `edge_guard = true` | `EdgeMask::ALL` (regardless of `allow_peek`) | The emergency gesture only (2 s in a top corner) |
//! | `allow_peek = false` + `status_bar = Hide` | `Top` | Left and right back, and the bottom |
//! | `allow_peek = false` + `nav_bar = Hide` | `Bottom` | — |
//! | Anything else (`Show`, `Overlay`, or `allow_peek = true`) | Nothing | All of it |
//!
//! `edge_guard` **overrides** `allow_peek`: a full lock is only a lock if it also stops peeking
//! at a hidden bar. Conversely a hidden bar with `allow_peek = true` keeps its
//! edge alive so the shade can be pulled — that decision belongs to the overlay; this only
//! produces the mask.
//!
//! [`hold`]: crate::gesture::GestureEngine::hold

use crate::gesture::{Edge, EdgeMask};
use crate::icons::parametric::{progress_ring, ParamStyle};
use crate::screen::{BarMode, ChromePolicy};
use crate::services::DisplayBackend;
use crate::theme::{ColorRole, Theme};
use egui::{Id, LayerId, Order, Pos2, Rect};
use std::time::{Duration, Instant};

/// The emergency gate's name.
pub const EMERGENCY_GATE: &str = "chrome.emergency";

/// The name of the layer the progress ring is drawn on. Not an `Area` but a draw-only layer via
/// [`egui::Context::layer_painter`] — the ring takes no input (the corner press is the gesture
/// engine's).
pub(super) const RING_LAYER: &str = "fairing.chrome.guard";

/// The progress ring's diameter (px). Larger than a finger (≈ 48 px) so it wraps the press.
pub(super) const RING_SIZE: f32 = 56.0;

/// The progress ring's stroke (px).
const RING_STROKE: f32 = 3.0;

/// The cancel reverse's length (A7's long-press contract, "80 ms reverse on cancel").
/// Leaving the corner or lifting the finger takes the ring to 0 over this, **from wherever it
/// is**.
pub(super) const RING_CANCEL: Duration = Duration::from_millis(80);

/// This frame's decision.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct GuardFrame {
    /// The edges the engine must not start a swipe on.
    pub(crate) blocked: EdgeMask,
    /// The emergency gesture completed on this frame (once).
    pub(crate) emergency: bool,
    /// The emergency gesture's progress, 0..=1 (for display). Lifting the finger rewinds it to 0
    /// over `RING_CANCEL` (80 ms) — it is not 0 during that.
    pub(crate) emergency_progress: f32,
}

/// The policy driver's state.
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct PolicyDriver {
    emergency_ms: Duration,
    corner_px: f32,
    emergency_fired: bool,
    keep_awake: Option<bool>,
    /// The ring's progress (including the rewind).
    progress: f32,
    /// The ring's centre (the corner last pressed).
    at: Option<Pos2>,
    /// The rewind rate (progress per second). 0 means it is not rewinding.
    rewind_rate: f32,
    /// The time of the last [`PolicyDriver::update`] (the rewind's dt).
    last: Option<Instant>,
}

impl PolicyDriver {
    /// `[gesture] emergency_ms`·`emergency_corner_px`.
    #[must_use]
    pub fn new(emergency_ms: Duration, corner_px: f32) -> Self {
        Self {
            emergency_ms,
            corner_px,
            emergency_fired: false,
            keep_awake: None,
            progress: 0.0,
            at: None,
            rewind_rate: 0.0,
            last: None,
        }
    }

    /// Policy → the block mask. The rule table is in the module docs.
    #[must_use]
    pub(crate) fn blocked_edges(policy: &ChromePolicy) -> EdgeMask {
        if policy.edge_guard {
            // A full lock blocks the peek too — it does not look at `allow_peek`.
            return EdgeMask::ALL;
        }
        let mut mask = EdgeMask::NONE;
        if !policy.allow_peek {
            if policy.status_bar == BarMode::Hide {
                mask = mask.with(Edge::Top);
            }
            if policy.nav_bar == BarMode::Hide {
                mask = mask.with(Edge::Bottom);
            }
        }
        mask
    }

    /// Frame stage 5. `hold` is the engine's still press (the position and the elapsed time).
    ///
    /// The emergency gesture is alive even with `[gesture] enabled = false` — the
    /// engine keeps emitting [`hold`](crate::gesture::GestureEngine::hold) alone. The decision is
    /// independent of `edge_guard`: on an unlocked screen, 2 s in a corner takes the same path
    /// (the emergency gesture is a safety device against a wedged screen).
    pub(crate) fn update(
        &mut self,
        policy: &ChromePolicy,
        screen: Rect,
        hold: Option<(Pos2, Duration)>,
        now: Instant,
    ) -> GuardFrame {
        let dt = self.last.map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32()
        });
        self.last = Some(now);
        let blocked = Self::blocked_edges(policy);
        let mut frame = GuardFrame {
            blocked,
            ..GuardFrame::default()
        };
        match hold {
            Some((pos, elapsed)) if crate::gesture::in_top_corner(screen, pos, self.corner_px) => {
                let t = (elapsed.as_secs_f32() / self.emergency_ms.as_secs_f32().max(1e-3))
                    .clamp(0.0, 1.0);
                self.progress = t;
                self.at = Some(pos);
                self.rewind_rate = 0.0;
                if t >= 1.0 && !self.emergency_fired {
                    self.emergency_fired = true;
                    frame.emergency = true;
                    log::debug!(
                        "emergency gesture completed at ({:.0}, {:.0}) - {EMERGENCY_GATE}",
                        pos.x,
                        pos.y
                    );
                }
            }
            other => {
                if other.is_none() {
                    self.emergency_fired = false;
                }
                self.rewind(dt);
            }
        }
        frame.emergency_progress = self.progress;
        frame
    }

    /// The corner was left or the finger lifted: rewind the ring to 0 over [`RING_CANCEL`],
    /// **from wherever it is** (A7 is 80 ms regardless of the starting value — the
    /// same contract as `BigButton`'s ring).
    fn rewind(&mut self, dt: f32) {
        if self.progress <= 0.0 {
            self.at = None;
            self.rewind_rate = 0.0;
            return;
        }
        if self.rewind_rate <= 0.0 {
            self.rewind_rate = self.progress / RING_CANCEL.as_secs_f32();
        }
        self.progress = self
            .progress
            .mul_add(1.0, -(self.rewind_rate * dt))
            .max(0.0);
        if self.progress <= 0.0 {
            self.at = None;
            self.rewind_rate = 0.0;
        }
    }

    /// The emergency gesture's progress, 0..=1 (including the rewind). For an integrator drawing the ring themselves.
    #[must_use]
    pub fn emergency_progress(&self) -> f32 {
        self.progress
    }

    /// The centre to draw the ring at (the corner pressed). `None` at zero progress.
    #[must_use]
    pub fn emergency_at(&self) -> Option<Pos2> {
        self.at.filter(|_| self.progress > 0.0)
    }

    /// Whether the ring is moving (including the rewind) — for the repaint policy.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.progress > 0.0
    }

    /// The emergency gesture's progress ring (A7's long-press contract: linear
    /// progress, an 80 ms reverse on cancel).
    ///
    /// Drawn straight onto a draw-only layer (`RING_LAYER`, `Order::Foreground`) — not an
    /// `Area`, so it does not intercept input. While rewinding it requests its own repaints
    /// (there is no finger, so nothing else would).
    pub fn ui(&self, ctx: &egui::Context, theme: &Theme) {
        let Some(center) = self.emergency_at() else {
            return;
        };
        let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::new(RING_LAYER)));
        let style = ParamStyle {
            color: theme.color(ColorRole::Primary),
            muted: theme.color(ColorRole::Muted),
            danger: theme.color(ColorRole::Danger),
            stroke_px: RING_STROKE,
        };
        let rect = Rect::from_center_size(center, egui::Vec2::splat(RING_SIZE));
        progress_ring(&painter, rect, self.progress, &style);
        if self.progress < 1.0 {
            ctx.request_repaint();
        }
    }

    /// Pass `keep_awake` to the backend only on the frame it changed. `Some(on)` if it was
    /// passed.
    ///
    /// The shell logs the transition (stage 5) — logging here too would print the line twice.
    pub(crate) fn apply_keep_awake(
        &mut self,
        policy: &ChromePolicy,
        display: &mut dyn DisplayBackend,
    ) -> Option<bool> {
        if self.keep_awake == Some(policy.keep_awake) {
            return None;
        }
        self.keep_awake = Some(policy.keep_awake);
        display.set_idle_inhibit(policy.keep_awake);
        Some(policy.keep_awake)
    }

    /// The last `keep_awake` passed on.
    #[must_use]
    pub fn keep_awake(&self) -> Option<bool> {
        self.keep_awake
    }
}

#[cfg(test)]
mod tests {
    use super::{GuardFrame, PolicyDriver};
    use crate::gesture::{Edge, EdgeMask};
    use crate::screen::{BarMode, ChromePolicy};
    use egui::{pos2, Rect};
    use std::time::{Duration, Instant};

    fn screen() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1024.0, 600.0))
    }

    fn driver() -> PolicyDriver {
        PolicyDriver::new(Duration::from_secs(2), 64.0)
    }

    /// `edge_guard` overrides `allow_peek`, and `allow_peek = false` blocks only
    /// the edges of **hidden** bars.
    #[test]
    fn edge_guard_beats_allow_peek() {
        let guarded = ChromePolicy {
            edge_guard: true,
            allow_peek: true,
            ..ChromePolicy::default()
        };
        assert_eq!(PolicyDriver::blocked_edges(&guarded), EdgeMask::ALL);

        let hidden = ChromePolicy {
            status_bar: BarMode::Hide,
            nav_bar: BarMode::Hide,
            allow_peek: false,
            ..ChromePolicy::default()
        };
        let mask = PolicyDriver::blocked_edges(&hidden);
        assert!(mask.contains(Edge::Top) && mask.contains(Edge::Bottom));
        assert!(!mask.contains(Edge::Left) && !mask.contains(Edge::Right));

        let peeking = ChromePolicy {
            allow_peek: true,
            ..hidden
        };
        assert!(PolicyDriver::blocked_edges(&peeking).is_empty());

        // A visible bar is not blocked even with `allow_peek = false` (the edge = the bar itself).
        let shown = ChromePolicy {
            status_bar: BarMode::Show,
            nav_bar: BarMode::Overlay,
            allow_peek: false,
            ..ChromePolicy::default()
        };
        assert!(PolicyDriver::blocked_edges(&shown).is_empty());
    }

    /// 2 s in a corner completes exactly once. The progress is linear.
    #[test]
    fn emergency_fires_once_after_two_seconds() {
        let mut guard = driver();
        let policy = ChromePolicy::default();
        let t0 = Instant::now();
        let corner = pos2(1010.0, 10.0);
        let at = |ms: u64| t0 + Duration::from_millis(ms);

        let half = guard.update(
            &policy,
            screen(),
            Some((corner, Duration::from_secs(1))),
            at(1000),
        );
        assert!(!half.emergency);
        assert!((half.emergency_progress - 0.5).abs() < 1e-6);

        let done = guard.update(
            &policy,
            screen(),
            Some((corner, Duration::from_secs(2))),
            at(2000),
        );
        assert!(done.emergency && (done.emergency_progress - 1.0).abs() < 1e-6);
        let again = guard.update(
            &policy,
            screen(),
            Some((corner, Duration::from_millis(2100))),
            at(2100),
        );
        assert!(!again.emergency, "once only");

        // On release it arms again.
        let _ = guard.update(&policy, screen(), None, at(2200));
        let _ = guard.update(&policy, screen(), None, at(2400));
        let re = guard.update(
            &policy,
            screen(),
            Some((corner, Duration::from_secs(2))),
            at(2500),
        );
        assert!(
            re.emergency,
            "letting go and pressing again fires it once more"
        );
    }

    /// A press outside the corner does not progress. Leaving mid-progress reverses to 0 over 80 ms.
    #[test]
    fn ring_rewinds_after_leaving_the_corner() {
        let mut guard = driver();
        let policy = ChromePolicy::default();
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);

        let middle = guard.update(
            &policy,
            screen(),
            Some((pos2(500.0, 300.0), Duration::from_secs(1))),
            at(1000),
        );
        assert_eq!(
            middle,
            GuardFrame::default(),
            "outside the corner there is no progress"
        );

        let _ = guard.update(
            &policy,
            screen(),
            Some((pos2(10.0, 10.0), Duration::from_millis(1600))),
            at(2000),
        );
        assert!(guard.emergency_at().is_some());
        let rewinding = guard.update(&policy, screen(), None, at(2040));
        assert!(
            (rewinding.emergency_progress - 0.4).abs() < 1e-3,
            "40 ms from 0.8 = wound back by half: {}",
            rewinding.emergency_progress
        );
        let gone = guard.update(&policy, screen(), None, at(2081));
        assert!(gone.emergency_progress.abs() < 1e-6 && guard.emergency_at().is_none());
        assert!(!guard.is_animating());
    }

    /// It reaches the backend only on the frame it changed.
    #[test]
    fn keep_awake_only_on_change() {
        let mut guard = driver();
        let mut display = crate::services::mock::MockDisplay::default();
        let awake = ChromePolicy {
            keep_awake: true,
            ..ChromePolicy::default()
        };
        assert_eq!(guard.keep_awake(), None);
        assert_eq!(guard.apply_keep_awake(&awake, &mut display), Some(true));
        assert_eq!(guard.apply_keep_awake(&awake, &mut display), None);
        assert_eq!(
            guard.apply_keep_awake(&ChromePolicy::default(), &mut display),
            Some(false)
        );
        assert_eq!(guard.keep_awake(), Some(false));
    }
}
