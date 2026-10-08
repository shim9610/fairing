//! The manta mark. Not an icon but a **ribbon mesh**.
//!
//! The original mark is **two-tone** — a navy back over a white belly — and its silhouette is
//! concave. The icon pipeline cannot do it: `IconDef::fill` is one `bool` for the whole icon, the
//! painter fills every sub-path in the same colour, and filling adjacent convex pieces in the same
//! colour has epaint anti-alias each piece separately, leaving a hairline along the seam.
//!
//! **A strip goes round the concavity**. Sampling the upper and lower boundaries N times at
//! the same `t` and making a quad out of each neighbouring pair keeps the triangles right however
//! concave the outline gets. The deep notches of the cephalic fins are inside the boundary curves
//! too, so they need no drawing of their own.
//!
//! The icon set carries a separate **stroked** manta (`assets/icons/manta.svg`) — the two outputs
//! are each right in their own place.

// This is procedural geometry. The names in this file use the formula notation as it stands —
// `s` (the object unit) · `w`/`h` (the screen) · `t` (a curve parameter) · `u`/`v` (screen fractions) ·
// `g` (geometry) · `c` (colours) · `p` (parameters). Spelled out at length, the formulas and the code
// could no longer be compared, so two lints are lifted for the whole file (the rest of `pedantic` stays).
#![allow(clippy::many_single_char_names, clippy::similar_names)]

use super::ribbon::{self, Cubic};
use egui::{pos2, Color32, Mesh, Pos2, Rect};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

// ── The authored coordinates (a 32×20 mark grid, y increasing downwards) ──
//
// The 24×24 square is for icons; the mark is a wide swept wing, so it is authored at 32×20. The head
// faces right, the same way as the original `assets/brand/manta-mark.webp`.
//
// **These coordinates were not placed by hand but drawn from the original art.**
// `assets/brand/manta-mark-black.png` (a 1600×1000 flat silhouette) was traced sub-pixel and fitted by
// least squares with a cubic Bézier chain, and the back/belly boundary was drawn the same way from the
// two-colour boundary in `manta-mark-two-tone.png`. The fit error is **at most 0.36** grid units (1.1 px
// on a 96 px mark), and the IoU between the result rasterised at 32 samples and the original is **0.976**.
//
// Normalisation: the bbox of the **whole** mark (the wings plus the tail) is scaled uniformly into 32×20
// and centred, with a 2 % margin. So even the tail's tip comes inside the target Rect.

/// The **upper** boundary of the silhouette. The near wingtip (upper left) → the far wingtip
/// (right). The two cephalic-fin notches are inside this curve.
pub(super) const BODY_UPPER: [Cubic; 8] = [
    Cubic((1.85, 2.70), (3.01, 0.15), (3.83, -0.61), (4.49, 2.63)),
    Cubic((4.49, 2.63), (5.43, 4.30), (7.11, 5.39), (8.85, 6.07)),
    Cubic((8.85, 6.07), (10.63, 6.74), (12.49, 7.09), (14.42, 7.06)),
    Cubic((14.42, 7.06), (16.33, 7.08), (18.08, 6.44), (19.75, 5.50)),
    Cubic((19.75, 5.50), (22.29, 4.55), (23.45, 3.89), (21.92, 6.69)),
    Cubic((21.92, 6.69), (22.47, 7.68), (26.72, 10.22), (25.28, 7.12)),
    Cubic((25.28, 7.12), (27.07, 5.56), (27.00, 9.71), (26.16, 10.84)),
    Cubic(
        (26.16, 10.84),
        (27.12, 13.88),
        (30.52, 9.45),
        (30.23, 11.29),
    ),
];

/// The **lower** boundary of the silhouette. It shares the two endpoints `(1.85, 2.70)` and
/// `(30.23, 11.29)` with [`BODY_UPPER`], so the space between them closes into a ribbon.
pub(super) const BODY_LOWER: [Cubic; 6] = [
    Cubic((1.85, 2.70), (1.31, 4.82), (2.57, 6.90), (4.22, 8.21)),
    Cubic((4.22, 8.21), (5.81, 9.56), (7.89, 10.27), (9.49, 11.62)),
    Cubic((9.49, 11.62), (12.09, 13.25), (7.61, 16.83), (11.87, 13.98)),
    Cubic(
        (11.87, 13.98),
        (13.84, 13.26),
        (16.06, 13.17),
        (18.21, 13.30),
    ),
    Cubic(
        (18.21, 13.30),
        (20.40, 13.43),
        (22.51, 13.69),
        (24.74, 13.64),
    ),
    Cubic(
        (24.74, 13.64),
        (26.64, 13.50),
        (29.42, 13.36),
        (30.23, 11.29),
    ),
];

/// The near wing's **belly** — the upper boundary. In the original art the belly is not one band
/// but **two lobes** (under the near wing, and under the far one). So rather than splitting it
/// with a single centre line, a ribbon goes over each lobe. They **overlap** the body, so there is
/// no seam problem.
pub(super) const BELLY_NEAR_UPPER: [Cubic; 5] = [
    Cubic((1.87, 3.19), (2.05, 4.22), (2.83, 5.20), (3.75, 5.82)),
    Cubic((3.75, 5.82), (4.69, 6.43), (5.75, 6.75), (6.83, 6.95)),
    Cubic((6.83, 6.95), (7.91, 7.17), (9.04, 7.31), (10.07, 7.67)),
    Cubic((10.07, 7.67), (11.10, 7.98), (12.18, 8.50), (12.83, 9.45)),
    Cubic(
        (12.83, 9.45),
        (13.48, 10.39),
        (13.42, 11.65),
        (12.78, 12.57),
    ),
];

/// The near wing's belly — the lower boundary.
pub(super) const BELLY_NEAR_LOWER: [Cubic; 5] = [
    Cubic((1.87, 3.19), (1.71, 4.57), (2.22, 5.80), (2.98, 6.88)),
    Cubic((2.98, 6.88), (3.74, 7.99), (4.85, 8.87), (5.96, 9.55)),
    Cubic((5.96, 9.55), (7.06, 10.26), (8.25, 10.78), (9.32, 11.55)),
    Cubic(
        (9.32, 11.55),
        (10.56, 12.36),
        (10.89, 13.79),
        (10.06, 15.00),
    ),
    Cubic(
        (10.06, 15.00),
        (10.41, 14.79),
        (12.16, 13.48),
        (12.78, 12.57),
    ),
];

/// The far wing's belly — the upper boundary.
pub(super) const BELLY_FAR_UPPER: [Cubic; 5] = [
    Cubic(
        (29.28, 12.49),
        (28.03, 12.97),
        (26.43, 12.55),
        (25.67, 11.39),
    ),
    Cubic((25.67, 11.39), (25.08, 10.07), (24.32, 9.03), (22.76, 8.91)),
    Cubic((22.76, 8.91), (21.35, 8.80), (20.05, 9.24), (18.82, 9.76)),
    Cubic(
        (18.82, 9.76),
        (17.60, 10.31),
        (16.46, 11.01),
        (15.33, 11.75),
    ),
    Cubic(
        (15.33, 11.75),
        (14.20, 12.48),
        (13.07, 13.23),
        (11.94, 13.97),
    ),
];

/// The far wing's belly — the lower boundary.
pub(super) const BELLY_FAR_LOWER: [Cubic; 5] = [
    Cubic(
        (29.28, 12.49),
        (28.33, 13.15),
        (27.19, 13.35),
        (26.05, 13.50),
    ),
    Cubic(
        (26.05, 13.50),
        (24.88, 13.64),
        (23.66, 13.62),
        (22.46, 13.58),
    ),
    Cubic(
        (22.46, 13.58),
        (21.27, 13.51),
        (20.09, 13.40),
        (18.90, 13.32),
    ),
    Cubic(
        (18.90, 13.32),
        (17.73, 13.23),
        (16.49, 13.18),
        (15.30, 13.26),
    ),
    Cubic(
        (15.30, 13.26),
        (14.13, 13.36),
        (12.99, 13.56),
        (11.94, 13.97),
    ),
];

/// The tail's **centre line**. It comes out from under the body and flows down to the left.
pub(super) const TAIL: [Cubic; 3] = [
    Cubic((9.73, 14.92), (9.68, 15.78), (8.28, 16.44), (7.61, 16.97)),
    Cubic((7.61, 16.97), (6.72, 17.50), (5.72, 17.86), (4.72, 18.14)),
    Cubic((4.72, 18.14), (3.74, 18.44), (2.66, 18.76), (2.03, 19.60)),
];

/// The tail's half-width (in grid units) — at the root, and at the tip. Half of the original art's measurements (a diameter of 0.345 → 0.093).
pub(super) const TAIL_HALF: (f32, f32) = (0.17, 0.05);

/// The authoring grid's width.
pub(super) const GRID_W: f32 = 32.0;
/// The authoring grid's height.
pub(super) const GRID_H: f32 = 20.0;

/// The LOD. Reckoned in **device pixels**, not logical points. The same `size_pt` at a
/// different `pixels_per_point` gives a different LOD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MantaLod {
    /// `< 16 px` — a size at which any silhouette is a smudge. Only the outline is stroked.
    Glyph,
    /// `16..24 px` — a flat mass. No eyes, no tail, no fins.
    Silhouette,
    /// `24..48 px` — flat plus the fins, one eye and the tail.
    Solid,
    /// `>= 48 px` — two-tone plus the far wing, both eyes, the tail and the feather.
    Full,
}

impl MantaLod {
    /// Chosen by the size in device pixels.
    #[must_use]
    pub fn for_size_px(size_px: f32) -> Self {
        if size_px < 16.0 {
            Self::Glyph
        } else if size_px < 24.0 {
            Self::Silhouette
        } else if size_px < 48.0 {
            Self::Solid
        } else {
            Self::Full
        }
    }
}

/// The **two** colours actually drawn. Always derived from one base colour — the mark keeps no
/// colour system of its own, so the icon tint, the theme cross-fade and the disabled alpha all
/// follow it unchanged.
///
/// There are no eyes. The mark in the original art
/// (`assets/brand/manta-mark-black.png` · `-two-tone.png`) is an eyeless two-tone silhouette, and
/// even at a size where eyes would read, the two-tone split alone reads as a manta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MantaTones {
    /// The back.
    pub body: Color32,
    /// The belly.
    pub belly: Color32,
}

impl MantaTones {
    /// Derived from the one body colour.
    #[must_use]
    pub fn from_body(body: Color32, belly_mix: f32) -> Self {
        Self {
            body,
            belly: body.lerp_to_gamma(Color32::WHITE, belly_mix.clamp(0.0, 1.0)),
        }
    }
}

/// The mark's **overrides**.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MantaStyle {
    /// `None` = `body.lerp_to_gamma(WHITE, belly_mix)`.
    pub belly: Option<Color32>,
    /// The belly colour's mix ratio.
    pub belly_mix: f32,
    /// `None` = chosen by size (recommended).
    pub lod: Option<MantaLod>,
    /// Mirror horizontally — head to the left.
    pub flip_x: bool,
}

impl Default for MantaStyle {
    fn default() -> Self {
        Self {
            belly: None,
            belly_mix: 0.62,
            lod: None,
            flip_x: false,
        }
    }
}

impl MantaStyle {
    /// Make the two colours from a base colour (with the overrides applied).
    #[must_use]
    pub fn tones(self, body: Color32) -> MantaTones {
        let base = MantaTones::from_body(body, self.belly_mix);
        MantaTones {
            body,
            belly: self.belly.unwrap_or(base.belly),
        }
    }
}

/// The baked mark mesh cache.
///
/// Why `Arc<Mesh>`: `Shape::mesh` takes an `impl Into<Arc<Mesh>>` and the variant is
/// `Shape::Mesh(Arc<Mesh>)` — an `Rc` cannot be handed over.
///
/// **The vertices are in absolute screen coordinates.** `Shape::transform` is an `Arc::make_mut`,
/// so moving a shared mesh copies both `Vec`s whole. Which is why **the origin is part of the key**.
#[derive(Debug, Default)]
pub(super) struct MantaCache {
    entries: HashMap<MantaKey, Arc<Mesh>>,
    /// The insertion order — dropping half takes the oldest first.
    order: Vec<MantaKey>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MantaKey {
    x_px: i32,
    y_px: i32,
    w_px: i32,
    h_px: i32,
    ppp_q: u32,
    lod: MantaLod,
    flip_x: bool,
    tones: MantaTones,
    stroke_q: u32,
}

impl MantaCache {
    /// The cap. Past it, the oldest half is dropped.
    pub(super) const CAPACITY: usize = 32;

    /// Empty it (after a theme change or a palette swap).
    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// How many meshes it holds.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is empty.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn get_or_bake(&mut self, key: MantaKey, rect: Rect, ppp: f32, stroke_px: f32) -> Arc<Mesh> {
        if let Some(mesh) = self.entries.get(&key) {
            return Arc::clone(mesh);
        }
        if self.entries.len() >= Self::CAPACITY {
            let drop_n = Self::CAPACITY / 2;
            log::debug!(
                "MantaCache: hit the {} cap - dropping the {drop_n} oldest entries",
                Self::CAPACITY
            );
            for k in self.order.drain(..drop_n.min(self.order.len())) {
                self.entries.remove(&k);
            }
        }
        let mut mesh = Mesh::default();
        append_manta(
            &mut mesh, rect, key.lod, key.tones, key.flip_x, ppp, stroke_px,
        );
        let mesh = Arc::new(mesh);
        self.entries.insert(key, Arc::clone(&mesh));
        self.order.push(key);
        mesh
    }
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "screen coordinates are inside i32's range"
)]
fn px(v: f32) -> i32 {
    v.round() as i32
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "ppp and the line width are small positive numbers"
)]
fn q64(v: f32) -> u32 {
    (v.max(0.0) * 64.0).round() as u32
}

fn key_for(
    rect: Rect,
    ppp: f32,
    lod: MantaLod,
    tones: MantaTones,
    flip_x: bool,
    stroke_px: f32,
) -> MantaKey {
    MantaKey {
        x_px: px(rect.min.x * ppp),
        y_px: px(rect.min.y * ppp),
        w_px: px(rect.width() * ppp),
        h_px: px(rect.height() * ppp),
        ppp_q: q64(ppp),
        lod,
        flip_x,
        tones,
        stroke_q: q64(stroke_px),
    }
}

/// The form where the cache is shared through a `RefCell`. Used inside [`manta_painter`] — an
/// `IconPainter` is an `Fn`, so a `&mut` cannot be threaded through it, and sync-check bans
/// `OnceLock` / `LazyLock`.
pub(super) fn paint_manta_in(
    painter: &egui::Painter,
    rect: Rect,
    lod: MantaLod,
    tones: MantaTones,
    flip_x: bool,
    stroke_px: f32,
    cache: &RefCell<MantaCache>,
) {
    let ppp = painter.pixels_per_point();
    let key = key_for(rect, ppp, lod, tones, flip_x, stroke_px);
    let mesh = cache.borrow_mut().get_or_bake(key, rect, ppp, stroke_px);
    painter.add(egui::Shape::mesh(mesh));
}

/// Put it into [`crate::icons::IconSet::register_painter`] and the mark becomes an
/// `IconRef::Custom`, usable **anywhere an icon goes**.
///
/// It keeps the icon contract exactly: the body colour is the `color` passed in, so the
/// `IconColor::Role` tint, the theme cross-fade and `IconStyle::enabled(false)`'s translucency all
/// follow. The LOD is chosen from `style.size × painter.pixels_per_point()`, so the size need not
/// be known at declaration time.
///
/// ```
/// use fairing::brand::{manta_painter, MantaStyle};
///
/// let painter = manta_painter(MantaStyle {
///     belly_mix: 0.30, // a less bright belly
///     flip_x: true,    // head to the left
///     ..MantaStyle::default()
/// });
/// # let _ = painter;
/// ```
pub fn manta_painter(style: MantaStyle) -> crate::icons::IconPainter {
    let cache = RefCell::new(MantaCache::default());
    Box::new(move |painter, rect, icon_style, color| {
        let ppp = painter.pixels_per_point();
        let size_px = rect.width().min(rect.height()) * ppp;
        let lod = style.lod.unwrap_or_else(|| MantaLod::for_size_px(size_px));
        paint_manta_in(
            painter,
            rect,
            lod,
            style.tones(color),
            style.flip_x,
            icon_style.stroke.unwrap_or(2.0),
            &cache,
        );
    })
}

/// Append straight onto a mesh with no cache. Used by Abyss when it merges the formation into its own mesh.
pub(super) fn append_manta(
    mesh: &mut Mesh,
    rect: Rect,
    lod: MantaLod,
    tones: MantaTones,
    flip_x: bool,
    ppp: f32,
    stroke_px: f32,
) {
    // The 32×20 grid is scaled uniformly into the target Rect and centred.
    let fit = (rect.width() / GRID_W).min(rect.height() / GRID_H);
    if fit <= 0.0 {
        return;
    }
    let center = rect.center();
    let map = move |q: Pos2| {
        let x = (q.x - GRID_W * 0.5) * fit;
        let y = (q.y - GRID_H * 0.5) * fit;
        pos2(center.x + if flip_x { -x } else { x }, center.y + y)
    };
    let feather_pt = ribbon::FEATHER_PX / ppp.max(0.1);
    let n = sample_count(rect, ppp);

    let (mut upper, mut lower) = (Vec::new(), Vec::new());
    chain_px(&BODY_UPPER, n, &mut upper, map);
    chain_px(&BODY_LOWER, n, &mut lower, map);

    if lod == MantaLod::Glyph {
        // At a size where it would be a smudge, it is **stroked** rather than filled.
        let w = (stroke_px / ppp.max(0.1)).max(0.5);
        stroke_chain(mesh, &upper, tones.body, w);
        stroke_chain(mesh, &lower, tones.body, w);
        return;
    }

    // One silhouette. The cephalic fins' notches are inside the upper boundary, so they are not drawn separately.
    ribbon::strip(mesh, &upper, &lower, tones.body, tones.body);

    // The belly is laid **over** the body — it overlaps rather than abuts, so there is no
    // seam. `Silhouette` is a flat mass, so it is left out.
    if lod != MantaLod::Silhouette {
        let (mut bu, mut bl) = (Vec::new(), Vec::new());
        chain_px(&BELLY_NEAR_UPPER, n, &mut bu, map);
        chain_px(&BELLY_NEAR_LOWER, n, &mut bl, map);
        ribbon::strip(mesh, &bu, &bl, tones.belly, tones.belly);
        if lod == MantaLod::Full {
            chain_px(&BELLY_FAR_UPPER, n, &mut bu, map);
            chain_px(&BELLY_FAR_LOWER, n, &mut bl, map);
            ribbon::strip(mesh, &bu, &bl, tones.belly, tones.belly);
        }
    }

    if matches!(lod, MantaLod::Solid | MantaLod::Full) {
        tail(mesh, n, map, tones.body, fit);
    }
    if lod == MantaLod::Full {
        // The feather goes only on the edges that meet the background. The outline = the upper boundary → the lower boundary reversed.
        let mut outline = upper;
        outline.extend(lower.iter().rev().skip(1).copied());
        ribbon::feather(mesh, &outline, tones.body, feather_pt);
    }
}

/// The sample count `N = clamp(round(size_px / 6), 8, 32)`. `size_px` is in device pixels.
fn sample_count(rect: Rect, ppp: f32) -> usize {
    let size_px = rect.width().max(rect.height()) * ppp;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the sample count is clamped to 8..32"
    )]
    let n = (size_px / 6.0).round().clamp(8.0, 32.0) as usize;
    n
}

/// Sample an authored-coordinate chain at **exactly `n` points** and move it into pixels. Only by
/// separating the point count from the segment count do the leading edge (3 segments) and the
/// centre line (2 segments) go into the same strip.
fn chain_px(chain: &[Cubic], n: usize, out: &mut Vec<Pos2>, map: impl Fn(Pos2) -> Pos2) {
    ribbon::sample_chain_n(chain, n, out);
    for q in out.iter_mut() {
        *q = map(*q);
    }
}

/// The tail — thick at the root, thin at the tip. The widths are the original art's measurements ([`TAIL_HALF`]).
fn tail(mesh: &mut Mesh, n: usize, map: impl Fn(Pos2) -> Pos2, color: Color32, fit: f32) {
    let mut pts = Vec::new();
    chain_px(&TAIL, n, &mut pts, map);
    taper(mesh, &pts, color, TAIL_HALF.0 * fit, TAIL_HALF.1 * fit);
}

/// A curve as a band of even width.
fn thicken(mesh: &mut Mesh, pts: &[Pos2], color: Color32, half: f32) {
    taper(mesh, pts, color, half, half);
}

/// A curve as a band narrowing from `half0` at the root to `half1` at the tip.
fn taper(mesh: &mut Mesh, pts: &[Pos2], color: Color32, half0: f32, half1: f32) {
    if pts.len() < 2 {
        return;
    }
    let (mut a, mut b) = (Vec::with_capacity(pts.len()), Vec::with_capacity(pts.len()));
    let n = pts.len();
    for (i, &p) in pts.iter().enumerate() {
        // An endpoint takes itself as its neighbour — the one-sided difference becomes the tangent.
        let prev = pts.get(i.wrapping_sub(1)).copied().unwrap_or(p);
        let next = pts.get(i + 1).copied().unwrap_or(p);
        let t = next - prev;
        let len = t.length();
        let normal = if len > f32::EPSILON {
            egui::vec2(t.y, -t.x) / len
        } else {
            egui::Vec2::ZERO
        };
        #[expect(clippy::cast_precision_loss, reason = "the sample index is small")]
        let k = i as f32 / (n - 1) as f32;
        let half = (half1 - half0).mul_add(k, half0);
        a.push(p + normal * half);
        b.push(p - normal * half);
    }
    ribbon::strip(mesh, &a, &b, color, color);
}

/// The `Glyph` LOD's stroking — one thin band.
fn stroke_chain(mesh: &mut Mesh, pts: &[Pos2], color: Color32, width_pt: f32) {
    thicken(mesh, pts, color, width_pt * 0.5);
}

#[cfg(test)]
mod tests {
    use super::{
        append_manta, px, MantaCache, MantaLod, MantaStyle, MantaTones, BELLY_FAR_LOWER,
        BELLY_FAR_UPPER, BELLY_NEAR_LOWER, BELLY_NEAR_UPPER, BODY_LOWER, BODY_UPPER, GRID_H,
        GRID_W, TAIL,
    };
    use egui::{Color32, Mesh, Rect};

    const BODY: Color32 = Color32::from_rgb(0x06, 0x18, 0x2a);

    fn tones() -> MantaTones {
        MantaTones::from_body(BODY, 0.62)
    }

    /// **A regression test**: the upper boundary has 8 segments, the lower 6 and the belly 5 — all
    /// different. Sampled per segment the point counts can never agree, and then [`ribbon::strip`]
    /// silently skips and **a whole wing disappears** (which is exactly what happened). This pins
    /// down that the curves used in pairs come out at the same point count.
    #[test]
    fn paired_edges_sample_to_the_same_count() {
        for (a_chain, b_chain) in [
            (&BODY_UPPER[..], &BODY_LOWER[..]),
            (&BELLY_NEAR_UPPER[..], &BELLY_NEAR_LOWER[..]),
            (&BELLY_FAR_UPPER[..], &BELLY_FAR_LOWER[..]),
        ] {
            let mut a = Vec::new();
            let mut b = Vec::new();
            super::ribbon::sample_chain_n(a_chain, 17, &mut a);
            super::ribbon::sample_chain_n(b_chain, 17, &mut b);
            assert_eq!((a.len(), b.len()), (17, 17));
            assert_eq!(a.first(), b.first(), "they share one end");
            assert_eq!(a.last(), b.last(), "they share the other end too");
        }
    }

    /// The wing ribbon really does make triangles — a skipped strip leaves vertices with zero area.
    #[test]
    fn the_wing_ribbon_covers_real_area() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(320.0, 200.0));
        let mut mesh = Mesh::default();
        append_manta(&mut mesh, rect, MantaLod::Full, tones(), false, 1.0, 2.0);
        // The wings cover a good part of the target Rect. It is checked by the bounding box.
        let (mut lo, mut hi) = (
            egui::pos2(f32::MAX, f32::MAX),
            egui::pos2(f32::MIN, f32::MIN),
        );
        for v in &mesh.vertices {
            lo = egui::pos2(lo.x.min(v.pos.x), lo.y.min(v.pos.y));
            hi = egui::pos2(hi.x.max(v.pos.x), hi.y.max(v.pos.y));
        }
        assert!(hi.x - lo.x > rect.width() * 0.7, "it spreads horizontally");
        assert!(hi.y - lo.y > rect.height() * 0.5, "and vertically too");
        assert!(
            mesh.indices.len() > 300,
            "there are enough triangles: {}",
            mesh.indices.len() / 3
        );
    }

    /// A paired boundary has to share both endpoints exactly for the ribbon to close.
    /// The property is easy to break when the coordinates are re-traced from the original art, so
    /// it is pinned down separately.
    #[test]
    fn paired_edges_share_their_endpoints() {
        assert_eq!(
            BODY_UPPER[0].0, BODY_LOWER[0].0,
            "they share the near wingtip"
        );
        assert_eq!(BODY_UPPER[7].3, BODY_LOWER[5].3, "and the far wingtip too");
        assert_eq!(BELLY_NEAR_UPPER[0].0, BELLY_NEAR_LOWER[0].0);
        assert_eq!(BELLY_NEAR_UPPER[4].3, BELLY_NEAR_LOWER[4].3);
        assert_eq!(BELLY_FAR_UPPER[0].0, BELLY_FAR_LOWER[0].0);
        assert_eq!(BELLY_FAR_UPPER[4].3, BELLY_FAR_LOWER[4].3);
    }

    /// A belly lobe has to sit **inside** the silhouette — outside it, a white smudge shows beside
    /// the body. Checked cheaply against the authored coordinates' bbox.
    #[test]
    fn the_belly_lobes_sit_inside_the_silhouette() {
        let bbox = |chains: &[super::Cubic]| {
            let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
            for c in chains {
                for (x, y) in [c.0, c.1, c.2, c.3] {
                    lo = (lo.0.min(x), lo.1.min(y));
                    hi = (hi.0.max(x), hi.1.max(y));
                }
            }
            (lo, hi)
        };
        let mut body: Vec<super::Cubic> = Vec::new();
        body.extend(BODY_UPPER);
        body.extend(BODY_LOWER);
        let (blo, bhi) = bbox(&body);
        for lobe in [
            &[BELLY_NEAR_UPPER, BELLY_NEAR_LOWER].concat()[..],
            &[BELLY_FAR_UPPER, BELLY_FAR_LOWER].concat()[..],
        ] {
            let (lo, hi) = bbox(lobe);
            assert!(
                lo.0 >= blo.0 - 0.5 && lo.1 >= blo.1 - 0.5,
                "{lo:?} vs {blo:?}"
            );
            assert!(
                hi.0 <= bhi.0 + 0.5 && hi.1 <= bhi.1 + 0.5,
                "{hi:?} vs {bhi:?}"
            );
        }
    }

    /// The authored coordinates stay within the 32×20 grid — outside it, they overflow the target
    /// Rect.
    ///
    /// The **points on the curve** are what counts. A control point may stray a little outside the
    /// grid (the original art's wingtips are sharp, so that is how the fit came out) and that does
    /// not make the curve overflow.
    #[test]
    fn authored_curves_stay_in_the_grid() {
        let mut chains: Vec<super::Cubic> = Vec::new();
        for c in [
            &BODY_UPPER[..],
            &BODY_LOWER[..],
            &BELLY_NEAR_UPPER[..],
            &BELLY_NEAR_LOWER[..],
            &BELLY_FAR_UPPER[..],
            &BELLY_FAR_LOWER[..],
            &TAIL[..],
        ] {
            chains.extend_from_slice(c);
        }
        let mut pts = Vec::new();
        super::ribbon::sample_chain_n(&chains, 4000, &mut pts);
        for p in pts {
            assert!((0.0..=GRID_W).contains(&p.x), "x = {}", p.x);
            assert!((0.0..=GRID_H).contains(&p.y), "y = {}", p.y);
        }
    }

    /// The same `size_pt` at a different ppp gives a different LOD.
    #[test]
    fn manta_lod_follows_device_pixels() {
        let size_pt = 20.0;
        assert_eq!(MantaLod::for_size_px(size_pt * 1.0), MantaLod::Silhouette);
        assert_eq!(MantaLod::for_size_px(size_pt * 1.5), MantaLod::Solid);
        assert_eq!(MantaLod::for_size_px(size_pt * 2.0), MantaLod::Solid);
        assert_eq!(MantaLod::for_size_px(size_pt * 3.0), MantaLod::Full);
        assert_eq!(MantaLod::for_size_px(8.0), MantaLod::Glyph);
    }

    /// The indices are valid at every LOD — a mesh with bad ones **silently disappears** in release.
    #[test]
    fn every_lod_builds_a_valid_mesh() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(96.0, 60.0));
        for lod in [
            MantaLod::Glyph,
            MantaLod::Silhouette,
            MantaLod::Solid,
            MantaLod::Full,
        ] {
            let mut mesh = Mesh::default();
            append_manta(&mut mesh, rect, lod, tones(), false, 2.0, 2.0);
            assert!(!mesh.is_empty(), "{lod:?} drew nothing");
            let n = u32::try_from(mesh.vertices.len()).unwrap_or(0);
            assert!(mesh.indices.iter().all(|&i| i < n), "{lod:?} index range");
            assert_eq!(mesh.indices.len() % 3, 0, "{lod:?} triangle count");
            for v in &mesh.vertices {
                assert!(
                    v.pos.x.is_finite() && v.pos.y.is_finite(),
                    "{lod:?} coordinates"
                );
            }
        }
    }

    /// Every vertex stays inside the target Rect (plus the feather's margin).
    #[test]
    fn vertices_stay_inside_the_target_rect() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(120.0, 80.0));
        let mut mesh = Mesh::default();
        append_manta(&mut mesh, rect, MantaLod::Full, tones(), false, 1.0, 2.0);
        let slack = rect.expand(2.0);
        for v in &mesh.vertices {
            assert!(slack.contains(v.pos), "{:?} is outside {rect:?}", v.pos);
        }
    }

    /// A flip mirrors x only.
    ///
    /// It does not require the vertex **order** to match — the normals that thicken a curve swap
    /// the two boundaries over in a mirror image, so the same shape comes out in a different order.
    /// The shape is what is under test: whether the mirrored coordinate sets agree.
    #[test]
    fn flip_mirrors_horizontally() {
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(96.0, 60.0));
        let bake = |flip| {
            let mut mesh = Mesh::default();
            append_manta(&mut mesh, rect, MantaLod::Solid, tones(), flip, 1.0, 2.0);
            mesh
        };
        let (a, b) = (bake(false), bake(true));
        assert_eq!(a.vertices.len(), b.vertices.len());
        let cx = rect.center().x;
        let key = |x: f32, y: f32| (px(x * 16.0), px(y * 16.0));
        let mut want: Vec<_> = a
            .vertices
            .iter()
            .map(|v| key(2.0f32.mul_add(cx, -v.pos.x), v.pos.y))
            .collect();
        let mut got: Vec<_> = b.vertices.iter().map(|v| key(v.pos.x, v.pos.y)).collect();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(want, got, "the mirrored coordinate sets have to match");
    }

    /// `len()` is unchanged by repeated draws at a fixed Rect.
    #[test]
    fn manta_cache_hits_for_a_fixed_slot() {
        let rect = Rect::from_min_size(egui::pos2(4.0, 4.0), egui::vec2(48.0, 30.0));
        let mut cache = MantaCache::default();
        let key = super::key_for(rect, 1.0, MantaLod::Solid, tones(), false, 2.0);
        for _ in 0..20 {
            let _ = cache.get_or_bake(key, rect, 1.0, 2.0);
        }
        assert_eq!(
            cache.len(),
            1,
            "the same place and the same colour bakes once"
        );
    }

    /// Past the cap it drops half and carries on (no panic, no unbounded growth).
    #[test]
    fn cache_drops_half_at_capacity() {
        let mut cache = MantaCache::default();
        for i in 0..(MantaCache::CAPACITY + 4) {
            #[expect(clippy::cast_precision_loss, reason = "it is a small integer")]
            let x = i as f32;
            let rect = Rect::from_min_size(egui::pos2(x, 0.0), egui::vec2(48.0, 30.0));
            let key = super::key_for(rect, 1.0, MantaLod::Solid, tones(), false, 2.0);
            let _ = cache.get_or_bake(key, rect, 1.0, 2.0);
        }
        assert!(
            cache.len() <= MantaCache::CAPACITY,
            "it keeps to the ceiling"
        );
        cache.clear();
        assert!(cache.is_empty());
    }

    /// The style overrides take effect, and without them the colours are derived from the body.
    #[test]
    fn style_overrides_win_over_derived_tones() {
        let pink = Color32::from_rgb(0xff, 0x00, 0x99);
        let derived = MantaStyle::default().tones(BODY);
        assert_eq!(derived.body, BODY);
        assert_eq!(derived.belly, MantaTones::from_body(BODY, 0.62).belly);
        let forced = MantaStyle {
            belly: Some(pink),
            ..MantaStyle::default()
        }
        .tones(BODY);
        assert_eq!(forced.belly, pink);
        assert_eq!(forced.body, BODY, "the body is always the colour passed in");
    }
}
