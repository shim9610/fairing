//! Icon painting: design-grid conversion, curve flattening and a per-size polyline cache.
//! Despite legacy `*_px` names, sizes and output coordinates used by [`paint`] are
//! egui points (du); the host applies physical pixel scaling later.
//!
//! # The per-frame cost
//!
//! [`IconCache`] stores flattened curves by path and rounded point size; a cache
//! miss builds the path again. Drawing translates cached points into the target
//! `Rect`. A two-point subpath uses [`egui::Shape::LineSegment`] without creating
//! a point vector. With three or more points,
//! `epaint` wants an owned `Vec<Pos2>` (`PathShape`), so one `Vec` appears per subpath — that is
//! the representation the icon design specifies, and it handles round joins and feathering in a
//! single stroke so seams do not overlap and darken. Drawing it as separate pieces would remove
//! the point-vector allocation but may overlap translucent pieces at their joints.

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
use std::collections::TryReserveError;

/// One flattened subpath, with the icon's top-left as the origin.
/// Coordinates follow the supplied size; [`paint`] uses egui points (du).
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    /// The points.
    pub points: Vec<Pos2>,
    /// Whether it was closed with `Z`.
    pub closed: bool,
}

/// One icon flattened at a particular supplied size; [`paint`] uses egui points.
#[derive(Debug, Clone, PartialEq)]
pub struct Flattened {
    /// The subpaths.
    pub polylines: Vec<Polyline>,
    /// Whether it is a filled icon.
    pub fill: bool,
}

/// Choose the curve subdivision count from the size (4–16).
///
/// For a quarter-circle example, four segments at radius 12 px give approximately
/// 0.23 px chord error; twelve segments at radius 48 px give approximately 0.10 px.
/// These examples are not an error bound for arbitrary paths or sizes.
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
    fn flush(&mut self, closed: bool) -> Result<(), TryReserveError> {
        if self.current.is_empty() {
            return Ok(());
        }
        self.polylines.try_reserve(1)?;
        let single = self.current.len() == 1;
        if let Some(point) = self.current.first().copied().filter(|_| single) {
            self.current.try_reserve(1)?;
            self.current.push(point);
        }
        self.polylines.push(Polyline {
            points: std::mem::take(&mut self.current),
            closed: closed && !single,
        });
        Ok(())
    }

    fn resume(&mut self) -> Result<(), TryReserveError> {
        if self.current.is_empty() {
            if let Some(start) = self.pending.take() {
                self.current.try_reserve(1)?;
                self.cursor = start;
                self.current.push(start);
            }
        }
        Ok(())
    }

    fn move_to(&mut self, point: Pos2) -> Result<(), TryReserveError> {
        self.flush(false)?;
        self.current.try_reserve(1)?;
        self.pending = None;
        self.cursor = point;
        self.current.push(point);
        Ok(())
    }

    fn line_to(&mut self, point: Pos2) -> Result<(), TryReserveError> {
        self.resume()?;
        self.current.try_reserve(1)?;
        self.cursor = point;
        self.current.push(point);
        Ok(())
    }

    fn close(&mut self) -> Result<(), TryReserveError> {
        let start = self.current.first().copied();
        self.flush(true)?;
        self.pending = start;
        if let Some(start) = start {
            self.cursor = start;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<Polyline>, TryReserveError> {
        self.flush(false)?;
        Ok(self.polylines)
    }
}

/// Flatten 24-grid segments into local pixel coordinates using size-based curve
/// subdivision. Successful subdivision is deterministic for the supplied definition
/// and size. Allocation failure returns an empty path; use [`try_flatten`] to
/// distinguish that omission from an empty definition.
#[must_use]
pub fn flatten(def: &IconDef, size_px: f32) -> Flattened {
    try_flatten(def, size_px).unwrap_or_else(|_| Flattened {
        polylines: Vec::new(),
        fill: def.fill,
    })
}

/// Flatten using fallible storage throughout. No partially built path is returned.
///
/// # Errors
/// The path's point/subpath buffers cannot grow. All temporary storage is released.
pub fn try_flatten(def: &IconDef, size_px: f32) -> Result<Flattened, TryReserveError> {
    let scale = size_px / 24.0;
    let n = subdivisions(size_px);
    let map = |x: f32, y: f32| Pos2::new(x * scale, y * scale);
    let mut build = Builder::new();
    for seg in def.segs {
        match *seg {
            Seg::M(x, y) => build.move_to(map(x, y))?,
            Seg::L(x, y) => build.line_to(map(x, y))?,
            Seg::Q(cx, cy, x, y) => {
                build.resume()?;
                build.current.try_reserve(n)?;
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
                build.resume()?;
                build.current.try_reserve(n)?;
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
            Seg::Z => build.close()?,
        }
    }
    Ok(Flattened {
        polylines: build.finish()?,
        fill: def.fill,
    })
}

/// A cache lookup could not produce an icon. No partial icon is returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconCacheError {
    /// Capacity zero disables cached icon display.
    Disabled,
    /// Storage still could not grow after reclaiming older cached paths.
    AllocationFailed,
}
impl std::fmt::Display for IconCacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Disabled => "icon cache disabled",
            Self::AllocationFailed => "icon storage unavailable",
        })
    }
}
impl std::error::Error for IconCacheError {}

#[derive(Debug)]
struct CacheEntry {
    key: (usize, usize, bool, u32),
    flat: Flattened,
}

/// Bounded per-path/size cache; default 128 entries, oldest-created paths reclaimed first.
/// Keys include segment identity, segment count, fill and rounded size (not the icon name).
/// Entries own their paths directly; no infallible reference-count allocation is needed.
#[derive(Debug)]
pub struct IconCache {
    entries: Vec<CacheEntry>,
    capacity: usize,
    status: crate::memory::DisplayStatus,
}

impl Default for IconCache {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            capacity: 128,
            status: crate::memory::DisplayStatus::default(),
        }
    }
}

impl IconCache {
    /// An empty cache with at most 128 entries.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the entry limit, dropping oldest entries if necessary. Zero disables icon display.
    /// This does not allocate and is an entry limit, not a process or renderer byte budget.
    #[must_use]
    pub fn with_capacity_limit(mut self, capacity: usize) -> Self {
        self.capacity = capacity;
        let remove = self.entries.len().saturating_sub(capacity);
        self.entries.drain(..remove);
        self
    }

    /// Borrow a cached path, or omit it if storage is unavailable. Observe [`Self::display_status`]
    /// or use [`Self::try_get`] for the reason. The borrow ends before the next cache mutation.
    pub fn get(&mut self, def: &IconDef, size_px: f32) -> Option<&Flattened> {
        self.try_get(def, size_px).ok()
    }

    /// Borrow or build a complete cached path with fallible allocations. A cache hit does not allocate.
    /// On failure, reclaim oldest cached paths one at a time and retry; never spin without reclamation.
    ///
    /// # Errors
    /// Capacity is zero, or allocation still fails after all old paths have been reclaimed.
    pub fn try_get(&mut self, def: &IconDef, size_px: f32) -> Result<&Flattened, IconCacheError> {
        if self.capacity == 0 {
            self.status.discarded = self.status.discarded.saturating_add(1);
            return Err(IconCacheError::Disabled);
        }
        let px = size_px.round().max(1.0) as u32;
        let key = (def.segs.as_ptr().addr(), def.segs.len(), def.fill, px);
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            return self
                .entries
                .get(index)
                .map(|entry| &entry.flat)
                .ok_or(IconCacheError::AllocationFailed);
        }
        if self.entries.len() >= self.capacity {
            self.entries.remove(0);
        }
        loop {
            let built = self
                .entries
                .try_reserve_exact(1)
                .and_then(|()| try_flatten(def, px as f32));
            if let Ok(flat) = built {
                self.entries.push(CacheEntry { key, flat });
                return self
                    .entries
                    .last()
                    .map(|entry| &entry.flat)
                    .ok_or(IconCacheError::AllocationFailed);
            }
            self.status.allocation_failed = true;
            if self.entries.is_empty() {
                self.status.discarded = self.status.discarded.saturating_add(1);
                return Err(IconCacheError::AllocationFailed);
            }
            self.entries.remove(0);
        }
    }

    /// Omission/pressure observations without allocation.
    #[must_use]
    pub const fn display_status(&self) -> crate::memory::DisplayStatus {
        self.status
    }

    /// Take and acknowledge pressure observations.
    pub fn take_display_status(&mut self) -> crate::memory::DisplayStatus {
        std::mem::take(&mut self.status)
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

    /// Release paths and cache metadata. Does not clear unacknowledged pressure observations.
    pub fn clear(&mut self) {
        self.entries = Vec::new();
    }
}

/// A cached subpath's points translated to the target position. The one per-frame allocation,
/// and it exists because `epaint`'s `PathShape` wants an owned `Vec`.
fn translated(line: &Polyline, offset: Vec2) -> Option<Vec<Pos2>> {
    let mut points = Vec::new();
    points.try_reserve_exact(line.points.len()).ok()?;
    points.extend(line.points.iter().map(|point| *point + offset));
    Some(points)
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
    let Some(flat) = cache.get(def, size) else {
        return;
    };
    let offset = (rect.center() - Vec2::splat(size / 2.0)).to_vec2();
    let cap = stroke_px / 2.0;
    let mut omitted = 0usize;
    for line in &flat.polylines {
        if flat.fill {
            if line.points.len() >= 3 {
                let Some(points) = translated(line, offset) else {
                    omitted = omitted.saturating_add(1);
                    continue;
                };
                painter.add(Shape::Path(PathShape::convex_polygon(
                    points,
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
                let Some(points) = translated(line, offset) else {
                    omitted = omitted.saturating_add(1);
                    continue;
                };
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
    if omitted != 0 {
        cache.status.allocation_failed = true;
        cache.status.discarded = cache.status.discarded.saturating_add(omitted);
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
    fn retention_limit_reclaims_oldest_created_path_and_zero_omits() {
        let mut cache = IconCache::new().with_capacity_limit(2);
        assert!(cache.try_get(&MINUS, 24.0).is_ok());
        assert!(cache.try_get(&MINUS, 32.0).is_ok());
        assert!(cache.try_get(&MINUS, 24.0).is_ok()); // Hits do not allocate or reorder.
        assert!(cache.try_get(&MINUS, 48.0).is_ok());
        assert_eq!(cache.len(), 2);
        let sizes: Vec<_> = cache.entries.iter().map(|entry| entry.key.3).collect();
        assert_eq!(sizes, [32, 48]);
        assert!(!cache.display_status().is_reduced()); // Cache eviction did not omit this icon.
        let mut cache = cache.with_capacity_limit(0);
        assert!(cache.is_empty());
        assert_eq!(
            cache.try_get(&MINUS, 24.0),
            Err(super::IconCacheError::Disabled)
        );
        assert_eq!(cache.take_display_status().discarded, 1);
        assert!(!cache.display_status().is_reduced());
    }

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

    /// Draw the built-in set through `paint` and check bounds plus half a stroke.
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

        // **The band, and why it is two numbers and not one.** Measured over the 78 icons shipped
        // then (`eye` and `eye-off`, added since, are 19.9 and 20.0 on their longest side, inside
        // it), the longest side has a median of 20.0 on the 24 grid and an interquartile range of
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
        // There is only the one size, so there is at most one entry per icon (two icons with the
        // same path share one), and drawing them all again adds none.
        let filled = cache.len();
        assert!(filled > 0 && filled <= ICONS.len());
        for def in ICONS {
            let _ = cache.get(def, 32.0);
        }
        assert_eq!(cache.len(), filled);
    }

    #[test]
    fn cache_is_keyed_by_rounded_size() {
        let mut cache = IconCache::new();
        let a = cache.get(&MINUS, 24.2).map(std::ptr::from_ref);
        let b = cache.get(&MINUS, 23.8).map(std::ptr::from_ref);
        assert!(a.is_some());
        assert_eq!(a, b);
        assert_eq!(cache.len(), 1);
        let c = cache.get(&MINUS, 32.0).map(std::ptr::from_ref);
        assert!(c.is_some());
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }
}
