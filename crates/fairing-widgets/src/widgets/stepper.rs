//! `Stepper` — `−  n  +`, one track with three cells.
//!
//! # Why this is a control and not screen content
//!
//! `layout`'s module doc used to list "a quantity stepper" among the things the crate deliberately
//! does not make, beside a cart line and a star rating. That line is right about two of the three
//! and wrong about this one, and the crate's own kiosk is the evidence: it wrote 55 lines of
//! stepper by hand.
//!
//! A cart line and a star rating are **trade content** — what they hold differs in every shop. A
//! stepper is a **scalar editor**, and it is the same object in a kiosk (how many coffees), on a
//! thermostat (what temperature), on an oven (how many minutes) and on a press (how many parts).
//! It belongs beside [`Switch`](super::Switch), [`Checkbox`](super::Checkbox) and
//! [`TouchSlider`](super::TouchSlider), all three of which edit a value the caller owns. iOS ships
//! `UIStepper` and Windows ships `NumberBox` for the same reason.
//!
//! # What the crate's own stepper got wrong
//!
//! Three things, each of which this fixes:
//!
//! 1. It drew `"+"` and `"−"` as **text**. The checkbox's doc already forbids exactly that — a
//!    typeface an integrator supplies may not carry the glyph, and tofu in a control is worse than
//!    no control. The crate has `plus` and `minus` icons; this draws those.
//! 2. Its radius, its digit size and its value cell were eyeballed multiples of a local `step`
//!    (`0.34`, `0.46`, `0.9`), so a panel that retuned its metrics retuned nothing here.
//! 3. It used [`BigButton`](super::BigButton) for each end, which senses and grows as a *button*
//!    inside a track it does not know about.
//!
//! # Why one track and not three buttons
//!
//! Both reference kiosks draw one pill with three cells — a faint disc at each end and the figure
//! between — and not three separate objects. One track says "these two ends change this number";
//! three buttons say "here are three things".
//!
//! The track's corner is **half its height**, and that is concentricity rather than decoration:
//! the end discs are circles of `control.icon_button` inside a track of `metrics.touch_target`, so
//! a half-height corner makes the cap and the disc concentric and the inset reads as even. It is
//! the argument the switch's own geometry makes for its knob, applied to a wider track.
//!
//! Measured, the references' end disc is 0.68 of their track's height; `control.icon_button` is
//! 0.68 of a touch target, and the track here is one touch target tall — so the proportion comes
//! out at the reference's exactly, from tokens that were derived somewhere else.
//!
//! # Why there is no press-and-hold repeat
//!
//! A repeat needs a rate, a rate needs a token, and a token needs a reason. A quantity a finger
//! walks to is a quantity under about a dozen; past that the control wanted was a field with a
//! keypad, not a button pressed forty times. If a range really needs repeating, that is the
//! argument for a number field, and it is a different element.
//!
//! # Why the exhausted end fades rather than disappearing
//!
//! At the bottom of the range the `−` is unavailable, and it goes to `control.disabled_alpha` and
//! stops sensing — the crate's one disabled mechanism. Removing it would reflow the track under
//! the finger, which is Rule 1.5 read the other way round: the drawing may change, the places may
//! not.

use super::{StepEnd, StepperLook};
use crate::cx::WidgetCx as Cx;
use crate::icons::{builtin, IconColor, IconRef, IconStyle};
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use std::ops::RangeInclusive;

/// A `−  n  +` editor for a whole number.
#[derive(Debug)]
pub struct Stepper<'a> {
    value: &'a mut i32,
    range: RangeInclusive<i32>,
    step: i32,
    enabled: bool,
}

impl<'a> Stepper<'a> {
    /// Edits `value`.
    #[must_use]
    pub fn new(value: &'a mut i32) -> Self {
        Self {
            value,
            range: 0..=99,
            step: 1,
            enabled: true,
        }
    }

    /// The range, inclusive. `0..=99` by default.
    ///
    /// An inverted range is taken as the caller meant it — the ends are sorted — because a stepper
    /// that refused to draw would be a blank hole in a cart row.
    #[must_use]
    pub fn range(mut self, range: RangeInclusive<i32>) -> Self {
        self.range = if range.start() <= range.end() {
            range
        } else {
            *range.end()..=*range.start()
        };
        self
    }

    /// How much one press moves it. 1 by default; zero or negative is taken as 1.
    #[must_use]
    pub fn step(mut self, step: i32) -> Self {
        self.step = step.max(1);
        self
    }

    /// Enabled. Disabled it senses `hover()`, so the tap falls through to the row beneath.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. On the frame the value changed, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let target = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
        let digits = digit_cells(*self.range.start(), *self.range.end());
        let value_w = value_width(ui, cx, digits, target);
        let size = Vec2::new(2.0f32.mul_add(target, value_w), target);

        // The track itself never senses a click: the two ends do, inside it. A track that also
        // sensed would swallow a tap that missed an end and report nothing, which reads as a
        // broken control rather than as a miss.
        let (rect, mut response) = ui.allocate_exact_size(size, Sense::hover());
        *self.value = (*self.value).clamp(*self.range.start(), *self.range.end());

        // The two ends are their own targets inside the track; the figure between is not tappable,
        // so a fat finger landing on the number does nothing rather than guessing a direction.
        let mut moved = 0;
        let mut ends = [StepEnd {
            rect: Rect::NOTHING,
            live: false,
            pressed: false,
            press: 0.0,
        }; 2];
        for (slot, (delta, at_end)) in [
            (-self.step, *self.value <= *self.range.start()),
            (self.step, *self.value >= *self.range.end()),
        ]
        .into_iter()
        .enumerate()
        {
            let cell = if slot == 0 {
                Rect::from_min_size(rect.min, Vec2::new(target, target))
            } else {
                Rect::from_min_size(
                    egui::pos2(rect.max.x - target, rect.min.y),
                    Vec2::new(target, target),
                )
            };
            let live = self.enabled && !at_end;
            let end = ui.interact(
                cell,
                response.id.with(slot),
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
        if moved != 0 {
            *self.value = self
                .value
                .saturating_add(moved)
                .clamp(*self.range.start(), *self.range.end());
            response.mark_changed();
        }
        let [minus, plus] = ends;
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.stepper.as_mut()) {
            custom(
                ui.painter(),
                &mut StepperLook {
                    rect,
                    minus,
                    plus,
                    value: *self.value,
                    range: self.range.clone(),
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }
        let ink = Ink::of(cx, self.enabled);
        paint_track(ui.painter(), cx, rect, ink);
        for (end, icon) in [(minus, builtin::MINUS), (plus, builtin::PLUS)] {
            let style = End {
                live: end.live,
                t_press: end.press,
            };
            paint_end(ui, cx, end.rect, &icon, style, ink);
        }

        let font = egui::FontId::proportional(cx.theme.metrics.type_scale.body);
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            self.value.to_string(),
            font,
            ink.figure,
        );
        response
    }
}

/// How one end is drawn this frame.
#[derive(Debug, Clone, Copy)]
struct End {
    /// Whether this direction is still available.
    live: bool,
    t_press: f32,
}

/// The widest the figure can get, as a count of `0`-wide cells.
///
/// The cells rule the badge uses, for the same reason: with proportional figures a run-laid number
/// changes width between 1 and 8, so a stepper laid out that way breathes as its value walks.
fn digit_cells(lo: i32, hi: i32) -> u16 {
    let width = |n: i32| u16::try_from(n.to_string().chars().count()).unwrap_or(u16::MAX);
    width(lo).max(width(hi))
}

/// The value cell's width, floored so the track never gets narrower than its two ends plus a gap.
fn value_width(ui: &egui::Ui, cx: &Cx<'_>, cells: u16, target: f32) -> f32 {
    let font = egui::FontId::proportional(cx.theme.metrics.type_scale.body);
    let cell = ui
        .painter()
        .layout_no_wrap("0".to_owned(), font, Color32::PLACEHOLDER)
        .rect
        .width();
    let wanted = 2.0f32.mul_add(cx.theme.control.gap, f32::from(cells) * cell);
    wanted.max(target * 0.5)
}

/// The colours one frame paints with, faded **once** at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    track: Color32,
    edge: Color32,
    /// The faint disc under each end's glyph.
    well: Color32,
    glyph: Color32,
    figure: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, enabled: bool) -> Self {
        let ink = Self {
            track: cx
                .theme
                .color(ColorRole::OnSurface)
                .gamma_multiply(cx.theme.control.fill_alpha),
            edge: cx.theme.color(ColorRole::ControlEdge),
            // Half the track's own tint: the disc has to lift off the track without becoming a
            // second boundary, so it is the same ink at half the strength rather than a new role.
            well: cx
                .theme
                .color(ColorRole::OnSurface)
                .gamma_multiply(cx.theme.control.fill_alpha * 0.5),
            glyph: cx.theme.color(ColorRole::OnSurface),
            figure: cx.theme.color(ColorRole::OnSurface),
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
            }
        }
    }
}

/// The pill, with the boundary that identifies it.
fn paint_track(painter: &egui::Painter, cx: &Cx<'_>, rect: Rect, ink: Ink) {
    // Half the height, so the cap and the end disc inside it are concentric — the switch's own
    // argument, on a wider track.
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
fn paint_end(ui: &egui::Ui, cx: &mut Cx<'_>, cell: Rect, icon: &IconRef, end: End, ink: Ink) {
    let d = cx.theme.control.icon_button;
    let radius = d * 0.5 + cx.theme.control.press_grow * end.t_press;
    let alpha = if end.live {
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
    let style = IconStyle {
        color: IconColor::Fixed(ink.glyph.gamma_multiply(alpha)),
        ..IconStyle::default()
    };
    cx.icons.paint(ui.painter(), at, icon, &style, cx.theme);
}

#[cfg(test)]
mod tests {
    use super::digit_cells;

    /// **The figure's cell count comes from the range, not the value**, so a stepper does not
    /// change width as its number walks past ten.
    #[test]
    fn the_width_comes_from_the_range_and_not_from_the_value() {
        assert_eq!(digit_cells(0, 9), 1);
        assert_eq!(digit_cells(0, 99), 2);
        assert_eq!(digit_cells(0, 100), 3);
        // A negative bound is wider than its magnitude, because the sign takes a cell.
        assert_eq!(digit_cells(-5, 5), 2);
        assert_eq!(digit_cells(-40, 250), 3);
    }

    /// The range's ends are sorted rather than refused: a stepper that would not draw is a hole in
    /// a cart row, and the caller's intent is unambiguous either way round.
    #[test]
    fn an_inverted_range_is_taken_as_it_was_meant() {
        let mut value = 3;
        // Built from its ends rather than written `10..=1`, which clippy reads as a mistake
        // at the call site — which it would be, everywhere except here.
        let (lo, hi) = (10, 1);
        let stepper = super::Stepper::new(&mut value).range(lo..=hi);
        assert_eq!(stepper.range, 1..=10);
    }

    /// A zero or negative step would freeze the control or invert its ends; it is taken as one.
    #[test]
    fn a_step_of_zero_still_moves_the_value() {
        let mut value = 0;
        assert_eq!(super::Stepper::new(&mut value).step(0).step, 1);
        let mut value = 0;
        assert_eq!(super::Stepper::new(&mut value).step(-4).step, 1);
        let mut value = 0;
        assert_eq!(super::Stepper::new(&mut value).step(5).step, 5);
    }
}
