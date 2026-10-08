//! `TouchSlider` — a thick pill track with a mover that stands proud of it. The value follows the
//! finger 1:1 (no easing), and pressing swells the track, tints it and raises a value label
//! (A7, vocabulary).
//!
//! # The thick track: the original argument, and what vindicated it
//!
//! A thin line track with a large round knob is **a shape from the mouse era**. The knob is much
//! bigger than the track, so a finger on it hides the very place it is holding, and a thin track
//! does not tell a gloved hand where to press.
//!
//! A thick track solves three things at once.
//!
//! - **You can see where to press.** The track itself looks like a touch target.
//! - **The value reads as an area.** The filled length is a bar chart, so a glance gives you the
//!   rough value — which matters on an instrument panel.
//! - **A finger does not hide it.** The mover overhangs the track by
//!   `(metrics.slider_thumb − control.slider_track) / 2` — 8.60 du, 1.365 mm, on each side. That
//!   is a small fraction of a 13 mm contact patch, so the finger still does not eat the view, and
//!   when the finger lifts the mover still has a silhouette against the container's ground.
//!
//! Material 3 Expressive reached the same thick-track conclusion independently. What it got right
//! and this module originally got wrong is one number: M3 Expressive makes its bar **2.75×** the
//! track height, so the bar overhangs and can be found; the shipped fairing bar was **0.72×** and
//! buried **inside** the track. Same shape, opposite proportion — and a mover buried in a track is
//! a notch whose edges run parallel to the track's own edges a few du away, which is exactly the
//! gap-closing that disappears under glare. So the sentence that used to stand here ("the handle
//! is inside the track, so no knob eats the view") is wrong in its second half and has been
//! replaced by the overhang above: containment was never what stopped the finger hiding the value
//! — the small size of the mover is.
//!
//! # Which handle suits which panel
//!
//! [`HandleStyle`] is a choice, not a taste. The default is [`HandleStyle::Bar`], and the
//! vocabulary picks it: a value track is one where the mover sits on a two-colour boundary
//! anywhere along the length and must be **found** there, so it stands proud.
//!
//! - [`HandleStyle::Bar`] — the default, and the right answer on every panel a gloved finger
//!   drives. A narrow proud pill: it marks the boundary without covering much of the filled area,
//!   and stacked sliders keep a calm rhythm because the mover is thin along the track.
//! - [`HandleStyle::Knob`] — a circle of the same proud diameter. Take it on **a panel driven by a
//!   pointer** (mouse, trackball, rotary encoder in a rack) — the objection at the top of this
//!   module is an objection to a 13 mm contact patch, not to a circle, and with no finger on the
//!   glass a circle is the grab affordance a pointer user already knows. Take it also on **a wide,
//!   low-density panel** (the 289 mm-wide 800×1280 reference): there the track's length grows with
//!   the panel while the bar's width does not, so the bar thins to a scratch across a very long
//!   track, and a disc of the mover's full diameter is the mark that still reads at 600 mm.
//! - [`HandleStyle::Inset`] — the shipped shape, a bar set **inside** the track. It is the right
//!   answer where a proud mover would overhang into something else: a dense stack of tracks (a
//!   mixer strip, a gauge list whose row pitch is near the mover's diameter), or a track read far
//!   more often than it is dragged, where the mover is a tick on a bar chart rather than a grip.
//!   It is a deliberate departure from the proud rule and it costs legibility under glare.
//! - [`HandleStyle::None`] — no mover at all. For a value that is displayed rather than dragged: a
//!   level, a progress, a read-only gauge. Nothing is being found by hand, so nothing has to stand
//!   proud, and the filled boundary carries the value on its own.
//!
//! The hit area is a whole `metrics.touch_target` row whichever style is chosen — what changes is
//! what you see, not what you press (vocabulary Rule 1.1).

use super::SliderLook;
use crate::cx::WidgetCx as Cx;
use crate::theme::{ColorRole, Theme};
use crate::unit::round_u8;
use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Vec2};
use std::f32::consts::{FRAC_PI_2, PI};
use std::ops::RangeInclusive;

/// The floor on a hand-set [`TouchSlider::thickness`], as a multiple of `control.stroke_edge`.
///
/// Three edges: the mover carries one edge on each side and needs at least one edge's worth of
/// body between them, or the two-tone rule that lets it be seen over both sides of the track has
/// nothing left to work with. Written as a multiple rather than a du value so it follows the
/// stroke ladder instead of freezing at one panel's numbers.
const MIN_TRACK_EDGES: f32 = 3.0;

/// Which mover a slider draws on its track.
///
/// The module docs say which one suits which panel. The default is [`Self::Bar`] because a value
/// track needs a mover that can be found anywhere along its length; the other three are for the
/// cases where that is not what the panel needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HandleStyle {
    /// A narrow pill standing proud of the track. The default.
    #[default]
    Bar,
    /// A circle standing proud of the track, at the same diameter the bar overhangs to.
    Knob,
    /// The shipped shape: a bar set **inside** the track, clear of the track's own edge.
    Inset,
    /// No mover. The filled boundary is the only mark.
    None,
}

/// A slider's three colours. They default to palette roles, so one `[theme.palette]` carries
/// them along.
///
/// Use this to paint **one** slider differently — needed on an instrument panel where the
/// meaning of the value sets the colour: `Warning` for a temperature, `Success` for a level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderColors {
    /// The filled side. [`ColorRole::Primary`] by default.
    pub fill: ColorSpec,
    /// The remaining side. [`ColorRole::SurfaceVariant`] by default.
    pub track: ColorSpec,
    /// The mover's **edge**. [`ColorRole::OnSurface`] by default.
    ///
    /// # Why it is the edge and no longer the whole handle
    ///
    /// A mover crosses both faces of the track, and measured across the four shipped palettes no
    /// single role clears 3.0 against both `Primary` and `SurfaceVariant`. So the mover is
    /// two-tone: a body that carries it over the filled side and this edge that carries it over
    /// the empty one (11.94–17.38 for `OnSurface`). The old default of `OnSurface` therefore keeps
    /// painting exactly what it painted, on the part of the mover it was always doing the work on.
    ///
    /// The body is [`ColorRole::OnPrimary`], the ink of the side it has to be seen against
    /// (4.59–9.37). It used to be `Surface` — the container's own ground, which is near-white in a
    /// light palette and near-black in a dark one, so the dark-theme mover came out as a black bar
    /// inside a white outline and read as a **seam in the track** rather than a grip. The pair the
    /// palette gate already holds is `on_primary/primary`, so this body cannot go quietly missing
    /// on a new palette either.
    ///
    /// Overriding it is allowed and is the same kind of escape hatch as
    /// [`TouchSlider::thickness`]: taking the track's own accent here loses the two-tone
    /// guarantee, because on the filled side the edge then matches what it lies on.
    pub handle: ColorSpec,
}

impl Default for SliderColors {
    fn default() -> Self {
        Self {
            fill: ColorSpec::Role(ColorRole::Primary),
            track: ColorSpec::Role(ColorRole::SurfaceVariant),
            handle: ColorSpec::Role(ColorRole::OnSurface),
        }
    }
}

/// A palette role or a fixed colour. The same contract as [`IconColor`](crate::icons::IconColor)
/// on the icon side — **a role follows the theme and a fixed colour does not.**
///
/// Both `ColorRole` and `Color32` go in through `into()`, so the call site has nothing to think
/// about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColorSpec {
    /// A palette role. It follows the theme toggle and a palette swap.
    Role(ColorRole),
    /// A fixed colour. It does not follow — only for pinning a brand colour.
    Fixed(egui::Color32),
}

impl ColorSpec {
    /// The colour resolved against a theme.
    #[must_use]
    pub fn resolve(self, theme: &Theme) -> egui::Color32 {
        match self {
            Self::Role(role) => theme.color(role),
            Self::Fixed(color) => color,
        }
    }
}

impl From<ColorRole> for ColorSpec {
    fn from(role: ColorRole) -> Self {
        Self::Role(role)
    }
}

impl From<egui::Color32> for ColorSpec {
    fn from(color: egui::Color32) -> Self {
        Self::Fixed(color)
    }
}

/// A touch slider.
#[derive(Debug)]
pub struct TouchSlider<'a> {
    value: &'a mut f32,
    range: RangeInclusive<f32>,
    enabled: bool,
    thickness: Option<f32>,
    style: HandleStyle,
    colors: SliderColors,
}

impl<'a> TouchSlider<'a> {
    /// Edits `value` within `range`.
    ///
    /// A range given high to low is taken as the caller meant it — the ends are sorted, as
    /// [`Stepper::range`](super::Stepper::range) does — so the slider still edits rather than
    /// pinning every touch to one end.
    pub fn new(value: &'a mut f32, range: RangeInclusive<f32>) -> Self {
        let range = if range.start() <= range.end() {
            range
        } else {
            *range.end()..=*range.start()
        };
        Self {
            value,
            range,
            enabled: true,
            thickness: None,
            style: HandleStyle::default(),
            colors: SliderColors::default(),
        }
    }

    /// Set the track thickness directly (du). The default is `control.slider_track`, which
    /// follows the gloved-hand policy, so **setting it directly breaks that link** — use it only
    /// when one slider has to be thicker.
    ///
    /// It is floored at three × `control.stroke_edge`: below that the mover has no body left
    /// between its two edges.
    #[must_use]
    pub fn thickness(mut self, du: f32) -> Self {
        self.thickness = Some(du);
        self
    }

    /// Which mover to draw. See [`HandleStyle`] and the module docs — the default suits a gloved
    /// finger, and the other three each suit a panel this one does not.
    #[must_use]
    pub fn handle_style(mut self, style: HandleStyle) -> Self {
        self.style = style;
        self
    }

    /// All three colours at once.
    #[must_use]
    pub fn colors(mut self, colors: SliderColors) -> Self {
        self.colors = colors;
        self
    }

    /// The filled side's colour. For when the meaning of the value sets the colour — `Warning`
    /// for a temperature, `Success` for a level.
    ///
    /// ```
    /// # fn ui(ui: &mut egui::Ui, cx: &mut fairing_widgets::WidgetCx<'_>, temp: &mut f32) {
    /// use fairing_widgets::theme::ColorRole;
    /// use fairing_widgets::widgets::TouchSlider;
    ///
    /// TouchSlider::new(temp, 0.0..=120.0)
    ///     .fill(ColorRole::Warning)
    ///     .show(ui, cx);
    /// # }
    /// ```
    #[must_use]
    pub fn fill(mut self, color: impl Into<ColorSpec>) -> Self {
        self.colors.fill = color.into();
        self
    }

    /// The remaining side's colour.
    #[must_use]
    pub fn track_color(mut self, color: impl Into<ColorSpec>) -> Self {
        self.colors.track = color.into();
        self
    }

    /// The mover's edge colour (see [`SliderColors::handle`]).
    #[must_use]
    pub fn handle(mut self, color: impl Into<ColorSpec>) -> Self {
        self.colors.handle = color.into();
        self
    }

    /// Enabled.
    ///
    /// A disabled slider senses `hover()` only, so the press falls through to whatever is beneath
    /// it (Rule 1.4) instead of being swallowed by a control that cannot act on it.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Draw it. When the value changes, `response.changed()`.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        // `cx.theme` is a shared reference field, so copying it out leaves `cx` free for `animate`.
        let theme = cx.theme;
        let base_track = self.thickness.map_or_else(
            || base_thickness(theme),
            |du| du.max(theme.control.stroke_edge * MIN_TRACK_EDGES),
        );
        // Rule 1.1: the slot is a full touch target on both axes whatever the track is drawn at.
        let (rect, mut response) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width(),
                crate::theme::control_height(&theme.metrics, &theme.control),
            ),
            if self.enabled {
                Sense::click_and_drag()
            } else {
                Sense::hover()
            },
        );
        let (lo, hi) = (*self.range.start(), *self.range.end());
        let span = (hi - lo).max(f32::EPSILON);
        if self.enabled {
            follow_finger(
                &mut response,
                value_axis(rect, base_track),
                self.value,
                lo,
                span,
            );
        }
        // It follows the finger, and says so: a gesture read off the raw pointer — the rail's
        // fold on a swipe across the page — leaves this drag alone.
        crate::drag::claim_if_held(&response);
        let t = ((*self.value - lo) / span).clamp(0.0, 1.0);

        let pressed = self.enabled && response.is_pointer_button_down_on();
        // 80 ms out, 120 ms back — the shared press tokens. The slider's private `PRESS_MS`
        // is retired: one press timing for every control is the point of the token.
        let grow = cx.animate(
            response.id.with("press"),
            if pressed { 1.0 } else { 0.0 },
            if pressed {
                theme.motion.press
            } else {
                theme.motion.press_release
            },
        );
        let track_h = swollen(theme, base_track, grow);
        let focused = response.has_focus();
        if let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.slider.as_mut()) {
            let axis = value_axis(rect, base_track);
            let y = rect.center().y;
            let text = format_value(*self.value, span);
            custom(
                ui.painter(),
                &mut SliderLook {
                    rect,
                    track: Rect::from_min_max(
                        egui::pos2(rect.min.x, y - track_h / 2.0),
                        egui::pos2(rect.max.x, y + track_h / 2.0),
                    ),
                    axis,
                    value: *self.value,
                    range: self.range.clone(),
                    fraction: t,
                    handle: egui::pos2(axis.min.x + axis.width() * t, y),
                    value_text: &text,
                    style: self.style,
                    colors: (
                        self.colors.fill.resolve(theme),
                        self.colors.track.resolve(theme),
                        self.colors.handle.resolve(theme),
                    ),
                    pressed,
                    press: grow,
                    enabled: self.enabled,
                    focused,
                    theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }

        let (paint, label) = self.track_paint(theme, (base_track, track_h), grow);

        // The mover is the tallest part, so both the ring and the label measure off the silhouette
        // rather than off the track — off the track alone the ring is pierced and the label lands
        // on the mover's top edge.
        let silhouette = paint.mover.silhouette(track_h);
        if focused {
            paint_focus_ring(ui.painter(), rect, silhouette, theme);
        }
        let x = paint_track(ui.painter(), rect, t, paint);
        // The value label: only while held, disappearing on release by the same tween. At alpha 0
        // it is not built.
        if grow > 0.01 {
            let top = rect.center().y - silhouette / 2.0 - theme.components.slider.label_gap;
            ui.painter().text(
                egui::pos2(x, top),
                egui::Align2::CENTER_BOTTOM,
                format_value(*self.value, span),
                egui::TextStyle::Small.resolve(ui.style()),
                label.gamma_multiply(grow),
            );
        }
        response
    }

    /// The built-in track's paint at thickness `(base, drawn)` and press `grow`, and the value
    /// label's colour: the caller's colours, the press tint over both faces, one fade when off.
    fn track_paint(
        &self,
        theme: &Theme,
        (base, drawn): (f32, f32),
        grow: f32,
    ) -> (TrackPaint, Color32) {
        let mut paint = TrackPaint::from_theme(theme, base, drawn);
        paint.mover.style = self.style;
        paint.done = self.colors.fill.resolve(theme);
        paint.todo = self.colors.track.resolve(theme);
        paint.handle = self.colors.handle.resolve(theme);
        // `self.blend(on_top)` puts the **receiver behind**, so the face is the receiver.
        // Written the other way round the tint is multiplied to zero alpha over an opaque fill and
        // the press is invisible. The tint rides the same tween as the swell.
        let tint = theme.color(ColorRole::Pressed).gamma_multiply(grow);
        paint.done = paint.done.blend(tint);
        paint.todo = paint.todo.blend(tint);
        let mut label = theme.color(ColorRole::OnSurface);
        if !self.enabled {
            // One mechanism, one number, one call site — no role swaps.
            paint = paint.gamma_multiply(theme.control.disabled_alpha);
            label = label.gamma_multiply(theme.control.disabled_alpha);
        }
        (paint, label)
    }
}

/// The mover — the moving part that crosses both faces of the track (vocabulary).
///
/// It is a struct rather than four loose arguments to [`TrackPaint`] because the four move
/// together: a style change without the matching geometry draws a circle at a bar's diameter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mover {
    /// Which shape.
    pub style: HandleStyle,
    /// The body colour. `Surface` — the colour of the ground the control sits on, so the mover
    /// reads as a cap punched through the track.
    pub body: Color32,
    /// Its size **across** the track: `metrics.slider_thumb`, larger than the track, which is what
    /// makes it stand proud. Also the [`HandleStyle::Knob`] diameter.
    pub across: f32,
    /// Its size **along** the track: `control.slider_track × control.handle_ratio`. Narrow, so the
    /// mover marks the boundary without covering the filled area it is reporting.
    pub along: f32,
    /// The edge width — `control.stroke_edge`, the middle rung of the stroke ladder.
    pub edge: f32,
}

impl Mover {
    /// The vocabulary's mover, resolved against a theme.
    #[must_use]
    pub fn from_theme(theme: &Theme) -> Self {
        Self {
            style: HandleStyle::default(),
            // The ink of the filled side, not the container's ground — see [`SliderColors::handle`].
            body: theme.color(ColorRole::OnPrimary),
            across: theme.metrics.slider_thumb,
            along: theme.control.slider_track * theme.control.handle_ratio,
            edge: theme.control.stroke_edge,
        }
    }

    /// How tall the drawn control is, mover included — what a focus ring has to clear.
    ///
    /// A proud mover is the tallest thing on the track, so a ring measured off the track alone
    /// would be pierced by it.
    #[must_use]
    pub fn silhouette(self, track_h: f32) -> f32 {
        match self.style {
            HandleStyle::Bar | HandleStyle::Knob => track_h.max(self.across),
            HandleStyle::Inset | HandleStyle::None => track_h,
        }
    }
}

/// The arguments to [`paint_track`], bundled. Colours and thicknesses get their order confused loose.
///
/// **Public since the element split.** The shade's brightness control draws the same
/// track and used to reach in as a crate sibling; across a crate boundary that has to be a stated
/// part of the surface. Better stated than duplicated — two copies of the track arithmetic drift.
#[derive(Debug, Clone, Copy)]
pub struct TrackPaint {
    /// The **unswollen** thickness that fixes the value axis. Pressing does not shift where 0 % and 100 % are.
    pub base_track: f32,
    /// The thickness actually drawn (swollen when pressed).
    pub track_h: f32,
    /// The filled side.
    pub done: Color32,
    /// The remaining side.
    pub todo: Color32,
    /// The mover's **edge** (see [`SliderColors::handle`]); its body is `Mover::body`.
    pub handle: Color32,
    /// The track's own identifying edge — `Muted` at `control.stroke_edge`. `Stroke::NONE` draws
    /// none, which is what a decorative read-out track wants.
    pub edge: Stroke,
    /// The mover.
    pub mover: Mover,
}

impl TrackPaint {
    /// The vocabulary's track, resolved against a theme: `Primary` filled, `SurfaceVariant` empty,
    /// a `Muted` edge and the two-tone mover. Recolour the fields that differ afterwards, so a
    /// caller inherits every part it did not have an opinion about.
    #[must_use]
    pub fn from_theme(theme: &Theme, base_track: f32, track_h: f32) -> Self {
        Self {
            base_track,
            track_h,
            done: theme.color(ColorRole::Primary),
            todo: theme.color(ColorRole::SurfaceVariant),
            handle: theme.color(ColorRole::OnSurface),
            edge: Stroke::new(
                theme.control.stroke_edge,
                theme.color(ColorRole::ControlEdge),
            ),
            mover: Mover::from_theme(theme),
        }
    }

    /// Every colour dimmed by one alpha — the disabled path.
    ///
    /// One multiply over the whole paint, rather than a role swap per part: a swap loses the live
    /// state (a disabled slider must still read at its value) and on a palette where `Muted` sits
    /// near `OnSurface` it dims some parts invisibly and others plainly.
    #[must_use]
    pub fn gamma_multiply(mut self, alpha: f32) -> Self {
        self.done = self.done.gamma_multiply(alpha);
        self.todo = self.todo.gamma_multiply(alpha);
        self.handle = self.handle.gamma_multiply(alpha);
        self.edge.color = self.edge.color.gamma_multiply(alpha);
        self.mover.body = self.mover.body.gamma_multiply(alpha);
        self
    }
}

/// Draw the track and the mover, and return **the value's x** (unclamped by the mover's width).
///
/// No `#[must_use]`: the returned x is for a caller that wants to hang something off the mover
/// (the slider's own value label does), and the shade draws the track for its own sake and has no
/// use for it. Marking it would make the ordinary call the one that needs an `let _ =`.
///
/// The one place [`TouchSlider`] and the shade's Slider expanded row share **the same drawing**.
/// Two copies drift — and they had. The shade drew two line segments and a disc with a hard
/// `4.0` thickness, so it did not follow the slider's own thickness token, and changing that token
/// thickened only the settings screen's sliders.
#[expect(
    clippy::must_use_candidate,
    reason = "it is a draw call first; the returned x is a convenience for a caller hanging a label off the mover"
)]
pub fn paint_track(painter: &egui::Painter, rect: Rect, t: f32, p: TrackPaint) -> f32 {
    let axis = value_axis(rect, p.base_track);
    let y = rect.center().y;
    let x = axis.min.x + axis.width() * t.clamp(0.0, 1.0);
    let full = Rect::from_min_max(
        egui::pos2(rect.min.x, y - p.track_h / 2.0),
        egui::pos2(rect.max.x, y + p.track_h / 2.0),
    );
    let radius = CornerRadius::same(round_u8(p.track_h / 2.0));
    // The empty track is laid down whole and the filled part drawn over it — cutting the two separately
    // leaves a half-pixel gap at the seam.
    painter.rect_filled(full, radius, p.todo);
    // The filled side is a pill from the track's end to `x`, **squeezed into the track's own cap**
    // ([`fill_polygon`]). It was a rounded rect, and a rounded rect narrower than the track cannot
    // carry the track's radius: epaint clamps the corner to half the width, the corners go nearly
    // square, and at a few per cent the fill stood out of the capsule's curved end top and bottom
    // (user report). The polygon has the same outline wherever the fill is wider than the track,
    // so nothing above a few per cent changes. At 0 % the value axis starts half a track in, so
    // what remains is the cap's own half-disc — the visible accent cap the old two-edge-width
    // floor was there to keep, no longer under the edge stroke.
    painter.add(egui::Shape::convex_polygon(
        fill_polygon(full, x),
        p.done,
        Stroke::NONE,
    ));
    if p.edge.width > 0.0 {
        // The identifying boundary of the empty side. `Outline` is never a control's edge — it is
        // 1.15–1.65 against both grounds, a ghost — and `Muted` is text at 4.5, which drew this
        // line as loud as the label beside it. `ControlEdge` is the same job at WCAG 1.4.11's own
        // 3.0 floor (Rule 3.1).
        painter.rect_stroke(full, radius, p.edge, StrokeKind::Inside);
    }
    paint_mover(painter, egui::pos2(x, y), &p, rect);
    x
}

/// Segments per quarter arc of the filled side's outline.
const FILL_SEGS: usize = 8;

/// **The filled side, as a convex polygon**: the pill `[full.min.x, x]`, its ends rounded as far as
/// its own width allows, squeezed into the track's capsule so no corner of it shows outside the
/// cap it sits in.
///
/// Wherever the fill is at least as wide as the track is tall, the pill's radius is the track's
/// and nothing is squeezed — the outline is exactly the rounded rect that was drawn before.
/// Narrower than that, the pill's corners round to half its width and would stand outside the
/// cap's curve; [`into_cap`] moves each point back onto the curve, and the shape stays convex
/// because both the pill and the cap are.
fn fill_polygon(full: Rect, x: f32) -> Vec<Pos2> {
    let radius = full.height() / 2.0;
    let cy = full.center().y;
    let x = x.clamp(full.min.x, full.max.x);
    let width = x - full.min.x;
    let corner = radius.min(width / 2.0);
    let cap = egui::pos2(full.min.x + radius, cy);
    // The four corner arcs, clockwise from the top right, each a quarter turn from its start.
    let corners = [
        (egui::pos2(x - corner, cy - radius + corner), -FRAC_PI_2),
        (egui::pos2(x - corner, cy + radius - corner), 0.0),
        (
            egui::pos2(full.min.x + corner, cy + radius - corner),
            FRAC_PI_2,
        ),
        (egui::pos2(full.min.x + corner, cy - radius + corner), PI),
    ];
    let mut points = Vec::with_capacity(corners.len() * (FILL_SEGS + 1));
    for (centre, start) in corners {
        for i in 0..=FILL_SEGS {
            #[expect(clippy::cast_precision_loss, reason = "a handful of segments")]
            let angle = start + FRAC_PI_2 * (i as f32 / FILL_SEGS as f32);
            let point = egui::pos2(
                centre.x + corner * angle.cos(),
                centre.y + corner * angle.sin(),
            );
            points.push(into_cap(point, cap, radius));
        }
    }
    points
}

/// `point` moved inside the track's left cap — the disc of `radius` about `cap` — where it lay
/// outside it. Points past the cap's centre are already in the track's straight part.
fn into_cap(point: Pos2, cap: Pos2, radius: f32) -> Pos2 {
    if point.x >= cap.x {
        return point;
    }
    let dx = cap.x - point.x;
    let reach = radius.mul_add(radius, -(dx * dx)).max(0.0).sqrt();
    egui::pos2(point.x, cap.y + (point.y - cap.y).clamp(-reach, reach))
}

/// The mover, centred on the value. `slot` only clamps the drawing: a [`HandleStyle::Knob`] is
/// wider than the axis inset, so at 0 % and 100 % it would otherwise hang outside the rect the
/// widget allocated and land on its neighbour. The value's x is left alone, so the label and the
/// filled boundary still tell the truth at the ends.
fn paint_mover(painter: &egui::Painter, at: egui::Pos2, p: &TrackPaint, slot: Rect) {
    let m = p.mover;
    let stroke = Stroke::new(m.edge, p.handle);
    let (along, across) = match m.style {
        HandleStyle::None => return,
        // The knob's stroke is centred on its radius, so the outer edge lands on `across`.
        HandleStyle::Knob => {
            let half = (m.across - m.edge).max(m.edge) / 2.0;
            let x = held_inside(at.x, slot, m.across);
            painter.circle(egui::pos2(x, at.y), half, m.body, stroke);
            return;
        }
        HandleStyle::Bar => (m.along, m.across),
        // Inside the track and clear of the track's own edge, which is what "inset" has to mean
        // once the track has an edge at all. It follows the swell, as the shipped bar did.
        HandleStyle::Inset => (m.along, (p.track_h - 2.0 * m.edge).max(m.edge)),
    };
    let x = held_inside(at.x, slot, along);
    let body = Rect::from_center_size(egui::pos2(x, at.y), Vec2::new(along, across));
    painter.rect(
        body,
        CornerRadius::same(round_u8(along.min(across) / 2.0)),
        m.body,
        stroke,
        StrokeKind::Inside,
    );
}

/// A shape of `width` held inside `slot`, centred where it does not fit.
///
/// The bounds are compared before the clamp because `f32::clamp` **panics** when its low bound is
/// above its high one, and a slider laid in a column narrower than its own mover is a real case
/// (a two-pane split at the narrow end, a gauge row beside a long label). A control that panics on
/// a narrow column is worse than one that draws its mover a little off the value.
fn held_inside(x: f32, slot: Rect, width: f32) -> f32 {
    let (lo, hi) = (slot.min.x + width / 2.0, slot.max.x - width / 2.0);
    if lo > hi {
        slot.center().x
    } else {
        x.clamp(lo, hi)
    }
}

/// The focus ring: `Focus` at `control.stroke_mark`, `control.focus_gap` clear of the silhouette.
///
/// **Outside is a contrast decision.** `Focus` against `Primary` measures 1.00–1.62, so a ring on
/// the control's own edge fails 3.0 in every palette; against the container it is 3.10–11.53.
///
/// Vertically it clears the silhouette — the mover included, since the mover is the tallest part.
/// Horizontally it takes the allocated rect's own edge instead: a full-width track has nothing
/// outside itself to breathe into, and a ring painted past the rect lands on the neighbouring row.
/// The gap that has to be seen is the one across the track, and that one is kept.
fn paint_focus_ring(painter: &egui::Painter, slot: Rect, silhouette: f32, theme: &Theme) {
    let width = theme.control.stroke_mark;
    let half = silhouette / 2.0 + theme.control.focus_gap + width / 2.0;
    let ring = Rect::from_min_max(
        egui::pos2(slot.min.x + width / 2.0, slot.center().y - half),
        egui::pos2(slot.max.x - width / 2.0, slot.center().y + half),
    );
    painter.rect_stroke(
        ring,
        CornerRadius::same(round_u8(ring.height().min(ring.width()) / 2.0)),
        Stroke::new(width, theme.color(ColorRole::Focus)),
        StrokeKind::Middle,
    );
}

/// The value axis — **inside the track's rounded ends**, so the filled cap shows at 0 % and the
/// boundary does not pass the end at 100 %. It uses the **unswollen** thickness, so pressing does
/// not move where 0 % and 100 % are.
#[must_use]
pub(crate) fn value_axis(rect: Rect, base_track: f32) -> Rect {
    rect.shrink2(Vec2::new(base_track / 2.0, 0.0))
}

/// The value follows the finger from the moment of the press — a tap and a drag take the same
/// path (A7, "the value is 1:1"). Marks the response changed on the frame it moves.
fn follow_finger(response: &mut Response, axis: Rect, value: &mut f32, lo: f32, span: f32) {
    if !(response.is_pointer_button_down_on() || response.dragged() || response.clicked()) {
        return;
    }
    let Some(pos) = response.interact_pointer_pos() else {
        return;
    };
    let t = ((pos.x - axis.min.x) / axis.width().max(1.0)).clamp(0.0, 1.0);
    let next = span.mul_add(t, lo);
    // Written so a value that is not a number (a bad reading upstream) counts as different:
    // a touch is how the operator puts it right.
    let same = (next - *value).abs() <= f32::EPSILON;
    if !same {
        *value = next;
        response.mark_changed();
    }
}

/// The press swell — [`TouchSlider`] and the shade use the same factor, `control.press_swell`.
///
/// It takes the theme because the factor is a token now: the private `PRESS_SWELL` constant it
/// used to read could not be reached by an integrator, and press feedback that cannot be tuned
/// per panel is the one piece of feedback a glove hides.
#[must_use]
pub fn swollen(theme: &Theme, base_track: f32, grow: f32) -> f32 {
    base_track * (theme.control.press_swell - 1.0).mul_add(grow, 1.0)
}

/// The track's base thickness. The shade uses this too — this one place is the only source.
///
/// It is `control.slider_track` directly. It used to be `slider_thumb × track_ratio`, a derivation
/// in which the **thumb** silently set the **track**: changing the mover's size changed the thing
/// it moves along. The token reproduces that value at both ends of the scale, so this is a rename.
#[must_use]
pub fn base_thickness(theme: &Theme) -> f32 {
    theme.control.slider_track
}

/// The value string. Two decimal places over a narrow range, an integer over a wide one.
fn format_value(value: f32, span: f32) -> String {
    if span <= 2.0 {
        format!("{value:.2}")
    } else if span <= 20.0 {
        format!("{value:.1}")
    } else {
        format!("{value:.0}")
    }
}

#[cfg(test)]
mod tests {
    use super::{fill_polygon, Rect};

    /// A track 24 tall, from 10 to 310.
    fn track() -> Rect {
        Rect::from_min_max(egui::pos2(10.0, 100.0), egui::pos2(310.0, 124.0))
    }

    /// Whether `p` is inside the track's capsule (its left cap and its straight part).
    fn in_capsule(full: Rect, p: egui::Pos2) -> bool {
        let r = full.height() / 2.0;
        let (cx, cy) = (full.min.x + r, full.center().y);
        let dy = (p.y - cy).abs();
        if p.x < cx {
            let dx = cx - p.x;
            dx * dx + dy * dy <= r * r + 0.01
        } else {
            dy <= r + 0.01
        }
    }

    /// The bug: at a few per cent the rounded rect's corners stood outside the cap. The polygon
    /// never does, at any width from a sliver to the whole track.
    #[test]
    fn the_filled_side_never_leaves_the_capsule() {
        let full = track();
        for w in [0.5, 2.0, 5.0, 8.0, 12.0, 20.0, 23.9, 24.0, 40.0, 300.0] {
            let x = full.min.x + w;
            let pts = fill_polygon(full, x);
            assert!(pts.len() >= 4, "a polygon: {pts:?}");
            for p in &pts {
                assert!(
                    in_capsule(full, *p),
                    "width {w}: {p:?} is outside the track's capsule"
                );
                assert!(p.x <= x + 0.01, "width {w}: {p:?} is past the value at {x}");
            }
            let right = pts.iter().map(|p| p.x).fold(f32::MIN, f32::max);
            assert!(
                (right - x).abs() < 0.01,
                "width {w}: the fill reaches the value: {right} vs {x}"
            );
        }
    }

    /// Wider than the track is tall, the fill is the pill it always was: full height, the track's
    /// radius at both ends, and nothing moved.
    #[test]
    fn past_the_cap_the_fill_is_the_pill_it_was() {
        let full = track();
        let r = full.height() / 2.0;
        let x = full.min.x + 100.0;
        let pts = fill_polygon(full, x);
        let top = pts.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let bottom = pts.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!((top - full.min.y).abs() < 0.01 && (bottom - full.max.y).abs() < 0.01);
        for p in &pts {
            // Every point is on the pill's outline: within `r` of one of its two end centres, or
            // on one of its two long sides.
            let on_side = (p.y - full.min.y).abs() < 0.01 || (p.y - full.max.y).abs() < 0.01;
            let left = egui::pos2(full.min.x + r, full.center().y);
            let right_c = egui::pos2(x - r, full.center().y);
            let on_arc = ((p.distance(left) - r).abs() < 0.01 && p.x <= left.x + 0.01)
                || ((p.distance(right_c) - r).abs() < 0.01 && p.x >= right_c.x - 0.01);
            assert!(on_side || on_arc, "{p:?} is not on the pill's outline");
        }
    }
    use super::{
        base_thickness, format_value, held_inside, swollen, value_axis, HandleStyle, Mover,
    };
    use crate::theme::{contrast, Palette, Preset, Theme};
    use crate::unit::Scale;

    /// **The mover is visible on both sides of the boundary it sits on, in every palette.**
    ///
    /// Unlike the switch's knob, this one straddles the two faces at once — it marks the seam — so
    /// it cannot cross-fade between them: the body carries the filled side and the edge carries the
    /// empty one. The body used to be `Surface`, the container's own ground, which is near-black in
    /// a dark palette; the mover then read as a gap in the track. It is now `OnPrimary`, and both
    /// pairs below are ones `every_preset_meets_the_contrast_floor` already gates.
    #[test]
    fn the_mover_reads_on_both_sides_of_the_boundary() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                for (part, ink, face) in [
                    ("body over the filled side", p.on_primary, p.primary),
                    ("edge over the empty side", p.on_surface, p.surface_variant),
                ] {
                    let got = contrast(ink, face);
                    assert!(got >= 3.0, "{name} {mode}: the {part} measures {got:.2}");
                }
            }
        }
    }

    #[test]
    fn value_label_precision_follows_the_range() {
        assert_eq!(format_value(0.5, 1.0), "0.50");
        assert_eq!(format_value(7.24, 10.0), "7.2");
        assert_eq!(format_value(70.4, 100.0), "70");
    }

    /// The axis is inset by half the **unswollen** track at each end, so 0 % and 100 % do not move
    /// while the track swells under a finger.
    #[test]
    fn the_value_axis_does_not_move_when_the_track_swells() {
        let rect = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 80.0));
        let axis = value_axis(rect, 30.0);
        assert!((axis.min.x - 15.0).abs() < f32::EPSILON);
        assert!((axis.max.x - 385.0).abs() < f32::EPSILON);
    }

    /// A theme whose metrics are resolved at the reference scale. `Theme::default()` alone mixes
    /// the two: `Metrics::default()` is the frozen legacy table while the token specs resolve, and
    /// the mover's diameter comes from `Metrics` while the track comes from the tokens.
    fn reference_theme() -> Theme {
        let mut theme = Theme::default();
        theme.metrics = crate::theme::MetricsSpec::default().resolve(&Scale::identity());
        theme
    }

    /// The mover stands proud: it is taller than the track it rides, at rest and pressed alike, so
    /// it is what a focus ring has to clear.
    #[test]
    fn the_default_mover_stands_proud_of_its_track() {
        let theme = reference_theme();
        let base = base_thickness(&theme);
        let mover = Mover::from_theme(&theme);
        assert_eq!(mover.style, HandleStyle::Bar);
        assert!(
            mover.across > base,
            "{} is not proud of {base}",
            mover.across
        );
        assert!(
            mover.along < base,
            "the mover must be narrow along the track"
        );
        assert!(mover.silhouette(base) > base);
        assert!((mover.silhouette(base) - mover.across).abs() < f32::EPSILON);
    }

    /// A slot narrower than the mover centres it instead of inverting the clamp — `f32::clamp`
    /// panics when it is handed a low bound above its high one.
    #[test]
    fn a_slot_narrower_than_the_mover_does_not_invert_the_clamp() {
        let slot = egui::Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(12.0, 80.0));
        assert!((held_inside(0.0, slot, 48.0) - slot.center().x).abs() < f32::EPSILON);
        let wide = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(400.0, 80.0));
        assert!((held_inside(0.0, wide, 48.0) - 24.0).abs() < f32::EPSILON);
    }

    /// An inset mover is contained, so the silhouette is the track alone — a ring drawn off it
    /// would otherwise float clear of anything.
    #[test]
    fn a_contained_mover_leaves_the_silhouette_at_the_track() {
        let mut mover = Mover::from_theme(&Theme::default());
        for style in [HandleStyle::Inset, HandleStyle::None] {
            mover.style = style;
            assert!((mover.silhouette(30.0) - 30.0).abs() < f32::EPSILON);
        }
    }

    /// Pressing only ever grows the track: press geometry never shrinks.
    #[test]
    fn the_press_swell_only_grows() {
        let theme = reference_theme();
        let base = base_thickness(&theme);
        assert!((swollen(&theme, base, 0.0) - base).abs() < f32::EPSILON);
        assert!(swollen(&theme, base, 1.0) > base);
        assert!(swollen(&theme, base, 0.5) >= base);
    }
}
