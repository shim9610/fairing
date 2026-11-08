//! Shared control-size and appearance tokens.
//!
//! [`ControlSpec`] defines configurable lengths; [`ControlMetrics`] contains their
//! resolved values. Text-relative and finger-relative sizes can have du floors;
//! millimetre strokes can have pixel floors. [`Scale`] resolves these values into
//! egui points using the configured policy and host scale.
//!
//! The design conventions in `docs/architecture.md` cover touch allocation, row-owned
//! interaction, press geometry, boundaries and state cues. They guide built-in
//! implementations; custom painters and product screens need separate review.
//! Boundary contrast follows the relevant colour-pair requirement in
//! [WCAG 1.4.11](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).
//! The convention alone does not establish product accessibility conformance.

use super::Metrics;
use crate::unit::{Dim, Eye, Free, Hand, Scale, Span};
use crate::Result;

/// The resolved control tokens (= `Theme::control`). Lengths are du; the ratios are unitless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlMetrics {
    /// Drawn control height. Use [`control_height`] to include the configured
    /// touch-target floor.
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
    /// Painted-silhouette growth distance applied during a press.
    pub press_grow: f32,
    /// A hairline — a separator, a divider inside a card.
    pub stroke_hairline: f32,
    /// Selected rail entry's leading bar thickness.
    pub accent_bar: f32,
    /// A resting control's edge.
    pub stroke_edge: f32,
    /// A drawn mark — a tick, a radio dot's ring, a focus ring.
    pub stroke_mark: f32,
    /// The gap between a control's silhouette and its focus ring.
    pub focus_gap: f32,
    /// Icon-only button disc size. The default du floor is 34.0 so the default
    /// 24 du icon fits within the disc; touch allocation is configured separately.
    pub icon_button: f32,
    /// Badge height. The du floor is independent of the displayed count.
    pub badge_h: f32,
    /// The padding on each side of a badge's digits.
    pub badge_pad: f32,
    /// Checkbox/radio mark size as a fraction of [`Self::mark_size`].
    /// The allocated touch target is configured separately.
    pub box_ratio: f32,
    /// A knob's diameter as a fraction of [`Self::mark_size`].
    pub knob_ratio: f32,
    /// A slider handle's width as a fraction of the track's thickness.
    pub handle_ratio: f32,
    /// Switch-track width as a multiple of its height.
    /// The allocated touch target is configured separately.
    pub switch_aspect: f32,
    /// A radio's dot diameter as a fraction of its ring.
    pub dot_ratio: f32,
    /// A segmented control's raised cell inset as a fraction of the strip's height.
    pub tick_ratio: f32,
    /// How much a slider's track swells while it is held.
    pub press_swell: f32,
    /// Configured alpha multiplier for disabled control paint paths.
    pub disabled_alpha: f32,
    /// A tinted badge's alpha behind an icon.
    pub badge_alpha: f32,
    /// A card's radius as a multiple of `metrics.corner_radius`.
    pub card_radius_ratio: f32,
    /// Progress track thickness as a fraction of slider-track thickness.
    ///
    /// The default ratio is 0.47. Both thicknesses change together when the slider
    /// track metric changes.
    pub progress_ratio: f32,
    /// Progress-ring stroke thickness as a fraction of its diameter.
    ///
    /// The default ratio is 0.12; check the resulting stroke at the intended size.
    pub ring_ratio: f32,
    /// Indeterminate segment length as a fraction of track length.
    ///
    /// The default is 0.30.
    pub indeterminate_span: f32,
    /// Status-lamp outer diameter as a multiple of the adjacent body-text size.
    /// It is independent of switch-knob geometry.
    pub lamp_ratio: f32,
    /// Badge digit size as a fraction of badge height.
    pub badge_text_ratio: f32,
    /// Dot-badge diameter as a fraction of numeric-badge height.
    pub badge_dot_ratio: f32,
    /// Largest count shown without an overflow suffix; the default is 99.
    /// Larger counts use `<max>+`.
    pub badge_max: u16,
    /// Translucent control-face alpha applied over its background.
    pub fill_alpha: f32,
    /// Minimum chip width as a multiple of its height.
    pub chip_min_aspect: f32,
    /// The most of a row's width a trailing value may take before it is elided.
    pub trailing_text_max: f32,
    /// Maximum number of segments accepted by a
    /// [`SegmentedControl`](crate::widgets::SegmentedControl).
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
    /// Drawn control height, defaulting to three body ems.
    /// Touch allocation uses a separate minimum based on the finger-size policy.
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
    /// Default control-size specification.
    ///
    /// Lengths combine text/finger or millimetre terms with logical-unit and pixel floors.
    /// The resulting size depends on the scale supplied to resolution.
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

/// Resolve drawn control height as the maximum of `ControlSpec::height` and
/// `metrics.touch_target`, combining text-relative sizing with the touch-size floor.
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
