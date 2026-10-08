//! `StatusLamp` — a lit disc and the word beside it. The instrument panel's one missing element.
//!
//! # Why a lamp is sized off its word and not off the touch target
//!
//! The instrument-panel reference draws a 40 px lamp against an 18 px cap height — 2.22 cap heights
//! — and against that panel's own 63 px control height the same lamp is 0.635 T, where this crate's
//! would be 0.525 T. The two anchors disagree by 21 %, and the disagreement vanishes when both are
//! measured against the word beside them, which is how a lamp is actually read: as a pair. So
//! `control.lamp_ratio` hangs off `type_scale.body` and a lamp beside a larger label grows with it.
//!
//! # Why the mark is `Surface` — the opposite colour the switch reached for
//!
//! The switch's knob rejected `Surface` and was right to: a knob has to be seen over the on track
//! *and* the off one, and only one of those flips with the theme. A lamp's mark sits over the lit
//! fill and nothing else, and **the lit fills flip with the theme exactly as `Surface` does** — a
//! dark palette gets bright severities and a light one gets dark ones — so the ground colour tracks
//! them in both directions. Measured over all four states in all four palettes the mark runs
//! 3.81–10.52 against a 3.0 floor, where `OnPrimary` bottoms out at 1.97 and `OnSurface` at 1.41.
//!
//! Same rule, opposite answer, because the question is not the same one.
//!
//! # Why every state carries a shape
//!
//! Colour alone is never the channel (Rule 7.1, and the reason notifications were given severity
//! *shapes*). `Ok` is a tick, `Warn` a bang, `Fault` a cross, `Active` a bullseye, `Unknown` a bar —
//! and `Off` and `Unknown` are **hollow**, a heavier ring around nothing, which reads as an empty
//! socket at a glance. In greyscale the five are still five.
//!
//! Three of the marks are the crate's own, reused rather than redrawn: the tick is
//! [`Checkbox`](super::Checkbox)'s, the bar is its indeterminate mark, and the bullseye is the
//! radio's dot at the radio's own ratio. "This one is live" ought to be the same mark the crate
//! already means by a filled centre.
//!
//! # Why a lamp is never `enabled(false)`
//!
//! Disabled means *this control is unavailable*, and a lamp is never unavailable — it is unlit, or
//! the reading has not arrived. Those are [`LampState::Off`] and [`LampState::Unknown`], and they
//! are states of the instrument rather than of the widget. The flag exists for a lamp inside a
//! panel the operator has no rights to, and it costs what the crate's disabled policy costs
//! everywhere: one `gamma_multiply`, which drops the fill to 1.98–3.99 and the mark under it. A
//! reading you do not have is `Unknown`, not `enabled(false)`.
//!
//! # Why the bezel radius is pulled in by the whole stroke
//!
//! epaint 0.36.1 forces `PathStroke::outside()` on a circle (`tessellate_circle`), so a stroke
//! occupies `[r, r + w]`. To land the lamp's outer extent exactly on the diameter the token asks
//! for, the radius is `d / 2 − w` and not `d / 2 − w / 2`. Half would put the silhouette a stroke
//! over budget, and a row of lamps would no longer sit on the text's rhythm.

use super::LampLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::{lamp_size, ColorRole};
use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, Vec2};

/// What a lamp says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LampState {
    /// Nominal.
    Ok,
    /// Attention, but running.
    Warn,
    /// Stopped, or refusing to start.
    Fault,
    /// Live — the plain "this is on" with no verdict attached.
    #[default]
    Active,
    /// Not lit, and known not to be.
    Off,
    /// No reading. **Not** the same as [`Self::Off`], and not
    /// [`StatusLamp::enabled(false)`](StatusLamp::enabled) either.
    Unknown,
}

/// The silhouette a state carries, so that colour is never the only channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LampMark {
    /// [`LampState::Ok`] — the checkbox's own tick.
    Tick,
    /// [`LampState::Warn`].
    Bang,
    /// [`LampState::Fault`].
    Cross,
    /// [`LampState::Active`] — the radio's own dot.
    Bullseye,
    /// [`LampState::Unknown`] — the indeterminate checkbox's own bar.
    Bar,
    /// [`LampState::Off`] — an empty socket.
    None,
}

impl LampState {
    /// The fill's role, or `None` for the two hollow states.
    #[must_use]
    pub fn role(self) -> Option<ColorRole> {
        match self {
            Self::Ok => Some(ColorRole::Success),
            Self::Warn => Some(ColorRole::Warning),
            Self::Fault => Some(ColorRole::Danger),
            Self::Active => Some(ColorRole::Primary),
            Self::Off | Self::Unknown => None,
        }
    }

    /// The mark drawn inside it.
    #[must_use]
    pub fn mark(self) -> LampMark {
        match self {
            Self::Ok => LampMark::Tick,
            Self::Warn => LampMark::Bang,
            Self::Fault => LampMark::Cross,
            Self::Active => LampMark::Bullseye,
            Self::Unknown => LampMark::Bar,
            Self::Off => LampMark::None,
        }
    }

    /// Whether the lamp is lit.
    #[must_use]
    pub fn lit(self) -> bool {
        self.role().is_some()
    }
}

/// A status lamp.
#[derive(Debug)]
pub struct StatusLamp<'a> {
    state: LampState,
    /// Borrowed: a bar of lamps repaints every frame and must not allocate to be drawn.
    label: &'a str,
    enabled: bool,
}

impl<'a> StatusLamp<'a> {
    /// A lamp in `state`, labelled.
    ///
    /// There is no label-less constructor. A bare coloured disc on a machine panel is a puzzle, and
    /// the one thing the reference screens agree on is that every lamp has its word.
    #[must_use]
    pub fn new(state: LampState, label: &'a str) -> Self {
        Self {
            state,
            label,
            enabled: true,
        }
    }

    /// Enabled. See the module doc — a reading you do not have is [`LampState::Unknown`].
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The size this lamp will take, without drawing it — what a strip needs to lay equal cells out.
    #[must_use]
    pub fn measure(&self, ui: &egui::Ui, cx: &Cx<'_>) -> Vec2 {
        let d = lamp_size(&cx.theme.metrics, &cx.theme.control);
        let galley = self.galley(ui, cx, Color32::PLACEHOLDER);
        Vec2::new(
            d + cx.theme.control.gap + galley.rect.width(),
            d.max(galley.rect.height()),
        )
    }

    /// The label, laid out.
    fn galley(&self, ui: &egui::Ui, cx: &Cx<'_>, color: Color32) -> std::sync::Arc<egui::Galley> {
        let font = egui::FontId::proportional(cx.theme.metrics.type_scale.body);
        ui.painter()
            .layout_no_wrap(self.label.to_owned(), font, color)
    }

    /// Draw it.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let ink = Ink::of(cx, self.state, self.enabled);
        let d = lamp_size(&cx.theme.metrics, &cx.theme.control);
        let gap = cx.theme.control.gap;
        let galley = self.galley(ui, cx, ink.label);
        let size = Vec2::new(d + gap + galley.rect.width(), d.max(galley.rect.height()));
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());

        let centre = Pos2::new(rect.min.x + d * 0.5, rect.center().y);
        let at = egui::pos2(
            rect.min.x + d + gap,
            rect.center().y - galley.rect.height() * 0.5,
        );
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.lamp.as_mut()) {
            custom(
                ui.painter(),
                &mut LampLook {
                    rect,
                    lens: Rect::from_center_size(centre, Vec2::splat(d)),
                    word: Rect::from_min_size(at, galley.rect.size()),
                    label: self.label,
                    state: self.state,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }
        paint_lens(ui.painter(), cx, centre, d, ink, self.state);
        ui.painter().galley(at, galley, ink.label);
        response
    }
}

/// The colours one frame paints with, dimmed once at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    /// The lit disc, or transparent.
    fill: Color32,
    /// The ring.
    bezel: Color32,
    /// The mark inside.
    mark: Color32,
    /// The word.
    label: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, state: LampState, enabled: bool) -> Self {
        let ink = Self {
            fill: state
                .role()
                .map_or(Color32::TRANSPARENT, |role| cx.theme.color(role)),
            bezel: cx.theme.color(ColorRole::ControlEdge),
            // `Surface`, not `OnPrimary` — see the module doc. The lit fills flip with the theme
            // exactly as `Surface` does, so the mark tracks them in both directions.
            mark: cx.theme.color(ColorRole::Surface),
            label: cx.theme.color(if state.lit() {
                ColorRole::OnSurface
            } else {
                ColorRole::Muted
            }),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                fill: ink.fill.gamma_multiply(a),
                bezel: ink.bezel.gamma_multiply(a),
                mark: ink.mark.gamma_multiply(a),
                label: ink.label.gamma_multiply(a),
            }
        }
    }
}

/// The disc, its ring and the mark inside it.
fn paint_lens(
    painter: &egui::Painter,
    cx: &Cx<'_>,
    centre: Pos2,
    d: f32,
    ink: Ink,
    state: LampState,
) {
    let lit = state.lit();
    // A hollow lamp gets the heavier ring: a thick circle around nothing reads as an empty socket,
    // which is the silhouette that carries "not lit" without leaning on hue.
    let w = if lit {
        cx.theme.control.stroke_edge
    } else {
        cx.theme.control.stroke_mark
    };
    // R11: epaint strokes a circle **outside** its path, so the radius is pulled in by the whole
    // stroke and the outer extent lands exactly on `d`.
    let r = (d * 0.5 - w).max(0.0);
    painter.circle(centre, r, ink.fill, Stroke::new(w, ink.bezel));
    if !lit && state == LampState::Off {
        return;
    }
    let fill_d = (d - 2.0 * w).max(0.0);
    let at = Rect::from_center_size(centre, Vec2::splat(fill_d * cx.theme.control.tick_ratio));
    let mark_w = cx.theme.control.stroke_mark;
    // An unknown lamp is hollow, so its bar takes the ring's colour rather than the ground's.
    let color = if lit { ink.mark } else { ink.bezel };
    match state.mark() {
        LampMark::Tick => super::checkbox::paint_tick(painter, at, mark_w, color),
        LampMark::Bar => super::checkbox::paint_mixed(painter, at, mark_w, color),
        LampMark::Bullseye => {
            painter.circle_filled(centre, fill_d * cx.theme.control.dot_ratio * 0.5, color);
        }
        LampMark::Bang => paint_bang(painter, at, mark_w, color),
        LampMark::Cross => paint_cross(painter, at, mark_w, color),
        LampMark::None => {}
    }
}

/// The warning mark: a stem and a dot under it.
///
/// Two filled shapes and no path, for the reason the tick gives — epaint miters a path join, and a
/// bang drawn as a tapering triangle grows a spike at its apex.
fn paint_bang(painter: &egui::Painter, at: Rect, width: f32, color: Color32) {
    let r = width * 0.5;
    let stem = Rect::from_min_max(
        egui::pos2(at.center().x - r, at.min.y),
        egui::pos2(at.center().x + r, at.min.y + at.height() * 0.60),
    );
    painter.rect_filled(
        stem,
        egui::CornerRadius::same(crate::unit::round_u8(r)),
        color,
    );
    painter.circle_filled(egui::pos2(at.center().x, at.max.y - r), r, color);
}

/// The fault mark: two segments across the box with a disc at each end and at the crossing.
///
/// The identical construction the tick uses, for the identical reason: epaint's line joins are
/// mitered, and two crossed segments would spike at four corners without the caps.
fn paint_cross(painter: &egui::Painter, at: Rect, width: f32, color: Color32) {
    let stroke = Stroke::new(width, color);
    let down = [at.left_top(), at.right_bottom()];
    let up = [at.right_top(), at.left_bottom()];
    painter.line_segment(down, stroke);
    painter.line_segment(up, stroke);
    for end in down.into_iter().chain(up).chain([at.center()]) {
        painter.circle_filled(end, width * 0.5, color);
    }
}

#[cfg(test)]
mod tests {
    use super::{LampMark, LampState};
    use crate::theme::{contrast, ColorRole, Palette, Preset};

    /// **Every state is told apart by shape as well as by colour**, and no two share a mark — the
    /// rule the notifications were given severity shapes for.
    #[test]
    fn each_state_has_a_mark_of_its_own() {
        let states = [
            LampState::Ok,
            LampState::Warn,
            LampState::Fault,
            LampState::Active,
            LampState::Off,
            LampState::Unknown,
        ];
        let mut marks: Vec<LampMark> = states.iter().map(|s| s.mark()).collect();
        let before = marks.len();
        marks.dedup_by(|a, b| a == b);
        marks.sort_by_key(|m| format!("{m:?}"));
        marks.dedup();
        assert_eq!(marks.len(), before, "two states draw the same mark");
    }

    /// The two hollow states are the two unlit ones, and nothing else is.
    #[test]
    fn only_off_and_unknown_are_hollow() {
        for state in [
            LampState::Ok,
            LampState::Warn,
            LampState::Fault,
            LampState::Active,
        ] {
            assert!(state.lit(), "{state:?} should be lit");
        }
        for state in [LampState::Off, LampState::Unknown] {
            assert!(!state.lit(), "{state:?} should be hollow");
            assert!(state.role().is_none());
        }
    }

    /// **A lit lamp reads on both grounds, and its mark reads on the lamp**, in every palette.
    ///
    /// The mark is `Surface` and not `OnPrimary`, which is the opposite of the switch's answer;
    /// this is the measurement that makes the difference legitimate rather than inconsistent.
    #[test]
    fn a_lit_lamp_and_its_mark_clear_the_floor_in_every_palette() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                for state in [
                    LampState::Ok,
                    LampState::Warn,
                    LampState::Fault,
                    LampState::Active,
                ] {
                    let Some(role) = state.role() else { continue };
                    let fill = p.get(role);
                    for (label, ground) in [("page", p.surface), ("card", p.surface_variant)] {
                        let got = contrast(fill, ground);
                        assert!(
                            got >= 3.0,
                            "{name} {mode}: a {state:?} lamp measures {got:.2} on the {label}"
                        );
                    }
                    let mark = contrast(p.get(ColorRole::Surface), fill);
                    assert!(
                        mark >= 3.0,
                        "{name} {mode}: a {state:?} lamp's mark measures {mark:.2} on its own lens"
                    );
                }
            }
        }
    }

    /// The colour the switch chose for its knob would **fail** here, and the one this element chose
    /// would fail there. Both are recorded, so neither can be "tidied" into the other later.
    #[test]
    fn the_switchs_knob_colour_would_not_survive_on_a_lamp() {
        let p = Palette::preset(Preset::Base, true);
        let worst = [LampState::Ok, LampState::Warn, LampState::Fault]
            .into_iter()
            .filter_map(LampState::role)
            .map(|role| contrast(p.on_primary, p.get(role)))
            .fold(f32::INFINITY, f32::min);
        assert!(
            worst < 3.0,
            "`OnPrimary` now measures {worst:.2} on the severities in base dark — if that has \
             changed, the module doc's reason for choosing `Surface` needs rewriting"
        );
    }
}
