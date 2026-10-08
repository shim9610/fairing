//! The ribbon mesh. It fills the space between two boundary curves with a strip and
//! puts an alpha-0 feather only on the edges that meet the background.
//!
//! **Why a strip.** `epaint`'s convex fill is a triangle fan from point 0, so it cannot draw a
//! concave silhouette. Sampling both curves N times at the same `t` and making a quad out of
//! each neighbouring pair keeps the triangles right however concave the outline gets. This is an
//! authored strip, not runtime triangulation.

// This is procedural geometry. The names in this file use the brand design's formula notation as it stands —
// `s` (the object unit) · `w`/`h` (the screen) · `t` (a curve parameter) · `u`/`v` (screen fractions) ·
// `g` (geometry) · `c` (colours) · `p` (parameters). Spelled out at length, the document and the code
// could no longer be compared, so two lints are lifted for the whole file (the rest of the pedantic set stays).
#![allow(clippy::many_single_char_names, clippy::similar_names)]

use egui::{Color32, Mesh, Pos2};

/// The feather ring's width in device pixels. `Mesh` triangles get none of epaint's
/// anti-aliasing, so an authored alpha-0 band imitates it. The actual width in points is
/// `FEATHER_PX / ppp`.
pub(super) const FEATHER_PX: f32 = 0.75;

/// The colour of the outer feather vertices.
///
/// **`Color32` is premultiplied** — writing `from_rgba_unmultiplied(r, g, b, 0)` fades to black
/// and leaves a dark rim. It has to be a premultiplied zero.
const FEATHER_EDGE: Color32 = Color32::TRANSPARENT;

/// The four control points of a cubic Bézier (in authored coordinates).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cubic(
    pub (f32, f32),
    pub (f32, f32),
    pub (f32, f32),
    pub (f32, f32),
);

impl Cubic {
    /// The point at `t ∈ [0, 1]`.
    #[must_use]
    pub fn at(self, t: f32) -> Pos2 {
        let (p0, p1, p2, p3) = (self.0, self.1, self.2, self.3);
        let u = 1.0 - t;
        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        egui::pos2(
            a.mul_add(p0.0, b.mul_add(p1.0, c.mul_add(p2.0, d * p3.0))),
            a.mul_add(p0.1, b.mul_add(p1.1, c.mul_add(p2.1, d * p3.1))),
        )
    }
}

/// Sample a joined Bézier chain at **exactly `total` points** (whatever the segment count).
///
/// [`strip`] needs the two boundaries to have the same number of points, and pairing curves with
/// different segment counts (a three-segment leading edge against a two-segment centre line)
/// will never agree when each segment takes a fixed share of points. That is what this is for —
/// it spreads the parameter `t ∈ [0, 1]` evenly across the segments.
pub(super) fn sample_chain_n(chain: &[Cubic], total: usize, out: &mut Vec<Pos2>) {
    out.clear();
    if chain.is_empty() {
        return;
    }
    let total = total.max(2);
    #[expect(clippy::cast_precision_loss, reason = "the counts are small")]
    let segs = chain.len() as f32;
    for i in 0..total {
        #[expect(clippy::cast_precision_loss, reason = "the sample index is small")]
        let t = i as f32 / (total - 1) as f32 * segs;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "t is inside 0..segs"
        )]
        let seg = (t.floor() as usize).min(chain.len() - 1);
        let local = (t - t.floor()).clamp(0.0, 1.0);
        // The last point is the last segment's t = 1.
        let local = if i + 1 == total { 1.0 } else { local };
        if let Some(c) = chain.get(seg) {
            out.push(c.at(local));
        }
    }
}

/// Fill the space between two boundary curves sampled at the same `t` with a strip.
///
/// On mismatched lengths, or fewer than 2 points, it does **nothing** and leaves a `log::warn!`.
/// The guard runs in release too — `debug_assert` disappears there, and a `Mesh` with bad indices
/// is skipped whole by `tessellate_mesh`, so the drawing **silently vanishes**.
pub(super) fn strip(mesh: &mut Mesh, a: &[Pos2], b: &[Pos2], ca: Color32, cb: Color32) {
    if a.len() != b.len() || a.len() < 2 {
        log::warn!(
            "ribbon::strip: edge samples do not line up (a = {}, b = {}) - skipping",
            a.len(),
            b.len()
        );
        return;
    }
    let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for (&pa, &pb) in a.iter().zip(b) {
        mesh.colored_vertex(pa, ca);
        mesh.colored_vertex(pb, cb);
    }
    for i in 0..u32::try_from(a.len() - 1).unwrap_or(0) {
        let (v0, v1, v2, v3) = (
            base + i * 2,
            base + i * 2 + 1,
            base + i * 2 + 2,
            base + i * 2 + 3,
        );
        mesh.add_triangle(v0, v1, v2);
        mesh.add_triangle(v1, v3, v2);
    }
}

/// Put an alpha-0 band outside a closed outline. `width_pt` is a width in **logical points**.
///
/// Outward is the normal of the tangent through each point's two neighbours, and whichever way
/// the polygon winds, the **sign of its area** decides, so it always faces out.
pub(super) fn feather(mesh: &mut Mesh, outline: &[Pos2], inner: Color32, width_pt: f32) {
    if outline.len() < 3 || width_pt <= 0.0 {
        return;
    }
    let sign = if signed_area(outline) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    let n = outline.len();
    let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for (i, &here) in outline.iter().enumerate() {
        let prev = outline.get((i + n - 1) % n).copied().unwrap_or(here);
        let next = outline.get((i + 1) % n).copied().unwrap_or(here);
        let tangent = next - prev;
        let len = tangent.length();
        // A point coincident with its neighbour yields no normal — it becomes a zero-width quad, which is harmless.
        let normal = if len > f32::EPSILON {
            egui::vec2(tangent.y, -tangent.x) / len * sign
        } else {
            egui::Vec2::ZERO
        };
        mesh.colored_vertex(here, inner);
        mesh.colored_vertex(here + normal * width_pt, FEATHER_EDGE);
    }
    for i in 0..u32::try_from(n).unwrap_or(0) {
        let j = (i + 1) % u32::try_from(n).unwrap_or(1);
        let (a0, a1) = (base + i * 2, base + i * 2 + 1);
        let (b0, b1) = (base + j * 2, base + j * 2 + 1);
        mesh.add_triangle(a0, a1, b0);
        mesh.add_triangle(a1, b1, b0);
    }
}

/// A polygon's signed area (the shoelace formula). The sign is the winding direction.
fn signed_area(poly: &[Pos2]) -> f32 {
    let n = poly.len();
    let mut sum = 0.0;
    for (i, &a) in poly.iter().enumerate() {
        let b = poly.get((i + 1) % n).copied().unwrap_or(a);
        sum += a.x.mul_add(b.y, -(b.x * a.y));
    }
    sum * 0.5
}

/// Fill a convex polygon as a fan. Use it **only on convex shapes** — pieces like the rocks, the
/// fish and the bubbles, where convexity is guaranteed at authoring time.
pub(super) fn convex(mesh: &mut Mesh, points: &[Pos2], color: Color32) {
    if points.len() < 3 {
        return;
    }
    let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
    for &p in points {
        mesh.colored_vertex(p, color);
    }
    for i in 1..u32::try_from(points.len() - 1).unwrap_or(0) {
        mesh.add_triangle(base, base + i, base + i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::{convex, feather, strip, FEATHER_EDGE};
    use egui::{pos2, Color32, Mesh};

    const RED: Color32 = Color32::from_rgb(200, 30, 30);

    /// On mismatched or short slices it **draws nothing** rather than panicking.
    #[test]
    fn strip_ignores_mismatched_boundaries() {
        let mut mesh = Mesh::default();
        strip(&mut mesh, &[pos2(0.0, 0.0)], &[], RED, RED);
        strip(&mut mesh, &[pos2(0.0, 0.0)], &[pos2(1.0, 1.0)], RED, RED);
        strip(&mut mesh, &[], &[], RED, RED);
        assert!(mesh.is_empty(), "a bad input makes no vertices");
    }

    /// Sound input gives `2n` vertices · `2(n − 1)` triangles, with every index valid.
    #[test]
    fn strip_builds_a_valid_quad_run() {
        let a = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(2.0, 0.0)];
        let b = [pos2(0.0, 1.0), pos2(1.0, 1.0), pos2(2.0, 1.0)];
        let mut mesh = Mesh::default();
        strip(&mut mesh, &a, &b, RED, RED);
        assert_eq!(mesh.vertices.len(), 6);
        assert_eq!(mesh.indices.len(), 12);
        let n = u32::try_from(mesh.vertices.len()).unwrap_or(0);
        assert!(
            mesh.indices.iter().all(|&i| i < n),
            "the indices are in range"
        );
    }

    /// The outer feather vertices have to be a **premultiplied zero** — otherwise a dark
    /// rim is left behind.
    #[test]
    fn feather_vertices_are_transparent() {
        let outline = [
            pos2(0.0, 0.0),
            pos2(4.0, 0.0),
            pos2(4.0, 3.0),
            pos2(0.0, 3.0),
        ];
        let mut mesh = Mesh::default();
        feather(&mut mesh, &outline, RED, 1.0);
        assert_eq!(mesh.vertices.len(), 8);
        for (i, v) in mesh.vertices.iter().enumerate() {
            if i % 2 == 1 {
                assert_eq!(v.color, FEATHER_EDGE, "outer vertex {i}");
                assert_eq!(v.color.a(), 0, "alpha 0");
                assert_eq!((v.color.r(), v.color.g(), v.color.b()), (0, 0, 0));
            }
        }
    }

    /// The feather goes **outward** for either winding direction.
    #[test]
    fn feather_goes_outward_for_both_windings() {
        for reversed in [false, true] {
            let mut outline = vec![
                pos2(0.0, 0.0),
                pos2(4.0, 0.0),
                pos2(4.0, 4.0),
                pos2(0.0, 4.0),
            ];
            if reversed {
                outline.reverse();
            }
            let mut mesh = Mesh::default();
            feather(&mut mesh, &outline, RED, 1.0);
            let center = egui::pos2(2.0, 2.0);
            for [in_v, out_v] in mesh.vertices.as_chunks::<2>().0 {
                let inner = (in_v.pos - center).length();
                let outer = (out_v.pos - center).length();
                assert!(outer > inner, "reversed = {reversed}: {outer} > {inner}");
            }
        }
    }

    /// A convex fill is `n` vertices · `n − 2` triangles.
    #[test]
    fn convex_fan_has_the_expected_counts() {
        let mut mesh = Mesh::default();
        convex(
            &mut mesh,
            &[pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0)],
            RED,
        );
        assert_eq!(mesh.vertices.len(), 3);
        assert_eq!(mesh.indices.len(), 3);
        convex(&mut mesh, &[pos2(0.0, 0.0), pos2(1.0, 0.0)], RED);
        assert_eq!(mesh.vertices.len(), 3, "fewer than three points is ignored");
    }
}
