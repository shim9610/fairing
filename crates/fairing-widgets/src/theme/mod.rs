//! The theme — the palette, metrics and motion tokens, and applying them to egui's `Style`
//! (`theme/`, the render principles).
//!
//! Touch first: no hover colour, with the tuning done on the pressed (`active`) state. No large
//! blurred shadows; flat surfaces with 1 px borders.

mod fade;

pub use fade::ThemeFade;

mod components;
mod control;
mod elevation;
mod metrics_spec;
pub use components::{
    ButtonMetrics, ComponentMetrics, ComponentSpec, HeadsUpMetrics, ListRowMetrics,
    OverviewMetrics, PopoverMetrics, ShadeMetrics, SliderMetrics, SwitchMetrics, ToastMetrics,
};
pub use control::{
    card_radius, control_height, lamp_size, ControlMetrics, ControlRatios, ControlSpec,
};
pub use elevation::{
    paint_elevation, Elevated, Elevation, ElevationMetrics, ElevationRatios, ElevationSpec,
    ElevationStyle,
};
pub use metrics_spec::MetricsSpec;

use crate::config::{MotionConfig, ThemeConfig};
use crate::error::{Error, Result};
use crate::motion::{Easing, RubberBand, Spring, Tween};
use egui::{Color32, Stroke};
use std::collections::BTreeMap;
use std::time::Duration;

/// A palette role (`ColorRole`). Referred to in the config file by its `snake_case` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ColorRole {
    /// The screen background (the desktop's base).
    Background,
    /// A surface (panels, cards, screen backgrounds).
    Surface,
    /// An accented surface (a pressed row, a selection).
    SurfaceVariant,
    /// Text and icons on a surface.
    OnSurface,
    /// Dimmed text (disabled, secondary).
    Muted,
    /// Brand and accent.
    Primary,
    /// Text on the accent.
    OnPrimary,
    /// Danger (battery at 20 % or below, errors).
    Danger,
    /// A warning.
    Warning,
    /// Success.
    Success,
    /// The focus outline. **A different event from a press (`Primary`)** — this is the outline
    /// a text field raises when it takes focus, and `Theme::egui_style` puts it in
    /// `visuals.selection.stroke`.
    Focus,
    /// The scrim (the dim).
    Scrim,
    /// Borders — the decorative line between two containers: a divider inside a card, the hairline
    /// where one surface meets another. It measures 1.15–1.65 against the grounds on purpose.
    Outline,
    /// **A control's own boundary** — the ring of an unticked checkbox, an unselected radio, an off
    /// switch's track, a slider's empty side.
    ///
    /// It is separate from both of its neighbours because it answers a different rule. `Outline` is
    /// decoration and is allowed to be a ghost; this is what identifies a control at all, so it is
    /// gated at WCAG 1.4.11's **3.0**. `Muted` is *text*, gated at 4.5 — borrowing it, as every
    /// control did, drew each boundary as loud as the label beside it, and a card holding a
    /// checkbox, a radio, a segmented strip and a slider came out as four bright outlines.
    ControlEdge,
    /// The press tint.
    Pressed,
    /// **Height.** The ink a container casts under itself on a light ground, and the rim it wears
    /// on its own edge on a dark one — see [`Theme::elevate`].
    ///
    /// It is an alpha role like [`Self::Scrim`] and [`Self::Pressed`], and like them it carries one
    /// number whose polarity flips with the mode: `from_black_alpha(14)` light,
    /// `from_white_alpha(14)` dark. It is **not** gated at 3.0, and that is deliberate — a cast
    /// shadow measures about 1.11 against the ground it falls on, which is figure-ground help for a
    /// sighted reader in bright light and not a boundary that identifies anything. What identifies
    /// a card stays its fill step and, where a real boundary is wanted, [`Self::Outline`].
    Shadow,
    /// **The face of the pull-down shade** — the curtain, or the floating card with
    /// `[overlay] reveal = "card"`, and the tile pucks on it. Unset, it is [`Self::Surface`], so a
    /// palette that does not name it looks as it always did; set it (`shade_surface` under
    /// `[theme.palette]`, or [`Palette::shade_surface`]) to give the shade a colour of its own
    /// without touching every screen's surface. Text on it stays [`Self::OnSurface`], so keep the
    /// two readable together.
    ShadeSurface,
}

impl ColorRole {
    /// Look one up by its config file name (`"on_surface"`).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "background" => Self::Background,
            "surface" => Self::Surface,
            "surface_variant" => Self::SurfaceVariant,
            "on_surface" => Self::OnSurface,
            "muted" => Self::Muted,
            "primary" => Self::Primary,
            "on_primary" => Self::OnPrimary,
            "danger" => Self::Danger,
            "warning" => Self::Warning,
            "success" => Self::Success,
            "focus" => Self::Focus,
            "scrim" => Self::Scrim,
            "outline" => Self::Outline,
            "control_edge" => Self::ControlEdge,
            "pressed" => Self::Pressed,
            "shadow" => Self::Shadow,
            "shade_surface" => Self::ShadeSurface,
            _ => return None,
        })
    }
}

/// The colour per role.
///
/// One `Color32` per role, and an optional face for the shade, so it is `Copy` — the places that hold a pair
/// and the crossfade pass it by value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// [`ColorRole::Background`].
    pub background: Color32,
    /// [`ColorRole::Surface`].
    pub surface: Color32,
    /// [`ColorRole::SurfaceVariant`].
    pub surface_variant: Color32,
    /// [`ColorRole::OnSurface`].
    pub on_surface: Color32,
    /// [`ColorRole::Muted`].
    pub muted: Color32,
    /// [`ColorRole::Primary`].
    pub primary: Color32,
    /// [`ColorRole::OnPrimary`].
    pub on_primary: Color32,
    /// [`ColorRole::Danger`].
    pub danger: Color32,
    /// [`ColorRole::Warning`].
    pub warning: Color32,
    /// [`ColorRole::Success`].
    pub success: Color32,
    /// [`ColorRole::Focus`].
    pub focus: Color32,
    /// [`ColorRole::Scrim`].
    pub scrim: Color32,
    /// [`ColorRole::Outline`].
    pub outline: Color32,
    /// [`ColorRole::ControlEdge`].
    pub control_edge: Color32,
    /// [`ColorRole::Pressed`].
    pub pressed: Color32,
    /// [`ColorRole::Shadow`].
    pub shadow: Color32,
    /// [`ColorRole::ShadeSurface`]. `None` follows [`Self::surface`], including a `surface` changed
    /// later by an override.
    pub shade_surface: Option<Color32>,
}

/// A hairline border — the thinnest thing the eye still catches. Unrelated to touch targets, so not a token.
const HAIRLINE: f32 = 1.0;
/// The border width that **announces an event**, like a press or focus. Touch has no hover, so this is the only signal.
const ACCENT_STROKE: f32 = 2.0;
/// The opacity of the text selection background.
const SELECTION_ALPHA: f32 = 0.35;
/// Button side padding = `screen_inset × this`.
const BUTTON_PAD_X: f32 = 4.0 / 3.0;
/// Button vertical padding = `screen_inset × this`.
const BUTTON_PAD_Y: f32 = 5.0 / 6.0;
/// The gap between widgets = `screen_inset × this`.
const ITEM_GAP: f32 = 5.0 / 6.0;
/// A text field's default width = `touch_target × this`. Enough for a one-line name or note.
const TEXT_EDIT_SPAN: f32 = 7.5;

/// A palette preset.
///
/// Adding one costs a variant, two arms in [`Preset::parse`] / [`Preset::as_str`], and one entry
/// in the [`Preset::ALL`] slice. It is outside the brand feature — a palette is only a handful of
/// `Color32`s, so it survives `brand` being off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
// **No `#[non_exhaustive]`**. It carried one until the element layer became its own
// crate, at which point `fairing` itself became a downstream crate and the attribute
// started forcing `_ =>` arms inside this project. A new preset is meant to break the matches.
pub enum Preset {
    /// Neutral dark and light. The default, and what [`Palette::dark`] / [`Palette::light`] are.
    #[default]
    Base,
    /// Abyss — the deep-sea palette drawn from the manta art.
    Abyss,
    /// Linen — warm neutrals and a quiet sage, for an appliance that lives in a room.
    ///
    /// The other two are instrument palettes: they assume a panel that is looked *at*, in a
    /// workshop or a dark cabin, and they are built dark-first. This one assumes a panel that sits
    /// in a kitchen beside oak and wool and is mostly **not** being looked at — so it is built
    /// light-first, its greys carry a little red and yellow rather than none at all, and the accent
    /// is a sage that reads as a material rather than as a signal. The alarm colours stay
    /// saturated, which is the point of restraining the accent: an alarm has to outrank it.
    Linen,
}

impl Preset {
    /// The `[theme] preset` name. An unknown name is `None`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "base" => Some(Self::Base),
            "abyss" => Some(Self::Abyss),
            "linen" => Some(Self::Linen),
            _ => None,
        }
    }

    /// The name used in the config.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Abyss => "abyss",
            Self::Linen => "linen",
        }
    }

    /// Every preset. The contrast test iterates it.
    ///
    /// **A slice, not an array.** A new preset breaks the matches on purpose, but code that only
    /// walks the presets should not break with it — `[Self; 3]` would, the moment the length
    /// changed.
    pub const ALL: &'static [Self] = &[Self::Base, Self::Abyss, Self::Linen];
}

impl Palette {
    /// Preset × dark/light. **The single source of every palette**.
    #[must_use]
    pub fn preset(preset: Preset, dark: bool) -> Self {
        match (preset, dark) {
            (Preset::Base, true) => Self::base_dark(),
            (Preset::Base, false) => Self::base_light(),
            (Preset::Abyss, true) => Self::abyss_dark(),
            (Preset::Abyss, false) => Self::abyss_light(),
            (Preset::Linen, true) => Self::linen_dark(),
            (Preset::Linen, false) => Self::linen_light(),
        }
    }

    /// **Linen light — the one this preset is for**.
    ///
    /// Warm neutrals rather than neutral ones: every grey here carries a little red and yellow, so
    /// the page sits beside oak and wool instead of against them. Nothing is pure white — the
    /// lightest surface is `#FAF8F4`, because a white card in a room with daylight in it reads as a
    /// hole rather than as paper.
    ///
    /// The accent is a **sage at 4.74 against the page**, deliberately quieter than its own
    /// `danger` at 5.56. An appliance's accent marks what you chose; the alarm has to be able to
    /// outrank it, and on a palette this soft that only happens if the accent gives way first.
    fn linen_light() -> Self {
        Self {
            background: Color32::from_rgb(0xe4, 0xde, 0xd4),
            surface: Color32::from_rgb(0xed, 0xe8, 0xdf),
            surface_variant: Color32::from_rgb(0xfa, 0xf8, 0xf4),
            on_surface: Color32::from_rgb(0x23, 0x21, 0x1c),
            muted: Color32::from_rgb(0x67, 0x60, 0x4f),
            primary: Color32::from_rgb(0x4c, 0x6d, 0x56),
            on_primary: Color32::WHITE,
            danger: Color32::from_rgb(0x9e, 0x3a, 0x2c),
            warning: Color32::from_rgb(0x85, 0x54, 0x15),
            success: Color32::from_rgb(0x3b, 0x64, 0x46),
            focus: Color32::from_rgb(0x5f, 0x83, 0x69),
            scrim: Color32::from_black_alpha(100),
            outline: Color32::from_rgb(0xda, 0xd3, 0xc6),
            control_edge: Color32::from_rgb(0x8a, 0x83, 0x73),
            pressed: Color32::from_black_alpha(24),
            shadow: Color32::from_black_alpha(14),
            shade_surface: None,
        }
    }

    /// Linen dark — the same room after dark, not a different instrument.
    ///
    /// Smoked oak rather than black: the greys keep the warm cast the light form has, so switching
    /// at dusk changes the light in the room and not the product. The sage is lifted only as far as
    /// its `danger` still beats it (5.54 against 6.62).
    fn linen_dark() -> Self {
        Self {
            background: Color32::from_rgb(0x15, 0x13, 0x0f),
            // The page and the card are set **against the rim, not by eye**: a floating rim has to
            // clear 3.0 against the page and a raised one stay inside `Outline`'s 1.15..1.80 band.
            // At `#1E1B16` / `#2A261F` the floating rim measured 2.87 — a boundary you
            // cannot quite see. One step apart in each direction puts it at 3.06.
            surface: Color32::from_rgb(0x1c, 0x19, 0x14),
            surface_variant: Color32::from_rgb(0x2e, 0x2a, 0x22),
            on_surface: Color32::from_rgb(0xf1, 0xec, 0xe1),
            muted: Color32::from_rgb(0xa6, 0x9d, 0x8b),
            primary: Color32::from_rgb(0x76, 0x9b, 0x82),
            // Dark ink on a mid sage — white measures 2.63 here, under the 4.5 a label needs.
            on_primary: Color32::from_rgb(0x10, 0x15, 0x08),
            danger: Color32::from_rgb(0xe0, 0x8a, 0x7d),
            warning: Color32::from_rgb(0xd9, 0xa8, 0x5c),
            success: Color32::from_rgb(0x8f, 0xbf, 0x9b),
            focus: Color32::from_rgb(0x9c, 0xbf, 0xa6),
            scrim: Color32::from_black_alpha(130),
            outline: Color32::from_rgb(0x3a, 0x35, 0x2c),
            control_edge: Color32::from_rgb(0x7e, 0x76, 0x66),
            pressed: Color32::from_white_alpha(20),
            // **White, not black.** On a dark palette height is a rim inside the silhouette,
            // and a black rim on a near-black page measures 1.00 — no boundary at all.
            shadow: Color32::from_white_alpha(14),
            shade_surface: None,
        }
    }

    /// Abyss dark — the deep sea. Darker than the reference art, because the
    /// labels (`on_surface`) have to be readable over water. `scrim` is α150, so this preset
    /// darkens the shade scrim with it.
    fn abyss_dark() -> Self {
        Self {
            background: Color32::from_rgb(0x04, 0x12, 0x1f),
            surface: Color32::from_rgb(0x0a, 0x1e, 0x31),
            surface_variant: Color32::from_rgb(0x12, 0x30, 0x49),
            on_surface: Color32::from_rgb(0xe6, 0xf2, 0xfa),
            muted: Color32::from_rgb(0x7f, 0xa0, 0xb8),
            // **Deep enough to let an alarm be louder than it.** `#38C6E8` measured **8.38** against
            // this preset's page while its own `danger` measured 6.09 — so on the deep-sea preset
            // the accent shouted over the error colour, and an error had nothing left to say with.
            // `#00A2CA` is the same hue (192°) at the same full saturation, two steps down: 5.65 on
            // the page, under danger's 6.09 and well under warning's 10.52 and success's 10.03.
            //
            // The order this restores is the one base dark already had, and it is the order a
            // machine panel wants: the accent marks what is *chosen*, and the four severity colours
            // are the only things allowed to interrupt.
            primary: Color32::from_rgb(0x00, 0xa2, 0xca),
            // Not white — the contrast over `primary` falls to 3.4:1, under the 4.5 a label needs.
            // The dark ink reads at 6.32, and unlike base dark this preset can afford it: a bright
            // cyan is what the reference art *is*, and deepening it far enough to carry white would
            // have cost the preset its identity rather than its loudness.
            on_primary: Color32::from_rgb(0x04, 0x12, 0x1f),
            danger: Color32::from_rgb(0xff, 0x6b, 0x6b),
            warning: Color32::from_rgb(0xff, 0xc2, 0x4d),
            success: Color32::from_rgb(0x43, 0xe0, 0xa8),
            focus: Color32::from_rgb(0x7f, 0xe3, 0xff),
            scrim: Color32::from_black_alpha(150),
            outline: Color32::from_rgb(0x1b, 0x3a, 0x55),
            control_edge: Color32::from_rgb(0x56, 0x7d, 0x9a),
            pressed: Color32::from_white_alpha(26),
            shadow: Color32::from_white_alpha(14),
            shade_surface: None,
        }
    }

    /// Abyss light — the bright water at the top of the reference art, a shallow sea.
    ///
    /// `primary` is **a computed step down** from the literal reading of the art, `#0E7FA8`
    /// (4.55:1 against white text, 0.05 of headroom): `#0B6A8C` is 6.08:1.
    fn abyss_light() -> Self {
        Self {
            background: Color32::from_rgb(0xe8, 0xf3, 0xfb),
            surface: Color32::WHITE,
            surface_variant: Color32::from_rgb(0xd6, 0xe7, 0xf5),
            on_surface: Color32::from_rgb(0x0b, 0x22, 0x37),
            // 4.03 against the `#D6E7F5` card was under the 4.5 a subtitle needs; this is 4.59
            // (and 5.81 on `surface`). One step, computed, not an eyeballed nudge.
            muted: Color32::from_rgb(0x4e, 0x68, 0x80),
            primary: Color32::from_rgb(0x0b, 0x6a, 0x8c),
            on_primary: Color32::WHITE,
            // As `muted` above: 4.28 on the card was short, this is 4.59 (5.80 on `surface`).
            danger: Color32::from_rgb(0xbd, 0x30, 0x2b),
            warning: Color32::from_rgb(0xa9, 0x6a, 0x00),
            success: Color32::from_rgb(0x0e, 0x7a, 0x55),
            focus: Color32::from_rgb(0x0b, 0x6a, 0x8c),
            scrim: Color32::from_black_alpha(110),
            outline: Color32::from_rgb(0xb7, 0xcc, 0xdf),
            control_edge: Color32::from_rgb(0x64, 0x83, 0xa0),
            pressed: Color32::from_black_alpha(24),
            shadow: Color32::from_black_alpha(14),
            shade_surface: None,
        }
    }

    /// The dark palette. An alias for [`Palette::preset(Preset::Base, true)`](Palette::preset).
    #[must_use]
    pub fn dark() -> Self {
        Self::base_dark()
    }

    /// Base dark — the system-UI shape at the dark end.
    ///
    /// The three reference platforms agree here too: the page is a near-black neutral and the card
    /// on it is a step **lighter** (iOS `#1C1C1E` on `#000000`, Windows 11 `#2B2B2B` on `#202020`),
    /// so `surface_variant` stays above `surface` in both modes. The accent is the same steel blue
    /// as the light end, one step lighter — deep enough to carry white text on a filled control,
    /// which is why `on_primary` is white here and not the dark ink a pastel accent would force.
    fn base_dark() -> Self {
        Self {
            background: Color32::from_rgb(0x0b, 0x0b, 0x0d),
            surface: Color32::from_rgb(0x14, 0x14, 0x16),
            surface_variant: Color32::from_rgb(0x24, 0x24, 0x28),
            on_surface: Color32::from_rgb(0xf0, 0xf0, 0xf3),
            muted: Color32::from_rgb(0x93, 0x93, 0x9d),
            primary: Color32::from_rgb(0x28, 0x78, 0xa9),
            // **White, and the accent moved to meet it.** The rule used to be that a dark ground
            // forces a light accent and therefore dark label text, and that is what produced a
            // pastel `#57A9FF` with near-black on it — read, correctly, as toy plastic. The way out
            // is not a lower floor but a deeper accent: `#2878A9` carries white at 4.83:1, clearing
            // the 4.5 text floor outright. Its own contrast against the surfaces is 4.07 / 3.81 /
            // 3.20, over the 3.0 a filled shape needs.
            //
            // It is a **steel** blue and not a pure one, and that is the second thing this value
            // fixes. A fully saturated accent — `#1B6EF3`, which this replaces — is the one every
            // stock panel ships, and at the sizes a device UI fills it with (a whole switch track,
            // a whole slider) the saturation is what reads as cheap. Rotating the hue toward cyan
            // and dropping the chroma keeps the job and leaves the four severity colours louder
            // than the accent, which is the order a machine panel wants.
            //
            // The consequence has to be said: `Primary` is now **never** a text colour. 3.81 on
            // `surface` clears the non-text floor and not the text one, so an accent tick, an
            // accent fill and an accent icon are all fine and accent *words* are not.
            on_primary: Color32::WHITE,
            danger: Color32::from_rgb(0xff, 0x6b, 0x5e),
            warning: Color32::from_rgb(0xe9, 0xa8, 0x3b),
            success: Color32::from_rgb(0x4f, 0xd0, 0x7a),
            focus: Color32::from_rgb(0x54, 0x9a, 0xc5),
            scrim: Color32::from_black_alpha(128),
            outline: Color32::from_rgb(0x30, 0x30, 0x36),
            control_edge: Color32::from_rgb(0x70, 0x70, 0x7b),
            pressed: Color32::from_white_alpha(28),
            shadow: Color32::from_white_alpha(14),
            shade_surface: None,
        }
    }

    /// The light palette. An alias for [`Palette::preset(Preset::Base, false)`](Palette::preset).
    #[must_use]
    pub fn light() -> Self {
        Self::base_light()
    }

    /// Base light — **a grey page, white cards, one system blue**.
    ///
    /// # Where the values come from
    ///
    /// The reference is the family this shell stands beside on real hardware — iOS, One UI and
    /// Windows 11 — because a device shell's *default* has to read as a competent system UI, not as
    /// a point of view. All three agree on three things and this preset takes all three:
    ///
    /// 1. **The page is a light grey and the card on it is white**, not the other way round. iOS
    ///    groups `#FFFFFF` cards on a `#F2F2F7` page; Windows 11 puts `#FBFBFB` cards on `#F3F3F3`.
    ///    So [`ColorRole::Surface`] (the screen ground and the bars) is the grey and
    ///    [`ColorRole::SurfaceVariant`] (cards, tiles, plain buttons) is the white — a card is
    ///    **lifted**, not recessed. [`ColorRole::Background`] is a step deeper again, which is what
    ///    makes a text field read as a hole to write in.
    /// 2. **The neutrals are neutral** — neither a warm tint nor the blue-grey ramp every UI kit
    ///    ships. The three channels stay within a few points of each other.
    /// 3. **The accent is a system blue.** Green, amber and red are already
    ///    [`Success`](ColorRole::Success), [`Warning`](ColorRole::Warning) and
    ///    [`Danger`](ColorRole::Danger), which is why every platform lands in the blue arc.
    ///    `#106AA2` is a steel blue: deeper than Windows 11's `#0078D4`, and rotated off the pure
    ///    arc those three sit on. Depth is the accessibility half — `#0078D4` is 4.53:1 against
    ///    white text, clearing the 4.5 floor by 0.03 with no headroom, and iOS `#007AFF` (4.02) and
    ///    One UI `#0381FE` (3.77) do not clear it at all, while `#106AA2` is 5.82:1. The rotation
    ///    is the other half: a fully saturated blue filling a whole slider reads as cheap, and a
    ///    device shell fills large areas with its accent.
    ///
    /// [`ColorRole::Focus`] is deliberately **not** `primary`: a focus ring drawn on top of a
    /// pressed control has to be visible against the press.
    fn base_light() -> Self {
        Self {
            background: Color32::from_rgb(0xe1, 0xe1, 0xe8),
            surface: Color32::from_rgb(0xec, 0xec, 0xf1),
            surface_variant: Color32::WHITE,
            on_surface: Color32::from_rgb(0x1a, 0x1a, 0x1c),
            muted: Color32::from_rgb(0x63, 0x63, 0x6b),
            primary: Color32::from_rgb(0x10, 0x6a, 0xa2),
            on_primary: Color32::WHITE,
            danger: Color32::from_rgb(0xc0, 0x32, 0x2a),
            warning: Color32::from_rgb(0x8f, 0x5e, 0x00),
            success: Color32::from_rgb(0x1a, 0x7f, 0x37),
            // Following the accent fixed a floor this role was under: the old `#3A86E8` measured
            // **2.80** against `background`, and a focus ring is non-text at 3.0. This is 3.15.
            focus: Color32::from_rgb(0x27, 0x84, 0xbe),
            scrim: Color32::from_black_alpha(100),
            outline: Color32::from_rgb(0xd2, 0xd2, 0xda),
            control_edge: Color32::from_rgb(0x85, 0x85, 0x8e),
            pressed: Color32::from_black_alpha(24),
            shadow: Color32::from_black_alpha(14),
            shade_surface: None,
        }
    }

    /// Look up a role colour.
    #[must_use]
    pub fn get(&self, role: ColorRole) -> Color32 {
        match role {
            ColorRole::Background => self.background,
            ColorRole::Surface => self.surface,
            ColorRole::SurfaceVariant => self.surface_variant,
            ColorRole::OnSurface => self.on_surface,
            ColorRole::Muted => self.muted,
            ColorRole::Primary => self.primary,
            ColorRole::OnPrimary => self.on_primary,
            ColorRole::Danger => self.danger,
            ColorRole::Warning => self.warning,
            ColorRole::Success => self.success,
            ColorRole::Focus => self.focus,
            ColorRole::Scrim => self.scrim,
            ColorRole::Outline => self.outline,
            ColorRole::ControlEdge => self.control_edge,
            ColorRole::Pressed => self.pressed,
            ColorRole::Shadow => self.shadow,
            ColorRole::ShadeSurface => self.shade_surface.unwrap_or(self.surface),
        }
    }

    /// Replace a role colour. The `[theme.palette]` overrides and assembling a palette in code take the same path.
    pub fn set(&mut self, role: ColorRole, color: Color32) {
        let slot = match role {
            ColorRole::Background => &mut self.background,
            ColorRole::Surface => &mut self.surface,
            ColorRole::SurfaceVariant => &mut self.surface_variant,
            ColorRole::OnSurface => &mut self.on_surface,
            ColorRole::Muted => &mut self.muted,
            ColorRole::Primary => &mut self.primary,
            ColorRole::OnPrimary => &mut self.on_primary,
            ColorRole::Danger => &mut self.danger,
            ColorRole::Warning => &mut self.warning,
            ColorRole::Success => &mut self.success,
            ColorRole::Focus => &mut self.focus,
            ColorRole::Scrim => &mut self.scrim,
            ColorRole::Outline => &mut self.outline,
            ColorRole::ControlEdge => &mut self.control_edge,
            ColorRole::Pressed => &mut self.pressed,
            ColorRole::Shadow => &mut self.shadow,
            ColorRole::ShadeSurface => {
                self.shade_surface = Some(color);
                return;
            }
        };
        *slot = color;
    }

    /// Apply the `[theme.palette]` overrides (role name → `"#RRGGBB"` or `"#RRGGBBAA"`).
    ///
    /// Alpha is accepted because of [`ColorRole::Scrim`] and [`ColorRole::Pressed`] — being
    /// translucent is **the reason those two roles exist**, and six digits left no way to touch
    /// them from config.
    ///
    /// # Errors
    /// [`Error::Config`] if the role name is not in [`ColorRole::parse`] or the value is not a
    /// colour — swallowing a typo leaves nothing but "why is the colour not changing".
    pub fn apply_overrides(&mut self, overrides: &BTreeMap<String, String>) -> Result<()> {
        for (name, value) in overrides {
            let role = ColorRole::parse(name).ok_or_else(|| {
                Error::Config(format!(
                    "[theme.palette] `{name}` is not a palette role (see ColorRole)"
                ))
            })?;
            let Some(crate::icons::IconColor::Fixed(color)) = crate::icons::IconColor::parse(value)
            else {
                return Err(Error::Config(format!(
                    "[theme.palette] {name} = \"{value}\" is neither \"#RRGGBB\" nor \"#RRGGBBAA\""
                )));
            };
            self.set(role, color);
        }
        Ok(())
    }

    /// Interpolate between two palettes (A7, the theme switch: called every frame for
    /// 200 ms and applied to `Visuals`). Every role is interpolated per channel with
    /// `Color32::lerp_to_gamma`, so dark and light join with no colour jump.
    #[must_use]
    pub fn lerp(&self, other: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            background: self.background.lerp_to_gamma(other.background, t),
            surface: self.surface.lerp_to_gamma(other.surface, t),
            surface_variant: self.surface_variant.lerp_to_gamma(other.surface_variant, t),
            on_surface: self.on_surface.lerp_to_gamma(other.on_surface, t),
            muted: self.muted.lerp_to_gamma(other.muted, t),
            primary: self.primary.lerp_to_gamma(other.primary, t),
            on_primary: self.on_primary.lerp_to_gamma(other.on_primary, t),
            danger: self.danger.lerp_to_gamma(other.danger, t),
            warning: self.warning.lerp_to_gamma(other.warning, t),
            success: self.success.lerp_to_gamma(other.success, t),
            focus: self.focus.lerp_to_gamma(other.focus, t),
            scrim: self.scrim.lerp_to_gamma(other.scrim, t),
            outline: self.outline.lerp_to_gamma(other.outline, t),
            control_edge: self.control_edge.lerp_to_gamma(other.control_edge, t),
            pressed: self.pressed.lerp_to_gamma(other.pressed, t),
            shadow: self.shadow.lerp_to_gamma(other.shadow, t),
            // Only a palette that names it carries its own; otherwise the shade keeps following
            // `surface`, which is interpolated above.
            shade_surface: (self.shade_surface.is_some() || other.shade_surface.is_some()).then(
                || {
                    self.get(ColorRole::ShadeSurface)
                        .lerp_to_gamma(other.get(ColorRole::ShadeSurface), t)
                },
            ),
        }
    }
}

/// **The five text sizes**. They go out as egui `TextStyle`s.
///
/// # Why they have to be tokens
///
/// While these five were baked into `egui_style()`, **no rung of the override ladder
/// could reach the text size.** Injecting a whole theme still had `egui_style()` write the
/// constants again. And on a device UI the text size is the physical question "is this readable
/// standing 70 cm away" — the archetypal value that has to differ per screen.
///
/// The defaults are exactly today's — a promotion is giving you a handle, not changing the
/// render.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TypeScale {
    /// Secondary text (13).
    pub small: f32,
    /// Body (16).
    pub body: f32,
    /// Button labels (17).
    pub button: f32,
    /// Headings (22).
    pub heading: f32,
    /// Monospace (16).
    pub monospace: f32,
}

impl Default for TypeScale {
    fn default() -> Self {
        Self {
            small: 13.0,
            body: 16.0,
            button: 17.0,
            heading: 22.0,
            monospace: 16.0,
        }
    }
}

/// The metric tokens (the shared UI contract).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// The status bar height.
    pub status_bar_height: f32,
    /// The nav bar height.
    pub nav_bar_height: f32,
    /// The minimum touch target (48).
    pub touch_target: f32,
    /// The side of a desktop cell (96).
    pub icon_cell: f32,
    /// The desktop icon size (48).
    pub icon_size: f32,
    /// The status bar icon size.
    pub status_icon_size: f32,
    /// The status bar value text size (the clock, `Text`, `User`).
    pub status_text_size: f32,
    /// The status bar item caption text size.
    pub status_label_size: f32,
    /// The settings row height (48): one touch target.
    pub row_height: f32,
    /// Where a row's, a card's or a bar's content starts, measured in from its edge (16).
    ///
    /// **One token for every row-like thing.** It used to be five: `list_row.pad`, `button.pad`,
    /// `toast.pad_x`, `heads_up.pad` and, in the shell's hand-drawn rows, `screen_inset`. Two of
    /// those were 16 and one was 12, so a switch row's label and a nav row's label in the same card
    /// sat four du apart — measured at five pixels on a real render, and visible as sloppiness
    /// without anyone being able to name it.
    pub content_inset: f32,
    /// The corner radius of **a container** — a card, a window, a menu (12).
    pub corner_radius: f32,
    /// The corner radius of **a control** — a button, a field, a combo (6).
    ///
    /// # Why it is not `corner_radius`
    ///
    /// While the two were one value, a button laid inside a card wore the card's own corner, so
    /// the card and the thing inside it read as two cards; a settings page came out as boxes of
    /// boxes with no hierarchy left to read. Half the container's radius is enough for the eye to
    /// sort container from content, and it is still nowhere near egui's near-square default.
    ///
    /// A device that wants the old flat look sets it equal to `corner_radius`.
    pub control_radius: f32,
    /// A card's inner padding — the gap between its edge and the row inside it, and the `gap` of
    /// the concentric-corner rule (`inner = outer − gap`).
    pub card_pad: f32,
    /// The edge zone width (24).
    pub edge_px: f32,
    /// The dock height.
    pub dock_height: f32,
    /// The padding inside the rect a screen receives (12). The shell shrinks the content rect by
    /// this much before handing it over — the layout (`fairing_widgets::shell::Layout`) does not shrink.
    /// Per screen it is overridden by `fairing_widgets::screen::ChromePolicy::inset`.
    pub screen_inset: f32,
    /// The desktop label text size (13).
    pub desktop_label_size: f32,
    /// The desktop label line-height multiplier (1.3). An approximation, for space calculations.
    pub desktop_label_line: f32,
    /// The gap between a desktop icon and its label (6).
    pub desktop_label_gap: f32,
    /// The desktop badge text size (10).
    pub desktop_badge_size: f32,
    /// The side of the desktop padlock badge (16).
    pub desktop_lock_size: f32,
    /// The desktop icon stroke (in 24-grid units, 2.0). The same unit as
    /// [`crate::icons::IconStyle::stroke`], so it is 4 px on a 48 px icon.
    pub desktop_icon_stroke: f32,
    /// The page indicator strip height (24). It only takes space with two or more pages.
    pub page_indicator_height: f32,
    /// The spacing between page indicator dots (16).
    pub page_indicator_step: f32,
    /// The radius of an unselected page dot (3).
    pub page_indicator_dot_off: f32,
    /// The radius of the selected page dot (4).
    pub page_indicator_dot_on: f32,
    /// The indicator's hit strip height = `touch_target × this ratio` (0.5).
    ///
    /// **Why 1.0 is not the default.** The dots sit right under the grid, so a full touch target
    /// steals taps from the last icon row. A device with room in its grid can raise it to 1.0 —
    /// which is why it is a token and not a constant.
    pub page_indicator_hit_ratio: f32,
    /// **The band between two split panes** (A8) — what is drawn. The
    /// handle a finger takes is a whole `touch_target` across it, so a thin band costs no reach.
    pub split_divider: f32,
    /// The upper bound on how much of a cell the desktop icon takes (0.5).
    pub desktop_icon_ratio: f32,
    /// How far the press tint sits inside the cell (4).
    pub desktop_press_inset: f32,
    /// How far the label width pulls back from the cell (8). Also its floor.
    pub desktop_label_pad: f32,
    /// How much larger the padlock badge is than the padlock (4).
    pub desktop_lock_ring_pad: f32,
    /// The radius of the numberless notification dot (5).
    pub desktop_badge_dot_r: f32,
    /// The minimum gap between the icon+label block and the top and bottom of the cell (2).
    pub desktop_content_min_pad: f32,
    /// The nav bar icon's side (24).
    pub nav_icon_size: f32,
    /// The maximum width of one nav item (a multiple of the touch target, 3.0).
    pub nav_item_max_span: f32,
    /// The home indicator's length — the pill a gesture-style nav bar draws (108).
    pub nav_indicator_length: f32,
    /// The home indicator's thickness (5).
    pub nav_indicator_thickness: f32,
    /// The status bar's side padding (12).
    pub status_edge_pad: f32,
    /// **The background alpha of a `BarMode::Overlay` status bar (1.0 — opaque).**
    ///
    /// It shipped at 0.72 and was never gated, and measured it does not hold: a bar at 0.72 over
    /// arbitrary content puts `muted` caption text at **2.41** against the 4.5 a label needs, in
    /// `base dark` over a white photograph. Every other preset fails too, in one direction or the
    /// other.
    ///
    /// A scrim under the bar does not rescue it. [`ColorRole::Scrim`] is black in all four presets,
    /// which is the right way for a dark palette and exactly the wrong way for a light one — with a
    /// scrim under it, `base light`'s caption goes 5.06 → **2.57**. The general reason is that
    /// `muted` and `danger` are *mid*-luminance by construction, and they clear the floor against an
    /// opaque surface with 12 % and 6 % of headroom respectively; anything that moves the ground
    /// toward the middle spends that headroom immediately, whichever way it moves.
    ///
    /// The token stays because an integrator who knows what is behind their bar may lower it (rung
    /// 2 of the ladder), and `a_translucent_bar_is_gated_at_its_default` pins the shipped value.
    pub status_overlay_alpha: f32,
    /// The side of a quick-settings tile (72) (M2).
    pub tile_size: f32,
    /// The width of the shade panel's bottom handle (40) (M2, A1).
    pub shade_handle_width: f32,
    /// The notification row height (72) (M2).
    pub notification_row_height: f32,
    /// The maximum toast width (420) (M2, A6).
    pub toast_width: f32,
    /// The heads-up banner height (88) (M2, A6).
    pub heads_up_height: f32,
    /// The gap between OSK keys (6) (M2).
    pub osk_key_gap: f32,
    /// The tallest a row of OSK keys gets, gap included (72). It caps `[osk] height_ratio` on a
    /// tall panel, where a fraction of the screen made each row as big as a palm.
    /// `f32::INFINITY` lifts the cap — what [`MetricsSpec::osk_max_key`] set to `None` resolves to.
    pub osk_max_key: f32,
    /// The default widget height — `BigButton` and `ListRow` (48): one touch target.
    pub widget_height: f32,
    /// The slider thumb diameter (28) (M2, A7).
    pub slider_thumb: f32,
    /// The five text sizes ([`TypeScale`]).
    pub type_scale: TypeScale,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            status_bar_height: 32.0,
            nav_bar_height: 56.0,
            touch_target: 48.0,
            icon_cell: 96.0,
            icon_size: 48.0,
            status_icon_size: 18.0,
            status_text_size: 15.0,
            status_label_size: 12.0,
            row_height: 48.0,
            content_inset: 16.0,
            corner_radius: 12.0,
            control_radius: 6.0,
            card_pad: 5.6,
            edge_px: 24.0,
            dock_height: 96.0,
            screen_inset: 12.0,
            desktop_label_size: 13.0,
            desktop_label_line: 1.3,
            desktop_label_gap: 6.0,
            desktop_badge_size: 10.0,
            desktop_lock_size: 16.0,
            desktop_icon_stroke: 2.0,
            page_indicator_height: 24.0,
            page_indicator_step: 16.0,
            page_indicator_dot_off: 3.0,
            page_indicator_dot_on: 4.0,
            page_indicator_hit_ratio: 0.5,
            split_divider: 8.0,
            desktop_icon_ratio: 0.5,
            desktop_press_inset: 4.0,
            desktop_label_pad: 8.0,
            desktop_lock_ring_pad: 4.0,
            desktop_badge_dot_r: 5.0,
            desktop_content_min_pad: 2.0,
            nav_icon_size: 24.0,
            nav_item_max_span: 3.0,
            nav_indicator_length: 108.0,
            nav_indicator_thickness: 5.0,
            status_edge_pad: 12.0,
            status_overlay_alpha: 1.0,
            tile_size: 72.0,
            shade_handle_width: 40.0,
            notification_row_height: 72.0,
            toast_width: 420.0,
            heads_up_height: 88.0,
            osk_key_gap: 6.0,
            osk_max_key: 72.0,
            widget_height: 48.0,
            slider_thumb: 28.0,
            type_scale: TypeScale::default(),
        }
    }
}

/// The motion tokens — `[motion]` turned into motion-system types.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionTokens {
    /// `motion.reduce`.
    pub reduce: bool,
    /// The default spring.
    pub spring: Spring,
    /// The release distance threshold.
    pub snap_ratio: f32,
    /// The release velocity threshold (px/s).
    pub fling_px_s: f32,
    /// slop (px).
    pub slop_px: f32,
    /// The tap window.
    pub tap: Duration,
    /// Long press.
    pub long_press: Duration,
    /// The push tween (A3).
    pub push: Tween,
    /// The pop tween (A3).
    pub pop: Tween,
    /// The push parallax ratio.
    pub parallax: f32,
    /// The push dim's maximum alpha.
    pub dim: f32,
    /// Home → task (A2).
    pub home_open: Tween,
    /// Task → home (A2).
    pub home_close: Tween,
    /// The desktop's shrink scale (A2).
    pub desktop_scale: f32,
    /// The press tween (A7).
    pub press: Tween,
    /// The release tween (A7).
    pub press_release: Tween,
    /// The press scale (A7).
    pub press_scale: f32,
    /// The shade (A1): spring, snap ratio, rubber band (M2).
    pub shade: ShadeTokens,
    /// The desktop page swipe (A4) (M2).
    pub page: PageTokens,
    /// The OSK show tween (A5) (M2).
    pub osk_show: Tween,
    /// The OSK hide tween (A5).
    pub osk_hide: Tween,
    /// The OSK hide debounce (A5, 100 ms).
    pub osk_hide_debounce: Duration,
    /// **One sweep of an indeterminate progress bar**, and half the period of the pulse that
    /// replaces it under [`Self::reduce`].
    ///
    /// A [`Duration`] and not a [`Tween`], so `reduce` cannot zero it — the reduced path *needs* a
    /// clock. At 1.6 s the leading edge crosses a 245 du track at about 4 °/s at arm's length,
    /// well inside smooth pursuit and nowhere near the rate at which the eye starts to saccade.
    pub progress_cycle: Duration,
    /// A toast entering (A6).
    pub toast_in: Tween,
    /// A toast leaving (A6).
    pub toast_out: Tween,
    /// The tween that lifts an existing toast 8 px (A6).
    pub toast_shift: Tween,
    /// A heads-up entering (A6, `CubicOut`).
    pub heads_up_in: Tween,
    /// A heads-up leaving (A6, `CubicIn`).
    pub heads_up_out: Tween,
    /// How long a heads-up holds (A6, 4 s).
    pub heads_up_hold: Duration,
    /// The switch knob and track (A7).
    pub switch: Tween,
    /// The parametric icon and theme crossfade (A7: 120 ms; the theme's 200 ms is `theme_fade`).
    pub crossfade: Tween,
    /// The palette interpolation on a theme switch (A7, 200 ms).
    pub theme_fade: Tween,
    /// The clear-top crossfade when a Single screen is reused (160 ms `CubicOut`, M2).
    pub clear_top: Tween,
    /// **The split shade's crossing between its two panels** (`split_crossfade_ms`).
    pub split_crossfade: Tween,
    /// Two panes coming in (A8, `[motion.panes] enter_ms`).
    pub panes_enter: Tween,
    /// A pane going (A8, `leave_ms`).
    pub panes_leave: Tween,
    /// The divider evening out on a double tap (A8, `even_ms`).
    pub panes_even: Tween,
    /// The screen on show shrinking into its card, and back (A10, `[motion.overview] in_ms`).
    pub overview_in: Tween,
    /// The other cards fading in, within [`Self::overview_in`] (A10, `cards_in_ms`).
    pub overview_cards_in: Duration,
    /// A card thrown away leaving (A10, `throw_ms`).
    pub overview_throw: Tween,
}

/// The shade tokens (`[motion.shade]`, A1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadeTokens {
    /// The release spring.
    pub spring: Spring,
    /// The distance threshold that commits to open (a fraction of the height).
    pub snap_ratio: f32,
    /// The rubber band past the full height.
    pub rubber: RubberBand,
    /// **The card's way out** (`card_close_ms`) — a tween rather than the spring, because
    /// a card leaves rather than being drawn back up.
    pub card_close: Tween,
}

/// The page swipe tokens (`[motion.page]`, A4).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageTokens {
    /// The release spring.
    pub spring: Spring,
    /// The fling velocity threshold (px/s).
    pub fling_px_s: f32,
    /// The rubber band past the ends (bounded per page).
    pub rubber: RubberBand,
}

impl MotionTokens {
    /// Built from `[motion]`. With `reduce`, every tween is 0 ms.
    #[must_use]
    pub fn from_config(cfg: &MotionConfig) -> Self {
        let ms = |v: u64| {
            if cfg.reduce {
                Tween::instant()
            } else {
                Tween::cubic_out(Duration::from_millis(v))
            }
        };
        Self {
            reduce: cfg.reduce,
            spring: Spring::new(cfg.spring.k, cfg.spring.c),
            snap_ratio: cfg.snap_ratio,
            fling_px_s: cfg.fling_px_s,
            slop_px: cfg.slop_px,
            tap: Duration::from_millis(cfg.tap_ms),
            long_press: Duration::from_millis(cfg.long_press_ms),
            push: ms(cfg.push.ms),
            pop: ms(cfg.pop.ms),
            parallax: cfg.push.parallax,
            dim: cfg.push.dim,
            home_open: ms(cfg.home.open_ms),
            home_close: ms(cfg.home.close_ms),
            desktop_scale: cfg.home.desktop_scale,
            press: ms(cfg.press.ms),
            press_release: ms(cfg.press.release_ms),
            press_scale: cfg.press.scale,
            shade: ShadeTokens {
                spring: Spring::new(cfg.shade.spring.k, cfg.shade.spring.c),
                snap_ratio: cfg.shade.snap_ratio,
                rubber: RubberBand::new(cfg.shade.rubber, cfg.shade.rubber_max_px),
                card_close: ms(cfg.shade.card_close_ms),
            },
            page: PageTokens {
                spring: Spring::new(cfg.page.spring.k, cfg.page.spring.c),
                fling_px_s: cfg.page.fling_px_s,
                rubber: RubberBand::new(cfg.page.rubber, cfg.page.rubber_max),
            },
            osk_show: ms(cfg.osk.show_ms),
            osk_hide: ms(cfg.osk.hide_ms),
            osk_hide_debounce: Duration::from_millis(cfg.osk.hide_debounce_ms),
            progress_cycle: Duration::from_millis(1600),
            toast_in: ms(cfg.toast.in_ms),
            toast_out: ms(cfg.toast.out_ms),
            toast_shift: ms(cfg.toast.shift_ms),
            heads_up_in: ms(cfg.toast.heads_up_in_ms),
            heads_up_out: if cfg.reduce {
                Tween::instant()
            } else {
                Tween {
                    duration: Duration::from_millis(cfg.toast.heads_up_out_ms),
                    easing: Easing::CubicIn,
                }
            },
            heads_up_hold: Duration::from_millis(cfg.toast.heads_up_hold_ms),
            switch: ms(cfg.switch.ms),
            crossfade: ms(cfg.crossfade_ms),
            theme_fade: ms(cfg.theme_fade_ms),
            clear_top: ms(cfg.clear_top_ms),
            split_crossfade: ms(cfg.split_crossfade_ms),
            panes_enter: ms(cfg.panes.enter_ms),
            panes_leave: ms(cfg.panes.leave_ms),
            panes_even: ms(cfg.panes.even_ms),
            overview_in: ms(cfg.overview.in_ms),
            overview_cards_in: Duration::from_millis(cfg.overview.cards_in_ms),
            overview_throw: ms(cfg.overview.throw_ms),
        }
    }
}

impl Default for MotionTokens {
    fn default() -> Self {
        Self::from_config(&MotionConfig::default())
    }
}

/// A theme = the palette + the metrics + the motion tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// The colours.
    pub palette: Palette,
    /// The metrics.
    pub metrics: Metrics,
    /// The component metrics. The twenty values widgets and notifications draw
    /// themselves with — named apart from [`Metrics`], which moves the layout.
    pub components: ComponentMetrics,
    /// The control tokens — what every control shares, so that a switch, a
    /// slider and a button read as one family rather than three drawings. See [`ControlMetrics`].
    pub control: ControlMetrics,
    /// How high a container sits — see [`Theme::elevate`].
    pub elevation: ElevationMetrics,
    /// Motion.
    pub motion: MotionTokens,
    /// Whether this is dark mode (egui's `Visuals::dark_mode`).
    pub dark: bool,
}

/// `[theme] preset` → [`Preset`]. An unknown name is an [`Error::Config`].
fn preset_from_config(cfg: &ThemeConfig) -> Result<Preset> {
    Preset::parse(&cfg.preset).ok_or_else(|| {
        // Read from `Preset::ALL`, so a new preset cannot be left out of the message.
        let known: Vec<&str> = Preset::ALL.iter().map(|preset| preset.as_str()).collect();
        Error::Config(format!(
            "[theme] preset = \"{}\" is not a known preset ({})",
            cfg.preset,
            known.join(" | ")
        ))
    })
}

/// The config a [`Theme`] is built from ([`Theme::from_inputs`]).
///
/// Five values out of the shell's TOML, named rather than passed positionally so that adding one
/// later does not silently reorder a call.
#[derive(Debug, Clone, Copy)]
pub struct ThemeInputs<'a> {
    /// `[shell] theme` — `"dark"` or `"light"`. Anything else warns and takes dark.
    pub theme_name: &'a str,
    /// `[theme]` — the preset name and the palette role overrides.
    pub theme: &'a ThemeConfig,
    /// `[motion]`.
    pub motion: &'a MotionConfig,
    /// `[status_bar] height`.
    pub status_bar_height: f32,
    /// `[status_bar] icon_size`.
    pub status_icon_size: f32,
    /// `[nav_bar] height`.
    pub nav_bar_height: f32,
}

impl Theme {
    /// A **bold** [`egui::FontId`] at `size`.
    ///
    /// # Why weight is used at all
    ///
    /// The rule used to be that hierarchy must never rest on weight, because a manufacturer can
    /// swap in a typeface with no weight axis and the hierarchy would collapse with it. The rule
    /// is right about the risk and wrong about the conclusion: iOS, One UI and Windows 11 all lean
    /// on weight for the top of the hierarchy, and a shell that never uses it reads flat however
    /// well its sizes and colours are chosen — which is what egui's own single-weight font
    /// (`Ubuntu-Light`) leaves you with.
    ///
    /// The way out is to use weight **and** keep the size and colour separation that works without
    /// it. [`crate::fonts::FontFamilies::Strong`] is always bound: with no bold registered it holds
    /// the regular faces, so this returns a font that renders identically and nothing collapses.
    #[must_use]
    pub fn strong(&self, size: f32) -> egui::FontId {
        egui::FontId::new(size, crate::fonts::strong_family())
    }

    /// A [`FontId`](egui::FontId) in the **display** face at `size`.
    ///
    /// The display face is the half of a brand a palette cannot carry — the two kiosk references
    /// this crate was measured against are recognised by their serif headline before a single
    /// colour is read — and until this existed the crate could not express it: `Proportional`,
    /// `Monospace` and `Strong` are all the coverage faces, and swapping `Proportional` for a
    /// headline face would have changed every label on every screen with it.
    ///
    /// [`crate::fonts::FontFamilies::Display`] is always bound, and seeded from the *strong* faces
    /// rather than the regular ones, so this returns [`Self::strong`]'s font exactly until an
    /// integrator registers a display face. A screen title is the only thing in the crate that
    /// asks for it: a face chosen for a headline may carry no Hangul, no tabular figures and no
    /// `℃`, and a row that must show a value cannot afford any of those to be missing.
    #[must_use]
    pub fn display(&self, size: f32) -> egui::FontId {
        egui::FontId::new(size, crate::fonts::display_family())
    }

    /// The dark theme.
    #[must_use]
    pub fn dark() -> Self {
        Self {
            palette: Palette::dark(),
            metrics: Metrics::default(),
            components: ComponentMetrics::default(),
            control: ControlMetrics::default(),
            elevation: ElevationMetrics::default(),
            motion: MotionTokens::default(),
            dark: true,
        }
    }

    /// The light theme.
    #[must_use]
    pub fn light() -> Self {
        Self {
            palette: Palette::light(),
            metrics: Metrics::default(),
            components: ComponentMetrics::default(),
            control: ControlMetrics::default(),
            elevation: ElevationMetrics::default(),
            motion: MotionTokens::default(),
            dark: false,
        }
    }

    /// Built from the config sections that describe a theme ([`ThemeInputs`]).
    ///
    /// The shell's own `ShellConfig` gathers these — `fairing_widgets::ShellConfig::build_theme` is the
    /// one-argument form. This takes the five pieces directly, because a theme has no business
    /// knowing what else is in the shell's config file.
    ///
    /// # Errors
    /// [`Error::Config`] if `[theme] preset` is an unknown name, or a `[theme.palette]` role
    /// name or colour format is wrong ([`Palette::apply_overrides`]).
    pub fn from_inputs(inputs: &ThemeInputs<'_>) -> Result<Self> {
        if inputs.theme_name != "light" && inputs.theme_name != "dark" {
            log::warn!(
                "[shell] theme = \"{}\" is neither dark nor light - using dark",
                inputs.theme_name
            );
        }
        let dark = inputs.theme_name != "light";
        // Passing a typo quietly to the default preset makes "why is the brand not coming on" impossible to find.
        let preset = preset_from_config(inputs.theme)?;
        let mut theme = Self {
            palette: Palette::preset(preset, dark),
            metrics: Metrics::default(),
            components: ComponentMetrics::default(),
            control: ControlMetrics::default(),
            elevation: ElevationMetrics::default(),
            motion: MotionTokens::default(),
            dark,
        };
        theme.metrics.status_bar_height = inputs.status_bar_height;
        theme.metrics.nav_bar_height = inputs.nav_bar_height;
        theme.metrics.status_icon_size = inputs.status_icon_size;
        theme.motion = MotionTokens::from_config(inputs.motion);
        theme.palette.apply_overrides(&inputs.theme.palette)?;
        Ok(theme)
    }

    /// A role colour.
    #[must_use]
    pub fn color(&self, role: ColorRole) -> Color32 {
        self.palette.get(role)
    }

    /// An egui `Style` carrying this theme (with the touch-first adjustments).
    ///
    /// # Why it paints this far
    ///
    /// If only the crate's widgets ([`widgets`](crate::widgets)) followed the theme and **egui's own
    /// widgets** the integrator uses (`TextEdit`, `Button`, `ComboBox`, `Checkbox`) kept egui's
    /// defaults, two designs would mix inside one screen. That is what happened: card corners
    /// were 26 du while the text field right beside them was egui's default 2 du, so they did
    /// not read as one screen, and changing the palette left the field's hint text behind.
    /// **A theme is only a theme if changing it changes the small things too.**
    ///
    /// So the role colours and the metric tokens go into egui's `Visuals` and `Spacing` **in
    /// full**. An integrator writing one line of `ui.text_edit_singleline` gets corners, borders,
    /// hint colour and the focus ring from the theme.
    #[must_use]
    pub fn egui_style(&self) -> egui::Style {
        let mut style = if self.dark {
            egui::Style {
                visuals: egui::Visuals::dark(),
                ..Default::default()
            }
        } else {
            egui::Style {
                visuals: egui::Visuals::light(),
                ..Default::default()
            }
        };
        // **egui's own debug overlay is off.**
        //
        // `DebugOptions::show_unaligned` defaults to on in a debug build, and it paints an orange
        // rule and the word "Unaligned" across any widget it thinks sits off the pixel grid. On a
        // device panel that is not a developer hint, it is a defect on the glass: the crate sets
        // the whole `Style` (see above), so an integrator has no obvious place to turn it off, and
        // the first anyone hears of it is a photograph of a shipped screen with a warning drawn
        // through the middle of a card. A crate that owns the style owns this too. (The field
        // exists only in a debug build — egui gates it on `debug_assertions`.)
        #[cfg(debug_assertions)]
        {
            style.debug = egui::style::DebugOptions::default();
            style.debug.debug_on_hover = false;
            style.debug.show_unaligned = false;
        }
        // **Labels are not selectable.** egui's default lets a drag across a label select its
        // text, and on a panel a drag across a label is a scroll: the demo's tour dragged a page
        // and got a highlighted sentence instead of a moved one. There is no clipboard to paste
        // into on a machine, so nothing is lost.
        style.interaction.selectable_labels = false;
        let p = &self.palette;
        let m = &self.metrics;
        let v = &mut style.visuals;
        v.dark_mode = self.dark;
        v.override_text_color = Some(p.on_surface);
        // Hint and disabled text are **the `Muted` role**. Without it egui fades the body colour by gamma,
        // and then changing `muted` in the palette leaves the hint text alone.
        v.weak_text_color = Some(p.muted);
        v.panel_fill = p.surface;
        v.window_fill = p.surface;
        v.window_stroke = Stroke::new(HAIRLINE, p.outline);
        v.extreme_bg_color = p.background;
        // A text field is **a recessed place** — laid on a card (`SurfaceVariant`) or on a page
        // (`Surface`), it has to be a step darker to read as "write here".
        v.text_edit_bg_color = Some(p.background);
        v.faint_bg_color = p.surface_variant;
        v.window_shadow = egui::epaint::Shadow::NONE;
        v.popup_shadow = egui::epaint::Shadow::NONE;
        v.hyperlink_color = p.primary;
        v.warn_fg_color = p.warning;
        v.error_fg_color = p.danger;

        // **The corners are matched — to the right one of the two.** egui's default is 2 du, so left
        // alone the buttons and fields inside a card come out square and stand apart. Given the card's
        // own radius instead, they read as more cards. Containers take `corner_radius`, controls take
        // `control_radius`.
        let corner = egui::CornerRadius::same(crate::unit::round_u8(m.corner_radius));
        let control = egui::CornerRadius::same(crate::unit::round_u8(m.control_radius));
        v.window_corner_radius = corner;
        v.menu_corner_radius = corner;

        v.widgets.noninteractive.bg_fill = p.surface_variant;
        v.widgets.noninteractive.weak_bg_fill = p.surface_variant;
        v.widgets.noninteractive.bg_stroke = Stroke::new(HAIRLINE, p.outline);
        v.widgets.noninteractive.fg_stroke = Stroke::new(HAIRLINE, p.on_surface);
        v.widgets.inactive.bg_fill = p.surface_variant;
        v.widgets.inactive.weak_bg_fill = p.surface_variant;
        v.widgets.inactive.bg_stroke = Stroke::new(HAIRLINE, p.outline);
        v.widgets.inactive.fg_stroke = Stroke::new(HAIRLINE, p.on_surface);
        // Touch has no hover: hovered is left the same as inactive and only active (pressed) is accented.
        v.widgets.hovered = v.widgets.inactive;
        v.widgets.active.bg_fill = p.pressed.blend(p.surface_variant);
        v.widgets.active.weak_bg_fill = p.pressed.blend(p.surface_variant);
        v.widgets.active.bg_stroke = Stroke::new(ACCENT_STROKE, p.primary);
        v.widgets.active.fg_stroke = Stroke::new(HAIRLINE, p.on_surface);
        // An open combo or menu wears the same face as a press — three states would confuse the finger.
        v.widgets.open = v.widgets.active;
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = control;
        }

        v.selection.bg_fill = p.primary.gamma_multiply(SELECTION_ALPHA);
        // The focus ring — this border stands when a `TextEdit` takes focus. That is why there is a
        // separate `Focus` role: a press (`Primary`) and a focus are different events.
        v.selection.stroke = Stroke::new(ACCENT_STROKE, p.focus);

        style.spacing.interact_size = egui::vec2(m.touch_target, m.touch_target);
        // The padding comes from the tokens too — grow `screen_inset` and the inside of a button widens with it.
        style.spacing.button_padding =
            egui::vec2(m.screen_inset * BUTTON_PAD_X, m.screen_inset * BUTTON_PAD_Y);
        style.spacing.item_spacing = egui::Vec2::splat(m.screen_inset * ITEM_GAP);
        style.spacing.menu_margin = egui::Margin::same(crate::unit::round_i8(m.screen_inset));
        style.spacing.text_edit_width = m.touch_target * TEXT_EDIT_SPAN;
        // The scrollbars are thin and hide themselves (floating goes transparent unless scrolling or
        // hovered). **This is the one place that is not a token** — a hairline's thickness is a matter for
        // the eye rather than the finger, and thickening with the touch target it would cover the content.
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.spacing.scroll.bar_width = 4.0;
        style.animation_time = if self.motion.reduce { 0.0 } else { 0.08 };
        // Touch has no hover: the pointing-hand hover cursor means nothing, so it is turned off.
        v.interact_cursor = None;
        // Four or five text steps: egui's default sizes (9–18) are reckoned for the desktop and
        // are small for touch. They are redefined as five steps, each a step larger, with the body as the reference.
        let t = self.metrics.type_scale;
        style.text_styles = [
            (egui::TextStyle::Small, egui::FontId::proportional(t.small)),
            (egui::TextStyle::Body, egui::FontId::proportional(t.body)),
            (
                egui::TextStyle::Button,
                egui::FontId::proportional(t.button),
            ),
            (
                egui::TextStyle::Heading,
                egui::FontId::proportional(t.heading),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::monospace(t.monospace),
            ),
        ]
        .into();
        style
    }

    /// Apply it to an egui `Context`. `Shell::new` calls it once, and a theme switch calls it again.
    pub fn apply(&self, ctx: &egui::Context) {
        let theme = if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.set_theme(theme);
        ctx.set_style_of(theme, self.egui_style());
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// WCAG 2.x contrast, `(L1 + 0.05) / (L2 + 0.05)` with the lighter colour on top.
///
/// One copy for the whole crate: the palette gate and each control that pins its own colours
/// against a face all have to compute it the same way, or two tests disagree about whether the
/// same pair passes.
#[cfg(test)]
pub(crate) fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
    fn luminance(c: egui::Color32) -> f32 {
        fn channel(v: u8) -> f32 {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126f32.mul_add(
            channel(c.r()),
            0.7152f32.mul_add(channel(c.g()), 0.0722 * channel(c.b())),
        )
    }
    let (x, y) = (luminance(a), luminance(b));
    let (hi, lo) = if x > y { (x, y) } else { (y, x) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::{ColorRole, Metrics, MotionTokens, Palette, Preset, Theme, ThemeInputs};

    /// The shade's role follows `surface` until it is named, even when `surface` itself is
    /// overridden later; named, it keeps its own colour and survives an interpolation.
    #[test]
    fn the_shade_surface_follows_the_surface_until_it_is_named() -> crate::Result<()> {
        use egui::Color32;
        use std::collections::BTreeMap;
        let mut p = Palette::dark();
        assert_eq!(p.get(ColorRole::ShadeSurface), p.surface);
        let mut over = BTreeMap::new();
        over.insert("surface".to_owned(), "#202020".to_owned());
        p.apply_overrides(&over)?;
        assert_eq!(
            p.get(ColorRole::ShadeSurface),
            Color32::from_rgb(0x20, 0x20, 0x20)
        );
        assert_eq!(
            ColorRole::parse("shade_surface"),
            Some(ColorRole::ShadeSurface)
        );
        let shade = Color32::from_rgb(0x12, 0x34, 0x56);
        p.set(ColorRole::ShadeSurface, shade);
        assert_eq!(p.get(ColorRole::ShadeSurface), shade);
        assert_eq!(p.surface, Color32::from_rgb(0x20, 0x20, 0x20));
        let mut q = Palette::light();
        q.set(ColorRole::ShadeSurface, shade);
        assert_eq!(p.lerp(&q, 0.5).get(ColorRole::ShadeSurface), shade);
        let plain = Palette::dark().lerp(&Palette::light(), 0.5);
        assert_eq!(plain.shade_surface, None, "an unnamed role stays unnamed");
        Ok(())
    }
    use crate::config::MotionConfig;

    use super::contrast;

    /// **The shipped overlay status bar reads over anything that can be behind it.**
    ///
    /// The bar's own fill is composited over the screen below it, so unlike every other pair in
    /// `every_preset_meets_the_contrast_floor` its ground is not a palette colour — it is whatever
    /// the screen drew. The two extremes bound it. This is the test that did not exist while
    /// `status_overlay_alpha` shipped at 0.72, which is how a 2.41 got out.
    #[test]
    fn a_translucent_bar_is_gated_at_its_default() {
        let alpha = Metrics::default().status_overlay_alpha;
        let over = |fg: egui::Color32, a: f32, bg: egui::Color32| {
            let m = |f: u8, b: u8| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    a.mul_add(f32::from(f), (1.0 - a) * f32::from(b)).round() as u8
                }
            };
            egui::Color32::from_rgb(m(fg.r(), bg.r()), m(fg.g(), bg.g()), m(fg.b(), bg.b()))
        };
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                for (behind, content) in [
                    ("white", egui::Color32::WHITE),
                    ("black", egui::Color32::BLACK),
                ] {
                    let ground = over(p.surface, alpha, content);
                    for (label, fg, floor) in [
                        ("clock", p.on_surface, 4.5),
                        ("caption", p.muted, 4.5),
                        ("alert", p.danger, 3.0),
                    ] {
                        let got = contrast(fg, ground);
                        assert!(
                            got >= floor,
                            "{} {} overlay bar at alpha {alpha}: {label} over {behind} content is {got:.2}, under {floor}",
                            preset.as_str(),
                            if dark { "dark" } else { "light" }
                        );
                    }
                }
            }
        }
    }

    /// **Every preset** × dark/light × four pairs clears the floor. Adding a
    /// preset is caught automatically through [`Preset::ALL`] — which is why `ALL` exists.
    ///
    /// The alpha roles (`scrim`, `pressed`) are out of scope — they lie over a background, so
    /// their contrast on their own means nothing.
    #[test]
    fn every_preset_meets_the_contrast_floor() {
        for &preset in Preset::ALL {
            for dark in [true, false] {
                let p = Palette::preset(preset, dark);
                let name = preset.as_str();
                let mode = if dark { "dark" } else { "light" };
                // **Fourteen gates, not four.** The four this list used to hold let real defects
                // through, and the control vocabulary found them by computing the rest: a subtitle
                // on a CARD rather than on the page (G3), a danger label on a card (G6), a disabled
                // control (G7/G8), and every non-text pair a drawn control actually relies on
                // (G9–G14, WCAG 1.4.11's 3.0 floor). Three of the four palette values that had to
                // move were only visible once the pairs below existed.
                //
                // The floors differ per row on purpose: 7.0 for body text on its own ground, 4.5
                // for WCAG AA text, 3.0 for a shape whose meaning does not depend on reading it.
                let dim = p.on_surface.gamma_multiply(0.55);
                for (label, fg, bg, floor) in [
                    // Text.
                    ("on_surface/surface", p.on_surface, p.surface, 7.0),
                    ("muted/surface", p.muted, p.surface, 4.5),
                    ("muted/surface_variant", p.muted, p.surface_variant, 4.5),
                    ("on_primary/primary", p.on_primary, p.primary, 4.5),
                    ("danger/surface", p.danger, p.surface, 4.5),
                    ("danger/surface_variant", p.danger, p.surface_variant, 4.5),
                    // Disabled: deliberately under the active state, never under legibility.
                    ("disabled/surface", dim, p.surface, 3.0),
                    ("disabled/surface_variant", dim, p.surface_variant, 3.0),
                    // Drawn shapes — a filled control, a track, a mover's edge, a focus ring.
                    ("primary/surface", p.primary, p.surface, 3.0),
                    ("primary/surface_variant", p.primary, p.surface_variant, 3.0),
                    ("primary/background", p.primary, p.background, 3.0),
                    (
                        "on_surface/surface_variant",
                        p.on_surface,
                        p.surface_variant,
                        3.0,
                    ),
                    ("focus/surface", p.focus, p.surface, 3.0),
                    ("focus/surface_variant", p.focus, p.surface_variant, 3.0),
                    // A control's boundary carries WCAG 1.4.11's floor and nothing above it: it is
                    // what identifies an unticked checkbox or an off switch, and it is not text.
                    ("control_edge/surface", p.control_edge, p.surface, 3.0),
                    (
                        "control_edge/surface_variant",
                        p.control_edge,
                        p.surface_variant,
                        3.0,
                    ),
                ] {
                    let got = contrast(fg, bg);
                    assert!(got >= floor, "{name} {mode} {label}: {got:.2} < {floor:.1}");
                }
            }
        }
    }

    /// Names round-trip. If `parse` and `as_str` drift, a config quietly builds a different preset.
    #[test]
    fn preset_names_round_trip_and_unknown_is_none() {
        for &preset in Preset::ALL {
            assert_eq!(Preset::parse(preset.as_str()), Some(preset));
        }
        assert_eq!(Preset::parse("shallows"), None);
        assert_eq!(Preset::default(), Preset::Base);
    }

    /// An unknown preset name is **an error at startup** — falling back to the
    /// default quietly leaves no way to find out why the brand did not come on.
    #[test]
    fn unknown_preset_is_a_config_error() {
        let theme = crate::config::ThemeConfig {
            preset: "abbys".to_owned(),
            ..crate::config::ThemeConfig::default()
        };
        let motion = crate::config::MotionConfig::default();
        let got = Theme::from_inputs(&ThemeInputs {
            theme_name: "dark",
            theme: &theme,
            motion: &motion,
            status_bar_height: 32.0,
            status_icon_size: 18.0,
            nav_bar_height: 56.0,
        })
        .err();
        assert!(
            matches!(&got, Some(crate::Error::Config(m)) if m.contains("abbys") && m.contains("abyss")),
            "it has to show the typo as it is and name the ones that can be chosen: {got:?}"
        );
    }

    /// `Palette::dark` / `light` are aliases for `preset(Base, ..)`.
    #[test]
    fn base_aliases_match_the_preset_constructor() {
        assert_eq!(Palette::dark(), Palette::preset(Preset::Base, true));
        assert_eq!(Palette::light(), Palette::preset(Preset::Base, false));
    }

    #[test]
    fn palette_lerp_at_endpoints_matches_each_palette() {
        let dark = Palette::dark();
        let light = Palette::light();
        assert_eq!(dark.lerp(&light, 0.0), dark);
        assert_eq!(dark.lerp(&light, 1.0), light);
    }

    #[test]
    fn palette_lerp_interpolates_every_role() {
        let dark = Palette::dark();
        let light = Palette::light();
        let mid = dark.lerp(&light, 0.5);
        // All 14 roles have to lie between the two endpoints (linear interpolation, so each channel within
        // min..=max) — it catches the regression of handing one palette back whole (the t<0.5 branch of the
        // original skeleton).
        for role in [
            ColorRole::Background,
            ColorRole::Surface,
            ColorRole::SurfaceVariant,
            ColorRole::OnSurface,
            ColorRole::Muted,
            ColorRole::Primary,
            ColorRole::OnPrimary,
            ColorRole::Danger,
            ColorRole::Warning,
            ColorRole::Success,
            ColorRole::Focus,
            ColorRole::Scrim,
            ColorRole::Outline,
            ColorRole::Pressed,
            ColorRole::Shadow,
            ColorRole::ShadeSurface,
        ] {
            let a = dark.get(role);
            let b = light.get(role);
            let m = mid.get(role);
            if a == b {
                assert_eq!(m, a, "role {role:?} unchanged but lerp moved it");
                continue;
            }
            let between = |lo: u8, hi: u8, v: u8| {
                let (lo, hi) = (lo.min(hi), lo.max(hi));
                (lo..=hi).contains(&v)
            };
            assert!(
                between(a.r(), b.r(), m.r())
                    && between(a.g(), b.g(), m.g())
                    && between(a.b(), b.b(), m.b())
                    && between(a.a(), b.a(), m.a()),
                "role {role:?}: {a:?} .. {b:?} midpoint {m:?} out of range"
            );
        }
    }

    /// **Changing the theme carries egui's own widgets with it.**
    ///
    /// A crate widget and an `egui::TextEdit` standing on the same screen is normal, and if the
    /// two look different then it is not a theme. What this test pins is **the source, not the
    /// value** — corners must come from `metrics.corner_radius`, hint text from the `Muted`
    /// role, padding from `screen_inset`. It compares two themes to confirm that changing a
    /// token changes the style with it.
    #[test]
    fn egui_widgets_follow_the_tokens() {
        let mut theme = Theme::dark();
        let base = theme.egui_style();

        // The corners: a container takes `corner_radius`, an egui widget takes `control_radius`.
        let want = crate::unit::round_u8(theme.metrics.corner_radius);
        let want_control = crate::unit::round_u8(theme.metrics.control_radius);
        assert_ne!(
            want, want_control,
            "the two radii have to differ or the split is not being tested"
        );
        for w in [
            base.visuals.widgets.noninteractive,
            base.visuals.widgets.inactive,
            base.visuals.widgets.hovered,
            base.visuals.widgets.active,
            base.visuals.widgets.open,
        ] {
            assert_eq!(w.corner_radius, egui::CornerRadius::same(want_control));
        }
        assert_eq!(
            base.visuals.window_corner_radius,
            egui::CornerRadius::same(want)
        );
        assert_eq!(
            base.visuals.menu_corner_radius,
            egui::CornerRadius::same(want)
        );

        // Hint and disabled text are the `Muted` role (egui's default is the body colour faded by gamma).
        assert_eq!(base.visuals.weak_text_color, Some(theme.palette.muted));
        // A text field's ground comes from a role colour.
        assert_eq!(
            base.visuals.text_edit_bg_color,
            Some(theme.palette.background)
        );
        // The focus ring is the `Focus` role, not a press (`Primary`).
        assert_eq!(base.visuals.selection.stroke.color, theme.palette.focus);

        // **Change a token and the style follows.** That matters more than the values themselves.
        theme.metrics.corner_radius = 3.0;
        theme.metrics.control_radius = 2.0;
        theme.metrics.screen_inset = 24.0;
        theme.palette.muted = egui::Color32::from_rgb(1, 2, 3);
        let moved = theme.egui_style();
        assert_eq!(
            moved.visuals.widgets.inactive.corner_radius,
            egui::CornerRadius::same(2)
        );
        assert_eq!(
            moved.visuals.window_corner_radius,
            egui::CornerRadius::same(3)
        );
        assert_eq!(
            moved.visuals.weak_text_color,
            Some(egui::Color32::from_rgb(1, 2, 3))
        );
        assert!(moved.spacing.button_padding.x > base.spacing.button_padding.x);
        assert!(moved.spacing.item_spacing.x > base.spacing.item_spacing.x);
    }

    #[test]
    fn metrics_defaults_match_touch_targets() {
        // The shared convention: a touch target of 48, an icon cell of 96, a status bar of 32, a nav bar of 56.
        let m = Metrics::default();
        assert!((m.touch_target - 48.0).abs() < f32::EPSILON);
        assert!((m.icon_cell - 96.0).abs() < f32::EPSILON);
        assert!((m.status_bar_height - 32.0).abs() < f32::EPSILON);
        assert!((m.nav_bar_height - 56.0).abs() < f32::EPSILON);
    }

    /// The constants promoted to tokens **did not change value**. The
    /// numbers here are the `const` values from `desktop/mod.rs` and
    /// `chrome/{nav_bar,status_bar}.rs` before the promotion.
    #[test]
    fn promoted_metrics_keep_the_old_constant_values() {
        let m = Metrics::default();
        for (name, got, want) in [
            ("desktop_label_size", m.desktop_label_size, 13.0),
            ("desktop_label_line", m.desktop_label_line, 1.3),
            ("desktop_label_gap", m.desktop_label_gap, 6.0),
            ("desktop_badge_size", m.desktop_badge_size, 10.0),
            ("desktop_lock_size", m.desktop_lock_size, 16.0),
            ("desktop_icon_stroke", m.desktop_icon_stroke, 2.0),
            ("page_indicator_height", m.page_indicator_height, 24.0),
            ("page_indicator_step", m.page_indicator_step, 16.0),
            ("nav_icon_size", m.nav_icon_size, 24.0),
            ("nav_item_max_span", m.nav_item_max_span, 3.0),
            ("status_edge_pad", m.status_edge_pad, 12.0),
            // **Deliberately not the old constant.** 0.72 was the shipped value and it was
            // wrong — see the field's own docs and `a_translucent_bar_is_gated_at_its_default`.
            // This row is the record that it moved on purpose rather than by a slip.
            ("status_overlay_alpha", m.status_overlay_alpha, 1.0),
            ("screen_inset", m.screen_inset, 12.0),
        ] {
            assert!((got - want).abs() < 1e-6, "{name} = {got} != {want}");
        }
    }

    #[test]
    fn egui_style_has_touch_interact_size_and_text_scale() {
        let style = Theme::dark().egui_style();
        // Touch: interact_size equals the minimum touch target (48).
        assert!((style.spacing.interact_size.x - 48.0).abs() < f32::EPSILON);
        assert!((style.spacing.interact_size.y - 48.0).abs() < f32::EPSILON);
        // Touch has no hover: no cursor change.
        assert_eq!(style.visuals.interact_cursor, None);
        // Four or five text steps, ascending (smallest first).
        let mut sizes: Vec<f32> = style.text_styles.values().map(|f| f.size).collect();
        sizes.sort_by(f32::total_cmp);
        assert!((4..=5).contains(&sizes.len()), "got {} steps", sizes.len());
        assert!(sizes
            .windows(2)
            .all(|w| w.first().zip(w.get(1)).is_some_and(|(a, b)| a <= b)));
        // The scrollbars: thin (4 px) and floating (self-hiding).
        assert!((style.spacing.scroll.bar_width - 4.0).abs() < f32::EPSILON);
        assert!(style.spacing.scroll.floating);
        assert!((style.spacing.scroll.dormant_handle_opacity - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn egui_style_reduce_zeroes_animation_time() {
        let mut theme = Theme::dark();
        theme.motion.reduce = true;
        assert!((theme.egui_style().animation_time - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn motion_tokens_from_config_matches_design_defaults() {
        // Whether the motion token table's defaults were carried over as they are (and agree with the table).
        let tokens = MotionTokens::from_config(&MotionConfig::default());
        assert!((tokens.spring.stiffness - 400.0).abs() < f32::EPSILON);
        assert!((tokens.spring.damping - 40.0).abs() < f32::EPSILON);
        assert!((tokens.snap_ratio - 0.33).abs() < f32::EPSILON);
        assert!((tokens.fling_px_s - 800.0).abs() < f32::EPSILON);
        assert!((tokens.slop_px - 12.0).abs() < f32::EPSILON);
        assert!((tokens.desktop_scale - 0.92).abs() < f32::EPSILON);
        assert!((tokens.press_scale - 0.97).abs() < f32::EPSILON);
        assert!(!tokens.reduce);
    }

    #[test]
    fn motion_tokens_reduce_zeroes_all_tweens() {
        let cfg = MotionConfig {
            reduce: true,
            ..MotionConfig::default()
        };
        let tokens = MotionTokens::from_config(&cfg);
        for tween in [
            tokens.push,
            tokens.pop,
            tokens.home_open,
            tokens.home_close,
            tokens.press,
            tokens.press_release,
            tokens.osk_show,
            tokens.osk_hide,
            tokens.toast_in,
            tokens.toast_out,
            tokens.toast_shift,
            tokens.heads_up_in,
            tokens.heads_up_out,
            tokens.switch,
            tokens.crossfade,
            tokens.theme_fade,
            tokens.clear_top,
        ] {
            assert!(tween.duration.is_zero(), "{tween:?} should be instant");
        }
    }
}
