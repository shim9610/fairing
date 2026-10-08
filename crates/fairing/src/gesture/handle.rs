//! **Gesture handles** — thin strips at the edge of the glass, swiped the way One Hand
//! Operation+ handles are: straight in or diagonally, let go at once or after a rest, each way
//! bound to a [`LaunchAction`].
//!
//! A handle sits on the left, the right or the bottom edge, a few millimetres thick,
//! over the stretch of the edge it is given: the glass's outermost millimetres there. The bottom
//! edge is the nav bar's or the handles', not both, so a handle goes there only with
//! the nav bar off. Every press that begins in the strip is the
//! handle's, as One Hand Operation+ has it: nothing under the strip sees one — not a
//! screen's list or button, not a desktop page — and the shell's own edge gesture does not start
//! there. A stroke that runs in from the edge is the handle's swipe; a tap, or a stroke along the
//! edge, does nothing. Just inward of the strip all is as it would have been: the back gesture
//! starts there on a side handle's edge.
//!
//! A handle is a [`GestureRegion`] the shell places itself: the strip is where the
//! region is, and the swipe is the region's touch.
//!
//! A swipe counts once it has run [`GestureHandle::reach_mm`] from where it began. Its way is its
//! angle off straight in: within [`GestureHandle::diagonal_from`] it is straight, past it it is
//! diagonal (up or down on the sides, left or right on the bottom), and past 70° it runs along
//! the edge and is no gesture at all.
//! Let go, it is the short gesture of that way. Held still for [`GestureHandle::long_after`] once
//! it counts, it is the long one, which runs there and then — where the handle has one for that
//! way; where it has none, the rest changes nothing and the release runs the short one.
//!
//! Nothing follows the finger but an arrow that says which gesture the swipe is: the action runs
//! when the gesture completes. Every gesture should have another way to the same place — a desktop
//! icon, a nav bar item, a button.

use super::{Edge, GestureRegion, Phase, RegionCx, RegionLook, RegionPlaceCx, RegionTouch};
use crate::access::Gate;
use crate::icons::IconSet;
use crate::screen::LaunchAction;
use crate::theme::{ColorRole, Theme};
use crate::unit::Scale;
use egui::{Pos2, Rect, Stroke, Vec2};
use std::time::{Duration, Instant};

/// The most handles on one edge — One Hand Operation+'s three.
pub const MAX_HANDLES_A_SIDE: usize = 3;
/// Past this angle off straight in, a swipe runs along the edge: scrolling, not a handle's.
pub(crate) const MAX_ANGLE_DEG: f32 = 70.0;

/// Which way a handle is swiped, seen from its edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HandleDirection {
    /// Straight in from the edge.
    Straight,
    /// In and up, from the left or the right edge.
    DiagonalUp,
    /// In and down, from the left or the right edge.
    DiagonalDown,
    /// Up and to the left, from the bottom edge.
    DiagonalLeft,
    /// Up and to the right, from the bottom edge.
    DiagonalRight,
}

impl HandleDirection {
    /// All five, straight first.
    pub const ALL: [Self; 5] = [
        Self::Straight,
        Self::DiagonalUp,
        Self::DiagonalDown,
        Self::DiagonalLeft,
        Self::DiagonalRight,
    ];

    /// **The three ways a handle on `edge` is swiped**, straight first: up and down on the left
    /// and the right, left and right on the bottom (and the top).
    #[must_use]
    pub const fn on(edge: Edge) -> [Self; 3] {
        match edge {
            Edge::Left | Edge::Right => [Self::Straight, Self::DiagonalUp, Self::DiagonalDown],
            Edge::Top | Edge::Bottom => [Self::Straight, Self::DiagonalLeft, Self::DiagonalRight],
        }
    }

    /// Whether a handle on `edge` is swiped this way.
    #[must_use]
    pub fn fits(self, edge: Edge) -> bool {
        Self::on(edge).contains(&self)
    }
}

/// One gesture on a handle: a way, let go at once or after a rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HandleGesture {
    /// Which way.
    pub direction: HandleDirection,
    /// Whether it rests before it is let go — the long gesture.
    pub long: bool,
}

impl HandleGesture {
    /// The short gesture of `direction`: swiped and let go.
    #[must_use]
    pub const fn short(direction: HandleDirection) -> Self {
        Self {
            direction,
            long: false,
        }
    }

    /// The long gesture of `direction`: swiped, then held still.
    #[must_use]
    pub const fn long(direction: HandleDirection) -> Self {
        Self {
            direction,
            long: true,
        }
    }
}

/// What a handle gesture runs: an action, and the gate in front of it.
#[derive(Debug, Clone, PartialEq)]
pub struct HandleAction {
    pub(crate) action: LaunchAction,
    pub(crate) gate: Option<Gate>,
}

impl HandleAction {
    /// `action`, with no gate.
    #[must_use]
    pub fn new(action: LaunchAction) -> Self {
        Self { action, gate: None }
    }

    /// Ask for `gate` first. A session that does not pass it gets the unlock prompt, and the
    /// action runs once it is passed.
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }
}

impl From<LaunchAction> for HandleAction {
    fn from(action: LaunchAction) -> Self {
        Self::new(action)
    }
}

/// **Where a handle's stretch of its edge ends**, at either end: the top and the
/// bottom on the sides, the left and the right on the bottom. [`GestureHandle::along`] measures
/// its shares between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HandleEnd {
    /// The shell's own band at that end: the bar where it shows, `[theme.metrics] edge_px` deep
    /// where it does not (the back gesture's band at a bottom handle's ends). The strip leaves
    /// that edge's gesture its corner: the shade's pull, the home swipe, back. The default.
    #[default]
    EdgeZone,
    /// The bar at that end where it shows, the glass's edge where it is hidden. A bottom handle
    /// has no bar at its ends, so there it is the glass's edge.
    Bar,
    /// The glass's edge, over a bar where one shows.
    Glass,
}

/// **A gesture handle**: a thin strip at the edge of the glass and the gestures it
/// answers. Add it with [`Shell::add_gesture_handle`](crate::Shell::add_gesture_handle).
///
/// On the left and the right edges the strip is the glass's outermost millimetres between the
/// bars, and ends above the on-screen keyboard while it is up; [`ends`](Self::ends) says where
/// else it may end. The bottom edge is the nav bar's or the handles', not both:
/// a handle goes there only with the nav bar off (`[nav_bar] enabled = false`), and its
/// strip is then the glass's last millimetres, standing aside while the keyboard is up.
///
/// ```
/// use fairing::gesture::{
///     Edge, GestureHandle, HandleAction, HandleDirection, HandleEnd, HandleGesture,
/// };
/// use fairing::LaunchAction;
///
/// # fn add(shell: &mut fairing::Shell) -> fairing::Result<()> {
/// shell.add_gesture_handle(
///     GestureHandle::new("right", Edge::Right)
///         // The lower two thirds of the right edge, from under the status bar to the glass's
///         // bottom, over the nav bar.
///         .ends(HandleEnd::EdgeZone, HandleEnd::Glass)
///         .along(0.33, 1.0)
///         .gesture(HandleGesture::short(HandleDirection::Straight), LaunchAction::open("batches"))
///         .gesture(HandleGesture::short(HandleDirection::DiagonalUp), LaunchAction::OpenOverview)
///         .gesture(
///             HandleGesture::long(HandleDirection::Straight),
///             HandleAction::new(LaunchAction::open("service_menu")).gate("service"),
///         ),
/// )?;
/// // The bottom edge, with the nav bar off (`[nav_bar] enabled = false`).
/// shell.add_gesture_handle(
///     GestureHandle::new("bottom", Edge::Bottom)
///         // The middle third of the width.
///         .along(0.33, 0.67)
///         .gesture(HandleGesture::short(HandleDirection::Straight), LaunchAction::open("batches"))
///         .gesture(HandleGesture::short(HandleDirection::DiagonalLeft), LaunchAction::OpenOverview),
/// )?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct GestureHandle {
    pub(crate) id: String,
    pub(crate) edge: Edge,
    pub(crate) along: (f32, f32),
    pub(crate) ends: (HandleEnd, HandleEnd),
    pub(crate) thickness_mm: f32,
    pub(crate) reach_mm: f32,
    pub(crate) diagonal_deg: f32,
    pub(crate) long_after: Duration,
    pub(crate) visible: bool,
    pub(crate) gestures: Vec<(HandleGesture, HandleAction)>,
}

impl GestureHandle {
    /// A handle on `edge` — [`Edge::Left`], [`Edge::Right`] or [`Edge::Bottom`] — over the whole
    /// edge, two millimetres thick, answering nothing yet.
    #[must_use]
    pub fn new(id: impl Into<String>, edge: Edge) -> Self {
        Self {
            id: id.into(),
            edge,
            along: (0.0, 1.0),
            ends: (HandleEnd::EdgeZone, HandleEnd::EdgeZone),
            thickness_mm: 2.0,
            reach_mm: 10.0,
            diagonal_deg: 25.0,
            long_after: Duration::from_millis(400),
            visible: false,
            gestures: Vec::new(),
        }
    }

    /// **Where along the edge it sits**, as shares of the edge between its two
    /// [`ends`](Self::ends): 0 at the top and 1 at the bottom on the sides, 0 at the left and 1 at
    /// the right on the bottom. With the on-screen keyboard up, a side strip ends above the keys.
    #[must_use]
    pub fn along(mut self, from: f32, to: f32) -> Self {
        self.along = (from, to);
        self
    }

    /// **Where the edge it is measured over ends**: `from` at the top and `to` at the
    /// bottom on the sides, `from` at the left and `to` at the right on the bottom. The shell's
    /// edge zones by default ([`HandleEnd::EdgeZone`]), which are the bars where they show; a
    /// strip that should reach the glass's very end takes [`HandleEnd::Glass`].
    #[must_use]
    pub fn ends(mut self, from: HandleEnd, to: HandleEnd) -> Self {
        self.ends = (from, to);
        self
    }

    /// **How thick the strip is**, in millimetres. Two by default: every press inside it is the
    /// handle's, so it stays thin enough to leave the content beside it alone, and wide enough that
    /// a swipe in from the bezel lands in it.
    #[must_use]
    pub fn thickness_mm(mut self, mm: f32) -> Self {
        self.thickness_mm = mm;
        self
    }

    /// **How far a swipe runs before it counts**, in millimetres from where it began. Ten by
    /// default.
    #[must_use]
    pub fn reach_mm(mut self, mm: f32) -> Self {
        self.reach_mm = mm;
        self
    }

    /// **Where diagonal begins**, in degrees off straight in. 25 by default; up to 70, past which a
    /// swipe runs along the edge and is no handle's.
    #[must_use]
    pub fn diagonal_from(mut self, degrees: f32) -> Self {
        self.diagonal_deg = degrees;
        self
    }

    /// **How long a rest makes a long gesture.** 400 ms by default.
    #[must_use]
    pub fn long_after(mut self, rest: Duration) -> Self {
        self.long_after = rest;
        self
    }

    /// **Show the strip at rest.** Off by default: the strip is drawn only while it is swiped. Shown
    /// or not, it takes the presses that begin in it.
    #[must_use]
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// **Run `action` on `gesture`.** A gesture given again replaces what it ran. Its way has to
    /// be one of the edge's ([`HandleDirection::on`]).
    #[must_use]
    pub fn gesture(mut self, gesture: HandleGesture, action: impl Into<HandleAction>) -> Self {
        let action = action.into();
        if let Some(slot) = self.gestures.iter_mut().find(|(g, _)| *g == gesture) {
            slot.1 = action;
        } else {
            self.gestures.push((gesture, action));
        }
        self
    }

    /// Its id — what [`ShellEvent::Gesture`](crate::ShellEvent::Gesture) carries.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The edge it is on.
    #[must_use]
    pub fn edge(&self) -> Edge {
        self.edge
    }

    /// What `gesture` runs on this handle, if anything.
    #[must_use]
    pub fn action(&self, gesture: HandleGesture) -> Option<&HandleAction> {
        self.gestures
            .iter()
            .find(|(g, _)| *g == gesture)
            .map(|(_, action)| action)
    }

    /// What is wrong with it, if anything — checked by `add_gesture_handle`.
    pub(crate) fn fault(&self) -> Option<String> {
        let (from, to) = self.along;
        let foreign = self
            .gestures
            .iter()
            .find(|(g, _)| !g.direction.fits(self.edge));
        if self.edge == Edge::Top {
            Some("a handle goes on the left, the right or the bottom edge".to_owned())
        } else if let Some((gesture, _)) = foreign {
            Some(format!(
                "`{:?}` is not a way on the {:?} edge",
                gesture.direction, self.edge
            ))
        } else if !(0.0..=1.0).contains(&from) || !(0.0..=1.0).contains(&to) || from >= to {
            Some(format!(
                "`along({from}, {to})` is not a stretch of the edge"
            ))
        } else if !(self.thickness_mm > 0.0 && self.thickness_mm.is_finite()) {
            Some(format!("thickness {} mm", self.thickness_mm))
        } else if !(self.reach_mm > 0.0 && self.reach_mm.is_finite()) {
            Some(format!("reach {} mm", self.reach_mm))
        } else if !(self.diagonal_deg > 0.0 && self.diagonal_deg < MAX_ANGLE_DEG) {
            Some(format!(
                "diagonal from {}° — between 0 and {MAX_ANGLE_DEG}",
                self.diagonal_deg
            ))
        } else {
            None
        }
    }

    /// Whether its stretch of the edge overlaps `other`'s on the same edge.
    pub(crate) fn overlaps(&self, other: &Self) -> bool {
        self.edge == other.edge && self.along.0 < other.along.1 && other.along.0 < self.along.1
    }
}

/// **Which way `offset` runs from a handle on `edge`**, diagonal from `diagonal_deg` off straight
/// in — `None` for a stroke that does not run in: outward, or along the edge past
/// [`MAX_ANGLE_DEG`].
pub(crate) fn direction_of(offset: Vec2, edge: Edge, diagonal_deg: f32) -> Option<HandleDirection> {
    let inward = offset.dot(edge.inward());
    if inward <= 0.0 {
        return None;
    }
    // Along the edge: downward on the sides, rightward on the top and the bottom.
    let angle = offset.dot(edge.along()).atan2(inward).to_degrees();
    let [straight, back, ahead] = HandleDirection::on(edge);
    if angle.abs() > MAX_ANGLE_DEG {
        None
    } else if angle >= diagonal_deg {
        Some(ahead)
    } else if angle <= -diagonal_deg {
        Some(back)
    } else {
        Some(straight)
    }
}

/// Whether a stroke of `delta` from a handle on `edge` runs in rather than along — what turns a
/// press in the strip into the handle's swipe.
pub(crate) fn runs_in(delta: Vec2, edge: Edge) -> bool {
    let inward = delta.dot(edge.inward());
    inward > 0.0 && delta.dot(edge.along()).abs() <= inward * MAX_ANGLE_DEG.to_radians().tan()
}

/// **What a gesture handle painter draws one handle with** — its strip, and the swipe
/// from it while there is one. Called every frame for every handle in play.
pub struct HandleLook<'a> {
    /// The handle's id.
    pub id: &'a str,
    /// The strip.
    pub rect: Rect,
    /// The edge it is on.
    pub edge: Edge,
    /// Whether it asked to show at rest ([`GestureHandle::visible`]).
    pub visible: bool,
    /// The swipe from it, while there is one.
    pub swipe: Option<HandleSwipe>,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons — the built-in set and yours.
    pub icons: &'a mut IconSet,
}

/// A swipe from a handle as it stands this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandleSwipe {
    /// Where it began.
    pub origin: Pos2,
    /// Where the finger is.
    pub finger: Pos2,
    /// The gesture it is if it ends now — the long one once a rest has run it. `None` while it
    /// runs no way a handle counts.
    pub gesture: Option<HandleGesture>,
    /// How far it has run, as a share of the reach: 1 and past counts.
    pub reach: f32,
    /// How far a rest has gone towards the long gesture, 0 to 1 — 0 where the handle has no long
    /// gesture that way.
    pub rest: f32,
    /// Whether [`gesture`](Self::gesture) runs anything on this handle.
    pub bound: bool,
    /// Whether it has run already — a long gesture, on its rest. The rest of the swipe reaches
    /// nothing.
    pub done: bool,
}

/// The callback that draws a gesture handle.
pub type HandlePainter = Box<dyn FnMut(&egui::Painter, &mut HandleLook<'_>)>;

/// The unit vector a gesture's arrow points along, from a handle on `edge`.
pub(crate) fn arrow_of(direction: HandleDirection, edge: Edge) -> Vec2 {
    let inward = edge.inward();
    match direction {
        HandleDirection::Straight => inward,
        HandleDirection::DiagonalUp => (inward - Vec2::Y).normalized(),
        HandleDirection::DiagonalDown => (inward + Vec2::Y).normalized(),
        HandleDirection::DiagonalLeft => (inward - Vec2::X).normalized(),
        HandleDirection::DiagonalRight => (inward + Vec2::X).normalized(),
    }
}

/// **The built-in handle**: the strip, faint, where it asked to show; and while it is swiped, a
/// disc ahead of the finger with an arrow the way the swipe runs — filling as it reaches, in the
/// accent once it counts and runs something, ringed as a rest makes it long.
pub(crate) fn paint_handle(painter: &egui::Painter, look: &HandleLook<'_>) {
    let theme = look.theme;
    if look.visible || look.swipe.is_some() {
        let radius = look.rect.width().min(look.rect.height()) * 0.5;
        painter.rect_filled(
            look.rect,
            crate::unit::round_u8(radius),
            theme
                .color(ColorRole::Outline)
                .gamma_multiply(theme.control.disabled_alpha),
        );
    }
    let Some(swipe) = look.swipe else {
        return;
    };
    let Some(gesture) = swipe.gesture else {
        return;
    };
    let arrow = arrow_of(gesture.direction, look.edge);
    let r = theme.control.icon * 0.9;
    let center = swipe.finger + arrow * (r * 1.8);
    let counts = swipe.reach >= 1.0;
    let lit = (counts && swipe.bound) || swipe.done;
    let (fill, ink) = if lit {
        (
            theme.color(ColorRole::Primary),
            theme.color(ColorRole::OnPrimary),
        )
    } else {
        (
            theme
                .color(ColorRole::SurfaceVariant)
                .gamma_multiply(swipe.reach.clamp(0.0, 1.0).mul_add(0.5, 0.5)),
            theme.color(if swipe.bound {
                ColorRole::OnSurface
            } else {
                ColorRole::Muted
            }),
        )
    };
    painter.circle_filled(center, r, fill);
    let width = (r * 0.16).max(1.0);
    let tip = center + arrow * (r * 0.45);
    let tail = center - arrow * (r * 0.45);
    painter.line_segment([tail, tip], Stroke::new(width, ink));
    let head = r * 0.3;
    for turn in [2.4_f32, -2.4] {
        let (sin, cos) = turn.sin_cos();
        let back = Vec2::new(
            arrow.x.mul_add(cos, -arrow.y * sin),
            arrow.x.mul_add(sin, arrow.y * cos),
        );
        painter.line_segment([tip, tip + back * head], Stroke::new(width, ink));
    }
    if swipe.rest > 0.0 || (swipe.done && gesture.long) {
        let alpha = if swipe.done { 1.0 } else { swipe.rest };
        painter.circle_stroke(
            center,
            r + width * 1.5,
            Stroke::new(width, theme.color(ColorRole::Primary).gamma_multiply(alpha)),
        );
    }
}

/// **A gesture handle at work**: the shell's own region — the strip it is placed
/// at each frame, and the swipe from it followed to its gesture.
pub(crate) struct HandleRegion {
    /// The handle.
    pub(crate) handle: GestureHandle,
    /// The touch in the strip, while there is one.
    state: Option<HandleTouch>,
}

/// A touch in a handle's strip.
#[derive(Debug, Clone, Copy, PartialEq)]
enum HandleTouch {
    /// Within the slop yet.
    Pressed,
    /// Past the slop along the edge or outward: nothing of the handle's, to the release.
    Elsewhere,
    /// Past the slop running in: a swipe.
    Swipe(HandleTrack),
}

/// A swipe a handle is following, from its start to its end.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HandleTrack {
    /// Where the swipe began.
    origin: Pos2,
    /// Where the finger is.
    finger: Pos2,
    /// Where the finger last came to rest, and when.
    still: (Pos2, Instant),
    /// Whether its gesture has run — a long one, on its rest. The rest of the swipe reaches
    /// nothing.
    done: bool,
}

impl HandleRegion {
    /// `handle`, at rest.
    pub(crate) fn new(handle: GestureHandle) -> Self {
        Self {
            handle,
            state: None,
        }
    }

    /// The swipe from the strip, while there is one.
    fn swipe(&self) -> Option<&HandleTrack> {
        match &self.state {
            Some(HandleTouch::Swipe(track)) => Some(track),
            _ => None,
        }
    }

    /// Once the touch has passed the slop, whether it runs in: a swipe, or nothing to its
    /// release.
    fn decide(&mut self, touch: &RegionTouch, now: Instant) {
        if self.state != Some(HandleTouch::Pressed) || !touch.moved {
            return;
        }
        self.state = Some(if runs_in(touch.offset(), self.handle.edge) {
            HandleTouch::Swipe(HandleTrack {
                origin: touch.origin,
                finger: touch.pos,
                still: (touch.pos, now),
                done: false,
            })
        } else {
            HandleTouch::Elsewhere
        });
    }

    /// A frame of the swipe: where the finger is, and whether it has rested long enough, far
    /// enough in, to be the long gesture. A finger at rest sends nothing, but frames keep coming
    /// while a touch is under way (the shell's wake, as for an edge swipe's pause).
    fn follow(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        self.decide(touch, cx.now());
        let still_px = cx.slop_px() * 0.5;
        let Some(HandleTouch::Swipe(track)) = self.state.as_mut() else {
            return;
        };
        if (touch.pos - track.still.0).length() > still_px {
            track.still = (touch.pos, cx.now());
        }
        track.finger = touch.pos;
        if track.done {
            return;
        }
        let track = *track;
        let handle = &self.handle;
        let offset = touch.pos - track.origin;
        let Some(direction) = direction_of(offset, handle.edge, handle.diagonal_deg) else {
            return;
        };
        let long = HandleGesture::long(direction);
        if offset.length() < cx.scale().mm_to_du(handle.reach_mm) || handle.action(long).is_none() {
            return;
        }
        if cx.now().saturating_duration_since(track.still.1) < handle.long_after {
            return;
        }
        if let Some(HandleTouch::Swipe(track)) = self.state.as_mut() {
            track.done = true;
        }
        self.run(long, cx);
    }

    /// The release: the short gesture of the swipe's way, where it ran far enough and the handle
    /// has one — unless a rest has run the long one already.
    fn let_go(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        self.decide(touch, cx.now());
        let Some(HandleTouch::Swipe(track)) = self.state.take() else {
            return;
        };
        let handle = &self.handle;
        let offset = touch.pos - track.origin;
        if track.done || offset.length() < cx.scale().mm_to_du(handle.reach_mm) {
            return;
        }
        let Some(direction) = direction_of(offset, handle.edge, handle.diagonal_deg) else {
            return;
        };
        self.run(HandleGesture::short(direction), cx);
    }

    /// Run `gesture`, where the handle has it: say so, then its action — behind its gate, where it
    /// has one.
    fn run(&self, gesture: HandleGesture, cx: &mut RegionCx<'_>) {
        let Some(action) = self.handle.action(gesture).cloned() else {
            return;
        };
        // Like a hidden entry, it does not happen quietly.
        cx.out.events.push(crate::ShellEvent::Gesture {
            handle: self.handle.id.clone(),
            gesture,
        });
        match action.gate {
            Some(gate) => cx.launch_gated(action.action, gate),
            None => cx.launch(action.action),
        }
    }
}

impl GestureRegion for HandleRegion {
    /// **The strip**: the glass's outermost millimetres along the handle's edge, over its stretch
    /// between its [`ends`](GestureHandle::ends). A side strip ends above the keyboard while it is
    /// up; a bottom strip stands aside while the keyboard is up, and while the nav bar is on — the
    /// bottom edge is then the bar's. None on an edge the screen keeps (`edge_guard`):
    /// there the strip is not there at all, and the edge's presses are the screen's.
    fn place(&mut self, cx: &RegionPlaceCx<'_>) -> Option<Rect> {
        let handle = &self.handle;
        if cx.blocked.contains(handle.edge) {
            return None;
        }
        let thick = cx.scale.mm_to_du(handle.thickness_mm);
        let (from, to) = handle.along;
        let share = |a: f32, b: f32, t: f32| (b - a).mul_add(t, a);
        let screen = cx.screen;
        match handle.edge {
            Edge::Left | Edge::Right => {
                let top = end_of(cx, handle.ends.0, Edge::Top);
                let floor = end_of(cx, handle.ends.1, Edge::Bottom);
                // One Hand Operation+'s "fit to the keyboard": a strip down the keys' side would
                // take the outer edge of their outermost keys. The keys slide, and a strip is a
                // frame behind them (last frame's layout) with egui's hit test a frame behind
                // that, so `keys` reaches up to where they will stop while they come up.
                let bottom = cx.keys.map_or(floor, |keys| floor.min(keys.min.y));
                if bottom <= top {
                    return None;
                }
                let y = share(top, bottom, from)..=share(top, bottom, to);
                let x = if handle.edge == Edge::Left {
                    screen.min.x..=screen.min.x + thick
                } else {
                    screen.max.x - thick..=screen.max.x
                };
                Some(Rect::from_x_y_ranges(x, y))
            }
            Edge::Bottom if cx.keys.is_none() && !cx.nav_bar_on => {
                let left = end_of(cx, handle.ends.0, Edge::Left);
                let right = end_of(cx, handle.ends.1, Edge::Right);
                if right <= left {
                    return None;
                }
                let x = share(left, right, from)..=share(left, right, to);
                Some(Rect::from_x_y_ranges(
                    x,
                    screen.max.y - thick..=screen.max.y,
                ))
            }
            Edge::Bottom | Edge::Top => None,
        }
    }

    /// **Follow a swipe from the strip**, and run its gesture when it completes: a rest past the
    /// reach runs the long gesture of its way where the handle has one, and the release runs the
    /// short one.
    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        match touch.phase {
            Phase::Started => {
                self.state = Some(HandleTouch::Pressed);
                self.follow(touch, cx);
            }
            Phase::Moved => self.follow(touch, cx),
            Phase::Ended => self.let_go(touch, cx),
            Phase::Cancelled => self.state = None,
        }
    }

    /// **Draw the handle** through the integrator's painter, or the built-in one.
    fn paint(&mut self, painter: &egui::Painter, look: &mut RegionLook<'_>) {
        let swipe = self
            .swipe()
            .map(|track| swipe_look(&self.handle, track, look.scale, look.now));
        let mut handle_look = HandleLook {
            id: &self.handle.id,
            rect: look.rect,
            edge: self.handle.edge,
            visible: self.handle.visible,
            swipe,
            theme: look.theme,
            icons: &mut *look.icons,
        };
        match look.handle_painter.as_deref_mut() {
            Some(custom) => custom(painter, &mut handle_look),
            None => paint_handle(painter, &handle_look),
        }
    }
}

/// Where a handle's stretch ends at `side` — the top or the bottom of a side handle's edge, the
/// left or the right of a bottom handle's — for `end`.
fn end_of(cx: &RegionPlaceCx<'_>, end: HandleEnd, side: Edge) -> f32 {
    let screen = cx.screen;
    let zone = cx.edge_zone(side);
    match (end, side) {
        (HandleEnd::EdgeZone, Edge::Top) => zone.max.y.max(screen.min.y),
        (HandleEnd::EdgeZone, Edge::Bottom) => zone.min.y.min(screen.max.y),
        (HandleEnd::EdgeZone, Edge::Left) => zone.max.x.max(screen.min.x),
        (HandleEnd::EdgeZone, Edge::Right) => zone.min.x.min(screen.max.x),
        (HandleEnd::Bar, Edge::Top) => cx
            .status_bar
            .map_or(screen.min.y, |bar| bar.max.y.max(screen.min.y)),
        (HandleEnd::Bar, Edge::Bottom) => cx
            .nav_bar
            .map_or(screen.max.y, |bar| bar.min.y.min(screen.max.y)),
        (HandleEnd::Bar | HandleEnd::Glass, Edge::Left) => screen.min.x,
        (HandleEnd::Bar | HandleEnd::Glass, Edge::Right) => screen.max.x,
        (HandleEnd::Glass, Edge::Top) => screen.min.y,
        (HandleEnd::Glass, Edge::Bottom) => screen.max.y,
    }
}

/// How a swipe from `handle` stands this frame.
fn swipe_look(
    handle: &GestureHandle,
    track: &HandleTrack,
    scale: Scale,
    now: Instant,
) -> HandleSwipe {
    let offset = track.finger - track.origin;
    let reach = offset.length() / scale.mm_to_du(handle.reach_mm).max(1.0);
    let direction = direction_of(offset, handle.edge, handle.diagonal_deg);
    let has_long = direction.is_some_and(|d| handle.action(HandleGesture::long(d)).is_some());
    let rest = if has_long && reach >= 1.0 && !track.done {
        let rested = now.saturating_duration_since(track.still.1).as_secs_f32();
        (rested / handle.long_after.as_secs_f32().max(1e-3)).min(1.0)
    } else {
        0.0
    };
    let gesture = direction.map(|d| {
        if track.done && has_long {
            HandleGesture::long(d)
        } else {
            HandleGesture::short(d)
        }
    });
    HandleSwipe {
        origin: track.origin,
        finger: track.finger,
        gesture,
        reach,
        rest,
        bound: gesture.is_some_and(|g| handle.action(g).is_some()),
        done: track.done,
    }
}

#[cfg(test)]
mod tests {
    use super::{direction_of, runs_in, GestureHandle, HandleDirection, HandleGesture};
    use crate::gesture::Edge;
    use egui::vec2;

    /// **The angle off straight in decides the way**, mirrored on the left and the right edges,
    /// and left and right on the bottom one.
    #[test]
    fn the_angle_decides_the_way() {
        assert_eq!(
            direction_of(vec2(40.0, -3.0), Edge::Left, 25.0),
            Some(HandleDirection::Straight)
        );
        assert_eq!(
            direction_of(vec2(40.0, -40.0), Edge::Left, 25.0),
            Some(HandleDirection::DiagonalUp)
        );
        assert_eq!(
            direction_of(vec2(-40.0, 40.0), Edge::Right, 25.0),
            Some(HandleDirection::DiagonalDown)
        );
        assert_eq!(direction_of(vec2(-40.0, 0.0), Edge::Left, 25.0), None);
        assert_eq!(
            direction_of(vec2(5.0, -60.0), Edge::Left, 25.0),
            None,
            "along the edge is scrolling"
        );
        assert_eq!(
            direction_of(vec2(3.0, -40.0), Edge::Bottom, 25.0),
            Some(HandleDirection::Straight)
        );
        assert_eq!(
            direction_of(vec2(-40.0, -40.0), Edge::Bottom, 25.0),
            Some(HandleDirection::DiagonalLeft)
        );
        assert_eq!(
            direction_of(vec2(40.0, -40.0), Edge::Bottom, 25.0),
            Some(HandleDirection::DiagonalRight)
        );
        assert_eq!(
            direction_of(vec2(0.0, 40.0), Edge::Bottom, 25.0),
            None,
            "down from the bottom is outward"
        );
        assert_eq!(
            direction_of(vec2(-60.0, -5.0), Edge::Bottom, 25.0),
            None,
            "along the bottom"
        );
    }

    /// A press in the strip is the handle's only once it runs in, not along.
    #[test]
    fn a_stroke_along_the_edge_is_not_the_handles() {
        assert!(runs_in(vec2(-20.0, 10.0), Edge::Right));
        assert!(!runs_in(vec2(4.0, 30.0), Edge::Left));
        assert!(!runs_in(vec2(-20.0, 0.0), Edge::Left));
        assert!(runs_in(vec2(10.0, -20.0), Edge::Bottom));
        assert!(!runs_in(vec2(30.0, -4.0), Edge::Bottom));
    }

    /// A handle on the top edge, with no stretch, or with a way foreign to its edge is refused —
    /// the bottom edge takes one.
    #[test]
    fn a_handle_off_its_edges_is_refused() {
        assert!(GestureHandle::new("t", Edge::Top).fault().is_some());
        assert!(GestureHandle::new("r", Edge::Right).fault().is_none());
        assert!(GestureHandle::new("b", Edge::Bottom).fault().is_none());
        assert!(GestureHandle::new("r", Edge::Right)
            .along(0.5, 0.5)
            .fault()
            .is_some());
        assert!(GestureHandle::new("r", Edge::Right)
            .diagonal_from(80.0)
            .fault()
            .is_some());
        let left_way = HandleGesture::short(HandleDirection::DiagonalLeft);
        let up_way = HandleGesture::short(HandleDirection::DiagonalUp);
        let open = crate::LaunchAction::open("a");
        assert!(GestureHandle::new("r", Edge::Right)
            .gesture(left_way, open.clone())
            .fault()
            .is_some());
        assert!(GestureHandle::new("b", Edge::Bottom)
            .gesture(up_way, open.clone())
            .fault()
            .is_some());
        assert!(GestureHandle::new("b", Edge::Bottom)
            .gesture(left_way, open)
            .fault()
            .is_none());
        let a = GestureHandle::new("a", Edge::Left).along(0.0, 0.5);
        let b = GestureHandle::new("b", Edge::Left).along(0.4, 1.0);
        let c = GestureHandle::new("c", Edge::Right).along(0.4, 1.0);
        assert!(a.overlaps(&b) && !a.overlaps(&c));
    }
}
