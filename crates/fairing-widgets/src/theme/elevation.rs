//! **How high a container sits** — one token, two mechanisms.
//!
//! # Why height needed a token at all
//!
//! The crate was flat by policy: `Theme::egui_style` sets `window_shadow` and `popup_shadow` to
//! [`Shadow::NONE`] and nothing else ever drew one. Measured against the screens this element was
//! designed from, that is not a style — it is a missing cue. In base light a card
//! (`surface_variant`, white) measures **1.177** against the page it sits on (`surface`), which is
//! *below* the crate's own decorative hairline (`Outline` on a card, 1.503): today a card reads
//! fainter than a divider.
//!
//! The reference screens do not solve that with a bigger fill step. They solve it with a smaller
//! one plus a penumbra. Bella Tavola's item card is `#FAFAFA` on a `#F6F6F6` page — a fill step of
//! **1.035**, almost nothing — and the separation is a soft band under the card. La Table's card
//! and page are the same value to the pixel. A card reads as *a sheet lying on the page* rather
//! than *a region painted a different colour*, and that difference is most of what makes a shell
//! look like a product.
//!
//! # Why the geometry is inverted from a measurement and not chosen
//!
//! Bella Tavola's card at (31, 516)–(236, 808), on the raw PNG, averaged along each edge:
//!
//! * below it the ground falls 246 → 238.5 and recovers over **10 px**,
//! * beside it, **4 px**,
//! * above it, **0 px** — the page actually *brightens*, so there is no shadow there at all.
//!
//! epaint publishes its own extent as `spread + 0.5 × blur ± offset` ([`Shadow::margin`]), so with
//! `spread = 0` those three numbers invert exactly: `side = blur/2` gives `blur = 8 px`,
//! `bottom = blur/2 + offset` gives `offset = 6 px`, and `top = blur/2 − offset = −2`, which is the
//! measured zero. That screen's category chips are 88 px tall — a gloved 13 mm target — so
//! 1 px = 0.1477 mm and the pair is **1.18 mm / 0.89 mm**. The tokens are 1.20 and 0.90.
//!
//! The offset being *larger* than half the blur is the whole shape of the thing: it is what keeps a
//! shadow off the top of a card, which is what all four reference screens do and what neither
//! Material nor Fluent ever violates (Fluent's ramp is literally `y = 0.5 × blur`).
//!
//! # Why a dark palette gets a rim instead
//!
//! A cast shadow is light that did not arrive. On a near-black page there is none to withhold, and
//! both platform references say so outright: Apple's guidance is that drop shadows are ineffective
//! in Dark Mode and ships a second set of *Elevated* background colours instead, and Windows 11
//! "uses strokes instead of key shadows to outline an object" in dark theme. So
//! [`ElevationStyle::Auto`] — the only value a shell normally sets — casts on a light palette and
//! rims on a dark one, and [`ColorRole::Shadow`] flips polarity with the mode exactly as
//! [`ColorRole::Pressed`] already does.
//!
//! [`ElevationStyle::Cast`], [`Rim`](ElevationStyle::Rim) and [`Off`](ElevationStyle::Off) exist
//! for the panels whose physics a palette does not describe. The e-ink reference here is the case
//! that makes them first-class: a *bright* UI carrying no shadow anywhere, its cards bounded by a
//! single hairline. A display that cannot reproduce a 5 % feather does not degrade gracefully — it
//! dithers the ramp into rings — and that display is the embedded panel this crate targets.
//!
//! # Why `Raised` is decoration and only `Floating` is identified
//!
//! `rim_alpha` is not taste. It is the value at which the **`Floating`** rim clears WCAG 1.4.11's
//! 3.0 against the page in both dark palettes (3.03 and 3.01) while **`Raised`** stays at
//! 1.62–1.65 — inside the band [`ColorRole::Outline`] occupies and which this crate documents as
//! allowed to be a ghost. A thing that genuinely floats over content is identified; a card is
//! decorated.
//!
//! Which is also why the cast shadow is **not** gated: at 1.11 against its ground it is
//! figure-ground help for a sighted reader in bright ambient light, not a boundary. What identifies
//! a card is still its fill step, and, where an integrator wants a real boundary,
//! `Deco::stroke(control.stroke_hairline, ColorRole::Outline)`.
//!
//! # Why no widget may have one
//!
//! Elevation attaches to a container — [`Deco`](../../fairing/layout/struct.Deco.html) and the
//! shell's own chrome — and never to a control. A control's height already has a vocabulary:
//! `control.press_grow` on the painted silhouette and `ColorRole::Pressed` on the face. A shadow
//! that also deepened under the finger would be a second, slower signal for one event. (There is a
//! cheaper reason too: a shadow doubles a shape's tessellation, and thirty checkboxes on a settings
//! page are thirty shapes whose wasted interior is larger than the control.)

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
    /// A rim's ink as a multiple of a cast shadow's. A hairline covers a sixth of the width a
    /// feather does, so the same role has to be louder to carry the same weight.
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
    /// Both lengths are the **eye** family — a millimetre with a device-pixel floor — and neither
    /// carries a `finger` term. No finger lands on a shadow; a penumbra's job is to be seen at
    /// arm's length through glass, and a gloved hand does not need a bigger one.
    ///
    /// The pixel floors are 4 and 2. At α 14/255 spread over an 8 du ramp each step is 1.75/255;
    /// held at 4 physical pixels it is 3.5/255, which is where a gradient starts to band on an
    /// 8-bit panel. Below that the honest answer is [`ElevationStyle::Off`], not a thinner shadow.
    /// The floors are a *pair*: at 4 and 2 the defining property survives, because
    /// `top = 4/2 − 2 = 0` still puts nothing above the caster.
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

/// Paint `level` for a container whose drawn rect is `rect`.
///
/// `radius` is the caster's **own** radius. `Shadow::as_shape` adds `CornerRadius::from(spread)`
/// and the tessellator adds half the blur on top, so the penumbra comes out rounder than the thing
/// casting it — which is what a penumbra is, and must not be corrected.
///
/// The rim is `StrokeKind::Inside`, so an elevated container never measures wider than a flat one
/// and the rows inside it do not reflow when a manufacturer turns elevation on.
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

    /// **`Floating` is identified and `Raised` is decoration** — the whole reason `rim_alpha` has
    /// the value it does. A thing that floats over content clears WCAG 1.4.11's 3.0 against the
    /// page behind it; a card stays inside the band `Outline` occupies, which this crate documents
    /// as allowed to be a ghost.
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
