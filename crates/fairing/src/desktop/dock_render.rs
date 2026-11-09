//! Dock render.

use super::{
    draw_slot, soft_band, Axis, Color32, ColorRole, DesktopAction, DesktopConfig, DesktopCtx, Dock,
    DockPlacement, IconColor, IconSlot, IconStyle, PageSwipe, Rect, Sense, SlotPaint,
    DOCK_BAND_TINT, RAIL_RADIUS,
};

/// The dock (fixed, with no cap on the count). While it is empty the band is not drawn either.
pub(super) fn draw_dock(
    ui: &mut egui::Ui,
    dock: &Dock,
    rect: Rect,
    cell: f32,
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) -> Option<DesktopAction> {
    if dock.is_empty() {
        return None;
    }
    let metrics = cx.theme.metrics;
    let dock_rect = dock.rect_in(rect, &metrics);
    let count = dock.slots.len();
    #[expect(clippy::cast_precision_loss, reason = "the cell count is small")]
    let n = count as f32;
    let vertical = dock.is_vertical();
    // Divide available dock length between entries, capped by the preferred cell
    // size. Crowded docks can make the assigned slots smaller than touch_target.
    let span = if vertical {
        dock_rect.height()
    } else {
        dock_rect.width()
    };
    let step = (span / n).min(cell.max(1.0));
    let start = if vertical {
        dock_rect.center().y - step * n / 2.0
    } else {
        dock_rect.center().x - step * n / 2.0
    };
    let surface = cx
        .theme
        .color(ColorRole::Surface)
        .gamma_multiply(DOCK_BAND_TINT);
    match dock.placement() {
        // A dock against an edge is a band running right up to the edge — the screen's border is the
        // plate's border. Its **inner corners are softened**, though: painted as a flat rectangle it
        // becomes a knife line across the screen over a procedural background or a photograph and cuts
        // the water column dead (which is exactly what happened).
        DockPlacement::Edge(edge) => {
            soft_band(ui.painter(), dock_rect, edge, surface);
        }
        // A crossing row is **a floating plate**. It is widened only as far as the row of icons and its
        // corners rounded, so it reads as an object laid on the background rather than a horizontal line
        // cutting it.
        DockPlacement::Band { .. } => {
            let pad = metrics.corner_radius;
            let run = if vertical {
                Rect::from_min_max(
                    egui::pos2(dock_rect.min.x, start),
                    egui::pos2(dock_rect.max.x, start + step * n),
                )
            } else {
                Rect::from_min_max(
                    egui::pos2(start, dock_rect.min.y),
                    egui::pos2(start + step * n, dock_rect.max.y),
                )
            };
            let plate = run.expand2(egui::vec2(if vertical { 0.0 } else { pad }, pad * 0.5));
            // Rounded to a stadium, the plate reads as an oval smudge. It is rounded to twice the theme's
            // radius at most, and stops at half the short side.
            let radius = (pad * 2.0).min(plate.height().min(plate.width()) * 0.5);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "the corner radius is a small positive pixel count"
            )]
            let radius = egui::CornerRadius::same(radius.clamp(0.0, 255.0) as u8);
            ui.painter().rect_filled(plate, radius, surface);
            ui.painter().rect_stroke(
                plate,
                radius,
                egui::Stroke::new(
                    cx.theme.control.stroke_hairline,
                    cx.theme.color(ColorRole::Outline),
                ),
                egui::StrokeKind::Inside,
            );
        }
    }
    let mut action = None;
    for (i, slot) in dock.slots.iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "the cell count is small")]
        let offset = start + step * i as f32;
        let cell_rect = if vertical {
            Rect::from_min_size(
                egui::pos2(dock_rect.min.x, offset),
                egui::vec2(dock_rect.width(), step),
            )
        } else {
            Rect::from_min_size(
                egui::pos2(offset, dock_rect.min.y),
                egui::vec2(step, dock_rect.height()),
            )
        };
        if let Some(a) = draw_slot(ui, cell_rect, slot, true, cx, paint) {
            action = Some(a);
        }
    }
    action
}
/// The page indicator. A tap goes to that page. The dots **cross-fade** by A4's weight
/// `1 − |i − pos|` — the radius `DOT_R_OFF → DOT_R_ON`, the colour `Muted → OnSurface`. Mid-swipe two
/// dots are half lit at once. It hands back the page pressed.
pub(super) fn draw_indicator(
    ui: &mut egui::Ui,
    rect: Rect,
    dock: &Dock,
    count: usize,
    swipe: &PageSwipe,
    cx: &DesktopCtx<'_>,
    rects: &mut Vec<Rect>,
) -> Option<usize> {
    rects.clear();
    if count <= 1 {
        return None;
    }
    rects.resize(count, Rect::NOTHING);
    let metrics = cx.theme.metrics;
    // The indicator sits at the bottom of the screen. With a bottom-band dock it goes above it — a dock
    // anywhere else (the top, the sides, a crossing row) does not cover the bottom, so it does not push it.
    let dock_h = if matches!(
        dock.placement(),
        DockPlacement::Edge(crate::gesture::Edge::Bottom)
    ) {
        dock.band_thickness(&metrics)
    } else {
        0.0
    };
    let y = rect.max.y - dock_h - metrics.page_indicator_height / 2.0;
    let hit_height = metrics
        .page_indicator_height
        .max(metrics.touch_target * metrics.page_indicator_hit_ratio);
    let mut goto = None;
    for i in 0..count {
        let step = metrics.page_indicator_step;
        let x = rect.center().x + (i as f32 - (count as f32 - 1.0) / 2.0) * step;
        let center = egui::pos2(x, y);
        let hit = Rect::from_center_size(center, egui::vec2(step, hit_height));
        if let Some(slot) = rects.get_mut(i) {
            *slot = hit;
        }
        let response = ui.interact(
            hit,
            egui::Id::unique(("fairing.desktop.page", i)),
            Sense::click(),
        );
        if response.clicked() {
            goto = Some(i);
        }
        let weight = swipe.dot_weight(i);
        let color = cx
            .theme
            .color(ColorRole::Muted)
            .lerp_to_gamma(cx.theme.color(ColorRole::OnSurface), weight);
        let (off, on) = (
            metrics.page_indicator_dot_off,
            metrics.page_indicator_dot_on,
        );
        let radius = (on - off).mul_add(weight, off);
        ui.painter().circle_filled(center, radius, color);
    }
    goto
}
/// `[desktop] dock_edge` · `dock_band` → [`DockPlacement`].
pub(super) fn dock_placement(cfg: &DesktopConfig) -> DockPlacement {
    // `ShellConfig::validate` refuses a band that is not a number; one that got past it anyway is
    // the edge's dock, rather than a dock drawn nowhere.
    if let Some(at) = cfg.dock_band.filter(|at| at.is_finite()) {
        // A crossing row's direction is settled by the edge name — `left` / `right` makes it vertical.
        let axis = match cfg.dock_edge.as_str() {
            "left" | "right" => Axis::Vertical,
            _ => Axis::Horizontal,
        };
        return DockPlacement::Band { axis, at };
    }
    let edge = match cfg.dock_edge.as_str() {
        "top" => crate::gesture::Edge::Top,
        "left" => crate::gesture::Edge::Left,
        "right" => crate::gesture::Edge::Right,
        "bottom" => crate::gesture::Edge::Bottom,
        other => {
            log::warn!("[desktop] dock_edge = \"{other}\" is not a known edge - using bottom");
            crate::gesture::Edge::Bottom
        }
    };
    DockPlacement::Edge(edge)
}
/// One rail entry — the icon above, the label below.
///
/// Putting the label **below** the icon is the same convention as the grid. Beside it, a two-line
/// label is cut off whole on a narrow rail.
pub(super) fn paint_rail_item(
    ui: &egui::Ui,
    item: Rect,
    slot: &IconSlot,
    allowed: bool,
    is_open: bool,
    cx: &mut DesktopCtx<'_>,
) {
    let m = cx.theme.metrics;
    let icon_size = m.icon_size * 0.72;
    let label_size = m.desktop_label_size;
    let icon_rect = Rect::from_center_size(
        egui::pos2(item.center().x, item.top() + icon_size * 0.72),
        egui::Vec2::splat(icon_size),
    );
    let color = if !allowed {
        ColorRole::Muted
    } else if is_open {
        ColorRole::Primary
    } else {
        ColorRole::OnSurface
    };
    let style = IconStyle {
        stroke: Some(m.desktop_icon_stroke),
        ..IconStyle::sized(icon_size)
            .enabled(allowed)
            .color(IconColor::Role(color))
    };
    cx.icons
        .paint(ui.painter(), icon_rect, &slot.icon, &style, cx.theme);
    let job = egui::text::LayoutJob::simple(
        cx.strings.get(&slot.label).to_owned(),
        egui::FontId::proportional(label_size),
        Color32::PLACEHOLDER,
        item.width(),
    );
    let galley = ui.painter().layout_job(egui::text::LayoutJob {
        halign: egui::Align::Center,
        wrap: egui::text::TextWrapping {
            max_rows: 1,
            overflow_character: Some('…'),
            ..job.wrap.clone()
        },
        ..job
    });
    ui.painter().galley(
        egui::pos2(item.center().x, icon_rect.max.y + m.desktop_label_gap),
        galley,
        cx.theme.color(color),
    );
}
/// A rail entry's corner radius (`CornerRadius` is a `u8`).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "it is after clamp(0,255), so it is inside u8's range"
)]
pub(super) fn rail_radius(corner: f32) -> u8 {
    (corner * RAIL_RADIUS).round().clamp(0.0, 255.0) as u8
}
