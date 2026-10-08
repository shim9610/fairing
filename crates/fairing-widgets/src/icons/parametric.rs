//! The parametric icons — drawing functions that take their state as an argument.
//!
//! Wi-Fi strength, battery level, volume, Bluetooth, signal and the progress ring. Drawn in code
//! rather than as static SVGs. Every function draws on the 24-grid into **the largest square
//! inside `rect`** and stays within that square, stroke width included (the test
//! `parametric_shapes_stay_inside_the_rect`). The one exception is [`battery`] — a battery is a
//! wide shape by nature, so it stretches the 24-grid horizontally and vertically and uses the
//! whole `rect` (filling the space the status bar allots it at `size × 1.4`).
//!
//! # The crossfade (A7)
//!
//! "When the Wi-Fi strength or the battery step changes, crossfade the previous and next icons
//! over 120 ms" is done by **the caller**. These functions remember no state, so the shell
//! animates a progress `u ∈ [0, 1]` over [`CROSSFADE`] and draws twice into the same `rect`:
//!
//! ```no_run
//! # use fairing_widgets::icons::parametric::{self, ParamStyle};
//! # fn draw(painter: &egui::Painter, rect: egui::Rect, style: ParamStyle, prev: u8, next: u8, u: f32) {
//! parametric::wifi(painter, rect, prev, false, &style.with_alpha(1.0 - u));
//! parametric::wifi(painter, rect, next, false, &style.with_alpha(u));
//! # }
//! ```
//!
//! # The per-frame cost
//!
//! Arcs accumulate their points in a stack buffer and go out as [`egui::Shape::LineSegment`]s,
//! so there is no heap allocation. The same holds for fills, rectangles and circles. `epaint`'s
//! `PathShape`, which wants an owned `Vec`, is used only where round joins are needed: the rune
//! in [`bluetooth`] (6 points) and the cone in [`volume`] (4 points).

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the rest of clippy's `pedantic` stays).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use egui::epaint::{PathShape, PathStroke};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, TAU};
use std::time::Duration;

/// How long the previous and next icons are drawn over each other on a state change (A7, "parametric status icons").
pub const CROSSFADE: Duration = Duration::from_millis(120);

/// The Bluetooth state (`bluetooth(state)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BtIconState {
    /// Off.
    Off,
    /// On, not connected.
    On,
    /// Connected (with two dots).
    Connected,
}

/// The arguments the parametric icons share.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamStyle {
    /// The main colour.
    pub color: Color32,
    /// The dim colour (empty arcs, the empty remainder).
    pub muted: Color32,
    /// The danger colour (a battery at 20 % or below).
    pub danger: Color32,
    /// The stroke in pixels.
    pub stroke_px: f32,
}

impl ParamStyle {
    /// A copy with the same alpha multiplied into all three colours. The caller's handle for the A7 crossfade.
    #[must_use]
    pub fn with_alpha(mut self, alpha: f32) -> Self {
        let alpha = alpha.clamp(0.0, 1.0);
        self.color = self.color.gamma_multiply(alpha);
        self.muted = self.muted.gamma_multiply(alpha);
        self.danger = self.danger.gamma_multiply(alpha);
        self
    }
}

/// The transform taking the 24-grid to the largest square inside the target rect.
#[derive(Debug, Clone, Copy)]
struct Grid {
    origin: Pos2,
    unit: f32,
}

impl Grid {
    fn new(rect: Rect) -> Self {
        let side = rect.width().min(rect.height()).max(0.0);
        Self {
            origin: rect.center() - Vec2::splat(side / 2.0),
            unit: side / 24.0,
        }
    }

    /// Grid coordinates → screen coordinates.
    fn at(self, x: f32, y: f32) -> Pos2 {
        self.origin + Vec2::new(x * self.unit, y * self.unit)
    }

    /// A grid length → a pixel length.
    fn len(self, value: f32) -> f32 {
        value * self.unit
    }

    /// The rect two grid coordinates make.
    fn rect(self, min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Rect {
        Rect::from_min_max(self.at(min_x, min_y), self.at(max_x, max_y))
    }

    fn is_empty(self) -> bool {
        self.unit <= 0.0
    }
}

/// The maximum subdivision count for an arc polyline. It fixes the stack buffer's size (zero heap allocations per frame).
const ARC_MAX: usize = 64;

/// Draw an arc as a chain of segments plus round caps at each end. No heap allocation.
///
/// Adjacent segments share an endpoint and the angle between them is small (about 2 px per
/// subdivision), so the gap on the outside is `stroke/2 · tan(θ/2)` — under 0.1 px. No joint
/// circles are stamped, so a colour with alpha does not double-composite.
fn arc(painter: &Painter, center: Pos2, radius: f32, start: f32, sweep: f32, stroke: Stroke) {
    if radius <= 0.0 || stroke.width <= 0.0 || stroke.color == Color32::TRANSPARENT {
        return;
    }
    let steps = ((sweep.abs() * radius / 2.0).ceil() as usize).clamp(4, ARC_MAX);
    let mut buffer = [Pos2::ZERO; ARC_MAX + 1];
    for (index, slot) in buffer.iter_mut().take(steps + 1).enumerate() {
        let angle = sweep.mul_add(index as f32 / steps as f32, start);
        *slot = center + Vec2::angled(angle) * radius;
    }
    let Some(points) = buffer.get(..=steps) else {
        return;
    };
    for pair in points.windows(2) {
        if let [from, to] = pair {
            painter.line_segment([*from, *to], stroke);
        }
    }
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        painter.circle_filled(*first, stroke.width / 2.0, stroke.color);
        painter.circle_filled(*last, stroke.width / 2.0, stroke.color);
    }
}

/// Wi-Fi strength `level: 0..=4`. 0 is not connected (four empty arcs), and `off` mutes
/// everything and adds a slash.
///
/// Four concentric arcs spread 45° each about (12, 19). Only the
/// arcs with `i < level` take the main colour; the rest are [`ParamStyle::muted`].
pub fn wifi(painter: &Painter, rect: Rect, level: u8, off: bool, style: &ParamStyle) {
    let grid = Grid::new(rect);
    if grid.is_empty() {
        return;
    }
    let level = level.min(4);
    let center = grid.at(12.0, 19.0);
    for index in 0..4u8 {
        let radius = grid.len(3.5 + 3.5 * f32::from(index));
        let color = if !off && index < level {
            style.color
        } else {
            style.muted
        };
        arc(
            painter,
            center,
            radius,
            -FRAC_PI_2 - FRAC_PI_4,
            FRAC_PI_2,
            Stroke::new(style.stroke_px, color),
        );
    }
    if off {
        slash(painter, grid, style.color, style.stroke_px);
    }
}

/// The top-left to bottom-right slash (the "off" mark).
fn slash(painter: &Painter, grid: Grid, color: Color32, stroke_px: f32) {
    let (from, to) = (grid.at(4.5, 4.5), grid.at(19.5, 19.5));
    painter.line_segment([from, to], Stroke::new(stroke_px, color));
    painter.circle_filled(from, stroke_px / 2.0, color);
    painter.circle_filled(to, stroke_px / 2.0, color);
}

/// Battery level `percent: 0..=100`, with a bolt while charging. At 20 % or below it is the
/// `danger` colour.
///
/// Unlike the other parametric icons, it stretches the 24-grid horizontally and vertically
/// **separately** and uses the whole `rect`. A battery is a wide shape and the status bar allots
/// it that much room. The outline is an inside stroke so it does not exceed the
/// body rectangle, and the terminal attaches on the right.
pub fn battery(painter: &Painter, rect: Rect, percent: u8, charging: bool, style: &ParamStyle) {
    let unit_x = rect.width() / 24.0;
    let unit_y = rect.height() / 24.0;
    if unit_x <= 0.0 || unit_y <= 0.0 {
        return;
    }
    let at =
        |x: f32, y: f32| Pos2::new(x.mul_add(unit_x, rect.min.x), y.mul_add(unit_y, rect.min.y));
    let percent = percent.min(100);
    let body = Rect::from_min_max(at(2.0, 7.0), at(18.0, 17.0));
    painter.rect_stroke(
        body,
        unit_y * 2.0,
        Stroke::new(style.stroke_px, style.color),
        StrokeKind::Inside,
    );
    painter.rect_filled(
        Rect::from_min_max(at(18.8, 9.8), at(21.4, 14.2)),
        unit_y,
        style.color,
    );

    let inner = body.shrink(style.stroke_px + unit_y * 0.6);
    if inner.width() > 0.0 && inner.height() > 0.0 && percent > 0 {
        let fill = if percent <= 20 && !charging {
            style.danger
        } else {
            style.color
        };
        painter.rect_filled(
            Rect::from_min_size(
                inner.min,
                Vec2::new(inner.width() * f32::from(percent) / 100.0, inner.height()),
            ),
            unit_y * 0.75,
            fill,
        );
    }
    if charging {
        bolt(painter, body, style.muted);
    }
}

/// The charging bolt. Placed in the centre, scaled uniformly to the body's height, and drawn as
/// two convex triangles (no concave polygon fills).
fn bolt(painter: &Painter, body: Rect, color: Color32) {
    // The body is 10 units high on the 24-grid.
    let unit = body.height() / 10.0;
    let center = body.center();
    let at = |x: f32, y: f32| center + Vec2::new(x * unit, y * unit);
    let upper = vec![at(1.2, -4.2), at(-2.4, 0.6), at(0.2, 0.6)];
    let lower = vec![at(-1.2, 4.2), at(2.4, -0.6), at(-0.2, -0.6)];
    painter.add(Shape::Path(PathShape::convex_polygon(
        upper,
        color,
        Stroke::NONE,
    )));
    painter.add(Shape::Path(PathShape::convex_polygon(
        lower,
        color,
        Stroke::NONE,
    )));
}

/// Volume level `level: 0..=3`, with an X on the right when `muted`.
pub fn volume(painter: &Painter, rect: Rect, level: u8, muted: bool, style: &ParamStyle) {
    let grid = Grid::new(rect);
    if grid.is_empty() {
        return;
    }
    let level = level.min(3);
    // The speaker: a box (a rectangle) plus a cone (a convex quadrilateral). They overlap by 0.6 of a grid unit to hide the seam.
    painter.rect_filled(grid.rect(4.0, 9.5, 8.6, 14.5), grid.len(0.5), style.color);
    painter.add(Shape::Path(PathShape::convex_polygon(
        vec![
            grid.at(8.0, 9.5),
            grid.at(12.5, 5.0),
            grid.at(12.5, 19.0),
            grid.at(8.0, 14.5),
        ],
        style.color,
        Stroke::NONE,
    )));
    if muted {
        let stroke = Stroke::new(style.stroke_px, style.color);
        cross(painter, grid.at(16.0, 9.0), grid.at(21.0, 14.0), stroke);
        cross(painter, grid.at(21.0, 9.0), grid.at(16.0, 14.0), stroke);
        return;
    }
    let center = grid.at(12.5, 12.0);
    // Arcs spread 52° each way. A radius of 9.5 grid units is the right-hand end (22 plus half the stroke).
    let half = 0.907_571_2_f32;
    for index in 0..3u8 {
        if index >= level {
            break;
        }
        arc(
            painter,
            center,
            grid.len(3.5 + 3.0 * f32::from(index)),
            -half,
            half * 2.0,
            Stroke::new(style.stroke_px, style.color),
        );
    }
}

/// One line segment with round caps.
fn cross(painter: &Painter, from: Pos2, to: Pos2, stroke: Stroke) {
    painter.line_segment([from, to], stroke);
    painter.circle_filled(from, stroke.width / 2.0, stroke.color);
    painter.circle_filled(to, stroke.width / 2.0, stroke.color);
}

/// The Bluetooth state. `Connected` puts two dots either side of the rune.
pub fn bluetooth(painter: &Painter, rect: Rect, state: BtIconState, style: &ParamStyle) {
    let grid = Grid::new(rect);
    if grid.is_empty() {
        return;
    }
    let color = match state {
        BtIconState::Off => style.muted,
        BtIconState::On | BtIconState::Connected => style.color,
    };
    // The rune's bends are sharp and need round joins — one `PathShape` (6 points).
    let points = vec![
        grid.at(7.0, 7.0),
        grid.at(17.0, 17.0),
        grid.at(12.0, 22.0),
        grid.at(12.0, 2.0),
        grid.at(17.0, 7.0),
        grid.at(7.0, 17.0),
    ];
    if let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) {
        painter.circle_filled(first, style.stroke_px / 2.0, color);
        painter.circle_filled(last, style.stroke_px / 2.0, color);
    }
    painter.add(Shape::Path(PathShape::line(
        points,
        PathStroke::new(style.stroke_px, color),
    )));
    if state == BtIconState::Connected {
        let radius = grid.len(1.4);
        painter.circle_filled(grid.at(3.6, 12.0), radius, color);
        painter.circle_filled(grid.at(20.4, 12.0), radius, color);
    }
}

/// Signal bars `bars: 0..=4`. Filled bars take the main colour and the rest [`ParamStyle::muted`].
pub fn signal(painter: &Painter, rect: Rect, bars: u8, style: &ParamStyle) {
    let grid = Grid::new(rect);
    if grid.is_empty() {
        return;
    }
    let bars = bars.min(4);
    for index in 0..4u8 {
        let left = 2.5 + 5.0 * f32::from(index);
        let top = 15.5 - 4.0 * f32::from(index);
        let color = if index < bars {
            style.color
        } else {
            style.muted
        };
        painter.rect_filled(grid.rect(left, top, left + 4.0, 20.0), grid.len(1.0), color);
    }
}

/// A progress ring, `t ∈ [0, 1]`. Clockwise from 12 o'clock.
pub fn progress_ring(painter: &Painter, rect: Rect, t: f32, style: &ParamStyle) {
    let side = rect.width().min(rect.height());
    if side <= 0.0 || style.stroke_px <= 0.0 {
        return;
    }
    let t = t.clamp(0.0, 1.0);
    let radius = (side - style.stroke_px) / 2.0;
    if radius <= 0.0 {
        return;
    }
    let center = rect.center();
    painter.circle_stroke(center, radius, Stroke::new(style.stroke_px, style.muted));
    if t > 0.0 {
        arc(
            painter,
            center,
            radius,
            -FRAC_PI_2,
            TAU * t,
            Stroke::new(style.stroke_px, style.color),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{battery, bluetooth, progress_ring, signal, volume, wifi, BtIconState, ParamStyle};
    use egui::{pos2, Color32, Rect, Vec2};

    fn style() -> ParamStyle {
        ParamStyle {
            color: Color32::WHITE,
            muted: Color32::GRAY,
            danger: Color32::RED,
            stroke_px: 2.0,
        }
    }

    /// Draw once and return the shapes. The `TexturesDelta` is discarded rather than applied.
    fn painted(rect: Rect, draw: impl FnMut(&egui::Painter, Rect)) -> Vec<egui::Shape> {
        let mut draw = draw;
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), Vec2::new(200.0, 200.0))),
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| draw(ui.painter(), rect));
        let shapes = std::mem::take(&mut output.shapes);
        output.textures_delta.clear();
        shapes.into_iter().map(|clipped| clipped.shape).collect()
    }

    /// The union of the visual bounds of every shape drawn.
    fn painted_bounds(draw: impl FnMut(&egui::Painter, Rect)) -> Rect {
        let mut bounds = Rect::NOTHING;
        for shape in painted(target(), draw) {
            let shape = shape.visual_bounding_rect();
            if shape.is_finite() {
                bounds |= shape;
            }
        }
        bounds
    }

    fn target() -> Rect {
        Rect::from_min_size(pos2(40.0, 30.0), Vec2::splat(24.0))
    }

    fn assert_inside(name: &str, bounds: Rect) {
        let allowed = target().expand(0.01);
        assert!(bounds.is_finite(), "{name}: the bounds are not finite");
        assert!(
            allowed.contains_rect(bounds),
            "{name}: {bounds:?} goes outside {allowed:?}"
        );
    }

    #[test]
    fn parametric_shapes_stay_inside_the_rect() {
        let style = style();
        for level in 0..=4u8 {
            for off in [false, true] {
                assert_inside(
                    "wifi",
                    painted_bounds(|p, r| wifi(p, r, level, off, &style)),
                );
            }
        }
        for percent in [0u8, 5, 20, 55, 100] {
            for charging in [false, true] {
                assert_inside(
                    "battery",
                    painted_bounds(|p, r| battery(p, r, percent, charging, &style)),
                );
            }
        }
        for level in 0..=3u8 {
            for muted in [false, true] {
                assert_inside(
                    "volume",
                    painted_bounds(|p, r| volume(p, r, level, muted, &style)),
                );
            }
        }
        for state in [BtIconState::Off, BtIconState::On, BtIconState::Connected] {
            assert_inside(
                "bluetooth",
                painted_bounds(|p, r| bluetooth(p, r, state, &style)),
            );
        }
        for bars in 0..=4u8 {
            assert_inside("signal", painted_bounds(|p, r| signal(p, r, bars, &style)));
        }
        for t in [0.0f32, 0.25, 0.5, 0.99, 1.0] {
            assert_inside(
                "progress_ring",
                painted_bounds(|p, r| progress_ring(p, r, t, &style)),
            );
        }
    }

    /// The battery fills a wide slot (`size × 1.4`) and stays inside it.
    #[test]
    fn battery_fills_a_wide_rect() {
        let style = style();
        let wide = Rect::from_min_size(pos2(40.0, 30.0), Vec2::new(33.6, 24.0));
        let mut bounds = Rect::NOTHING;
        for shape in painted(wide, |painter, rect| {
            battery(painter, rect, 100, false, &style);
        }) {
            let shape = shape.visual_bounding_rect();
            if shape.is_finite() {
                bounds |= shape;
            }
        }
        assert!(wide.expand(0.01).contains_rect(bounds), "{bounds:?}");
        // It has to spread wider than when drawn in a square (it stretches horizontally).
        assert!(bounds.width() > 24.0, "{bounds:?}");
    }

    /// At 20 % or below it is `danger`, and while charging it is the main colour.
    #[test]
    fn battery_turns_red_below_twenty_percent() {
        let style = style();
        let fills = |percent: u8, charging: bool| -> Vec<Color32> {
            painted(target(), |painter, rect| {
                battery(painter, rect, percent, charging, &style);
            })
            .iter()
            .filter_map(|shape| match shape {
                egui::Shape::Rect(rect) => Some(rect.fill),
                _ => None,
            })
            .collect()
        };
        assert!(fills(15, false).contains(&style.danger));
        assert!(!fills(15, true).contains(&style.danger));
        assert!(!fills(21, false).contains(&style.danger));
        // At 0 % the bar is not drawn at all (only the terminal is left).
        assert!(!fills(0, false).contains(&style.danger));
    }

    #[test]
    fn empty_rects_draw_nothing() {
        let style = style();
        let empty = Rect::from_min_size(pos2(10.0, 10.0), Vec2::ZERO);
        let shapes = painted(empty, |painter, rect| {
            wifi(painter, rect, 3, false, &style);
            battery(painter, rect, 50, true, &style);
            volume(painter, rect, 2, false, &style);
            bluetooth(painter, rect, BtIconState::On, &style);
            signal(painter, rect, 3, &style);
            progress_ring(painter, rect, 0.5, &style);
        });
        let drawn = shapes
            .iter()
            .filter(|shape| shape.visual_bounding_rect().is_positive())
            .count();
        assert_eq!(drawn, 0);
    }

    #[test]
    fn with_alpha_dims_every_role() {
        let dimmed = style().with_alpha(0.5);
        assert!(dimmed.color.a() < Color32::WHITE.a());
        assert!(dimmed.muted.a() < Color32::GRAY.a());
        assert!(dimmed.danger.a() < Color32::RED.a());
        assert_eq!(style().with_alpha(1.0), style());
        assert_eq!(style().with_alpha(2.0), style());
        assert_eq!(style().with_alpha(-1.0).color.a(), 0);
    }

    /// A higher strength lights more arcs — the count of main-colour segments has to increase monotonically.
    #[test]
    fn wifi_level_changes_the_number_of_lit_arcs() {
        let style = style();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(48.0));
        let counts: Vec<usize> = (0..=4u8)
            .map(|level| {
                painted(rect, |painter, rect| {
                    wifi(painter, rect, level, false, &style);
                })
                .iter()
                .filter(|shape| match shape {
                    egui::Shape::LineSegment { stroke, .. } => stroke.color == style.color,
                    _ => false,
                })
                .count()
            })
            .collect();
        for pair in counts.windows(2) {
            if let [lower, higher] = pair {
                assert!(higher > lower, "{counts:?}");
            }
        }
        assert_eq!(counts.first(), Some(&0));
    }

    /// `off` mutes every arc regardless of the strength and adds the slash.
    #[test]
    fn wifi_off_mutes_every_arc_and_adds_a_slash() {
        let style = style();
        let rect = Rect::from_min_size(pos2(0.0, 0.0), Vec2::splat(48.0));
        let shapes = painted(rect, |painter, rect| wifi(painter, rect, 4, true, &style));
        let lit = shapes
            .iter()
            .filter(|shape| match shape {
                egui::Shape::LineSegment { stroke, .. } => stroke.color == style.color,
                _ => false,
            })
            .count();
        // Only the one diagonal is in the main colour.
        assert_eq!(lit, 1);
    }
}
