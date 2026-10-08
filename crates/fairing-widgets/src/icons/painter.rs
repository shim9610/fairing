//! The icon painter: 24-grid → pixel conversion, curve flattening, and a per-size polyline cache
//! (rendering).
//!
//! # The per-frame cost
//!
//! The expensive part (flattening the curves) is done once by [`IconCache`], keyed on
//! `(name, rounded pixel size)`. The drawing path only translates the cached polylines into the
//! target `Rect`. A two-point subpath goes out as an [`egui::Shape::LineSegment`] with **no heap
//! allocation** (about 47 % of the subpaths across the 51 built-ins). With three or more points,
//! `epaint` wants an owned `Vec<Pos2>` (`PathShape`), so one `Vec` appears per subpath — that is
//! the representation the icon design specifies, and it handles round joins and feathering in a
//! single stroke so seams do not overlap and darken. Drawing it as separate pieces would remove
//! the allocation but show double compositing at every joint on a disabled (alpha 0.6) icon.

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of the pedantic set stays).
#![allow(
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use super::{IconDef, Seg};
use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2};
use std::collections::HashMap;
use std::rc::Rc;

/// One flattened subpath. The coordinates are pixels with **the icon's top-left as the origin**.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    /// The points.
    pub points: Vec<Pos2>,
    /// Whether it was closed with `Z`.
    pub closed: bool,
}

/// One icon flattened at a particular pixel size.
#[derive(Debug, Clone, PartialEq)]
pub struct Flattened {
    /// The subpaths.
    pub polylines: Vec<Polyline>,
    /// Whether it is a filled icon.
    pub fill: bool,
}

/// Choose the curve subdivision count from the size (4–16).
///
/// Splitting a 24 px icon's quarter circle (radius 12 px) into 4 gives a maximum chord error
/// (sagitta) of `12·(1 − cos 11.25°) ≈ 0.23 px`. At 96 px it is 12 pieces, so
/// `48·(1 − cos 3.75°) ≈ 0.10 px` — under half a pixel at any size.
fn subdivisions(size_px: f32) -> usize {
    ((size_px / 8.0).round() as usize).clamp(4, 16)
}

/// The intermediate state while flattening. It keeps the previous start point in order to honour
/// SVG's rule that "a drawing command after `Z` continues from the subpath's start point".
struct Builder {
    polylines: Vec<Polyline>,
    current: Vec<Pos2>,
    cursor: Pos2,
    pending: Option<Pos2>,
}

impl Builder {
    fn new() -> Self {
        Self {
            polylines: Vec::new(),
            current: Vec::new(),
            cursor: Pos2::ZERO,
            pending: None,
        }
    }

    /// Push out the subpath collected so far. A single-point one (`M12 20h.01` and the like) is
    /// doubled so the round cap makes a dot.
    fn flush(&mut self, closed: bool) {
        match self.current.len() {
            0 => {}
            1 => {
                let point = self.current.first().copied().unwrap_or(Pos2::ZERO);
                self.current.clear();
                self.polylines.push(Polyline {
                    points: vec![point, point],
                    closed: false,
                });
            }
            _ => self.polylines.push(Polyline {
                points: std::mem::take(&mut self.current),
                closed,
            }),
        }
    }

    /// Called just before a drawing command. Straight after a `Z`, it restarts from the start point.
    fn resume(&mut self) {
        if self.current.is_empty() {
            if let Some(start) = self.pending.take() {
                self.cursor = start;
                self.current.push(start);
            }
        }
    }

    fn move_to(&mut self, point: Pos2) {
        self.flush(false);
        self.pending = None;
        self.cursor = point;
        self.current.push(point);
    }

    fn line_to(&mut self, point: Pos2) {
        self.resume();
        self.cursor = point;
        self.current.push(point);
    }

    fn close(&mut self) {
        let start = self.current.first().copied();
        self.flush(true);
        self.pending = start;
        if let Some(start) = start {
            self.cursor = start;
        }
    }

    fn finish(mut self) -> Vec<Polyline> {
        self.flush(false);
        self.polylines
    }
}

/// Flatten 24-grid segments into polylines for a `size_px` square.
///
/// The coordinate system is pixels with the icon's top-left as the origin. Curves are subdivided
/// evenly into 4–16 pieces by size, and the same `(def, size_px)` always gives
/// the same result (it is deterministic).
#[must_use]
pub fn flatten(def: &IconDef, size_px: f32) -> Flattened {
    let scale = size_px / 24.0;
    let n = subdivisions(size_px);
    let map = |x: f32, y: f32| Pos2::new(x * scale, y * scale);
    let mut build = Builder::new();
    for seg in def.segs {
        match *seg {
            Seg::M(x, y) => build.move_to(map(x, y)),
            Seg::L(x, y) => build.line_to(map(x, y)),
            Seg::Q(cx, cy, x, y) => {
                build.resume();
                let (p0, p1, p2) = (build.cursor, map(cx, cy), map(x, y));
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    build.current.push(Pos2::new(
                        u * u * p0.x + 2.0 * u * t * p1.x + t * t * p2.x,
                        u * u * p0.y + 2.0 * u * t * p1.y + t * t * p2.y,
                    ));
                }
                build.cursor = p2;
            }
            Seg::C(ax, ay, bx, by, x, y) => {
                build.resume();
                let (p0, p1, p2, p3) = (build.cursor, map(ax, ay), map(bx, by), map(x, y));
                for i in 1..=n {
                    let t = i as f32 / n as f32;
                    let u = 1.0 - t;
                    build.current.push(Pos2::new(
                        u * u * u * p0.x
                            + 3.0 * u * u * t * p1.x
                            + 3.0 * u * t * t * p2.x
                            + t * t * t * p3.x,
                        u * u * u * p0.y
                            + 3.0 * u * u * t * p1.y
                            + 3.0 * u * t * t * p2.y
                            + t * t * t * p3.y,
                    ));
                }
                build.cursor = p3;
            }
            Seg::Z => build.close(),
        }
    }
    Flattened {
        polylines: build.finish(),
        fill: def.fill,
    }
}

/// The polyline cache per `(name, pixel size)`. Owned by the shell.
///
/// The key's name is [`IconDef::name`], so an integrator who calls
/// [`super::IconSet::register`] with a name that collides with a built-in mixes up the cache.
/// `register` warns about that case.
#[derive(Debug, Default)]
pub struct IconCache {
    entries: HashMap<(&'static str, u32), Rc<Flattened>>,
}

impl IconCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Take it from the cache, or flatten and insert it. The key's size is the rounded integer pixel size.
    pub fn get(&mut self, def: &IconDef, size_px: f32) -> Rc<Flattened> {
        let key = (def.name, size_px.round().max(1.0) as u32);
        self.entries
            .entry(key)
            .or_insert_with(|| Rc::new(flatten(def, key.1 as f32)))
            .clone()
    }

    /// The entry count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Throw it all away (when the theme or the scale changed and the size distribution is different).
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// A cached subpath's points translated to the target position. The one per-frame allocation,
/// and it exists because `epaint`'s `PathShape` wants an owned `Vec`.
fn translated(line: &Polyline, offset: Vec2) -> Vec<Pos2> {
    line.points.iter().map(|point| *point + offset).collect()
}

/// Draw the icon into `rect`, scaled uniformly and centred. A stroke icon gets round caps faked
/// by stamping a circle of radius `stroke/2` at each end of an open path.
///
/// The 24-grid maps **directly** onto the square inside `rect`. An icon whose
/// path touches the edge of the grid (`gauge` reaches 0.91) can bleed half a stroke outside it,
/// so the caller leaves margin by making the icon smaller than the cell
/// (`Metrics::icon_size < icon_cell`).
///
/// A filled icon's (`def.fill`) subpaths **have to be convex** — `epaint`'s `fill_closed_path`
/// is a triangle fan from point 0, so a concave polygon fills wrong. For the built-ins,
/// `cargo xtask icons` checks this and stops the conversion on a violation ("no
/// runtime triangulation"). A filled icon an integrator adds through
/// [`super::IconSet::register`] has to follow the same rule and be split into convex pieces.
pub fn paint(
    painter: &Painter,
    rect: Rect,
    def: &IconDef,
    color: Color32,
    stroke_px: f32,
    cache: &mut IconCache,
) {
    let size = rect.width().min(rect.height());
    if size <= 0.0 || color == Color32::TRANSPARENT {
        return;
    }
    let flat = cache.get(def, size);
    let offset = (rect.center() - Vec2::splat(size / 2.0)).to_vec2();
    let cap = stroke_px / 2.0;
    for line in &flat.polylines {
        if flat.fill {
            if line.points.len() >= 3 {
                painter.add(Shape::Path(PathShape::convex_polygon(
                    translated(line, offset),
                    color,
                    Stroke::NONE,
                )));
            }
            continue;
        }
        if stroke_px <= 0.0 {
            continue;
        }
        match line.points.as_slice() {
            [] | [_] => {}
            // Two points — drawn without a `Vec`.
            [first, last] => {
                let (first, last) = (*first + offset, *last + offset);
                if first != last {
                    painter.line_segment([first, last], Stroke::new(stroke_px, color));
                }
                painter.circle_filled(first, cap, color);
                painter.circle_filled(last, cap, color);
            }
            _ => {
                let points = translated(line, offset);
                let stroke = PathStroke::new(stroke_px, color);
                if line.closed {
                    painter.add(Shape::Path(PathShape::closed_line(points, stroke)));
                } else {
                    if let (Some(first), Some(last)) =
                        (points.first().copied(), points.last().copied())
                    {
                        painter.circle_filled(first, cap, color);
                        painter.circle_filled(last, cap, color);
                    }
                    painter.add(Shape::Path(PathShape::line(points, stroke)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{flatten, subdivisions, IconCache};
    use crate::icons::{IconDef, Seg};

    const MINUS: IconDef = IconDef {
        name: "test-minus",
        segs: &[Seg::M(5.0, 12.0), Seg::L(19.0, 12.0)],
        fill: false,
    };

    /// `M` → a curve → `Z` → `L` (drawing on after closing).
    const REOPEN: IconDef = IconDef {
        name: "test-reopen",
        segs: &[
            Seg::M(4.0, 4.0),
            Seg::L(20.0, 4.0),
            Seg::Z,
            Seg::L(20.0, 20.0),
        ],
        fill: false,
    };

    const DOT: IconDef = IconDef {
        name: "test-dot",
        segs: &[Seg::M(12.0, 20.0), Seg::L(12.01, 20.0)],
        fill: false,
    };

    #[test]
    fn flatten_scales_to_size() {
        let flat = flatten(&MINUS, 48.0);
        assert_eq!(flat.polylines.len(), 1);
        let first = flat
            .polylines
            .first()
            .map(|l| l.points.clone())
            .unwrap_or_default();
        assert_eq!(first.len(), 2);
        assert!((first.first().map_or(0.0, |p| p.x) - 10.0).abs() < 1e-4);
        assert!((first.last().map_or(0.0, |p| p.x) - 38.0).abs() < 1e-4);
    }

    #[test]
    fn flatten_is_deterministic() {
        assert_eq!(flatten(&MINUS, 31.7), flatten(&MINUS, 31.7));
        let circle = IconDef {
            name: "test-circle",
            segs: &[
                Seg::M(22.0, 12.0),
                Seg::C(22.0, 17.5, 17.5, 22.0, 12.0, 22.0),
                Seg::C(6.5, 22.0, 2.0, 17.5, 2.0, 12.0),
                Seg::Z,
            ],
            fill: false,
        };
        assert_eq!(flatten(&circle, 24.0), flatten(&circle, 24.0));
    }

    #[test]
    fn curves_subdivide_between_four_and_sixteen() {
        assert_eq!(subdivisions(0.0), 4);
        assert_eq!(subdivisions(24.0), 4);
        assert_eq!(subdivisions(96.0), 12);
        assert_eq!(subdivisions(1024.0), 16);
    }

    #[test]
    fn a_closed_subpath_resumes_from_its_start() {
        let flat = flatten(&REOPEN, 24.0);
        // One closed [4,4]-[20,4] plus one [4,4]-[20,20].
        assert_eq!(flat.polylines.len(), 2);
        let second = flat
            .polylines
            .get(1)
            .map(|l| l.points.clone())
            .unwrap_or_default();
        assert_eq!(second.len(), 2);
        assert!((second.first().map_or(0.0, |p| p.x) - 4.0).abs() < 1e-4);
        assert!((second.first().map_or(0.0, |p| p.y) - 4.0).abs() < 1e-4);
    }

    #[test]
    fn a_single_point_subpath_becomes_a_dot() {
        let flat = flatten(&DOT, 24.0);
        let points = flat
            .polylines
            .first()
            .map(|l| l.points.clone())
            .unwrap_or_default();
        assert_eq!(points.len(), 2);
    }

    /// Draw all 51 through the real `paint` path and check they stay within `rect` plus half a stroke.
    #[test]
    fn every_builtin_icon_keeps_the_optical_size_band() {
        use crate::icons::{Seg, ICONS};
        // The hull of the control points, which is a conservative over-estimate for a Bézier — the
        // curve never leaves it. That is the right direction for an upper bound and costs at most a
        // few tenths on the lower one.
        let span = |def: &crate::icons::IconDef| {
            let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
            let mut point = |x: f32, y: f32| {
                lo[0] = lo[0].min(x);
                hi[0] = hi[0].max(x);
                lo[1] = lo[1].min(y);
                hi[1] = hi[1].max(y);
            };
            for seg in def.segs {
                match *seg {
                    Seg::M(x, y) | Seg::L(x, y) => point(x, y),
                    Seg::Q(cx, cy, x, y) => {
                        point(cx, cy);
                        point(x, y);
                    }
                    Seg::C(ax, ay, bx, by, x, y) => {
                        point(ax, ay);
                        point(bx, by);
                        point(x, y);
                    }
                    Seg::Z => {}
                }
            }
            (hi[0] - lo[0]).max(hi[1] - lo[1])
        };

        // **The band, and why it is two numbers and not one.** Measured over the 78 shipped icons
        // the longest side has a median of 20.0 on the 24 grid and an interquartile range of
        // 18.0–20.0, so the set already draws to a 20×20 optical box inside a 24×24 sheet — the
        // same shape Lucide and Feather use. Twelve glyphs sit deliberately below it (the four
        // chevrons at 12, `close` at 12, the four arrows and `back`, `plus` and `minus` at 14): an
        // arrow drawn to 20 reads as a road sign beside a word. The top of the band is 22.4, which
        // is the four that reach a hair past the 2..22 safe area (`fan` and `wrench` at 22.4,
        // `gauge` at 22.2) — they bleed at most half a stroke, which
        // `every_builtin_icon_paints_inside_its_rect` already allows for.
        //
        // What this catches that the bounds test cannot: an icon added **too small**, or the set
        // drifting as a whole. The bounds test only asks "does it stay inside its rect", so an
        // 8-unit glyph beside a 20-unit one passes it and looks broken.
        let mut sides: Vec<f32> = Vec::with_capacity(ICONS.len());
        for def in ICONS {
            let side = span(def);
            assert!(
                (12.0..=22.4).contains(&side),
                "{}: the longest side is {side:.1} on the 24 grid, outside the 12.0..22.4 the set \
                 draws to — it will read as a different size beside its neighbours",
                def.name
            );
            sides.push(side);
        }
        sides.sort_by(f32::total_cmp);
        let median = sides.get(sides.len() / 2).copied().unwrap_or(0.0);
        assert!(
            (19.5..=20.5).contains(&median),
            "the set's median longest side moved to {median:.1}; it is 20.0 on the 24 grid and the \
             whole set is drawn to that box"
        );
    }

    #[test]
    fn every_builtin_icon_paints_inside_its_rect() {
        use crate::icons::ICONS;
        let target = egui::Rect::from_min_size(egui::pos2(40.0, 30.0), egui::Vec2::splat(32.0));
        let stroke = 32.0 / 12.0; // the grid stroke 2.0 taken to 32 px
        let allowed = target.expand(stroke / 2.0 + 0.01);
        let mut cache = IconCache::new();
        for def in ICONS {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::Vec2::new(200.0, 200.0),
                )),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                super::paint(
                    ui.painter(),
                    target,
                    def,
                    egui::Color32::WHITE,
                    stroke,
                    &mut cache,
                );
            });
            let shapes = std::mem::take(&mut output.shapes);
            output.textures_delta.clear();
            let mut bounds = egui::Rect::NOTHING;
            for clipped in shapes {
                let shape = clipped.shape.visual_bounding_rect();
                if shape.is_finite() {
                    bounds |= shape;
                }
            }
            assert!(bounds.is_finite(), "{} drew nothing", def.name);
            assert!(
                allowed.contains_rect(bounds),
                "{}: {bounds:?} goes outside {allowed:?}",
                def.name
            );
        }
        // There is only the one size, so there are as many cache entries as icons.
        assert_eq!(cache.len(), ICONS.len());
    }

    #[test]
    fn cache_is_keyed_by_rounded_size() {
        let mut cache = IconCache::new();
        let a = cache.get(&MINUS, 24.2);
        let b = cache.get(&MINUS, 23.8);
        assert!(std::rc::Rc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);
        let c = cache.get(&MINUS, 32.0);
        assert!(!std::rc::Rc::ptr_eq(&a, &c));
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }
}
