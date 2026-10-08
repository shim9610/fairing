//! The gesture engine. egui only unifies pointers and touch, so higher-level
//! gestures like an "edge swipe" or a "long press" are recognised here, reading `ctx.input()`
//! every frame.
//!
//! The contract:
//! - Recognition order: a press in an edge zone → movement inward past `slop` →
//!   [`Phase::Started`]. From then on the shield is up. Short of that it passes through as an
//!   ordinary tap ([`Recognizer::Passed`]).
//! - Progress is **the inward-axis movement from the press point (px)** — the slop is not
//!   subtracted (A1's "y = 146 at frame 6" is this contract).
//! - The velocity is `pointer.velocity()` (egui smooths it over a 100 ms window). It is valid on
//!   the release frame too (`interact_pos` also survives to the release frame — egui 0.36.1
//!   `input_state`).
//! - **An edge tap**: released within the slop in an edge zone, the engine emits nothing. The
//!   shield only goes up past the slop, so that tap reaches the widget underneath (the status
//!   bar → `tap_opens_shade`). The tap window (`tap`, 300 ms) is the widget's rule; the engine
//!   does not decide taps by time.
//! - [`Gesture::Swipe`]: pressed outside an edge (or in an edge zone but not inward) and
//!   released at a velocity ≥ `fling_px_s`, once. [`Gesture::SwipeHold`]: during an edge swipe,
//!   the finger standing still within `slop / 2` for `hold` (150 ms) — once a pause, so a swipe
//!   that stops too early for its consumer can still stop again further on (the recent screens
//!   in the bottom gesture navigation). It comes instead of `Moved` on that frame (the
//!   finger is still, so the consumer loses no movement).
//! - Page swipes and swipe-to-dismiss on notifications are handled by the widgets with their own
//!   `drag_delta`. The engine only does global gestures.
//! - [`Gesture::EdgeSlide`]: pressed in the zone of an edge listed in `GestureFrame::slide_edges`
//!   and moved **along** it past the slop, more than inward — the home indicator's left and right.
//!   Only on the edges asked for: a slide along the left edge is someone scrolling.
//! - [`Gesture::Region`]: pressed in a gesture region's zone (`GestureFrame::regions`).
//!   The press is the region's at once and to its release, reported every frame from `Started` on
//!   the press, so no edge swipe, slide, long press or fling comes of it. The zones are looked up
//!   before the edge zones, the topmost first. A still press in a region is still a held press
//!   ([`GestureEngine::hold`]), so the emergency gesture works through one. The gesture handles
//!   are regions the shell places; the shell keeps every press in a zone from what is
//!   under it.
//! - Blocking: a swipe does not start on an edge listed in `GestureFrame::blocked`
//!   (`ChromePolicy::edge_guard`). The shell sees the emergency gesture through
//!   `GestureEngine::hold`.
//! - Priority: a consumer cuts a lower one off with
//!   `GestureEngine::cancel`.
//! - Zero heap allocations per frame.
//!
//! Headless testing: a `PointerButton` / `PointerMoved` sequence in `RawInput.events`
//! ([`crate::testing::Harness::drag`]).

mod edge;
mod handle;
mod recognizers;
mod region;
pub(crate) mod shield;

pub use edge::{edge_at, in_top_corner, Edge, EdgeMask};
pub(crate) use handle::HandleRegion;
pub use handle::{
    GestureHandle, HandleAction, HandleDirection, HandleEnd, HandleGesture, HandleLook,
    HandlePainter, HandleSwipe, MAX_HANDLES_A_SIDE,
};
pub use recognizers::{Dir, DragSample, Gesture, Phase, Recognizer};
pub use region::{
    GestureRegion, RegionCx, RegionLook, RegionPlaceCx, RegionTouch, MAX_GESTURE_REGIONS,
};
pub(crate) use region::{RegionOut, RegionZones};

use egui::{Pos2, Rect, Vec2};
use std::time::{Duration, Instant};

/// The engine's tuning values (gathered from `[motion]`, `[gesture]` and `theme.metrics`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GestureTuning {
    /// The drag detection slop (px).
    pub(crate) slop_px: f32,
    /// The tap window.
    pub(crate) tap: Duration,
    /// Long press.
    pub(crate) long_press: Duration,
    /// How long standing still after a swipe counts as held.
    pub(crate) hold: Duration,
    /// The fling velocity threshold (px/s).
    pub(crate) fling_px_s: f32,
    /// Global gestures on.
    pub(crate) enabled: bool,
}

impl Default for GestureTuning {
    fn default() -> Self {
        Self {
            slop_px: 12.0,
            tap: Duration::from_millis(300),
            long_press: Duration::from_millis(500),
            hold: Duration::from_millis(150),
            fling_px_s: 800.0,
            enabled: true,
        }
    }
}

/// The input to frame stage 5.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GestureFrame {
    /// The screen.
    pub(crate) screen: Rect,
    /// The edge zones (`Layout::edge_zones`).
    pub(crate) edge_zones: [Rect; 4],
    /// The edges no swipe may start on this frame (policy).
    pub(crate) blocked: EdgeMask,
    /// The edges a slide along them is recognised on ([`Gesture::EdgeSlide`]).
    pub(crate) slide_edges: EdgeMask,
    /// The gesture regions' zones: a press in one is the region's, to its release.
    pub(crate) regions: RegionZones,
    /// The shell's time.
    pub(crate) now: Instant,
}

/// The gesture engine.
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct GestureEngine {
    tuning: GestureTuning,
    rec: Recognizer,
    current: Option<Gesture>,
    sample: Option<DragSample>,
    screen: Rect,
    /// The anchor for deciding stillness during an edge swipe (the position, and the elapsed time then). Refreshed on leaving `slop / 2`.
    still: Option<(Pos2, Duration)>,
    /// Whether `SwipeHold` has already been emitted for this pause.
    hold_reported: bool,
}

impl GestureEngine {
    /// A new engine.
    #[must_use]
    pub(crate) fn new(tuning: GestureTuning) -> Self {
        Self {
            tuning,
            rec: Recognizer::Idle,
            current: None,
            sample: None,
            screen: Rect::NOTHING,
            still: None,
            hold_reported: false,
        }
    }

    /// The tuning values.
    #[must_use]
    pub(crate) fn tuning(&self) -> &GestureTuning {
        &self.tuning
    }

    /// Replace the tuning (`motion_lab`).
    pub(crate) fn set_tuning(&mut self, tuning: GestureTuning) {
        self.tuning = tuning;
    }

    /// Frame stage 5: read the pointer state and return this frame's gesture. The same value can
    /// be queried until the end of the frame with [`GestureEngine::current`].
    pub(crate) fn update(&mut self, ctx: &egui::Context, frame: &GestureFrame) -> Option<Gesture> {
        self.current = None;
        self.screen = frame.screen;
        // `enabled = false`: no gesture (an edge swipe, a long press, a fling) is raised, but **the press
        // tracking is kept** — the emergency gesture, decided through [`GestureEngine::hold`], is
        // a safety device and stays. Every edge is treated as blocked.
        let disabled = GestureFrame {
            blocked: EdgeMask::ALL,
            regions: RegionZones::NONE,
            ..*frame
        };
        let frame = if self.tuning.enabled {
            frame
        } else {
            &disabled
        };
        let input = ctx.input(|i| PointerFrame {
            down: i.pointer.primary_down(),
            pressed: i.pointer.primary_pressed(),
            released: i.pointer.primary_released(),
            press: crate::drag::press_point(i),
            pos: i.pointer.interact_pos(),
            velocity: i.pointer.velocity(),
            dt: Duration::from_secs_f32(i.stable_dt.clamp(0.0, crate::motion::MAX_DT)),
        });
        self.rec = match self.rec {
            Recognizer::Idle => self.on_idle(&input, frame),
            Recognizer::Pressed {
                origin,
                at,
                edge,
                long_reported,
            } => self.on_pressed(&input, frame, (origin, at), edge, long_reported),
            Recognizer::EdgeSwipe { edge, origin } => self.on_swipe(&input, edge, origin),
            Recognizer::EdgeSlide { edge, origin } => self.on_slide(&input, edge, origin),
            Recognizer::Region {
                region,
                origin,
                at,
                moved,
            } => self.on_region(&input, frame, region, (origin, at), moved),
            Recognizer::Passed { origin } => self.on_passed(&input, origin),
        };
        if !self.tuning.enabled {
            self.current = None;
        }
        self.current
    }

    fn on_idle(&mut self, input: &PointerFrame, frame: &GestureFrame) -> Recognizer {
        self.still = None;
        self.hold_reported = false;
        // From where the finger came down, not from where this frame left it: a press and the
        // first move of a quick pull can arrive in one frame, and the moved point may already be
        // past the edge zone the press began in.
        let (true, Some(origin)) = (input.pressed, input.press.or(input.pos)) else {
            self.sample = None;
            return Recognizer::Idle;
        };
        let pos = input.pos.unwrap_or(origin);
        self.sample = Some(sample(origin, pos, input.velocity, Duration::ZERO));
        // A region's zone first: the press is the region's, whatever edge it is on.
        if let Some(region) = frame.regions.at(origin) {
            return self.region_pressed(input, frame, region, origin, pos);
        }
        let edge = edge_at(&frame.edge_zones, origin).filter(|e| !frame.blocked.contains(*e));
        Recognizer::Pressed {
            origin,
            at: frame.now,
            edge,
            long_reported: false,
        }
    }

    /// A press in a region's zone: `Started` on this frame, or `Ended` already for a press let go
    /// within it (a quick tap on a slow frame), which the shell tells the region as both.
    fn region_pressed(
        &mut self,
        input: &PointerFrame,
        frame: &GestureFrame,
        region: u8,
        origin: Pos2,
        pos: Pos2,
    ) -> Recognizer {
        let moved = (pos - origin).length() > self.tuning.slop_px;
        let phase = if input.released {
            Phase::Ended
        } else if !input.down {
            Phase::Cancelled
        } else {
            Phase::Started
        };
        self.current = Some(Gesture::Region {
            region,
            origin,
            pos,
            delta: pos - origin,
            velocity: input.velocity,
            held: Duration::ZERO,
            moved,
            phase,
        });
        if phase == Phase::Started {
            Recognizer::Region {
                region,
                origin,
                at: frame.now,
                moved,
            }
        } else {
            Recognizer::Idle
        }
    }

    fn on_pressed(
        &mut self,
        input: &PointerFrame,
        frame: &GestureFrame,
        (origin, at): (Pos2, Instant),
        edge: Option<Edge>,
        long_reported: bool,
    ) -> Recognizer {
        let pos = input.pos.unwrap_or(origin);
        let elapsed = frame.now.saturating_duration_since(at);
        self.sample = Some(sample(origin, pos, input.velocity, elapsed));
        let delta = pos - origin;
        if input.released || !input.down {
            // A release within the slop = a tap. The engine raises nothing and the widget below takes it.
            return Recognizer::Idle;
        }
        if delta.length() > self.tuning.slop_px {
            return match edge {
                Some(edge)
                    if frame.slide_edges.contains(edge)
                        && is_along(delta, edge, self.tuning.slop_px) =>
                {
                    self.current = Some(Gesture::EdgeSlide {
                        edge,
                        offset: delta.dot(edge.along()),
                        velocity: input.velocity.dot(edge.along()),
                        phase: Phase::Started,
                    });
                    Recognizer::EdgeSlide { edge, origin }
                }
                Some(edge) if is_inward(delta, edge, self.tuning.slop_px) => {
                    self.current = Some(Gesture::EdgeSwipe {
                        edge,
                        progress: delta.dot(edge.inward()),
                        velocity: input.velocity.dot(edge.inward()),
                        phase: Phase::Started,
                    });
                    self.still = Some((pos, elapsed));
                    self.hold_reported = false;
                    Recognizer::EdgeSwipe { edge, origin }
                }
                _ => Recognizer::Passed { origin },
            };
        }
        let long_reported = if !long_reported && elapsed >= self.tuning.long_press {
            self.current = Some(Gesture::LongPress { pos });
            true
        } else {
            long_reported
        };
        Recognizer::Pressed {
            origin,
            at,
            edge,
            long_reported,
        }
    }

    /// A touch in a region: where the finger is, each frame, to its release. Nothing else comes
    /// of it: no long press, no fling.
    fn on_region(
        &mut self,
        input: &PointerFrame,
        frame: &GestureFrame,
        region: u8,
        (origin, at): (Pos2, Instant),
        moved: bool,
    ) -> Recognizer {
        let last = self.sample.map_or(origin, |s| s.pos);
        let pos = input.pos.unwrap_or(last);
        let held = frame.now.saturating_duration_since(at);
        self.sample = Some(sample(origin, pos, input.velocity, held));
        let moved = moved || (pos - origin).length() > self.tuning.slop_px;
        let phase = if input.released {
            Phase::Ended
        } else if !input.down {
            Phase::Cancelled
        } else {
            Phase::Moved
        };
        self.current = Some(Gesture::Region {
            region,
            origin,
            pos,
            delta: pos - last,
            velocity: input.velocity,
            held,
            moved,
            phase,
        });
        if phase == Phase::Moved {
            Recognizer::Region {
                region,
                origin,
                at,
                moved,
            }
        } else {
            Recognizer::Idle
        }
    }

    fn on_swipe(&mut self, input: &PointerFrame, edge: Edge, origin: Pos2) -> Recognizer {
        // A frame where the pointer disappeared (a `PointerGone` alone, with no release) is wrapped up at the last position.
        let pos = input
            .pos
            .or_else(|| self.sample.map(|s| s.pos))
            .unwrap_or(origin);
        let elapsed = self.sample.map_or(Duration::ZERO, |s| s.elapsed) + input.dt;
        self.sample = Some(sample(origin, pos, input.velocity, elapsed));
        let delta = pos - origin;
        let progress = delta.dot(edge.inward());
        let velocity = input.velocity.dot(edge.inward());
        let phase = if input.released {
            Phase::Ended
        } else if !input.down {
            Phase::Cancelled
        } else {
            Phase::Moved
        };
        self.current = Some(Gesture::EdgeSwipe {
            edge,
            progress,
            velocity,
            phase,
        });
        if phase != Phase::Moved {
            self.still = None;
            return Recognizer::Idle;
        }
        // A hold after a swipe (`SwipeHold`): once a pause, where it stops within `slop / 2` for
        // `hold`. Moving on starts the next pause.
        let still_px = self.tuning.slop_px * 0.5;
        match self.still {
            Some((anchor, since)) if (pos - anchor).length() <= still_px => {
                if !self.hold_reported && elapsed.saturating_sub(since) >= self.tuning.hold {
                    self.hold_reported = true;
                    self.current = Some(Gesture::SwipeHold { edge });
                }
            }
            _ => {
                self.still = Some((pos, elapsed));
                self.hold_reported = false;
            }
        }
        Recognizer::EdgeSwipe { edge, origin }
    }

    fn on_slide(&mut self, input: &PointerFrame, edge: Edge, origin: Pos2) -> Recognizer {
        let pos = input
            .pos
            .or_else(|| self.sample.map(|s| s.pos))
            .unwrap_or(origin);
        let elapsed = self.sample.map_or(Duration::ZERO, |s| s.elapsed) + input.dt;
        self.sample = Some(sample(origin, pos, input.velocity, elapsed));
        let phase = if input.released {
            Phase::Ended
        } else if !input.down {
            Phase::Cancelled
        } else {
            Phase::Moved
        };
        self.current = Some(Gesture::EdgeSlide {
            edge,
            offset: (pos - origin).dot(edge.along()),
            velocity: input.velocity.dot(edge.along()),
            phase,
        });
        if phase == Phase::Moved {
            Recognizer::EdgeSlide { edge, origin }
        } else {
            Recognizer::Idle
        }
    }

    fn on_passed(&mut self, input: &PointerFrame, origin: Pos2) -> Recognizer {
        if input.released || !input.down {
            let speed = input.velocity.length();
            if input.released && speed >= self.tuning.fling_px_s {
                self.current = Some(Gesture::Swipe {
                    dir: Dir::of(input.velocity),
                    velocity: speed,
                });
            }
            self.sample = None;
            return Recognizer::Idle;
        }
        if let Some(pos) = input.pos {
            let elapsed = self.sample.map_or(Duration::ZERO, |s| s.elapsed) + input.dt;
            self.sample = Some(sample(origin, pos, input.velocity, elapsed));
        }
        Recognizer::Passed { origin }
    }

    /// This frame's gesture.
    #[must_use]
    pub fn current(&self) -> Option<Gesture> {
        self.current
    }

    /// The recogniser's state (for tests and diagnostics).
    #[must_use]
    pub fn recognizer(&self) -> Recognizer {
        self.rec
    }

    /// The edge of the edge swipe or slide in progress.
    #[must_use]
    pub fn active_edge(&self) -> Option<Edge> {
        match self.rec {
            Recognizer::EdgeSwipe { edge, .. } | Recognizer::EdgeSlide { edge, .. } => Some(edge),
            _ => None,
        }
    }

    /// Whether an edge swipe or slide is in progress, or a touch in a region that has moved past
    /// the slop (that is, the shield is active).
    #[must_use]
    pub fn is_active(&self) -> bool {
        matches!(
            self.rec,
            Recognizer::EdgeSwipe { .. }
                | Recognizer::EdgeSlide { .. }
                | Recognizer::Region { moved: true, .. }
        )
    }

    /// Whether the shield should be raised.
    #[must_use]
    pub(crate) fn shield_active(&self) -> bool {
        self.is_active()
    }

    /// The position and elapsed time if a press is being held without moving past the slop, in a
    /// region's zone too. The shell and the guard decide the emergency gesture (a 2 s long press)
    /// from this — [`Gesture::LongPress`] fires only once, at 500 ms.
    #[must_use]
    pub fn hold(&self) -> Option<(Pos2, Duration)> {
        match (self.rec, self.sample) {
            (
                Recognizer::Pressed { origin, .. }
                | Recognizer::Region {
                    origin,
                    moved: false,
                    ..
                },
                Some(s),
            ) => Some((origin, s.elapsed)),
            _ => None,
        }
    }

    /// A higher priority took it: end the edge swipe or slide, or the touch in a region, in
    /// progress as `Cancelled` and ignore this press until it is released.
    pub fn cancel(&mut self) {
        match self.rec {
            Recognizer::EdgeSwipe { edge, origin } => {
                let (progress, velocity) = self.sample.map_or((0.0, 0.0), |s| {
                    (s.delta.dot(edge.inward()), s.velocity.dot(edge.inward()))
                });
                self.current = Some(Gesture::EdgeSwipe {
                    edge,
                    progress,
                    velocity,
                    phase: Phase::Cancelled,
                });
                self.still = None;
                self.rec = Recognizer::Passed { origin };
            }
            Recognizer::EdgeSlide { edge, origin } => {
                let (offset, velocity) = self.sample.map_or((0.0, 0.0), |s| {
                    (s.delta.dot(edge.along()), s.velocity.dot(edge.along()))
                });
                self.current = Some(Gesture::EdgeSlide {
                    edge,
                    offset,
                    velocity,
                    phase: Phase::Cancelled,
                });
                self.rec = Recognizer::Passed { origin };
            }
            Recognizer::Region {
                region,
                origin,
                moved,
                ..
            } => {
                let (pos, velocity, held) = self
                    .sample
                    .map_or((origin, Vec2::ZERO, Duration::ZERO), |s| {
                        (s.pos, s.velocity, s.elapsed)
                    });
                self.current = Some(Gesture::Region {
                    region,
                    origin,
                    pos,
                    delta: Vec2::ZERO,
                    velocity,
                    held,
                    moved,
                    phase: Phase::Cancelled,
                });
                self.rec = Recognizer::Passed { origin };
            }
            _ => {}
        }
    }

    /// Stage 11: cover the screen if the shield is active.
    pub fn shield(&self, ctx: &egui::Context) {
        if self.shield_active() && self.screen.is_positive() {
            let _ = shield::show(ctx, self.screen);
        }
    }
}

impl Default for GestureEngine {
    fn default() -> Self {
        Self::new(GestureTuning::default())
    }
}

/// One frame's pointer state (read from `ctx.input` in a single call).
#[derive(Debug, Clone, Copy)]
struct PointerFrame {
    down: bool,
    pressed: bool,
    released: bool,
    /// Where this frame's press went down ([`crate::drag::press_point`]) — not `pos`, which a
    /// fast finger has already carried on by the frame that sees the press.
    press: Option<Pos2>,
    pos: Option<Pos2>,
    velocity: Vec2,
    dt: Duration,
}

fn sample(origin: Pos2, pos: Pos2, velocity: Vec2, elapsed: Duration) -> DragSample {
    DragSample {
        origin,
        pos,
        delta: pos - origin,
        velocity,
        elapsed,
    }
}

/// An edge swipe is when the inward-axis movement passes the slop and exceeds the cross-axis movement.
fn is_inward(delta: Vec2, edge: Edge, slop: f32) -> bool {
    let along = delta.dot(edge.inward());
    let across = (delta - edge.inward() * along).length();
    along > slop && along >= across
}

/// A slide is when the movement along the edge passes the slop and **beats** the inward one — a
/// tie goes to the swipe, so a diagonal pull up from the bottom stays a swipe.
fn is_along(delta: Vec2, edge: Edge, slop: f32) -> bool {
    let along = delta.dot(edge.along()).abs();
    let inward = delta.dot(edge.inward()).abs();
    along > slop && along > inward
}

#[cfg(test)]
mod tests {
    use super::{
        Dir, Edge, EdgeMask, Gesture, GestureEngine, GestureFrame, GestureTuning, Phase,
        Recognizer, RegionZones,
    };
    use egui::{pos2, vec2, Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
    use std::time::{Duration, Instant};

    const DT: f64 = 1.0 / 60.0;

    /// Drives the engine frame by frame on a headless context.
    struct Rig {
        ctx: egui::Context,
        engine: GestureEngine,
        time: f64,
        base: Instant,
        blocked: EdgeMask,
        slide_edges: EdgeMask,
        regions: RegionZones,
    }

    impl Rig {
        fn new(tuning: GestureTuning) -> Self {
            Self {
                ctx: egui::Context::default(),
                engine: GestureEngine::new(tuning),
                time: 0.0,
                base: Instant::now(),
                blocked: EdgeMask::NONE,
                slide_edges: EdgeMask::NONE,
                regions: RegionZones::NONE,
            }
        }

        fn screen() -> Rect {
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1024.0, 600.0))
        }

        fn frame(&mut self, events: Vec<Event>) -> Option<Gesture> {
            let input = RawInput {
                screen_rect: Some(Self::screen()),
                time: Some(self.time),
                predicted_dt: 1.0 / 60.0,
                events,
                ..Default::default()
            };
            let frame = GestureFrame {
                screen: Self::screen(),
                edge_zones: crate::shell::edge_zones(Self::screen(), 24.0, 32.0, 56.0),
                blocked: self.blocked,
                slide_edges: self.slide_edges,
                regions: self.regions,
                now: self.base + Duration::from_secs_f64(self.time),
            };
            let Self { ctx, engine, .. } = self;
            ctx.run_ui(input, |ui| {
                engine.update(ui.ctx(), &frame);
            })
            .drop_without_applying_deltas();
            self.time += DT;
            self.engine.current()
        }

        fn press(&mut self, pos: Pos2) -> Option<Gesture> {
            self.frame(vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::default(),
                },
            ])
        }

        fn move_to(&mut self, pos: Pos2) -> Option<Gesture> {
            self.frame(vec![Event::PointerMoved(pos)])
        }

        fn release(&mut self, pos: Pos2) -> Option<Gesture> {
            self.frame(vec![
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::default(),
                },
                Event::PointerGone,
            ])
        }
    }

    /// A region over the top edge's zone, the full width.
    fn top_region() -> Rect {
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1024.0, 40.0))
    }

    /// **A press in a region is the region's from the press to the release**: `Started`
    /// on the press, held and not yet active while it stays within the slop, `Moved` with the
    /// frame's own movement after, the shield up once past the slop, and `Ended` on the release.
    /// The pull down from the top edge it is on is no edge swipe.
    #[test]
    fn a_press_in_a_region_is_the_regions_to_its_release() {
        let mut r = Rig::new(GestureTuning::default());
        r.regions.push(top_region(), 7);
        let g = r.press(pos2(500.0, 10.0));
        assert!(
            matches!(g, Some(Gesture::Region { region: 7, phase: Phase::Started, moved: false, held, .. }) if held.is_zero()),
            "{g:?}"
        );
        let g = r.move_to(pos2(504.0, 12.0));
        assert!(
            matches!(g, Some(Gesture::Region { phase: Phase::Moved, moved: false, delta, .. }) if (delta - vec2(4.0, 2.0)).length() < 1e-3),
            "{g:?}"
        );
        assert!(
            r.engine.hold().is_some(),
            "a still press in a region is held"
        );
        assert!(!r.engine.is_active() && !r.engine.shield_active());
        let g = r.move_to(pos2(504.0, 112.0));
        assert!(
            matches!(g, Some(Gesture::Region { phase: Phase::Moved, moved: true, pos, delta, .. })
                if (pos - pos2(504.0, 112.0)).length() < 1e-3 && (delta - vec2(0.0, 100.0)).length() < 1e-3),
            "a pull down the top edge stays the region's: {g:?}"
        );
        assert!(r.engine.is_active() && r.engine.hold().is_none());
        let g = r.release(pos2(504.0, 120.0));
        assert!(
            matches!(g, Some(Gesture::Region { region: 7, phase: Phase::Ended, held, .. }) if held > Duration::ZERO),
            "{g:?}"
        );
        assert_eq!(r.engine.recognizer(), Recognizer::Idle);
        assert!(!r.engine.is_active());
    }

    /// **Nothing else comes of a press in a region**: held still past the long press it raises no
    /// `LongPress`, only `Moved` frames with the time held — and a quick release is no fling.
    #[test]
    fn a_region_press_raises_no_long_press_or_fling() {
        let mut r = Rig::new(GestureTuning::default());
        r.regions.push(
            Rect::from_min_max(pos2(300.0, 200.0), pos2(700.0, 400.0)),
            0,
        );
        let _ = r.press(pos2(400.0, 300.0));
        for _ in 0..40 {
            let g = r.move_to(pos2(400.0, 300.0));
            assert!(
                matches!(
                    g,
                    Some(Gesture::Region {
                        phase: Phase::Moved,
                        ..
                    })
                ),
                "{g:?}"
            );
        }
        assert!(r
            .engine
            .hold()
            .is_some_and(|(_, held)| held >= Duration::from_millis(600)));
        let _ = r.release(pos2(400.0, 300.0));
        let _ = r.press(pos2(320.0, 300.0));
        let mut pos = pos2(320.0, 300.0);
        for _ in 0..6 {
            pos.x += 40.0;
            let _ = r.move_to(pos);
        }
        let g = r.release(pos);
        assert!(
            matches!(
                g,
                Some(Gesture::Region {
                    phase: Phase::Ended,
                    ..
                })
            ),
            "a fast release in a region is its end, not a fling: {g:?}"
        );
    }

    /// **The topmost zone takes the press**, and a press let go within its own frame is one
    /// `Ended`.
    #[test]
    fn the_topmost_zone_takes_the_press() {
        let mut r = Rig::new(GestureTuning::default());
        r.regions
            .push(Rect::from_min_max(pos2(0.0, 0.0), pos2(600.0, 600.0)), 1);
        r.regions
            .push(Rect::from_min_max(pos2(400.0, 0.0), pos2(1024.0, 600.0)), 2);
        let g = r.press(pos2(500.0, 300.0));
        assert!(
            matches!(g, Some(Gesture::Region { region: 2, .. })),
            "{g:?}"
        );
        let _ = r.release(pos2(500.0, 300.0));
        let g = r.frame(vec![
            Event::PointerMoved(pos2(100.0, 300.0)),
            Event::PointerButton {
                pos: pos2(100.0, 300.0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
            Event::PointerButton {
                pos: pos2(100.0, 300.0),
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            },
        ]);
        assert!(
            matches!(
                g,
                Some(Gesture::Region {
                    region: 1,
                    phase: Phase::Ended,
                    ..
                })
            ),
            "{g:?}"
        );
        assert_eq!(r.engine.recognizer(), Recognizer::Idle);
    }

    /// **A region's touch can be called off**: `Cancelled`, then nothing more of that press.
    #[test]
    fn a_region_touch_can_be_called_off() {
        let mut r = Rig::new(GestureTuning::default());
        r.regions.push(top_region(), 0);
        let _ = r.press(pos2(500.0, 10.0));
        let _ = r.move_to(pos2(560.0, 10.0));
        r.engine.cancel();
        assert!(
            matches!(r.engine.current(), Some(Gesture::Region { phase: Phase::Cancelled, pos, .. }) if (pos - pos2(560.0, 10.0)).length() < 1e-3),
            "{:?}",
            r.engine.current()
        );
        assert!(matches!(r.engine.recognizer(), Recognizer::Passed { .. }));
        assert_eq!(r.move_to(pos2(600.0, 10.0)), None, "the press is let go");
    }

    /// **With gestures off there are no regions**: the press is an ordinary one, and the
    /// emergency hold still counts it.
    #[test]
    fn with_gestures_off_there_are_no_regions() {
        let mut r = Rig::new(GestureTuning {
            enabled: false,
            ..GestureTuning::default()
        });
        r.regions.push(top_region(), 0);
        assert_eq!(r.press(pos2(500.0, 10.0)), None);
        assert!(matches!(r.engine.recognizer(), Recognizer::Pressed { .. }));
        assert!(r.engine.hold().is_some());
    }

    /// **A press and its first move in one frame still start from the press**. On a
    /// slow frame, or a quick flick on a touch digitizer, the finger comes down on the edge and is
    /// past the edge zone before the frame that sees it runs. egui's `interact_pos` is then the
    /// moved point, and an engine that took it for the origin found no edge under it and let the
    /// whole pull go — the console lost its first pull to this in one capture run of seven.
    #[test]
    fn a_press_that_lands_with_its_first_move_starts_from_the_press() {
        let mut r = Rig::new(GestureTuning::default());
        r.frame(vec![
            Event::PointerMoved(pos2(10.0, 4.0)),
            Event::PointerButton {
                pos: pos2(10.0, 4.0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
            Event::PointerMoved(pos2(10.0, 60.0)),
        ]);
        let g = r.move_to(pos2(10.0, 100.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSwipe { edge: Edge::Top, phase: Phase::Started, progress, .. }) if (progress - 96.0).abs() < 1e-3),
            "a pull from the top edge, measured from where it came down: {g:?}"
        );
    }

    /// The top edge: `Started` on the frame it passes the slop, with progress 1:1 from the press point (the slop is not subtracted).
    #[test]
    fn top_edge_swipe_starts_after_slop_and_reports_raw_progress() {
        let mut r = Rig::new(GestureTuning::default());
        assert_eq!(r.press(pos2(10.0, 8.0)), None);
        assert_eq!(
            r.move_to(pos2(10.0, 18.0)),
            None,
            "10 px is inside the slop"
        );
        assert!(!r.engine.is_active());
        let g = r.move_to(pos2(10.0, 32.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSwipe { edge: Edge::Top, phase: Phase::Started, progress, .. }) if (progress - 24.0).abs() < 1e-3),
            "{g:?}"
        );
        assert!(r.engine.is_active() && r.engine.shield_active());
        let g = r.move_to(pos2(10.0, 100.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSwipe { phase: Phase::Moved, progress, .. }) if (progress - 92.0).abs() < 1e-3),
            "{g:?}"
        );
        // The release frame: the `Ended` and that position's progress come together (`interact_pos` remains).
        let g = r.release(pos2(10.0, 150.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSwipe { phase: Phase::Ended, progress, .. }) if (progress - 142.0).abs() < 1e-3),
            "{g:?}"
        );
        assert!(!r.engine.is_active());
        assert_eq!(r.engine.recognizer(), Recognizer::Idle);
    }

    /// A slide along the bottom edge, where the shell asks for one: `Started` past the slop with
    /// the offset along the edge from the press, then `Moved`, then `Ended`, the shield up
    /// throughout (the home indicator's left and right).
    #[test]
    fn a_slide_along_an_edge_asked_for_is_recognised() {
        let mut r = Rig::new(GestureTuning::default());
        r.slide_edges = EdgeMask::NONE.with(Edge::Bottom);
        assert_eq!(r.press(pos2(500.0, 590.0)), None);
        let g = r.move_to(pos2(520.0, 588.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSlide { edge: Edge::Bottom, phase: Phase::Started, offset, .. }) if (offset - 20.0).abs() < 1e-3),
            "{g:?}"
        );
        assert!(r.engine.is_active() && r.engine.shield_active());
        let g = r.move_to(pos2(380.0, 592.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSlide { phase: Phase::Moved, offset, .. }) if (offset + 120.0).abs() < 1e-3),
            "leftwards is negative: {g:?}"
        );
        let g = r.release(pos2(360.0, 592.0));
        assert!(
            matches!(g, Some(Gesture::EdgeSlide { phase: Phase::Ended, offset, .. }) if (offset + 140.0).abs() < 1e-3),
            "{g:?}"
        );
        assert_eq!(r.engine.recognizer(), Recognizer::Idle);
    }

    /// Only on the edges asked for: the same slide with none asked for passes through as an
    /// ordinary drag, and so does one on a blocked edge.
    #[test]
    fn a_slide_on_an_edge_not_asked_for_passes_through() {
        let mut r = Rig::new(GestureTuning::default());
        assert_eq!(r.press(pos2(500.0, 590.0)), None);
        assert_eq!(r.move_to(pos2(540.0, 588.0)), None);
        assert!(matches!(r.engine.recognizer(), Recognizer::Passed { .. }));
        assert!(!r.engine.shield_active());
        assert_eq!(r.release(pos2(540.0, 588.0)), None);

        let mut r = Rig::new(GestureTuning::default());
        r.slide_edges = EdgeMask::NONE.with(Edge::Bottom);
        r.blocked = EdgeMask::NONE.with(Edge::Bottom);
        assert_eq!(r.press(pos2(500.0, 590.0)), None);
        assert_eq!(r.move_to(pos2(540.0, 588.0)), None);
        assert!(matches!(r.engine.recognizer(), Recognizer::Passed { .. }));
    }

    /// With slides on, a pull up from the bottom is still a swipe — a diagonal tie goes to it — and
    /// a cancelled slide reports `Cancelled` and lets the press go.
    #[test]
    fn a_pull_up_stays_a_swipe_and_a_slide_can_be_called_off() {
        let mut r = Rig::new(GestureTuning::default());
        r.slide_edges = EdgeMask::NONE.with(Edge::Bottom);
        assert_eq!(r.press(pos2(500.0, 590.0)), None);
        // Exactly as far along as up: the tie is the swipe's.
        let g = r.move_to(pos2(515.0, 575.0));
        assert!(
            matches!(
                g,
                Some(Gesture::EdgeSwipe {
                    edge: Edge::Bottom,
                    phase: Phase::Started,
                    ..
                })
            ),
            "{g:?}"
        );
        let _ = r.release(pos2(515.0, 575.0));

        assert_eq!(r.press(pos2(500.0, 590.0)), None);
        assert!(matches!(
            r.move_to(pos2(530.0, 590.0)),
            Some(Gesture::EdgeSlide { .. })
        ));
        r.engine.cancel();
        assert!(matches!(
            r.engine.current(),
            Some(Gesture::EdgeSlide {
                phase: Phase::Cancelled,
                ..
            })
        ));
        assert!(matches!(r.engine.recognizer(), Recognizer::Passed { .. }));
        assert_eq!(r.move_to(pos2(600.0, 590.0)), None, "the press is let go");
    }

    /// An edge tap (released within the slop) emits nothing and raises no shield — the status bar tap passes through.
    #[test]
    fn edge_tap_within_slop_emits_nothing() {
        let mut r = Rig::new(GestureTuning::default());
        assert_eq!(r.press(pos2(10.0, 8.0)), None);
        assert!(!r.engine.shield_active() && r.engine.hold().is_some());
        assert_eq!(r.move_to(pos2(12.0, 14.0)), None);
        assert_eq!(r.release(pos2(12.0, 14.0)), None);
        assert_eq!(r.engine.recognizer(), Recognizer::Idle);
    }

    /// Leaving an edge zone in a direction that is not inward (upward) is `Passed`, and a quick release is a `Swipe`.
    #[test]
    fn non_inward_edge_press_passes_and_flings_as_swipe() {
        let mut r = Rig::new(GestureTuning::default());
        r.press(pos2(500.0, 300.0));
        let mut pos = pos2(500.0, 300.0);
        let mut last = None;
        for _ in 0..8 {
            pos.x += 20.0;
            last = r.move_to(pos);
        }
        assert_eq!(last, None);
        assert!(matches!(r.engine.recognizer(), Recognizer::Passed { .. }));
        let g = r.release(pos);
        assert!(
            matches!(g, Some(Gesture::Swipe { dir: Dir::Right, velocity }) if velocity >= 800.0),
            "{g:?}"
        );
    }

    /// Standing still within `slop / 2` for 150 ms during a swipe emits `SwipeHold` once a pause
    /// (the bottom navigation's recent screens): one for a long pause, and another only
    /// after moving on and stopping again.
    #[test]
    fn swipe_hold_fires_once_a_pause() {
        let mut r = Rig::new(GestureTuning::default());
        r.press(pos2(500.0, 595.0));
        r.move_to(pos2(500.0, 575.0));
        let g = r.move_to(pos2(500.0, 540.0));
        assert!(matches!(
            g,
            Some(Gesture::EdgeSwipe {
                edge: Edge::Bottom,
                phase: Phase::Moved,
                ..
            })
        ));
        let mut holds = 0;
        let mut first_at = None;
        for i in 0..20 {
            let g = r.move_to(pos2(501.0, 540.0));
            if matches!(g, Some(Gesture::SwipeHold { edge: Edge::Bottom })) {
                holds += 1;
                first_at.get_or_insert(i);
            }
        }
        assert_eq!(holds, 1, "once a pause");
        let at = first_at.unwrap_or(usize::MAX);
        assert!((8..=10).contains(&at), "150 ms ≈ 9 frames: {at}");
        // Moving again after a stop returns to `Moved`.
        assert!(matches!(
            r.move_to(pos2(500.0, 500.0)),
            Some(Gesture::EdgeSwipe {
                phase: Phase::Moved,
                ..
            })
        ));
        // And a second stop is a second hold.
        let again = (0..20)
            .filter(|_| {
                matches!(
                    r.move_to(pos2(500.0, 500.0)),
                    Some(Gesture::SwipeHold { edge: Edge::Bottom })
                )
            })
            .count();
        assert_eq!(again, 1, "the next pause");
    }

    /// `enabled = false`: no gestures, but press tracking (`hold`) remains.
    #[test]
    fn disabled_keeps_hold_but_emits_nothing() {
        let mut r = Rig::new(GestureTuning {
            enabled: false,
            ..GestureTuning::default()
        });
        r.press(pos2(10.0, 8.0));
        assert!(r.engine.hold().is_some());
        for _ in 0..10 {
            assert_eq!(r.move_to(pos2(10.0, 8.0)), None);
        }
        assert!(r
            .engine
            .hold()
            .is_some_and(|(_, d)| d >= Duration::from_millis(150)));
        assert_eq!(r.move_to(pos2(10.0, 200.0)), None);
        assert!(!r.engine.shield_active());
    }

    /// A swipe does not start on a blocked edge (`edge_guard`) — it is `Passed`.
    #[test]
    fn blocked_edge_passes() {
        let mut r = Rig::new(GestureTuning::default());
        r.blocked = EdgeMask::ALL;
        r.press(pos2(10.0, 8.0));
        assert_eq!(r.move_to(pos2(10.0, 100.0)), None);
        assert!(matches!(r.engine.recognizer(), Recognizer::Passed { .. }));
    }
}
