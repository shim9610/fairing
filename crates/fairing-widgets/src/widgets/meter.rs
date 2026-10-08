//! `Meter` — a measured value against its normal range: the range band of a high-performance HMI.
//!
//! # Why it is not a `ProgressBar`
//!
//! A bar answers "how much of the job is done": 0 to 1, monotone, trusted only while it behaves
//! like a clock. A meter answers "is this value normal": it has a unit, it goes up and
//! down all day, and the question is not where the pointer is but whether it is **inside the
//! band**. The bar's contract — ease towards the value, hold the high-water mark, never rewind —
//! is exactly wrong for it, and what it needs instead (a normal range, alarm limits, a setpoint,
//! a verdict) the bar has no place for. So it is its own element: a measurement,
//! not progress.
//!
//! # Why the base is grey and colour is kept for a verdict
//!
//! ISA-101 and the High Performance HMI practice behind it: the panel is grey, and colour is
//! reserved for what is *wrong*, so an abnormal reading is the only coloured thing on the screen
//! and the eye goes to it. In the band the pointer is `OnSurface`, the ink the labels are in.
//! Past a limit it takes the limit's own verdict — `Warning` or `Danger` — and so does the
//! read-out. A reading outside the band with no limit on that side is `Warn`: the band's edge is
//! then the only threshold the caller gave, so it is the warning. With a limit on that side the
//! band's edge is only where *normal* ends and the pointer's offset says so; the colour waits for
//! the limit, because that is where the caller said the alarm is.
//!
//! # Why the pointer is eased and the verdict is not
//!
//! The pointer glides over `motion.switch` so a noisy signal does not jitter across the band. The
//! verdict judges the **reported** value the moment it arrives: a reading past an alarm limit is
//! coloured on the frame it is reported, not when the pointer gets there.
//!
//! # Why the number is beside the track and not on it
//!
//! Also ISA-101: the analogue shape is what is read from across the room, and the digit confirms
//! it up close. The string is the caller's, like the bar's read-out, and it is coloured with
//! the verdict so the two cannot disagree.
//!
//! # Why stale means dim, and not a pulse
//!
//! A stalled *job* breathes, because a job is a thing that is working. A stale *measurement* is
//! a number that is old: it is greyed, the way a bad-quality value is on an HMI, and it does not
//! move, because movement on a meter reads as the value moving. [`Meter::stale_after`] and
//! [`Meter::heartbeat`] are the bar's, with that treatment.

use super::MeterLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::ColorRole;
use crate::unit::round_u8;
use crate::widgets::progress::{beat_of, stalled};
use crate::widgets::LampState;
use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use std::ops::RangeInclusive;
use std::time::Duration;

/// The normal band's ink, as the alpha of `OnSurface` laid over the track: a step the eye
/// finds and the pointer still stands out from.
const BAND_ALPHA: f32 = 0.14;
/// The deviation fill between the setpoint and the pointer, as the alpha of the verdict's ink.
const DEVIATION_ALPHA: f32 = 0.30;
/// The pointer's height above the track, as a fraction of the track's thickness.
const POINTER_RISE: f32 = 0.6;
/// The pointer's base width, as a fraction of the track's thickness.
const POINTER_BASE: f32 = 0.9;
/// How far a limit's tick reaches past the track's edges, in du.
const TICK_REACH: f32 = 2.0;

/// A threshold on the scale, and the verdict a reading past it gets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limit<'a> {
    at: f32,
    /// Crossed by readings at or above `at`; otherwise at or below.
    high: bool,
    state: LampState,
    label: Option<&'a str>,
}

impl<'a> Limit<'a> {
    /// A limit readings must stay **below**: crossed at or above `at`.
    ///
    /// `state` is the verdict past it — [`LampState::Warn`] or [`LampState::Fault`]; any other
    /// state reads as `Warn`, because a limit is a threshold and a threshold is at least that.
    #[must_use]
    pub fn high(at: f32, state: LampState) -> Self {
        Self {
            at,
            high: true,
            state,
            label: None,
        }
    }

    /// A limit readings must stay **above**: crossed at or below `at`.
    #[must_use]
    pub fn low(at: f32, state: LampState) -> Self {
        Self {
            high: false,
            ..Self::high(at, state)
        }
    }

    /// A caption under the tick — already formatted, in the caller's unit and locale.
    #[must_use]
    pub fn label(mut self, text: &'a str) -> Self {
        self.label = Some(text);
        self
    }

    /// Where it is on the scale.
    #[must_use]
    pub fn at(&self) -> f32 {
        self.at
    }

    /// Whether readings must stay below it — crossed at or above — rather than above it.
    #[must_use]
    pub fn is_high(&self) -> bool {
        self.high
    }

    /// Its caption under the tick, where it has one.
    #[must_use]
    pub fn caption(&self) -> Option<&'a str> {
        self.label
    }

    fn crossed(&self, value: f32) -> bool {
        if self.high {
            value >= self.at
        } else {
            value <= self.at
        }
    }

    /// The verdict a reading past it gets: `Fault` for a fault limit, `Warn` for any other.
    #[must_use]
    pub fn verdict(&self) -> LampState {
        match self.state {
            LampState::Fault => LampState::Fault,
            _ => LampState::Warn,
        }
    }
}

/// A meter: a value on a scale, its normal band, its limits, and a verdict.
#[derive(Debug)]
pub struct Meter<'a> {
    value: f32,
    scale: RangeInclusive<f32>,
    normal: Option<RangeInclusive<f32>>,
    setpoint: Option<f32>,
    limits: &'a [Limit<'a>],
    readout: Option<&'a str>,
    thickness: Option<f32>,
    enabled: bool,
    stale_after: Option<Duration>,
    heartbeat: Option<u64>,
    salt: &'a str,
}

/// What a meter said when it was shown.
#[derive(Debug)]
pub struct MeterReading {
    /// The row's response (it senses `hover()` only, like a bar).
    pub response: Response,
    /// The verdict on the **reported** value: `Ok` in the band, a limit's own state past it,
    /// `Unknown` for a value that is not a number. The same vocabulary a lamp speaks, so a
    /// lamp beside the meter can be lit from it.
    pub state: LampState,
    /// Whether the reading is older than [`Meter::stale_after`].
    pub stale: bool,
}

impl<'a> Meter<'a> {
    /// A meter showing `value` on `scale` (the whole span the track stands for).
    ///
    /// A value off the scale sits at the scale's end; a non-finite value draws no pointer and
    /// reads [`LampState::Unknown`].
    #[must_use]
    pub fn new(value: f32, scale: RangeInclusive<f32>) -> Self {
        Self {
            value,
            scale,
            normal: None,
            setpoint: None,
            limits: &[],
            readout: None,
            thickness: None,
            enabled: true,
            stale_after: None,
            heartbeat: None,
            salt: "fairing.meter",
        }
    }

    /// The normal operating range: the band on the track a reading is expected to sit in.
    #[must_use]
    pub fn normal(mut self, band: RangeInclusive<f32>) -> Self {
        self.normal = Some(band);
        self
    }

    /// The target the value is being held at. Marked under the track, with the deviation from
    /// it to the pointer filled in the verdict's ink, so drift is read as a length.
    #[must_use]
    pub fn setpoint(mut self, at: f32) -> Self {
        self.setpoint = Some(at);
        self
    }

    /// The limits, each a tick on the track and a verdict past it. See [`Limit`].
    #[must_use]
    pub fn limits(mut self, limits: &'a [Limit<'a>]) -> Self {
        self.limits = limits;
        self
    }

    /// The read-out beside the track — already formatted, unit and all.
    #[must_use]
    pub fn readout(mut self, text: &'a str) -> Self {
        self.readout = Some(text);
        self
    }

    /// Override the track's thickness (du). `control.slider_track` by default — a meter is
    /// read like a slider that cannot be grabbed, and its band needs the room a bar's track
    /// does not.
    #[must_use]
    pub fn thickness(mut self, du: f32) -> Self {
        self.thickness = Some(du.max(0.0));
        self
    }

    /// Enabled. Only the ink changes: a meter senses nothing but hover either way.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Grey the reading out when nothing has been heard for this long — the bar's rule
    /// ([`ProgressBar::stale_after`](crate::widgets::ProgressBar::stale_after)), with a
    /// measurement's treatment: dimmed, and still.
    #[must_use]
    pub fn stale_after(mut self, after: Duration) -> Self {
        self.stale_after = Some(after);
        self
    }

    /// The source's "still alive" counter, so a steady value is not a stale one
    /// ([`ProgressBar::heartbeat`](crate::widgets::ProgressBar::heartbeat)).
    #[must_use]
    pub fn heartbeat(mut self, beat: u64) -> Self {
        self.heartbeat = Some(beat);
        self
    }

    /// Salt the id the meter eases its pointer and remembers its last heartbeat under.
    #[must_use]
    pub fn id_salt(mut self, salt: &'a str) -> Self {
        self.salt = salt;
        self
    }

    /// The verdict on the reported value. See the module doc for the rule.
    #[must_use]
    pub fn verdict(&self) -> LampState {
        verdict(self.value, self.normal.as_ref(), self.limits)
    }

    /// Draw it.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> MeterReading {
        let c = &cx.theme.control;
        let thickness = self.thickness.unwrap_or(c.slider_track).max(1.0);
        let gap = c.gap;
        let rise = thickness * POINTER_RISE;
        let state = self.verdict();

        // The row: the pointer's rise, the track, and a line of captions if any limit has one.
        let font = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::FontId::proportional(cx.theme.metrics.type_scale.small);
        let caption_h = if self.limits.iter().any(|l| l.label.is_some()) {
            ui.fonts_mut(|f| f.row_height(&small))
        } else {
            0.0
        };
        let body_h = self
            .readout
            .map_or(0.0, |_| ui.text_style_height(&egui::TextStyle::Body));
        let height = (rise + thickness + TICK_REACH + caption_h).max(body_h);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
        let id = response.id.with(self.salt);

        let aged = self.stale_after.is_some_and(|after| {
            stalled(
                ui,
                id.with("beat"),
                beat_of(Some(self.value), self.heartbeat),
                after,
            )
            .is_some()
        });
        let ink = Ink::of(cx, state, self.enabled, aged);

        // The read-out first, so the track knows how much width is left to it.
        let readout = self.readout.map(|text| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font, ink.readout)
        });
        let cap = rect.width() * c.trailing_text_max;
        let readout_w = readout.as_ref().map_or(0.0, |g| g.rect.width().min(cap));
        let track_w = (rect.width() - readout_w - if readout_w > 0.0 { gap } else { 0.0 }).max(0.0);
        let track = Rect::from_min_size(
            Pos2::new(rect.min.x, rect.min.y + rise),
            Vec2::new(track_w, thickness),
        );
        // The pointer eases; everything else is where the numbers put it.
        let shown = if self.value.is_finite() {
            Some(cx.animate(id.with("v"), self.value, cx.theme.motion.switch))
        } else {
            None
        };
        let readout_at = readout.as_ref().map(|galley| {
            Rect::from_min_size(
                Pos2::new(
                    rect.max.x - galley.rect.width(),
                    track.center().y - galley.rect.height() * 0.5,
                ),
                galley.rect.size(),
            )
        });
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.meter.as_mut()) {
            custom(
                ui.painter(),
                &mut MeterLook {
                    rect,
                    track,
                    scale: self.scale.clone(),
                    value: self.value,
                    pointer: shown,
                    normal: self.normal.clone(),
                    setpoint: self.setpoint,
                    limits: self.limits,
                    readout: self.readout.zip(readout_at),
                    verdict: state,
                    stale: aged,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            let at = |v: f32| track.min.x + fraction(v, &self.scale) * track.width();
            let painter = ui.painter();
            self.paint_track(painter, cx, track, ink, &at);
            self.paint_marks(painter, cx, track, ink, &small, &at, shown);
            if let (Some(galley), Some(pos)) = (readout, readout_at) {
                painter.galley(pos.min, galley, ink.readout);
            }
        }
        MeterReading {
            response,
            state,
            stale: aged,
        }
    }

    /// The track, the band on it, and the edge that says how far the scale goes.
    fn paint_track(
        &self,
        painter: &egui::Painter,
        cx: &Cx<'_>,
        track: Rect,
        ink: Ink,
        at: &dyn Fn(f32) -> f32,
    ) {
        let radius = CornerRadius::same(round_u8(track.height() * 0.25));
        painter.rect_filled(track, radius, ink.track);
        if let Some(band) = &self.normal {
            let (a, b) = (at(*band.start()), at(*band.end()));
            if b > a {
                painter.rect_filled(
                    Rect::from_x_y_ranges(a..=b, track.y_range()),
                    CornerRadius::ZERO,
                    ink.band,
                );
            }
        }
        painter.rect_stroke(
            track,
            radius,
            Stroke::new(cx.theme.control.stroke_edge, ink.edge),
            StrokeKind::Inside,
        );
    }

    /// The setpoint and its deviation, the limits' ticks and captions, and the pointer.
    #[expect(
        clippy::too_many_arguments,
        reason = "one frame's geometry, handed over rather than recomputed"
    )]
    fn paint_marks(
        &self,
        painter: &egui::Painter,
        cx: &Cx<'_>,
        track: Rect,
        ink: Ink,
        small: &egui::FontId,
        at: &dyn Fn(f32) -> f32,
        shown: Option<f32>,
    ) {
        let thickness = track.height();
        let mark = Stroke::new(cx.theme.control.stroke_mark, ink.tick);
        if let Some(sp) = self.setpoint.filter(|s| s.is_finite()) {
            let x = at(sp);
            if let Some(v) = shown {
                let (a, b) = (x.min(at(v)), x.max(at(v)));
                if b > a {
                    painter.rect_filled(
                        Rect::from_x_y_ranges(a..=b, track.y_range()),
                        CornerRadius::ZERO,
                        ink.pointer.gamma_multiply(DEVIATION_ALPHA),
                    );
                }
            }
            // A small triangle under the track, pointing at the target.
            let base = track.max.y + TICK_REACH;
            let half = thickness * 0.3;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x, track.max.y),
                    Pos2::new(x - half, base + half),
                    Pos2::new(x + half, base + half),
                ],
                ink.setpoint,
                Stroke::NONE,
            ));
        }
        // Where each limit's caption would sit. Two that would overlap on a narrow track are
        // not both drawn: the one that yields is the less severe, then the one to the right.
        let caption = |index: usize, limit: &Limit<'_>| {
            limit.label.map(|text| {
                let galley = painter.layout_no_wrap(text.to_owned(), small.clone(), ink.tick);
                let w = galley.rect.width();
                let x = at(limit.at);
                let left = (x - w * 0.5).clamp(track.min.x, (track.max.x - w).max(track.min.x));
                (
                    Caption {
                        left,
                        right: left + w,
                        verdict: limit.verdict(),
                        index,
                    },
                    galley,
                )
            })
        };
        for (i, limit) in self.limits.iter().enumerate() {
            let x = at(limit.at);
            painter.line_segment(
                [
                    Pos2::new(x, track.min.y - TICK_REACH),
                    Pos2::new(x, track.max.y + TICK_REACH),
                ],
                mark,
            );
            let Some((this, galley)) = caption(i, limit) else {
                continue;
            };
            let yields = self
                .limits
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .filter_map(|(j, other)| caption(j, other).map(|(c, _)| c))
                .any(|other| this.yields_to(&other));
            if !yields {
                painter.galley(
                    Pos2::new(this.left, track.max.y + TICK_REACH),
                    galley,
                    ink.tick,
                );
            }
        }
        if let Some(v) = shown {
            let x = at(v);
            let half = thickness * POINTER_BASE * 0.5;
            let rise = thickness * POINTER_RISE;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(x, track.min.y),
                    Pos2::new(x - half, track.min.y - rise),
                    Pos2::new(x + half, track.min.y - rise),
                ],
                ink.pointer,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(x, track.min.y), Pos2::new(x, track.max.y)],
                Stroke::new(cx.theme.control.stroke_mark, ink.pointer),
            );
        }
    }
}

/// A limit's caption under the track: where it would sit, and what it says.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Caption {
    left: f32,
    right: f32,
    verdict: LampState,
    index: usize,
}

impl Caption {
    /// Whether this caption gives way to `other` when the two would overlap: to a more severe
    /// verdict first, then to the one further left, then to the one listed first.
    fn yields_to(&self, other: &Self) -> bool {
        let overlap = self.left < other.right && other.left < self.right;
        overlap
            && match (self.verdict, other.verdict) {
                (LampState::Fault, LampState::Fault) | (LampState::Warn, LampState::Warn) => {
                    (other.left, other.index) < (self.left, self.index)
                }
                (_, LampState::Fault) => true,
                _ => false,
            }
    }
}

/// Where `value` sits on `scale`, 0 to 1, clamped — and at the start of a scale with no width.
pub(crate) fn fraction(value: f32, scale: &RangeInclusive<f32>) -> f32 {
    let (lo, hi) = (*scale.start(), *scale.end());
    let span = hi - lo;
    if !value.is_finite() || !span.is_finite() || span <= 0.0 {
        return 0.0;
    }
    ((value - lo) / span).clamp(0.0, 1.0)
}

/// The verdict: the most severe limit crossed; else `Warn` outside the band on a side with no
/// limit of its own; else `Ok`. Not a number is `Unknown`.
fn verdict(value: f32, normal: Option<&RangeInclusive<f32>>, limits: &[Limit<'_>]) -> LampState {
    if !value.is_finite() {
        return LampState::Unknown;
    }
    let crossed = limits
        .iter()
        .filter(|l| l.crossed(value))
        .map(Limit::verdict);
    if crossed.clone().any(|s| s == LampState::Fault) {
        return LampState::Fault;
    }
    if crossed.count() > 0 {
        return LampState::Warn;
    }
    let Some(band) = normal else {
        return LampState::Ok;
    };
    let above = value > *band.end();
    let below = value < *band.start();
    let guarded = |high: bool| limits.iter().any(|l| l.high == high);
    if (above && !guarded(true)) || (below && !guarded(false)) {
        LampState::Warn
    } else {
        LampState::Ok
    }
}

/// One frame's colours.
#[derive(Debug, Clone, Copy)]
struct Ink {
    track: Color32,
    edge: Color32,
    band: Color32,
    tick: Color32,
    setpoint: Color32,
    pointer: Color32,
    readout: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, state: LampState, enabled: bool, aged: bool) -> Self {
        let on_surface = cx.theme.color(ColorRole::OnSurface);
        let muted = cx.theme.color(ColorRole::Muted);
        let (pointer, readout) = match state {
            LampState::Warn => {
                let c = cx.theme.color(ColorRole::Warning);
                (c, c)
            }
            LampState::Fault => {
                let c = cx.theme.color(ColorRole::Danger);
                (c, c)
            }
            LampState::Unknown => (muted, muted),
            _ => (on_surface, muted),
        };
        let mut ink = Self {
            track: cx.theme.color(ColorRole::SurfaceVariant),
            edge: cx.theme.color(ColorRole::ControlEdge),
            band: on_surface.gamma_multiply(BAND_ALPHA),
            tick: muted,
            setpoint: cx.theme.color(ColorRole::Primary),
            pointer,
            readout,
        };
        let a = cx.theme.control.disabled_alpha;
        if !enabled {
            ink = Self {
                track: ink.track.gamma_multiply(a),
                edge: ink.edge.gamma_multiply(a),
                band: ink.band.gamma_multiply(a),
                tick: ink.tick.gamma_multiply(a),
                setpoint: ink.setpoint.gamma_multiply(a),
                pointer: ink.pointer.gamma_multiply(a),
                readout: ink.readout.gamma_multiply(a),
            };
        } else if aged {
            // Old data: the reading greys, the scale it sits on does not.
            ink.pointer = ink.pointer.gamma_multiply(a);
            ink.readout = ink.readout.gamma_multiply(a);
        }
        ink
    }
}

#[cfg(test)]
mod tests {
    use super::{fraction, verdict, Limit, BAND_ALPHA};
    use crate::theme::{contrast, ColorRole, Palette, Preset};
    use crate::widgets::LampState;

    /// **The verdict is the most severe limit crossed, else the band's edge where no limit
    /// guards it, else `Ok`.**
    #[test]
    fn the_verdict_follows_the_limits_then_the_band() {
        let band = 55.0..=65.0;
        let limits = [
            Limit::high(67.0, LampState::Warn),
            Limit::high(70.0, LampState::Fault),
        ];
        let v = |value: f32| verdict(value, Some(&band), &limits);
        assert_eq!(v(60.0), LampState::Ok, "in the band");
        assert_eq!(
            v(66.0),
            LampState::Ok,
            "above the band but under a limit that guards that side: a deviation, not a verdict"
        );
        assert_eq!(v(67.0), LampState::Warn, "at the warning limit");
        assert_eq!(
            v(72.0),
            LampState::Fault,
            "past the alarm: the most severe wins"
        );
        assert_eq!(
            v(50.0),
            LampState::Warn,
            "below the band with no low limit: the band's edge is the warning"
        );
        assert_eq!(
            verdict(50.0, None, &limits),
            LampState::Ok,
            "no band, no low limit"
        );
        assert_eq!(
            verdict(40.0, Some(&band), &[Limit::low(45.0, LampState::Fault)]),
            LampState::Fault
        );
        assert_eq!(v(f32::NAN), LampState::Unknown);
        assert_eq!(
            verdict(99.0, None, &[Limit::high(90.0, LampState::Active)]),
            LampState::Warn,
            "a limit with a state that is not a verdict reads as a warning"
        );
    }

    /// **The pointer never leaves the track**, and a scale with no width cannot divide by zero.
    #[test]
    fn the_pointer_is_clamped_to_the_scale() {
        let scale = 40.0..=80.0;
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(near(fraction(60.0, &scale), 0.5));
        assert!(near(fraction(20.0, &scale), 0.0), "off the low end");
        assert!(near(fraction(120.0, &scale), 1.0), "off the high end");
        assert!(near(fraction(60.0, &(60.0..=60.0)), 0.0), "no width");
        assert!(
            near(fraction(60.0, &(80.0..=40.0)), 0.0),
            "a reversed scale is empty"
        );
        assert!(near(fraction(f32::NAN, &scale), 0.0));
    }

    /// **The pointer's inks clear the non-text floor on the track in every palette, and the
    /// band is a visible step on it** — the two things the meter is read by.
    #[test]
    fn the_pointer_and_the_band_are_seen_in_every_palette() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let name = format!(
                    "{} {}",
                    preset.as_str(),
                    if dark { "dark" } else { "light" }
                );
                for role in [ColorRole::OnSurface, ColorRole::Warning, ColorRole::Danger] {
                    let got = contrast(p.get(role), p.surface_variant);
                    assert!(
                        got >= 3.0,
                        "{name}: a {role:?} pointer measures {got:.2} on the track"
                    );
                }
                let band = blend(p.on_surface.gamma_multiply(BAND_ALPHA), p.surface_variant);
                let step = contrast(band, p.surface_variant);
                assert!(
                    step >= 1.15,
                    "{name}: the band measures {step:.2} on the track — invisible"
                );
            }
        }
    }

    /// **Two captions that would overlap are not both drawn**: the less severe yields, then the
    /// one to the right — so a narrow meter keeps "70 !" and drops "67", never the reverse.
    #[test]
    fn overlapping_captions_yield_to_severity_then_to_the_left() {
        use super::Caption;
        let warn = Caption {
            left: 100.0,
            right: 120.0,
            verdict: LampState::Warn,
            index: 0,
        };
        let fault = Caption {
            left: 112.0,
            right: 140.0,
            verdict: LampState::Fault,
            index: 1,
        };
        assert!(warn.yields_to(&fault), "the warning gives way to the alarm");
        assert!(!fault.yields_to(&warn));
        let fault_right = Caption {
            left: 130.0,
            index: 2,
            ..fault
        };
        assert!(
            fault_right.yields_to(&fault),
            "of two alarms, the one to the right yields"
        );
        assert!(!fault.yields_to(&fault_right));
        let apart = Caption {
            left: 200.0,
            right: 220.0,
            ..warn
        };
        assert!(
            !apart.yields_to(&fault) && !fault.yields_to(&apart),
            "no overlap, no yielding"
        );
    }

    /// `over` (premultiplied) composited on an opaque `under`.
    fn blend(over: egui::Color32, under: egui::Color32) -> egui::Color32 {
        let a = f32::from(over.a()) / 255.0;
        let ch = |o: u8, u: u8| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0..=255"
            )]
            let v = (f32::from(o) + f32::from(u) * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8;
            v
        };
        egui::Color32::from_rgb(
            ch(over.r(), under.r()),
            ch(over.g(), under.g()),
            ch(over.b(), under.b()),
        )
    }
}
