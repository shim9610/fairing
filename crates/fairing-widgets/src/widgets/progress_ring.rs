//! `ProgressRing` — a ring that reports, determinate or not, with room in the middle for the
//! number.
//!
//! # Why a ring is not the bar bent round
//!
//! `ProgressBar` is a strip beside a label, read in a list. A ring is read **from across the
//! bench**: it sits alone in a tile or a hero, the number sits inside it, and the band has to be
//! thick enough that the eye takes the fraction in without stopping. So it has its own stroke
//! token (`control.ring_ratio`, over its own diameter), its own centre slot, and its own
//! way of saying "still going".
//!
//! # Why the fill is a sweep of colour and not a flat arc
//!
//! On a flat arc the only thing that says which end is the *head* is the position of the cut,
//! and at 63 % that is a matter of remembering where twelve o'clock was. The reference draws the
//! band from a pale tint at the start to the full accent at the head, and a soft halo around the
//! filled part, so the direction of travel is in the colour: the eye finds the head first. That is
//! [`RingStyle::Sweep`], the default. [`RingStyle::Flat`] is the Material shape, for a panel that
//! wants the quieter one.
//!
//! A gradient along an arc is not something a stroke can draw. The band is a **mesh** with a
//! colour on every vertex, which epaint interpolates — the same way the desktop paints its
//! gradient wallpaper. The mesh is cached against the ring's geometry and its quantised value, so
//! a ring that is not moving costs no allocation and a moving one costs one rebuild per frame.
//!
//! # Why the shown value is eased, and never goes backwards
//!
//! Apple's guidance is to *even out* the pace of a determinate indicator, and Fluent's is never to
//! let one rewind; both are about the same thing, which is that a bar is trusted only while it
//! behaves like a clock. A worker reporting in bursts would have the ring jump; a worker that
//! restarts a phase would have it snap back. So the ring eases towards each reported value over
//! `motion.switch`, and holds the highest value it has shown unless the caller says a rewind is
//! meant ([`ProgressRing::allow_rewind`]) — a *new run* is a rewind that is meant.
//!
//! And it reports silence the way the bar does: past [`ProgressRing::stale_after`]
//! with no new value and no [`ProgressRing::heartbeat`], the band breathes and the centre value
//! dims. The stall does not change the tone — that is the caller's judgement to make.
//!
//! # Why the read-out is the caller's string
//!
//! As for the bar: `63 %`, `63%`, `12.4 L`, `3 / 4` — the crate lays the number out and
//! does not decide what it is.

use super::{ProgressFill as Fill, ProgressRingLook};
use crate::cx::WidgetCx as Cx;
use crate::theme::ColorRole;
use crate::widgets::progress::{
    beat_of, eased_value, pulse_alpha, segment_span, stall_alpha, stalled,
};
use egui::{Color32, Pos2, Rect, Response, Sense, Vec2};
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::Arc;
use std::time::Duration;

/// How the filled part of the band is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum RingStyle {
    /// A sweep from a pale tint at the start to the full tone at the head, with a soft halo. The
    /// default — the head is found by colour, not by remembering where the start was.
    #[default]
    Sweep,
    /// A solid band with round caps. The Material shape.
    Flat,
}

/// The pale end of the sweep, as the tone's alpha.
const SWEEP_TAIL_ALPHA: f32 = 0.42;
/// How much of the hole's radius the centre text may use.
const CENTRE_FIT: f32 = 0.92;
/// The smallest text worth drawing in the centre (px). Below it a line is left out, not smeared.
const MIN_TEXT: f32 = 5.0;
/// How finely a stalled band's breathing is quantised (steps of alpha per unit).
const STALL_STEPS: f32 = 24.0;
/// The halo's alpha at its brightest.
const HALO_ALPHA: f32 = 0.20;
/// The halo's width over the band's.
const HALO_WIDTH: f32 = 1.9;
/// The centre number's size over the ring's diameter. Measured off the console reference: a
/// cap height 0.22 of the diameter, which is a font size of about 0.30.
const VALUE_SIZE: f32 = 0.30;
/// The arc is quantised to this many steps around the circle for the mesh cache key.
const CACHE_STEPS: f32 = 720.0;
/// The most segments a band is built from.
const BAND_MAX: usize = 180;
/// The indeterminate arc's length as a fraction of the circle, at its shortest and longest.
const INDET_MIN: f32 = 0.10;
const INDET_MAX: f32 = 0.55;

/// A progress ring.
#[derive(Debug)]
pub struct ProgressRing<'a> {
    /// `None` is indeterminate. Set only by the two constructors.
    value: Option<f32>,
    value_text: Option<&'a str>,
    label: Option<&'a str>,
    diameter: Option<f32>,
    thickness: Option<f32>,
    gap_degrees: f32,
    style: RingStyle,
    tone: ColorRole,
    enabled: bool,
    allow_rewind: bool,
    stale_after: Option<Duration>,
    heartbeat: Option<u64>,
    salt: &'a str,
    /// How often an indeterminate ring repaints; `None` is every frame.
    repaint_every: Option<Duration>,
}

impl<'a> ProgressRing<'a> {
    /// A ring at `value`, 0 to 1.
    ///
    /// Out of range is clamped and **non-finite reads as empty**, for the bar's reason: `NaN` in a
    /// vertex is a mesh the tessellator drops, and the ring would lose its track with its fill.
    #[must_use]
    pub fn determinate(value: f32) -> Self {
        Self {
            value: Some(if value.is_finite() {
                value.clamp(0.0, 1.0)
            } else {
                0.0
            }),
            value_text: None,
            label: None,
            diameter: None,
            thickness: None,
            gap_degrees: 0.0,
            style: RingStyle::default(),
            tone: ColorRole::Primary,
            enabled: true,
            allow_rewind: false,
            stale_after: None,
            heartbeat: None,
            salt: "fairing.progress_ring",
            repaint_every: None,
        }
    }

    /// A ring for work whose extent is not known: an arc that grows, shrinks and goes round.
    #[must_use]
    pub fn indeterminate() -> Self {
        Self {
            value: None,
            ..Self::determinate(0.0)
        }
    }

    /// **Repaint an indeterminate ring at most this often**, rather than every frame — for a
    /// ring that may go round for hours (a badge reader's wait on a lock screen), where sixty
    /// frames a second all night is heat and power spent on nothing. The arc keeps its pace; it
    /// only moves in coarser steps. Nothing for a determinate ring, which repaints on change.
    #[must_use]
    pub fn repaint_every(mut self, interval: Duration) -> Self {
        self.repaint_every = Some(interval);
        self
    }

    /// The number in the middle, already formatted and already in the caller's locale.
    #[must_use]
    pub fn value_text(mut self, text: &'a str) -> Self {
        self.value_text = Some(text);
        self
    }

    /// A word under the number — what the ring is reporting on.
    #[must_use]
    pub fn label(mut self, text: &'a str) -> Self {
        self.label = Some(text);
        self
    }

    /// The outer diameter (du). `metrics.tile_size` by default, which is a ring that fills a
    /// quick tile; a hero wants more.
    #[must_use]
    pub fn diameter(mut self, du: f32) -> Self {
        self.diameter = Some(du.max(0.0));
        self
    }

    /// Override the band's thickness (du). `control.ring_stroke(diameter)` by default.
    #[must_use]
    pub fn thickness(mut self, du: f32) -> Self {
        self.thickness = Some(du.max(0.0));
        self
    }

    /// **Open the ring at the bottom** by this many degrees, which makes it a gauge.
    ///
    /// 75° is the dashboard convention; 180° is a speedometer. Clamped to `0..=300` — past that
    /// there is no arc left to read.
    #[must_use]
    pub fn gap_degrees(mut self, degrees: f32) -> Self {
        self.gap_degrees = if degrees.is_finite() {
            degrees.clamp(0.0, 300.0)
        } else {
            0.0
        };
        self
    }

    /// Sweep or flat. See [`RingStyle`].
    #[must_use]
    pub fn style(mut self, style: RingStyle) -> Self {
        self.style = style;
        self
    }

    /// The band's colour. [`Primary`](ColorRole::Primary) by default; the bar's other three are
    /// the ones this is measured for.
    #[must_use]
    pub fn tone(mut self, role: ColorRole) -> Self {
        self.tone = role;
        self
    }

    /// Enabled. A ring senses `hover()` either way — it reports and is not pressed — so this
    /// changes only the ink.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// **Let the shown value go down.** Off by default: a ring holds the highest value it has
    /// shown, because a reading that goes backwards is a reading the operator stops trusting. A
    /// new run is the case this is for — say so, and the ring follows the value down.
    #[must_use]
    pub fn allow_rewind(mut self, allow: bool) -> Self {
        self.allow_rewind = allow;
        self
    }

    /// Report a stall when nothing has been heard for this long — the bar's rule
    /// ([`ProgressBar::stale_after`](crate::widgets::ProgressBar::stale_after)): the band
    /// breathes and the centre value dims until the next word.
    #[must_use]
    pub fn stale_after(mut self, after: Duration) -> Self {
        self.stale_after = Some(after);
        self
    }

    /// The worker's "still alive" counter, so a slow step is not a stall
    /// ([`ProgressBar::heartbeat`](crate::widgets::ProgressBar::heartbeat)).
    #[must_use]
    pub fn heartbeat(mut self, beat: u64) -> Self {
        self.heartbeat = Some(beat);
        self
    }

    /// Separate two rings that would otherwise share their eased value and cache. Only needed
    /// where two are drawn in the same `Ui` scope.
    #[must_use]
    pub fn id_salt(mut self, salt: &'a str) -> Self {
        self.salt = salt;
        self
    }

    /// Draw it.
    pub fn show(self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Response {
        let m = &cx.theme.metrics;
        let diameter = self.diameter.unwrap_or(m.tile_size).max(1.0);
        let thickness = self
            .thickness
            .unwrap_or_else(|| cx.theme.control.ring_stroke(diameter))
            .clamp(1.0, diameter * 0.5);
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::hover());
        // The auto id, so two rings in one `Ui` keep separate memories.
        let id = response.id.with(self.salt);
        let geom = Geometry {
            center: rect.center(),
            radius: (diameter - thickness) * 0.5,
            width: thickness,
            start: start_angle(self.gap_degrees),
            sweep: TAU - self.gap_degrees.to_radians(),
            feather: 1.0 / ui.ctx().pixels_per_point().max(0.5),
        };
        let mut ink = Ink::of(cx, self.tone, self.enabled);

        let fill = match self.value {
            Some(target) => Fill::To(eased_value(ui, cx, id, target, self.allow_rewind)),
            None => indeterminate(ui, cx, self.repaint_every),
        };
        let stale = self.stale_after.and_then(|after| {
            let beat = beat_of(self.value, self.heartbeat);
            // Quantised, so the breathing band rebuilds its mesh a few times a second and not
            // once a frame (the cache is keyed on the fill colour).
            stalled(ui, id.with("beat"), beat, after)
                .map(|since| (stall_alpha(ui, cx, since) * STALL_STEPS).round() / STALL_STEPS)
        });
        if let Some(alpha) = stale {
            ink.fill = ink.fill.gamma_multiply(alpha);
            ink.value = ink.value.gamma_multiply(cx.theme.control.disabled_alpha);
            ink.label = ink.label.gamma_multiply(cx.theme.control.disabled_alpha);
        }
        if let Some(custom) = cx
            .painters
            .as_deref_mut()
            .and_then(|p| p.progress_ring.as_mut())
        {
            custom(
                ui.painter(),
                &mut ProgressRingLook {
                    rect,
                    center: geom.center,
                    radius: geom.radius,
                    thickness: geom.width,
                    start: geom.start,
                    sweep: geom.sweep,
                    fill,
                    value_text: self.value_text,
                    label: self.label,
                    style: self.style,
                    tone: self.tone,
                    stale,
                    enabled: self.enabled,
                    theme: cx.theme,
                    icons: &mut *cx.icons,
                },
            );
            return response;
        }

        // The track first, always the whole arc: an empty ring is still a ring. A gauge's track
        // has round ends like the fill's, so the two line up.
        let ends = if geom.closes() {
            Ends::Flat
        } else {
            Ends::Round
        };
        let track = band(&geom, 0.0, 1.0, |_| ink.track, HaloNone, ends);
        ui.painter().add(egui::Shape::mesh(track));
        match fill {
            Fill::To(t) | Fill::Segment(0.0, t) if t <= 0.0 => {}
            Fill::To(t) => self.paint_fill(ui, id, &geom, ink, 0.0, t),
            Fill::Segment(a, b) => self.paint_fill(ui, id, &geom, ink, a, b),
            Fill::Pulse(alpha) => {
                let mesh = band(
                    &geom,
                    0.0,
                    1.0,
                    |_| ink.fill.gamma_multiply(alpha),
                    HaloNone,
                    ends,
                );
                ui.painter().add(egui::Shape::mesh(mesh));
            }
        }
        self.paint_centre(ui, cx, rect, geom.radius - geom.width * 0.5, ink);
        response
    }

    /// The filled part: the halo under it where the style has one, then the band with its caps.
    fn paint_fill(
        &self,
        ui: &egui::Ui,
        id: egui::Id,
        geom: &Geometry,
        ink: Ink,
        from: f32,
        to: f32,
    ) {
        // **Cached against what it depends on.** A ring that is not moving rebuilds nothing; a
        // moving one rebuilds once per frame. `to` is quantised to a step around the circle so the
        // eased value's last few thousandths do not defeat the cache.
        let key = CacheKey {
            center: geom.center.round(),
            radius: geom.radius.round(),
            width: geom.width.round(),
            from: quantise(from),
            to: quantise(to),
            gap: quantise(geom.sweep / TAU),
            fill: ink.fill,
            style: self.style,
        };
        let cache_id = id.with("mesh");
        let hit = ui
            .data(|d| d.get_temp::<(CacheKey, Arc<egui::Mesh>)>(cache_id))
            .filter(|(k, _)| *k == key)
            .map(|(_, m)| m);
        let mesh = hit.unwrap_or_else(|| {
            let mesh = Arc::new(self.build_fill(geom, ink, from, to));
            ui.data_mut(|d| d.insert_temp(cache_id, (key, Arc::clone(&mesh))));
            mesh
        });
        ui.painter().add(egui::Shape::mesh(mesh));
    }

    fn build_fill(&self, geom: &Geometry, ink: Ink, from: f32, to: f32) -> egui::Mesh {
        let tail = ink.fill.gamma_multiply(SWEEP_TAIL_ALPHA);
        let colour_at = |k: f32| match self.style {
            RingStyle::Sweep => lerp_colour(tail, ink.fill, k),
            RingStyle::Flat => ink.fill,
        };
        // A ring that closes has no ends to round: at 100 % with no gap the head meets the tail.
        let ends = if geom.closes() && to - from >= 1.0 - 1e-4 {
            Ends::Flat
        } else {
            Ends::Round
        };
        match self.style {
            RingStyle::Sweep => band(geom, from, to, colour_at, Halo(ink.fill), ends),
            RingStyle::Flat => band(geom, from, to, colour_at, HaloNone, ends),
        }
    }

    /// The number and the word, centred, each sized to the **chord** of the hole where it sits.
    ///
    /// The hole is round: a line below the centre has less room than one through it, and the
    /// word under the number is exactly there. So each line is measured against the chord at its
    /// farther edge, and shrunk to it — the number first, then the word — rather than against the
    /// hole's full width, which the first render of a small ring showed the word overrunning.
    fn paint_centre(&self, ui: &egui::Ui, cx: &Cx<'_>, rect: Rect, inner_r: f32, ink: Ink) {
        let Some(text) = self.value_text else {
            return;
        };
        let centre = rect.center();
        let room = inner_r * CENTRE_FIT;
        // The widest a line may be when its farther edge is `dy` from the centre.
        let chord = |dy: f32| {
            if dy >= room {
                0.0
            } else {
                2.0 * ((room - dy) * (room + dy)).sqrt()
            }
        };
        let gap = cx.theme.control.line_gap * 0.5;
        let lay = |text: &str, font: egui::FontId, colour: Color32| {
            ui.painter().layout_no_wrap(text.to_owned(), font, colour)
        };
        let mut value_size = rect.width() * VALUE_SIZE;
        let mut label_size = cx.theme.metrics.type_scale.small;
        let mut value = lay(text, cx.theme.display(value_size), ink.value);
        let mut label = self
            .label
            .map(|l| lay(l, egui::FontId::proportional(label_size), ink.label));

        // Where the lines sit at these sizes, and how much each is over its chord.
        let block_h = value.rect.height() + label.as_ref().map_or(0.0, |g| gap + g.rect.height());
        let top = centre.y - block_h * 0.5;
        let far = |y0: f32, y1: f32| (y0 - centre.y).abs().max((y1 - centre.y).abs());
        let value_room = chord(far(top, top + value.rect.height()));
        let label_room = label.as_ref().map_or(0.0, |g| {
            let y0 = top + value.rect.height() + gap;
            chord(far(y0, y0 + g.rect.height()))
        });
        let mut shrunk = false;
        if value.rect.width() > value_room {
            value_size *= value_room / value.rect.width().max(1.0);
            shrunk = true;
        }
        if let Some(g) = &label {
            if g.rect.width() > label_room {
                label_size *= label_room / g.rect.width().max(1.0);
                shrunk = true;
            }
        }
        if shrunk {
            // Smaller lines make a shorter block, which sits nearer the centre where the chord is
            // wider — so one pass is enough, and nothing can end up outside.
            value = lay(text, cx.theme.display(value_size.max(MIN_TEXT)), ink.value);
            label = self
                .label
                .filter(|_| label_size >= MIN_TEXT)
                .map(|l| lay(l, egui::FontId::proportional(label_size), ink.label));
        }

        let block_h = value.rect.height() + label.as_ref().map_or(0.0, |g| gap + g.rect.height());
        let top = centre.y - block_h * 0.5;
        ui.painter().galley(
            Pos2::new(centre.x - value.rect.width() * 0.5, top),
            value.clone(),
            ink.value,
        );
        if let Some(label) = label {
            ui.painter().galley(
                Pos2::new(
                    centre.x - label.rect.width() * 0.5,
                    top + value.rect.height() + gap,
                ),
                label,
                ink.label,
            );
        }
    }
}

/// **Where the arc starts.** A ring starts at twelve o'clock, which is where every clock and every
/// progress ring since has started. A gauge — a ring with a gap — starts at the gap's clockwise
/// edge, so the arc runs up one side, over the top and down the other, with the opening at the
/// bottom: the dashboard convention. The two are different rules, and a formula that slid from one
/// to the other as the gap closed would put a plain ring's start at six o'clock, which is what the
/// first render of this did.
fn start_angle(gap_degrees: f32) -> f32 {
    if gap_degrees > 0.0 {
        FRAC_PI_2 + gap_degrees.to_radians() * 0.5
    } else {
        -FRAC_PI_2
    }
}

/// The ring's arc: where it is, how thick, where it starts and how far it goes.
#[derive(Debug, Clone, Copy)]
struct Geometry {
    center: Pos2,
    /// The band's centre-line radius.
    radius: f32,
    width: f32,
    /// The angle of the arc's start (radians, egui's screen sense: +y down, clockwise positive).
    start: f32,
    /// The whole arc, in radians — `TAU` less the gap.
    sweep: f32,
    /// The anti-aliasing skirt outside each edge (du): one physical pixel, fading to clear. A
    /// mesh gets none of egui's feathering, and a band without it is a staircase.
    feather: f32,
}

impl Geometry {
    /// Whether the arc is the whole circle, so its two ends meet.
    fn closes(&self) -> bool {
        self.sweep >= TAU - 1e-4
    }

    /// The point at fraction `t` of the arc, `offset` du out from the centre-line.
    fn point(&self, t: f32, offset: f32) -> Pos2 {
        let angle = self.sweep.mul_add(t, self.start);
        self.center + Vec2::angled(angle) * (self.radius + offset)
    }
}

/// The indeterminate fill for this frame, and the repaint that keeps it running.
///
/// The head goes round once per `progress_cycle`, and the arc behind it breathes between
/// [`INDET_MIN`] and [`INDET_MAX`] of the circle on a slower beat — Material's "advance". Under
/// `motion.reduce` nothing rotates and the whole track pulses instead, as the bar does.
// The clock is `f64` seconds and only its fraction crosses to `f32`.
#[allow(clippy::cast_possible_truncation)]
fn indeterminate(ui: &egui::Ui, cx: &Cx<'_>, every: Option<Duration>) -> Fill {
    let cycle = cx.theme.motion.progress_cycle.as_secs_f64().max(0.001);
    let time = ui.input(|i| i.time);
    let phase = f64::rem_euclid(time / cycle, 1.0).clamp(0.0, 1.0) as f32;
    match every {
        Some(interval) => ui.ctx().request_repaint_after(interval),
        None => ui.ctx().request_repaint(),
    }
    if cx.theme.motion.reduce {
        return Fill::Pulse(pulse_alpha(phase * 0.5, cx.theme.control.disabled_alpha));
    }
    let breath = f64::rem_euclid(time / (cycle * 2.7), 1.0).clamp(0.0, 1.0) as f32;
    let wave = (breath * TAU).cos().mul_add(-0.5, 0.5);
    let len = INDET_MIN + (INDET_MAX - INDET_MIN) * wave;
    // `segment_span` walks a segment along a track; on a circle the walk wraps, so the head is
    // taken from the same phase and the tail is `len` behind it, modulo one turn.
    let (_, head) = segment_span(phase, 0.0);
    let tail = head - len;
    if tail >= 0.0 {
        Fill::Segment(tail, head)
    } else {
        // Across twelve o'clock: draw the part before the seam; the rest comes round next frame.
        Fill::Segment(0.0, head)
    }
}

/// One frame's colours, dimmed once at the end.
#[derive(Debug, Clone, Copy)]
struct Ink {
    track: Color32,
    fill: Color32,
    value: Color32,
    label: Color32,
}

impl Ink {
    fn of(cx: &Cx<'_>, tone: ColorRole, enabled: bool) -> Self {
        let fill = cx.theme.color(tone);
        let ink = Self {
            // The track is the tone at the same wash a lit tile or a `Tinted` group carries, so a
            // ring on one of those reads as part of the same family and not as a grey stranger.
            track: fill.gamma_multiply(cx.theme.control.fill_alpha),
            fill,
            value: cx.theme.color(ColorRole::OnSurface),
            label: cx.theme.color(ColorRole::Muted),
        };
        if enabled {
            ink
        } else {
            let a = cx.theme.control.disabled_alpha;
            Self {
                track: ink.track.gamma_multiply(a),
                fill: ink.fill.gamma_multiply(a),
                value: ink.value.gamma_multiply(a),
                label: ink.label.gamma_multiply(a),
            }
        }
    }
}

/// The key a cached fill mesh is valid for.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CacheKey {
    center: Pos2,
    radius: f32,
    width: f32,
    from: f32,
    to: f32,
    gap: f32,
    fill: Color32,
    style: RingStyle,
}

fn quantise(t: f32) -> f32 {
    (t * CACHE_STEPS).round() / CACHE_STEPS
}

/// Whether a band gets a halo, and in what colour.
#[derive(Debug, Clone, Copy)]
struct Halo(Color32);
#[derive(Debug, Clone, Copy)]
struct HaloNone;

trait HaloSpec: Copy {
    fn colour(self) -> Option<Color32>;
}
impl HaloSpec for Halo {
    fn colour(self) -> Option<Color32> {
        Some(self.0)
    }
}
impl HaloSpec for HaloNone {
    fn colour(self) -> Option<Color32> {
        None
    }
}

/// A band of the arc from fraction `from` to `to`, as a mesh with `colour_at(k)` on each vertex,
/// `k` running 0 to 1 along it — and, where asked, a halo under it that fades to nothing at
/// [`HALO_WIDTH`] times the band's width.
fn band(
    geom: &Geometry,
    from: f32,
    to: f32,
    colour_at: impl Fn(f32) -> Color32,
    halo: impl HaloSpec,
    ends: Ends,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::default();
    let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
    if to <= from {
        return mesh;
    }
    let arc_len = (to - from) * geom.sweep * geom.radius;
    // A segment every ~3 du of arc, and never so few that a short arc is a polygon.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a segment count from a positive length, clamped"
    )]
    let steps = ((arc_len / 3.0).ceil() as usize).clamp(6, BAND_MAX);
    let half_w = geom.width * 0.5;

    if let Some(glow) = halo.colour() {
        // Three rings of vertices: clear at the inner edge, the halo at the centre-line, clear at
        // the outer edge. Under the band, so the band's own edge stays crisp.
        let reach = half_w * HALO_WIDTH;
        let first = mesh.vertices.len();
        for step in 0..=steps {
            #[expect(clippy::cast_precision_loss, reason = "a step index under 200")]
            let k = step as f32 / steps as f32;
            let t = from + (to - from) * k;
            mesh.colored_vertex(geom.point(t, -reach), Color32::TRANSPARENT);
            mesh.colored_vertex(geom.point(t, 0.0), glow.gamma_multiply(HALO_ALPHA));
            mesh.colored_vertex(geom.point(t, reach), Color32::TRANSPARENT);
        }
        // Each step has three vertices; each pair of steps makes two quads (inner→mid, mid→outer).
        for step in 0..steps {
            let here = first + step * 3;
            let next = here + 3;
            for lane in 0..2 {
                quad(
                    &mut mesh,
                    here + lane,
                    here + lane + 1,
                    next + lane,
                    next + lane + 1,
                );
            }
        }
        if ends == Ends::Round {
            // The glow goes round the caps too: a fan from the glow at the centre to clear at the
            // rim has the same cross-section as the lanes, so the end face is invisible.
            let glow = glow.gamma_multiply(HALO_ALPHA);
            end_cap(
                &mut mesh,
                geom,
                from,
                reach,
                false,
                glow,
                Color32::TRANSPARENT,
            );
            end_cap(&mut mesh, geom, to, reach, true, glow, Color32::TRANSPARENT);
        }
    }

    // Four lanes a step: a clear skirt, the two coloured edges, a clear skirt — the band's
    // own anti-aliasing, one pixel wide, since a mesh gets none from the tessellator.
    let skirt = half_w + geom.feather.max(0.0);
    let first = mesh.vertices.len();
    for step in 0..=steps {
        #[expect(clippy::cast_precision_loss, reason = "a step index under 200")]
        let k = step as f32 / steps as f32;
        let t = from + (to - from) * k;
        let colour = colour_at(k);
        let clear = colour.gamma_multiply(0.0);
        mesh.colored_vertex(geom.point(t, -skirt), clear);
        mesh.colored_vertex(geom.point(t, -half_w), colour);
        mesh.colored_vertex(geom.point(t, half_w), colour);
        mesh.colored_vertex(geom.point(t, skirt), clear);
    }
    for step in 0..steps {
        let here = first + step * 4;
        let next = here + 4;
        for lane in 0..3 {
            quad(
                &mut mesh,
                here + lane,
                here + lane + 1,
                next + lane,
                next + lane + 1,
            );
        }
    }
    if ends == Ends::Round {
        let (tail, head) = (colour_at(0.0), colour_at(1.0));
        end_cap(&mut mesh, geom, from, half_w, false, tail, tail);
        end_cap(&mut mesh, geom, to, half_w, true, head, head);
    }
    for v in &mut mesh.vertices {
        v.uv = egui::epaint::WHITE_UV;
    }
    mesh
}

/// How a band's two ends are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ends {
    /// Cut square, on the radial line. A closed ring, whose ends meet.
    Flat,
    /// A half-disc on each end.
    Round,
}

/// Two triangles across four vertex indices: `inner_a`/`outer_a` on one side, `inner_b`/`outer_b`
/// on the next.
fn quad(mesh: &mut egui::Mesh, inner_a: usize, outer_a: usize, inner_b: usize, outer_b: usize) {
    mesh.add_triangle(idx(inner_a), idx(outer_a), idx(inner_b));
    mesh.add_triangle(idx(outer_a), idx(outer_b), idx(inner_b));
}

/// A half-disc of radius `r` on the far side of the end at `t` — the head's bulges forward along
/// the arc, the tail's backward — coloured `centre` in the middle and `rim` at the edge.
///
/// **Its flat edge is the end face itself**, from `point(t, -r)` through `point(t, 0)` to
/// `point(t, r)`, the same positions the band's own end vertices have, so the cap shares that
/// edge with the band and covers nothing the band covers. A whole disc laid over the end did:
/// the tail is 0.42 of the tone, and 0.42 over 0.42 drew a darker half-moon at twelve o'clock.
fn end_cap(
    mesh: &mut egui::Mesh,
    geom: &Geometry,
    t: f32,
    r: f32,
    forward: bool,
    centre: Color32,
    rim: Color32,
) {
    if r <= 0.0 {
        return;
    }
    let base = mesh.vertices.len();
    let at = geom.point(t, 0.0);
    mesh.colored_vertex(at, centre);
    // From the inner end of the face (angle φ + π) round to the outer end (φ): the forward
    // tangent lies at φ + π/2 for a clockwise sweep, the backward one at φ − π/2.
    let phi = geom.sweep.mul_add(t, geom.start);
    let turn = if forward == (geom.sweep >= 0.0) {
        -std::f32::consts::PI
    } else {
        std::f32::consts::PI
    };
    let points = 12;
    let skirt = r + geom.feather.max(0.0);
    let clear = rim.gamma_multiply(0.0);
    // The rim, then the same points one feather further out and clear: the fan's own
    // anti-aliasing, joining the band's skirt at the two ends of the face.
    for (radius, colour) in [(r, rim), (skirt, clear)] {
        for step in 0..=points {
            let pos = match step {
                0 => geom.point(t, -radius),
                n if n == points => geom.point(t, radius),
                _ => {
                    #[expect(clippy::cast_precision_loss, reason = "12 points round a half-disc")]
                    let angle = phi + std::f32::consts::PI + turn * step as f32 / points as f32;
                    at + Vec2::angled(angle) * radius
                }
            };
            mesh.colored_vertex(pos, colour);
        }
    }
    for step in 0..points {
        let this = base + 1 + step;
        mesh.add_triangle(idx(base), idx(this), idx(this + 1));
        // The skirt quad outside this rim segment.
        let outer = this + points + 1;
        quad(mesh, this, outer, this + 1, outer + 1);
    }
    for v in mesh.vertices.iter_mut().skip(base) {
        v.uv = egui::epaint::WHITE_UV;
    }
}

/// A vertex index for the mesh. A ring is a few hundred vertices; `u32::MAX` is not a thing that
/// happens, and saturating is the honest answer if it ever did.
fn idx(i: usize) -> u32 {
    u32::try_from(i).unwrap_or(u32::MAX)
}

/// Linear interpolation in gamma space between two colours.
fn lerp_colour(a: Color32, b: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 {
        let v = f32::from(x).mul_add(1.0 - k, f32::from(y) * k).round();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a mix of two u8 channels stays in 0..=255"
        )]
        {
            v.clamp(0.0, 255.0) as u8
        }
    };
    Color32::from_rgba_premultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        band, lerp_colour, start_angle, Ends, Geometry, Halo, HaloNone, ProgressRing, RingStyle,
    };
    use egui::{Color32, Pos2};
    use std::f32::consts::{FRAC_PI_2, TAU};

    #[test]
    fn the_value_is_clamped_and_a_non_finite_value_reads_as_empty() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(ProgressRing::determinate(bad).value, Some(0.0), "{bad}");
        }
        assert_eq!(ProgressRing::determinate(2.0).value, Some(1.0));
        assert_eq!(ProgressRing::indeterminate().value, None);
    }

    /// **A gauge's gap is really empty.** With 180° open at the bottom, no vertex of the full
    /// track lands below the centre by more than the band's own half-width.
    #[test]
    fn a_gauges_gap_has_nothing_in_it() {
        let gap = 180.0_f32.to_radians();
        let geom = Geometry {
            center: Pos2::new(100.0, 100.0),
            radius: 40.0,
            width: 8.0,
            start: FRAC_PI_2 + gap * 0.5,
            sweep: TAU - gap,
            feather: 1.0,
        };
        let mesh = band(&geom, 0.0, 1.0, |_| Color32::WHITE, HaloNone, Ends::Flat);
        assert_ne!(mesh.vertices.len(), 0, "the band draws something");
        let lowest = mesh
            .vertices
            .iter()
            .map(|v| v.pos.y)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            lowest <= 100.0 + 4.0 + 0.5,
            "a half ring open at the bottom reached y = {lowest}"
        );
    }

    /// **A band's vertices all sit on the band**, between the inner and outer radius.
    #[test]
    fn every_vertex_is_on_the_band() {
        let geom = Geometry {
            center: Pos2::new(0.0, 0.0),
            radius: 50.0,
            width: 10.0,
            start: -FRAC_PI_2,
            sweep: TAU,
            feather: 1.0,
        };
        let mesh = band(&geom, 0.2, 0.7, |_| Color32::WHITE, HaloNone, Ends::Flat);
        for v in &mesh.vertices {
            let r = v.pos.to_vec2().length();
            // The band, plus its one-du clear skirt on each side.
            assert!((43.9..=56.1).contains(&r), "a vertex at radius {r}");
            if !(44.9..=55.1).contains(&r) {
                assert_eq!(v.color.a(), 0, "a skirt vertex that is not clear");
            }
        }
        assert_eq!(mesh.indices.len() % 3, 0, "whole triangles only");
    }

    /// **A ring starts at twelve, a gauge at its gap.** The first render of this started every
    /// ring at six o'clock.
    #[test]
    fn a_ring_starts_at_the_top_and_a_gauge_at_its_gap() {
        let up = egui::Vec2::angled(start_angle(0.0));
        assert!(up.y < -0.99, "a full ring's start points {up:?}, not up");
        let gauge = egui::Vec2::angled(start_angle(90.0));
        assert!(
            gauge.y > 0.0 && gauge.x < 0.0,
            "a 90° gauge starts at the bottom-left, not {gauge:?}"
        );
    }

    /// The sweep's colour runs from the tail to the head, and only between them.
    #[test]
    fn the_sweep_interpolates_and_clamps() {
        let a = Color32::from_rgb(0, 0, 0);
        let b = Color32::from_rgb(200, 100, 50);
        assert_eq!(lerp_colour(a, b, 0.0), a);
        assert_eq!(lerp_colour(a, b, 1.0), b);
        assert_eq!(lerp_colour(a, b, 2.0), b);
        let mid = lerp_colour(a, b, 0.5);
        assert_eq!((mid.r(), mid.g(), mid.b()), (100, 50, 25));
        assert_eq!(RingStyle::default(), RingStyle::Sweep);
    }

    /// How many of the mesh's triangles have `p` strictly inside them.
    fn covered(mesh: &egui::Mesh, p: Pos2) -> usize {
        let side = |a: Pos2, b: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        mesh.indices
            .chunks(3)
            .filter(|tri| {
                let at = |i: usize| {
                    tri.get(i)
                        .and_then(|&k| mesh.vertices.get(k as usize))
                        .map_or(Pos2::ZERO, |v| v.pos)
                };
                let (a, b, c) = (at(0), at(1), at(2));
                let (s0, s1, s2) = (side(a, b), side(b, c), side(c, a));
                let eps = 1e-3;
                (s0 > eps && s1 > eps && s2 > eps) || (s0 < -eps && s1 < -eps && s2 < -eps)
            })
            .count()
    }

    /// **No two triangles of a capped band cover the same point.** The tail is translucent, and
    /// a cap drawn as a whole disc over the band's end painted its start twice — a darker
    /// half-moon at twelve o'clock, on the console's own ring. With a halo the band sits over it
    /// by design, so two is the ceiling there and three (the old cap) is the defect.
    #[test]
    fn a_capped_band_paints_no_point_twice() {
        let geom = Geometry {
            center: Pos2::new(0.0, 0.0),
            radius: 50.0,
            width: 12.0,
            start: -FRAC_PI_2,
            sweep: TAU,
            feather: 1.0,
        };
        let flat = band(&geom, 0.0, 0.63, |_| Color32::WHITE, HaloNone, Ends::Round);
        let glow = band(
            &geom,
            0.0,
            0.63,
            |_| Color32::WHITE,
            Halo(Color32::WHITE),
            Ends::Round,
        );
        // Just behind twelve o'clock, inside the tail's cap and outside the band proper — and
        // off the fan's own spokes, which a strict inside test does not count.
        let in_tail_cap = Pos2::new(-2.3, -50.9);
        assert_eq!(covered(&flat, in_tail_cap), 1, "the tail has no round cap");
        assert_eq!(
            covered(&glow, in_tail_cap),
            2,
            "the halo does not go round the tail cap"
        );
        let mut samples = 0;
        let mut y = -70.0_f32;
        while y < 70.0 {
            let mut x = -70.0_f32;
            while x < 70.0 {
                let p = Pos2::new(x + 0.137, y + 0.211);
                let n = covered(&flat, p);
                assert!(
                    n <= 1,
                    "{p:?} is painted {n} times by the band and its caps"
                );
                let n = covered(&glow, p);
                assert!(n <= 2, "{p:?} is painted {n} times with the halo");
                samples += 1;
                x += 1.37;
            }
            y += 1.37;
        }
        assert!(samples > 5000);
    }

    /// A closed ring — no gap, 100 % — is joined, not capped: caps there would lap the tail.
    #[test]
    fn a_closed_ring_has_no_caps_to_lap_its_own_tail() {
        let closed = Geometry {
            center: Pos2::new(0.0, 0.0),
            radius: 50.0,
            width: 12.0,
            start: -FRAC_PI_2,
            sweep: TAU,
            feather: 1.0,
        };
        assert!(closed.closes());
        let gauge = Geometry {
            sweep: TAU - 75.0_f32.to_radians(),
            ..closed
        };
        assert!(!gauge.closes());
    }
}
