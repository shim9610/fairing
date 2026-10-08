//! The recogniser's state: press → slop → an edge swipe, a long press, or passing
//! through as an ordinary tap.
//!
//! The engine ([`super::GestureEngine`]) hands over the pointer state each frame and the
//! transitions happen here. It is value types only, so there are no heap allocations per frame.
//!
//! The transition table (as completed in M2, with regions):
//! ```text
//! Idle ──press──▶ Pressed{edge?} ──inward past slop──▶ EdgeSwipe ──release/lost──▶ Idle
//!   │               │  ├─ along a slide edge past slop ──▶ EdgeSlide ──release/lost──▶ Idle
//!   │               │  └─ past slop (not an edge, or not inward) ──▶ Passed ──release (≥ fling → Swipe)──▶ Idle
//!   │               └─ release (a tap) ──▶ Idle     during EdgeSwipe, 150 ms still within `slop/2` → SwipeHold (once a pause)
//!   └─press in a region's zone──▶ Region ──release/lost──▶ Idle
//! ```
//! In `Pressed`, passing `long_press` emits `LongPress` once, and `cancel()` moves
//! `EdgeSwipe`, `EdgeSlide` and `Region` to `Passed`.

use super::edge::Edge;
use egui::{Pos2, Vec2};
use std::time::{Duration, Instant};

/// A swipe direction (an ordinary fling away from an edge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Left.
    Left,
    /// Right.
    Right,
}

impl Dir {
    /// The direction of the velocity vector's dominant axis.
    #[must_use]
    pub fn of(v: Vec2) -> Self {
        if v.x.abs() >= v.y.abs() {
            if v.x >= 0.0 {
                Self::Right
            } else {
                Self::Left
            }
        } else if v.y >= 0.0 {
            Self::Down
        } else {
            Self::Up
        }
    }
}

/// A gesture's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The first frame past the slop.
    Started,
    /// In progress.
    Moved,
    /// Released.
    Ended,
    /// A higher priority took it, or the pointer was lost.
    Cancelled,
}

/// A recognised global gesture.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Gesture {
    /// An edge swipe. `progress` is the distance moved from the press point along the inward
    /// axis (px; it can be negative — the consumer clamps), and `velocity` is the velocity on
    /// that axis (px/s, positive inward).
    EdgeSwipe {
        /// The edge it started on.
        edge: Edge,
        /// The inward-axis movement (px).
        progress: f32,
        /// The inward-axis velocity (px/s).
        velocity: f32,
        /// The phase.
        phase: Phase,
    },
    /// A fling away from an edge (once, on release).
    Swipe {
        /// The direction.
        dir: Dir,
        /// The speed (px/s).
        velocity: f32,
    },
    /// A long press (once, on passing `long_press`).
    LongPress {
        /// The position.
        pos: Pos2,
    },
    /// Standing still after an edge swipe — once a pause (the recent screens in the bottom
    /// gesture navigation).
    SwipeHold {
        /// The edge.
        edge: Edge,
    },
    /// A slide **along** an edge, from inside its zone — only on the edges the shell asks for
    /// (the home indicator's left and right, `NavStyle::Gesture`). `offset` is the movement from
    /// the press point along [`Edge::along`] (px, either sign), `velocity` the velocity on it.
    EdgeSlide {
        /// The edge it started on.
        edge: Edge,
        /// The movement along the edge (px).
        offset: f32,
        /// The velocity along the edge (px/s).
        velocity: f32,
        /// The phase.
        phase: Phase,
    },
    /// A touch in a gesture region, every frame from its press to its release:
    /// `Started` on the press, `Moved` on each frame after (a finger at rest included), `Ended`
    /// on the release, `Cancelled` when the pointer is lost or a higher priority takes it. A
    /// press let go within its own frame is reported once, as `Ended`.
    Region {
        /// Which region, as the frame's zones name it.
        region: u8,
        /// The press point.
        origin: Pos2,
        /// Where the finger is.
        pos: Pos2,
        /// The movement since the frame before (px).
        delta: Vec2,
        /// The finger's velocity (px/s).
        velocity: Vec2,
        /// The time since the press.
        held: Duration,
        /// Whether it has gone past the slop since the press.
        moved: bool,
        /// The phase.
        phase: Phase,
    },
}

/// The tracking values for one press.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragSample {
    /// The press point.
    pub origin: Pos2,
    /// The current point.
    pub pos: Pos2,
    /// `pos − origin`.
    pub delta: Vec2,
    /// The smoothed finger velocity (`pointer.velocity()`).
    pub velocity: Vec2,
    /// The time since the press.
    pub elapsed: Duration,
}

/// The recogniser's state machine.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Recognizer {
    /// Not pressed.
    #[default]
    Idle,
    /// Pressed, within the slop.
    Pressed {
        /// The press point.
        origin: Pos2,
        /// When it was pressed.
        at: Instant,
        /// The edge zone pressed in (`None` for a blocked edge).
        edge: Option<Edge>,
        /// Whether the long press has already been reported.
        long_reported: bool,
    },
    /// An edge swipe in progress (the shield is active).
    EdgeSwipe {
        /// The edge.
        edge: Edge,
        /// The press point.
        origin: Pos2,
    },
    /// A slide along an edge in progress (the shield is active).
    EdgeSlide {
        /// The edge.
        edge: Edge,
        /// The press point.
        origin: Pos2,
    },
    /// A touch in a gesture region, the region's from its press to its release. The
    /// shield is up once it has moved.
    Region {
        /// Which region, as the frame's zones name it.
        region: u8,
        /// The press point.
        origin: Pos2,
        /// When it was pressed.
        at: Instant,
        /// Whether it has gone past the slop since the press.
        moved: bool,
    },
    /// Passed through as an ordinary tap or drag — ignored until release.
    Passed {
        /// The press point (for deciding a fling).
        origin: Pos2,
    },
}
