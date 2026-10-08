//! The `[theme]` and `[motion]` config sections.
//!
//! These are the parts of the shell's TOML that describe **an element's look and timing** rather
//! than the shell's behaviour, so they sit with the types that read them: [`ThemeConfig`] feeds
//! [`Palette::apply_overrides`](crate::theme::Palette::apply_overrides) and
//! [`MotionConfig`] feeds [`MotionTokens::from_config`](crate::theme::MotionTokens::from_config).
//! The rest of the file they came from — the bars, the desktop, access, the OSK, gestures — stays
//! with the shell, which is the only thing that knows what those mean.
//!
//! `fairing` re-exports every type here from `fairing_widgets::config`, so a config written against the
//! old paths still resolves.

use serde::Deserialize;
use std::collections::BTreeMap;

/// `[theme]`: the preset and the
/// `[theme.palette]` overrides. The metric tokens are changed in code — a metrics spec
/// (`fairing::ShellBuilder::metrics_spec`) or a whole [`Theme`](crate::theme::Theme)
/// (`fairing::ShellBuilder::theme`).
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    /// `[theme] preset` — the palette preset name (`"base"` by default, `"abyss"` or `"linen"`). It is
    /// the [`Preset`](crate::theme::Preset) of that name, and an unknown name is an
    /// [`Error::Config`](crate::error::Error::Config) at startup (falling back to the default quietly leaves no way to find
    /// out why the brand did not come on).
    pub preset: String,
    /// `[theme.palette]` — role name (`"primary"`) → `"#RRGGBB"`. It overrides **both** of the
    /// preset's dark and light palettes. An unknown role name or format is an [`Error::Config`](crate::error::Error::Config)
    /// at startup.
    pub palette: BTreeMap<String, String>,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            preset: "base".to_owned(),
            palette: BTreeMap::new(),
        }
    }
}

/// The `[motion]` tokens (the M1 part of the motion token table).
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct MotionConfig {
    /// Every tween 0 ms, springs settle instantly.
    pub reduce: bool,
    /// The default spring.
    pub spring: SpringConfig,
    /// The release distance threshold (a ratio).
    pub snap_ratio: f32,
    /// The release velocity threshold (px/s).
    pub fling_px_s: f32,
    /// The drag detection slop (px).
    pub slop_px: f32,
    /// The tap window (ms).
    pub tap_ms: u64,
    /// The long-press duration (ms).
    pub long_press_ms: u64,
    /// `[motion.push]`.
    pub push: PushConfig,
    /// `[motion.pop]`.
    pub pop: DurationConfig,
    /// `[motion.home]`.
    pub home: HomeConfig,
    /// `[motion.press]`.
    pub press: PressConfig,
    /// `[motion.shade]` (A1).
    pub shade: ShadeConfig,
    /// `[motion.page]` (A4).
    pub page: PageMotionConfig,
    /// `[motion.osk]` (A5).
    pub osk: OskMotionConfig,
    /// `[motion.toast]` (A6).
    pub toast: ToastMotionConfig,
    /// `[motion.switch]` (A7).
    pub switch: DurationConfig,
    /// The parametric status icon crossfade (ms, A7; 120 by default).
    pub crossfade_ms: u64,
    /// The theme (palette) switch interpolation (ms, A7; 200 by default).
    pub theme_fade_ms: u64,
    /// The clear-top crossfade when a Single screen is reused (ms, 160 by default).
    pub clear_top_ms: u64,
    /// **Crossing between the split shade's two panels** (ms, 180 by
    /// default) — the header segment's dissolve from the notifications to the controls and back.
    pub split_crossfade_ms: u64,
    /// `[motion.panes]` (A8) — the workspace's two panes, not the split shade.
    pub panes: PanesMotionConfig,
    /// `[motion.overview]` (A10) — the recent screens.
    pub overview: OverviewMotionConfig,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            reduce: false,
            spring: SpringConfig::default(),
            snap_ratio: 0.33,
            fling_px_s: 800.0,
            slop_px: 12.0,
            tap_ms: 300,
            long_press_ms: 500,
            push: PushConfig::default(),
            pop: DurationConfig { ms: 200 },
            home: HomeConfig::default(),
            press: PressConfig::default(),
            shade: ShadeConfig::default(),
            page: PageMotionConfig::default(),
            osk: OskMotionConfig::default(),
            toast: ToastMotionConfig::default(),
            switch: DurationConfig { ms: 140 },
            crossfade_ms: 120,
            theme_fade_ms: 200,
            clear_top_ms: 160,
            split_crossfade_ms: 180,
            panes: PanesMotionConfig::default(),
            overview: OverviewMotionConfig::default(),
        }
    }
}

/// `[motion.panes]` (A8): two panes coming and going, and the divider evening out.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct PanesMotionConfig {
    /// The pane coming in slides in while the one there shrinks to its share.
    pub enter_ms: u64,
    /// The pane going slides out while the other grows to fill the content.
    pub leave_ms: u64,
    /// A double tap on the divider evens the panes out.
    pub even_ms: u64,
}

impl Default for PanesMotionConfig {
    fn default() -> Self {
        Self {
            enter_ms: 220,
            leave_ms: 220,
            even_ms: 200,
        }
    }
}

/// `[motion.overview]` (A10): the recent screens.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct OverviewMotionConfig {
    /// The screen on show shrinking into its card, and growing back out of it.
    pub in_ms: u64,
    /// The other cards fading in — within `in_ms`.
    pub cards_in_ms: u64,
    /// A card thrown away leaving.
    pub throw_ms: u64,
}

impl Default for OverviewMotionConfig {
    fn default() -> Self {
        Self {
            in_ms: 240,
            cards_in_ms: 160,
            throw_ms: 200,
        }
    }
}

/// `[motion.shade]` (A1): the spring, the snap ratio and the rubber band.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShadeConfig {
    /// The release spring.
    pub spring: SpringConfig,
    /// The distance threshold that commits to open (a fraction of the height).
    pub snap_ratio: f32,
    /// The rubber-band factor past the full height.
    pub rubber: f32,
    /// The rubber band's limit (px).
    pub rubber_max_px: f32,
    /// **How long a card takes to go** (ms) — `[overlay] reveal = "card"` only.
    ///
    /// A card does not spring back shut the way a curtain is drawn up: it fades and lifts, and it
    /// is gone well inside the time it took to arrive. One UI's panel is out in about 40 ms, which
    /// on a 60 Hz panel is two frames and reads as a cut; 90 ms keeps it a motion while staying a
    /// quarter of the open. Under `reduce` it is 0.
    pub card_close_ms: u64,
}

impl Default for ShadeConfig {
    fn default() -> Self {
        Self {
            spring: SpringConfig::default(),
            snap_ratio: 0.33,
            rubber: 0.25,
            rubber_max_px: 40.0,
            card_close_ms: 90,
        }
    }
}

/// `[motion.page]` (A4): the desktop page swipe.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct PageMotionConfig {
    /// The release spring.
    pub spring: SpringConfig,
    /// The fling velocity threshold (px/s).
    pub fling_px_s: f32,
    /// The rubber-band factor past the ends.
    pub rubber: f32,
    /// The rubber band's limit (in pages).
    pub rubber_max: f32,
}

impl Default for PageMotionConfig {
    fn default() -> Self {
        Self {
            spring: SpringConfig { k: 300.0, c: 35.0 },
            fling_px_s: 600.0,
            rubber: 0.3,
            rubber_max: 0.15,
        }
    }
}

/// `[motion.osk]` (A5).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct OskMotionConfig {
    /// The show tween.
    pub show_ms: u64,
    /// The hide tween.
    pub hide_ms: u64,
    /// Focus has to stay lost this long before it hides (so tapping between fields does not flicker).
    pub hide_debounce_ms: u64,
}

impl Default for OskMotionConfig {
    fn default() -> Self {
        Self {
            show_ms: 180,
            hide_ms: 160,
            hide_debounce_ms: 100,
        }
    }
}

/// `[motion.toast]` (A6). The heads-up timing is here too.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct ToastMotionConfig {
    /// A toast entering.
    pub in_ms: u64,
    /// A toast leaving.
    pub out_ms: u64,
    /// The tween that lifts the existing toasts when a new one arrives.
    pub shift_ms: u64,
    /// A heads-up entering.
    pub heads_up_in_ms: u64,
    /// A heads-up leaving.
    pub heads_up_out_ms: u64,
    /// How long a heads-up holds.
    pub heads_up_hold_ms: u64,
}

impl Default for ToastMotionConfig {
    fn default() -> Self {
        Self {
            in_ms: 160,
            out_ms: 200,
            shift_ms: 120,
            heads_up_in_ms: 220,
            heads_up_out_ms: 200,
            heads_up_hold_ms: 4000,
        }
    }
}

/// `spring = { k = 400, c = 40 }`.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct SpringConfig {
    /// The stiffness.
    pub k: f32,
    /// The damping. `2√k` is critical.
    pub c: f32,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self { k: 400.0, c: 40.0 }
    }
}

/// `[motion.push]` (A3).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct PushConfig {
    /// The tween's length.
    pub ms: u64,
    /// The outgoing layer's parallax ratio.
    pub parallax: f32,
    /// The outgoing layer's maximum dim alpha.
    pub dim: f32,
}

impl Default for PushConfig {
    fn default() -> Self {
        Self {
            ms: 220,
            parallax: 0.25,
            dim: 0.15,
        }
    }
}

/// A token that is only a duration (`[motion.pop]`).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct DurationConfig {
    /// The tween's length (ms).
    pub ms: u64,
}

impl Default for DurationConfig {
    fn default() -> Self {
        Self { ms: 200 }
    }
}

/// `[motion.home]` (A2).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct HomeConfig {
    /// The opening length.
    pub open_ms: u64,
    /// The closing length.
    pub close_ms: u64,
    /// The desktop's shrink scale.
    pub desktop_scale: f32,
}

impl Default for HomeConfig {
    fn default() -> Self {
        Self {
            open_ms: 240,
            close_ms: 200,
            desktop_scale: 0.92,
        }
    }
}

/// `[motion.press]` (A7).
#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default)]
pub struct PressConfig {
    /// The press tween.
    pub ms: u64,
    /// The release tween.
    pub release_ms: u64,
    /// The press scale.
    pub scale: f32,
}

impl Default for PressConfig {
    fn default() -> Self {
        Self {
            ms: 80,
            release_ms: 120,
            scale: 0.97,
        }
    }
}
