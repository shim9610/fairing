//! The chrome policy. Set on the declaration (`.chrome()` / `.fullscreen()`) and at runtime (`cx.set_chrome()`).

use crate::theme::ColorRole;

/// How a bar is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum BarMode {
    /// Shown (the content shrinks by that much).
    #[default]
    Show,
    /// Hidden (the edge zone stays if `allow_peek`).
    Hide,
    /// Translucent, over the content (the status bar only).
    Overlay,
}

/// The on-screen keyboard mode (M2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OskMode {
    /// Shown automatically when `wants_keyboard_input`.
    #[default]
    Auto,
    /// Only on a manual toggle.
    Manual,
    /// Never shown (there is a hardware keyboard).
    Off,
}

/// The chrome policy.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // The policy's fields as they are — each is an independent switch.
pub struct ChromePolicy {
    /// The status bar.
    pub status_bar: BarMode,
    /// The nav bar (there is no `Overlay` for it — it is treated as `Show`).
    pub nav_bar: BarMode,
    /// Let an edge pull reveal hidden chrome (true by default).
    pub allow_peek: bool,
    /// Block edge gestures (false by default). The emergency gesture always remains.
    pub edge_guard: bool,
    /// The on-screen keyboard.
    pub osk: OskMode,
    /// Stop the display idle timer.
    pub keep_awake: bool,
    /// Allow entering split while this screen is visible (true by default).
    pub allow_split: bool,
    /// Override the pane background colour.
    pub background: Option<ColorRole>,
    /// Override the padding inside the rect the screen receives (px). `None` means
    /// [`crate::theme::Metrics::screen_inset`] (12 by default). Only the rect the shell hands
    /// over shrinks; [`crate::shell::Layout`] and the pane background do not.
    pub inset: Option<f32>,
}

impl ChromePolicy {
    /// Both bars hidden, `allow_peek: true`, inset 0 (`.fullscreen()`).
    /// A fullscreen screen draws to the edge of the display.
    #[must_use]
    pub fn fullscreen() -> Self {
        Self {
            status_bar: BarMode::Hide,
            nav_bar: BarMode::Hide,
            inset: Some(0.0),
            ..Self::default()
        }
    }

    /// The inset this policy uses (px). Without `inset` it is the theme default. Negatives clamp to 0.
    #[must_use]
    pub fn inset_or(&self, default: f32) -> f32 {
        self.inset.unwrap_or(default).max(0.0)
    }
}

impl Default for ChromePolicy {
    fn default() -> Self {
        Self {
            status_bar: BarMode::Show,
            nav_bar: BarMode::Show,
            allow_peek: true,
            edge_guard: false,
            osk: OskMode::Auto,
            keep_awake: false,
            allow_split: true,
            background: None,
            inset: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BarMode, ChromePolicy, OskMode};

    /// The default policy: both bars shown, peek allowed, edge gestures pass through.
    #[test]
    fn default_shows_both_bars() {
        let p = ChromePolicy::default();
        assert_eq!(p.status_bar, BarMode::Show);
        assert_eq!(p.nav_bar, BarMode::Show);
        assert!(p.allow_peek);
        assert!(!p.edge_guard);
        assert_eq!(p.osk, OskMode::Auto);
        assert!(!p.keep_awake);
        assert!(p.allow_split);
        assert_eq!(p.background, None);
        assert_eq!(p.inset, None);
        // The default inset is the theme token (12).
        assert!((p.inset_or(crate::theme::Metrics::default().screen_inset) - 12.0).abs() < 1e-6);
    }

    /// `.fullscreen()` is `status_bar: Hide, nav_bar: Hide, allow_peek: true`. Everything else is the default.
    #[test]
    fn fullscreen_hides_both_bars_but_keeps_peek() {
        let p = ChromePolicy::fullscreen();
        assert_eq!(p.status_bar, BarMode::Hide);
        assert_eq!(p.nav_bar, BarMode::Hide);
        assert!(
            p.allow_peek,
            "hidden, it still has to be reachable by pulling the edge"
        );
        let d = ChromePolicy::default();
        assert_eq!(p.edge_guard, d.edge_guard);
        assert_eq!(p.osk, d.osk);
        assert_eq!(p.allow_split, d.allow_split);
        assert_eq!(p.background, d.background);
        // Fullscreen has an inset of 0.
        assert_eq!(p.inset, Some(0.0));
        assert!(p.inset_or(12.0).abs() < 1e-6);
    }

    /// That the defaults do not drift from the enums' `Default`.
    #[test]
    fn bar_and_osk_defaults() {
        assert_eq!(BarMode::default(), BarMode::Show);
        assert_eq!(OskMode::default(), OskMode::Auto);
    }
}
