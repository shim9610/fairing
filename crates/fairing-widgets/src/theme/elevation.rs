//! Container elevation tokens and painting.
//!
//! `Elevation` selects a level; `ElevationStyle` selects cast-shadow, rim or disabled
//! presentation. `Auto` chooses cast shadows for a light palette and rims for a dark
//! palette. Explicit style choices override that selection.
//!
//! Cast geometry uses egui/epaint [`Shadow`] parameters. Rim colours and dimensions
//! use theme and elevation metrics. Floating surfaces use a stronger boundary than
//! decorative card separation. Check boundary contrast against the actual product
//! background, including custom palettes and translucent content.

use crate::theme::{ColorRole, Theme};
use crate::unit::{round_i8, round_u8, Dim, Scale, Span};
use crate::Result;
use egui::epaint::Shadow;
use egui::{CornerRadius, Painter, Rect, Stroke, StrokeKind};

/// How high a container sits. Three values; two of them draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Elevation {
    /// On the page. Draws nothing.
    #[default]
    None,
    /// A sheet lying on the page — a card, a tile, a grid cell.
    Raised,
    /// Over content that scrolls under it — an action bar, a toast, a heads-up banner, an open
    /// list. The only level that is *identified* rather than decorated; see the module doc.
    Floating,
}

/// Which mechanism expresses height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ElevationStyle {
    /// Cast on a light palette, rim on a dark one. The only value a shell normally sets.
    #[default]
    Auto,
    /// Always a cast shadow, whatever the mode.
    Cast,
    /// Always a rim on the caster's own edge.
    Rim,
    /// Draw nothing. For a panel that cannot reproduce a soft ramp — an e-ink or 16-level display
    /// dithers a 5 % feather into rings rather than fading it.
    Off,
}

/// The two things one frame paints height with.
///
/// **The only place either colour is made is [`Theme::elevate`]** — the `Ink::of` rule the controls
/// follow, applied to a container: every ratio is folded into one `gamma_multiply` in one function
/// rather than sprinkled down a paint path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Elevated {
    /// Under the caster. [`Shadow::NONE`] when the style is [`Rim`](ElevationStyle::Rim) or
    /// [`Off`](ElevationStyle::Off).
    pub shadow: Shadow,
    /// Inside the caster's silhouette. [`Stroke::NONE`] when the style is
    /// [`Cast`](ElevationStyle::Cast) or [`Off`](ElevationStyle::Off).
    pub rim: Stroke,
}

impl Elevated {
    /// Nothing drawn.
    pub const NONE: Self = Self {
        shadow: Shadow::NONE,
        rim: Stroke::NONE,
    };

    /// Whether this level draws nothing at all.
    #[must_use]
    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }
}

/// The resolved elevation tokens (`Theme::elevation`). Lengths in du; the rest unitless.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationMetrics {
    /// The penumbra's width.
    pub blur: f32,
    /// How far the shadow sits below the caster. **Larger than half the blur on purpose** — that is
    /// what keeps a shadow off the top of a card.
    pub offset_y: f32,
    /// [`Elevation::Floating`]'s geometry as a multiple of [`Elevation::Raised`]'s.
    pub float_scale: f32,
    /// [`Elevation::Floating`]'s ink as a multiple of [`Elevation::Raised`]'s.
    pub float_alpha: f32,
    /// Rim opacity as a multiple of cast-shadow opacity.
    pub rim_alpha: f32,
    /// The level a card is given when nothing says otherwise.
    pub card: Elevation,
    /// Which mechanism expresses height.
    pub style: ElevationStyle,
}

impl Default for ElevationMetrics {
    fn default() -> Self {
        ElevationSpec::default().resolve(&Scale::identity())
    }
}

/// The unitless part — fractions and choices, which no [`Scale`] touches (as `ControlRatios`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationRatios {
    /// [`ElevationMetrics::float_scale`].
    pub float_scale: f32,
    /// [`ElevationMetrics::float_alpha`].
    pub float_alpha: f32,
    /// [`ElevationMetrics::rim_alpha`].
    pub rim_alpha: f32,
    /// [`ElevationMetrics::card`].
    pub card: Elevation,
    /// [`ElevationMetrics::style`].
    pub style: ElevationStyle,
}

impl Default for ElevationRatios {
    fn default() -> Self {
        Self {
            // Raised 8/6 du → Floating 11/9 du, a 14.5 du = 2.30 mm bottom extent. Material 3's
            // level 4 — FABs and snackbars, the same population as a toast and an action bar — is
            // an ambient `0 6 10 4`, which is 15 dp = 2.38 mm. Ours lands 0.08 mm under it.
            float_scale: 1.5,
            // The value at which the Floating **rim** clears 3.0 against the page in both dark
            // palettes (3.03 / 3.01). Independently, `14 × 2.85 × 1.8 = 72/255 = 28.2 %` lands on
            // Fluent's published dark shadow opacity of 28 % to one decimal.
            float_alpha: 1.8,
            // A hairline covers 1.26 du where the feather covers 8, so it has to be louder for the
            // same weight. 2.85 puts a Raised rim at 1.62-1.65 against its card — the top of the
            // band `Outline` occupies, which is decoration — and Floating on 3.0.
            rim_alpha: 2.85,
            card: Elevation::Raised,
            style: ElevationStyle::Auto,
        }
    }
}

/// The elevation tokens before a [`Scale`] resolves them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationSpec {
    /// [`ElevationMetrics::blur`].
    pub blur: Span,
    /// [`ElevationMetrics::offset_y`].
    pub offset_y: Span,
    /// The unitless part.
    pub ratios: ElevationRatios,
}

impl Default for ElevationSpec {
    /// Default elevation specification. Blur and offset use millimetre terms with
    /// pixel floors and no finger term. Their values determine the cast-shadow extent;
    /// applications can select `ElevationStyle::Off` when no elevation is desired.
    fn default() -> Self {
        Self {
            blur: Span::fixed(Dim::mm(1.20)).min(Dim::px(4.0)),
            offset_y: Span::fixed(Dim::mm(0.90)).min(Dim::px(2.0)),
            ratios: ElevationRatios::default(),
        }
    }
}

impl ElevationSpec {
    /// **The switch that reproduces the render this element replaced**, exactly: no card
    /// elevation, no mechanism. The `MetricsSpec::legacy_du` analogue, for a panel that cannot
    /// carry a feather or a manufacturer who does not want one.
    #[must_use]
    pub fn flat() -> Self {
        Self {
            ratios: ElevationRatios {
                card: Elevation::None,
                style: ElevationStyle::Off,
                ..ElevationRatios::default()
            },
            ..Self::default()
        }
    }

    /// Resolve every length against `scale`.
    #[must_use]
    pub fn resolve(&self, scale: &Scale) -> ElevationMetrics {
        let x = self.ratios;
        ElevationMetrics {
            blur: self.blur.resolve(scale),
            offset_y: self.offset_y.resolve(scale),
            float_scale: x.float_scale,
            float_alpha: x.float_alpha,
            rim_alpha: x.rim_alpha,
            card: x.card,
            style: x.style,
        }
    }

    /// Check every token.
    ///
    /// # Errors
    ///
    /// [`Error::Config`](crate::Error::Config) naming the token that is wrong.
    pub fn validate(&self) -> Result<()> {
        let spans: [(&str, Span); 2] = [
            ("elevation.blur", self.blur),
            ("elevation.offset_y", self.offset_y),
        ];
        for (name, span) in spans {
            span.validate(name)?;
        }
        let x = self.ratios;
        let positive: [(&str, f32); 3] = [
            ("elevation.float_scale", x.float_scale),
            ("elevation.float_alpha", x.float_alpha),
            ("elevation.rim_alpha", x.rim_alpha),
        ];
        for (name, value) in positive {
            if !value.is_finite() || value <= 0.0 {
                return Err(crate::Error::Config(format!(
                    "[theme.elevation] {name} = {value} — a ratio has to be finite and above zero"
                )));
            }
        }
        Ok(())
    }
}

impl Theme {
    /// **The ink for one level — the only place an elevation colour is made.**
    ///
    /// [`ElevationStyle::Auto`] resolves here, against `self.dark`: a cast shadow on a light
    /// palette, a rim on a dark one. Both ratios are folded into a single `gamma_multiply` so that
    /// a level's alpha is arrived at once rather than by chaining multiplies that each round.
    #[must_use]
    pub fn elevate(&self, level: Elevation) -> Elevated {
        let e = &self.elevation;
        let (scale, alpha) = match level {
            Elevation::None => return Elevated::NONE,
            Elevation::Raised => (1.0, 1.0),
            Elevation::Floating => (e.float_scale, e.float_alpha),
        };
        let style = match e.style {
            ElevationStyle::Auto if self.dark => ElevationStyle::Rim,
            ElevationStyle::Auto => ElevationStyle::Cast,
            other => other,
        };
        let ink = self.color(ColorRole::Shadow);
        match style {
            // `Auto` cannot reach here — it resolved above — but a match has to be total and a
            // wildcard would swallow a style added later.
            ElevationStyle::Off | ElevationStyle::Auto => Elevated::NONE,
            ElevationStyle::Cast => Elevated {
                shadow: Shadow {
                    offset: [0, round_i8(e.offset_y * scale)],
                    blur: round_u8(e.blur * scale),
                    spread: 0,
                    color: ink.gamma_multiply(alpha),
                },
                rim: Stroke::NONE,
            },
            ElevationStyle::Rim => Elevated {
                shadow: Shadow::NONE,
                rim: Stroke::new(
                    self.control.stroke_hairline,
                    ink.gamma_multiply(e.rim_alpha * alpha),
                ),
            },
        }
    }

    /// The level a card is given when nothing says otherwise.
    #[must_use]
    pub fn card_elevation(&self) -> Elevation {
        self.elevation.card
    }
}

/// Paint `level` for a container at `rect`, using its own corner radius.
///
/// `Shadow::as_shape` and tessellation expand the shadow geometry for spread and
/// blur. The rim uses `StrokeKind::Inside`, so it does not increase layout bounds.
pub fn paint_elevation(
    painter: &Painter,
    theme: &Theme,
    rect: Rect,
    radius: CornerRadius,
    level: Elevation,
) {
    let ink = theme.elevate(level);
    if ink.is_none() {
        return;
    }
    if ink.shadow != Shadow::NONE {
        painter.add(ink.shadow.as_shape(rect, radius));
    }
    if ink.rim != Stroke::NONE {
        painter.rect_stroke(rect, radius, ink.rim, StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::{Elevated, Elevation, ElevationSpec, ElevationStyle};
    use crate::theme::{contrast, ColorRole, Palette, Preset, Theme};

    /// A theme on one preset and mode. `Theme` has no such constructor of its own —
    /// the shell always arrives through `from_inputs` — and every test here needs all
    /// four palettes.
    fn theme_of(preset: Preset, dark: bool) -> Theme {
        Theme {
            palette: Palette::preset(preset, dark),
            dark,
            ..Theme::dark()
        }
    }
    use crate::unit::{Scale, ScaleConfidence, ScalePolicy, ScaleSource};

    /// A scale for a panel of this density, the way `metrics_spec`'s own tests build one.
    fn scale_for(px_per_mm: f32) -> Scale {
        Scale::resolve(
            px_per_mm,
            &ScalePolicy::default(),
            ScaleSource::Backend,
            ScaleConfidence::Measured,
            egui::vec2(800.0, 480.0),
        )
    }

    /// **Nothing is drawn above the caster.** Not one of the four reference screens puts a shadow
    /// over a card, Fluent's own ramp is `offset = 0.5 × blur`, and epaint publishes the extent as
    /// `spread + 0.5 × blur ± offset` — so the property is exactly `offset >= blur / 2`, and it has
    /// to survive both the ratio that scales `Floating` and the pixel floors that catch a coarse
    /// panel.
    #[test]
    fn a_shadow_never_reaches_above_the_thing_casting_it() {
        for density in [4.0_f32, 6.67, 10.0, 20.0, 40.0] {
            {
                let scale = scale_for(density);
                let m = ElevationSpec::default().resolve(&scale);
                for (name, k) in [("raised", 1.0), ("floating", m.float_scale)] {
                    let top = m.blur * k / 2.0 - m.offset_y * k;
                    assert!(
                        top <= 0.0,
                        "{name} at {density} px/mm: the penumbra reaches {top:.2} du above the \
                         caster"
                    );
                }
            }
        }
    }

    /// Check floating-rim contrast against the supported page backgrounds.
    /// Card and inset rims use decorative styling.
    #[test]
    fn only_the_floating_rim_is_a_boundary_you_can_measure() {
        for &preset in Preset::ALL {
            let theme = theme_of(preset, true);
            let name = preset.as_str();
            let card = theme.color(ColorRole::SurfaceVariant);
            let page = theme.color(ColorRole::Surface);
            // `card.blend(ink)`, not the other way round: `blend`'s receiver is the layer
            // **behind**, so the reverse composites the card over the rim and measures the
            // card against itself.
            let rim = |level| card.blend(theme.elevate(level).rim.color);

            let floating = contrast(rim(Elevation::Floating), page);
            assert!(
                floating >= 3.0,
                "{name} dark: a floating rim measures {floating:.2} against the page"
            );
            let raised = contrast(rim(Elevation::Raised), card);
            assert!(
                (1.15..=1.80).contains(&raised),
                "{name} dark: a raised rim measures {raised:.2} on its card — outside the band \
                 `Outline` occupies, so it is no longer decoration"
            );
        }
    }

    /// **`Auto` is the mode's own mechanism**, in every preset: a cast shadow where there is light
    /// to withhold and a rim where there is not.
    #[test]
    fn auto_casts_on_a_light_palette_and_rims_on_a_dark_one() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let theme = theme_of(preset, dark);
                let ink = theme.elevate(Elevation::Raised);
                let (name, mode) = (preset.as_str(), if dark { "dark" } else { "light" });
                if dark {
                    assert_eq!(ink.shadow, egui::epaint::Shadow::NONE, "{name} {mode}");
                    assert!(ink.rim.width > 0.0, "{name} {mode}: no rim");
                } else {
                    assert_eq!(ink.rim, egui::Stroke::NONE, "{name} {mode}");
                    assert!(ink.shadow.blur > 0, "{name} {mode}: no blur");
                }
            }
        }
    }

    /// **`flat()` restores the render this element replaced**, so a manufacturer whose panel cannot
    /// carry a feather has one switch rather than a search for every caller.
    #[test]
    fn the_flat_spec_draws_nothing_at_any_level() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let mut theme = theme_of(preset, dark);
                theme.elevation = ElevationSpec::flat().resolve(&Scale::identity());
                for level in [Elevation::None, Elevation::Raised, Elevation::Floating] {
                    assert_eq!(theme.elevate(level), Elevated::NONE, "{level:?}");
                }
                assert_eq!(theme.card_elevation(), Elevation::None);
            }
        }
    }

    /// `Off` and `None` draw nothing whichever way they are reached, and a style the shell pins
    /// beats the mode.
    #[test]
    fn a_pinned_style_beats_the_mode_and_off_beats_everything() {
        let mut theme = theme_of(Preset::Base, true);
        theme.elevation.style = ElevationStyle::Cast;
        assert!(
            theme.elevate(Elevation::Raised).shadow.blur > 0,
            "a pinned Cast did not cast on a dark palette"
        );
        theme.elevation.style = ElevationStyle::Off;
        assert_eq!(theme.elevate(Elevation::Raised), Elevated::NONE);
        theme.elevation.style = ElevationStyle::Auto;
        assert_eq!(theme.elevate(Elevation::None), Elevated::NONE);
    }

    /// A bad token is a named config error, not a panic and not a silent zero.
    #[test]
    fn validate_names_the_token_that_is_wrong() {
        assert!(ElevationSpec::default().validate().is_ok());
        assert!(ElevationSpec::flat().validate().is_ok());
        let mut spec = ElevationSpec::default();
        spec.ratios.rim_alpha = 0.0;
        let err = spec.validate().err().map(|e| e.to_string());
        assert!(
            err.as_deref().is_some_and(|e| e.contains("rim_alpha")),
            "a zero ratio should be a config error naming the token, got {err:?}"
        );
    }
}
