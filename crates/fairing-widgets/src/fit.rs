//! Aspect-preserving image fitting: containment and cropped coverage.

/// Fit the source aspect ratio inside `into`, as with CSS `object-fit: contain`.
///
/// The caller supplies the source dimensions; a `TextureId` alone does not carry
/// them. Return `into` unchanged when a source dimension is zero or negative.
///
/// ```
/// use fairing_widgets::fit;
/// let into = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
/// // A square original in a wide slot — it fits the height and leaves room at the sides.
/// let at = fit::contain(egui::vec2(50.0, 50.0), into);
/// assert_eq!(at.size(), egui::vec2(100.0, 100.0));
/// assert_eq!(at.center(), into.center());
/// ```
#[must_use]
pub fn contain(source: egui::Vec2, into: egui::Rect) -> egui::Rect {
    if source.x <= 0.0 || source.y <= 0.0 {
        return into;
    }
    let k = (into.width() / source.x).min(into.height() / source.y);
    egui::Rect::from_center_size(into.center(), source * k)
}

/// The UV Rect that **covers** `into` keeping the aspect ratio — CSS's `object-fit: cover`.
///
/// The opposite of [`contain`]: it fills the place and crops what overflows. A background photo is
/// this one. The point is that what comes back is not a Rect but a **UV** (0…1) — the place drawn to
/// stays `into` and it narrows **the region read** from the original. Hand it to
/// `painter.image(id, into, this, tint)`.
///
/// ```
/// use fairing_widgets::fit;
/// let into = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
/// // A wide original in a square slot — the sides are cropped and only the middle read.
/// let uv = fit::cover_uv(egui::vec2(200.0, 100.0), into);
/// assert!((uv.width() - 0.5).abs() < 1e-6);
/// assert!((uv.height() - 1.0).abs() < 1e-6);
/// ```
///
/// Where the original's size or the place is unknown (0 or below) it hands the whole original (`0…1`) back.
#[must_use]
pub fn cover_uv(source: egui::Vec2, into: egui::Rect) -> egui::Rect {
    let full = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if source.x <= 0.0 || source.y <= 0.0 || into.width() <= 0.0 || into.height() <= 0.0 {
        return full;
    }
    let want = into.width() / into.height();
    let have = source.x / source.y;
    let (u, v) = if have > want {
        (want / have, 1.0)
    } else {
        (1.0, have / want)
    };
    egui::Rect::from_center_size(egui::pos2(0.5, 0.5), egui::vec2(u, v))
}
