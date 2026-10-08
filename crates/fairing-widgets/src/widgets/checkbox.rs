//! `Checkbox` — a `control.mark_size` box inside a full `touch_target` slot, carrying a tick that
//! is **drawn, not typed** (control vocabulary).
//!
//! # Why the box is smaller than the thing you press
//!
//! The slot is a whole `metrics.touch_target` on both axes and the box is `control.mark_size`
//! (0.600 T) centred in it. Drawing the box at the size of the target was the alternative, and it
//! is worse in both directions: at 13 mm the box becomes a slab that reads as a card rather than a
//! control, and shrinking the target to the box is what made the switch's hit rect 0.60 T on its
//! short axis — a miss a gloved hand makes on a moving vehicle. What you press and what you see
//! are two rects, and only the drawn one is allowed to be small.
//!
//! # Why the tick is two line segments and not a glyph or an icon
//!
//! A glyph (`"✓"`) comes out as tofu (□) the moment an integrator installs a typeface without it —
//! the same reason [`ListRow::trailing_icon`](super::ListRow::trailing_icon) exists instead of a
//! text mark. Routing it through the icon painter is rejected for a different reason: a tick's
//! vertex is a sharp corner, and epaint's path strokes miter it (`PathShape::line` bevels only
//! below 90°, `closed_line` does not cut at all), so the corner grows a spike or a flat depending
//! on the angle. Two `line_segment`s plus a filled circle at the vertex and at each free end give
//! a round join whose radius is exactly half the stroke, at every scale, with no path code.
//!
//! # Why the same size as the radio
//!
//! Checkbox and radio share `control.mark_size`, so "one of these" and "any of these" are told
//! apart by **shape alone** — square versus circle — at a glance and in greyscale. Making the
//! checkbox its own size would leave shape *and* size differing, and neither difference readable.

use super::CheckboxLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// A checkbox.
///
/// It edits a `&mut bool` and hands back the [`Response`], the same shape as
/// [`Switch`](super::Switch) — a control that owns its value would force the screen to keep a
/// second copy of state it already has.
#[derive(Debug)]
pub struct Checkbox<'a> {
    on: &'a mut bool,
    enabled: bool,
    indeterminate: bool,
}

impl<'a> Checkbox<'a> {
    /// Edits `on`.
    pub fn new(on: &'a mut bool) -> Self {
        Self {
            on,
            enabled: true,
            indeterminate: false,
        }
    }

    /// Enabled.
    ///
    /// Disabled, it senses `hover()` rather than `click()`, so the tap **falls through** to
    /// whatever is beneath it — a disabled control that swallows the press makes the row it sits
    /// in feel broken rather than unavailable.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw the **mixed** mark — a bar rather than a tick — for a box that stands for a set of
    /// values that are not all the same ("some of these are on").
    ///
    /// It is a flag beside `on` rather than a third value of `on`, because the caller already owns
    /// the bool and a tri-state enum would make every ordinary call site match on a case it never
    /// has. A tap on a mixed box reports `changed()` with `on = true` (the convention iOS, GTK and
    /// Windows share: the mixed state is the caller's to clear, and a tap leaves it by committing
    /// to "all on", never by going back to it).
    #[must_use]
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        self.indeterminate = indeterminate;
        self
    }

    /// Draw it. On the frame the value changed, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let ms = cx.theme.control.box_size();
        // Rule 1.1: the slot is a full target on **both** axes; `max` so a device that sets a
        // mark larger than its target still gets a slot that holds the drawing.
        let side = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control).max(ms);
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(side), sense);
        if self.enabled && response.clicked() {
            *self.on = if self.indeterminate { true } else { !*self.on };
            response.mark_changed();
        }
        let on = *self.on || self.indeterminate;
        let pressed = self.enabled && response.is_pointer_button_down_on();

        // Two tweens on one control: the face crossfades as the mark appears, and the press growth
        // runs on its own clock (80 ms out, 120 ms back) so a tap during a crossfade does not
        // restart it.
        let t_on = cx.animate(
            response.id.with("on"),
            if on { 1.0 } else { 0.0 },
            cx.theme.motion.crossfade,
        );
        let t_press = cx.animate(
            response.id.with("press"),
            if pressed { 1.0 } else { 0.0 },
            if pressed {
                cx.theme.motion.press
            } else {
                cx.theme.motion.press_release
            },
        );

        // Rule 1.5: the growth is on the **painted** rect only. `rect` above is already allocated
        // and does not move, so a pressed row never reflows.
        let drawn = Rect::from_center_size(rect.center(), Vec2::splat(ms))
            .expand(cx.theme.control.press_grow * t_press);
        let focused = response.has_focus();
        if let Some(paint) = cx.painters.as_deref_mut().and_then(|p| p.checkbox.as_mut()) {
            paint(
                ui.painter(),
                &mut CheckboxLook {
                    rect,
                    drawn,
                    on: *self.on,
                    indeterminate: self.indeterminate,
                    mark: t_on,
                    pressed,
                    press: t_press,
                    enabled: self.enabled,
                    focused,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }
        let ink = Ink::of(cx, t_on, pressed, self.enabled);
        paint_body(ui.painter(), cx, drawn, t_on, ink);
        if t_on > 0.0 {
            let at = Rect::from_center_size(
                drawn.center(),
                Vec2::splat(ms * cx.theme.control.tick_ratio),
            );
            let width = cx.theme.control.stroke_mark;
            if self.indeterminate {
                paint_mixed(ui.painter(), at, width, ink.mark);
            } else {
                paint_tick(ui.painter(), at, width, ink.mark);
            }
        }
        if focused {
            paint_focus_ring(ui.painter(), cx, drawn, ink.ring);
        }
        response
    }
}

/// The four colours one frame paints with, so the disabled fade is **one** call on **one** value
/// rather than a `gamma_multiply` sprinkled down the paint path: one mechanism, one number.
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// The filled body.
    face: Color32,
    /// The identifying boundary, which only the off face carries.
    edge: Color32,
    /// The tick or the mixed bar.
    mark: Color32,
    /// The focus ring.
    ring: Color32,
}

impl Ink {
    /// The colours for this frame, dimmed once at the end when the control is disabled.
    fn of(cx: &Cx<'_>, t_on: f32, pressed: bool, enabled: bool) -> Self {
        let mut face = cx
            .theme
            .color(ColorRole::SurfaceVariant)
            .lerp_to_gamma(cx.theme.color(ColorRole::Primary), t_on);
        if pressed {
            // `face.blend(tint)`, not `tint.blend(face)`: `blend`'s **receiver is the layer
            // behind**, so the other order multiplies the tint by `255 - 255` over an opaque face
            // and paints nothing at all.
            face = face.blend(cx.theme.color(ColorRole::Pressed));
        }
        let ink = Self {
            face,
            // The on face is its own boundary, so the edge leaves on the same tween the fill
            // arrives on — holding it to the end would draw a `Muted` line around a `Primary` box.
            edge: cx
                .theme
                .color(ColorRole::ControlEdge)
                .gamma_multiply(1.0 - t_on),
            mark: cx.theme.color(ColorRole::OnPrimary).gamma_multiply(t_on),
            ring: cx.theme.color(ColorRole::Focus),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                face: ink.face.gamma_multiply(a),
                edge: ink.edge.gamma_multiply(a),
                mark: ink.mark.gamma_multiply(a),
                ring: ink.ring.gamma_multiply(a),
            }
        }
    }
}

/// The body: a `mark_radius` box, edged while it is off. `t_on` is 0 off, 1 on; `rect` is the
/// silhouette actually drawn this frame, press growth included.
fn paint_body(painter: &egui::Painter, cx: &Cx<'_>, rect: Rect, t_on: f32, ink: Ink) {
    let radius = CornerRadius::same(round_u8(cx.theme.control.mark_radius));
    painter.rect_filled(rect, radius, ink.face);
    if t_on < 1.0 {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(cx.theme.control.stroke_edge, ink.edge),
            // Inside, so the edge is part of the silhouette and the box never measures wider than
            // `mark_size` — the leading column in a row is exactly that wide.
            StrokeKind::Inside,
        );
    }
}

/// The tick, inside `at`.
///
/// `at` is the mark's **outer** extent: the path is inset by half the stroke first, so the round
/// caps land on `at`'s edge and the drawn tick measures `mark_size × tick_ratio` however thick the
/// stroke has resolved. The inset is capped at a quarter of the box so a pathological
/// stroke-to-mark ratio degrades to a small tick rather than an inside-out rect.
pub(crate) fn paint_tick(painter: &egui::Painter, at: Rect, width: f32, color: Color32) {
    let path = at.shrink((width * 0.5).min(at.width() * 0.25));
    let [a, b, c] = tick_points(path);
    let stroke = Stroke::new(width, color);
    painter.line_segment([a, b], stroke);
    painter.line_segment([b, c], stroke);
    // The joins and the two free ends. epaint's line joins are mitered, and a tick's vertex is
    // exactly the acute corner that grows a spike, so the corner is drawn as a disc instead.
    for p in [a, b, c] {
        painter.circle_filled(p, width * 0.5, color);
    }
}

/// The three points of the tick within its path box: the short leg's free end, the vertex, and the
/// long leg's free end. Fractions rather than lengths, so the shape is identical at every scale.
fn tick_points(path: Rect) -> [Pos2; 3] {
    let at = |u: f32, v: f32| {
        Pos2::new(
            path.min.x + path.width() * u,
            path.min.y + path.height() * v,
        )
    };
    // The vertex sits about a third along, which is the proportion iOS, Material and One UI all
    // land on; a centred vertex reads as a "v" rather than a tick.
    [at(0.0, 0.55), at(0.36, 1.0), at(1.0, 0.0)]
}

/// The mixed mark: a bar as wide as the tick and as thick as the tick's stroke, pill-ended so it
/// reads as the same instrument as the tick rather than as a separate slab.
pub(crate) fn paint_mixed(painter: &egui::Painter, at: Rect, width: f32, color: Color32) {
    let bar = Rect::from_center_size(at.center(), Vec2::new(at.width(), width));
    painter.rect_filled(bar, CornerRadius::same(round_u8(width * 0.5)), color);
}

/// The focus ring, **outside** the silhouette.
///
/// Outside is a contrast decision, not a styling one: `Focus` measured against `Primary` is
/// 1.00–1.62 in the shipped palettes, so a ring on the box's own edge is invisible on a checked
/// box in every one of them, while against the container behind it the same colour clears 3.0
/// everywhere. It is measured from the **painted** rect rather than the resting one, because
/// `press_grow` is larger than `focus_gap` — from the resting rect a pressed box would grow
/// through its own ring.
fn paint_focus_ring(painter: &egui::Painter, cx: &Cx<'_>, silhouette: Rect, color: Color32) {
    let gap = cx.theme.control.focus_gap;
    let width = cx.theme.control.stroke_mark;
    let out = gap + width * 0.5;
    painter.rect_stroke(
        silhouette.expand(out),
        CornerRadius::same(round_u8(cx.theme.control.mark_radius + out)),
        Stroke::new(width, color),
        // Middle: the stroke straddles the expanded rect, so its inner face sits exactly `gap`
        // clear of the silhouette.
        StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::tick_points;
    use egui::{pos2, Rect};

    /// The tick fills its path box and its vertex is the lowest of the three points — the two
    /// properties the caps and the inset arithmetic depend on.
    #[test]
    fn the_tick_fills_its_box_with_the_vertex_at_the_bottom() {
        let path = Rect::from_min_max(pos2(10.0, 20.0), pos2(40.0, 50.0));
        let [a, b, c] = tick_points(path);
        for p in [a, b, c] {
            assert!(path.contains(p), "{p:?} escaped {path:?}");
        }
        assert!(b.y > a.y && b.y > c.y, "the vertex is not the lowest point");
        assert!(
            a.x < b.x && b.x < c.x,
            "the tick does not run left to right"
        );
        // The short leg is the short one: a tick, not a chevron.
        assert!((b - a).length() < (c - b).length());
    }
}
