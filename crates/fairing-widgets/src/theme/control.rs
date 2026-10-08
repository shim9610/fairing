//! [`ControlSpec`] / [`ControlMetrics`] — the tokens every control shares.
//!
//! # Why a second spec next to [`ComponentSpec`](super::ComponentSpec)
//!
//! [`ComponentSpec`] holds tokens that belong to **one** thing each: a toast's stack gap, the
//! shade's handle. What was missing was the set a *control* needs and that every control needs the
//! same way — the gap beside a mark, the thickness of an edge, the alpha a disabled thing fades to.
//! Those were literals scattered across six widget files, which is why a switch, a slider and a
//! button ended up looking like three unrelated drawings rather than one family.
//!
//! # The two families
//!
//! Every length here is in exactly one of two, and the rule that sorts them is what a length is
//! *for*:
//!
//! - **hand** — [`Dim::finger`] with a `du` floor. If a finger has to land on it, hit it, or clear
//!   it, it belongs to the hand: touch targets, control bodies, marks, insets, gaps, radii.
//! - **eye** — [`Dim::mm`] with a [`Dim::px`] floor. If only the eye has to find it, it belongs to
//!   the eye: stroke widths. A hairline's job is to be seen at arm's length through glass, and a
//!   glove does not change that job; the pixel floor stops it vanishing on a low-density panel.
//!
//! # The control rules
//!
//! Every built-in control keeps these (also listed in `docs/architecture.md`). The comments in
//! the widget files cite them by number.
//!
//! - **Rule 1.1.** The slot is a full touch target on both axes, whatever size the control is
//!   drawn at: what you see is not what you press.
//! - **Rule 1.3.** Inside a row, the row owns the hit rect. A control drawn into it senses
//!   nothing of its own, so pressing anywhere on the row is pressing the control.
//! - **Rule 1.4.** A disabled control lets the tap through to whatever is beneath it, rather than
//!   swallowing a tap it cannot act on.
//! - **Rule 1.5.** A press only ever grows the painted shape. The allocated rect never moves, so
//!   press geometry never shrinks and nothing beside it shifts.
//! - **Rule 3.1.** A control's identifying boundary meets the 3.0 contrast floor of WCAG 1.4.11.
//!   `Outline` is a divider, never a control's edge: against both grounds it is a ghost.
//! - **Rule 4.1.** The pill shape is kept for movers that ride a track, such as a switch's knob,
//!   so it stays the cue that something travels.
//! - **Rule 7.1.** Colour alone is never the channel. A state also shows in shape or fill, so a
//!   greyscale render still tells it apart.
//!
//! # Why the defaults are all `du` today
//!
//! **This is adoption step 1 and it is deliberate.** Every `Span` below is written with its `du`
//! base only, so the render is byte-identical to before the vocabulary existed. The point of the
//! step is that the tokens come into being and every widget starts reading them, with nothing
//! moving. The finger terms go on afterwards, as their own reviewable change, and only *after* the
//! shell's hand-drawn rows are composed out of [`ListRow`](crate::widgets::ListRow) — done the
//! other way round, the four-du gap between a `ListRow` label and a hand-drawn one widens to
//! fifteen instead of closing.

use super::Metrics;
use crate::unit::{Dim, Eye, Free, Hand, Scale, Span};
use crate::Result;

/// The resolved control tokens (= `Theme::control`). Lengths are du; the ratios are unitless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlMetrics {
    /// **How tall a control is drawn.** Three body ems — see [`ControlSpec::height`].
    ///
    /// Read it through [`control_height`], never on its own: on a panel whose text is set small
    /// this can fall under the hand, and that free function is where the floor is applied.
    pub height: f32,
    /// The gap between a control's mark and its label, and between a leading icon and text.
    pub gap: f32,
    /// The gap between the two lines of a two-line row.
    pub line_gap: f32,
    /// The corner radius of a small mark — a checkbox's box, a segment's raised cell.
    pub mark_radius: f32,
    /// The side of a control's mark: the switch track's height, the checkbox box, the radio ring.
    pub mark_size: f32,
    /// An icon drawn inside a control or a row.
    pub icon: f32,
    /// A slider track's thickness.
    pub slider_track: f32,
    /// The narrowest a button may be, so a two-word label is not squeezed.
    pub min_button_w: f32,
    /// The narrowest a row's label column may be before the row stacks into two lines.
    pub row_min_label: f32,
    /// How much a pressed control's painted silhouette grows.
    ///
    /// It **grows**, never shrinks. A control that shrinks under the finger moves away from the
    /// thing pressing it, and on a panel the finger already covers it.
    pub press_grow: f32,
    /// A hairline — a separator, a divider inside a card.
    pub stroke_hairline: f32,
    /// **The bar down a selected rail entry's leading edge**.
    ///
    /// Thicker than a hairline on purpose: a hairline is a boundary between two things, and this is
    /// a mark pointing at one of them. Three times the hairline is where it stops reading as a
    /// divider that happens to be coloured.
    pub accent_bar: f32,
    /// A resting control's edge.
    pub stroke_edge: f32,
    /// A drawn mark — a tick, a radio dot's ring, a focus ring.
    pub stroke_mark: f32,
    /// The gap between a control's silhouette and its focus ring.
    pub focus_gap: f32,
    /// **An icon-only button's drawn disc.**
    ///
    /// 0.68 of a fingertip is iOS's system close button to three figures (30 pt in a 44 pt target).
    /// Material 3's icon-button container is 40/48 = 0.833 and the reference kiosks' add buttons
    /// measure 0.385–0.661 of the controls around them, so this sits inside the band and on a
    /// published value. The du floor is 34.0 and not `0.68 × 48 = 32.64`, because a square of side
    /// `control.icon` has to fit inside the disc and `icon`'s own floor is the legacy 24: `24/√2`
    /// is 33.94.
    pub icon_button: f32,
    /// **A badge's height** — fixed, not derived from its text, because that is what a badge is:
    /// Material's `largeSize` is the height and nothing else. The du floor is the size the dock's
    /// badge already drew.
    pub badge_h: f32,
    /// The padding on each side of a badge's digits.
    pub badge_pad: f32,
    /// A checkbox's or radio's drawn size as a fraction of [`Self::mark_size`].
    ///
    /// **A box is not a switch track.** `mark_size` is `finger(0.6)`, which puts a switch at
    /// 5.4 mm on a 9 mm panel — iOS's 5.1, One UI's 5.1. A checkbox at that size is far too big:
    /// Material 3 draws 18 dp (2.86 mm), One UI about 24 dp (3.8 mm), and ours came out at the full
    /// 5.4 because the two shared one token. 0.62 lands between the two references, and what a
    /// finger can hit does not change - the slot stays a whole `touch_target`.
    pub box_ratio: f32,
    /// A knob's diameter as a fraction of [`Self::mark_size`].
    pub knob_ratio: f32,
    /// A slider handle's width as a fraction of the track's thickness.
    pub handle_ratio: f32,
    /// A switch track's width as a multiple of its height.
    ///
    /// **1.65, which is iOS's 51 × 31.** It was 1.8 — longer than everything but Windows 11's
    /// 2.0 — while the knob stayed at 0.875, the *largest* of any shipped switch. Measured against
    /// the references, nothing pairs those two: iOS puts the same big knob on a much shorter track
    /// (1.65 / 0.87), Windows puts a small knob on a long one (2.0 / 0.60). Carrying both read as a
    /// ball rattling in a tube. Shortening the track costs no touch area — what a switch senses is
    /// a full `touch_target` slot, and only the drawn rect is allowed to be small.
    pub switch_aspect: f32,
    /// A radio's dot diameter as a fraction of its ring.
    pub dot_ratio: f32,
    /// A segmented control's raised cell inset as a fraction of the strip's height.
    pub tick_ratio: f32,
    /// How much a slider's track swells while it is held.
    pub press_swell: f32,
    /// The one alpha a disabled control fades to. There is exactly one, for every control.
    pub disabled_alpha: f32,
    /// A tinted badge's alpha behind an icon.
    pub badge_alpha: f32,
    /// A card's radius as a multiple of `metrics.corner_radius`.
    pub card_radius_ratio: f32,
    /// **A progress track's thickness as a fraction of a slider's.**
    ///
    /// `slider_track` is 0.375 T because a gloved finger has to grab it, and the slider's own doc
    /// spends four paragraphs on that. **Nothing grabs a progress bar**, and halving it is the only
    /// change that follows from taking the hand away. 0.47 is a shade under half: the e-reader
    /// reference measures an 11 px bar against a 63 px control, which is 0.1746 T, and
    /// `0.47 x 0.375 T` is 0.17625 T — 0.9 % off.
    ///
    /// It is a ratio over the slider rather than a `Span` of its own so that the one question this
    /// element has to answer lives in the token: a panel that thickens what you grab thickens what
    /// it reports with, and the two cannot drift apart.
    pub progress_ratio: f32,
    /// **A progress ring's stroke over its diameter.**
    ///
    /// Measured off the console reference: a 14 px band on a 118 px ring, 0.12. Thicker than the
    /// bar by design — a ring is read at a glance from across the bench, and a thin one at that
    /// distance is a circle with a scratch on it. It is a ratio over the ring's own diameter and
    /// not a `Span`, because a ring is sized by whatever it sits in (a tile, a card, a hero) and
    /// the band has to keep its proportion at every one of those.
    pub ring_ratio: f32,
    /// An indeterminate segment's length as a fraction of its track's.
    ///
    /// Under about a quarter the segment reads as a dot crossing a line at 600 mm. Material's
    /// disjoint primary segment peaks near two thirds, which makes a bar look nearly full and then
    /// empty again — the one thing an indeterminate bar must never imply. 0.30 leaves seven tenths
    /// of the track visibly empty at every phase.
    pub indeterminate_span: f32,
    /// **A status lamp's outer diameter over the body text beside it.**
    ///
    /// It hangs off the *text* and not off `mark_size`, because the measurement says so: the
    /// instrument-panel reference draws a 40 px lamp against an 18 px cap height, which is 2.22
    /// cap heights, and against that panel's own 63 px control height it is 0.635 T while ours
    /// would be 0.525 T. The two anchors disagree by 21 %, and the disagreement vanishes when both
    /// are measured against the word beside them — which is how a lamp is read.
    ///
    /// Taking a cap at 0.70 em, `2.25 × 0.70 = 1.575`. It resolves to exactly a switch knob's
    /// diameter today, and it is still a token of its own for the reason `box_ratio` was split out
    /// of `mark_size`: a knob answers "how much travel fits in this track" and a lamp answers "how
    /// far away can this be read", so tying them would let a switch retune silently retune the
    /// panel's status strip.
    pub lamp_ratio: f32,
    /// **A badge's digit size over its height.** `0.625 × 16 du = 10 du`, which is exactly the
    /// size the desktop's dock badge already drew, so promoting it changed no glyph. Material's is
    /// 11 sp in a 16 dp badge (0.6875), so this sits 9 % under.
    pub badge_text_ratio: f32,
    /// **A dot badge's diameter over a badge's height.** Numerically equal to
    /// [`Self::badge_text_ratio`] today and kept apart, because one answers "how big is a glyph"
    /// and the other "how big is a mark that has no glyph to fall back on". Material's small badge
    /// is 0.375 of its large one and Windows 11's dot is 0.25 of its numeric one; this crate is
    /// deliberately above both, because 6 dp subtends under a millimetre at 600 mm.
    pub badge_dot_ratio: f32,
    /// **The largest count a badge draws as itself**; above it it reads `99+`.
    ///
    /// The dock's hard-coded 99, promoted. Material makes the same thing settable and defaults to
    /// four characters; 99 stays the default here because at the gloved size a four-cell badge is
    /// about 12 mm wide and overhangs the icon it is badging.
    pub badge_max: u16,
    /// **A translucent control face's alpha over whatever is behind it.**
    ///
    /// A neutral control's face is a tint of the ink rather than a colour of its own, the way iOS
    /// defines its secondary fills, so the control reads as material and composites for free over
    /// a card, a page or a wallpaper alike. It was a private constant in `button.rs` until a
    /// second and a third control needed the same number — which is how the crate ended up with
    /// five uncoordinated 1-du strokes the first time.
    pub fill_alpha: f32,
    /// **The least a chip's width may be as a multiple of its height**, so a one-word category is
    /// not a stub between two long ones.
    ///
    /// Measured: six of the reference row's eight chips sit at 1.230 while their labels vary by
    /// 2.7× — the floor is plainly an aspect and not the content. Windows 11's 40 epx target on a
    /// 32 px control is 1.25 exactly, and Material's 48 dp target on a 32 dp chip works out at
    /// 1.50. This is Fluent's value, 1.6 % off the measurement.
    pub chip_min_aspect: f32,
    /// The most of a row's width a trailing value may take before it is elided.
    pub trailing_text_max: f32,
    /// The most segments a [`SegmentedControl`](crate::widgets::SegmentedControl) may hold.
    ///
    /// Past four, every cell is narrower than a touch target on the smallest panel and the control
    /// should have been a list.
    pub segment_max: u8,
}

impl Default for ControlMetrics {
    fn default() -> Self {
        ControlSpec::default().resolve(&Scale::identity())
    }
}

/// The control spec in physical units. Resolved each frame with a [`Scale`] into [`ControlMetrics`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlSpec {
    /// The gap beside a mark.
    /// **A control's drawn height** — `text(3.0)`, three body ems.
    ///
    /// The token that did not exist, and whose absence is why a standing kiosk came out with
    /// phone-sized furniture on it. Eleven controls read `metrics.touch_target` for their size,
    /// and that token answers a different question — *the least a finger may be asked to hit* — so
    /// it is pinned to the hand and does not move when the text moves for the eye.
    ///
    /// 3.0 is not a new proportion. The old `finger(1.0)` was already 2.994 body ems at the
    /// default policy, so this reproduces the shipped render to 0.2 % and states the relationship
    /// the numbers already had. Everything else follows from it: a switch track is 0.6 of a
    /// control, an icon button 0.68, a row a control plus 8 du.
    ///
    /// The `du` floor is `touch_target`'s own, and [`control_height`] puts the finger back under it.
    pub height: Span<Eye>,
    /// The gap between a control's mark and its label.
    pub gap: Span<Eye>,
    /// The gap between two lines of a row.
    pub line_gap: Span<Eye>,
    /// A small mark's corner radius.
    pub mark_radius: Span<Eye>,
    /// A control's mark.
    pub mark_size: Span<Eye>,
    /// An icon inside a control.
    pub icon: Span<Eye>,
    /// A slider track.
    pub slider_track: Span<Eye>,
    /// A button's minimum width.
    pub min_button_w: Span<Hand>,
    /// A row's minimum label column.
    pub row_min_label: Span<Hand>,
    /// The pressed silhouette's growth.
    pub press_grow: Span<Eye>,
    /// A hairline.
    pub stroke_hairline: Span,
    /// A selected rail entry's leading bar — see [`ControlMetrics::accent_bar`].
    pub accent_bar: Span,
    /// A resting edge.
    pub stroke_edge: Span,
    /// A drawn mark.
    pub stroke_mark: Span,
    /// The focus ring's gap.
    pub focus_gap: Span,
    /// An icon-only button's drawn disc.
    pub icon_button: Span<Eye>,
    /// A badge's height.
    pub badge_h: Span<Eye>,
    /// The padding on each side of a badge's digits.
    pub badge_pad: Span<Eye>,
    /// The unitless ratios, which are not lengths and so are not resolved.
    pub ratios: ControlRatios,
}

/// The unitless part of [`ControlSpec`] — fractions and counts, which no [`Scale`] touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlRatios {
    /// A checkbox's or radio's drawn size over [`ControlMetrics::mark_size`].
    pub box_ratio: f32,
    /// A knob's diameter over [`ControlMetrics::mark_size`].
    pub knob_ratio: f32,
    /// A slider handle's width over the track thickness.
    pub handle_ratio: f32,
    /// A switch track's width over its height.
    pub switch_aspect: f32,
    /// A radio dot over its ring.
    pub dot_ratio: f32,
    /// A segment's raised-cell inset over the strip height.
    pub tick_ratio: f32,
    /// A held slider track's swell.
    pub press_swell: f32,
    /// The disabled alpha.
    pub disabled_alpha: f32,
    /// A badge's alpha.
    pub badge_alpha: f32,
    /// A card's radius over `metrics.corner_radius`.
    pub card_radius_ratio: f32,
    /// A progress track's thickness over [`ControlMetrics::slider_track`].
    pub progress_ratio: f32,
    /// A progress ring's stroke over its diameter.
    pub ring_ratio: f32,
    /// An indeterminate segment's length over its track's.
    pub indeterminate_span: f32,
    /// A status lamp's outer diameter over `metrics.type_scale.body`.
    pub lamp_ratio: f32,
    /// A badge's digit size over its height.
    pub badge_text_ratio: f32,
    /// A dot badge's diameter over a badge's height.
    pub badge_dot_ratio: f32,
    /// The largest count a badge draws as itself.
    pub badge_max: u16,
    /// A translucent control face's alpha over whatever is behind it.
    pub fill_alpha: f32,
    /// The least a chip's width may be as a multiple of its height.
    pub chip_min_aspect: f32,
    /// A trailing value's share of the row width.
    pub trailing_text_max: f32,
    /// The segment cap.
    pub segment_max: u8,
}

impl Default for ControlRatios {
    fn default() -> Self {
        Self {
            box_ratio: 0.62,
            knob_ratio: 0.875,
            handle_ratio: 0.40,
            switch_aspect: 1.65,
            dot_ratio: 0.50,
            tick_ratio: 0.60,
            press_swell: 1.22,
            disabled_alpha: 0.55,
            badge_alpha: 0.16,
            card_radius_ratio: 1.6,
            progress_ratio: 0.47,
            ring_ratio: 0.12,
            indeterminate_span: 0.30,
            lamp_ratio: 1.575,
            badge_text_ratio: 0.625,
            badge_dot_ratio: 0.625,
            badge_max: 99,
            fill_alpha: 0.10,
            chip_min_aspect: 1.25,
            trailing_text_max: 0.40,
            segment_max: 4,
        }
    }
}

impl Default for ControlSpec {
    /// **Adoption step 4: the finger and millimetre terms are on.**
    ///
    /// Every length keeps its old value as a floor, so a panel resolving there renders as it did.
    /// The strokes are the one `eye`-family group: `0.20 / 0.40 / 0.60 mm` on a 1 : 2 : 3 ladder,
    /// falling back to exactly one, two and three device pixels where a millimetre would be under a
    /// pixel. Today's five uncoordinated 1-du strokes are 0.159 mm, which is about 0.9 arcmin at
    /// 600 mm — at the eye's resolving limit before glare, and before a glove smears the glass.
    fn default() -> Self {
        Self {
            // **Adoption step 5: what is looked at is written in `text`, what is pressed in
            // `finger`.**
            //
            // These all carried finger terms, on the argument that a mark and a gap have to grow
            // with the hand. Half of that is right and half of it was the whole bug: a *gap* is
            // read, not pressed, and welding it to the hand meant it stopped following the text the
            // moment an integrator stated the text another way.
            //
            // Divide the old finger fractions by the body's own 0.334 and the vocabulary turns out
            // to have been a clean ratio of the text all along, written in a unit that hid it:
            //
            //   line_gap 1/4 · badge_pad 1/4 · gap 1/2 · badge_h 1 · slider_track 9/8 ·
            //   icon 1.272 · lamp 1.575 (already a `body` ratio) · mark_size 1.796 ·
            //   icon_button 2.036 · touch_target 3
            //
            // Each is written here at that ratio, so the largest move at the default policy is
            // **0.3 %** and the table now says what it means. The `du` floors are untouched.
            height: Span::fixed(Dim::text(3.0)).min(Dim::du(48.0)),
            gap: Span::fixed(Dim::text(0.5)).min(Dim::du(8.0)),
            line_gap: Span::fixed(Dim::text(0.25)).min(Dim::du(4.0)),
            mark_radius: Span::fixed(Dim::text(0.187)).min(Dim::du(3.0)),
            // The one exception, and it is not a new value: this is `components.switch.height`
            // moved here unchanged, finger term and all, because a switch that suddenly shrank on a
            // gloved device would be a regression rather than a no-op.
            mark_size: Span::fixed(Dim::text(1.8)).min(Dim::du(28.8)),
            icon: Span::fixed(Dim::text(1.272)).min(Dim::du(24.0)),
            slider_track: Span::fixed(Dim::text(1.125)).min(Dim::du(18.0)),
            min_button_w: Span::fixed(Dim::finger(1.5)).min(Dim::du(120.0)),
            row_min_label: Span::fixed(Dim::finger(1.5)).min(Dim::du(72.0)),
            press_grow: Span::fixed(Dim::text(0.18)).min(Dim::du(3.0)),
            stroke_hairline: Span::fixed(Dim::mm(0.20)).min(Dim::px(1.0)),
            accent_bar: Span::fixed(Dim::mm(0.60)).min(Dim::px(3.0)),
            // **0.30 mm, which is Material 3's 2 dp.** It was 0.40 - and 0.40 mm on a checkbox that
            // is itself smaller now is a heavy frame: the border came out at 0.13 of the box where
            // M3 sits at 0.11 and Windows 11 at 0.05. A control's boundary is meant to bound it,
            // not to draw attention on its own.
            stroke_edge: Span::fixed(Dim::mm(0.30)).min(Dim::px(1.0)),
            stroke_mark: Span::fixed(Dim::mm(0.60)).min(Dim::px(3.0)),
            focus_gap: Span::fixed(Dim::mm(0.60)).min(Dim::px(3.0)),
            // Material's 16 dp badge against a 48 dp target is 0.333 T exactly, and the du floor
            // below is that 16, so the dock's badge is unchanged at the floor. (It used to be
            // `metrics.desktop_badge_min`; that token is gone — this one replaced it.)
            icon_button: Span::fixed(Dim::text(2.036)).min(Dim::du(34.0)),
            badge_h: Span::fixed(Dim::text(1.0)).min(Dim::du(16.0)),
            // Material's badge padding is 4 per side. The crate's old `desktop_badge_pad_x` was
            // the same number written as a **total**, which is why it read as twice Material's —
            // the token is gone now and this one is per side, as Material states it.
            badge_pad: Span::fixed(Dim::text(0.25)).min(Dim::du(4.0)),
            ratios: ControlRatios::default(),
        }
    }
}

/// **How tall to draw a control** — [`ControlSpec::height`], never below the hand.
///
/// The two facts a control's size answers to, in one place: it is sized to be *seen* beside its
/// label ([`ControlSpec::height`], which follows
/// [`ScalePolicy::viewing_distance_mm`](crate::unit::ScalePolicy::viewing_distance_mm)), and it is
/// floored so a finger can still land on it (`metrics.touch_target`, which follows
/// [`ScalePolicy::finger_mm`](crate::unit::ScalePolicy::finger_mm)). Neither is the other's proxy
/// and neither is negotiable, so both are applied here rather than argued at each of the eleven
/// call sites.
///
/// It is a free function for the same reason [`lamp_size`] is: the two tokens live in different
/// structs, so nothing owns the answer.
#[must_use]
pub fn control_height(metrics: &super::Metrics, control: &ControlMetrics) -> f32 {
    control.height.max(metrics.touch_target)
}

impl ControlMetrics {
    /// **A checkbox's or radio's drawn side** — `mark_size × box_ratio`.
    ///
    /// One place rather than three multiplications: the box, its ring and the label column a
    /// `RadioGroup` lays out all have to agree, and they did not have to before because they all
    /// read `mark_size` whole.
    #[must_use]
    pub fn box_size(&self) -> f32 {
        self.mark_size * self.box_ratio
    }

    /// **A progress track's thickness** — `slider_track × progress_ratio`.
    ///
    /// One place rather than a multiplication at each call site, exactly as [`Self::box_size`]:
    /// the bar, the radius of its caps and the row a caller lays it out in all have to agree.
    #[must_use]
    pub fn progress_track(&self) -> f32 {
        self.slider_track * self.progress_ratio
    }

    /// **A progress ring's stroke** for a ring of `diameter` — `diameter × ring_ratio`.
    #[must_use]
    pub fn ring_stroke(&self, diameter: f32) -> f32 {
        diameter.max(0.0) * self.ring_ratio
    }
}

impl ControlSpec {
    /// Resolve at this scale.
    #[must_use]
    pub fn resolve(&self, s: &Scale) -> ControlMetrics {
        let x = self.ratios;
        ControlMetrics {
            height: self.height.resolve(s),
            gap: self.gap.resolve(s),
            line_gap: self.line_gap.resolve(s),
            mark_radius: self.mark_radius.resolve(s),
            mark_size: self.mark_size.resolve(s),
            icon: self.icon.resolve(s),
            slider_track: self.slider_track.resolve(s),
            min_button_w: self.min_button_w.resolve(s),
            row_min_label: self.row_min_label.resolve(s),
            press_grow: self.press_grow.resolve(s),
            stroke_hairline: self.stroke_hairline.resolve(s),
            accent_bar: self.accent_bar.resolve(s),
            stroke_edge: self.stroke_edge.resolve(s),
            stroke_mark: self.stroke_mark.resolve(s),
            focus_gap: self.focus_gap.resolve(s),
            icon_button: self.icon_button.resolve(s),
            badge_h: self.badge_h.resolve(s),
            badge_pad: self.badge_pad.resolve(s),
            box_ratio: x.box_ratio,
            knob_ratio: x.knob_ratio,
            handle_ratio: x.handle_ratio,
            switch_aspect: x.switch_aspect,
            dot_ratio: x.dot_ratio,
            tick_ratio: x.tick_ratio,
            press_swell: x.press_swell,
            disabled_alpha: x.disabled_alpha,
            badge_alpha: x.badge_alpha,
            card_radius_ratio: x.card_radius_ratio,
            progress_ratio: x.progress_ratio,
            ring_ratio: x.ring_ratio,
            indeterminate_span: x.indeterminate_span,
            lamp_ratio: x.lamp_ratio,
            badge_text_ratio: x.badge_text_ratio,
            badge_dot_ratio: x.badge_dot_ratio,
            badge_max: x.badge_max,
            fill_alpha: x.fill_alpha,
            chip_min_aspect: x.chip_min_aspect,
            trailing_text_max: x.trailing_text_max,
            segment_max: x.segment_max,
        }
    }

    /// Every `Span` resolves positive at the reference scale, and every ratio is in range.
    ///
    /// # Errors
    /// [`Error::Config`](crate::Error::Config) naming the token that is wrong. A control spec with
    /// a negative stroke or a zero mark draws nothing and gives no reason, which is the failure
    /// this is here to turn into a message.
    pub fn validate(&self) -> Result<()> {
        let spans: [(&str, Span<Free>); 17] = [
            ("control.height", self.height.erased()),
            ("control.gap", self.gap.erased()),
            ("control.line_gap", self.line_gap.erased()),
            ("control.mark_radius", self.mark_radius.erased()),
            ("control.mark_size", self.mark_size.erased()),
            ("control.icon", self.icon.erased()),
            ("control.slider_track", self.slider_track.erased()),
            ("control.min_button_w", self.min_button_w.erased()),
            ("control.row_min_label", self.row_min_label.erased()),
            ("control.press_grow", self.press_grow.erased()),
            ("control.stroke_hairline", self.stroke_hairline.erased()),
            ("control.stroke_edge", self.stroke_edge.erased()),
            ("control.stroke_mark", self.stroke_mark.erased()),
            ("control.focus_gap", self.focus_gap.erased()),
            ("control.icon_button", self.icon_button.erased()),
            ("control.badge_h", self.badge_h.erased()),
            ("control.badge_pad", self.badge_pad.erased()),
        ];
        for (name, span) in spans {
            span.validate(name)?;
        }
        let x = self.ratios;
        let positive: [(&str, f32); 18] = [
            ("control.box_ratio", x.box_ratio),
            ("control.knob_ratio", x.knob_ratio),
            ("control.handle_ratio", x.handle_ratio),
            ("control.switch_aspect", x.switch_aspect),
            ("control.dot_ratio", x.dot_ratio),
            ("control.tick_ratio", x.tick_ratio),
            ("control.press_swell", x.press_swell),
            ("control.disabled_alpha", x.disabled_alpha),
            ("control.badge_alpha", x.badge_alpha),
            ("control.card_radius_ratio", x.card_radius_ratio),
            ("control.progress_ratio", x.progress_ratio),
            ("control.ring_ratio", x.ring_ratio),
            ("control.indeterminate_span", x.indeterminate_span),
            ("control.lamp_ratio", x.lamp_ratio),
            ("control.badge_text_ratio", x.badge_text_ratio),
            ("control.badge_dot_ratio", x.badge_dot_ratio),
            ("control.fill_alpha", x.fill_alpha),
            ("control.chip_min_aspect", x.chip_min_aspect),
        ];
        for (name, value) in positive {
            if !value.is_finite() || value <= 0.0 {
                return Err(crate::Error::Config(format!(
                    "[theme.control] {name} = {value} — a ratio has to be finite and above zero"
                )));
            }
        }
        if x.badge_max < 1 {
            return Err(crate::Error::Config(
                "[theme.control] control.badge_max = 0 — a badge that can never show a count"
                    .to_owned(),
            ));
        }
        if x.segment_max < 2 {
            return Err(crate::Error::Config(format!(
                "[theme.control] control.segment_max = {} — a choice of fewer than two is not a choice",
                x.segment_max
            )));
        }
        Ok(())
    }
}

/// **A status lamp's outer diameter** for these metrics — `type_scale.body × lamp_ratio`.
///
/// A free function taking both halves of the theme, the same shape as [`card_radius`], because the
/// two tokens it multiplies live in different structs: the lamp is sized off the text it is read
/// with, and the text is `Metrics`.
#[must_use]
pub fn lamp_size(metrics: &Metrics, control: &ControlMetrics) -> f32 {
    metrics.type_scale.body * control.lamp_ratio
}

/// A card's corner radius for these metrics — `corner_radius × card_radius_ratio`.
#[must_use]
pub fn card_radius(metrics: &Metrics, control: &ControlMetrics) -> f32 {
    metrics.corner_radius * control.card_radius_ratio
}
