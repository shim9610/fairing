//! The scrim (A1): covers the content at alpha `0.5 × y/H` (linear). It absorbs input
//! with `Sense::click`, and a tap closes. A card shade draws no scrim and uses
//! [`catch`] — the same layer, taking the tap and painting nothing.
//!
//! **The colour is [`ColorRole::Scrim`].** Hard-coding black would mean the scrim each preset
//! chose (128 for core, 150 / 110 / 100 elsewhere) is ignored in the shade alone — inside one
//! shell the desktop scrim used the role colour while the shade did not. The role colour's
//! **alpha is the maximum darkness**, and the pull progress sweeps from zero up to it.
// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of the pedantic set stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::theme::{ColorRole, Theme};
use egui::{Rect, Sense};

/// The scrim's maximum alpha.
pub(crate) const MAX_ALPHA: f32 = 0.5;

/// The scrim `Area`'s id name (the z-order table). The panel is a sublayer of this layer.
pub(crate) const AREA_ID: &str = "fairing.overlay.scrim";

/// The scrim alpha at a `y/H` progress.
#[must_use]
pub(crate) fn alpha(progress: f32) -> f32 {
    MAX_ALPHA * progress.clamp(0.0, 1.0)
}

/// The colour to paint — the hue of [`ColorRole::Scrim`], with an alpha swept by the pull
/// progress and capped at **that role colour's own alpha**.
///
/// `alpha` is what [`alpha`] produced, `MAX_ALPHA × progress`, so dividing leaves the pure
/// progress. On the core preset (128) this is pixel-identical to today's render, and a preset
/// that chose a different darkness now gets exactly that.
#[must_use]
pub(crate) fn fill(alpha: f32, theme: &Theme) -> egui::Color32 {
    let progress = (alpha / MAX_ALPHA).clamp(0.0, 1.0);
    // `Color32` is **premultiplied**, so multiplying `.r()` by the alpha again as it stands multiplies it
    // twice and darkens the colour. It is unpacked, read, and multiplied again.
    let [r, g, b, a] = theme.color(ColorRole::Scrim).to_srgba_unmultiplied();
    egui::Color32::from_rgba_unmultiplied(r, g, b, (f32::from(a) * progress) as u8)
}

/// Draw the scrim over the content (`Area(Order::Foreground)`, registered before the panel so it
/// is underneath). Returns `true` if it was tapped.
#[must_use]
pub(crate) fn show(ctx: &egui::Context, rect: Rect, alpha: f32, theme: &Theme) -> bool {
    if alpha <= 0.0 {
        return false;
    }
    let fill = fill(alpha, theme);
    egui::Area::new(egui::Id::new(AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.painter().rect_filled(rect, 0.0, fill);
            ui.allocate_rect(rect, Sense::click()).clicked()
        })
        .inner
}

/// **Catch the tap outside without darkening anything** — the card reveal's stand-in for the
/// scrim. A card lies on an undimmed page, so there is nothing to paint; but a tap
/// beside an open card still closes it and still goes nowhere else, and that needs a layer under
/// the panel that takes the press. The same `Area` as [`show`]'s, so the panel's sublayer pinning
/// ([`super::Overlay::ui`]) holds for both. Returns `true` if it was tapped.
#[must_use]
pub(crate) fn catch(ctx: &egui::Context, rect: Rect) -> bool {
    egui::Area::new(egui::Id::new(AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .fade_in(false)
        .show(ctx, |ui| ui.allocate_rect(rect, Sense::click()).clicked())
        .inner
}

#[cfg(test)]
mod tests {
    use super::{alpha, fill, MAX_ALPHA};
    use crate::theme::{ColorRole, Theme};

    /// Change the role colour and the scrim follows — hard-coded black would mean the scrim each
    /// preset chose is ignored in the shade alone.
    #[test]
    fn the_scrim_follows_the_theme_role() {
        let mut theme = Theme::default();
        let full = fill(MAX_ALPHA, &theme).to_srgba_unmultiplied();
        let role = theme.color(ColorRole::Scrim).to_srgba_unmultiplied();
        assert_eq!(
            full, role,
            "a fully drawn scrim differs from the role colour"
        );

        theme.palette.set(
            ColorRole::Scrim,
            egui::Color32::from_rgba_unmultiplied(20, 0, 60, 200),
        );
        let tinted = fill(MAX_ALPHA, &theme).to_srgba_unmultiplied();
        assert_eq!(
            tinted,
            [20, 0, 60, 200],
            "it did not follow the role colour's hue and depth"
        );
    }

    /// A1: the alpha is `0.5 × y/H`, linear. The reference frame-6 figure is y = 146 on H = 480,
    /// which is 0.152.
    #[test]
    fn the_alpha_is_half_the_pull_progress() {
        assert!((alpha(146.0 / 480.0) - 0.152).abs() < 0.001);
        assert!((alpha(0.5) - 0.25).abs() < f32::EPSILON);
    }

    /// Progress sweeps the alpha. Not pulled means transparent.
    #[test]
    fn the_pull_progress_sweeps_the_alpha() {
        let theme = Theme::default();
        assert_eq!(fill(alpha(0.0), &theme).a(), 0);
        let half = fill(alpha(0.5), &theme).a();
        let whole = fill(alpha(1.0), &theme).a();
        assert!(half > 0 && half < whole, "{half} / {whole}");
        // Outside the range it is clamped.
        assert_eq!(fill(alpha(2.0), &theme).a(), whole);
    }
}
