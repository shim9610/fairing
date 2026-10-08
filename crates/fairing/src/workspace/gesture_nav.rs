//! The gesture navigation's two motions, as values: the **lift** — the screen on show
//! following a finger up from the bottom edge, shrinking as it rises — and the **quick switch** —
//! two tasks' screens sliding sideways with a finger along the home indicator. What they draw and
//! what they end in is `Workspace`'s; this file only works the numbers out.
//!
//! # The lift
//!
//! The point of the screen the finger went down on stays under the finger: the screen scales
//! about the press point and moves with the finger's offset from it. The scale follows the
//! finger's height alone, from 1 down to the overview's card scale over `travel` — so a lift held
//! into the overview carries on into its card. Let go short of home, it springs back down, the
//! offset going back in step with the scale, so the screen comes home along one path rather than
//! shrinking back and sliding back on two clocks.
//!
//! # The quick switch
//!
//! The screen on show slides with the finger and the next task's slides in beside it, from the
//! side the finger is moving away from: right brings the task used before it, left the one used
//! after it. Which tasks those are is fixed for a run of switches ([`QuickOrder`]), so swiping
//! back returns where it came from instead of bouncing between the last two.

use super::overview::CARD_SCALE;
use super::InstanceId;
use crate::motion::{Animated, Spring};
use egui::{Pos2, Rect, Vec2};

/// The smallest a lifted screen gets: the overview's card scale, so a lift held into the
/// overview hands over to it without a jump.
pub(crate) const LIFT_MIN_SCALE: f32 = CARD_SCALE;

/// Where there is no task to slide in, the slide follows the finger this much — enough to say
/// there is nothing that way.
const SWITCH_RESIST: f32 = 0.25;

/// The lift.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lift {
    /// Where the finger went down — the point of the screen that stays under it.
    press: Pos2,
    /// The finger's offset from `press` (du), as last moved.
    offset: Vec2,
    /// How far up, 0 → 1 over `travel`.
    p: Animated<f32>,
    /// How far the finger rises for `p` to reach 1 (du).
    travel: f32,
    /// The offset and `p` it was let go at — the offset goes back with `p` from there.
    released: Option<(Vec2, f32)>,
}

impl Lift {
    /// A lift from `press`, reaching the card scale `travel` du up.
    pub(crate) fn new(press: Pos2, travel: f32) -> Self {
        let mut p = Animated::new(0.0);
        p.drag(0.0, 0.0);
        Self {
            press,
            offset: Vec2::ZERO,
            p,
            travel: if travel.is_finite() {
                travel.max(1.0)
            } else {
                1.0
            },
            released: None,
        }
    }

    /// The finger is `offset` from where it went down, rising at `velocity_up` (du/s).
    pub(crate) fn drag(&mut self, offset: Vec2, velocity_up: f32) {
        if self.released.is_some() {
            return;
        }
        self.offset = offset;
        let p = (-offset.y / self.travel).clamp(0.0, 1.0);
        self.p.drag(p, velocity_up / self.travel);
    }

    /// How far up, `0..=1`.
    pub(crate) fn progress(&self) -> f32 {
        self.p.value().clamp(0.0, 1.0)
    }

    /// The screen's scale now.
    pub(crate) fn scale(&self) -> f32 {
        1.0 - (1.0 - LIFT_MIN_SCALE) * self.progress()
    }

    /// What the screen scales about: the press point.
    pub(crate) fn pivot(&self) -> Pos2 {
        self.press
    }

    /// How far the screen is moved, after scaling: the finger's offset while it is down; once let
    /// go, that offset in step with `p` on its way back.
    pub(crate) fn shift(&self) -> Vec2 {
        match self.released {
            None => self.offset,
            Some((offset, p0)) if p0 > 1e-4 => offset * (self.progress() / p0).clamp(0.0, 1.0),
            Some(_) => Vec2::ZERO,
        }
    }

    /// The lifted screen's rect, for a pane at `pane`.
    pub(crate) fn rect(&self, pane: Rect) -> Rect {
        let k = self.scale();
        let min = self.press + (pane.min - self.press) * k + self.shift();
        Rect::from_min_size(min, pane.size() * k)
    }

    /// Let go short of home: back down, springing (at once under `reduce`).
    pub(crate) fn release(&mut self, spring: Spring, reduce: bool) {
        if self.released.is_some() {
            return;
        }
        self.released = Some((self.offset, self.progress()));
        if reduce {
            self.p.snap(0.0);
        } else {
            self.p.release_scaled(0.0, spring, 1.0 / self.travel);
        }
    }

    /// Whether it has been let go.
    pub(crate) fn is_released(&self) -> bool {
        self.released.is_some()
    }

    /// Advance. `true` once it has been let go and is back down.
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        let moving = self.p.tick(dt);
        self.released.is_some() && !moving
    }

    /// Whether it moves on its own (a release springing back).
    pub(crate) fn is_moving(&self) -> bool {
        self.p.is_animating()
    }
}

/// The quick switch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Switch {
    /// The task sliding out — on show when the slide began (its root's id).
    pub(crate) from: InstanceId,
    /// The task a slide to the right brings in: the one used before.
    older: Option<InstanceId>,
    /// The task a slide to the left brings in: the one used after.
    newer: Option<InstanceId>,
    /// The slide as a fraction of the pane's width, positive to the right.
    p: Animated<f32>,
    /// The width `p` is a fraction of (du).
    width: f32,
    /// After the release: through to the task coming in (`true`) or back (`false`).
    confirmed: Option<bool>,
}

impl Switch {
    /// A switch away from `from`, with the tasks either side of it in the run's order.
    pub(crate) fn new(
        from: InstanceId,
        older: Option<InstanceId>,
        newer: Option<InstanceId>,
        width: f32,
    ) -> Self {
        let mut p = Animated::new(0.0);
        p.drag(0.0, 0.0);
        Self {
            from,
            older,
            newer,
            p,
            width: if width.is_finite() {
                width.max(1.0)
            } else {
                1.0
            },
            confirmed: None,
        }
    }

    /// The task on the side `sign` slides in from: `> 0` (the finger going right) the older.
    fn side(&self, sign: f32) -> Option<InstanceId> {
        if sign > 0.0 {
            self.older
        } else if sign < 0.0 {
            self.newer
        } else {
            None
        }
    }

    /// The finger is `offset` du along the indicator from where it went down, at `velocity` du/s.
    /// Toward a side with no task, the slide gives only a little.
    pub(crate) fn drag(&mut self, offset: f32, velocity: f32) {
        if self.confirmed.is_some() {
            return;
        }
        let damp = if self.side(offset).is_some() {
            1.0
        } else {
            SWITCH_RESIST
        };
        let p = (offset * damp / self.width).clamp(-1.0, 1.0);
        self.p.drag(p, velocity * damp / self.width);
    }

    /// The slide now, as a fraction of the width.
    pub(crate) fn progress(&self) -> f32 {
        self.p.value()
    }

    /// The task coming in at this point of the slide.
    pub(crate) fn incoming(&self) -> Option<InstanceId> {
        self.side(self.p.value())
    }

    /// Let go: through, where `through` and there is a task that way, or back.
    pub(crate) fn release(&mut self, through: bool, spring: Spring, reduce: bool) {
        if self.confirmed.is_some() {
            return;
        }
        let sign = self.p.value().signum();
        let through = through && self.side(sign).is_some();
        self.confirmed = Some(through);
        let target = if through { sign } else { 0.0 };
        if reduce {
            self.p.snap(target);
        } else {
            self.p.release_scaled(target, spring, 1.0 / self.width);
        }
    }

    /// The release's outcome — `None` while the finger has it.
    pub(crate) fn confirmed(&self) -> Option<bool> {
        self.confirmed
    }

    /// Advance. `true` once it has been let go and has landed.
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        let moving = self.p.tick(dt);
        self.confirmed.is_some() && !moving
    }

    /// Whether it moves on its own (a release landing).
    pub(crate) fn is_moving(&self) -> bool {
        self.p.is_animating()
    }

    /// The x offset of the screen sliding out, and of the one sliding in, for the width it was
    /// started over.
    pub(crate) fn offsets(&self) -> (f32, f32) {
        let p = self.p.value();
        let out = p * self.width;
        (out, out - p.signum() * self.width)
    }
}

/// The order a run of quick switches walks: the tasks by when they were last used, as
/// they stood when the run began.
#[derive(Debug, Clone, Default)]
pub(crate) struct QuickOrder {
    /// The tasks' root ids, the one used last first.
    keys: Vec<InstanceId>,
    /// Where in `keys` the run stands.
    at: usize,
}

impl QuickOrder {
    /// A run starting on `keys[0]`.
    pub(crate) fn new(keys: Vec<InstanceId>) -> Self {
        Self { keys, at: 0 }
    }

    /// The task the run stands on.
    pub(crate) fn current(&self) -> Option<InstanceId> {
        self.keys.get(self.at).copied()
    }

    /// The nearest live task either side of where the run stands: (older, newer).
    pub(crate) fn neighbours(
        &self,
        live: impl Fn(InstanceId) -> bool,
    ) -> (Option<InstanceId>, Option<InstanceId>) {
        let older = self
            .keys
            .iter()
            .skip(self.at + 1)
            .copied()
            .find(|k| live(*k));
        let newer = self
            .keys
            .iter()
            .take(self.at)
            .rev()
            .copied()
            .find(|k| live(*k));
        (older, newer)
    }

    /// The run moved on to `key`.
    pub(crate) fn step_to(&mut self, key: InstanceId) {
        if let Some(at) = self.keys.iter().position(|k| *k == key) {
            self.at = at;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Lift, QuickOrder, Switch, LIFT_MIN_SCALE};
    use crate::motion::Spring;
    use crate::workspace::InstanceId;
    use egui::{pos2, vec2, Rect};

    fn pane() -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))
    }

    /// The press point stays under the finger: wherever the finger takes it, the point of the
    /// screen it went down on is where the finger is, and the scale goes by the height alone.
    #[test]
    fn the_point_pressed_stays_under_the_finger() {
        let press = pos2(300.0, 590.0);
        let mut lift = Lift::new(press, 200.0);
        lift.drag(vec2(40.0, -100.0), 0.0);
        assert!((lift.progress() - 0.5).abs() < 1e-5);
        assert!((lift.scale() - (1.0 - (1.0 - LIFT_MIN_SCALE) * 0.5)).abs() < 1e-5);
        let r = lift.rect(pane());
        // The press point's place in the screen, carried by the screen's own rect.
        let u = (press - pane().min) / pane().size();
        let carried = r.min + u * r.size();
        assert!(
            (carried - pos2(340.0, 490.0)).length() < 1e-3,
            "{carried:?}"
        );
        // All the way up and past it: the card scale, no smaller.
        lift.drag(vec2(0.0, -500.0), 0.0);
        assert!((lift.scale() - LIFT_MIN_SCALE).abs() < 1e-5);
        // Below the press it is not lifted at all.
        lift.drag(vec2(0.0, 30.0), 0.0);
        assert!((lift.scale() - 1.0).abs() < 1e-5);
    }

    /// Let go, it comes back down along one path: the offset shrinks with `p`, so at half way
    /// back it is half the offset, and at rest both are gone.
    #[test]
    fn a_release_brings_the_offset_back_with_the_scale() {
        let mut lift = Lift::new(pos2(400.0, 590.0), 200.0);
        lift.drag(vec2(60.0, -100.0), 0.0);
        lift.release(Spring::default(), false);
        assert!(lift.is_released() && lift.is_moving());
        let mut done = false;
        for _ in 0..600 {
            let p = lift.progress();
            let shift = lift.shift();
            assert!(
                (shift.x - 60.0 * p / 0.5).abs() < 1e-3,
                "{shift:?} at p {p}"
            );
            if lift.tick(1.0 / 60.0) {
                done = true;
                break;
            }
        }
        assert!(done, "it came back down");
        assert!(lift.shift().length() < 1e-3 && (lift.scale() - 1.0).abs() < 1e-5);
        // A finger after the release changes nothing.
        lift.drag(vec2(0.0, -150.0), 0.0);
        assert!((lift.scale() - 1.0).abs() < 1e-5);
    }

    /// Right brings the older task in from the left, left the newer from the right; toward a side
    /// with nothing, the slide gives only a little and cannot go through.
    #[test]
    fn a_slide_brings_in_the_task_on_its_side() {
        let (a, b) = (InstanceId(1), InstanceId(2));
        let mut s = Switch::new(a, Some(b), None, 800.0);
        s.drag(200.0, 0.0);
        assert_eq!(s.incoming(), Some(b));
        let (out, inn) = s.offsets();
        assert!((out - 200.0).abs() < 1e-3 && (inn + 600.0).abs() < 1e-3);
        s.drag(-200.0, 0.0);
        assert_eq!(s.incoming(), None);
        assert!(
            (s.progress() + 0.0625).abs() < 1e-5,
            "a quarter of the finger"
        );
        s.release(true, Spring::default(), true);
        assert_eq!(s.confirmed(), Some(false), "nothing that way to go to");
        assert!(s.tick(1.0 / 60.0));
        assert!(s.progress().abs() < 1e-6);
    }

    /// Through: it lands a whole width over.
    #[test]
    fn a_switch_let_go_through_lands_a_width_over() {
        let (a, b) = (InstanceId(1), InstanceId(2));
        let mut s = Switch::new(a, Some(b), None, 800.0);
        s.drag(300.0, 0.0);
        s.release(true, Spring::default(), false);
        let mut landed = false;
        for _ in 0..600 {
            if s.tick(1.0 / 60.0) {
                landed = true;
                break;
            }
        }
        assert!(landed && s.confirmed() == Some(true));
        assert!((s.progress() - 1.0).abs() < 1e-5);
    }

    /// A run keeps its order: older and newer are either side of where it stands, skipping a
    /// task that has gone.
    #[test]
    fn a_run_walks_its_order_and_skips_the_gone() {
        let (a, b, c) = (InstanceId(1), InstanceId(2), InstanceId(3));
        let mut run = QuickOrder::new(vec![a, b, c]);
        assert_eq!(run.current(), Some(a));
        assert_eq!(run.neighbours(|_| true), (Some(b), None));
        run.step_to(b);
        assert_eq!(run.neighbours(|_| true), (Some(c), Some(a)));
        assert_eq!(run.neighbours(|k| k != c), (None, Some(a)));
        run.step_to(c);
        assert_eq!(run.neighbours(|k| k != b), (None, Some(a)));
    }
}
