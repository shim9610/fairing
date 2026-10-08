//! The split — two panes sharing the content, and the divider between them (A8).
//!
//! # One tree, one level
//!
//! The workspace is designed as a tree (`Leaf | Split { axis, ratio, a, b }`) able to hold
//! four panes. v1 holds two, so the tree is this one struct: the split is there or it is not, and
//! while it is there are two panes, the first at the start of the axis (left, or top).
//!
//! # Geometry is a function, not state
//!
//! Every rect is worked out each frame from the content, the axis, the ratio and how far in or out
//! the split is ([`Split::geometry`]). Screens are immediate mode and lay themselves out again in
//! whatever rect they get (A8: "no separate handling"), so what is stored is only what moves: the
//! ratio (a finger, a spring, a tween) and the presence (the enter and leave tweens).
//!
//! # Never smaller than a screen allows
//!
//! A pane is never *laid out* below its minimum along the axis (A8: "never drawn below
//! `MinSize`"). Pushed past it by the divider, its screen keeps the minimum and slides under the
//! divider, dimmed more the further it goes — what letting go there does (closing that pane) is on
//! the glass before the finger lifts.

use crate::motion::{Animated, Mode, Spring};
use crate::screen::SplitSupport;
use crate::theme::MotionTokens;
use egui::{Pos2, Rect, Vec2};
/// Released within this share of either end, the divider closes the pane it was pushed into.
pub(crate) const DISMISS_SHARE: f32 = 0.1;
/// The least share of the content a pane keeps when its screen names no minimum.
const DEFAULT_MIN_SHARE: f32 = 0.25;
/// ...and never fewer touch targets than this along the axis.
const DEFAULT_MIN_TARGETS: f32 = 3.0;
/// The dim over a pane squeezed past its minimum, at the end of the squeeze.
pub(crate) const SQUEEZE_DIM: f32 = 0.45;

/// How two panes share the content (the axis follows the content's shape, or the
/// config fixes it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitAxis {
    /// Side by side — the width is divided.
    SideBySide,
    /// One above the other — the height is divided.
    Stacked,
}

impl SplitAxis {
    /// The axis a content rect of this shape splits along: side by side when it is at least as
    /// wide as it is tall.
    #[must_use]
    pub fn for_content(content: Rect) -> Self {
        if content.width() >= content.height() {
            Self::SideBySide
        } else {
            Self::Stacked
        }
    }

    /// The length along the axis.
    pub(crate) fn along(self, v: Vec2) -> f32 {
        match self {
            Self::SideBySide => v.x,
            Self::Stacked => v.y,
        }
    }

    /// The length across the axis.
    fn across(self, v: Vec2) -> f32 {
        match self {
            Self::SideBySide => v.y,
            Self::Stacked => v.x,
        }
    }

    /// Where a point is along the axis.
    pub(crate) fn coord(self, p: Pos2) -> f32 {
        match self {
            Self::SideBySide => p.x,
            Self::Stacked => p.y,
        }
    }

    /// One unit along the axis.
    fn unit(self) -> Vec2 {
        match self {
            Self::SideBySide => Vec2::X,
            Self::Stacked => Vec2::Y,
        }
    }
}

/// Where the split stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    /// This pane is coming in; the other shrinks from the whole content to its share.
    Entering(usize),
    /// Both panes at the ratio (the ratio itself may be moving).
    Split,
    /// This pane is going out; the other grows to the whole content.
    Leaving(usize),
}

/// What letting go of the divider decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Released {
    /// The panes stay, the divider springs to where both keep their minimums.
    Rest,
    /// The divider was pushed to an end: this pane closes.
    Close(usize),
}

/// The split's moving parts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Split {
    phase: Phase,
    /// 0 → 1 entering, 1 → 0 leaving, 1 at rest.
    presence: Animated<f32>,
    /// The divider's place along the axis, as a share of the content.
    ratio: Animated<f32>,
    /// The settled geometry changed — `Resized` is owed once everything rests (once at
    /// the end of a tween, once on the divider's release).
    resize_owed: bool,
}

/// One frame of the split's rects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Geometry {
    /// What each pane shows: its clip, and the rect its backing fills.
    pub(crate) visible: [Rect; 2],
    /// What each pane's screen is laid out in — the visible rect, grown from the pane's outer
    /// edge to its minimum where the divider has pushed it smaller.
    pub(crate) laid: [Rect; 2],
    /// How far past its minimum each pane is squeezed, `0..=1` — the dim over it.
    pub(crate) squeeze: [f32; 2],
    /// The divider's band.
    pub(crate) divider: Rect,
    /// The divider's alpha: it fades in late on the way in and out early on the way out.
    pub(crate) divider_alpha: f32,
}

impl Split {
    /// A split coming in: pane `entering` slides in over `[motion.panes] enter_ms`, the divider at
    /// `ratio`. Under `reduce` it is in at once.
    pub(crate) fn enter(entering: usize, ratio: f32, tokens: &MotionTokens) -> Self {
        let reduce = tokens.reduce;
        let mut split = Self {
            phase: Phase::Entering(entering),
            presence: Animated::new(0.0),
            ratio: Animated::new(ratio),
            resize_owed: false,
        };
        if reduce {
            split.presence.snap(1.0);
            split.phase = Phase::Split;
            split.resize_owed = true;
        } else {
            split.presence.to(1.0, tokens.panes_enter);
        }
        split
    }

    /// Start `pane` leaving, over `[motion.panes] leave_ms`. `true` where it is gone at once
    /// (`reduce`) — the caller finishes it on the spot instead of waiting for [`Split::tick`].
    pub(crate) fn leave(&mut self, pane: usize, tokens: &MotionTokens) -> bool {
        let reduce = tokens.reduce;
        self.phase = Phase::Leaving(pane);
        // A divider still springing stops where it is; the pane slides out from there.
        self.ratio.snap(self.ratio.value());
        if reduce {
            self.presence.snap(0.0);
            return true;
        }
        self.presence.to(0.0, tokens.panes_leave);
        false
    }

    pub(crate) fn phase(&self) -> Phase {
        self.phase
    }

    /// The divider's place, as a share of the content along the axis.
    pub(crate) fn ratio(&self) -> f32 {
        self.ratio.value()
    }

    /// Whether a tween or a spring is running — the enter or leave, or the divider settling.
    pub(crate) fn is_moving(&self) -> bool {
        self.presence.is_animating() || self.ratio.is_animating()
    }

    /// Whether the divider is under a finger.
    pub(crate) fn is_dragging(&self) -> bool {
        matches!(self.ratio.mode(), Mode::Dragging)
    }

    /// Whether the enter or leave tween is running — input waits for it (A8, as A2 and A3).
    pub(crate) fn is_tweening(&self) -> bool {
        self.presence.is_animating()
    }

    /// The divider follows the finger 1:1: `ratio` and its velocity, both in shares of the content.
    pub(crate) fn drag(&mut self, ratio: f32, velocity: f32) {
        self.ratio.drag(ratio.clamp(0.0, 1.0), velocity);
    }

    /// The finger lifted. Pushed to within [`DISMISS_SHARE`] of an end, the pane it was pushed
    /// into closes — where `dismiss`: a drag cut short (the handle went before the lift was
    /// heard) only springs back, since nobody chose to close anything. Otherwise the divider
    /// springs to the nearest ratio in `bounds` (where both panes keep their minimums). `len` is
    /// the content's length along the axis, so the spring's settle test works in pixels rather
    /// than in shares.
    pub(crate) fn release(
        &mut self,
        bounds: Option<(f32, f32)>,
        spring: Spring,
        reduce: bool,
        len: f32,
        dismiss: bool,
    ) -> Released {
        let ratio = self.ratio.value();
        if dismiss && ratio < DISMISS_SHARE {
            return Released::Close(0);
        }
        if dismiss && ratio > 1.0 - DISMISS_SHARE {
            return Released::Close(1);
        }
        let (lo, hi) = bounds.unwrap_or((0.5, 0.5));
        let target = ratio.clamp(lo, hi);
        if reduce {
            self.ratio.snap(target);
        } else {
            self.ratio
                .release_scaled(target, spring, 1.0 / len.max(1.0));
        }
        self.resize_owed = true;
        Released::Rest
    }

    /// A double tap: even the panes out — or as near as `bounds` allows — over
    /// `[motion.panes] even_ms`.
    pub(crate) fn even(&mut self, bounds: Option<(f32, f32)>, tokens: &MotionTokens) {
        let reduce = tokens.reduce;
        let (lo, hi) = bounds.unwrap_or((0.5, 0.5));
        let target = 0.5_f32.clamp(lo, hi);
        if reduce {
            self.ratio.snap(target);
        } else {
            self.ratio.to(target, tokens.panes_even);
        }
        self.resize_owed = true;
    }

    /// Hold the divider inside `bounds` when they move under it (the content resized, a screen
    /// with a larger minimum came to the top). Nothing while a finger has it.
    pub(crate) fn keep_within(&mut self, bounds: Option<(f32, f32)>) {
        if self.is_dragging() || self.ratio.is_animating() {
            return;
        }
        let Some((lo, hi)) = bounds else {
            return;
        };
        let ratio = self.ratio.value();
        let held = ratio.clamp(lo, hi);
        if (held - ratio).abs() > f32::EPSILON {
            self.ratio.snap(held);
            self.resize_owed = true;
        }
    }

    /// Advance. Returns the pane that finished leaving, if one did — the caller takes it out.
    pub(crate) fn tick(&mut self, dt: f32) -> Option<usize> {
        let ratio_was = self.ratio.is_animating();
        if ratio_was && !self.ratio.tick(dt) {
            self.resize_owed = true;
        }
        let presence_was = self.presence.is_animating();
        if presence_was && !self.presence.tick(dt) {
            match self.phase {
                Phase::Entering(_) => {
                    self.phase = Phase::Split;
                    self.resize_owed = true;
                }
                Phase::Leaving(pane) => return Some(pane),
                Phase::Split => {}
            }
        }
        None
    }

    /// `true` once, when the geometry has come to rest after a change that owes the panes a
    /// `Resized`.
    pub(crate) fn take_resize(&mut self) -> bool {
        if self.resize_owed && !self.is_moving() && !self.is_dragging() {
            self.resize_owed = false;
            return true;
        }
        false
    }

    /// This frame's rects. `divider` is the band's thickness, `mins` each pane's least length
    /// along the axis.
    pub(crate) fn geometry(
        &self,
        content: Rect,
        axis: SplitAxis,
        divider: f32,
        mins: [f32; 2],
    ) -> Geometry {
        let [rest0, rest1, band] = rest(content, axis, self.ratio.value(), divider);
        let rest = [rest0, rest1];
        let laid_at_rest = [
            grow_to(rest0, mins[0], axis, false),
            grow_to(rest1, mins[1], axis, true),
        ];
        let squeeze = [squeeze(rest0, mins[0], axis), squeeze(rest1, mins[1], axis)];
        let (moving, p) = match self.phase {
            Phase::Split => {
                return Geometry {
                    visible: rest,
                    laid: laid_at_rest,
                    squeeze,
                    divider: band,
                    divider_alpha: 1.0,
                }
            }
            Phase::Entering(pane) | Phase::Leaving(pane) => (pane.min(1), self.presence.value()),
        };
        let stay = 1 - moving;
        // The pane coming or going is drawn at its share and slid out past the content's edge.
        let outward = if moving == 0 {
            -axis.unit()
        } else {
            axis.unit()
        };
        let away = outward * ((1.0 - p) * (axis.along(pick(rest, moving).size()) + divider));
        let mut visible = rest;
        let mut laid = laid_at_rest;
        set(&mut visible, moving, pick(rest, moving).translate(away));
        set(
            &mut laid,
            moving,
            pick(laid_at_rest, moving).translate(away),
        );
        // The one staying grows between its share and the whole content.
        let staying = lerp_rect(content, pick(rest, stay), p);
        set(&mut visible, stay, staying);
        set(&mut laid, stay, staying);
        // The band rides the staying pane's inner edge — left at its resting place it would cross
        // the pane still shrinking towards it.
        let band = band_beside(staying, axis, axis.along(band.size()), stay == 0);
        Geometry {
            visible,
            laid,
            squeeze: [0.0; 2],
            divider: band,
            divider_alpha: ((p - 0.5) * 2.0).clamp(0.0, 1.0),
        }
    }
}

/// The two panes and the divider's band at rest at `ratio`.
pub(crate) fn rest(content: Rect, axis: SplitAxis, ratio: f32, divider: f32) -> [Rect; 3] {
    let len = axis.along(content.size());
    let half = (divider / 2.0).min(len / 2.0).max(0.0);
    let start = axis.coord(content.min);
    let mid = (start + len * ratio).clamp(start + half, start + len - half);
    match axis {
        SplitAxis::SideBySide => [
            Rect::from_min_max(content.min, egui::pos2(mid - half, content.max.y)),
            Rect::from_min_max(egui::pos2(mid + half, content.min.y), content.max),
            Rect::from_min_max(
                egui::pos2(mid - half, content.min.y),
                egui::pos2(mid + half, content.max.y),
            ),
        ],
        SplitAxis::Stacked => [
            Rect::from_min_max(content.min, egui::pos2(content.max.x, mid - half)),
            Rect::from_min_max(egui::pos2(content.min.x, mid + half), content.max),
            Rect::from_min_max(
                egui::pos2(content.min.x, mid - half),
                egui::pos2(content.max.x, mid + half),
            ),
        ],
    }
}

/// The ratios at which both panes keep their minimums, or `None` where none does — the split
/// cannot hold in this content.
pub(crate) fn bounds(
    content: Rect,
    axis: SplitAxis,
    divider: f32,
    mins: [f32; 2],
) -> Option<(f32, f32)> {
    let len = axis.along(content.size());
    if len <= divider {
        return None;
    }
    let lo = (mins[0] + divider / 2.0) / len;
    let hi = 1.0 - (mins[1] + divider / 2.0) / len;
    (lo <= hi).then_some((lo, hi))
}

/// The least a pane showing a screen of this support keeps along `axis` in `content`. `None` for
/// a screen that is never split — `SplitSupport::No`, or a minimum across the axis the content
/// cannot give. `target` is the touch target: a screen that names no minimum keeps a quarter of
/// the content, and never fewer than three targets.
pub(crate) fn min_len(
    support: SplitSupport,
    content: Rect,
    axis: SplitAxis,
    target: f32,
) -> Option<f32> {
    match support {
        SplitSupport::Yes => {
            Some((axis.along(content.size()) * DEFAULT_MIN_SHARE).max(target * DEFAULT_MIN_TARGETS))
        }
        SplitSupport::MinSize(size) => {
            (axis.across(size) <= axis.across(content.size())).then(|| axis.along(size).max(target))
        }
        SplitSupport::No => None,
    }
}

/// The band `thick` across just past `rect`'s edge along the axis: after its end (`after`), or
/// before its start.
fn band_beside(rect: Rect, axis: SplitAxis, thick: f32, after: bool) -> Rect {
    let (min, max) = (rect.min, rect.max);
    match (axis, after) {
        (SplitAxis::SideBySide, true) => {
            Rect::from_min_max(egui::pos2(max.x, min.y), egui::pos2(max.x + thick, max.y))
        }
        (SplitAxis::SideBySide, false) => {
            Rect::from_min_max(egui::pos2(min.x - thick, min.y), egui::pos2(min.x, max.y))
        }
        (SplitAxis::Stacked, true) => {
            Rect::from_min_max(egui::pos2(min.x, max.y), egui::pos2(max.x, max.y + thick))
        }
        (SplitAxis::Stacked, false) => {
            Rect::from_min_max(egui::pos2(min.x, min.y - thick), egui::pos2(max.x, min.y))
        }
    }
}

/// `rect` grown to `min` along the axis, from its outer edge: the start for the first pane, the
/// end for the second (`from_end`).
fn grow_to(rect: Rect, min: f32, axis: SplitAxis, from_end: bool) -> Rect {
    let short = min - axis.along(rect.size());
    if short <= 0.0 {
        return rect;
    }
    let grow = axis.unit() * short;
    if from_end {
        Rect::from_min_max(rect.min - grow, rect.max)
    } else {
        Rect::from_min_max(rect.min, rect.max + grow)
    }
}

/// How far past `min` a pane of this rect is squeezed, `0..=1`.
fn squeeze(rect: Rect, min: f32, axis: SplitAxis) -> f32 {
    if min <= 0.0 {
        return 0.0;
    }
    ((min - axis.along(rect.size())) / min).clamp(0.0, 1.0)
}

/// `a` towards `b` by `t`.
fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(a.min.lerp(b.min, t), a.max.lerp(b.max, t))
}

/// The rect at `index` of a pair.
fn pick(pair: [Rect; 2], index: usize) -> Rect {
    if index == 0 {
        pair[0]
    } else {
        pair[1]
    }
}

/// Set the rect at `index` of a pair.
fn set(pair: &mut [Rect; 2], index: usize, rect: Rect) {
    if index == 0 {
        pair[0] = rect;
    } else {
        pair[1] = rect;
    }
}

#[cfg(test)]
mod tests {
    use super::{bounds, min_len, rest, Phase, Released, Split, SplitAxis};
    use crate::config::MotionConfig;
    use crate::motion::Spring;
    use crate::screen::SplitSupport;
    use crate::theme::MotionTokens;
    use egui::{pos2, vec2, Rect};

    fn animated() -> MotionTokens {
        MotionTokens::default()
    }

    fn reduced() -> MotionTokens {
        MotionTokens::from_config(&MotionConfig {
            reduce: true,
            ..MotionConfig::default()
        })
    }

    fn content() -> Rect {
        Rect::from_min_size(pos2(0.0, 40.0), vec2(1000.0, 600.0))
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn the_axis_follows_the_content_shape() {
        assert_eq!(SplitAxis::for_content(content()), SplitAxis::SideBySide);
        let tall = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 1000.0));
        assert_eq!(SplitAxis::for_content(tall), SplitAxis::Stacked);
    }

    #[test]
    fn at_rest_the_panes_meet_the_divider_band() {
        let [a, b, band] = rest(content(), SplitAxis::SideBySide, 0.5, 8.0);
        assert!(close(a.max.x, 496.0) && close(b.min.x, 504.0));
        assert!(close(band.width(), 8.0) && close(band.height(), 600.0));
        let [a, b, _] = rest(content(), SplitAxis::Stacked, 0.25, 10.0);
        assert!(close(a.max.y, 40.0 + 150.0 - 5.0) && close(b.min.y, 40.0 + 150.0 + 5.0));
    }

    #[test]
    fn a_minimum_bounds_the_ratio_and_one_too_big_rules_the_split_out() {
        let axis = SplitAxis::SideBySide;
        let yes = min_len(SplitSupport::Yes, content(), axis, 48.0);
        assert_eq!(yes, Some(250.0), "a quarter of the content");
        let wide = min_len(
            SplitSupport::MinSize(vec2(600.0, 100.0)),
            content(),
            axis,
            48.0,
        );
        assert_eq!(wide, Some(600.0));
        let tall = min_len(
            SplitSupport::MinSize(vec2(100.0, 700.0)),
            content(),
            axis,
            48.0,
        );
        assert_eq!(tall, None, "taller than the content");
        assert_eq!(min_len(SplitSupport::No, content(), axis, 48.0), None);
        let both = bounds(content(), axis, 8.0, [250.0, 250.0]);
        assert!(both.is_some_and(|(lo, hi)| close(lo, 0.254) && close(hi, 0.746)));
        assert!(
            bounds(content(), axis, 8.0, [600.0, 250.0]).is_some(),
            "600, 250 and the band fit 1000"
        );
        assert!(
            bounds(content(), axis, 8.0, [600.0, 450.0]).is_none(),
            "600 and 450 do not"
        );
    }

    #[test]
    fn a_release_springs_into_bounds_or_closes_the_pane_pushed_to_an_end() {
        let mut split = Split::enter(1, 0.5, &reduced());
        assert_eq!(split.phase(), Phase::Split);
        assert!(split.take_resize(), "in at once under reduce, sized once");
        let spring = Spring::new(400.0, 40.0);
        split.drag(0.2, 0.0);
        assert!(split.is_dragging());
        let bounds = Some((0.3, 0.7));
        assert_eq!(
            split.release(bounds, spring, true, 1000.0, true),
            Released::Rest
        );
        assert!(close(split.ratio(), 0.3), "clamped to the bound");
        assert!(split.take_resize() && !split.take_resize(), "once");
        split.drag(0.05, 0.0);
        assert_eq!(
            split.release(bounds, spring, true, 1000.0, true),
            Released::Close(0)
        );
        split.drag(0.97, 0.0);
        assert_eq!(
            split.release(bounds, spring, true, 1000.0, true),
            Released::Close(1)
        );
        // Cut short at an end, nothing closes: back inside the bounds.
        split.drag(0.03, 0.0);
        assert_eq!(
            split.release(bounds, spring, true, 1000.0, false),
            Released::Rest
        );
        assert!(close(split.ratio(), 0.3));
    }

    #[test]
    fn entering_starts_full_and_outside_and_ends_at_rest() {
        let axis = SplitAxis::SideBySide;
        let mut split = Split::enter(1, 0.5, &animated());
        let start = split.geometry(content(), axis, 8.0, [250.0, 250.0]);
        assert_eq!(
            start.visible[0],
            content(),
            "the pane already there starts whole"
        );
        assert!(
            start.visible[1].min.x >= content().max.x,
            "the new one starts outside"
        );
        assert!(start.divider_alpha <= 0.0);
        for _ in 0..30 {
            if split.tick(0.016).is_some() {
                break;
            }
        }
        assert_eq!(split.phase(), Phase::Split);
        let end = split.geometry(content(), axis, 8.0, [250.0, 250.0]);
        let [a, b, _] = rest(content(), axis, 0.5, 8.0);
        assert_eq!(end.visible, [a, b]);
        assert!(close(end.divider_alpha, 1.0));
        assert!(split.take_resize());
    }

    #[test]
    fn a_pane_pushed_past_its_minimum_keeps_it_and_dims() {
        let axis = SplitAxis::SideBySide;
        let mut split = Split::enter(1, 0.5, &reduced());
        split.drag(0.15, 0.0);
        let g = split.geometry(content(), axis, 8.0, [250.0, 250.0]);
        assert!(close(g.visible[0].width(), 146.0));
        assert!(close(g.laid[0].width(), 250.0), "laid out at its minimum");
        assert!(close(g.laid[0].min.x, 0.0), "from its outer edge");
        assert!(g.squeeze[0] > 0.4 && g.squeeze[1] <= 0.0);
    }

    #[test]
    fn leaving_slides_the_pane_out_and_reports_it_gone() {
        let axis = SplitAxis::SideBySide;
        let mut split = Split::enter(1, 0.5, &reduced());
        assert!(!split.leave(0, &animated()));
        let mut gone = None;
        for _ in 0..30 {
            gone = gone.or(split.tick(0.016));
        }
        assert_eq!(gone, Some(0));
        let g = split.geometry(content(), axis, 8.0, [250.0, 250.0]);
        assert_eq!(g.visible[1], content(), "the one staying fills the content");
        assert!(
            g.visible[0].max.x <= content().min.x,
            "the one leaving is out"
        );
        let at_once = reduced();
        let mut quick = Split::enter(1, 0.5, &at_once);
        assert!(quick.leave(1, &at_once), "gone at once under reduce");
    }
}
