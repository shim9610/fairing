//! `BigButton` is a text button with an optional icon and long-press interaction.
//!
//! Button kinds select ordinary, primary or destructive styling from theme roles.
//! Press feedback changes the painted shape without moving the allocated rectangle.
//! Long-press feedback is painted around the button; focus is painted outside it.
//! The caller supplies the label, enabled state and action handling.

use super::{ButtonLook, HoldLook};
use crate::cx::WidgetCx as Cx;
use crate::icons::{IconColor, IconRef, IconStyle};
use crate::interaction::concrete::HoldPhase;
use crate::motion::{Easing, Tween};
use crate::theme::ColorRole;
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use std::sync::Arc;
use std::time::Duration;

// Completion feedback uses `motion.press` for the pop and `motion.press_release`
// for the cancel reverse. The hold duration remains the caller's supplied duration.
/// Theme styling for ordinary, primary and destructive buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ButtonKind {
    /// Ordinary button: `SurfaceVariant` face, `Muted` edge and `OnSurface` label.
    ///
    /// The edge distinguishes the button when its face matches its container.
    #[default]
    Normal,
    /// Filled kind: `Primary` face, no separate edge, and `OnPrimary` label.
    Primary,
    /// Destructive-action styling: `SurfaceVariant` face and `Danger` edge/label.
    /// Use [`BigButton::long_press`] when the application requires a hold interaction.
    Danger,
}

impl ButtonKind {
    /// The three roles this kind paints with.
    pub(crate) const fn dress(self) -> Dress {
        match self {
            Self::Normal => Dress {
                face: ColorRole::OnSurface,
                translucent: true,
                edge: Some(ColorRole::ControlEdge),
                label: ColorRole::OnSurface,
            },
            Self::Primary => Dress {
                face: ColorRole::Primary,
                translucent: false,
                edge: None,
                label: ColorRole::OnPrimary,
            },
            Self::Danger => Dress {
                face: ColorRole::OnSurface,
                translucent: true,
                edge: Some(ColorRole::Danger),
                label: ColorRole::Danger,
            },
        }
    }
}

/// How one kind is dressed: the face, the identifying edge (`None` where the fill is its own
/// boundary), and the label. Roles rather than colours, so the disabled fade can be applied once
/// to the resolved set instead of at three paint sites.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Dress {
    /// The filled body.
    pub(crate) face: ColorRole,
    /// Whether the face is laid at `control.fill_alpha` rather than opaque.
    pub(crate) translucent: bool,
    /// The boundary, where the face is not one.
    pub(crate) edge: Option<ColorRole>,
    /// The label and the icon.
    pub(crate) label: ColorRole,
}

/// The colours one frame paints with, faded **once** when the button is disabled.
///
/// One `gamma_multiply(control.disabled_alpha)` over the whole set replaces the four alphas this
/// widget used to carry (0.5 face, 0.6 label, 0.5 outline, and the icon painter's own 0.6 over a
/// `Muted` role swap). A role swap is forbidden as well as redundant: it throws away the kind, so
/// a disabled `Danger` would stop reading as destructive while still being the thing you cannot
/// press.
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// The filled body, press tint included.
    face: Color32,
    /// The identifying boundary.
    edge: Option<Color32>,
    /// The label and the icon.
    label: Color32,
    /// The focus ring.
    focus: Color32,
}

impl Ink {
    /// The colours for this frame.
    fn of(cx: &Cx<'_>, kind: ButtonKind, pressed: bool, enabled: bool) -> Self {
        let dress = kind.dress();
        let mut face = cx.theme.color(dress.face);
        if dress.translucent {
            face = face.gamma_multiply(cx.theme.control.fill_alpha);
        }
        if pressed {
            // `face.blend(tint)`, not `tint.blend(face)` — the receiver is the layer **behind**.
            face = face.blend(cx.theme.color(ColorRole::Pressed));
        }
        let ink = Self {
            face,
            edge: dress.edge.map(|role| cx.theme.color(role)),
            label: cx.theme.color(dress.label),
            focus: cx.theme.color(ColorRole::Focus),
        };
        if enabled {
            return ink;
        }
        let a = cx.theme.control.disabled_alpha;
        Self {
            face: ink.face.gamma_multiply(a),
            edge: ink.edge.map(|c| c.gamma_multiply(a)),
            label: ink.label.gamma_multiply(a),
            focus: ink.focus.gamma_multiply(a),
        }
    }
}

/// The lengths one frame draws with, read from the theme once.
///
/// They are gathered up front rather than read at each paint site because the button's geometry is
/// arithmetic on six of them at once (width, the content lump, the ring's reserve), and reading
/// `cx.theme.…` inside that arithmetic is how a stray literal gets in unnoticed.
#[derive(Debug, Clone, Copy)]
struct Tokens {
    /// `metrics.content_inset` — the side padding.
    inset: f32,
    /// `control.gap` — icon to label, and the ring's clearance from the corner.
    gap: f32,
    /// `control.icon` — the leading icon, and the long-press ring's diameter.
    icon: f32,
    /// `metrics.control_radius` — the control rung, not the container rung the button used to wear.
    radius: f32,
    /// `control.stroke_edge` — the identifying boundary.
    stroke_edge: f32,
    /// `control.stroke_mark` — the ring and the focus ring.
    stroke_mark: f32,
    /// `control.focus_gap` — how far the focus ring clears the silhouette.
    focus_gap: f32,
    /// `control.press_grow` — how far the painted silhouette grows on every side while held.
    press_grow: f32,
    /// `control.min_button_w` — the floor on the width.
    min_width: f32,
    /// `max(row_height, touch_target)` — the height, and never below the target (rule 1.1).
    height: f32,
}

impl Tokens {
    /// Read them.
    fn read(cx: &Cx<'_>) -> Self {
        let m = &cx.theme.metrics;
        let c = &cx.theme.control;
        Self {
            inset: m.content_inset,
            gap: c.gap,
            icon: c.icon,
            radius: m.control_radius,
            stroke_edge: c.stroke_edge,
            stroke_mark: c.stroke_mark,
            focus_gap: c.focus_gap,
            press_grow: c.press_grow,
            min_width: c.min_button_w,
            height: m.row_height.max(m.touch_target),
        }
    }
}

/// The result of a long-press button.
#[derive(Debug)]
pub struct LongPressResponse {
    /// The underlying response.
    pub response: Response,
    /// The long press completed on this frame.
    pub completed: bool,
    /// The ring's progress, 0..=1.
    pub progress: f32,
}

impl LongPressResponse {
    /// Whether the underlying response was clicked.
    /// Equivalent to `response.clicked()` for the common non-hold case.
    #[must_use]
    pub fn clicked(&self) -> bool {
        self.response.clicked()
    }

    /// Whether this response reports a completed long press.
    #[must_use]
    pub const fn long_pressed(&self) -> bool {
        self.completed
    }
}

/// A large touch button.
#[derive(Debug, Clone)]
pub struct BigButton {
    label: String,
    icon: Option<IconRef>,
    kind: ButtonKind,
    min_size: Option<Vec2>,
    text_size: Option<f32>,
    enabled: bool,
    long_press: Option<Duration>,
}

impl BigButton {
    /// The label.
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            text_size: None,
            label: label.into(),
            icon: None,
            kind: ButtonKind::Normal,
            min_size: None,
            enabled: true,
            long_press: None,
        }
    }

    /// The icon, drawn at `control.icon` before the label.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The kind.
    #[must_use]
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    /// Override label text size in du; the default is `metrics.type_scale.button`.
    #[must_use]
    pub fn text_size(mut self, du: f32) -> Self {
        self.text_size = Some(du.max(1.0));
        self
    }

    /// Raise the minimum size above the configured token floor.
    /// Values below the floor are ignored; text size is configured separately.
    #[must_use]
    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = Some(size);
        self
    }

    /// Enabled. Disabled it senses `hover()`, so the tap falls through rather than being eaten.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Make it a long-press button (the ring fills and completes after `duration`).
    #[must_use]
    pub fn long_press(mut self, duration: Duration) -> Self {
        self.long_press = Some(duration);
        self
    }

    /// Draw it.
    pub fn show(mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> LongPressResponse {
        let t = Tokens::read(cx);
        let lead = self.lead(t);
        // The ring reserves its own diameter plus a gap, so the label never runs under it.
        let ring_w = if self.long_press.is_some() {
            t.icon + t.gap
        } else {
            0.0
        };
        // **A button does not outgrow its room.** A caller's `min_size` is a wish, capped at the
        // width there is; the token floor (`min_button_w`) still holds under that, so a button in
        // a room narrower than the floor bleeds where it is obvious rather than squeezing to
        // nothing. Before the cap, a pay button asked for twelve ems in a column that had ten
        // and was drawn past the column's clip — its left third cut off.
        let room = ui.available_width();
        let asked = self.min_size.unwrap_or(Vec2::ZERO);
        let floor = Vec2::new(asked.x.min(room).max(t.min_width), asked.y.max(t.height));
        let avail = room.max(floor.x);
        let font = self.text_size.map_or_else(
            || egui::TextStyle::Button.resolve(ui.style()),
            egui::FontId::proportional,
        );
        let galley = self.layout_label(ui, font.clone(), avail - t.inset * 2.0 - lead - ring_w);
        let width = (galley.size().x + t.inset * 2.0 + lead + ring_w)
            .max(floor.x)
            .min(avail);

        let sense = if self.enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, floor.y), sense);
        let id = response.id;
        let pressed = self.enabled && response.is_pointer_button_down_on();
        let press = press_growth(cx, id, pressed);
        // Rule 1.5: the growth is on the painted rect only — `rect` is allocated and does not move.
        let drawn = rect.expand(t.press_grow * press);
        // The hold advances whoever draws it.
        let hold = match self.long_press {
            Some(duration) if self.enabled => Some(hold(ui, cx, id, &response, duration)),
            _ => None,
        };
        let focused = response.has_focus();
        if let Some(paint) = cx.painters.as_deref_mut().and_then(|p| p.button.as_mut()) {
            paint(
                ui.painter(),
                &mut ButtonLook {
                    rect,
                    drawn,
                    label: galley.text(),
                    font,
                    icon: self.icon.as_ref(),
                    kind: self.kind,
                    pressed,
                    press,
                    enabled: self.enabled,
                    focused,
                    hold: hold.map(|h| HoldLook {
                        rect: ring_rect(drawn, t, h.scale),
                        progress: h.progress,
                        done: h.done,
                    }),
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            let ink = Ink::of(cx, self.kind, pressed, self.enabled);
            paint_face(ui.painter(), drawn, t, ink);
            let right = drawn.max.x - ring_w;
            let inner = Rect::from_min_max(drawn.min, egui::pos2(right, drawn.max.y));
            self.paint_content(ui, cx, inner, galley, ink, t);
            if focused {
                paint_focus_ring(ui.painter(), drawn, t, ink.focus);
            }
            if let Some(hold) = hold {
                paint_ring(ui.painter(), drawn, hold, ink, t);
            }
        }
        LongPressResponse {
            response,
            completed: hold.is_some_and(|h| h.completed),
            progress: hold.map_or(0.0, |h| h.progress),
        }
    }

    /// The width the leading icon and the gap after it take, or zero where there is no icon.
    ///
    /// One function, read by both the measuring pass and the painting pass — they have to agree
    /// on it or the label is centred in a box it was not measured against.
    fn lead(&self, t: Tokens) -> f32 {
        if self.icon.is_some() {
            t.icon + t.gap
        } else {
            0.0
        }
    }

    /// The label in `font`, truncated with `…` at the width left over after the padding, the icon
    /// and the ring have taken theirs. The galley keeps the whole text (`Galley::text`).
    fn layout_label(
        &mut self,
        ui: &egui::Ui,
        font: egui::FontId,
        max_width: f32,
    ) -> Arc<egui::Galley> {
        let mut job = egui::text::LayoutJob::simple_singleline(
            std::mem::take(&mut self.label),
            font,
            Color32::PLACEHOLDER,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(1.0));
        ui.painter().layout_job(job)
    }

    /// The icon and the label, centred in `inner` **as one lump** so a button with an icon still
    /// reads as one object rather than as two things pushed to opposite ends.
    fn paint_content(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        inner: Rect,
        galley: Arc<egui::Galley>,
        ink: Ink,
        t: Tokens,
    ) {
        let lead = self.lead(t);
        let mut x = inner.center().x - f32::midpoint(galley.size().x, lead);
        if let Some(icon) = &self.icon {
            let at = Rect::from_center_size(
                egui::pos2(x + t.icon / 2.0, inner.center().y),
                Vec2::splat(t.icon),
            );
            // A fixed colour, not a role plus `enabled(false)`: the icon painter's disabled path
            // throws the role away and paints `Muted`, which would be a fifth disabled mechanism
            // in a widget that is meant to have exactly one.
            let style = IconStyle::sized(t.icon).color(IconColor::Fixed(ink.label));
            cx.icons.paint(ui.painter(), at, icon, &style, cx.theme);
            x += lead;
        }
        let size = galley.size();
        ui.painter().galley(
            egui::pos2(x, inner.center().y - size.y / 2.0),
            galley,
            ink.label,
        );
    }
}

/// How far into the press growth we are, 0..=1. Out over `motion.press` (80 ms), back over
/// `motion.press_release` (120 ms) — the same clock every control in the family presses on.
fn press_growth(cx: &mut Cx<'_>, id: egui::Id, pressed: bool) -> f32 {
    let motion = cx.theme.motion;
    let tween = if pressed {
        motion.press
    } else {
        motion.press_release
    };
    cx.animate(id.with("press"), if pressed { 1.0 } else { 0.0 }, tween)
}

/// The face and, for the two outlined kinds, the edge that identifies them.
fn paint_face(painter: &egui::Painter, rect: Rect, t: Tokens, ink: Ink) {
    let radius = CornerRadius::same(round_u8(t.radius));
    painter.rect_filled(rect, radius, ink.face);
    if let Some(edge) = ink.edge {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(t.stroke_edge, edge),
            // Inside, so the edge is part of the silhouette and the button never measures wider
            // than the width it allocated.
            StrokeKind::Inside,
        );
    }
}

/// Paint the focus outline outside the control's painted boundary.
/// Check the focus colour against the surrounding background for custom palettes.
fn paint_focus_ring(painter: &egui::Painter, silhouette: Rect, t: Tokens, color: Color32) {
    let out = t.focus_gap + t.stroke_mark * 0.5;
    painter.rect_stroke(
        silhouette.expand(out),
        CornerRadius::same(round_u8(t.radius + out)),
        Stroke::new(t.stroke_mark, color),
        // Middle: the stroke straddles the expanded rect, so its inner face sits exactly
        // `focus_gap` clear of the silhouette.
        StrokeKind::Middle,
    );
}

/// Where a long press has got this frame — the state the ring is drawn from, by the built-in
/// drawing or a painter.
#[derive(Debug, Clone, Copy)]
struct Hold {
    /// 0..=1, filling while held.
    progress: f32,
    /// The hold completed on this frame — reported once.
    completed: bool,
    /// The hold has completed and the finger is still down.
    done: bool,
    /// The ring's scale: the completion pop, 1 otherwise.
    scale: f32,
}

/// The long press: linear `0 → 1` while held, with an 80 ms reverse on release. On the completing
/// frame it reports `completed` once and the ring pops `0.9 → 1.1 → 1`.
///
/// The progress itself is [`Cx::animate`](crate::cx::WidgetCx::animate), but the "armed →
/// reported" step is a flag rather than an animation, so it lives in egui's store per widget id.
/// Seeding the entry at 0 on the first frame happens here too — `animate` **puts an id it has
/// not seen straight on the target**, and this stops a button that first appears
/// already pressed from completing on that frame.
fn hold(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    id: egui::Id,
    response: &Response,
    duration: Duration,
) -> Hold {
    let ring_id = id.with("ring");
    let state_id = id.with("ring.state");
    let state = crate::interaction::load::<HoldPhase>(
        ui.ctx(),
        state_id,
        ui.is_enabled(),
        crate::interaction::Lifetime::Press,
    );
    let pressed = crate::interaction::concrete::hold_admitted(
        state,
        ui.is_enabled(),
        crate::interaction::pointer(response),
    );
    if state.is_none() {
        // A button seen for the first time: the ring is planted at 0.
        cx.animate(ring_id, 0.0, Tween::instant());
        crate::interaction::store(ui.ctx(), state_id, HoldPhase::Idle);
    }
    let state = state.unwrap_or(HoldPhase::Idle);

    let tween = if pressed {
        Tween {
            duration,
            easing: Easing::Linear,
        }
    } else {
        cx.theme.motion.press_release
    };
    let progress = cx.animate(ring_id, if pressed { 1.0 } else { 0.0 }, tween);
    let full = progress >= 1.0 - 1e-4;

    let outcome = crate::interaction::concrete::hold_finish(state, pressed, full);
    let (next, completed) = (outcome.phase, outcome.completed);
    crate::interaction::store(ui.ctx(), state_id, next);
    let done = next == HoldPhase::Done;
    let mut scale = 1.0;
    if progress > 0.0 {
        let pop = cx.animate(
            id.with("ring.pop"),
            if done { 1.0 } else { 0.0 },
            if done {
                cx.theme.motion.press
            } else {
                Tween::instant()
            },
        );
        if done {
            scale = pop_scale(pop);
        }
    }
    Hold {
        progress,
        completed,
        done,
        scale,
    }
}

/// The unfilled part of the long-press ring, as a fraction of the arc's own colour. Low enough to
/// read as "not yet" and high enough to show where the ring will go.
const RING_TRACK_ALPHA: f32 = 0.30;

/// The ring's square on the button `drawn`: at the top-right corner, scaled by the pop.
fn ring_rect(drawn: Rect, t: Tokens, scale: f32) -> Rect {
    Rect::from_center_size(ring_center(drawn, t), Vec2::splat(t.icon * scale))
}

/// The built-in ring on the button `drawn`, while the hold is under way.
fn paint_ring(painter: &egui::Painter, drawn: Rect, hold: Hold, ink: Ink, t: Tokens) {
    if hold.progress <= 0.0 {
        return;
    }
    // **The ring takes the button's own label colour, not `Primary`.**
    //
    // `progress_ring` paints its track in `muted` and its arc in `color`. With `color` fixed at
    // `Primary`, the arc on a `ButtonKind::Primary` button was the same colour as the face it was
    // drawn on — not low-contrast, identical — so the one control whose whole point is to show you
    // how long you have held it showed nothing. The label already has to read on this face, by
    // construction, so taking its colour is both correct and self-maintaining: a new kind gets a
    // visible ring without anyone remembering to think about it.
    let style = crate::icons::parametric::ParamStyle {
        color: ink.label,
        muted: ink.label.gamma_multiply(RING_TRACK_ALPHA),
        danger: ink.label,
        stroke_px: t.stroke_mark,
    };
    crate::icons::parametric::progress_ring(
        painter,
        ring_rect(drawn, t, hold.scale),
        hold.progress,
        &style,
    );
}

/// Place the long-press ring at the top-right of the painted rectangle,
/// inset by `control.gap`. It follows the press-growth geometry.
fn ring_center(rect: Rect, t: Tokens) -> egui::Pos2 {
    let d = t.icon;
    rect.right_top() + egui::vec2(-(t.gap + d / 2.0), t.gap + d / 2.0)
}

/// The completion pop curve: `0.9 → 1.1` over the first half, `1.1 → 1.0` over the second (A7).
fn pop_scale(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        0.9 + (1.1 - 0.9) * (t * 2.0)
    } else {
        1.1 + (1.0 - 1.1) * ((t - 0.5) * 2.0)
    }
}

#[cfg(test)]
mod tests {
    use super::{pop_scale, ring_center, ButtonKind, Tokens};
    use crate::theme::ColorRole;
    use egui::{pos2, Rect};

    /// The tokens of a made-up panel, so the geometry helpers can be exercised without a `Theme`.
    fn tokens() -> Tokens {
        Tokens {
            inset: 27.27,
            gap: 13.63,
            icon: 34.80,
            radius: 10.24,
            stroke_edge: 2.52,
            stroke_mark: 3.78,
            focus_gap: 3.78,
            press_grow: 4.91,
            min_width: 122.83,
            height: 89.89,
        }
    }

    /// A7: `0.9 → 1.1 → 1`.
    #[test]
    fn pop_curve_overshoots_then_settles() {
        assert!((pop_scale(0.0) - 0.9).abs() < 1e-5);
        assert!((pop_scale(0.5) - 1.1).abs() < 1e-5);
        assert!((pop_scale(1.0) - 1.0).abs() < 1e-5);
        assert!(pop_scale(0.25) > 0.9 && pop_scale(0.25) < 1.1);
    }

    /// The ring clears the corner by a gap on both axes, and fits inside the width the button
    /// reserves for it (`icon + gap`) — the check that the reserve and the placement agree.
    #[test]
    fn the_ring_sits_clear_of_the_finger_in_the_top_right_corner() {
        let t = tokens();
        let rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, t.height));
        let c = ring_center(rect, t);
        assert!((rect.max.x - c.x - (t.gap + t.icon / 2.0)).abs() < 1e-3);
        assert!((c.y - rect.min.y - (t.gap + t.icon / 2.0)).abs() < 1e-3);
        assert!(c.x - t.icon / 2.0 >= rect.max.x - (t.icon + t.gap));
        assert!(c.y + t.icon / 2.0 < rect.max.y, "the ring escapes the body");
    }

    /// Exactly one kind is filled, and neither outlined kind carries `OnPrimary` — the label over
    /// a `SurfaceVariant` face has to be a role that is gated against **that** ground.
    #[test]
    fn only_primary_is_filled() {
        let filled: Vec<_> = [ButtonKind::Normal, ButtonKind::Primary, ButtonKind::Danger]
            .into_iter()
            .filter(|k| k.dress().edge.is_none())
            .collect();
        assert_eq!(filled, [ButtonKind::Primary]);
        for kind in [ButtonKind::Normal, ButtonKind::Danger] {
            assert_ne!(kind.dress().label, ColorRole::OnPrimary);
            assert_eq!(kind.dress().face, ColorRole::OnSurface);
        }
    }
}
