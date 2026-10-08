//! **A page comes and goes the way it says** — [`transit`], [`Transit`] and [`Motion`].
//!
//! A screen that keeps several pages behind one rail — the console's tabs — swaps its body in a
//! single frame, because to the shell it is one screen: the two transitions the shell draws (the
//! icon zoom from the desktop, the slide of a screen pushed over another) are about screens and
//! never see a page change inside one. So a page's *identity* says how it enters and how it
//! leaves, by implementing [`Transit`], and [`transit`] draws that: the page going out plays its
//! exit, then the page coming in plays its entry, one page on the screen at a time. The
//! integrator knows one thing — implement the trait, get the motion — and the sequencing, the
//! layers and the input are this module's.
//!
//! **Sequential, not overlapped.** The exit finishes before the entry begins. It reads as a hand
//! setting one card down and picking the next up, and it is what makes the whole thing cheap: at
//! rest the page is drawn straight into the caller's `ui`, and while a transition runs one page
//! is laid out, not two.
//!
//! # What egui allows, and so what a [`Motion`] is
//!
//! A page can be moved (drawn at an offset), scaled (a layer transform on a sublayer of its own),
//! faded (`Ui::set_opacity`) and cut by an axis-aligned edge (`Ui::set_clip_rect`). That is the
//! whole vocabulary, and [`Motion`] is exactly it. There is no rotation and no diagonal edge: egui
//! has no stencil, and a clip is a rectangle. The move is a real offset of the drawn rect rather
//! than a layer translation, because a layer translation carries its clip along with it and a
//! page sliding out would be painted over the chrome around it; the clip stays put, and what
//! slides out of it is cut.
//!
//! The presets are starting points, and they compose: a page can come from beyond any edge
//! ([`Motion::fly`]) or any direction ([`Motion::far`]), grow in ([`Motion::zoom`]), and take a
//! scale, a fade, a wipe or a curve of its own on top — `Motion::fly(Side::Right).scaled(0.9)`,
//! or `.over(Tween::back_out(..))` to land with a little give.
//!
//! # A tap during a transition
//!
//! The moving page is drawn in an area of its own that takes no input, so nothing on a page on
//! its way out or in can be pressed; the rest comes soon enough. Between transitions the page is
//! the caller's `ui` itself, and takes input as it always did.

use crate::motion::{Easing, Tween};
use crate::Cx;
use egui::emath::TSTransform;
use egui::{LayerId, Rect, Ui, UiBuilder, Vec2};
use std::time::Duration;

/// Where the container keeps its stage between frames.
const TRANSIT_KEY: &str = "fairing.layout.transit";

/// How far a [`Motion::slide`] travels, as a fraction of the page's size on that axis: short, so
/// the page is seen to *follow* rather than to arrive from off the screen.
pub const SLIDE: f32 = 0.08;

/// The scale a [`Motion::CARD`] is set down at — the same step the shell's fallback zoom takes.
pub const CARD_SCALE: f32 = 0.96;

/// **A side of the page** — where an entering page comes from, or a leaving page goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Above the page.
    Top,
    /// Below the page.
    Bottom,
    /// To the left of it.
    Left,
    /// To the right of it.
    Right,
}

impl Side {
    /// A unit step outwards, towards this side.
    fn outward(self) -> Vec2 {
        match self {
            Self::Top => Vec2::new(0.0, -1.0),
            Self::Bottom => Vec2::new(0.0, 1.0),
            Self::Left => Vec2::new(-1.0, 0.0),
            Self::Right => Vec2::new(1.0, 0.0),
        }
    }
}

/// **How a page moves** at the far end of its entry or its exit: the position it comes from or
/// goes to, the scale there, whether it is transparent there, and whether an edge cuts it on the
/// way instead. An entry plays this from the far end to rest; an exit plays it from rest to the
/// far end. A page names one for each in [`Transit`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// Where the far end is, as a fraction of the page's width and height: `(0.0, 0.08)` is
    /// eight per cent of the height below. Zero for a page that does not move.
    pub offset: Vec2,
    /// The scale at the far end, about the page's centre. `1.0` for none.
    pub scale: f32,
    /// Whether the page is transparent at the far end.
    pub fade: bool,
    /// An edge that reveals an entering page from this side, or covers a leaving one from it,
    /// instead of the page moving.
    pub wipe: Option<Side>,
    /// How long, and along what curve. `None` takes the theme's: `crossfade` for an exit and
    /// `push` for an entry, so an exit is the shorter half.
    pub tween: Option<Tween>,
}

impl Motion {
    /// No motion at all: the page is cut.
    pub const CUT: Self = Self {
        offset: Vec2::ZERO,
        scale: 1.0,
        fade: false,
        wipe: None,
        tween: Some(Tween {
            duration: Duration::ZERO,
            easing: Easing::Linear,
        }),
    };

    /// A fade and nothing else. What a page that says nothing gets.
    pub const FADE: Self = Self {
        offset: Vec2::ZERO,
        scale: 1.0,
        fade: true,
        wipe: None,
        tween: None,
    };

    /// Set down smaller ([`CARD_SCALE`]) and faded, or picked up from there: a card.
    pub const CARD: Self = Self {
        offset: Vec2::ZERO,
        scale: CARD_SCALE,
        fade: true,
        wipe: None,
        tween: None,
    };

    /// From or towards `side`, a short way ([`SLIDE`]) with a fade — the way a page follows a
    /// mark that moved along a rail.
    #[must_use]
    pub fn slide(side: Side) -> Self {
        Self {
            offset: side.outward() * SLIDE,
            scale: 1.0,
            fade: true,
            wipe: None,
            tween: None,
        }
    }

    /// Revealed or covered by an edge sweeping in from `side`: an instrument's trace.
    #[must_use]
    pub const fn sweep(side: Side) -> Self {
        Self {
            offset: Vec2::ZERO,
            scale: 1.0,
            fade: false,
            wipe: Some(side),
            tween: None,
        }
    }

    /// From beyond the page's edge on `side` — its whole size away — and back out that way: a
    /// page that *arrives* rather than one browsed to. No fade: it is off the page to
    /// begin with. Give it [`Tween::back_out`] and it lands a little past its rest and settles.
    #[must_use]
    pub fn fly(side: Side) -> Self {
        Self {
            offset: side.outward(),
            scale: 1.0,
            fade: false,
            wipe: None,
            tween: None,
        }
    }

    /// With its far end at `offset`, in fractions of the page's size, faded there: `(1.0, -1.0)`
    /// is beyond the top-right corner, `(0.0, 0.5)` half a page below. The way to a direction
    /// the four sides do not name.
    #[must_use]
    pub const fn far(offset: Vec2) -> Self {
        Self {
            offset,
            scale: 1.0,
            fade: true,
            wipe: None,
            tween: None,
        }
    }

    /// Grown in from `scale` of its size about its centre, faded — or shrunk back to it on the
    /// way out. [`Self::CARD`] is this at [`CARD_SCALE`]; a smaller `scale` is a page that
    /// opens rather than one set down.
    #[must_use]
    pub const fn zoom(scale: f32) -> Self {
        Self {
            offset: Vec2::ZERO,
            scale,
            fade: true,
            wipe: None,
            tween: None,
        }
    }

    /// The same, over `tween` rather than the theme's.
    #[must_use]
    pub const fn over(mut self, tween: Tween) -> Self {
        self.tween = Some(tween);
        self
    }

    /// The same, faded at the far end or not.
    #[must_use]
    pub const fn faded(mut self, fade: bool) -> Self {
        self.fade = fade;
        self
    }

    /// The same, at `scale` of its size at the far end.
    #[must_use]
    pub const fn scaled(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// The same, cut by an edge from `side` as well.
    #[must_use]
    pub const fn wiped(mut self, side: Side) -> Self {
        self.wipe = Some(side);
        self
    }
}

/// **How a page comes and goes** — implemented by the page's identity: an index, an enum, an id.
///
/// Both methods have a default, a fade, so `impl Transit for Page {}` is already a transition; a
/// page that knows its neighbours says more. [`usize`] moves along its order (see below), and a
/// string key fades, so an index or a name needs no impl at all.
///
/// ```
/// use fairing::layout::{Motion, Side, Transit};
///
/// #[derive(Clone, PartialEq)]
/// enum Page { Overview, Run, Results }
///
/// impl Transit for Page {
///     // The overview is home: everything else comes up over it and goes back down.
///     fn enter(&self, from: &Self) -> Motion {
///         if *from == Page::Overview { Motion::slide(Side::Bottom) } else { Motion::FADE }
///     }
///     fn exit(&self, to: &Self) -> Motion {
///         if *to == Page::Overview { Motion::slide(Side::Bottom) } else { Motion::CARD }
///     }
/// }
/// ```
pub trait Transit: Clone + PartialEq + Send + Sync + 'static {
    /// How this page comes in, arriving from `from`.
    fn enter(&self, from: &Self) -> Motion {
        let _ = from;
        Motion::FADE
    }

    /// How this page goes out, on its way to `to`.
    fn exit(&self, to: &Self) -> Motion {
        let _ = to;
        Motion::FADE
    }
}

/// **An index moves along its order.** A later page comes up from below and an earlier one
/// down from above, each leaving the way the other arrives — the mark on the rail leads, and
/// the page follows it.
impl Transit for usize {
    fn enter(&self, from: &Self) -> Motion {
        Motion::slide(if self > from { Side::Bottom } else { Side::Top })
    }

    fn exit(&self, to: &Self) -> Motion {
        Motion::slide(if to > self { Side::Top } else { Side::Bottom })
    }
}

/// A name has no order: it fades.
impl Transit for &'static str {}

/// A name has no order: it fades.
impl Transit for String {}

/// Which half a page is in, and where it stands in it.
#[derive(Clone)]
enum Phase<K> {
    /// The page is up and still.
    Rest,
    /// The page shown is on its way out, towards `to`.
    Leaving { to: K, motion: Motion, since: f64 },
    /// The page shown is on its way in, from `from`.
    Arriving { from: K, motion: Motion, since: f64 },
}

/// What the container keeps between frames: the page that is up, and what it is doing.
#[derive(Clone)]
struct Stage<K> {
    shown: K,
    phase: Phase<K>,
}

/// What one frame draws: which page, and how far along which motion.
struct Cue<'k, K> {
    page: &'k K,
    /// `None` at rest. Otherwise the motion and where the page is on it: `0` at rest, `1` at
    /// the far end — and whether it is coming in (so a wipe reveals) or going out (it covers).
    moving: Option<(Motion, f32, bool)>,
}

/// **Draw the page `key` names, and the way there from the page before it**.
///
/// `body` draws a page for whichever key it is handed. While a page's exit plays, that is the
/// key before — so a body draws by the key it is given, never by the state it can see; the
/// console's `page(ui, cx, open, state)` already has that shape. Between transitions the page
/// is drawn straight into `ui`, so the container costs nothing at rest.
///
/// One container per `ui`: it keeps its stage under the `ui`'s id. A second one in the same
/// `ui` goes inside a `push_id`.
///
/// A key that changes again mid-way turns: a page still on its way out heads for the new key
/// from where it is, and a page on its way in goes back out — along the way it came, if that is
/// where it is sent. Under `[motion] reduce` every half is instant, and the swap is a cut.
#[expect(
    clippy::needless_pass_by_value,
    reason = "the key is the page's name, small and by value at every call site; the container keeps a copy"
)]
pub fn transit<K: Transit>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    key: K,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>, &K),
) {
    let ctx = ui.ctx().clone();
    let id = ui.id().with(TRANSIT_KEY);
    let clock = Clock {
        now: ctx.input(|i| i.time),
        exit_default: cx.theme.motion.crossfade,
        enter_default: cx.theme.motion.push,
        reduce: cx.theme.motion.reduce,
    };
    let mut stage: Stage<K> = ctx
        .data(|d| d.get_temp::<Stage<K>>(id))
        .unwrap_or_else(|| Stage {
            shown: key.clone(),
            phase: Phase::Rest,
        });
    steer(&mut stage, &key, clock);

    // Advance — at most twice, because an exit that has just finished starts the entry in the
    // same frame rather than leaving the page empty for one.
    let cue = loop {
        match stage.phase.clone() {
            Phase::Rest => {
                break Cue {
                    page: &stage.shown,
                    moving: None,
                }
            }
            Phase::Leaving { to, motion, since } => {
                let tween = motion.tween.unwrap_or(clock.exit_default);
                let gone = clock.progress(since, tween);
                if gone < 1.0 {
                    break Cue {
                        page: &stage.shown,
                        moving: Some((motion, tween.easing.apply(gone), false)),
                    };
                }
                let motion = to.enter(&stage.shown);
                let from = std::mem::replace(&mut stage.shown, to);
                stage.phase = Phase::Arriving {
                    from,
                    motion,
                    since: clock.now,
                };
            }
            Phase::Arriving { motion, since, .. } => {
                let tween = motion.tween.unwrap_or(clock.enter_default);
                let come = clock.progress(since, tween);
                if come < 1.0 {
                    break Cue {
                        page: &stage.shown,
                        moving: Some((motion, 1.0 - tween.easing.apply(come), true)),
                    };
                }
                stage.phase = Phase::Rest;
            }
        }
    };

    match cue.moving {
        None => {
            // At rest the page is the caller's own `ui`: full input, no layer of its own. The
            // moving page's area is kept up, empty and out of the way, for two reasons: a
            // transform left on it by the last transition is put back to nothing, and an area's
            // very first frame is a sizing pass that draws nothing — spent here, on nothing,
            // rather than as a blank frame at the start of the first exit.
            let rest = ui.available_rect_before_wrap();
            let layer = sublayer(ui, id);
            ctx.set_sublayer(ui.layer_id(), layer);
            ctx.set_transform_layer(layer, TSTransform::IDENTITY);
            moving_area(layer, rest).show(&ctx, |_| {});
            body(ui, cx, cue.page);
        }
        Some((motion, far, entering)) => {
            ctx.request_repaint();
            draw_moved(ui, cx, id, cue.page, motion, far, entering, body);
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, stage));
}

/// This frame's time, and the theme's two halves.
#[derive(Clone, Copy)]
struct Clock {
    now: f64,
    /// What an exit plays over when its motion names no tween.
    exit_default: Tween,
    /// What an entry plays over when its motion names no tween.
    enter_default: Tween,
    reduce: bool,
}

impl Clock {
    /// How far along a half that began at `since` is, `0..=1`. Instant under `reduce` or a
    /// zero tween.
    fn progress(self, since: f64, tween: Tween) -> f32 {
        let duration = tween.duration.as_secs_f64();
        if self.reduce || duration <= 0.0 {
            return 1.0;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a progress in 0..=1 loses nothing that matters in f32"
        )]
        let progress = ((self.now - since) / duration).clamp(0.0, 1.0) as f32;
        progress
    }
}

/// **The key moved, or moved back.** A page that is up starts leaving towards the new key; one
/// already leaving turns towards it, keeping its progress; one arriving is sent back out —
/// along the way it came, from where it is, if that is where it is sent. And a page called back
/// before it had left returns along the same path, from where it is.
fn steer<K: Transit>(stage: &mut Stage<K>, key: &K, clock: Clock) {
    if *key != stage.shown {
        let already = matches!(&stage.phase, Phase::Leaving { to, .. } if to == key);
        if already {
            return;
        }
        stage.phase = match stage.phase.clone() {
            Phase::Leaving { since, .. } => Phase::Leaving {
                to: key.clone(),
                motion: stage.shown.exit(key),
                since,
            },
            Phase::Arriving {
                from,
                motion,
                since,
            } if from == *key => {
                let tween = motion.tween.unwrap_or(clock.enter_default);
                let come = clock.progress(since, tween);
                Phase::Leaving {
                    to: from,
                    motion: motion.over(tween),
                    since: clock.now - f64::from(1.0 - come) * tween.duration.as_secs_f64(),
                }
            }
            Phase::Rest | Phase::Arriving { .. } => Phase::Leaving {
                to: key.clone(),
                motion: stage.shown.exit(key),
                since: clock.now,
            },
        };
    } else if let Phase::Leaving { to, motion, since } = stage.phase.clone() {
        let tween = motion.tween.unwrap_or(clock.exit_default);
        let gone = clock.progress(since, tween);
        stage.phase = Phase::Arriving {
            from: to,
            motion: motion.over(tween),
            since: clock.now - f64::from(1.0 - gone) * tween.duration.as_secs_f64(),
        };
    }
}

/// The sublayer the moving page is drawn on: the caller's order, an id of this container's own.
fn sublayer(ui: &Ui, id: egui::Id) -> LayerId {
    LayerId::new(ui.layer_id().order, id.with("layer"))
}

/// Draw `page` at `far` of the way along `motion` — `0` at rest, `1` at the far end — on the
/// container's sublayer: moved by drawing at an offset rect, scaled by a layer transform about
/// the page's centre, faded by the `Ui`'s opacity, and cut by a clip that stays where the page
/// rests.
#[expect(
    clippy::too_many_arguments,
    reason = "one frame of one page: the ui, the context, the id, the page, and the three values of its motion"
)]
fn draw_moved<K>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    id: egui::Id,
    page: &K,
    motion: Motion,
    far: f32,
    entering: bool,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>, &K),
) {
    let ctx = ui.ctx().clone();
    let rest = ui.available_rect_before_wrap();
    let offset = Vec2::new(
        motion.offset.x * rest.width(),
        motion.offset.y * rest.height(),
    ) * far;
    // `far` runs past `1` and below `0` under a back-out curve — the page lands beyond its
    // rest and comes back — so the move and the scale follow it out, and the opacity and the
    // clip, which have nowhere past their ends to go, are held at them.
    let scale = 1.0 + (motion.scale - 1.0) * far;
    let opacity = if motion.fade {
        (1.0 - far).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let clip = match motion.wipe {
        Some(side) => wiped(rest, side, far, entering).intersect(rest),
        None => rest,
    };

    // An area of its own, painted right after the caller's layer and **not interactable**: a
    // page on its way out or in takes no press. It is the same arrangement the shell uses for
    // the desktop under a zoom.
    let layer = sublayer(ui, id);
    ctx.set_sublayer(ui.layer_id(), layer);
    ctx.set_transform_layer(layer, scale_about(rest.center(), scale));
    moving_area(layer, rest).show(&ctx, |ui| {
        ui.set_clip_rect(clip);
        ui.set_min_size(rest.size());
        let mut moved = ui.new_child(UiBuilder::new().max_rect(rest.translate(offset)));
        moved.set_clip_rect(clip);
        moved.set_opacity(opacity);
        body(&mut moved, cx, page);
    });
}

/// The area a moving page is drawn in: on `layer`, over the page's resting rect, and taking no
/// input.
fn moving_area(layer: LayerId, rest: Rect) -> egui::Area {
    egui::Area::new(layer.id)
        .order(layer.order)
        .fixed_pos(rest.min)
        .default_size(rest.size())
        .constrain(false)
        .fade_in(false)
        .interactable(false)
}

/// A scale about `center`.
fn scale_about(center: egui::Pos2, scale: f32) -> TSTransform {
    TSTransform::from_translation(center.to_vec2())
        * TSTransform::from_scaling(scale)
        * TSTransform::from_translation(-center.to_vec2())
}

/// The part of `rest` a wipe leaves visible with the edge `far` of the way across from `side`:
/// an entering page is the part between `side` and the edge, growing as `far` falls; a leaving
/// page is the part beyond the edge, shrinking as `far` rises.
fn wiped(rest: Rect, side: Side, far: f32, entering: bool) -> Rect {
    // The edge, as a fraction of the way across from `side`.
    let edge = if entering { 1.0 - far } else { far };
    let across_x = rest.width() * edge;
    let across_y = rest.height() * edge;
    // An entering page keeps the part between `side` and the edge; a leaving one the part
    // beyond it.
    match (side, entering) {
        (Side::Left, true) => rest.with_max_x(rest.left() + across_x),
        (Side::Left, false) => rest.with_min_x(rest.left() + across_x),
        (Side::Right, true) => rest.with_min_x(rest.right() - across_x),
        (Side::Right, false) => rest.with_max_x(rest.right() - across_x),
        (Side::Top, true) => rest.with_max_y(rest.top() + across_y),
        (Side::Top, false) => rest.with_min_y(rest.top() + across_y),
        (Side::Bottom, true) => rest.with_min_y(rest.bottom() - across_y),
        (Side::Bottom, false) => rest.with_max_y(rest.bottom() - across_y),
    }
}

#[cfg(test)]
mod tests {
    use super::{wiped, Motion, Side, Transit};
    use egui::{pos2, Rect};

    /// An index leaves the way the next one arrives: down the rail the page goes up and the
    /// next comes up from below; back up the rail the other way round.
    #[test]
    fn an_index_moves_along_its_order() {
        assert_eq!(2_usize.enter(&0), Motion::slide(Side::Bottom));
        assert_eq!(0_usize.exit(&2), Motion::slide(Side::Top));
        assert_eq!(0_usize.enter(&2), Motion::slide(Side::Top));
        assert_eq!(2_usize.exit(&0), Motion::slide(Side::Bottom));
    }

    /// A wipe's edge sweeps from its side: nothing of an entering page is visible at the far end,
    /// all of it at rest, and the visible part grows from the side it comes in from.
    #[test]
    fn a_wipe_reveals_from_its_side_and_covers_from_it() {
        let page = Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 50.0));
        let entering = wiped(page, Side::Left, 0.75, true);
        assert!((entering.left() - 0.0).abs() < 1e-4 && (entering.right() - 25.0).abs() < 1e-4);
        let at_rest = wiped(page, Side::Left, 0.0, true);
        assert_eq!(at_rest, page);
        let leaving = wiped(page, Side::Left, 0.25, false);
        assert!((leaving.left() - 25.0).abs() < 1e-4 && (leaving.right() - 100.0).abs() < 1e-4);
        let gone = wiped(page, Side::Top, 1.0, false);
        assert!(gone.height() < 1e-4, "covered all the way: {gone:?}");
    }
}
