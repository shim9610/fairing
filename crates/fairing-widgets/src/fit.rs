//! **Fitting a picture into a place** — the two ways, and nothing else.
//!
//! [`contain`] gives the whole picture and leaves bars; [`cover_uv`] fills the place and crops. They
//! take no `Ui` and deal only in numbers, so they work where there is no `Ui` to hand — inside a
//! grid cell, inside a card's image box, or against a bare `Painter`.
//!
//! They live here rather than in `fairing::layout` because a widget needs them too: a
//! [`MediaCard`](crate::widgets::MediaCard)'s image box is a cover, and a second copy of this
//! arithmetic beside it is how this crate once ended up with five uncoordinated stroke widths.
//! `fairing::layout` re-exports both, so nothing moved for a caller.

/// The Rect fitted **inside** `into` keeping the aspect ratio — CSS's `object-fit: contain`.
///
/// [`IconRef::Texture`](crate::icons::IconRef::Texture) holds only a `TextureId` and so **does
/// not know the original size**.
/// It therefore fills the `rect` it is given as it stands, and a 3:2 photo handed to a square icon
/// slot is drawn squashed. It is not the kind of thing the crate can fix from inside — only whoever
/// uploaded the texture knows its size. They can narrow the Rect with this function before handing
/// it over.
///
/// `source` is the original's pixel size (`TextureHandle::size_vec2()`). It need not be a texture —
/// anything whose size is known will do.
///
/// ```
/// use fairing_widgets::fit;
/// let into = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
/// // A square original in a wide slot — it fits the height and leaves room at the sides.
/// let at = fit::contain(egui::vec2(50.0, 50.0), into);
/// assert_eq!(at.size(), egui::vec2(100.0, 100.0));
/// assert_eq!(at.center(), into.center());
/// ```
///
/// Where the original's size is unknown (0 or below) it hands `into` back as it stands — a texture
/// not yet uploaded is one such case.
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
