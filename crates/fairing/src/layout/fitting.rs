//! Fitting.

use super::{Cx, FIT_MIN_SIZE};

/// Draw text at a size reduced to fit the available width using a `Painter`.
/// Use [`fit_size`] first when sibling cells should share a fitted text size.
///
/// ```no_run
/// # fn draw(ui: &egui::Ui, cx: &fairing::Cx<'_>, cell_rect: egui::Rect) {
/// use fairing::layout;
/// layout::fit_text(
///     ui.painter(),
///     cell_rect.center(),
///     egui::Align2::CENTER_CENTER,
///     "Ethiopia Yirgacheffe 200 g",
///     cell_rect.width() * 0.9,
///     egui::FontId::proportional(cx.theme.metrics.row_height * 0.3),
///     cx.theme.color(fairing::ColorRole::OnSurface),
/// );
/// # }
/// ```
pub fn fit_text(
    painter: &egui::Painter,
    at: egui::Pos2,
    align: egui::Align2,
    text: &str,
    max_width: f32,
    font: egui::FontId,
    color: egui::Color32,
) {
    let font = fit_size(painter, std::slice::from_ref(&text), max_width, font);
    painter.text(at, align, text, font, color);
}
/// Compute a shared text size against the longest supplied label.
/// This can keep sibling cells at one size; [`fit_text`] can instead fit each
/// individual label. The caller supplies the group, maximum width and base font.
/// The result has an eight-point floor, so sufficiently narrow space can still overflow.
///
/// ```no_run
/// # fn draw(ui: &egui::Ui, base: egui::FontId, w: f32) {
/// use fairing::layout;
/// let labels = ["Credit / debit card", "Mobile pay", "Cash · call an assistant"];
/// // All three are drawn in this font — the size fitted to the longest, "Cash · call an assistant".
/// let font = layout::fit_size(ui.painter(), &labels, w, base);
/// # let _ = font;
/// # }
/// ```
#[must_use]
pub fn fit_size(
    painter: &egui::Painter,
    texts: &[&str],
    max_width: f32,
    base: egui::FontId,
) -> egui::FontId {
    let mut size = base.size;
    for text in texts {
        let width = painter
            .layout_no_wrap((*text).to_owned(), base.clone(), egui::Color32::WHITE)
            .size()
            .x;
        if width > max_width && width > 0.0 {
            size = size.min(base.size * max_width / width);
        }
    }
    egui::FontId {
        size: size.max(FIT_MIN_SIZE),
        family: base.family,
    }
}
/// A Wi-Fi strength of `0..=4` as a fan icon. Being parametric, it cannot be changed by swapping the
/// icon set (guide 09 §6's pitfall) — the colour is a role, so the palette does follow.
pub fn wifi_icon(cx: &Cx<'_>, painter: &egui::Painter, rect: egui::Rect, level: u8, off: bool) {
    let style = crate::icons::IconStyle::sized(rect.width()).param_style(cx.theme);
    crate::icons::parametric::wifi(painter, rect, level, off, &style);
}
/// The rounding for a slider's value display. The track's range is `f32`, so it is shown as an integer.
#[expect(
    clippy::cast_possible_truncation,
    reason = "for display, and the slider range is a device value so it never exceeds i64"
)]
pub(super) fn rounded(value: f32) -> i64 {
    value.round() as i64
}
