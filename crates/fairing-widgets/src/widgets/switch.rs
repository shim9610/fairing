//! `Switch` edits a Boolean value by tapping or dragging its knob.
//!
//! The knob follows an active drag and animates to the selected endpoint after
//! release. Track, knob, press feedback and focus outline use separate theme and
//! control tokens. Disabled presentation retains the selected-state distinction.
//! The caller owns the value and handles any associated device command.

use super::SwitchLook;
use crate::cx::WidgetCx as Cx;
use crate::motion::Tween;
use crate::theme::{ColorRole, Theme};
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};

/// Where the travel tween is remembered. Namespaced because in a composed row the id belongs to the
/// row, which hangs its own state off the same one.
const ON_KEY: &str = "fairing.switch.on";
/// Where the press tween is remembered.
const PRESS_KEY: &str = "fairing.switch.press";

/// A toggle switch.
///
/// It edits a `&mut bool` rather than owning one, so a screen that already holds the setting does
/// not keep a second copy of it that can disagree.
#[derive(Debug)]
pub struct Switch<'a> {
    on: &'a mut bool,
    enabled: bool,
}

impl<'a> Switch<'a> {
    /// Edits `on`.
    pub fn new(on: &'a mut bool) -> Self {
        Self { on, enabled: true }
    }

    /// Enabled state. A disabled control uses hover sensing rather than click/drag
    /// sensing, allowing another eligible target to handle the press.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The size the switch **draws** at — `control.mark_size × control.switch_aspect` by
    /// `control.mark_size`.
    ///
    /// Public because a row that composes a switch has to reserve the drawn size and not a whole
    /// target's worth of width: under Rule 1.3 the row owns the hit rect, so the trailing column is
    /// only as wide as the picture. The alternative — letting the row guess from `touch_target` —
    /// is how the shell ended up with its own `SWITCH_ASPECT` copy.
    #[must_use]
    pub fn drawn_size(theme: &Theme) -> Vec2 {
        let ms = theme.control.mark_size;
        Vec2::new(ms * theme.control.switch_aspect, ms)
    }

    /// Draw it standing on its own. On the frame the value changed, `response.changed()`.
    pub fn show(mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        // `cx.theme` is a shared reference field, so copying it out leaves `cx` free for `animate`.
        let theme = cx.theme;
        let drawn = Self::drawn_size(theme);
        // Rule 1.1: a full target on **both** axes, `max` so a device whose mark is larger than its
        // target still gets a slot that holds the drawing.
        let slot = Vec2::new(
            drawn
                .x
                .max(crate::theme::control_height(&theme.metrics, &theme.control)),
            drawn
                .y
                .max(crate::theme::control_height(&theme.metrics, &theme.control)),
        );
        let sense = if self.enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        };
        let (slot_rect, mut response) = ui.allocate_exact_size(slot, sense);
        // The knob follows the finger, and says so: the rail's page swipe leaves this drag alone.
        crate::drag::claim_if_held(&response);
        // The track is centred in the slot on both axes: the silhouette is derived from the slot,
        // never the other way round.
        let track = Rect::from_center_size(slot_rect.center(), drawn);
        let dragged_to = if self.enabled {
            self.settle(ui, &mut response, track, theme.motion.fling_px_s)
        } else {
            None
        };
        // The press runs on its own clock — 80 ms out, 120 ms back — so a tap part-way through the
        // travel does not restart it, and the tint fades out with the shape instead of popping.
        let held = self.enabled && response.is_pointer_button_down_on();
        let press = cx.animate(
            response.id.with(PRESS_KEY),
            if held { 1.0 } else { 0.0 },
            if held {
                theme.motion.press
            } else {
                theme.motion.press_release
            },
        );
        let drive = Drive {
            press,
            held,
            dragged_to,
            focused: response.has_focus(),
        };
        self.paint(ui.painter(), cx, response.id, (slot_rect, track), drive);
        response
    }

    /// Draw a switch into a caller-owned rectangle without allocating a separate UI
    /// slot or sensing a separate press. The owner handles interaction and updates the
    /// Boolean value. No independent focus ring or press growth is added.
    /// The row ID keys retained travel animation.
    pub fn draw(self, painter: &egui::Painter, cx: &mut Cx<'_>, id: Id, rect: Rect) {
        let track = Rect::from_center_size(rect.center(), Self::drawn_size(cx.theme));
        self.paint(painter, cx, id, (rect, track), Drive::default());
    }

    /// The pointer half: a tap flips it, a drag carries the knob, and the release decides. Returns
    /// the knob position the finger is holding it at, if it is being dragged right now.
    ///
    /// Split out of [`Switch::show`] because it is the one part of the widget the vocabulary did
    /// **not** change, and keeping it whole makes that visible in the diff.
    fn settle(
        &mut self,
        ui: &egui::Ui,
        response: &mut Response,
        track: Rect,
        fling: f32,
    ) -> Option<f32> {
        // The value axis is the **resting** track: a press swells the picture, and the finger's
        // reading of "how far along" must not move with it.
        let radius = track.height() / 2.0;
        let travel = (track.width() - track.height()).max(1.0);
        let at = |pos: Pos2| ((pos.x - track.min.x - radius) / travel).clamp(0.0, 1.0);
        if response.drag_stopped() {
            // The release is confirmed by half the distance, or by the velocity.
            let vx = ui.input(|i| i.pointer.velocity().x);
            let held = response
                .interact_pointer_pos()
                .map_or(if *self.on { 1.0 } else { 0.0 }, at);
            let next = if vx > fling {
                true
            } else if vx < -fling {
                false
            } else {
                held >= 0.5
            };
            if next != *self.on {
                *self.on = next;
                response.mark_changed();
            }
        } else if response.dragged() {
            return response.interact_pointer_pos().map(at);
        } else if response.clicked() {
            *self.on = !*self.on;
            response.mark_changed();
        }
        None
    }

    /// The shared paint path: the knob's travel, then a painter's drawing where one is given
    /// or the built-in one. `rect` is the slot and `track` the resting track in it. The
    /// built-in focus ring hangs off the silhouette actually painted rather than the resting one:
    /// `press_grow` is larger than `focus_gap`, so measured from the resting track a pressed switch
    /// would grow out through its own ring.
    fn paint(
        &self,
        painter: &egui::Painter,
        cx: &mut Cx<'_>,
        id: Id,
        (rect, track): (Rect, Rect),
        drive: Drive,
    ) {
        // `cx.theme` is a shared reference field, so copying it out leaves `cx` free for `animate`.
        let theme = cx.theme;
        let control = &theme.control;
        let t = match drive.dragged_to {
            // Mid-drag the store takes the finger's value too — on release the tween carries on
            // from there rather than snapping back to where the last tween had got to.
            Some(held) => cx.animate(id.with(ON_KEY), held, Tween::instant()),
            None => cx.animate(
                id.with(ON_KEY),
                if *self.on { 1.0 } else { 0.0 },
                theme.motion.switch,
            ),
        };
        if let Some(paint) = cx.painters.as_deref_mut().and_then(|p| p.switch.as_mut()) {
            paint(
                painter,
                &mut SwitchLook {
                    rect,
                    track,
                    on: *self.on,
                    travel: t,
                    pressed: drive.held,
                    press: drive.press,
                    enabled: self.enabled,
                    focused: drive.focused,
                    theme,
                    icons: &mut *cx.icons,
                },
            );
            return;
        }
        let ink = self.ink(theme, t, drive.press);
        let g = geometry(
            track,
            control.knob_ratio,
            control.press_grow * drive.press,
            t,
        );
        // Rule 4.1: a pill, because something travels along it. Half the **drawn** height, so the
        // press swell keeps the shape instead of squaring the ends off.
        let radius = CornerRadius::same(round_u8(g.track.height() / 2.0));
        painter.rect_filled(g.track, radius, ink.face);
        if ink.edge.a() > 0 {
            painter.rect_stroke(
                g.track,
                radius,
                Stroke::new(control.stroke_edge, ink.edge),
                // Inside, so the edge is part of the silhouette and the track never measures taller
                // than `mark_size` — the trailing column in a row is exactly that tall.
                StrokeKind::Inside,
            );
        }
        // No stroke: the knob is the ink of its own face, so it carries its contrast in the fill.
        // (When it had one, the radius had to be pulled in by the **whole** stroke width rather
        // than half — `epaint` strokes a circle outside the path it is given, `tessellate_circle`
        // forcing `PathStroke::from(stroke).outside()`, epaint 0.36.1 tessellator.rs:1531. Half
        // left the silhouette oversized: it broke out of the track and past the card's edge.)
        painter.circle_filled(g.knob, g.knob_radius, ink.knob);
        if drive.focused {
            let ring = self.fade(theme, theme.color(ColorRole::Focus));
            paint_focus_ring(painter, theme, g.track, ring);
        }
    }

    /// The colours one frame paints with, dimmed once at the end when the switch is disabled.
    ///
    /// `t_on` is 0 off and 1 on; `t_press` is the press tween.
    fn ink(&self, theme: &Theme, t_on: f32, t_press: f32) -> Ink {
        let face = theme
            .color(ColorRole::Outline)
            .lerp_to_gamma(theme.color(ColorRole::Primary), t_on)
            // `face.blend(tint)`, not `tint.blend(face)`: `blend`'s **receiver is the layer
            // behind**, so the other order multiplies the tint by `255 - 255` over an opaque face
            // and paints nothing at all. The tint rides the press tween, so it leaves with the
            // growth rather than popping off on the releasing frame.
            .blend(theme.color(ColorRole::Pressed).gamma_multiply(t_press));
        Ink {
            face: self.fade(theme, face),
            // The on face is its own boundary, so the edge leaves on the same tween the fill
            // arrives on — held to the end it would draw a `Muted` line around a `Primary` track.
            edge: self.fade(
                theme,
                theme
                    .color(ColorRole::ControlEdge)
                    .gamma_multiply(1.0 - t_on),
            ),
            // The knob rides the same tween as the face under it, so it is the ink of whichever
            // face it is over. Both ends are gated pairs; the crossing frames are transient and
            // land where the track is mid-tone too.
            knob: self.fade(
                theme,
                theme
                    .color(ColorRole::Muted)
                    .lerp_to_gamma(theme.color(ColorRole::OnPrimary), t_on),
            ),
        }
    }

    /// Disabled is **one** `gamma_multiply` over every colour the control paints, at one call
    /// site. Not a role swap, which loses the live state — a disabled *on* switch must still read as
    /// on — and not the per-element 0.4 / 0.7 / 0.6 this replaces, which dimmed the three parts of
    /// one object by three different amounts.
    fn fade(&self, theme: &Theme, color: Color32) -> Color32 {
        if self.enabled {
            color
        } else {
            color.gamma_multiply(theme.control.disabled_alpha)
        }
    }
}

/// How the switch is being driven this frame.
///
/// The two travel together and are meaningless apart — the drag decides where the knob is and the
/// press decides how big it is drawn — so they are one argument rather than two that can be passed
/// in the wrong order.
#[derive(Debug, Clone, Copy, Default)]
struct Drive {
    /// The press tween, 0 idle to 1 held. It drives both the growth and the tint, so the two cannot
    /// come apart: the tint alone measures 1.07–1.16 over `Primary` and is not a press treatment.
    press: f32,
    /// Whether a finger is on it now.
    held: bool,
    /// Where the finger is holding the knob, 0 to 1, while it is being dragged.
    dragged_to: Option<f32>,
    /// Whether it has the keyboard focus — never, drawn into a row.
    focused: bool,
}

/// The colours one frame paints with, resolved once so the disabled fade is a single mechanism
/// rather than a `gamma_multiply` sprinkled down the paint path.
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// The track's filled body.
    face: Color32,
    /// The track's identifying boundary, which only the off face carries.
    edge: Color32,
    /// The mover's body — the ink of the face it is over.
    knob: Color32,
}

/// What one switch draws this frame, in du.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Geometry {
    /// The track actually painted, press growth included.
    track: Rect,
    /// The knob's centre.
    knob: Pos2,
    /// The knob's outer radius.
    knob_radius: f32,
}

/// The geometry for one frame, from the **resting** track rect.
///
/// Free-standing, and taking plain numbers rather than the theme, so the two things the design leans on —
/// the knob travels `mark_size × (switch_aspect − 1)`, and the press only ever grows — can be
/// asserted without a palette or a `Ui`. `t` is 0 off and 1 on; `grow` is the press growth in du,
/// already tweened.
///
/// Every length is taken from the **drawn** track rather than from `mark_size`, so a press swells
/// the whole object at once. Deriving the knob from the resting size instead would leave it
/// rattling inside a track that had grown around it.
fn geometry(track: Rect, knob_ratio: f32, grow: f32, t: f32) -> Geometry {
    // Rule 1.5: the growth is on the **painted** rect only. The caller's allocated rect is fixed
    // for the frame, so a pressed switch never reflows the row it sits in.
    let drawn = track.expand(grow);
    let height = drawn.height();
    // The knob's centre stops half a track-height from each end, so the round end cap of the track
    // and the round knob are concentric at rest — which is what makes the inset read as even.
    let travel = (drawn.width() - height).max(0.0);
    Geometry {
        track: drawn,
        knob: Pos2::new(drawn.min.x + height / 2.0 + travel * t, drawn.center().y),
        knob_radius: height * knob_ratio / 2.0,
    }
}

/// Paint the focus outline outside the control's painted boundary.
/// Check the focus colour against the surrounding background for custom palettes.
fn paint_focus_ring(painter: &egui::Painter, theme: &Theme, silhouette: Rect, color: Color32) {
    let width = theme.control.stroke_mark;
    // `+ width / 2` because the stroke straddles its path: the ring's inner face then sits exactly
    // `focus_gap` clear of the silhouette.
    let ring = silhouette.expand(theme.control.focus_gap + width / 2.0);
    painter.rect_stroke(
        ring,
        // Half the ring's own height: a pill stays a pill, at any scale, with nothing to compute.
        CornerRadius::same(round_u8(ring.height() / 2.0)),
        Stroke::new(width, color),
        StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::geometry;
    use crate::theme::contrast;
    use egui::{pos2, Rect};

    /// Check knob/track contrast for the built-in on/off palette combinations.
    #[test]
    fn the_knob_reads_on_both_faces_in_every_palette() {
        for &preset in crate::theme::Preset::ALL {
            for dark in [true, false] {
                let p = crate::theme::Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                for (label, knob, face) in
                    [("on", p.on_primary, p.primary), ("off", p.muted, p.outline)]
                {
                    let got = contrast(knob, face);
                    assert!(
                        got >= 3.0,
                        "{name} {mode}: the {label} knob measures {got:.2} on its own track"
                    );
                }
            }
        }
    }

    /// **An off switch is a filled object, not an outline around nothing.**
    ///
    /// `SurfaceVariant` was the off face until base light showed what that meant there: the card
    /// *is* `SurfaceVariant`, so the track measured 1.00 against the surface under it and the
    /// switch existed only as its hairline. The floor is deliberately low — an off track is a quiet
    /// fill, not a contrasting one — but it is not 1.00.
    #[test]
    fn the_off_track_is_a_fill_you_can_see_on_the_card() {
        for &preset in crate::theme::Preset::ALL {
            for dark in [true, false] {
                let p = crate::theme::Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                let got = contrast(p.outline, p.surface_variant);
                assert!(
                    got >= 1.10,
                    "{name} {mode}: the off track measures {got:.2} on the card it sits on"
                );
            }
        }
    }

    /// The knob stays inside the track at both ends and travels `track_w − track_h` — the two
    /// properties the inset and the greyscale carrier depend on.""
    #[test]
    fn the_knob_travels_between_the_two_end_caps() {
        let track = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(88.44, 49.13));
        let (off, on) = (
            geometry(track, 0.875, 0.0, 0.0),
            geometry(track, 0.875, 0.0, 1.0),
        );
        assert!((on.knob.x - off.knob.x - (track.width() - track.height())).abs() < 0.01);
        for g in [off, on] {
            assert!(
                g.knob_radius < track.height() / 2.0,
                "the knob fills the track"
            );
            assert!(
                g.knob.x - g.knob_radius >= track.min.x && g.knob.x + g.knob_radius <= track.max.x,
                "the knob escaped the track"
            );
            assert!((g.knob.y - track.center().y).abs() < f32::EPSILON);
        }
    }

    /// A press only ever grows the painted silhouette, and the knob grows with it rather than
    /// rattling inside a track that moved out from around it.
    #[test]
    fn the_press_grows_the_whole_object() {
        let track = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(88.44, 49.13));
        let idle = geometry(track, 0.875, 0.0, 0.5);
        let held = geometry(track, 0.875, 4.91, 0.5);
        assert!(held.track.contains_rect(idle.track));
        assert!(held.knob_radius > idle.knob_radius);
        // The inset — the gap between the knob and the track's edge — keeps its proportion.
        let ratio =
            |g: super::Geometry| (g.track.height() / 2.0 - g.knob_radius) / g.track.height();
        assert!((ratio(held) - ratio(idle)).abs() < 1e-4);
    }
}
