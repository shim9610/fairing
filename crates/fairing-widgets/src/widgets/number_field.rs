//! `NumberField` — a figure you can both step and type.
//!
//! # The gap this closes
//!
//! The instrument-panel reference this was measured from carries **six** on one screen — Current
//! 120.0 mA, Temperature 25.0 °C, Piezo 45.0 V, Modulation Freq. 25.0 MHz, Lock Gain 3.0,
//! Integrator 100 ms — and the crate had nothing that could draw one. A machine panel is the thing
//! this crate says it is for, and a machine panel is mostly numbers with units on them.
//!
//! # Why the arrows are beside the figure and not stacked above one another
//!
//! The reference stacks a ▲ over a ▼ to the right of each field, each about 0.62 of the field's own
//! height. That is a **desk instrument**: a mouse at 400 mm, where a 4 mm target is comfortable.
//! This crate's default finger is a gloved 13 mm, so two stacked arrows would each need a full
//! `touch_target` and the row would come out **two targets tall** — a six-field screen would be
//! twelve targets deep and would not fit a seven-inch panel.
//!
//! Laid side by side they cost one target of height and some width, which a panel has. So a number
//! field is [`Stepper`](super::Stepper)'s track with a wider middle: the same pill, the same end
//! discs, the same concentric corner. They are one family and they are documented as one.
//!
//! # Why the unit sits outside the track
//!
//! The reference puts it there too, and there is a second reason: what is inside the track is
//! **editable**, and a unit is not. Drawing `mA` inside the cell would make the caret walk through
//! it, backspace eat it, and a numeric keypad type into it.
//!
//! # Why a bad entry reverts rather than clearing
//!
//! A field left mid-edit — the operator typed `12.` and walked away — parses as nothing. Clearing
//! to zero would command a machine to zero; leaving the text would leave the panel disagreeing
//! with the equipment. So the last value that parsed is restored, which is the only one of the
//! three that is certainly still true.
//!
//! # Why it takes an `f64`
//!
//! Alone among this crate's controls, because a setpoint is not a pixel: `25.0 °C` in an `f32`
//! carries about seven digits, and an instrument reading `1234.567 kPa` has already spent them.
//! The rest of the crate is `f32` because the rest of the crate is geometry.

use super::{NumberFieldLook, StepEnd};
use crate::cx::WidgetCx as Cx;
use crate::icons::{builtin, IconColor, IconRef, IconStyle};
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use std::ops::RangeInclusive;

/// A number with a unit, stepped by its ends and typed into in the middle.
#[derive(Debug)]
pub struct NumberField<'a> {
    value: &'a mut f64,
    range: RangeInclusive<f64>,
    step: f64,
    decimals: usize,
    unit: Option<&'a str>,
    enabled: bool,
}

impl<'a> NumberField<'a> {
    /// Edits `value`.
    #[must_use]
    pub fn new(value: &'a mut f64) -> Self {
        Self {
            value,
            range: 0.0..=100.0,
            step: 1.0,
            decimals: 1,
            unit: None,
            enabled: true,
        }
    }

    /// The range, inclusive. `0.0..=100.0` by default; the ends are sorted if they arrive the
    /// other way round, and a non-finite end is ignored.
    #[must_use]
    pub fn range(mut self, range: RangeInclusive<f64>) -> Self {
        let (lo, hi) = (*range.start(), *range.end());
        if lo.is_finite() && hi.is_finite() {
            self.range = if lo <= hi { lo..=hi } else { hi..=lo };
        }
        self
    }

    /// How much one press of an end moves it. Non-finite or non-positive is taken as 1.
    ///
    /// A press lands on the step's grid, counted from the range's low end: the next line up or
    /// down, so a value typed between two lines moves to the nearer one in that direction. Ten
    /// presses of `0.1` from `0.0` then read exactly `1.0`, not a float error short of it.
    #[must_use]
    pub fn step(mut self, step: f64) -> Self {
        if step.is_finite() && step > 0.0 {
            self.step = step;
        }
        self
    }

    /// How many decimal places are shown, and typed back. Capped at six — past that the figure is
    /// wider than the panel and the operator is reading noise.
    #[must_use]
    pub fn decimals(mut self, decimals: usize) -> Self {
        self.decimals = decimals.min(6);
        self
    }

    /// The unit, drawn **outside** the track so it cannot be edited.
    #[must_use]
    pub fn unit(mut self, unit: &'a str) -> Self {
        self.unit = Some(unit);
        self
    }

    /// Enabled.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. On the frame the value changed, `response.changed()`.
    pub fn show(mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let target = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
        let gap = cx.theme.control.gap;
        let cells = figure_cells(&self.range, self.decimals);
        let middle = middle_width(ui, cx, cells, target);
        let track_w = 2.0f32.mul_add(target, middle);

        let unit_galley = self.unit.map(|unit| {
            let font = egui::FontId::proportional(cx.theme.metrics.type_scale.body);
            ui.painter()
                .layout_no_wrap(unit.to_owned(), font, cx.theme.color(ColorRole::Muted))
        });
        let unit_w = unit_galley.as_ref().map_or(0.0, |g| gap + g.rect.width());
        let (rect, mut response) =
            ui.allocate_exact_size(Vec2::new(track_w + unit_w, target), Sense::hover());
        let track = Rect::from_min_size(rect.min, Vec2::new(track_w, target));

        *self.value = clamp(*self.value, &self.range);
        let ink = Ink::of(cx, self.enabled);

        let (ends, moved) = self.ends(ui, cx, track, response.id);
        if moved != 0.0 {
            // A figure typed when an end was pressed is taken first and the press steps from it.
            // Left in the store, it would be committed over the stepped value when the edit lets go.
            let typed = ui.data_mut(|d| d.remove_temp::<Draft>(response.id.with("buf")));
            if let Some(typed) = typed.and_then(|d| d.typed()) {
                *self.value = clamp(typed, &self.range);
            }
            *self.value = clamp(self.stepped(moved > 0.0), &self.range);
            response.mark_changed();
        }
        let [minus, plus] = ends;
        let cell = Rect::from_min_size(
            egui::pos2(track.min.x + target, track.min.y),
            Vec2::new(middle, target),
        );
        let unit_at = unit_galley.as_ref().map(|galley| {
            let at = egui::pos2(
                track.max.x + gap,
                rect.center().y - galley.rect.height() * 0.5,
            );
            Rect::from_min_size(at, galley.rect.size())
        });
        let custom = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.number_field.as_mut());
        let painted = custom.is_some();
        if let Some(custom) = custom {
            let figure = figure(*self.value, self.decimals);
            custom(
                ui.painter(),
                &mut NumberFieldLook {
                    rect,
                    track,
                    minus,
                    plus,
                    figure: cell,
                    figure_text: &figure,
                    value: *self.value,
                    unit: self.unit.zip(unit_at),
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            paint_track(ui.painter(), cx, track, ink);
            for (end, icon) in [(minus, builtin::MINUS), (plus, builtin::PLUS)] {
                paint_end(ui, cx, end.rect, &icon, end.live, end.press, ink);
            }
        }
        if self.edit(ui, cx, (cell, painted), response.id, ink) {
            response.mark_changed();
        }
        if let (Some(galley), Some(at), false) = (unit_galley, unit_at, painted) {
            ui.painter().galley(at.min, galley, ink.unit);
        }
        response
    }

    /// The two ends of `track` this frame — each one's state, and how far a tap on one moved the
    /// value. `id` is the field's.
    fn ends(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        track: Rect,
        id: egui::Id,
    ) -> ([StepEnd; 2], f64) {
        let target = track.height();
        let mut moved = 0.0;
        let mut ends = [StepEnd {
            rect: Rect::NOTHING,
            live: false,
            pressed: false,
            press: 0.0,
        }; 2];
        // Within a hair of an end counts as at it: a value a float error short of the top
        // reads as the top, and its `+` is spent.
        let hair = self.step * GRID_EPS;
        for (slot, (delta, spent)) in [
            (-self.step, *self.value <= *self.range.start() + hair),
            (self.step, *self.value >= *self.range.end() - hair),
        ]
        .into_iter()
        .enumerate()
        {
            let cell = if slot == 0 {
                Rect::from_min_size(track.min, Vec2::splat(target))
            } else {
                Rect::from_min_size(
                    egui::pos2(track.max.x - target, track.min.y),
                    Vec2::splat(target),
                )
            };
            let live = self.enabled && !spent;
            let end = ui.interact(
                cell,
                id.with(slot),
                if live { Sense::click() } else { Sense::hover() },
            );
            if end.clicked() {
                moved = delta;
            }
            let pressed = live && end.is_pointer_button_down_on();
            let press = cx.animate(
                end.id.with("press"),
                if pressed { 1.0 } else { 0.0 },
                if pressed {
                    cx.theme.motion.press
                } else {
                    cx.theme.motion.press_release
                },
            );
            if let Some(slot) = ends.get_mut(slot) {
                *slot = StepEnd {
                    rect: cell,
                    live,
                    pressed,
                    press,
                };
            }
        }
        (ends, moved)
    }

    /// The editable middle in `cell`. Returns whether the value moved. With `painted`, a painter
    /// drew the field and the figure of a disabled one with it.
    ///
    /// The in-progress text lives in egui's own per-frame store rather than in the widget, because
    /// the widget is rebuilt every frame and the half-typed figure has to outlive it.
    fn edit(
        &mut self,
        ui: &mut egui::Ui,
        cx: &Cx<'_>,
        (cell, painted): (Rect, bool),
        id: egui::Id,
        ink: Ink,
    ) -> bool {
        let key = id.with("buf");
        let shown = figure(*self.value, self.decimals);
        if !self.enabled {
            if !painted {
                paint_figure(ui.painter(), cx, cell, &shown, ink.figure);
            }
            return false;
        }
        let mut draft = ui
            .data(|d| d.get_temp::<Draft>(key))
            .unwrap_or_else(|| Draft {
                seed: shown.clone(),
                text: shown.clone(),
            });
        let buf = &mut draft.text;
        let font = egui::FontId::proportional(cx.theme.metrics.type_scale.body);
        let edit = egui::TextEdit::singleline(buf)
            .id(id.with("edit"))
            .horizontal_align(egui::Align::Center)
            .font(font.clone())
            // No frame of its own: the pill under it is already the field's boundary, and a
            // second one inside it would read as a box in a box.
            .frame(egui::Frame::NONE)
            .text_color(ink.figure);
        // Into a band the height of one line, centred in the cell — `Ui::put` lays a `TextEdit`
        // out at its own height and does not centre it, so put into the whole cell the figure sits
        // above the two ends beside it.
        let line = ui
            .painter()
            .layout_no_wrap(shown.clone(), font.clone(), ink.figure)
            .rect
            .height();
        let band = Rect::from_center_size(cell.center(), Vec2::new(cell.width(), line));
        let response = ui.put(band, edit);
        // Selecting the figure is a drag the field owns, and it says so.
        crate::drag::claim_if_held(&response);
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, draft));
            return false;
        }
        // Not focused: the store is stale by definition, so it goes, and whatever was typed is
        // committed once. Nothing typed commits nothing — the figure is the value rounded to
        // the decimals shown, and parsing it back would round the setpoint. A figure that
        // does not parse reverts — see the module doc.
        let had = ui.data_mut(|d| d.remove_temp::<Draft>(key));
        let Some(typed) = had.and_then(|d| d.typed()) else {
            return false;
        };
        let next = clamp(typed, &self.range);
        let moved = (next - *self.value).abs() > f64::EPSILON;
        *self.value = next;
        moved
    }
}

/// How close to a grid line or an end, in steps, counts as on it. Far below anything a figure
/// shows, far above the error a few hundred float additions gather.
const GRID_EPS: f64 = 1e-9;

/// The figure being typed, and the figure it started from — so a field focused and left
/// untouched commits nothing.
#[derive(Debug, Clone, Default)]
struct Draft {
    seed: String,
    text: String,
}

impl Draft {
    /// The number typed: `None` when the figure is untouched, or does not parse as a finite
    /// number (the field then reverts).
    fn typed(&self) -> Option<f64> {
        if self.text == self.seed {
            return None;
        }
        self.text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }
}

impl NumberField<'_> {
    /// The value one press of an end moves it to, before the range holds it: the next line of
    /// the step's grid above (`up`) or below, the grid counted from the range's low end.
    fn stepped(&self, up: bool) -> f64 {
        let lo = *self.range.start();
        let at = (*self.value - lo) / self.step;
        let line = if up {
            (at + GRID_EPS).floor() + 1.0
        } else {
            (at - GRID_EPS).ceil() - 1.0
        };
        line.mul_add(self.step, lo)
    }
}

/// `value` with `decimals` places, never `-0.0`: a value a float error below zero is zero on a
/// machine's panel.
fn figure(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    match text.strip_prefix('-') {
        Some(rest) if rest.chars().all(|c| c == '0' || c == '.') => rest.to_owned(),
        _ => text,
    }
}

/// `value`, held inside `range`. A non-finite value reads as the low end rather than reaching a
/// format string that would print `NaN` into a machine's setpoint.
fn clamp(value: f64, range: &RangeInclusive<f64>) -> f64 {
    if value.is_finite() {
        value.clamp(*range.start(), *range.end())
    } else {
        *range.start()
    }
}

/// The widest the figure can get, as a count of `0`-wide cells.
///
/// From the **range and the decimals**, never from the value — the cells rule the badge and the
/// stepper already follow, so a field does not breathe as its figure walks past ten.
fn figure_cells(range: &RangeInclusive<f64>, decimals: usize) -> u16 {
    // Counted rather than computed from a logarithm: `log10` of a value at the top of an `f64`'s
    // range lands on a boundary the float cannot resolve, and the count is what the format string
    // will actually produce.
    let digits = |v: f64| {
        let whole = format!("{:.0}", v.abs().trunc()).chars().count().max(1);
        let sign = usize::from(v < 0.0);
        let point = usize::from(decimals > 0);
        whole + sign + point + decimals
    };
    let widest = digits(*range.start()).max(digits(*range.end()));
    u16::try_from(widest).unwrap_or(u16::MAX)
}

/// The middle cell's width, floored so the track never collapses onto its two ends.
fn middle_width(ui: &egui::Ui, cx: &Cx<'_>, cells: u16, target: f32) -> f32 {
    let font = egui::FontId::proportional(cx.theme.metrics.type_scale.body);
    let cell = ui
        .painter()
        .layout_no_wrap("0".to_owned(), font, Color32::PLACEHOLDER)
        .rect
        .width();
    let wanted = 2.0f32.mul_add(cx.theme.control.gap, f32::from(cells) * cell);
    wanted.max(target)
}

/// The colours one frame paints with, faded **once** at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    track: Color32,
    edge: Color32,
    well: Color32,
    glyph: Color32,
    figure: Color32,
    unit: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, enabled: bool) -> Self {
        let tint = cx.theme.color(ColorRole::OnSurface);
        let ink = Self {
            track: tint.gamma_multiply(cx.theme.control.fill_alpha),
            edge: cx.theme.color(ColorRole::ControlEdge),
            well: tint.gamma_multiply(cx.theme.control.fill_alpha * 0.5),
            glyph: tint,
            figure: tint,
            // The unit is not the figure: it names the axis and the figure is the reading.
            unit: cx.theme.color(ColorRole::Muted),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                track: ink.track.gamma_multiply(a),
                edge: ink.edge.gamma_multiply(a),
                well: ink.well.gamma_multiply(a),
                glyph: ink.glyph.gamma_multiply(a),
                figure: ink.figure.gamma_multiply(a),
                unit: ink.unit.gamma_multiply(a),
            }
        }
    }
}

/// The pill, with the boundary that identifies it.
fn paint_track(painter: &egui::Painter, cx: &Cx<'_>, rect: Rect, ink: Ink) {
    let radius = CornerRadius::same(round_u8(rect.height() * 0.5));
    painter.rect_filled(rect, radius, ink.track);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(cx.theme.control.stroke_edge, ink.edge),
        StrokeKind::Inside,
    );
}

/// One end: its faint disc and its glyph.
fn paint_end(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    cell: Rect,
    icon: &IconRef,
    live: bool,
    t_press: f32,
    ink: Ink,
) {
    let d = cx.theme.control.icon_button;
    let radius = d * 0.5 + cx.theme.control.press_grow * t_press;
    let alpha = if live {
        1.0
    } else {
        cx.theme.control.disabled_alpha
    };
    ui.painter()
        .circle_filled(cell.center(), radius, ink.well.gamma_multiply(alpha));
    let glyph = cx
        .theme
        .control
        .icon
        .min(d * std::f32::consts::FRAC_1_SQRT_2);
    let at = Rect::from_center_size(cell.center(), Vec2::splat(glyph));
    // Sized to the glyph, so the stroke follows it rather than `default()`'s 24.
    let style = IconStyle::sized(glyph).color(IconColor::Fixed(ink.glyph.gamma_multiply(alpha)));
    cx.icons.paint(ui.painter(), at, icon, &style, cx.theme);
}

/// The figure, where the field is not editable.
fn paint_figure(painter: &egui::Painter, cx: &Cx<'_>, cell: Rect, text: &str, color: Color32) {
    painter.text(
        cell.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(cx.theme.metrics.type_scale.body),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::{clamp, figure_cells};

    /// **The width comes from the range and the decimals, never from the value**, so a field does
    /// not breathe as its figure walks.
    #[test]
    fn the_width_comes_from_the_range_and_the_decimals() {
        assert_eq!(figure_cells(&(0.0..=9.0), 0), 1);
        assert_eq!(figure_cells(&(0.0..=9.0), 1), 3); // "9.0"
        assert_eq!(figure_cells(&(0.0..=250.0), 1), 5); // "250.0"
                                                        // A negative end is one cell wider than its magnitude: the sign takes a place.
        assert_eq!(figure_cells(&(-40.0..=40.0), 1), 5); // "-40.0"
        assert_eq!(figure_cells(&(0.0..=1.0), 3), 5); // "1.000"
    }

    /// A non-finite value never reaches a format string, because the string would print `NaN` into
    /// a machine's setpoint.
    #[test]
    fn a_non_finite_value_reads_as_the_low_end() {
        let range = -10.0..=10.0;
        assert!((clamp(f64::NAN, &range) - -10.0).abs() < f64::EPSILON);
        assert!((clamp(f64::INFINITY, &range) - -10.0).abs() < f64::EPSILON);
        assert!((clamp(20.0, &range) - 10.0).abs() < f64::EPSILON);
        assert!((clamp(-20.0, &range) - -10.0).abs() < f64::EPSILON);
        assert!((clamp(2.5, &range) - 2.5).abs() < f64::EPSILON);
    }

    /// The builders refuse what would freeze or invert the control, rather than trusting it.
    #[test]
    fn the_builders_refuse_a_range_or_a_step_that_would_break_it() {
        let mut v = 0.0;
        let f = super::NumberField::new(&mut v).range(f64::NAN..=1.0);
        assert_eq!(f.range, 0.0..=100.0, "a non-finite end was let through");
        let mut v = 0.0;
        let (lo, hi) = (10.0, 1.0);
        let f = super::NumberField::new(&mut v).range(lo..=hi);
        assert_eq!(f.range, 1.0..=10.0, "an inverted range was not sorted");
        let mut v = 0.0;
        assert!((super::NumberField::new(&mut v).step(0.0).step - 1.0).abs() < f64::EPSILON);
        let mut v = 0.0;
        assert!((super::NumberField::new(&mut v).step(0.25).step - 0.25).abs() < f64::EPSILON);
        let mut v = 0.0;
        assert_eq!(super::NumberField::new(&mut v).decimals(99).decimals, 6);
    }
}
