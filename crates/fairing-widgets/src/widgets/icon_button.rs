//! `IconButton` — a disc with a glyph and no words.
//!
//! # What it is for, and why `BigButton` could not be it
//!
//! Every reference kiosk puts one on every product card: the `+` that adds the item. It is the
//! thing that lets a card be **information** and the button be the **action** — which is the
//! difference between a menu and a wall of accent-filled tiles, and the reason the crate's own
//! kiosk had to flood a whole card with `Primary` to say "picked".
//!
//! [`BigButton`](super::BigButton) is label-led: it lays an icon beside words and sizes itself to
//! them, and there is no width at which it becomes a disc. The two share their vocabulary instead
//! — [`ButtonKind`], [`LongPressResponse`] and the whole of `Dress` — so a `Primary` disc and a
//! `Primary` bar are the same object at two sizes rather than two objects that happen to agree.
//!
//! # Why the name is a constructor argument
//!
//! An icon-only button has nothing but its drawing to say what it does. A name that is a builder
//! option is a name that can be left off, so it is not one: `IconButton::new(icon::PLUS, "Add
//! one")`. The crate does not yet speak to a screen reader, and when it does this is the string it
//! will hand over; until then it is what a tooltip and a test both reach for.
//!
//! # Why the disc is 0.68 of a fingertip
//!
//! iOS's system close button is 30 pt inside a 44 pt target — 0.682. Material 3's icon-button
//! container is 40 dp in a 48 dp target (0.833), and Fluent's whole target is 40 epx. Measured off
//! the references, the `+` disc is 0.661 of Bella Tavola's circular language button, 0.607 of its
//! primary button, 0.492 of `McDonald's` nav row and 0.385 of the Sichuan kiosk's. 0.68 is iOS's value to three
//! figures and sits inside that band, and it puts `control.icon` at 0.625 of the disc against
//! Material's 0.60.
//!
//! What is **pressed** is a whole `touch_target` either way, and larger where the focus ring would
//! otherwise be clipped. Only the drawn disc is allowed to be small.
//!
//! # Why the long-press arc is the kind's colour and not the glyph's
//!
//! [`BigButton`](super::BigButton)'s ring sits *on* the face, so it takes the label's colour — it
//! has to read against the fill. This one sits **outside** the disc, where its neighbour is the
//! page: `OnPrimary` is white, and white on base light's `#ECECF1` page measures **1.10**. An arc
//! nobody can see is worse than no arc, because the control's whole point is showing how long you
//! have held it. So the arc takes the colour that identifies the kind — `Primary`, `OnSurface`,
//! `Danger` — every one of which clears 3.0 against both grounds in all four palettes.
//!
//! Same rule ("the ring must read where it is drawn"), different answer, because it is drawn
//! somewhere else.
//!
//! # Why the ring needs no half-stroke term
//!
//! epaint 0.36.1 forces `PathStroke::outside()` on a circle, so a circle stroked at radius `r`
//! occupies `[r, r + w]`. Passing `r = R + focus_gap` therefore puts the ring's inner face exactly
//! `focus_gap` clear of the disc with no correction — where [`Checkbox`](super::Checkbox) and
//! `BigButton`, which stroke rects down the middle, must pass `focus_gap + stroke_mark / 2`.

use crate::cx::WidgetCx as Cx;
use crate::icons::IconRef;
use crate::motion::Tween;
use crate::theme::ColorRole;
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::time::Duration;

use super::button::{ButtonKind, LongPressResponse};
use super::{HoldLook, IconButtonLook};

/// An icon-only button.
#[derive(Debug, Clone)]
pub struct IconButton {
    icon: IconRef,
    /// The name of the action. Not optional — see the module doc.
    name: String,
    kind: ButtonKind,
    enabled: bool,
    long_press: Option<Duration>,
}

impl IconButton {
    /// The icon, and **the name of the action it performs** — "Add one", "Close", "Clear order".
    #[must_use]
    pub fn new(icon: IconRef, name: impl Into<String>) -> Self {
        Self {
            icon,
            name: name.into(),
            kind: ButtonKind::Normal,
            enabled: true,
            long_press: None,
        }
    }

    /// The kind — the same three [`BigButton`](super::BigButton) has, not three of its own.
    #[must_use]
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }

    /// Enabled. Disabled it senses `hover()`, so the tap falls through to the card beneath.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Hold for `duration` to fire, with an arc that fills **outside** the disc.
    ///
    /// For the irreversible action that has no room for words. An ordinary tap does not fire it.
    #[must_use]
    pub fn long_press(mut self, duration: Duration) -> Self {
        self.long_press = Some(duration);
        self
    }

    /// The name of the action, for a caller building a tooltip or a test.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The side this button allocates, without drawing it.
    #[must_use]
    pub fn measure(cx: &Cx<'_>) -> f32 {
        let c = &cx.theme.control;
        slot_side(
            crate::theme::control_height(&cx.theme.metrics, &cx.theme.control),
            c.icon_button,
            c.press_grow,
            c.focus_gap + c.stroke_mark,
        )
    }

    /// Draw it.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> LongPressResponse {
        let c = &cx.theme.control;
        let (disc, grow, lane) = (c.icon_button, c.press_grow, c.focus_gap + c.stroke_mark);
        let side = slot_side(
            crate::theme::control_height(&cx.theme.metrics, &cx.theme.control),
            disc,
            grow,
            lane,
        );
        let sense = if self.enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), sense);
        let pressed = self.enabled && response.is_pointer_button_down_on();

        let t_press = cx.animate(
            response.id.with("press"),
            if pressed { 1.0 } else { 0.0 },
            if pressed {
                cx.theme.motion.press
            } else {
                cx.theme.motion.press_release
            },
        );
        // Rule 1.5: only the painted disc grows. The slot above is already allocated, so a row of
        // icon buttons never reflows under a finger.
        let radius = disc * 0.5 + cx.theme.control.press_grow * t_press;
        let centre = rect.center();
        // The hold advances whoever draws it.
        let (progress, completed) = match self.long_press {
            Some(duration) if self.enabled => hold(cx, response.id, pressed, duration),
            _ => (0.0, false),
        };
        let held = Held {
            centre,
            radius,
            progress,
        };
        let focused = response.has_focus();
        let hold_look = self.long_press.filter(|_| self.enabled).map(|_| HoldLook {
            rect: arc_rect(cx, held),
            progress,
            done: completed,
        });
        if let Some(paint) = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.icon_button.as_mut())
        {
            paint(
                ui.painter(),
                &mut IconButtonLook {
                    rect,
                    disc: Rect::from_center_size(centre, Vec2::splat(radius * 2.0)),
                    icon: &self.icon,
                    name: &self.name,
                    kind: self.kind,
                    pressed,
                    press: t_press,
                    enabled: self.enabled,
                    focused,
                    hold: hold_look,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
        } else {
            let ink = Ink::of(cx, self.kind, pressed, self.enabled);
            paint_disc(ui.painter(), cx, centre, radius, &ink);
            let glyph = glyph_box(disc, cx.theme.control.icon);
            let at = Rect::from_center_size(centre, Vec2::splat(glyph));
            let style = crate::icons::IconStyle {
                color: crate::icons::IconColor::Fixed(ink.glyph),
                ..crate::icons::IconStyle::default()
            };
            cx.icons
                .paint(ui.painter(), at, &self.icon, &style, cx.theme);
            paint_arc(ui.painter(), cx, held, &ink);
            if focused && progress <= 0.0 {
                let gap = cx.theme.control.focus_gap;
                ui.painter().circle_stroke(
                    centre,
                    radius + gap,
                    Stroke::new(cx.theme.control.stroke_mark, ink.focus),
                );
            }
        }
        LongPressResponse {
            response,
            completed,
            progress,
        }
    }
}

/// The side the widget allocates: a whole target, or the ring's outermost reach, whichever is more.
///
/// It grows past the target only where the ring would otherwise be clipped — the failure the
/// segmented control records, where a flush control's ring ran outside its parent's clip rect. At
/// a resolved gloved finger the reserve is already inside the target and this returns it exactly.
fn slot_side(touch_target: f32, disc: f32, press_grow: f32, lane: f32) -> f32 {
    touch_target.max(disc + 2.0 * (press_grow + lane))
}

/// The glyph box: `control.icon`, never larger than the square inscribed in the disc.
fn glyph_box(disc: f32, icon: f32) -> f32 {
    icon.min(disc * std::f32::consts::FRAC_1_SQRT_2)
}

/// The colours one frame paints with, faded **once** at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    face: Color32,
    /// The identifying boundary, where the fill is not one.
    edge: Option<Color32>,
    glyph: Color32,
    /// The long-press arc — **the kind's colour**, see the module doc.
    arc: Color32,
    focus: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, kind: ButtonKind, pressed: bool, enabled: bool) -> Self {
        let dress = kind.dress();
        let mut face = cx.theme.color(dress.face);
        if dress.translucent {
            face = face.gamma_multiply(cx.theme.control.fill_alpha);
        }
        if pressed {
            // `face.blend(tint)`, not the reverse: `blend`'s receiver is the layer **behind**.
            face = face.blend(cx.theme.color(ColorRole::Pressed));
        }
        // The arc is drawn outside the disc, so it has to read on the **page**, not on the face.
        // That is the kind's own identifying colour — its edge where it has one, its fill where the
        // fill is the boundary.
        let arc = cx.theme.color(dress.edge.unwrap_or(dress.face));
        let ink = Self {
            face,
            edge: dress.edge.map(|role| cx.theme.color(role)),
            glyph: cx.theme.color(dress.label),
            arc,
            focus: cx.theme.color(ColorRole::Focus),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                face: ink.face.gamma_multiply(a),
                edge: ink.edge.map(|c| c.gamma_multiply(a)),
                glyph: ink.glyph.gamma_multiply(a),
                arc: ink.arc.gamma_multiply(a),
                focus: ink.focus.gamma_multiply(a),
            }
        }
    }
}

/// The disc and its edge.
fn paint_disc(painter: &egui::Painter, cx: &Cx<'_>, centre: Pos2, radius: f32, ink: &Ink) {
    painter.circle_filled(centre, radius, ink.face);
    if let Some(edge) = ink.edge {
        let w = cx.theme.control.stroke_edge;
        // R11 again: pulled in by the **whole** stroke, so the outer face lands on `radius` and the
        // silhouette stays the width the token asked for.
        painter.circle_stroke(centre, (radius - w).max(0.0), Stroke::new(w, edge));
    }
}

/// Where the disc is this frame and how far a hold on it has got — what the arc is drawn from.
#[derive(Debug, Clone, Copy)]
struct Held {
    centre: Pos2,
    radius: f32,
    progress: f32,
}

/// The long press the arc tracks: linear `0 → 1` while held, 180 ms back on a release before the
/// end. Returns the progress and whether the hold is complete.
fn hold(cx: &mut Cx<'_>, id: egui::Id, pressed: bool, duration: Duration) -> (f32, bool) {
    let tween = if pressed {
        Tween {
            duration,
            easing: crate::motion::Easing::Linear,
        }
    } else {
        Tween::cubic_out(Duration::from_millis(180))
    };
    let progress = cx.animate(id.with("ring"), if pressed { 1.0 } else { 0.0 }, tween);
    (progress, pressed && progress >= 1.0 - 1e-4)
}

/// The square the arc is drawn in round the disc. `progress_ring` insets by half its stroke and
/// then strokes outside, so a box of `2(radius + gap) + w` lands the arc's inner face exactly
/// `gap` clear of the disc.
fn arc_rect(cx: &Cx<'_>, at: Held) -> Rect {
    let (gap, w) = (cx.theme.control.focus_gap, cx.theme.control.stroke_mark);
    Rect::from_center_size(at.centre, Vec2::splat(2.0f32.mul_add(at.radius + gap, w)))
}

/// The built-in long-press arc, while a hold is under way.
fn paint_arc(painter: &egui::Painter, cx: &Cx<'_>, at: Held, ink: &Ink) {
    if at.progress <= 0.0 {
        return;
    }
    let style = crate::icons::parametric::ParamStyle {
        color: ink.arc,
        muted: ink.arc.gamma_multiply(0.30),
        danger: ink.arc,
        stroke_px: cx.theme.control.stroke_mark,
    };
    crate::icons::parametric::progress_ring(painter, arc_rect(cx, at), at.progress, &style);
}

#[cfg(test)]
mod tests {
    use super::{glyph_box, slot_side};
    use crate::theme::{contrast, ColorRole, ControlSpec, Palette, Preset};
    use crate::unit::{Scale, ScaleConfidence, ScalePolicy, ScaleSource};

    /// **What is pressed is a whole touch target, on both axes** — and larger only where the ring
    /// would otherwise be clipped. This is the miss the switch shipped at 0.60 T on its short axis.
    #[test]
    fn the_slot_is_never_smaller_than_a_touch_target() {
        for finger in [6.0_f32, 9.0, 13.0, 16.0] {
            for density in [4.0_f32, 6.67, 12.0, 20.0] {
                let scale = Scale::resolve(
                    density,
                    &ScalePolicy::default().with_finger_mm(finger),
                    ScaleSource::Backend,
                    ScaleConfidence::Measured,
                    egui::vec2(800.0, 480.0),
                );
                let c = ControlSpec::default().resolve(&scale);
                let target = crate::theme::MetricsSpec::default()
                    .resolve(&scale)
                    .touch_target;
                let side = slot_side(
                    target,
                    c.icon_button,
                    c.press_grow,
                    c.focus_gap + c.stroke_mark,
                );
                assert!(
                    side >= target,
                    "{finger} mm at {density} px/mm: the slot is {side:.1} against a {target:.1} target"
                );
                assert!(
                    side >= c.icon_button + 2.0 * (c.press_grow + c.focus_gap + c.stroke_mark)
                        - 1e-3,
                    "{finger} mm at {density} px/mm: the focus ring would be clipped"
                );
            }
        }
    }

    /// The glyph always fits inside the disc, which is what the disc's du floor exists for.
    #[test]
    fn the_glyph_always_fits_inside_the_disc() {
        for finger in [6.0_f32, 9.0, 13.0, 16.0] {
            let scale = Scale::resolve(
                8.0,
                &ScalePolicy::default().with_finger_mm(finger),
                ScaleSource::Backend,
                ScaleConfidence::Measured,
                egui::vec2(800.0, 480.0),
            );
            let c = ControlSpec::default().resolve(&scale);
            let g = glyph_box(c.icon_button, c.icon);
            let inscribed = c.icon_button * std::f32::consts::FRAC_1_SQRT_2;
            assert!(
                g <= inscribed + 1e-3,
                "{finger} mm: {g:.2} escapes {inscribed:.2}"
            );
            assert!(g > 0.0);
        }
    }

    /// **The arc reads where it is drawn.** It sits outside the disc, so its neighbour is the page
    /// — and the colour `BigButton` uses for its own ring would be invisible there.
    #[test]
    fn the_long_press_arc_reads_on_the_page_where_the_labels_colour_would_not() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                for role in [ColorRole::Primary, ColorRole::OnSurface, ColorRole::Danger] {
                    for (label, ground) in [("page", p.surface), ("card", p.surface_variant)] {
                        let got = contrast(p.get(role), ground);
                        assert!(
                            got >= 3.0,
                            "{name} {mode}: a {role:?} arc measures {got:.2} on the {label}"
                        );
                    }
                }
            }
        }
        // And the reason it is not the glyph's colour, on the palette that shows it worst.
        let p = Palette::preset(Preset::Base, false);
        let white_on_page = contrast(p.on_primary, p.surface);
        assert!(
            white_on_page < 3.0,
            "`OnPrimary` now measures {white_on_page:.2} on base light's page — if that has \
             changed, this element's reason for not using it needs rewriting"
        );
    }
}
