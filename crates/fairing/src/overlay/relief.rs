//! **A card's relief** — what makes the frosted card read as a slab of glass standing
//! off the page, not a sheet of tracing paper lying on it.
//!
//! The light is taken to come from above, as the elevation's shadow already assumes, and a raised
//! slab lit from above shows three things a flat sheet does not:
//!
//! - its **top edge catches the light**: a hairline brighter than the face, strongest along the
//!   top and round the top corners, fading out a little way down the sides;
//! - its **bottom edge is in shade**: a hairline darker than the face, fading out the same way up;
//! - its **face has a sheen**: a faint brightening at the top, gone a third of the way down.
//!
//! None of it separates the card from the page. That is still the elevation's own ink — a cast
//! shadow on a light palette, a rim on a dark one ([`fairing_widgets::theme::paint_elevation`]) —
//! and the relief starts inside the rim rather than over it. The opacities differ by palette
//! because the face does: white on a near-white face shows only near opaque, and black on a
//! near-black one hardly at all.
//!
//! All of it is one mesh, built here as plain geometry so it can be tested without a shell, and
//! cached against what it was built from: a card at rest redraws it for nothing, and a
//! card in motion rebuilds it into the buffers it already has.

use egui::epaint::{CornerRadius, Mesh};
use egui::{pos2, vec2, Color32, Pos2, Rect, Vec2};
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

/// Peak opacities at `card_relief = 1` on a light palette. The face is near white, so the white
/// along the top has to be close to opaque to show at all, and the black along the bottom
/// carries most of the depth.
const ON_LIGHT: Relief = Relief {
    light: 0.85,
    shade: 0.12,
    sheen: 0.3,
};

/// The same on a dark palette, where white shows at a third of the opacity and black needs more.
/// The sheen stays faint and shallow: on a dark face it brightens what light text is read
/// against. At 0.03 over a third of the height it takes the console's notifications card from
/// 5.11 to 4.97 : 1 for muted text at its worst; 0.05 over half would have taken it to 4.83.
const ON_DARK: Relief = Relief {
    light: 0.3,
    shade: 0.4,
    sheen: 0.03,
};

/// How far down the sides the light reaches, and up them the shade, as a multiple of the corner
/// radius: round the corner and as far again.
const REACH: f32 = 2.0;

/// How far down the face the sheen reaches, as a share of the card's height.
const SHEEN_DEPTH: f32 = 0.35;

/// The longest step along a side or round a corner, in points — fine enough that a fade reads as
/// a fade, coarse enough that a whole card is a few hundred vertices.
const STEP: f32 = 6.0;

/// The most segments one corner is cut into.
const ARC_MAX: u8 = 32;

/// How strongly each part of the relief is drawn: peak opacities, 0..=1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Relief {
    /// The white along the top edge.
    pub light: f32,
    /// The black along the bottom edge.
    pub shade: f32,
    /// The white of the sheen at the top of the face.
    pub sheen: f32,
}

impl Relief {
    /// The relief on a dark or a light palette at `strength` — `[overlay] card_relief`, where 1
    /// is as designed.
    pub(super) fn of(dark: bool, strength: f32) -> Self {
        if dark { ON_DARK } else { ON_LIGHT }.times(strength)
    }

    /// Every opacity times `k`, none past opaque.
    pub(super) fn times(self, k: f32) -> Self {
        let k = k.max(0.0);
        Self {
            light: (self.light * k).min(1.0),
            shade: (self.shade * k).min(1.0),
            sheen: (self.sheen * k).min(1.0),
        }
    }

    fn is_none(self) -> bool {
        self.light <= 0.0 && self.shade <= 0.0 && self.sheen <= 0.0
    }
}

/// What one relief mesh is built from — and so what the cache compares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Slab {
    /// The card as drawn this frame, rounded to pixels as egui rounds the plate under it.
    pub rect: Rect,
    /// Its corners.
    pub corner: CornerRadius,
    /// How wide the edge lines are: the theme's hairline.
    pub line: f32,
    /// One physical pixel, in points — the anti-aliasing ramp either side of a line.
    pub feather: f32,
    /// How far in from the edge the relief starts: the rim's width where the palette draws one,
    /// so the light and the shade lie inside the boundary rather than over it.
    pub inset: f32,
    /// The opacities, with the card's own fade multiplied in.
    pub relief: Relief,
}

/// One plate's relief mesh and the slab it was built for.
#[derive(Debug, Default)]
pub(super) struct Cache(Option<(Slab, Arc<Mesh>)>);

impl Cache {
    /// The mesh for `slab`. The slab of last time gets the mesh of last time; a changed one is
    /// rebuilt into the same buffers where last frame's shapes have let go of them, and into new
    /// ones only where they have not.
    pub(super) fn mesh(&mut self, slab: &Slab) -> Arc<Mesh> {
        if let Some((built, mesh)) = &mut self.0 {
            if built != slab {
                if let Some(m) = Arc::get_mut(mesh) {
                    build(m, slab);
                } else {
                    let mut m = Mesh::default();
                    build(&mut m, slab);
                    *mesh = Arc::new(m);
                }
                *built = *slab;
            }
            return Arc::clone(mesh);
        }
        let mut m = Mesh::default();
        build(&mut m, slab);
        let mesh = Arc::new(m);
        self.0 = Some((*slab, Arc::clone(&mesh)));
        mesh
    }
}

/// **Build the relief for `slab` into `mesh`**, which is cleared first: the sheen, and over it
/// the edge in two halves. Each half runs from the middle of the top edge round its top corner,
/// down its side and round its bottom corner to the middle of the bottom edge, so every strip
/// runs one way in height and a side is sampled with no buffer.
pub(super) fn build(mesh: &mut Mesh, slab: &Slab) {
    // Not `Mesh::clear`: epaint's swaps the vertex buffer for a new one, and the cache is here to
    // keep it. A rebuild landed in the old buffer only when the allocator happened to hand the
    // same address back, which the test below caught once in forty runs.
    mesh.indices.clear();
    mesh.vertices.clear();
    if slab.relief.is_none() || !slab.rect.is_positive() {
        return;
    }
    let radii = radii(slab.corner, slab.rect);
    if slab.relief.sheen > 0.0 {
        sheen(mesh, slab, radii);
    }
    if slab.relief.light > 0.0 || slab.relief.shade > 0.0 {
        half(mesh, slab, radii, Side::Left);
        half(mesh, slab, radii, Side::Right);
    }
}

/// The four corner radii as `[nw, ne, sw, se]`, none more than half the shorter side.
fn radii(c: CornerRadius, rect: Rect) -> [f32; 4] {
    let most = rect.width().min(rect.height()) * 0.5;
    [c.nw, c.ne, c.sw, c.se].map(|r| f32::from(r).min(most))
}

/// A quadratic fall from 1 at `t = 0` to 0 at `t = 1` and past it — bright at the edge it belongs
/// to, and gone with no kink where it ends.
fn falloff(t: f32) -> f32 {
    let s = (1.0 - t).clamp(0.0, 1.0);
    s * s
}

/// Two premultiplied colours laid on each other as light adds: the light along the top and the
/// shade along the bottom meet only on a card shorter than their two reaches.
fn plus(a: Color32, b: Color32) -> Color32 {
    Color32::from_rgba_premultiplied(
        a.r().saturating_add(b.r()),
        a.g().saturating_add(b.g()),
        a.b().saturating_add(b.b()),
        a.a().saturating_add(b.a()),
    )
}

/// The index the next vertex will get. A relief is a few hundred vertices; a mesh past `u32`
/// would have failed long before this.
fn next_index(mesh: &Mesh) -> u32 {
    u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX)
}

/// How many segments a corner of `radius` is cut into: about one per [`STEP`] of its arc, at
/// least two and at most [`ARC_MAX`].
fn segments(radius: f32) -> u8 {
    let arc = radius * FRAC_PI_2;
    (2..=ARC_MAX)
        .find(|&k| f32::from(k) * STEP >= arc)
        .unwrap_or(ARC_MAX)
}

#[derive(Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// One half of the edge: the middle of the top edge to the middle of the bottom one, down `side`.
fn half(mesh: &mut Mesh, slab: &Slab, radii: [f32; 4], side: Side) {
    let r = slab.rect;
    let reach = (
        (REACH * radii[0].max(radii[1])).max(STEP),
        (REACH * radii[2].max(radii[3])).max(STEP),
    );
    let mut strip = Strip {
        mesh,
        slab,
        reach,
        last: None,
    };
    strip.point(pos2(r.center().x, r.min.y), vec2(0.0, -1.0));
    // An angle `a` is the direction (cos a, sin a) with y down: −π/2 is up, 0 out to the right,
    // π/2 down, π out to the left.
    match side {
        Side::Left => {
            let (top, bottom) = (radii[0], radii[2]);
            strip.arc(pos2(r.min.x + top, r.min.y + top), top, -FRAC_PI_2, -PI);
            strip.side(r.min.x, vec2(-1.0, 0.0), r.min.y + top, r.max.y - bottom);
            strip.arc(
                pos2(r.min.x + bottom, r.max.y - bottom),
                bottom,
                PI,
                FRAC_PI_2,
            );
        }
        Side::Right => {
            let (top, bottom) = (radii[1], radii[3]);
            strip.arc(pos2(r.max.x - top, r.min.y + top), top, -FRAC_PI_2, 0.0);
            strip.side(r.max.x, vec2(1.0, 0.0), r.min.y + top, r.max.y - bottom);
            strip.arc(
                pos2(r.max.x - bottom, r.max.y - bottom),
                bottom,
                0.0,
                FRAC_PI_2,
            );
        }
    }
    strip.point(pos2(r.center().x, r.max.y), vec2(0.0, 1.0));
}

/// A strip along the edge being built. Each point adds a cross-section of four vertices across
/// the line's band, `inset` to `inset + line` in from the edge — clear half a feather outside
/// it, the line's colour half a feather inside each side of it, clear again half a feather past
/// it — and joins it to the last one. On a rect rounded to pixels, as egui rounds the plate, a
/// line one pixel wide lands on exactly one row of them.
struct Strip<'a> {
    mesh: &'a mut Mesh,
    slab: &'a Slab,
    /// How far down from the top the light reaches, and up from the bottom the shade.
    reach: (f32, f32),
    last: Option<u32>,
}

impl Strip<'_> {
    /// The edge point `p`, whose outward normal is `n`.
    fn point(&mut self, p: Pos2, n: Vec2) {
        let s = self.slab;
        let rect = s.rect;
        let colour = plus(
            Color32::WHITE
                .gamma_multiply(s.relief.light * falloff((p.y - rect.min.y) / self.reach.0)),
            Color32::BLACK
                .gamma_multiply(s.relief.shade * falloff((rect.max.y - p.y) / self.reach.1)),
        );
        // A line narrower than a feather is a ramp up and down to a lower peak; either way it
        // covers one line's width.
        let half = 0.5 * s.line;
        let mid = s.inset + half;
        let flat = (half - 0.5 * s.feather).max(0.0);
        let ramp = half + 0.5 * s.feather;
        let peak = colour.gamma_multiply((s.line / s.feather).min(1.0));
        let across = [
            (mid - ramp, Color32::TRANSPARENT),
            (mid - flat, peak),
            (mid + flat, peak),
            (mid + ramp, Color32::TRANSPARENT),
        ];
        let base = next_index(self.mesh);
        for (depth, c) in across {
            self.mesh.colored_vertex(p - n * depth, c);
        }
        if let Some(prev) = self.last {
            for k in 0..3 {
                self.mesh.add_triangle(prev + k, prev + k + 1, base + k + 1);
                self.mesh.add_triangle(prev + k, base + k + 1, base + k);
            }
        }
        self.last = Some(base);
    }

    /// Round a corner of `radius` about `centre`, from the direction `from` to `to`, both ends
    /// included.
    fn arc(&mut self, centre: Pos2, radius: f32, from: f32, to: f32) {
        let n = segments(radius);
        for k in 0..=n {
            let a = from + (to - from) * f32::from(k) / f32::from(n);
            let dir = vec2(a.cos(), a.sin());
            self.point(centre + dir * radius, dir);
        }
    }

    /// Down a straight side at `x`, whose outward normal is `normal`, from `from` to `to` — both
    /// ends left to the corners. A point every [`STEP`] where the light or the shade reaches; in
    /// the quiet middle only its two ends, since nothing there changes.
    fn side(&mut self, x: f32, normal: Vec2, from: f32, to: f32) {
        let light_end = self.slab.rect.min.y + self.reach.0;
        let shade_start = self.slab.rect.max.y - self.reach.1;
        let mut y = from;
        loop {
            let quiet = y >= light_end && y < shade_start;
            let mut next = if quiet { shade_start } else { y + STEP };
            if y < light_end && next > light_end {
                next = light_end;
            }
            if next >= to {
                break;
            }
            self.point(pos2(x, next), normal);
            y = next;
        }
    }
}

/// **The sheen**: rungs across the face from the top down to [`SHEEN_DEPTH`] of the height, each
/// as wide as the card is at that height (inside the inset) and as bright as the fall says. A
/// ladder, so a fall that is not a straight line is drawn as one between close rungs; fine through
/// the corners, where the width changes fastest, and coarse below.
fn sheen(mesh: &mut Mesh, slab: &Slab, radii: [f32; 4]) {
    let r = slab.rect.shrink(slab.inset);
    if !r.is_positive() {
        return;
    }
    let radii = radii.map(|c| (c - slab.inset).max(0.0));
    let depth = slab.rect.height() * SHEEN_DEPTH;
    let end = (r.min.y + depth).min(r.max.y);
    let corners_end = r.min.y + radii[0].max(radii[1]);
    let mut last: Option<u32> = None;
    let mut y = r.min.y;
    loop {
        let rung = y.min(end);
        let (x0, x1) = span(r, radii, rung);
        let c =
            Color32::WHITE.gamma_multiply(slab.relief.sheen * falloff((rung - r.min.y) / depth));
        let base = next_index(mesh);
        mesh.colored_vertex(pos2(x0, rung), c);
        mesh.colored_vertex(pos2(x1, rung), c);
        if let Some(prev) = last {
            mesh.add_triangle(prev, prev + 1, base + 1);
            mesh.add_triangle(prev, base + 1, base);
        }
        last = Some(base);
        if rung >= end {
            break;
        }
        y += if y < corners_end {
            STEP * 0.5
        } else {
            STEP * 2.0
        };
    }
}

/// Where the rounded rect `r`, with corners `radii` (`[nw, ne, sw, se]`), starts and ends across
/// at height `y`.
fn span(r: Rect, radii: [f32; 4], y: f32) -> (f32, f32) {
    (
        r.min.x + bulge(r, radii[0], radii[2], y),
        r.max.x - bulge(r, radii[1], radii[3], y),
    )
}

/// How far in from its straight side the rounded rect `r`'s edge is at height `y`, on a side
/// whose top corner has radius `top` and bottom corner `bottom`.
fn bulge(r: Rect, top: f32, bottom: f32, y: f32) -> f32 {
    let (radius, off) = if y < r.min.y + top {
        (top, r.min.y + top - y)
    } else if y > r.max.y - bottom {
        (bottom, y - (r.max.y - bottom))
    } else {
        return 0.0;
    };
    radius - (radius * radius - off * off).max(0.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::{build, Cache, Relief, Slab};
    use egui::epaint::{CornerRadius, Mesh, Vertex};
    use egui::{pos2, vec2, Rect};
    use std::sync::Arc;

    /// A tall card like the console's, 40-point corners, hairline and feather of one pixel.
    fn slab(relief: Relief) -> Slab {
        Slab {
            rect: Rect::from_min_size(pos2(100.0, 50.0), vec2(300.0, 600.0)),
            corner: CornerRadius::same(40),
            line: 1.0,
            feather: 1.0,
            inset: 0.0,
            relief,
        }
    }

    fn built(s: &Slab) -> Mesh {
        let mut m = Mesh::default();
        build(&mut m, s);
        m
    }

    /// The vertices within `band` of height `y` that draw anything.
    fn lit_near(m: &Mesh, y: f32, band: f32) -> Vec<Vertex> {
        m.vertices
            .iter()
            .filter(|v| (v.pos.y - y).abs() <= band && v.color.a() > 0)
            .copied()
            .collect()
    }

    /// **The top edge catches the light and the bottom edge is in shade** — white along the top
    /// at the light's opacity, black along the bottom.
    #[test]
    fn the_top_edge_catches_the_light_and_the_bottom_is_in_shade() {
        let s = slab(Relief {
            light: 0.8,
            shade: 0.4,
            sheen: 0.0,
        });
        let m = built(&s);
        let top = lit_near(&m, s.rect.min.y, 2.5);
        let bottom = lit_near(&m, s.rect.max.y, 2.5);
        assert!(
            !top.is_empty() && top.iter().all(|v| v.color.r() == v.color.a()),
            "white along the top: {top:?}"
        );
        assert!(
            !bottom.is_empty() && bottom.iter().all(|v| v.color.r() == 0),
            "black along the bottom: {bottom:?}"
        );
        let peak = top.iter().map(|v| v.color.a()).max().unwrap_or(0);
        assert!((200..=208).contains(&peak), "the light at 0.8: {peak}");
    }

    /// **A line one pixel wide is one row of pixels** on a rect on pixel boundaries: its colour
    /// peaks at the centre of the first row in from the inset, and is clear at the centres of the
    /// rows either side.
    #[test]
    fn a_hairline_lands_on_one_row_of_pixels() {
        for inset in [0.0, 1.0] {
            let s = Slab {
                inset,
                ..slab(Relief {
                    light: 1.0,
                    shade: 0.0,
                    sheen: 0.0,
                })
            };
            let m = built(&s);
            let cx = s.rect.center().x;
            let at = |y: f32| {
                m.vertices
                    .iter()
                    .filter(|v| (v.pos.x - cx).abs() < 1e-3 && (v.pos.y - y).abs() < 1e-3)
                    .map(|v| v.color.a())
                    .max()
            };
            let row = s.rect.min.y + inset + 0.5;
            assert_eq!(at(row), Some(255), "the row the line is on (inset {inset})");
            assert_eq!(at(row - 1.0), Some(0), "the row outside it (inset {inset})");
            assert_eq!(at(row + 1.0), Some(0), "the row inside it (inset {inset})");
        }
    }

    /// **Halfway down a side there is no line at all** — the light is gone a little way below the
    /// top corners and the shade a little way above the bottom ones, so the sides are quiet.
    #[test]
    fn the_sides_are_quiet_in_the_middle() {
        let s = slab(Relief {
            light: 1.0,
            shade: 1.0,
            sheen: 0.0,
        });
        let m = built(&s);
        let loud = lit_near(&m, s.rect.center().y, 0.2 * s.rect.height());
        assert!(loud.is_empty(), "nothing drawn down the middle: {loud:?}");
    }

    /// **Nothing is drawn outside the card** — not past its sides, and not past its rounded
    /// corners — with or without an inset. Only the clear end of a line's outer ramp may sit
    /// half a feather out, where it draws nothing.
    #[test]
    fn the_relief_stays_inside_the_card() {
        for inset in [0.0, 1.0] {
            let s = Slab {
                inset,
                ..slab(Relief {
                    light: 1.0,
                    shade: 1.0,
                    sheen: 1.0,
                })
            };
            let m = built(&s);
            assert!(!m.vertices.is_empty(), "something is drawn");
            let r = 40.0;
            let inner = s.rect.shrink(r);
            for v in &m.vertices {
                // Inside the rounded rect: within `r` of the rect shrunk by `r`.
                let nearest = v.pos.clamp(inner.min, inner.max);
                let out = (v.pos - nearest).length() - r;
                let allowed = if v.color.a() > 0 {
                    1e-3
                } else {
                    0.5 * s.feather + 1e-3
                };
                assert!(
                    out <= allowed,
                    "{:?} ({:?}) is {out} outside the card (inset {inset})",
                    v.pos,
                    v.color
                );
            }
        }
    }

    /// **The sheen is brightest at the top and gone a third of the way down** — never brighter
    /// lower down.
    #[test]
    fn the_sheen_fades_down_the_face() {
        let s = slab(Relief {
            light: 0.0,
            shade: 0.0,
            sheen: 0.5,
        });
        let m = built(&s);
        let mut rungs: Vec<(f32, u8)> = m.vertices.iter().map(|v| (v.pos.y, v.color.a())).collect();
        rungs.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert!(
            rungs
                .windows(2)
                .all(|w| matches!(w, [upper, lower] if lower.1 <= upper.1)),
            "never brighter lower down: {rungs:?}"
        );
        assert_eq!(
            rungs.first().map(|r| r.1),
            Some(128),
            "half white at the top"
        );
        assert_eq!(
            rungs.last().map(|r| r.1),
            Some(0),
            "and nothing at its foot"
        );
        let foot = rungs.last().map_or(0.0, |r| r.0);
        let depth = s.rect.min.y + 0.35 * s.rect.height();
        assert!(
            (foot - depth).abs() < 1.0,
            "which is a third of the way down: {foot}"
        );
    }

    /// **No strength, no mesh** — `card_relief = 0` draws nothing at all.
    #[test]
    fn no_strength_draws_nothing() {
        let m = built(&slab(Relief::of(false, 0.0)));
        assert!(m.vertices.is_empty() && m.indices.is_empty());
    }

    /// **Each palette has its own opacities, and the strength scales them** — never past opaque.
    #[test]
    fn the_strength_scales_the_relief_and_stops_at_opaque() {
        let one = Relief::of(true, 1.0);
        let two = Relief::of(true, 2.0);
        assert!(
            (two.shade - 2.0 * one.shade).abs() < 1e-6,
            "{one:?} → {two:?}"
        );
        assert!(
            (two.sheen - 2.0 * one.sheen).abs() < 1e-6,
            "{one:?} → {two:?}"
        );
        let lit = Relief::of(false, 2.0);
        assert!(lit.light <= 1.0, "white stops at opaque: {lit:?}");
        assert_ne!(Relief::of(true, 1.0), Relief::of(false, 1.0));
    }

    /// **A card at rest is not rebuilt, and one that moved is rebuilt in the buffers it has**.
    #[test]
    fn the_cache_reuses_the_mesh_and_its_buffers() {
        let mut cache = Cache::default();
        let s = slab(Relief::of(false, 1.0));
        let first = cache.mesh(&s);
        let again = cache.mesh(&s);
        assert!(Arc::ptr_eq(&first, &again), "the same slab, the same mesh");
        let buffer = first.vertices.as_ptr();
        drop((first, again));
        let moved = Slab {
            rect: s.rect.translate(vec2(0.0, 3.0)),
            ..s
        };
        let rebuilt = cache.mesh(&moved);
        assert_eq!(
            rebuilt.vertices.as_ptr(),
            buffer,
            "rebuilt in place once nothing else held it"
        );
        let highest = rebuilt
            .vertices
            .iter()
            .map(|v| v.pos.y)
            .fold(f32::INFINITY, f32::min);
        let edge = moved.rect.min.y - 0.5 * moved.feather;
        assert!(
            (highest - edge).abs() < 1e-3,
            "and built for where the card is now: {highest} vs {edge}"
        );
        // A rebuild with fewer vertices keeps the bigger buffer — the check that does not lean on
        // the allocator handing an address back.
        let room = rebuilt.vertices.capacity();
        drop(rebuilt);
        let tight = Slab {
            corner: CornerRadius::same(4),
            ..moved
        };
        let smaller = cache.mesh(&tight);
        assert!(
            smaller.vertices.len() < room && smaller.vertices.capacity() >= room,
            "a smaller rebuild gave the buffer up: {} of {} against {room}",
            smaller.vertices.len(),
            smaller.vertices.capacity()
        );
    }
}
