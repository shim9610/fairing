//! `ProgressBar` displays determinate or indeterminate progress without editing it.
//!
//! Hiding a `TouchSlider` handle does not make a slider read-only. Use this widget
//! when pointer interaction must not change the reported value.
//!
//! With rewind disabled, a lower reported target is raised to the stored target
//! before easing over `motion.switch`. The configured easing determines the
//! intermediate displayed values. `paint_bar`, which has no retained `Ui` state,
//! paints the supplied value directly. The caller supplies the display string and
//! tone; the widget does not infer process completion or fault severity.
//!
//! Indeterminate progress translates normally and uses an opacity pulse under
//! reduced motion. `stale_after` and `heartbeat` control stale indication. A stale
//! fill pulses and the read-out dims. Stop indicators and segmented steps are
//! per-widget options. These presentation choices do not establish accessibility
//! conformance or confirm that a backend operation is still running.

use super::{ProgressBarLook, ProgressFill as Fill};
use crate::cx::WidgetCx as Cx;
use crate::theme::{ColorRole, Theme};
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use std::time::Duration;

/// A progress bar.
#[derive(Debug)]
pub struct ProgressBar<'a> {
    /// `None` is indeterminate. Set only by the two constructors.
    value: Option<f32>,
    /// The caller's already-formatted read-out.
    trailing: Option<&'a str>,
    /// A du override of `control.progress_track()`.
    thickness: Option<f32>,
    /// The filled side.
    tone: ColorRole,
    enabled: bool,
    /// The Material 2024 look: a gap at the fill's head and a dot at the track's end.
    stop_indicator: bool,
    /// Cells, for a quantity that is counted. 0 or 1 is one continuous track.
    steps: u8,
    allow_rewind: bool,
    /// How long without word before the bar reports a stall. `None` never does.
    stale_after: Option<Duration>,
    /// The caller's "still alive" counter; the reported value stands in when there is none.
    heartbeat: Option<u64>,
    salt: &'a str,
}

impl<'a> ProgressBar<'a> {
    /// A bar at `value`, 0 to 1.
    ///
    /// Out of range is clamped and **non-finite reads as empty** — `NaN` handed to epaint is a rect
    /// it silently drops, which would draw a bar with no track at all.
    #[must_use]
    pub fn determinate(value: f32) -> Self {
        Self {
            value: Some(if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            }),
            trailing: None,
            thickness: None,
            tone: ColorRole::Primary,
            enabled: true,
            stop_indicator: false,
            steps: 0,
            allow_rewind: false,
            stale_after: None,
            heartbeat: None,
            salt: "fairing.progress_bar",
        }
    }

    /// A bar for work whose extent is not known.
    #[must_use]
    pub fn indeterminate() -> Self {
        Self {
            value: None,
            ..Self::determinate(0.0)
        }
    }

    /// A read-out beside the track — already formatted, and already in the caller's locale.
    #[must_use]
    pub fn trailing(mut self, text: &'a str) -> Self {
        self.trailing = Some(text);
        self
    }

    /// Override the track's thickness (du). `control.progress_track()` by default.
    #[must_use]
    pub fn thickness(mut self, du: f32) -> Self {
        self.thickness = Some(du.max(0.0));
        self
    }

    /// The filled side's colour.
    ///
    /// [`Primary`](ColorRole::Primary) by default. [`Success`](ColorRole::Success),
    /// [`Warning`](ColorRole::Warning) and [`Danger`](ColorRole::Danger) are the other three this
    /// is measured for — all four clear the 3.0 a filled shape needs against the track in every
    /// shipped palette, worst case 3.50. Anything else is undocumented territory.
    #[must_use]
    pub fn tone(mut self, role: ColorRole) -> Self {
        self.tone = role;
        self
    }

    /// Enabled.
    ///
    /// A bar senses `hover()` either way — it reports and is not pressed — so this changes only the
    /// ink. The tap falls through to the card underneath, which is the common case: a bar usually
    /// sits inside a notification whose own tap opens it.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Add a gap at the fill head and a dot at the track end. Off by default.
    /// Both dimensions use one track thickness; the dot hides when the head reaches it.
    #[must_use]
    pub fn stop_indicator(mut self, on: bool) -> Self {
        self.stop_indicator = on;
        self
    }

    /// Cut the track into `n` cells with a gap of one thickness between them, for a quantity that
    /// is counted rather than measured: 3 of 7 steps, 2 of 4 stages.
    ///
    /// The value is still 0 to 1 — `3.0 / 7.0` fills three cells exactly — and it still eases, so
    /// the fourth cell fills over `motion.switch` rather than appearing. `0` or `1` is one
    /// continuous track. A track too narrow to give every cell at least its own thickness in width
    /// is drawn continuous instead: cells that are dots are not countable.
    #[must_use]
    pub fn steps(mut self, n: u8) -> Self {
        self.steps = n;
        self
    }

    /// Allow a lower reported target. Off by default: lower targets are raised to
    /// the stored target before easing. Displayed values follow the configured
    /// easing, including any overshoot and return.
    #[must_use]
    pub fn allow_rewind(mut self, allow: bool) -> Self {
        self.allow_rewind = allow;
        self
    }

    /// Report a stall when nothing has been heard for this long.
    ///
    /// "Heard" is the reported value changing, or [`heartbeat`](Self::heartbeat) changing where the
    /// caller gives one. Past the limit the fill breathes and the read-out dims until the next
    /// word; the bar does not change colour, because whether a stall is a *fault* is the caller's
    /// call ([`tone`](Self::tone)). Off by default: a bar has no way to know what silence means.
    #[must_use]
    pub fn stale_after(mut self, after: Duration) -> Self {
        self.stale_after = Some(after);
        self
    }

    /// The worker's "still alive" counter, for a stall limit tighter than the progress is slow.
    ///
    /// Bump it on every message the worker sends — progress or keep-alive — and hand it here;
    /// then a step that takes a minute is not a stall, and a worker that has stopped sending is.
    /// Without one, the reported value itself is the heartbeat. Only meaningful with
    /// [`stale_after`](Self::stale_after).
    #[must_use]
    pub fn heartbeat(mut self, beat: u64) -> Self {
        self.heartbeat = Some(beat);
        self
    }

    /// Salt the id the bar remembers its eased value, high-water mark and last heartbeat under.
    ///
    /// The id is the bar's own auto id plus this, so two bars in one `Ui` are already told apart;
    /// the salt is for a bar whose place in the `Ui` moves between frames.
    #[must_use]
    pub fn id_salt(mut self, salt: &'a str) -> Self {
        self.salt = salt;
        self
    }

    /// Draw it.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let thickness = self
            .thickness
            .unwrap_or_else(|| cx.theme.control.progress_track());
        let gap = cx.theme.control.gap;
        let mut ink = Ink::of(cx, self.tone, self.enabled);

        // The row is as tall as the read-out's line where there is one. The read-out itself is
        // laid out after the stall is known, because its colour depends on it.
        let font = egui::TextStyle::Body.resolve(ui.style());
        let line = self
            .trailing
            .map_or(0.0, |_| ui.text_style_height(&egui::TextStyle::Body));
        let height = thickness.max(line);
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());

        let id = response.id.with(self.salt);
        let fill = if let Some(t) = self.value {
            Fill::To(eased_value(ui, cx, id, t, self.allow_rewind))
        } else {
            sweep(ui, cx)
        };
        let stale = self.stale_after.and_then(|after| {
            let beat = beat_of(self.value, self.heartbeat);
            stalled(ui, id.with("beat"), beat, after).map(|since| stall_alpha(ui, cx, since))
        });
        if let Some(alpha) = stale {
            ink.fill = ink.fill.gamma_multiply(alpha);
            ink.label = ink.label.gamma_multiply(cx.theme.control.disabled_alpha);
        }

        let label = self.trailing.map(|text| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font, ink.label)
        });
        // The read-out takes what it needs and no more than its share, so a long string cannot
        // squeeze the track it belongs to out of existence.
        let cap = rect.width() * cx.theme.control.trailing_text_max;
        let label_w = label.as_ref().map_or(0.0, |g| g.rect.width().min(cap));
        let track = Rect::from_min_size(
            egui::pos2(rect.min.x, rect.center().y - thickness * 0.5),
            Vec2::new(
                (rect.width() - label_w - if label_w > 0.0 { gap } else { 0.0 }).max(0.0),
                thickness,
            ),
        );
        let readout_at = label.as_ref().map(|galley| {
            Rect::from_min_size(
                egui::pos2(
                    rect.max.x - galley.rect.width(),
                    rect.center().y - galley.rect.height() * 0.5,
                ),
                galley.rect.size(),
            )
        });
        if let Some(custom) = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.progress_bar.as_mut())
        {
            custom(
                ui.painter(),
                &mut ProgressBarLook {
                    rect,
                    track,
                    fill,
                    readout: self.trailing.zip(readout_at),
                    tone: self.tone,
                    stop_indicator: self.stop_indicator,
                    steps: self.steps,
                    stale,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }
        let look = Look {
            stop: self.stop_indicator,
            steps: self.steps,
        };
        paint_track(ui.painter(), cx.theme, track, ink, fill, look);
        if let (Some(galley), Some(at)) = (label, readout_at) {
            ui.painter().galley(at.min, galley, ink.label);
        }
        response
    }
}

/// The indeterminate fill for this frame, and the repaint that keeps it running.
///
/// The clock is `ui.input(|i| i.time)` and not `cx.animate`: `AnimationStore` eases *towards a
/// target* and this is free-running. The crate already reads the same clock for the long press.
// The clock is `f64` seconds since start and only its fraction crosses to `f32`. That fraction is
// in `[0, 1)`, so the narrowing loses nothing a pixel could show — but egui's clock itself is past
// an `f32`'s integer precision after a few hours, which is exactly the uptime this runs at.
#[allow(clippy::cast_possible_truncation)]
fn sweep(ui: &egui::Ui, cx: &Cx<'_>) -> Fill {
    let cycle = cx.theme.motion.progress_cycle.as_secs_f64().max(0.001);
    let phase = f64::rem_euclid(ui.input(|i| i.time) / cycle, 1.0);
    let phase = phase.clamp(0.0, 1.0) as f32;
    ui.ctx().request_repaint();
    if cx.theme.motion.reduce {
        // Half the rate, because a pulse reads as one event per *period* and a sweep as one per
        // cycle. Nothing translates, which is what the guidance actually targets.
        Fill::Pulse(pulse_alpha(phase * 0.5, cx.theme.control.disabled_alpha))
    } else {
        let span = cx.theme.control.indeterminate_span;
        let (a, b) = segment_span(phase, span);
        Fill::Segment(a, b)
    }
}

/// The segment's two normalised ends at `phase`.
///
/// It grows out of the leading cap, crosses at full length and shrinks into the trailing one, so it
/// is always inside the track and never needs a clip.
pub(crate) fn segment_span(phase: f32, span: f32) -> (f32, f32) {
    let head = phase * (1.0 + span);
    ((head - span).clamp(0.0, 1.0), head.clamp(0.0, 1.0))
}

/// The pulse's alpha at `phase`, breathing between `floor` and 1.
pub(crate) fn pulse_alpha(phase: f32, floor: f32) -> f32 {
    let wave = (phase * std::f32::consts::TAU).cos().mul_add(-0.5, 0.5);
    floor + (1.0 - floor) * wave
}

/// Clamp lower targets to the stored target when rewind is disabled, then ease
/// using `motion.switch`. Shared by the bar and ring; the stored target is not
/// the maximum of the intermediate displayed values.
pub(crate) fn eased_value(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    id: egui::Id,
    target: f32,
    allow_rewind: bool,
) -> f32 {
    let high_id = id.with("high");
    let high = ui.data(|d| d.get_temp::<f32>(high_id)).unwrap_or(0.0);
    let target = if allow_rewind {
        target
    } else {
        target.max(high)
    };
    ui.data_mut(|d| d.insert_temp(high_id, target));
    cx.animate(id.with("t"), target, cx.theme.motion.switch)
}

/// Use the caller's heartbeat when supplied, otherwise the reported value bits.
/// Repeated updates with an unchanged value need a changing heartbeat to be
/// distinguished from no updates.
pub(crate) fn beat_of(value: Option<f32>, heartbeat: Option<u64>) -> u64 {
    heartbeat.unwrap_or_else(|| u64::from(value.unwrap_or(0.0).to_bits()))
}

/// The heartbeat as last remembered: the beat and the clock when it changed.
type LastBeat = (u64, f64);

/// Remember a heartbeat under `id` and report whether `after` has elapsed.
/// A repaint is requested for that deadline; the host determines when another
/// update is supplied.
pub(crate) fn stalled(ui: &egui::Ui, id: egui::Id, beat: u64, after: Duration) -> Option<f64> {
    let now = ui.input(|i| i.time);
    let prev = ui.data(|d| d.get_temp::<LastBeat>(id));
    let (last, stale) = stall_state(prev, beat, now, after.as_secs_f64());
    ui.data_mut(|d| d.insert_temp(id, last));
    if stale {
        Some(last.1 + after.as_secs_f64())
    } else {
        let left = after.as_secs_f64() - (now - last.1);
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(left.max(0.001)));
        None
    }
}

/// The rule behind [`stalled`], with the clock passed in: the beat as it is now remembered, and
/// whether the limit has passed. A beat first seen is heard *now*.
pub(crate) fn stall_state(
    prev: Option<LastBeat>,
    beat: u64,
    now: f64,
    after: f64,
) -> (LastBeat, bool) {
    let last = match prev {
        Some((seen, at)) if seen == beat => (seen, at),
        _ => (beat, now),
    };
    (last, now - last.1 >= after)
}

/// The stalled fill's alpha this frame — the reduced-motion pulse, on the fill alone — and the
/// repaint that keeps it breathing.
///
/// The phase runs from `since`, the moment the stall began, so the fill **drops to the floor as
/// it goes stale** and breathes back up from there: the transition is an event the eye catches,
/// not a phase of a free-running clock that may happen to be at its peak.
#[allow(clippy::cast_possible_truncation)]
pub(crate) fn stall_alpha(ui: &egui::Ui, cx: &Cx<'_>, since: f64) -> f32 {
    let cycle = cx.theme.motion.progress_cycle.as_secs_f64().max(0.001);
    let phase = f64::rem_euclid((ui.input(|i| i.time) - since) / (cycle * 2.0), 1.0);
    ui.ctx().request_repaint();
    pulse_alpha(
        phase.clamp(0.0, 1.0) as f32,
        cx.theme.control.disabled_alpha,
    )
}

/// The four colours one frame paints with, dimmed once at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    track: Color32,
    edge: Color32,
    fill: Color32,
    label: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, tone: ColorRole, enabled: bool) -> Self {
        let ink = Self {
            track: cx.theme.color(ColorRole::SurfaceVariant),
            edge: cx.theme.color(ColorRole::ControlEdge),
            fill: cx.theme.color(tone),
            label: cx.theme.color(ColorRole::Muted),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                track: ink.track.gamma_multiply(a),
                edge: ink.edge.gamma_multiply(a),
                fill: ink.fill.gamma_multiply(a),
                label: ink.label.gamma_multiply(a),
            }
        }
    }
}

/// The geometry options of one bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Look {
    /// The head gap and the end dot.
    stop: bool,
    /// Cells. 0 or 1 is one track.
    steps: u8,
}

/// How many cells a track is cut into: `steps`, or one when the track cannot give each cell at
/// least its own thickness in width.
fn cell_count(track: Rect, steps: u8) -> u8 {
    let n = steps.max(1);
    if n > 1 && cell(track, n, 0).width() < track.height() {
        1
    } else {
        n
    }
}

/// Cell `i` of `n`: `n` equal cells with a gap of one thickness between them.
fn cell(track: Rect, n: u8, i: u8) -> Rect {
    let n = n.max(1);
    let gap = track.height();
    let width = ((track.width() - gap * f32::from(n - 1)) / f32::from(n)).max(0.0);
    let x = track.min.x + f32::from(i) * (width + gap);
    Rect::from_min_size(egui::pos2(x, track.min.y), Vec2::new(width, track.height()))
}

/// The track and its fill.
///
/// **The one place a bar is drawn.** [`ProgressBar::show`] and [`paint_bar`] both come here, so the
/// widget and the shell's own notification banner cannot drift the way the two hand-rolled
/// drawings did.
fn paint_track(
    painter: &egui::Painter,
    theme: &Theme,
    track: Rect,
    ink: Ink,
    fill: Fill,
    look: Look,
) {
    if track.width() <= 0.0 || track.height() <= 0.0 {
        return;
    }
    let radius = CornerRadius::same(round_u8(track.height() * 0.5));
    let edge = Stroke::new(theme.control.stroke_edge, ink.edge);
    let (from, to, color) = match fill {
        // **No minimum.** The slider keeps an accent cap at zero so its mover stays visible under
        // its own stroke; a bar that shows a sliver at 0.0 reports that a job has started when it
        // has not.
        Fill::To(t) => (0.0, t.clamp(0.0, 1.0), ink.fill),
        Fill::Segment(a, b) => (a, b, ink.fill),
        Fill::Pulse(alpha) => (0.0, 1.0, ink.fill.gamma_multiply(alpha)),
    };
    let head_x = |t: f32| track.min.x + t * track.width();
    let (x0, x1) = (head_x(from), head_x(to));
    // The pulse covers the whole track: there is no head to gap and no end to mark.
    let cut = look.stop && !matches!(fill, Fill::Pulse(_));
    let gap = track.height();

    let n = cell_count(track, look.steps);
    for i in 0..n {
        let seg = cell(track, n, i);
        let (f0, f1) = (x0.max(seg.min.x), x1.min(seg.max.x));
        let filled = x1 > x0 && f1 > f0;
        if cut && filled {
            // Around the fill, a gap away from each end of it. A remainder narrower than it is
            // thick is a sliver, and the dot does that job.
            for (a, b) in [(seg.min.x, f0 - gap), (f1 + gap, seg.max.x)] {
                if b - a >= track.height() {
                    let piece = Rect::from_x_y_ranges(a..=b, seg.y_range());
                    painter.rect_filled(piece, radius, ink.track);
                    painter.rect_stroke(piece, radius, edge, StrokeKind::Inside);
                }
            }
        } else {
            painter.rect_filled(seg, radius, ink.track);
            // Inside, so the drawn bar never measures thicker than `progress_track` and a column
            // of bars stays on one rhythm.
            painter.rect_stroke(seg, radius, edge, StrokeKind::Inside);
        }
        if filled {
            painter.rect_filled(Rect::from_x_y_ranges(f0..=f1, seg.y_range()), radius, color);
        }
    }

    // The stop dot: one thickness at the end of the last cell, gone once the head has come within
    // a gap of it. Determinate only — a sweep has no end to reach.
    if cut && matches!(fill, Fill::To(_)) {
        let last = cell(track, n, n - 1);
        if let Some(centre) = stop_dot(last, x1) {
            painter.circle_filled(centre, track.height() * 0.5, color);
        }
    }
}

/// Where the stop dot sits for a head at `head_x`, or `None` once the head has reached it.
fn stop_dot(last: Rect, head_x: f32) -> Option<egui::Pos2> {
    let d = last.height();
    (head_x + d <= last.max.x - d).then(|| egui::pos2(last.max.x - d * 0.5, last.center().y))
}

/// **A determinate bar where there is no `Ui`** — the shell's notification banner has a `Painter`
/// and a `&Theme` and nothing else.
///
/// Determinate only: an indeterminate bar needs a clock and a repaint request, and both come from
/// a `Ui`.
pub fn paint_bar(painter: &egui::Painter, theme: &Theme, rect: Rect, value: f32, tone: ColorRole) {
    let ink = Ink {
        track: theme.color(ColorRole::SurfaceVariant),
        edge: theme.color(ColorRole::ControlEdge),
        fill: theme.color(tone),
        label: theme.color(ColorRole::Muted),
    };
    let t = if value.is_finite() { value } else { 0.0 };
    paint_track(painter, theme, rect, ink, Fill::To(t), Look::default());
}

#[cfg(test)]
mod tests {
    use super::{cell, cell_count, pulse_alpha, segment_span, stall_state, stop_dot, ProgressBar};
    use crate::theme::{contrast, ColorRole, ControlSpec, Palette, Preset};
    use crate::unit::{Scale, ScaleConfidence, ScalePolicy, ScaleSource};
    use egui::Rect;

    /// **A non-finite value reads as empty rather than reaching epaint.** `NaN` in a rect is a
    /// shape the tessellator silently drops, so a bar handed one would lose its track as well as
    /// its fill — the reading would be "no bar", not "no progress".
    #[test]
    fn the_value_is_clamped_and_a_non_finite_value_reads_as_empty() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            let bar = ProgressBar::determinate(bad);
            assert_eq!(bar.value, Some(0.0), "{bad} did not read as empty");
        }
        assert_eq!(ProgressBar::determinate(2.0).value, Some(1.0));
        assert_eq!(ProgressBar::determinate(0.5).value, Some(0.5));
    }

    /// Check the default progress/slider thickness ratio for the sampled 6, 9, 13 and
    /// 16 mm finger policies.
    #[test]
    fn a_progress_track_stays_under_half_the_thing_you_grab() {
        for finger in [6.0_f32, 9.0, 13.0, 16.0] {
            let scale = Scale::resolve(
                8.0,
                &ScalePolicy::default().with_finger_mm(finger),
                ScaleSource::Backend,
                ScaleConfidence::Measured,
                egui::vec2(800.0, 480.0),
            );
            let c = ControlSpec::default().resolve(&scale);
            let ratio = c.progress_track() / c.slider_track;
            assert!(
                c.progress_track() > 0.0,
                "{finger} mm: a bar with no thickness"
            );
            assert!(
                (0.40..=0.55).contains(&ratio),
                "{finger} mm: the bar is {ratio:.3} of the slider"
            );
        }
    }

    /// **The segment never leaves the track**, at any phase — which is what lets it be drawn with
    /// no clip rect of its own.
    #[test]
    fn an_indeterminate_segment_never_leaves_the_track() {
        let span = 0.30;
        for step in 0_u8..=64 {
            let phase = f32::from(step) / 64.0;
            let (a, b) = segment_span(phase, span);
            assert!(
                (0.0..=1.0).contains(&a) && (0.0..=1.0).contains(&b),
                "{phase}"
            );
            assert!(a <= b, "{phase}: the segment is inside out");
            assert!(b - a <= span + 1e-6, "{phase}: it grew past its span");
        }
    }

    /// It grows out of one end and shrinks into the other, rather than appearing whole.
    #[test]
    fn the_segment_grows_out_of_one_end_and_shrinks_into_the_other() {
        let span = 0.30;
        let (a0, b0) = segment_span(0.0, span);
        assert!(
            (b0 - a0).abs() < 1e-6 && a0 < 1e-6,
            "it did not start at the left edge"
        );
        let (a1, b1) = segment_span(1.0, span);
        assert!(
            (b1 - a1).abs() < 1e-6 && b1 > 1.0 - 1e-6,
            "it did not end at the right edge"
        );
        let (a, b) = segment_span(0.5, span);
        assert!(b - a > span * 0.99, "the midpoint is not the full span");
        assert!(a > 0.0 && b < 1.0, "the midpoint touches an end");
    }

    /// Reduced motion replaces indeterminate translation with an opacity pulse.
    ///
    /// This is a visual status treatment; it does not confirm backend liveness or
    /// establish accessibility conformance.
    #[test]
    fn reduced_motion_keeps_the_indeterminate_bar_alive() {
        let floor = 0.55;
        let a = pulse_alpha(0.0, floor);
        let b = pulse_alpha(0.5, floor);
        assert!(
            (a - b).abs() >= 0.3,
            "the pulse is imperceptible: {a:.2} vs {b:.2}"
        );
        for step in 0_u8..=32 {
            let v = pulse_alpha(f32::from(step) / 32.0, floor);
            assert!(
                (floor - 1e-6..=1.0 + 1e-6).contains(&v),
                "{v} left its range"
            );
        }
    }

    /// Check track-boundary contrast for the built-in palette/background pairs,
    /// and keep the lower-contrast decorative Outline role out of that boundary.
    #[test]
    fn the_track_is_identified_in_every_palette_and_outline_would_not_be() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                for (label, ground) in [("page", p.surface), ("card", p.surface_variant)] {
                    let edge = contrast(p.control_edge, ground);
                    assert!(
                        edge >= 3.0,
                        "{name} {mode}: the track's edge measures {edge:.2} on the {label}"
                    );
                    let ghost = contrast(p.outline, ground);
                    assert!(
                        ghost < 3.0,
                        "{name} {mode}: `Outline` now measures {ghost:.2} on the {label} — if it \
                         has become a boundary, this element's reason for not using it is gone"
                    );
                }
                let fill = contrast(p.primary, p.surface_variant);
                assert!(
                    fill >= 3.0,
                    "{name} {mode}: the fill measures {fill:.2} on its track"
                );
            }
        }
    }

    /// Every tone the docs allow clears the floor a filled shape needs against the track.
    #[test]
    fn every_documented_tone_clears_the_non_text_floor_on_the_track() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                for role in [
                    ColorRole::Primary,
                    ColorRole::Success,
                    ColorRole::Warning,
                    ColorRole::Danger,
                ] {
                    let got = contrast(p.get(role), p.surface_variant);
                    assert!(
                        got >= 3.0,
                        "{} {}: {role:?} measures {got:.2} on the track",
                        preset.as_str(),
                        if dark { "dark" } else { "light" }
                    );
                }
            }
        }
    }

    /// **Cells are equal, a thickness apart, and add up to the track** — so `3.0 / 7.0` fills
    /// three of seven exactly and the fourth starts where the gap ends.
    #[test]
    fn cells_are_equal_a_thickness_apart_and_add_up_to_the_track() {
        let track = Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(300.0, 6.0));
        let n = cell_count(track, 7);
        assert_eq!(n, 7);
        let first = cell(track, n, 0);
        let last = cell(track, n, 6);
        assert!(
            (first.min.x - track.min.x).abs() < 1e-3,
            "the first cell starts late"
        );
        assert!(
            (last.max.x - track.max.x).abs() < 1e-3,
            "the last cell ends early"
        );
        for i in 1..n {
            let (a, b) = (cell(track, n, i - 1), cell(track, n, i));
            assert!(
                (b.width() - a.width()).abs() < 1e-3,
                "cell {i} is a different width"
            );
            assert!(
                (b.min.x - a.max.x - track.height()).abs() < 1e-3,
                "the gap before cell {i} is not one thickness"
            );
        }
        // Where 3/7 lands is the end of the third cell, not somewhere in the fourth's gap.
        let head = track.min.x + track.width() * 3.0 / 7.0;
        let third = cell(track, n, 2);
        assert!(
            head >= third.max.x - 1e-3 && head < cell(track, n, 3).min.x + 1e-3,
            "3/7 lands at {head}, the third cell ends at {}",
            third.max.x
        );
    }

    /// Check continuous-track fallback when the sampled width cannot fit the requested
    /// cell spacing; zero or one step also uses a continuous track.
    #[test]
    fn a_track_too_narrow_to_count_falls_back_to_one_cell() {
        let narrow = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(40.0, 6.0));
        assert_eq!(cell_count(narrow, 12), 1, "twelve cells in 40 px are dots");
        assert_eq!(
            cell_count(narrow, 3),
            3,
            "three cells in 40 px are countable"
        );
        for steps in [0, 1] {
            assert_eq!(cell_count(narrow, steps), 1);
            assert_eq!(
                cell(narrow, steps, 0),
                narrow,
                "{steps} steps is the whole track"
            );
        }
    }

    /// **The stop dot marks the end until the head reaches it**, one gap away — never drawn on
    /// top of the fill, never missing while there is track left to show.
    #[test]
    fn the_stop_dot_marks_the_end_until_the_head_reaches_it() {
        let track = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 8.0));
        let far = stop_dot(track, 80.0);
        let Some(at) = far else {
            return assert!(far.is_some(), "a dot is missing with the head at 80 of 200");
        };
        assert!(
            (at.x - 196.0).abs() < 1e-3 && (at.y - 4.0).abs() < 1e-3,
            "{at:?}"
        );
        assert!(
            stop_dot(track, 184.0).is_some(),
            "a gap and a dot still fit"
        );
        assert!(
            stop_dot(track, 184.1).is_none(),
            "the head has reached the dot"
        );
        assert!(stop_dot(track, 200.0).is_none(), "a full bar has no dot");
    }

    /// **A beat is heard when it changes and a stall is measured from then**, whatever the value
    /// does — a slow step with a live heartbeat is not a stall and a dead worker at 63 % is.
    #[test]
    fn a_stall_is_measured_from_the_last_beat() {
        let (last, stale) = stall_state(None, 7, 10.0, 2.0);
        assert_eq!(last, (7, 10.0), "a beat first seen is heard now");
        assert!(!stale);
        let (last, stale) = stall_state(Some(last), 7, 11.9, 2.0);
        assert_eq!(last, (7, 10.0));
        assert!(!stale, "1.9 s of silence is under a 2 s limit");
        let (last, stale) = stall_state(Some(last), 7, 12.0, 2.0);
        assert!(stale, "2 s of silence is the limit");
        let (last, stale) = stall_state(Some(last), 8, 12.0, 2.0);
        assert_eq!(last, (8, 12.0), "a new beat is heard now");
        assert!(!stale, "and the silence starts again");
    }
}
