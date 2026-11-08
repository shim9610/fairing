//! `SegmentedControl` selects one of a small set of alternatives.
//!
//! The selected face moves between cells using `motion.switch`. Cells share a
//! width and use `metrics.touch_target` as a minimum. When the options do not fit
//! the available strip, the control uses stacked `ListRow` choice rows instead of
//! shrinking or truncating the labels. Check the resulting layout in the available
//! pane, with the actual translated labels.

// UI geometry: a segment count of at most four and pixel values crossing to f32. The loss is
// meaningless in this range, so the cast lints are lifted for the whole file (the rest of the pedantic set stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::ListRow;
use super::SegmentedLook;
use crate::cx::WidgetCx as Cx;
use crate::icons::IconRef;
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// A row of mutually exclusive options. The caller supplies the selected index
/// and receives a newly picked index in the result.
#[derive(Debug)]
pub struct SegmentedControl<'a> {
    labels: &'a [&'a str],
    selected: usize,
    enabled: bool,
}

impl<'a> SegmentedControl<'a> {
    /// Construct from options and the caller's selected index.
    /// For nonempty options, an index past the end is clamped to the last option.
    #[must_use]
    pub fn new(labels: &'a [&'a str], selected: usize) -> Self {
        Self {
            labels,
            selected: selected.min(labels.len().saturating_sub(1)),
            enabled: true,
        }
    }

    /// Enabled state. A disabled control uses hover sensing rather than click/drag
    /// sensing, allowing another eligible target to handle the press.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Whether [`Self::show`] would use a strip at the current width.
    /// Uses the same measurement as `show`; false selects stacked rows. A caller may
    /// use this result to choose an alternative control.
    #[must_use]
    pub fn fits(&self, ui: &egui::Ui, cx: &Cx<'_>) -> bool {
        let font = egui::TextStyle::Button.resolve(ui.style());
        self.cell_width(ui, cx, &font).is_some()
    }

    /// Draw it.
    ///
    /// The strip form is taken when the options fit it; otherwise the stacked form is, and the
    /// decision is made per frame from the width actually available (see the module docs).
    /// [`Self::fits`] answers the same question before anything is drawn.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> SegmentedPick {
        if self.labels.is_empty() {
            // Nothing to choose between. Allocating a full-height empty strip would leave a
            // mystery gap in the screen, so it takes no space at all.
            let (_, response) = ui.allocate_exact_size(Vec2::ZERO, Sense::hover());
            return SegmentedPick {
                response,
                picked: None,
            };
        }
        let font = egui::TextStyle::Button.resolve(ui.style());
        let cell = self.cell_width(ui, cx, &font);
        if let Some(cell) = cell {
            self.strip(ui, cx, &font, cell)
        } else {
            self.stacked(ui, cx)
        }
    }

    /// The width one segment would get, or `None` when the strip form is not legal here.
    ///
    /// Three conditions, all of which mean the same failure — the options can no longer be read
    /// and hit side by side: more options than `control.segment_max`, a cell under
    /// `metrics.touch_target`, or a label that does not fit its cell.
    fn cell_width(&self, ui: &egui::Ui, cx: &Cx<'_>, font: &FontId) -> Option<f32> {
        let c = &cx.theme.control;
        let n = self.labels.len();
        if n > c.segment_max as usize {
            return None;
        }
        // The band the cells share: the slot less the focus ring's margin (see `strip`) and the
        // face's inset. Measured from the bare slot, the cells came out `2 × margin` too wide
        // between them, and the last one's face ran past the strip's right end.
        let band = ui.available_width() - 2.0 * (ring_margin(cx) + c.mark_radius);
        let cell = band / n as f32;
        if cell < crate::theme::control_height(&cx.theme.metrics, &cx.theme.control) {
            return None;
        }
        // The label is kept `control.gap` clear of the cell's edges: it is the hand-family gap
        // token, and two labels in adjacent cells must not read as one word.
        let room = cell - 2.0 * c.gap;
        let painter = ui.painter();
        for label in self.labels {
            let w = painter
                .layout_no_wrap((*label).to_owned(), font.clone(), Color32::PLACEHOLDER)
                .size()
                .x;
            if w > room {
                return None;
            }
        }
        Some(cell)
    }

    /// The strip form: one slot, `n` cells, one travelling face.
    fn strip(self, ui: &mut egui::Ui, cx: &mut Cx<'_>, font: &FontId, cell: f32) -> SegmentedPick {
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        // Rule 1.1: the slot is a full `touch_target` on the short axis, and every cell is at
        // least that wide, so each segment is a target in its own right.
        let size = Vec2::new(
            ui.available_width(),
            crate::theme::control_height(&cx.theme.metrics, &cx.theme.control),
        );
        let (slot, mut response) = ui.allocate_exact_size(size, sense);
        // **The strip is drawn inside the slot, with the focus ring's room left around it.**
        //
        // Drawing the strip flush and expanding the ring outwards puts the ring outside the rect
        // the parent clips to, and for a full-bleed control that is most of the ring: measured on a
        // 400-wide panel, the left, right and top runs fell entirely outside the clip and only the
        // bottom edge survived, so focus read as a stray underline or as nothing at all. Insetting
        // the ring instead is no better — it would cross a selected segment, and `Focus` against
        // `Primary` is 1.00–1.62 in the shipped palettes.
        //
        // So the slot keeps its full width (the touch target is the whole strip) and the *drawing*
        // gives up a margin the ring can live in.
        let rect = slot.shrink(ring_margin(cx));
        let strip = Strip::new(rect, cell, self.labels.len(), cx);

        let at = response
            .interact_pointer_pos()
            .map(|p| strip.index_at(p.x))
            .filter(|_| self.enabled);
        let mut picked = None;
        if response.clicked() {
            // A tap on the segment that is already selected is not a pick — the same contract
            // `layout::choice_rows` has, so a screen can swap one for the other unchanged.
            picked = at.filter(|i| *i != self.selected);
            if picked.is_some() {
                response.mark_changed();
            }
        }
        let held = at.filter(|_| response.is_pointer_button_down_on());

        // The face is aimed at the index picked **this** frame, so the travel starts on the frame
        // of the tap rather than on the frame after the caller writes the value back.
        let t_sel = cx.animate(
            response.id.with("sel"),
            picked.unwrap_or(self.selected) as f32,
            cx.theme.motion.switch,
        );
        // The press runs on its own clock (80 ms out, 120 ms back) so a tap during a travel does
        // not restart the travel.
        let t_press = cx.animate(
            response.id.with("press"),
            if held.is_some() { 1.0 } else { 0.0 },
            if held.is_some() {
                cx.theme.motion.press
            } else {
                cx.theme.motion.press_release
            },
        );

        let focused = response.has_focus();
        if let Some(paint) = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.segmented.as_mut())
        {
            paint(
                ui.painter(),
                &mut SegmentedLook {
                    rect: slot,
                    strip: rect,
                    band: strip.inner,
                    labels: self.labels,
                    selected: picked.unwrap_or(self.selected),
                    travel: t_sel,
                    held,
                    press: t_press,
                    enabled: self.enabled,
                    focused,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return SegmentedPick { response, picked };
        }
        let ink = Ink::of(cx, self.enabled);
        let painter = ui.painter();
        paint_strip(painter, cx, rect, ink);
        // Rule 1.5: the growth is on the **painted** rect only — `rect` is already allocated and
        // does not move, so a press never reflows the row the strip sits in.
        let grow = cx.theme.control.press_grow * t_press;
        let face_grow = grow * held.map_or(0.0, |i| coverage(t_sel, i));
        painter.rect_filled(
            strip.cell_rect(t_sel).expand(face_grow),
            strip.radius,
            ink.face,
        );
        strip.paint_dividers(painter, cx, t_sel, ink.divider);
        if let Some(i) = held {
            // The tint is *painted over* the face rather than blended into it: laying a
            // translucent `Pressed` on top is `face.blend(pressed)` by construction, which is the
            // order a review found inverted at three sites in the shell.
            painter.rect_filled(
                strip.cell_rect(i as f32).expand(grow),
                strip.radius,
                ink.tint,
            );
        }
        strip.paint_labels(painter, self.labels, font, t_sel, ink);
        if focused {
            paint_focus_ring(painter, cx, rect, ink.ring);
        }
        SegmentedPick { response, picked }
    }

    /// The stacked form: one [`ListRow`] per option, a `check` on the selected one.
    ///
    /// An icon rather than a `"✓"` glyph, for the reason [`ListRow::trailing_icon`] exists — with
    /// an integrator's own typeface installed a missing glyph comes out as tofu (□).
    fn stacked(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> SegmentedPick {
        let last = self.labels.len().saturating_sub(1);
        let mut picked = None;
        let mut response: Option<Response> = None;
        for (index, label) in self.labels.iter().enumerate() {
            let mut row = ListRow::new(*label)
                .chevron(false)
                // The last row's separator would land on the card's own edge.
                .separator(index != last)
                .enabled(self.enabled);
            if index == self.selected {
                row = row.trailing_icon(IconRef::Builtin("check"));
            }
            let r = row.show(ui, cx);
            if r.clicked() && index != self.selected {
                picked = Some(index);
            }
            response = Some(match response {
                Some(acc) => acc.union(r),
                None => r,
            });
        }
        SegmentedPick {
            // Unreachable — `show` returns early on an empty slice — but a widget does not panic
            // while painting, so the empty case degrades to a zero-size response instead.
            response: response
                .unwrap_or_else(|| ui.allocate_exact_size(Vec2::ZERO, Sense::hover()).1),
            picked,
        }
    }
}

/// What one [`SegmentedControl`] reported this frame.
///
/// Two fields rather than a bare `Option<usize>` because the caller still needs the
/// [`Response`] — its rect to lay something beside, its focus, its `changed()`. This is the shape
/// [`RowPress`](super::RowPress) and
/// [`LongPressResponse`](super::LongPressResponse) already use for "the response, plus what
/// happened".
#[derive(Debug)]
pub struct SegmentedPick {
    /// The strip's response. In the stacked form it is the union of the rows' responses, so its
    /// rect covers the whole stack.
    pub response: Response,
    /// The index the user just picked, and only then — `None` on every other frame, and `None`
    /// for a tap on the segment that was already selected. The same contract
    /// `layout::choice_rows` reports a pick with, so a screen can swap one for the other without
    /// touching its handler.
    pub picked: Option<usize>,
}

/// The room the strip leaves round itself for its focus ring (see `SegmentedControl::strip`).
fn ring_margin(cx: &Cx<'_>) -> f32 {
    cx.theme.control.focus_gap + cx.theme.control.stroke_mark
}

/// The strip's resolved geometry, gathered once so the paint helpers stay under the argument
/// limit and so no length is resolved twice in a frame.
#[derive(Debug, Clone, Copy)]
struct Strip {
    /// The slot inset by `mark_radius` on all four sides — the band the segments live in.
    inner: Rect,
    /// One segment's width. Every segment is equal; see the module docs.
    cell: f32,
    /// How many segments.
    n: usize,
    /// A segment's corner radius, `control_radius − mark_radius`. Because the radius ladder
    /// halves, that equals `mark_radius` at every scale, so a segment's curve is concentric with
    /// the strip's with nobody computing anything.
    radius: CornerRadius,
    /// How far a label is kept from its cell's edges (`control.gap`).
    pad: f32,
}

impl Strip {
    /// Resolve the band and the cell from the allocated slot.
    fn new(rect: Rect, cell: f32, n: usize, cx: &Cx<'_>) -> Self {
        let mr = cx.theme.control.mark_radius;
        Self {
            inner: rect.shrink(mr),
            cell,
            n,
            radius: CornerRadius::same(round_u8(cx.theme.metrics.control_radius - mr)),
            pad: cx.theme.control.gap,
        }
    }

    /// The rect of the segment at `at`, which may be fractional — that is how the selected face
    /// travels between two cells without a second geometry path.
    fn cell_rect(&self, at: f32) -> Rect {
        Rect::from_min_size(
            egui::pos2(self.inner.min.x + self.cell * at, self.inner.min.y),
            Vec2::new(self.cell, self.inner.height()),
        )
    }

    /// Which segment a pointer x lands in.
    fn index_at(&self, x: f32) -> usize {
        let k = ((x - self.inner.min.x) / self.cell).floor().max(0.0) as usize;
        k.min(self.n.saturating_sub(1))
    }

    /// The hairlines between segments.
    ///
    /// A divider only earns its place between two *unselected* segments: beside the filled face
    /// the fill is already the boundary, and a line there would draw a seam down the middle of a
    /// shape that is meant to read as one solid. It fades with the face's coverage rather than
    /// switching off, so nothing blinks while the face travels past it.
    fn paint_dividers(&self, painter: &egui::Painter, cx: &Cx<'_>, t_sel: f32, color: Color32) {
        let half = self.inner.height() / 2.0 - cx.theme.control.mark_radius;
        let mid = self.inner.center().y;
        for i in 1..self.n {
            let fade = 1.0 - coverage(t_sel, i - 1).max(coverage(t_sel, i));
            if fade <= 0.0 {
                continue;
            }
            painter.vline(
                self.inner.min.x + self.cell * i as f32,
                mid - half..=mid + half,
                Stroke::new(cx.theme.control.stroke_hairline, color.gamma_multiply(fade)),
            );
        }
    }

    /// Paint labels with colours interpolated by the selected face's cell coverage.
    /// The strip measurement rejects labels that do not fit before this paint path.
    fn paint_labels(
        &self,
        painter: &egui::Painter,
        labels: &[&str],
        font: &FontId,
        t_sel: f32,
        ink: Ink,
    ) {
        for (i, label) in labels.iter().enumerate() {
            let cell = self.cell_rect(i as f32);
            let mut job = egui::text::LayoutJob::simple_singleline(
                (*label).to_owned(),
                font.clone(),
                Color32::PLACEHOLDER,
            );
            // The `max(1.0)` is epaint's degenerate-width guard, not a length token — the same
            // floor `ListRow`'s own elision uses; a wrap width of zero lays out nothing at all.
            job.wrap = egui::text::TextWrapping::truncate_at_width(
                (cell.width() - 2.0 * self.pad).max(1.0),
            );
            let galley = painter.layout_job(job);
            let color = ink.label.lerp_to_gamma(ink.on_face, coverage(t_sel, i));
            painter.galley(cell.center() - galley.size() / 2.0, galley, color);
        }
    }
}

/// How much of segment `i` the travelling face covers, 0..=1. It is 1 when the face is parked on
/// `i` and falls linearly to 0 one cell away, which is exactly the split of the face across the
/// two cells it straddles.
fn coverage(t_sel: f32, i: usize) -> f32 {
    (1.0 - (t_sel - i as f32).abs()).clamp(0.0, 1.0)
}

/// The colours one frame paints with, so the disabled fade is **one** call on **one** value
/// rather than a `gamma_multiply` sprinkled down the paint path (one mechanism, one number).
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// The strip's ground.
    strip_face: Color32,
    /// The strip's identifying boundary.
    strip_edge: Color32,
    /// The travelling selected face.
    face: Color32,
    /// An unselected label.
    label: Color32,
    /// A label standing on the selected face.
    on_face: Color32,
    /// The hairline between two unselected segments.
    divider: Color32,
    /// The press tint, laid over whatever the pressed segment already has.
    tint: Color32,
    /// The focus ring.
    ring: Color32,
}

impl Ink {
    /// The colours for this frame, dimmed once at the end when the control is disabled.
    fn of(cx: &Cx<'_>, enabled: bool) -> Self {
        let ink = Self {
            // One step back from the card, not the card's own colour - see `paint_strip`.
            strip_face: cx.theme.color(ColorRole::Surface),
            // Use ControlEdge for the strip boundary and Outline for internal dividers.
            strip_edge: cx.theme.color(ColorRole::ControlEdge),
            face: cx.theme.color(ColorRole::Primary),
            label: cx.theme.color(ColorRole::Muted),
            on_face: cx.theme.color(ColorRole::OnPrimary),
            divider: cx.theme.color(ColorRole::Outline),
            tint: cx.theme.color(ColorRole::Pressed),
            ring: cx.theme.color(ColorRole::Focus),
        };
        if enabled {
            return ink;
        }
        let a = cx.theme.control.disabled_alpha;
        Self {
            strip_face: ink.strip_face.gamma_multiply(a),
            strip_edge: ink.strip_edge.gamma_multiply(a),
            face: ink.face.gamma_multiply(a),
            label: ink.label.gamma_multiply(a),
            on_face: ink.on_face.gamma_multiply(a),
            divider: ink.divider.gamma_multiply(a),
            tint: ink.tint.gamma_multiply(a),
            ring: ink.ring.gamma_multiply(a),
        }
    }
}

/// Paint the strip ground and its hairline boundary using the resolved theme roles.
fn paint_strip(painter: &egui::Painter, cx: &Cx<'_>, rect: Rect, ink: Ink) {
    let radius = CornerRadius::same(round_u8(cx.theme.metrics.control_radius));
    painter.rect_filled(rect, radius, ink.strip_face);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(cx.theme.control.stroke_hairline, ink.strip_edge),
        // Inside, so the edge is part of the silhouette and the strip never measures wider than
        // the width it allocated.
        StrokeKind::Inside,
    );
}

/// Paint the focus outline outside the control's painted boundary.
/// Check the focus colour against the surrounding background for custom palettes.
fn paint_focus_ring(painter: &egui::Painter, cx: &Cx<'_>, rect: Rect, color: Color32) {
    let width = cx.theme.control.stroke_mark;
    let out = cx.theme.control.focus_gap + width * 0.5;
    painter.rect_stroke(
        rect.expand(out),
        CornerRadius::same(round_u8(cx.theme.metrics.control_radius + out)),
        Stroke::new(width, color),
        // Middle: the stroke straddles the expanded rect, so its inner face sits exactly
        // `focus_gap` clear of the strip.
        StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::{coverage, Strip};
    use egui::{pos2, CornerRadius, Rect};

    /// A strip of four 20-wide cells starting at x = 100.
    fn strip() -> Strip {
        Strip {
            inner: Rect::from_min_max(pos2(100.0, 0.0), pos2(180.0, 40.0)),
            cell: 20.0,
            n: 4,
            radius: CornerRadius::same(2),
            pad: 3.0,
        }
    }

    /// The coverage split is what both the label colour and the divider fade rest on: it sums to
    /// 1 across the two cells the face straddles, and it is 0 everywhere else.
    #[test]
    fn coverage_splits_between_the_two_cells_the_face_straddles() {
        assert!((coverage(1.0, 1) - 1.0).abs() < 1e-6);
        assert!((coverage(1.25, 1) + coverage(1.25, 2) - 1.0).abs() < 1e-6);
        assert!(coverage(1.25, 3).abs() < 1e-6);
        assert!(coverage(0.5, 0) > 0.0 && coverage(0.5, 1) > 0.0);
    }

    /// Every point of the band hits a cell, and the ends do not fall off it — the pointer is
    /// clamped rather than reported as "no segment", because a tap inside the slot is always a tap
    /// on one of the options.
    #[test]
    fn every_x_in_the_band_lands_on_a_cell() {
        let s = strip();
        assert_eq!(s.index_at(100.0), 0);
        assert_eq!(s.index_at(119.9), 0);
        assert_eq!(s.index_at(120.1), 1);
        assert_eq!(s.index_at(179.9), 3);
        // Past both ends, clamped.
        assert_eq!(s.index_at(-5.0), 0);
        assert_eq!(s.index_at(9_999.0), 3);
    }

    /// The cells tile the band exactly: cell 0 starts at its left edge and the last one ends at
    /// its right, so the travelling face never overhangs the strip's own rounded corner.
    #[test]
    fn the_cells_tile_the_band() {
        let s = strip();
        assert!((s.cell_rect(0.0).min.x - s.inner.min.x).abs() < 1e-4);
        assert!((s.cell_rect(3.0).max.x - s.inner.max.x).abs() < 1e-4);
        // A fractional position sits between the two it straddles.
        let mid = s.cell_rect(1.5);
        assert!(mid.min.x > s.cell_rect(1.0).min.x && mid.min.x < s.cell_rect(2.0).min.x);
        assert!((mid.width() - s.cell).abs() < 1e-4);
    }
}
