//! The hidden entry point. The **invisible** door into a device's service menu.
//!
//! # The crate does not decide the trigger
//!
//! Android wants the build number pressed seven times; an industrial HMI wants the corners touched
//! in order; some devices use a hardware key combination; somewhere else it is particular
//! coordinates on the screen tapped in a set order. **None of that is the crate's to decide** — the
//! crate does not set policy, here as everywhere.
//!
//! So what this module offers is not a **list** of triggers but one [`KnockTrigger`] **trait**. An
//! integrator implements their own trigger, and the crate feeds it every frame and checks the gate
//! once it completes. The two common ones ([`TapKnock`] · [`ZoneKnock`]) come along as examples and
//! as defaults.
//!
//! # This is **not** a security boundary
//!
//! A hidden entry point is **concealment**, not authentication. It only makes the door hard to
//! find; it does not stop whoever finds it. What stops them is a [`Gate`](crate::Gate) — give
//! [`HiddenEntry::gate`] one and pulling the trigger still has to pass the gate, and on a failure
//! the shell raises
//! [`AccessEvent::UnlockRequested`](crate::access::AccessEvent::UnlockRequested).
//!
//! **A hidden entry point with no gate is a door anyone can walk through.** Leave it that way only
//! for something harmless to look at, like a diagnostics screen. Anything like a factory reset or
//! writing calibration values takes a gate, without exception.
//!
//! # It leaves a trace
//!
//! On completion the shell raises [`ShellEvent::HiddenEntry`](crate::ShellEvent::HiddenEntry).
//! Entering a service menu on a device is **auditable**, so an integrator has to be able to record
//! it — it does not open quietly (the counterpart of a configuration error never opening the door
//! quietly: a quiet **success** goes unrecorded too).

use crate::gesture::Edge;
use egui::{Pos2, Rect};
use std::fmt;
use std::time::{Duration, Instant};

/// One tap this frame. Where it went down, where it came up, and how far it moved in between.
///
/// **Dragged ones come in too.** Whether to filter a swipe out is the trigger's decision — some
/// device may want "swipe inward from an edge" as its trigger.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tap {
    /// Where it went down.
    pub press: Pos2,
    /// Where it came up.
    pub release: Pos2,
    /// How far it moved between the press and the release (pt).
    pub travel: f32,
}

impl Tap {
    /// Whether it is a tap that did not drag. `slop` is usually `theme.motion.slop_px`.
    #[must_use]
    pub fn is_still(self, slop: f32) -> bool {
        self.travel <= slop
    }
}

/// Everything a trigger sees this frame.
///
/// `#[non_exhaustive]`, since new fields may be added — it is a value the shell builds and hands
/// over rather than one an integrator makes, so nothing breaks on it.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct KnockInput<'a> {
    /// The shell's time. Measure any time limit from this — do not call `Instant::now()` (tests
    /// push a time in).
    pub now: Instant,
    /// This frame's screen Rect. For handling coordinates as fractions.
    pub screen: Rect,
    /// How many times **this entry point** was poked with [`Cx::knock`](crate::Cx::knock) this
    /// frame. Usually 0 or 1. Widget triggers — buttons, labels — come in here.
    pub pokes: u8,
    /// The taps that finished this frame. For coordinate triggers.
    pub taps: &'a [Tap],
    /// The keys pressed this frame. For hardware keypad and scanner triggers.
    pub keys: &'a [egui::Key],
    /// The slop, for reference (pt). Hand it to [`Tap::is_still`] to filter swipes out.
    pub slop: f32,
}

impl KnockInput<'_> {
    /// Whether there was no input at all this frame. For bailing out early when only an expiry needs handling.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn is_quiet(&self) -> bool {
        self.pokes == 0 && self.taps.is_empty() && self.keys.is_empty()
    }
}

/// The result of feeding a trigger one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnockStep {
    /// No change.
    Idle,
    /// It advanced a step. The shell requests a repaint so the hint refreshes.
    Advanced,
    /// It went back to the beginning (wrong input, or a timeout).
    Reset,
    /// **Complete.** The shell checks the gate and runs the action.
    Opened,
}

/// A hidden entry point's trigger. **An integrator implements it.**
///
/// The shell calls [`KnockTrigger::feed`] **every frame** — on frames with no input too, so a
/// timeout can be handled here.
///
/// # The contract
///
/// - `feed` is a **pure decision**. Do not draw to the screen, do not make a blocking call,
///   and use [`KnockInput::now`] rather than `Instant::now()`. Tests push a time in.
/// - After returning [`KnockStep::Opened`] it **has to go back to the beginning itself.** The
///   shell does not call `reset` for it — a door that has opened once must not open again on the
///   next frame.
/// - [`KnockTrigger::remaining`] is for the hint. A trigger with nothing to count (a single
///   coordinate, say) can return `None`, and then
///   [`Cx::knock_remaining`](crate::Cx::knock_remaining) is `None` too.
///
/// # An example — a hardware key combination
///
/// ```
/// use fairing::access::{KnockInput, KnockStep, KnockTrigger};
///
/// /// F1 · F2 · F1 in order. The physical keys on the front of the device.
/// #[derive(Debug, Default)]
/// struct KeyCombo {
///     hit: usize,
/// }
///
/// const WANT: [egui::Key; 3] = [egui::Key::F1, egui::Key::F2, egui::Key::F1];
///
/// impl KnockTrigger for KeyCombo {
///     fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep {
///         let mut step = KnockStep::Idle;
///         for key in input.keys {
///             if WANT.get(self.hit) == Some(key) {
///                 self.hit += 1;
///                 step = KnockStep::Advanced;
///                 if self.hit == WANT.len() {
///                     self.hit = 0; // back to the beginning by itself, once open.
///                     return KnockStep::Opened;
///                 }
///             } else if self.hit != 0 {
///                 self.hit = 0;
///                 step = KnockStep::Reset;
///             }
///         }
///         step
///     }
///
///     fn remaining(&self) -> Option<u8> {
///         u8::try_from(WANT.len() - self.hit).ok()
///     }
///
///     fn reset(&mut self) {
///         self.hit = 0;
///     }
/// }
/// ```
pub trait KnockTrigger: fmt::Debug + Send + 'static {
    /// Feed it one frame.
    fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep;

    /// How many inputs are left before completion (for the hint). `None` where there is nothing to count.
    fn remaining(&self) -> Option<u8> {
        None
    }

    /// Put it back to the beginning. The shell calls it when re-registering the entry point.
    fn reset(&mut self);
}

// ── The two triggers provided ─────────────────────────────────────────────────

/// Knock on the same spot `count` times. Android's "the build number seven times" is this one.
///
/// **The crate does not know where the knocking happens** — when a screen calls
/// [`Cx::knock`](crate::Cx::knock) from its own secret widget, that is one knock. Were the shell to
/// decide the spot, it would be the same on every device and no longer concealment.
#[derive(Debug, Clone)]
pub struct TapKnock {
    count: u8,
    within: Duration,
    hit: u8,
    last: Option<Instant>,
}

impl TapKnock {
    /// `count` times, within 2 seconds between taps.
    #[must_use]
    pub fn new(count: u8) -> Self {
        Self {
            count: count.max(1),
            within: Duration::from_secs(2),
            hit: 0,
            last: None,
        }
    }

    /// Change the gap allowed between two taps.
    #[must_use]
    pub fn within(mut self, gap: Duration) -> Self {
        self.within = gap;
        self
    }
}

impl KnockTrigger for TapKnock {
    fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep {
        let expired = self
            .last
            .is_some_and(|last| input.now.saturating_duration_since(last) > self.within);
        if expired {
            self.reset();
            if input.pokes == 0 {
                return KnockStep::Reset;
            }
        }
        if input.pokes == 0 {
            return KnockStep::Idle;
        }
        for _ in 0..input.pokes {
            self.hit = self.hit.saturating_add(1);
            self.last = Some(input.now);
            if self.hit >= self.count {
                self.reset();
                return KnockStep::Opened;
            }
        }
        KnockStep::Advanced
    }

    fn remaining(&self) -> Option<u8> {
        Some(self.count.saturating_sub(self.hit))
    }

    fn reset(&mut self) {
        self.hit = 0;
        self.last = None;
    }
}

/// An unnamed region on the screen. The coordinates are **screen fractions** (0..=1), so it is the same spot on any panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Zone {
    /// Top left (as a fraction).
    pub min: (f32, f32),
    /// Bottom right (as a fraction).
    pub max: (f32, f32),
    /// For [`Zone::corner`]: the corner and the side, as a fraction of the short side. It
    /// resolves to a square there, and `min` / `max` are only its outline on a square screen.
    square: Option<(Corner, f32)>,
}

impl Zone {
    /// A rectangle in fractions.
    #[must_use]
    pub const fn new(min: (f32, f32), max: (f32, f32)) -> Self {
        Self {
            min,
            max,
            square: None,
        }
    }

    /// A square in a screen corner. `size` is its side as a fraction of the screen's short side
    /// (at most 0.5), so it stays a square on a wide panel.
    #[must_use]
    pub fn corner(corner: Corner, size: f32) -> Self {
        let s = size.clamp(0.0, 0.5);
        let (min, max) = match corner {
            Corner::TopLeft => ((0.0, 0.0), (s, s)),
            Corner::TopRight => ((1.0 - s, 0.0), (1.0, s)),
            Corner::BottomLeft => ((0.0, 1.0 - s), (s, 1.0)),
            Corner::BottomRight => ((1.0 - s, 1.0 - s), (1.0, 1.0)),
        };
        Self {
            min,
            max,
            square: Some((corner, s)),
        }
    }

    /// Resolve into screen coordinates.
    #[must_use]
    pub fn resolve(self, screen: Rect) -> Rect {
        if let Some((corner, size)) = self.square {
            let side = screen.width().min(screen.height()) * size;
            let (x, y) = match corner {
                Corner::TopLeft => (screen.min.x, screen.min.y),
                Corner::TopRight => (screen.max.x - side, screen.min.y),
                Corner::BottomLeft => (screen.min.x, screen.max.y - side),
                Corner::BottomRight => (screen.max.x - side, screen.max.y - side),
            };
            return Rect::from_min_size(egui::pos2(x, y), egui::vec2(side, side));
        }
        let at = |u: f32, v: f32| {
            egui::pos2(
                screen.width().mul_add(u, screen.min.x),
                screen.height().mul_add(v, screen.min.y),
            )
        };
        Rect::from_min_max(at(self.min.0, self.min.1), at(self.max.0, self.max.1))
    }
}

/// The screen's four corners. Convenience names for [`Zone::corner`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Corner {
    /// Top left.
    TopLeft,
    /// Top right.
    TopRight,
    /// Bottom left.
    BottomLeft,
    /// Bottom right.
    BottomRight,
}

impl Corner {
    /// Every corner.
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    /// The two sides this corner touches.
    #[must_use]
    pub const fn edges(self) -> (Edge, Edge) {
        match self {
            Self::TopLeft => (Edge::Top, Edge::Left),
            Self::TopRight => (Edge::Top, Edge::Right),
            Self::BottomLeft => (Edge::Bottom, Edge::Left),
            Self::BottomRight => (Edge::Bottom, Edge::Right),
        }
    }
}

/// Touch **set regions in a set order** on the screen.
///
/// No widget is needed, so it works whatever a screen draws — on a kiosk screen with all the chrome
/// hidden too. [`ZoneKnock::corners`] makes the common shape of corner sequence for you.
///
/// **Dragged input does not count.** The corners overlap the edge zones, and counting drags would
/// advance the entry point every time the shade is pulled.
#[derive(Debug, Clone)]
pub struct ZoneKnock {
    order: Vec<Zone>,
    within: Duration,
    hit: usize,
    last: Option<Instant>,
}

impl ZoneKnock {
    /// A sequence of regions. The same region may repeat.
    #[must_use]
    pub fn new(order: impl Into<Vec<Zone>>) -> Self {
        let order = order.into();
        Self {
            order,
            within: Duration::from_secs(2),
            hit: 0,
            last: None,
        }
    }

    /// A sequence of corners. A corner square is `size` as a fraction of the screen's short side (0.12 by default).
    #[must_use]
    pub fn corners(order: impl AsRef<[Corner]>) -> Self {
        Self::new(
            order
                .as_ref()
                .iter()
                .map(|c| Zone::corner(*c, 0.12))
                .collect::<Vec<_>>(),
        )
    }

    /// The gap allowed between two inputs.
    #[must_use]
    pub fn within(mut self, gap: Duration) -> Self {
        self.within = gap;
        self
    }

    /// How many regions have to be touched. An empty sequence counts as **1** — at 0 it would open by itself on the first frame.
    #[must_use]
    pub fn len(&self) -> usize {
        self.order.len().max(1)
    }

    /// Always `false` ([`ZoneKnock::len`] never goes below 1). For clippy's sake.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        false
    }
}

impl KnockTrigger for ZoneKnock {
    fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep {
        if self
            .last
            .is_some_and(|last| input.now.saturating_duration_since(last) > self.within)
        {
            self.reset();
            if input.taps.is_empty() {
                return KnockStep::Reset;
            }
        }
        if self.order.is_empty() {
            return KnockStep::Idle;
        }
        let mut step = KnockStep::Idle;
        for tap in input.taps {
            // A drag is not a knock. The decision goes by the slop the shell handed over.
            if !tap.is_still(input.slop) {
                continue;
            }
            let inside = |zone: &Zone| {
                let r = zone.resolve(input.screen);
                r.contains(tap.press) && r.contains(tap.release)
            };
            if self.order.get(self.hit).is_some_and(inside) {
                self.hit += 1;
                self.last = Some(input.now);
                step = KnockStep::Advanced;
                if self.hit >= self.order.len() {
                    self.reset();
                    return KnockStep::Opened;
                }
                continue;
            }
            // Where it is the first region it counts as 1 — a hand that slipped is not made to start over.
            let restart = self.order.first().is_some_and(inside);
            if self.hit != 0 || restart {
                self.hit = usize::from(restart);
                self.last = restart.then_some(input.now);
                step = KnockStep::Reset;
                if restart && self.hit >= self.order.len() {
                    self.reset();
                    return KnockStep::Opened;
                }
            }
        }
        step
    }

    fn remaining(&self) -> Option<u8> {
        u8::try_from(self.order.len().saturating_sub(self.hit)).ok()
    }

    fn reset(&mut self) {
        self.hit = 0;
        self.last = None;
    }
}

// ── Registration ──────────────────────────────────────────────────────────────

/// One registered hidden entry point.
///
/// **The gate is the security and the trigger is the concealment.** Leave [`HiddenEntry::gate`]
/// empty and whoever finds the door walks in — leave it that way only for a screen that is harmless
/// to look at.
pub struct HiddenEntry {
    /// The entry point's id. [`Cx::knock`](crate::Cx::knock) pokes it by this name.
    pub id: String,
    /// The trigger. **An integrator's own implementation goes in here too.**
    pub trigger: Box<dyn KnockTrigger>,
    /// The gate to pass. `None` means **anyone walks in**.
    pub gate: Option<crate::Gate>,
    /// What to run on completion.
    pub action: crate::LaunchAction,
    /// Show the hint once the remaining count falls to this value **or below** (Android's is 3).
    /// `None` stays quiet to the end — that is the one for something you really want hidden.
    ///
    /// The crate **does not draw** the hint. It only holds the intent; the screen reads the
    /// remaining count with [`Cx::knock_remaining`](crate::Cx::knock_remaining) and draws it.
    pub hint_from: Option<u8>,
}

impl fmt::Debug for HiddenEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HiddenEntry")
            .field("id", &self.id)
            .field("trigger", &self.trigger)
            .field("gate", &self.gate)
            .field("hint_from", &self.hint_from)
            .finish_non_exhaustive()
    }
}

impl HiddenEntry {
    /// With an arbitrary trigger. **An integrator's own [`KnockTrigger`] goes in here.**
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        trigger: impl KnockTrigger,
        action: crate::LaunchAction,
    ) -> Self {
        Self {
            id: id.into(),
            trigger: Box::new(trigger),
            gate: None,
            action,
            hint_from: None,
        }
    }

    /// A [`TapKnock`] convenience constructor — the Android way.
    #[must_use]
    pub fn taps(id: impl Into<String>, count: u8, action: crate::LaunchAction) -> Self {
        Self::new(id, TapKnock::new(count), action)
    }

    /// A [`ZoneKnock::corners`] convenience constructor.
    #[must_use]
    pub fn corners(
        id: impl Into<String>,
        order: impl AsRef<[Corner]>,
        action: crate::LaunchAction,
    ) -> Self {
        Self::new(id, ZoneKnock::corners(order), action)
    }

    /// Put a gate on it. **Anything irreversible, like a factory reset or a calibration value, takes one without exception.**
    #[must_use]
    pub fn gate(mut self, gate: impl Into<crate::Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }

    /// Show the hint from `n` remaining down.
    #[must_use]
    pub fn hint_from(mut self, n: u8) -> Self {
        self.hint_from = Some(n);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 480.0))
    }

    fn input<'a>(
        now: Instant,
        taps: &'a [Tap],
        keys: &'a [egui::Key],
        pokes: u8,
    ) -> KnockInput<'a> {
        KnockInput {
            now,
            screen: screen(),
            pokes,
            taps,
            keys,
            slop: 8.0,
        }
    }

    fn still(at: egui::Pos2) -> Tap {
        Tap {
            press: at,
            release: at,
            travel: 0.0,
        }
    }

    /// The Android way: `count - 1` of them advance and the last one opens.
    #[test]
    fn tap_knock_opens_on_the_last_poke() {
        let now = Instant::now();
        let mut k = TapKnock::new(3);
        assert_eq!(k.remaining(), Some(3));
        assert_eq!(k.feed(&input(now, &[], &[], 1)), KnockStep::Advanced);
        assert_eq!(k.remaining(), Some(2));
        assert_eq!(k.feed(&input(now, &[], &[], 1)), KnockStep::Advanced);
        assert_eq!(k.feed(&input(now, &[], &[], 1)), KnockStep::Opened);
        assert_eq!(
            k.remaining(),
            Some(3),
            "after opening it goes back to the start by itself"
        );
    }

    /// Two pokes in one frame count the same as two.
    #[test]
    fn tap_knock_counts_every_poke_in_a_frame() {
        let now = Instant::now();
        let mut k = TapKnock::new(2);
        assert_eq!(k.feed(&input(now, &[], &[], 2)), KnockStep::Opened);
    }

    /// A frame with no input is quiet — nothing happens until the expiry.
    #[test]
    fn tap_knock_is_idle_without_pokes() {
        let now = Instant::now();
        let mut k = TapKnock::new(3);
        assert_eq!(k.feed(&input(now, &[], &[], 0)), KnockStep::Idle);
        assert_eq!(k.feed(&input(now, &[], &[], 1)), KnockStep::Advanced);
        assert_eq!(k.feed(&input(now, &[], &[], 0)), KnockStep::Idle);
    }

    /// Past the gap it goes back to the beginning by itself — it does not accumulate all day.
    #[test]
    fn tap_knock_expires() {
        let now = Instant::now();
        let mut k = TapKnock::new(3).within(Duration::from_millis(300));
        assert_eq!(k.feed(&input(now, &[], &[], 1)), KnockStep::Advanced);
        let late = now + Duration::from_secs(1);
        assert_eq!(k.feed(&input(late, &[], &[], 0)), KnockStep::Reset);
        assert_eq!(k.remaining(), Some(3));
    }

    /// 0 counts as 1 — no door that "opens without being pressed" gets built.
    #[test]
    fn tap_knock_never_opens_for_free() {
        let now = Instant::now();
        let mut k = TapKnock::new(0);
        assert_eq!(k.remaining(), Some(1));
        assert_eq!(k.feed(&input(now, &[], &[], 0)), KnockStep::Idle);
        assert_eq!(k.feed(&input(now, &[], &[], 1)), KnockStep::Opened);
    }

    /// A corner square is a fraction of the short side and resolves against the screen Rect.
    #[test]
    fn zones_resolve_against_the_screen() {
        let r = Zone::corner(Corner::BottomRight, 0.10).resolve(screen());
        assert!((r.min.x - 752.0).abs() < 0.01, "{r:?}");
        assert!((r.min.y - 432.0).abs() < 0.01, "{r:?}");
        assert!(r.contains(egui::pos2(795.0, 475.0)));
        assert!(!r.contains(egui::pos2(700.0, 475.0)));
    }

    /// It opens only when they are touched in order.
    #[test]
    fn zone_knock_wants_the_order() {
        let now = Instant::now();
        let mut k = ZoneKnock::corners([Corner::TopLeft, Corner::BottomRight]);
        assert_eq!(k.len(), 2);
        assert!(!k.is_empty());
        let br = still(egui::pos2(790.0, 470.0));
        let tl = still(egui::pos2(10.0, 10.0));
        assert_eq!(k.feed(&input(now, &[br], &[], 0)), KnockStep::Idle);
        assert_eq!(k.feed(&input(now, &[tl], &[], 0)), KnockStep::Advanced);
        assert_eq!(k.feed(&input(now, &[br], &[], 0)), KnockStep::Opened);
    }

    /// A wrong spot restarts it, and where that spot is the first region it counts as **1**.
    #[test]
    fn a_wrong_zone_restarts_at_one_when_it_is_the_first() {
        let now = Instant::now();
        let mut k = ZoneKnock::corners([Corner::TopLeft, Corner::TopRight, Corner::BottomLeft]);
        let tl = still(egui::pos2(10.0, 10.0));
        assert_eq!(k.feed(&input(now, &[tl], &[], 0)), KnockStep::Advanced);
        // The top left was touched again as the second — it is 1, not 0.
        assert_eq!(k.feed(&input(now, &[tl], &[], 0)), KnockStep::Reset);
        assert_eq!(k.remaining(), Some(2));
    }

    /// A drag is not a knock — an edge swipe must not advance the entry point.
    #[test]
    fn a_dragged_tap_is_not_a_zone_knock() {
        let now = Instant::now();
        let mut k = ZoneKnock::corners([Corner::TopLeft]);
        let dragged = Tap {
            press: egui::pos2(10.0, 10.0),
            release: egui::pos2(14.0, 60.0),
            travel: 50.2,
        };
        assert_eq!(k.feed(&input(now, &[dragged], &[], 0)), KnockStep::Idle);
    }

    /// An empty sequence never opens — an accidental `[]` does not throw the door wide.
    #[test]
    fn an_empty_zone_order_never_opens() {
        let now = Instant::now();
        let mut k = ZoneKnock::new(Vec::new());
        let tl = still(egui::pos2(10.0, 10.0));
        assert_eq!(k.feed(&input(now, &[tl], &[], 0)), KnockStep::Idle);
        assert_eq!(k.len(), 1, "an empty one counts as 1");
    }

    /// A one-corner sequence opens on the first tap (the restart branch does not swallow the opening).
    #[test]
    fn a_single_zone_opens_on_the_first_tap() {
        let now = Instant::now();
        let mut k = ZoneKnock::corners([Corner::TopLeft]);
        let tl = still(egui::pos2(10.0, 10.0));
        assert_eq!(k.feed(&input(now, &[tl], &[], 0)), KnockStep::Opened);
        assert_eq!(k.remaining(), Some(1), "back to the start after opening");
    }

    /// A corner knows its two sides (it interlocks with the dock and shade placement).
    #[test]
    fn corners_know_their_edges() {
        assert_eq!(Corner::ALL.len(), 4);
        assert_eq!(Corner::BottomRight.edges(), (Edge::Bottom, Edge::Right));
    }

    /// A quiet frame is recognisable (a trigger uses it to bail out early).
    #[test]
    fn a_quiet_frame_is_recognisable() {
        let now = Instant::now();
        assert!(input(now, &[], &[], 0).is_quiet());
        assert!(!input(now, &[], &[], 1).is_quiet());
        assert!(!input(now, &[], &[egui::Key::F1], 0).is_quiet());
    }

    /// It tells a drag from a tap by the slop.
    #[test]
    fn a_tap_knows_whether_it_moved() {
        let t = Tap {
            press: egui::pos2(0.0, 0.0),
            release: egui::pos2(3.0, 4.0),
            travel: 5.0,
        };
        assert!(t.is_still(8.0));
        assert!(!t.is_still(4.0));
    }

    /// The registration struct prints a readable `Debug` even with the trigger hidden.
    #[test]
    fn a_hidden_entry_prints_its_id_and_trigger() {
        let e = HiddenEntry::taps("service", 7, crate::LaunchAction::open("service_menu"))
            .gate("service")
            .hint_from(3);
        let s = format!("{e:?}");
        assert!(s.contains("service"), "{s}");
        assert!(s.contains("TapKnock"), "{s}");
    }
}
