//! **Gesture regions** — a stretch of the glass that takes every touch beginning in it,
//! and hears that touch from its press to its release. A [`GestureRegion`] of your own says where
//! it is each frame, follows its touches and draws itself: a part of a screen used as a trackpad,
//! a jog dial, a strip for a gesture of your own. The gesture handles are the shell's own
//! regions, built on the same trait.
//!
//! What the shell keeps for every region:
//! - **Placing.** [`GestureRegion::place`] says where the region is this frame, or that it stands
//!   aside. It is asked over a screen or the desktop only: not over the shade, the unlock prompt
//!   or the recent screens, and not at all with `[gesture] enabled = false`.
//! - **The touch.** A press inside the region is the region's to its release. Nothing under the
//!   region sees it, not a widget and not a desktop page, and no gesture of the shell's starts
//!   from it. [`GestureRegion::touch`] hears it every frame: [`Phase::Started`] on the press,
//!   [`Phase::Moved`] on each frame after (a finger at rest included, as frames keep coming while
//!   a finger is down), [`Phase::Ended`] on the release, and [`Phase::Cancelled`] when the touch
//!   is taken: the shade, the prompt or the recent screens coming over the regions, the pointer
//!   lost, gestures turned off.
//! - **Order.** Where two regions overlap, the one added later is on top and takes the press.
//! - **The emergency gesture** works through every region: two seconds still in a top
//!   corner. So do the hidden entries' corner knocks, read from the raw pointer: a tap in a
//!   region over a knock corner still counts as a knock.
//! - **Drawing.** [`GestureRegion::paint`] draws it each frame it is placed, on a layer over the
//!   screens and the shade.
//!
//! A region is laid out from last frame's layout, and egui finds what a press hits from where
//! things were the frame before, so a region that moves or first appears is a frame or two behind
//! the glass.

use super::{Edge, EdgeMask, HandlePainter, Phase, MAX_HANDLES_A_SIDE};
use crate::access::Gate;
use crate::icons::IconSet;
use crate::screen::LaunchAction;
use crate::theme::Theme;
use crate::unit::Scale;
use egui::{Pos2, Rect, Vec2};
use std::any::Any;
use std::time::{Duration, Instant};

/// The most regions of your own a shell takes.
pub const MAX_GESTURE_REGIONS: usize = 8;

/// The most zones a frame hands the engine: three handles on each of the three edges they go on,
/// and every region of your own.
pub(crate) const MAX_ZONES: usize = 3 * MAX_HANDLES_A_SIDE + MAX_GESTURE_REGIONS;

/// **A gesture region**: a stretch of the glass that takes every touch beginning in it.
/// Add one with [`Shell::add_gesture_region`](crate::Shell::add_gesture_region).
///
/// A part of a screen as a trackpad: the finger moves a pointer the app keeps, and a tap is a
/// click. The screen it serves draws the pointer from the same state.
///
/// ```
/// use fairing::gesture::{GestureRegion, Phase, RegionCx, RegionPlaceCx, RegionTouch};
///
/// /// The app's pointer, which the trackpad moves and the screen draws.
/// #[derive(Default)]
/// struct Pointer {
///     at: egui::Pos2,
///     clicks: u32,
/// }
///
/// /// The lower right of one screen, four centimetres a side.
/// struct Trackpad;
///
/// impl GestureRegion for Trackpad {
///     fn place(&mut self, cx: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
///         // Only over the screen it serves, and never over the keyboard.
///         if cx.focused != Some("remote") || cx.keys.is_some() {
///             return None;
///         }
///         let side = egui::Vec2::splat(cx.scale.mm_to_du(40.0));
///         Some(egui::Rect::from_min_size(cx.pane.max - side, side))
///     }
///
///     fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
///         let Some(pointer) = cx.app_mut::<Pointer>() else {
///             return;
///         };
///         match touch.phase {
///             // The pointer goes twice as far as the finger.
///             Phase::Moved => pointer.at += touch.delta * 2.0,
///             // Let go before it went anywhere, it is a click.
///             Phase::Ended if !touch.moved => pointer.clicks += 1,
///             _ => {}
///         }
///     }
/// }
///
/// # fn add(shell: &mut fairing::Shell) -> fairing::Result<()> {
/// shell.add_gesture_region("trackpad", Trackpad)?;
/// # Ok(())
/// # }
/// ```
pub trait GestureRegion {
    /// **Where the region is this frame**, or `None` to stand aside and leave the glass to what
    /// is under it. Asked every frame the regions are in play, before the frame's touch is read.
    /// The rect is clipped to the glass, so `Rect::EVERYTHING` is the whole glass; one with
    /// nothing left of it, or not finite, stands aside.
    fn place(&mut self, cx: &RegionPlaceCx<'_>) -> Option<Rect>;

    /// **A touch that began in the region**, every frame from its press to its end.
    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>);

    /// **Draw the region**, every frame it is placed, on a layer over the screens and the shade.
    /// Nothing by default.
    fn paint(&mut self, _painter: &egui::Painter, _look: &mut RegionLook<'_>) {}
}

/// **What a region is placed by**: the frame as the shell sees it, when
/// [`GestureRegion::place`] is asked.
#[derive(Debug, Clone, Copy)]
pub struct RegionPlaceCx<'a> {
    /// The glass.
    pub screen: Rect,
    /// The content: the glass between the bars, as last laid out.
    pub content: Rect,
    /// The focused screen's declaration id: `None` at home.
    pub focused: Option<&'a str>,
    /// Where the focused screen is laid out: its pane while two are up, the content otherwise.
    pub pane: Rect,
    /// The on-screen keyboard's band, from the top of its keys (or of where they are coming up
    /// to) down to the nav bar, the full width of the glass. `None` while it is down.
    pub keys: Option<Rect>,
    /// The edges the screen on show keeps from the shell's edge gestures: all four on a screen
    /// that guards its edges (`ChromePolicy::edge_guard`).
    pub blocked: EdgeMask,
    /// This frame's scale: millimetres to points.
    pub scale: Scale,
    pub(crate) edge_zones: [Rect; 4],
    /// The status bar where it shows, as last laid out — for a handle's
    /// [`HandleEnd::Bar`](super::HandleEnd::Bar).
    pub(crate) status_bar: Option<Rect>,
    /// The nav bar where it shows, as last laid out.
    pub(crate) nav_bar: Option<Rect>,
    /// Whether the nav bar is on (`[nav_bar] enabled`): then the bottom edge is the bar's, and no
    /// handle goes there.
    pub(crate) nav_bar_on: bool,
}

impl RegionPlaceCx<'_> {
    /// **The shell's own band at `edge`**, where its edge gesture begins: the status bar at the
    /// top and the nav bar at the bottom where they show, and `[theme.metrics] edge_px` deep
    /// where they do not and on the left and the right.
    #[must_use]
    pub fn edge_zone(&self, edge: Edge) -> Rect {
        self.edge_zones
            .get(edge.index())
            .copied()
            .unwrap_or(Rect::NOTHING)
    }
}

/// **A touch in a gesture region** as it stands this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegionTouch {
    /// Where it is in its life: `Started` on the press, `Moved` on each frame after (a finger at
    /// rest included), `Ended` on the release, `Cancelled` when it is taken.
    pub phase: Phase,
    /// Where it came down.
    pub origin: Pos2,
    /// Where the finger is.
    pub pos: Pos2,
    /// How far the finger moved since the frame before (px).
    pub delta: Vec2,
    /// The finger's velocity (px/s), smoothed over the last tenth of a second.
    pub velocity: Vec2,
    /// How long since it came down.
    pub held: Duration,
    /// Whether it has gone past the slop (`[motion] slop_px`) since it came down: it is no
    /// longer a tap.
    pub moved: bool,
}

impl RegionTouch {
    /// How far the finger is from where it came down.
    #[must_use]
    pub fn offset(&self) -> Vec2 {
        self.pos - self.origin
    }
}

/// **What a region hears a touch with**: the shell's time and scale, the app's state,
/// and the shell's actions, run once the region has heard the touch.
pub struct RegionCx<'a> {
    pub(crate) now: Instant,
    pub(crate) scale: Scale,
    pub(crate) slop_px: f32,
    pub(crate) ctx: &'a egui::Context,
    pub(crate) app: Option<&'a mut dyn Any>,
    pub(crate) out: &'a mut RegionOut,
}

impl RegionCx<'_> {
    /// The shell's time: the clock its deadlines run on.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.now
    }

    /// This frame's scale: millimetres to points.
    #[must_use]
    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// How far a finger goes before a press is no longer a tap (px).
    #[must_use]
    pub fn slop_px(&self) -> f32 {
        self.slop_px
    }

    /// The egui context, for what else this frame's input holds: a second finger
    /// (`multi_touch`), the keyboard's modifiers.
    #[must_use]
    pub fn ctx(&self) -> &egui::Context {
        self.ctx
    }

    /// **The app's state**, as [`Shell::frame_with`](crate::Shell::frame_with) lent it: the same
    /// state a screen reads through `Cx::app`. `None` for another type, or where nothing was
    /// lent.
    #[must_use]
    pub fn app<T: Any>(&self) -> Option<&T> {
        self.app.as_ref().and_then(|app| app.downcast_ref())
    }

    /// The same, mutably.
    pub fn app_mut<T: Any>(&mut self) -> Option<&mut T> {
        self.app.as_mut().and_then(|app| app.downcast_mut())
    }

    /// **Run `action`**, once the region has heard the touch: as a nav bar item runs it.
    pub fn launch(&mut self, action: LaunchAction) {
        self.out.launches.push((action, None));
    }

    /// **Run `action` behind `gate`.** A session that does not pass it gets the unlock prompt,
    /// and the action runs once it is passed.
    pub fn launch_gated(&mut self, action: LaunchAction, gate: impl Into<Gate>) {
        self.out.launches.push((action, Some(gate.into())));
    }
}

/// What a region asked for while it heard a touch, for the shell to run after.
#[derive(Debug, Default)]
pub(crate) struct RegionOut {
    /// The actions, each with the gate in front of it.
    pub(crate) launches: Vec<(LaunchAction, Option<Gate>)>,
    /// The events to say: a handle's gesture.
    pub(crate) events: Vec<crate::ShellEvent>,
}

/// **What a region is drawn with**: [`GestureRegion::paint`].
pub struct RegionLook<'a> {
    /// The region's id.
    pub id: &'a str,
    /// Where it is this frame.
    pub rect: Rect,
    /// The touch in it as it last stood, while there is one.
    pub touch: Option<RegionTouch>,
    /// The shell's time.
    pub now: Instant,
    /// This frame's scale.
    pub scale: Scale,
    /// The theme.
    pub theme: &'a Theme,
    /// The icons: the built-in set and yours.
    pub icons: &'a mut IconSet,
    /// The integrator's handle painter, for the shell's own handles.
    pub(crate) handle_painter: Option<&'a mut HandlePainter>,
}

/// This frame's region zones, for the engine: each zone and the region it is, by its place in
/// the shell's list.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RegionZones {
    zones: [(Rect, u8); MAX_ZONES],
    len: usize,
}

impl RegionZones {
    /// No zones.
    pub(crate) const NONE: Self = Self {
        zones: [(Rect::NOTHING, 0); MAX_ZONES],
        len: 0,
    };

    /// Add `rect` for the region `region`, on top of those before it. Past [`MAX_ZONES`] it is
    /// not added.
    pub(crate) fn push(&mut self, rect: Rect, region: u8) {
        if let Some(slot) = self.zones.get_mut(self.len) {
            *slot = (rect, region);
            self.len += 1;
        }
    }

    /// The region a press at `pos` lands in: the topmost, the one added last.
    pub(crate) fn at(&self, pos: Pos2) -> Option<u8> {
        self.zones
            .iter()
            .take(self.len)
            .rev()
            .find(|(rect, _)| rect.contains(pos))
            .map(|&(_, region)| region)
    }

    /// Each zone and its region, bottom first.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Rect, u8)> + '_ {
        self.zones.iter().take(self.len).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::RegionZones;
    use egui::{pos2, Rect};

    /// **A press finds the topmost zone it lands in**: where two overlap, the later one.
    #[test]
    fn a_press_finds_the_topmost_zone() {
        let mut zones = RegionZones::NONE;
        zones.push(Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 100.0)), 3);
        zones.push(Rect::from_min_max(pos2(50.0, 50.0), pos2(150.0, 150.0)), 5);
        assert_eq!(zones.at(pos2(20.0, 20.0)), Some(3));
        assert_eq!(zones.at(pos2(75.0, 75.0)), Some(5), "the later is on top");
        assert_eq!(zones.at(pos2(140.0, 140.0)), Some(5));
        assert_eq!(zones.at(pos2(200.0, 20.0)), None);
    }
}
