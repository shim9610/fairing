//! `Radio` — one of a set, drawn as a ring that **never fills**.
//!
//! # Why a radio exists at all, when a list of choice rows already works
//!
//! It is the narrow case, and the crate should say so out loud. A single choice among more than
//! three options belongs in a full-width list of [`ListRow`](super::ListRow) choice rows carrying a
//! trailing `check` — that reads at a glance, each option gets a whole row to press, and it already
//! scrolls. The radio is for the **inline** form: two or three options that have to sit inside a
//! card beside other settings, where a full-width list of rows would weigh more than the setting is
//! worth. Anything longer than that, and [`RadioGroup`] is the wrong tool.
//!
//! # Why the ring never fills
//!
//! [`Checkbox`](super::Checkbox) and [`Radio`] are deliberately the same diameter
//! (`control.mark_size`), so the only thing separating them is **square versus circle**. That is
//! what lets a user tell "any of these" from "one of these" without reading a label, at a glance,
//! and with the colour removed. A radio that filled its body on selection would be a checkbox with
//! round corners: the shape cue would still be there, but nobody would be looking for it, because
//! the state cue — the fill — would have become the checkbox's. So selection is carried by **the
//! mark alone**: the dot appears *and* the ring goes `Muted` at `stroke_edge` → `Primary` at
//! `stroke_mark`. Two carriers, both of which survive greyscale, and neither of which is a fill.
//!
//! # Why the value is a `bool` by value and not a `&mut bool`
//!
//! [`Switch`](super::Switch) and [`Checkbox`](super::Checkbox) take `&mut bool` because they own
//! their bit: a tap flips it and nothing else on the screen has an opinion. A radio's bit is not
//! its own — it is one cell of an exclusive set, and a radio that could write `false` into its own
//! cell would leave the set with **nothing** selected, which is a state the set is not allowed to
//! have. So [`Radio`] reads `selected: bool`, reports a tap through `Response::clicked()`, and the
//! *caller* assigns the index. That is the same split `egui::Ui::radio_value` makes, for the same
//! reason.
//!
//! [`RadioGroup`] is the form that owns the bookkeeping: it holds the `&mut usize` and is the only
//! thing here that may write it, so the "exactly one" invariant lives in exactly one place. Both
//! are public because they answer different questions — [`Radio`] is the control (and the piece a
//! row composes), [`RadioGroup`] is the set — and shipping only the group would leave a screen that
//! lays itself out no way to draw a radio at all.
//!
//! # How `fairing::layout::choice_rows` should be rebuilt on this
//!
//! `choice_rows` today builds the right thing for the wrong reason: a stack of `ListRow`s with the
//! chevron off and a trailing `check` icon on the selected one. That shape stays right for a
//! full-width card list — the trailing tick is the carrier, and a leading radio there would be a
//! *second* carrier for one bit. What `choice_rows` should take from this file is the **contract**,
//! not the drawing:
//!
//! - keep `choice_rows(ui, cx, options, selected) -> Option<usize>` as it stands, and keep the
//!   `ListRow` plus trailing `check`; the tick moves to `control.icon` in `Primary`, and the
//!   `strong(true)` title and `Primary` icon tint that `list_item` stacks on top of it go away.
//! - add a sibling, `inline_choice(ui, cx, labels, &mut index)`, whose body is one line —
//!   `RadioGroup::new(&mut index, labels).show(ui, &mut cx.widgets())` — for the two- and
//!   three-option settings that already sit inside a card (the theme picker in `settings::screens`,
//!   say) and do not deserve a screen of rows.
//! - both then report a change the same way, `Option<usize>`, so a screen can move a setting from
//!   one form to the other without touching its own state.
//!
//! A `ListRow` with a **leading** radio is the third form, and it is built by handing
//! [`Radio::draw`] the row's leading column: the row owns the hit rect and the radio is drawn only
//! (rule 1.3).

use super::RadioLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// A radio button: one of a set.
///
/// It reads `selected` and reports a tap; it never writes. See the module doc for why that is not
/// the `&mut bool` [`Checkbox`](super::Checkbox) takes.
#[derive(Debug, Clone, Copy)]
pub struct Radio {
    selected: bool,
    enabled: bool,
}

impl Radio {
    /// A radio showing `selected`.
    #[must_use]
    pub fn new(selected: bool) -> Self {
        Self {
            selected,
            enabled: true,
        }
    }

    /// Enabled.
    ///
    /// Disabled, it senses `hover()` rather than `click()`, so the tap **falls through** to
    /// whatever is beneath it — a disabled control that swallows the press makes the row it sits in
    /// feel broken rather than unavailable (rule 1.4).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it standing on its own, allocating a full touch target.
    ///
    /// A tap is `response.clicked()`. There is deliberately no `changed()`: the radio holds no
    /// value to change, and reporting one would invite a caller to read a tap on an
    /// already-selected radio as a deselection.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let ms = cx.theme.control.box_size();
        // Rule 1.1: the slot is a full target on **both** axes; `max` so a device that sets a mark
        // larger than its target still gets a slot that holds the drawing.
        let side = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control).max(ms);
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), sense);
        let held = self.enabled && response.is_pointer_button_down_on();
        // The press runs on its own clock (80 ms out, 120 ms back) so a tap part-way through the
        // selection crossfade does not restart it.
        let t_press = cx.animate(
            response.id.with("press"),
            if held { 1.0 } else { 0.0 },
            if held {
                cx.theme.motion.press
            } else {
                cx.theme.motion.press_release
            },
        );
        // Rule 1.5: the growth is on the **painted** rect only. `rect` is already allocated and
        // does not move, so a pressed row never reflows.
        let press = Press {
            held,
            t: t_press,
            grow: cx.theme.control.press_grow * t_press,
            focused: response.has_focus(),
        };
        self.paint(ui.painter(), cx, response.id, rect, press);
        response
    }

    /// Draw it **into a rect somebody else owns**, sensing nothing (rule 1.3).
    ///
    /// This is the composition path: a [`ListRow`](super::ListRow) carrying a leading radio, or one
    /// [`RadioGroup`] row, is a single target of the whole row, so the radio must not allocate, must
    /// not sense, and must not grow under a press — a full-bleed row tints instead, and a list whose
    /// marks grow wobbles. It draws no focus ring either: focus belongs to the row.
    ///
    /// `id` is the row's, so the selection crossfade is remembered per row rather than restarted
    /// every frame. The mark is centred in `rect` on both axes at `control.mark_size`, whatever size
    /// `rect` is.
    pub fn draw(self, painter: &egui::Painter, cx: &mut Cx<'_>, id: Id, rect: Rect) {
        self.paint(painter, cx, id, rect, Press::default());
    }

    /// The shared paint path: the selection's crossfade, then a painter's drawing where one is
    /// given or the built-in one. The built-in focus ring hangs off the radius actually
    /// painted rather than the resting one: `press_grow` is larger than `focus_gap`, so measured
    /// from the resting radius a pressed radio would grow out through its own ring.
    fn paint(self, painter: &egui::Painter, cx: &mut Cx<'_>, id: Id, rect: Rect, press: Press) {
        let t_on = cx.animate(
            id.with("fairing.radio.on"),
            if self.selected { 1.0 } else { 0.0 },
            cx.theme.motion.crossfade,
        );
        let drawn = geometry(
            cx.theme.control.box_size(),
            cx.theme.control.dot_ratio,
            cx.theme.control.stroke_edge,
            cx.theme.control.stroke_mark,
            press.grow,
            t_on,
        );
        let center = rect.center();
        if let Some(paint) = cx.painters.as_deref_mut().and_then(|p| p.radio.as_mut()) {
            paint(
                painter,
                &mut RadioLook {
                    rect,
                    drawn: Rect::from_center_size(center, Vec2::splat(drawn.radius * 2.0)),
                    selected: self.selected,
                    mark: t_on,
                    pressed: press.held,
                    press: press.t,
                    enabled: self.enabled,
                    focused: press.focused,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return;
        }
        // The face stays `SurfaceVariant` in **both** states (rule 7.1: the ring never fills), so
        // the press tint has exactly one colour to land on.
        let mut face = cx.theme.color(ColorRole::SurfaceVariant);
        if press.held {
            // `face.blend(tint)`, not `tint.blend(face)`: `blend`'s **receiver is the layer
            // behind**, so the other order multiplies the tint by `255 - 255` over an opaque face
            // and paints nothing at all.
            face = face.blend(cx.theme.color(ColorRole::Pressed));
        }
        let edge = cx
            .theme
            .color(ColorRole::ControlEdge)
            .lerp_to_gamma(cx.theme.color(ColorRole::Primary), t_on);
        painter.circle(
            center,
            // `epaint` strokes a circle **outside** the path it is given — `tessellate_circle`
            // forces `PathStroke::from(stroke).outside()` (epaint 0.36.1 tessellator.rs:1531), so
            // the stroke occupies `[r, r + width]` and the fill reaches exactly `r`. To keep the
            // OUTER extent at the token, the path is pulled in by the **whole** width, not half of
            // it. Half leaves the silhouette oversized — visibly, it broke out of the track and
            // past the card's edge. (`CircleShape::visual_bounding_rect` reports `2r + width`, as
            // if the stroke were centred; the tessellator disagrees with it and the tessellator is
            // what paints.)
            (drawn.radius - drawn.edge).max(0.0),
            self.fade(cx, face),
            Stroke::new(drawn.edge, self.fade(cx, edge)),
        );
        if drawn.dot > 0.0 {
            let dot = self.fade(cx, cx.theme.color(ColorRole::Primary));
            painter.circle_filled(center, drawn.dot, dot);
        }
        if press.focused {
            let ring = self.fade(cx, cx.theme.color(ColorRole::Focus));
            paint_focus_ring(painter, cx, center, drawn.radius, ring);
        }
    }

    /// Disabled is **one** `gamma_multiply` over every colour the control paints, at one call
    /// site. Not a role swap, which loses the live state — a disabled *selected* radio must still
    /// read as selected — and not a per-element alpha, which is the four-mechanism, seven-number
    /// mess this replaces.
    fn fade(self, cx: &Cx<'_>, color: Color32) -> Color32 {
        if self.enabled {
            color
        } else {
            color.gamma_multiply(cx.theme.control.disabled_alpha)
        }
    }
}

/// How hard the control is being pressed this frame.
///
/// The two travel together and are meaningless apart — the tint without the growth is a treatment
/// that measured 1.07–1.16 over `Primary`, and the growth without the tint is what
/// [`BigButton`](super::BigButton) shipped — so they are one argument rather than two.
#[derive(Debug, Clone, Copy, Default)]
struct Press {
    /// The finger is down on it now: the face takes the press tint.
    held: bool,
    /// The press tween, 0 to 1.
    t: f32,
    /// The tweened growth in du, which outlives `held` by the 120 ms release.
    grow: f32,
    /// It has the keyboard focus — never, drawn into a group's row.
    focused: bool,
}

/// What one radio draws this frame, in du.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    /// The ring's outer radius, press growth included.
    radius: f32,
    /// The ring's stroke width: `stroke_edge` unselected, `stroke_mark` selected.
    edge: f32,
    /// The dot's radius. Zero while unselected — the dot *appears*; it does not fade in at full
    /// size, because a fade is a colour change and selection needs a carrier that survives
    /// greyscale.
    dot: f32,
}

/// The geometry for one frame.
///
/// Free-standing, and taking plain numbers rather than the theme, so the two greyscale-proof
/// carriers selection depends on — the ring thickens, the dot appears — can be asserted without a
/// palette or a `Ui`.
/// `t_on` is 0 unselected and 1 selected; `grow` is the press growth, already tweened.
fn geometry(
    mark_size: f32,
    dot_ratio: f32,
    stroke_edge: f32,
    stroke_mark: f32,
    grow: f32,
    t_on: f32,
) -> Geometry {
    let radius = mark_size * 0.5 + grow;
    Geometry {
        radius,
        edge: stroke_edge + (stroke_mark - stroke_edge) * t_on,
        // A fraction of the **drawn** radius rather than of `mark_size`, so a press scales the whole
        // control instead of leaving the dot at its resting size while the ring moves out from
        // around it.
        dot: radius * dot_ratio * t_on,
    }
}

/// The focus ring, **outside** the silhouette.
///
/// Outside is a contrast decision, not a styling one: `Focus` measured against `Primary` is
/// 1.00–1.62 in the shipped palettes, so a ring laid on the control's own edge is invisible on a
/// selected radio in every one of them, while against the container behind it the same colour clears
/// 3.0 everywhere. It also puts the ring outside the press growth, and outside the finger.
fn paint_focus_ring(
    painter: &egui::Painter,
    cx: &Cx<'_>,
    center: Pos2,
    silhouette: f32,
    color: Color32,
) {
    let width = cx.theme.control.stroke_mark;
    // `+ width / 2` because the stroke straddles its path: the ring's inner face then sits exactly
    // `focus_gap` clear of the silhouette.
    let radius = silhouette + cx.theme.control.focus_gap + width * 0.5;
    painter.circle_stroke(center, radius, Stroke::new(width, color));
}

/// An exclusive choice among **two or three** options, laid out inline as a stack of rows.
///
/// It owns the `&mut usize` and is the only thing here that writes it, so "exactly one is selected"
/// is enforced in one place. Each row is one target of the full width (rule 1.3) and the radio drawn
/// inside it senses nothing.
///
/// # Why the options are bare labels
///
/// No subtitles, no icons, no per-option enable. The moment a choice needs a second line to explain
/// itself it has outgrown the inline form and belongs in a list of [`ListRow`](super::ListRow)
/// choice rows, where a subtitle has room and each option gets a whole row. Keeping the option type
/// as `&str` makes that boundary something the compiler shows you rather than something a doc asks
/// you to remember.
#[derive(Debug)]
pub struct RadioGroup<'a> {
    selected: &'a mut usize,
    options: &'a [&'a str],
    enabled: bool,
}

impl<'a> RadioGroup<'a> {
    /// A group over `options`, editing `selected`.
    ///
    /// A `selected` outside the options selects nothing, which is what an out-of-range index already
    /// means. It is deliberately not clamped: silently moving a caller's index would hide the bug
    /// that produced it, and the group has no way to tell a stale index from an intended one.
    #[must_use]
    pub fn new(selected: &'a mut usize, options: &'a [&'a str]) -> Self {
        Self {
            selected,
            options,
            enabled: true,
        }
    }

    /// Enabled. Disabled, the whole group fades by `control.disabled_alpha` and every row senses
    /// `hover()` so the taps fall through (rule 1.4).
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. Returns the index just chosen, on the frame it changed, and writes it through — the
    /// same shape `fairing::layout::choice_rows` already reports, so a screen can move a setting
    /// between the two forms without touching its own state.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Option<usize> {
        let m = &cx.theme.metrics;
        let (height, inset) = (m.row_height.max(m.touch_target), m.content_inset);
        let (ms, gap) = (cx.theme.control.box_size(), cx.theme.control.gap);
        let font = egui::TextStyle::Body.resolve(ui.style());
        let label_ink = self.label_color(cx);
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let was = *self.selected;
        let mut picked = None;
        for (index, &text) in self.options.iter().enumerate() {
            let size = Vec2::new(ui.available_width(), height);
            let (rect, response) = ui.allocate_exact_size(size, sense);
            if self.enabled && response.is_pointer_button_down_on() {
                // A full-bleed row tints over its whole rect and does not grow. The tint
                // already reaches far past the contact patch, so growth would add nothing visible,
                // and a list whose rows change size wobbles.
                ui.painter()
                    .rect_filled(rect, 0.0, cx.theme.color(ColorRole::Pressed));
            }
            let column = Rect::from_center_size(
                Pos2::new(rect.min.x + inset + ms * 0.5, rect.center().y),
                Vec2::splat(ms),
            );
            Radio::new(index == was).enabled(self.enabled).draw(
                ui.painter(),
                cx,
                response.id,
                column,
            );
            // The text starts at `content_inset + mark_size + gap`, the same column a
            // `ListRow` with a leading element uses, so a group sitting between rows in a card lines
            // up with them instead of forming a third left edge.
            let left = rect.min.x + inset + ms + gap;
            let max_w = (rect.max.x - inset - left).max(0.0);
            draw_label(
                ui.painter(),
                Pos2::new(left, rect.center().y),
                text,
                &font,
                max_w,
                label_ink,
            );
            if response.has_focus() {
                paint_row_focus(ui.painter(), cx, rect);
            }
            if self.enabled && response.clicked() && index != was {
                picked = Some(index);
            }
        }
        if let Some(index) = picked {
            *self.selected = index;
        }
        picked
    }

    /// The label colour: `OnSurface`, through the one disabled multiply. Split out so `show`
    /// reads as layout rather than as colour arithmetic.
    fn label_color(&self, cx: &Cx<'_>) -> Color32 {
        let color = cx.theme.color(ColorRole::OnSurface);
        if self.enabled {
            color
        } else {
            color.gamma_multiply(cx.theme.control.disabled_alpha)
        }
    }
}

/// One left-aligned line, vertically centred on `center_left.y` and elided with `…` past
/// `max_width`.
///
/// Elided rather than wrapped: an inline group's row is one target of a fixed height, and a label
/// that wrapped would either overflow the row it is the hit rect for or force the row to grow, which
/// breaks the rhythm of the card it sits in.
fn draw_label(
    painter: &egui::Painter,
    center_left: Pos2,
    text: &str,
    font: &egui::FontId,
    max_width: f32,
    color: Color32,
) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        font.clone(),
        Color32::PLACEHOLDER,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(1.0));
    let galley = painter.layout_job(job);
    let at = Pos2::new(center_left.x, center_left.y - galley.size().y * 0.5);
    painter.galley(at, galley, color);
}

/// A group row's focus ring, drawn **inside** the row band rather than outside it.
///
/// A row is full-bleed: there is no room outside it for the focus ring's clearance without running
/// into the rows above and below, so the ring is inset by its own width instead. The contrast
/// argument still holds, because the ring's neighbour is the card's ground either way — a row has
/// no face of its own until it is pressed.
fn paint_row_focus(painter: &egui::Painter, cx: &Cx<'_>, rect: Rect) {
    let width = cx.theme.control.stroke_mark;
    painter.rect_stroke(
        rect.shrink(width),
        CornerRadius::same(round_u8(cx.theme.metrics.control_radius)),
        Stroke::new(width, cx.theme.color(ColorRole::Focus)),
        StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::geometry;

    /// The REF column of the token table: `mark_size` 49.13, `dot_ratio` 0.50,
    /// `stroke_edge` 2.52, `stroke_mark` 3.78, `press_grow` 4.91.
    const REF: (f32, f32, f32, f32) = (49.13, 0.50, 2.52, 3.78);

    /// Selecting a radio must move something that survives greyscale. Two things do — the dot
    /// appears and the ring thickens — and both are arithmetic, so both are asserted here rather
    /// than in a render test.
    #[test]
    fn selection_is_carried_by_the_dot_and_the_ring_thickness() {
        let (ms, dot, se, sm) = REF;
        let off = geometry(ms, dot, se, sm, 0.0, 0.0);
        let on = geometry(ms, dot, se, sm, 0.0, 1.0);
        assert!(off.dot.abs() < 1e-6, "the dot is drawn while unselected");
        assert!(on.dot > 0.0, "the dot does not appear");
        assert!(on.edge > off.edge, "the ring does not thicken");
        assert!(
            (off.edge - se).abs() < 1e-4,
            "the resting ring is not `stroke_edge`"
        );
        assert!(
            (on.edge - sm).abs() < 1e-4,
            "the selected ring is not `stroke_mark`"
        );
        // Rule 7.1: the ring never fills — the dot stays clear of the stroke's inner face.
        assert!(on.dot + on.edge < on.radius, "the dot has reached the ring");
    }

    /// Rule 1.5: a press only ever makes the painted silhouette bigger, by `press_grow`
    /// on every side, and the dot goes with it instead of being left at its resting size.
    #[test]
    fn the_press_grows_the_whole_control() {
        let (ms, dot, se, sm) = REF;
        let idle = geometry(ms, dot, se, sm, 0.0, 1.0);
        let held = geometry(ms, dot, se, sm, 4.91, 1.0);
        assert!(held.radius > idle.radius, "the press did not grow the ring");
        assert!(held.dot > idle.dot, "the dot was left behind");
        assert!(
            (held.radius - idle.radius - 4.91).abs() < 1e-4,
            "the growth is not `press_grow` on every side"
        );
    }
}
