//! Grid render.

use super::{draw_slot, Axis, Color32, DesktopAction, DesktopCtx, Page, Rect, SlotPaint};

/// The current page's grid. The cells are square and the grid is centred.
pub(super) fn draw_grid(
    ui: &mut egui::Ui,
    page: &Page,
    grid: Rect,
    cell: f32,
    dims: (u8, u8),
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) -> Option<DesktopAction> {
    let (columns, rows) = dims;
    let origin = egui::pos2(
        grid.center().x - cell * f32::from(columns) / 2.0,
        grid.center().y - cell * f32::from(rows) / 2.0,
    );
    let mut action = None;
    for (col, row, slot) in page.slots() {
        let cell_rect = Rect::from_min_size(
            origin + egui::vec2(f32::from(col) * cell, f32::from(row) * cell),
            egui::vec2(cell, cell),
        );
        if let Some(a) = draw_slot(ui, cell_rect, slot, false, cx, paint) {
            action = Some(a);
        }
    }
    action
}
/// Paint a band along an edge, fading only its inner edge.
///
/// The outer 2/3 is solid `color` and the inner 1/3 falls to alpha 0. Eight vertices and four
/// triangles, and whatever the background is (flat, a gradient, procedural, a photo) the boundary
/// does not read as a line.
pub(super) fn soft_band(
    painter: &egui::Painter,
    rect: Rect,
    edge: crate::gesture::Edge,
    color: Color32,
) {
    /// What fraction of the band's thickness the fade takes.
    const FADE: f32 = 0.34;
    let (solid, fade) = match edge {
        crate::gesture::Edge::Bottom => (
            Rect::from_min_max(
                egui::pos2(rect.min.x, rect.min.y + rect.height() * FADE),
                rect.max,
            ),
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x, rect.min.y + rect.height() * FADE),
            ),
        ),
        crate::gesture::Edge::Top => (
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x, rect.max.y - rect.height() * FADE),
            ),
            Rect::from_min_max(
                egui::pos2(rect.min.x, rect.max.y - rect.height() * FADE),
                rect.max,
            ),
        ),
        crate::gesture::Edge::Left => (
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.max.x - rect.width() * FADE, rect.max.y),
            ),
            Rect::from_min_max(
                egui::pos2(rect.max.x - rect.width() * FADE, rect.min.y),
                rect.max,
            ),
        ),
        crate::gesture::Edge::Right => (
            Rect::from_min_max(
                egui::pos2(rect.min.x + rect.width() * FADE, rect.min.y),
                rect.max,
            ),
            Rect::from_min_max(
                rect.min,
                egui::pos2(rect.min.x + rect.width() * FADE, rect.max.y),
            ),
        ),
    };
    painter.rect_filled(solid, 0.0, color);
    // The fade band: transparent on the inner edge, `color` on the outer.
    let vertical = matches!(
        edge,
        crate::gesture::Edge::Left | crate::gesture::Edge::Right
    );
    let inner_first = matches!(
        edge,
        crate::gesture::Edge::Bottom | crate::gesture::Edge::Right
    );
    let (a, b) = if vertical {
        (
            [fade.left_top(), fade.left_bottom()],
            [fade.right_top(), fade.right_bottom()],
        )
    } else {
        (
            [fade.left_top(), fade.right_top()],
            [fade.left_bottom(), fade.right_bottom()],
        )
    };
    let (edge_a, edge_b) = if inner_first {
        (Color32::TRANSPARENT, color)
    } else {
        (color, Color32::TRANSPARENT)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(a[0], edge_a);
    mesh.colored_vertex(a[1], edge_a);
    mesh.colored_vertex(b[0], edge_b);
    mesh.colored_vertex(b[1], edge_b);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
}
/// Hand back **the wider side** left after a crossing row has divided the content.
///
/// Where the row goes outside the content, nothing is cut — it then hides nothing.
pub(super) fn carve_around(content: Rect, band: Rect, axis: Axis) -> Rect {
    let (lo, hi, c_lo, c_hi) = match axis {
        Axis::Horizontal => (band.min.y, band.max.y, content.min.y, content.max.y),
        Axis::Vertical => (band.min.x, band.max.x, content.min.x, content.max.x),
    };
    let before = (lo - c_lo).max(0.0);
    let after = (c_hi - hi).max(0.0);
    if before <= 0.0 && after <= 0.0 {
        return content;
    }
    let (keep_lo, keep_hi) = if before >= after {
        (c_lo, lo.min(c_hi))
    } else {
        (hi.max(c_lo), c_hi)
    };
    match axis {
        Axis::Horizontal => Rect::from_min_max(
            egui::pos2(content.min.x, keep_lo),
            egui::pos2(content.max.x, keep_hi),
        ),
        Axis::Vertical => Rect::from_min_max(
            egui::pos2(keep_lo, content.min.y),
            egui::pos2(keep_hi, content.max.y),
        ),
    }
}
