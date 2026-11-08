//! Tiles render.

use super::{
    builtin, galley, tile_gap, tile_width, Arc, CellPaint, Color32, ColorRole, CxParts, Gauge,
    GaugeRows, IconColor, IconStyle, InstanceId, LaunchAction, PaneInfo, PanelAction, PanelStyle,
    QuickTile, Rect, Sense, SettingValue, ShadeTileCx, ShadeTileLayoutCx, ShadeTilePainter,
    SliderOut, SliderSlot, TextKey, Theme, TileBrush, TileKind, TileRow, TileSpots, TileState,
    TilesOut, Vec2, MOVING_EPS, MOVING_MAX, SLIDER_ICON_GAP, STEPS,
};

/// The progress arc around the puck. `progress` is 0..=1 and grows clockwise from twelve o'clock.
///
/// Drawing the value **inside** the puck overlaps the icon, and drawing it as a thin band along the
/// bottom of the cell collides with the label. The circumference is empty space and fights with
/// neither.
pub(super) fn value_ring(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    progress: f32,
    stroke: egui::Stroke,
) {
    let progress = progress.clamp(0.0, 1.0);
    if progress <= 0.0 {
        return;
    }
    let last = (STEPS as f32 * progress).ceil().max(1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "STEPS is 32"
    )]
    let count = last as usize;
    let mut points = Vec::with_capacity(count + 1);
    for i in 0..=count {
        #[expect(clippy::cast_precision_loss, reason = "i <= 32")]
        let t = (i as f32 / STEPS as f32).min(progress);
        let angle = std::f32::consts::TAU * t - std::f32::consts::FRAC_PI_2;
        points.push(egui::pos2(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }
    painter.add(egui::Shape::Path(egui::epaint::PathShape {
        points,
        closed: false,
        fill: egui::Color32::TRANSPARENT,
        stroke: stroke.into(),
    }));
}
/// One tile's tap → an action (or a Slider expansion toggle). A `Toggle` tile sends **the opposite
/// of the state shown** as `Set(key, Bool)` — for a key the backend owns the truth of (`wifi.enabled`
/// and the like) the settings memory may hold no value at all, so a `LaunchAction::Toggle` (which
/// goes by that memory) would turn on something already on. The gate is the same for both, being the
/// key name.
pub(super) fn tile_tap(
    quick: &QuickTile,
    state: TileState,
    allowed: bool,
    live: bool,
    index: usize,
    expanded: &mut Option<usize>,
) -> Option<PanelAction> {
    if !allowed {
        return Some(PanelAction::TileLocked {
            id: quick.id.clone(),
            gate: quick.gate.clone(),
        });
    }
    if !live {
        return None;
    }
    match &quick.kind {
        TileKind::Toggle(key) => Some(PanelAction::Launch(LaunchAction::Set(
            key.clone(),
            SettingValue::Bool(state != TileState::On),
        ))),
        // A Gauges tile with no rows has nothing to open.
        TileKind::Gauges { rows, .. } if rows.is_empty() => None,
        // The kinds that open an expansion row. They share the same place, so one opening closes another.
        TileKind::Slider(_) | TileKind::Panel { .. } | TileKind::Gauges { .. } => {
            *expanded = if *expanded == Some(index) {
                None
            } else {
                Some(index)
            };
            None
        }
        TileKind::Action(launch) => Some(PanelAction::Launch(launch.clone())),
        TileKind::Status => None,
    }
}
/// **A slider tile's value, as an arc around its puck.**
///
/// The puck no longer fills with the accent, so this ring is the only thing that says 40 % rather
/// than 90 %. It is drawn at `stroke_mark` — the weight of a checkbox's tick — and stands off the
/// puck by `focus_gap`, clear of both the icon inside and the label below.
///
/// **The track is the same path as the value.** It used to be a `circle_stroke`, which epaint
/// tessellates with the stroke *outside* the radius (`PathStroke::outside`, `tessellator.rs`),
/// while `value_ring` is a path and is stroked down its middle — so the two rings sat half a stroke
/// apart and read as a double outline instead of one filling arc. Drawing the track through
/// `value_ring` at 1.0 puts both on one geometry.
pub(super) fn tile_value_arc(
    painter: &egui::Painter,
    theme: &Theme,
    puck: Rect,
    (value, live, fade): (f32, bool, f32),
) {
    let w = theme.control.stroke_mark;
    let r = puck.width() * 0.5 + theme.control.focus_gap + w * 0.5;
    let dim = theme.color(ColorRole::Outline).gamma_multiply(fade);
    value_ring(painter, puck.center(), r, 1.0, egui::Stroke::new(w, dim));
    if live {
        let ink = theme.color(ColorRole::Primary).gamma_multiply(fade);
        value_ring(painter, puck.center(), r, value, egui::Stroke::new(w, ink));
    }
}
/// Whether the tile uses its lit fill.
///
/// Toggles follow their state. Action, status, panel and gauge tiles can use a
/// `QuickTile::lit_by` Boolean setting. Sliders use their value arc and ignore
/// `lit_by`.
pub(super) fn tile_is_filled(tile: &QuickTile, state: TileState) -> bool {
    state == TileState::On
        && match tile.kind {
            TileKind::Toggle(_) => true,
            TileKind::Slider(_) => false,
            TileKind::Action(_)
            | TileKind::Status
            | TileKind::Panel { .. }
            | TileKind::Gauges { .. } => tile.lit_by.is_some(),
        }
}
/// One tile: **the integrator's painter draws it where there is one** (rung 5), the
/// built-in drawing otherwise. The tap, the long press and the row a tile opens stay the shell's,
/// in `draw_tiles`.
#[allow(clippy::too_many_arguments)] // One tile: where, what, its state, and who draws it.
pub(super) fn draw_tile_cell(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    index: usize,
    tile: (&QuickTile, TileState, bool),
    cell: Rect,
    paint: CellPaint,
    brush: &mut TileBrush<'_>,
) {
    if let Some(painter) = brush.painter.as_deref_mut() {
        let label = galley(row.texts, TextKey::TileLabel(index)).map_or("", |g| g.text());
        paint_tile_with(painter, ui, parts, label, tile, cell, paint);
    } else {
        draw_builtin_tile(ui, parts, row, index, tile, cell, (paint, brush.ground));
    }
}
/// Hand one tile to the integrator's painter, whole (rung 5).
pub(super) fn paint_tile_with(
    painter: &mut ShadeTilePainter,
    ui: &egui::Ui,
    parts: &mut CxParts<'_>,
    label: &str,
    (quick, state, allowed): (&QuickTile, TileState, bool),
    cell: Rect,
    paint: CellPaint,
) {
    let mut tile = ShadeTileCx {
        rect: cell,
        id: &quick.id,
        label,
        icon: &quick.icon,
        kind: &quick.kind,
        state,
        lit: tile_is_filled(quick, state),
        live: quick.enabled && state != TileState::Unavailable,
        allowed,
        pressed: paint.pressed,
        fade: paint.fade,
        theme: parts.theme,
        icons: &mut *parts.icons,
    };
    painter(ui.painter(), &mut tile);
}
/// Draw a built-in quick tile as an icon disc with its label below on the panel ground.
pub(super) fn draw_builtin_tile(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    index: usize,
    (quick, state, allowed): (&QuickTile, TileState, bool),
    cell: Rect,
    (paint, ground): (CellPaint, Color32),
) {
    let CellPaint { pressed, fade } = paint;
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let live = quick.enabled && state != TileState::Unavailable;
    let on = tile_is_filled(quick, state);

    let label = galley(row.texts, TextKey::TileLabel(index));
    // **A label on its way out takes no place either.** The height is reduced by `fade` — otherwise, as
    // the tile shrinks into the Slider row, the puck is squeezed into the label's place and the icon
    // disappears (measured).
    let label_h = label.map_or(0.0, |g| g.size().y) * fade;
    let gap = style.gap * 0.5 * fade;
    let puck_d = style
        .puck
        .min(cell.width())
        .min(cell.height() - label_h - gap);
    let content_h = puck_d + gap + label_h;
    let top = cell.center().y - content_h * 0.5;
    let puck = Rect::from_center_size(
        egui::pos2(cell.center().x, top + puck_d * 0.5),
        Vec2::splat(puck_d),
    );

    let mut fill = theme.color(if on {
        ColorRole::Primary
    } else {
        ColorRole::SurfaceVariant
    });
    let mut fg = if on {
        ColorRole::OnPrimary
    } else {
        ColorRole::OnSurface
    };
    if !live {
        fill = fill.gamma_multiply(0.45);
        fg = ColorRole::Muted;
    }
    if pressed {
        fill = fill.gamma_multiply(0.82);
    }
    let painter = ui.painter();
    painter.circle_filled(puck.center(), puck_d * 0.5, fill);
    // An unfilled puck gets one line of border to bound it — so the surfaces do not merge on a dark
    // background. Most tiles are unfilled now, so the line comes off `stroke_hairline` rather than a
    // literal 1.0 and holds at any scale.
    if !on && live {
        // epaint strokes a circle *outside* its radius, so `r` is the inner edge and `r + w` the
        // outer one — the ring is inset by the full width, not half of it.
        let w = theme.control.stroke_hairline;
        painter.circle_stroke(
            puck.center(),
            puck_d * 0.5 - w,
            egui::Stroke::new(w, theme.color(ColorRole::Outline)),
        );
    }

    if let (TileKind::Slider(_), TileState::Value(v)) = (&quick.kind, state) {
        tile_value_arc(painter, theme, puck, (v, live, fade));
    }

    let icon_px = puck_d * 0.46;
    let icon_style = IconStyle {
        color: IconColor::Role(fg),
        ..IconStyle::sized(icon_px)
    };
    parts.icons.paint(
        ui.painter(),
        Rect::from_center_size(puck.center(), Vec2::splat(icon_px)),
        &quick.icon,
        &icon_style,
        theme,
    );

    if let Some(label) = label {
        let size = label.size();
        let color = if live {
            ColorRole::OnSurface
        } else {
            ColorRole::Muted
        };
        ui.painter().with_clip_rect(cell).galley(
            egui::pos2(cell.center().x - size.x * 0.5, puck.max.y + gap),
            Arc::clone(label),
            theme.color(color).gamma_multiply(fade),
        );
    }

    if !allowed {
        let lock = theme.metrics.desktop_lock_size;
        let badge = Rect::from_center_size(
            egui::pos2(puck.max.x - lock * 0.25, puck.min.y + lock * 0.25),
            Vec2::splat(lock),
        );
        // A panel-coloured disc behind the padlock keeps it readable even where it overlaps the puck's border.
        ui.painter()
            .circle_filled(badge.center(), lock * 0.62, ground);
        let lock_style = IconStyle {
            color: IconColor::Role(ColorRole::Warning),
            ..IconStyle::sized(lock)
        };
        parts
            .icons
            .paint(ui.painter(), badge, &builtin::LOCK, &lock_style, theme);
    }
}
/// Where each tile rests, into `spots.homes`, when an integrator's layout places them:
/// the built-in rows at rest, then wherever the layout moves them. It returns where the tile block
/// ends — under the lowest tile placed — or `None` without a layout.
pub(super) fn place_tiles(
    rect: Rect,
    top: f32,
    theme: &Theme,
    row: &TileRow<'_>,
    spots: &mut TileSpots<'_>,
) -> Option<f32> {
    let layout = spots.layout.as_mut()?;
    let cols = row.columns.max(1);
    let tile_px = tile_width(rect.width(), cols, &theme.metrics);
    let count = row.tiles.len();
    let homes = &mut *spots.homes;
    homes.clear();
    for index in 0..count {
        let (line, col) = (index / cols, index % cols);
        let in_line = (count - line * cols).min(cols);
        let gap = tile_gap(&theme.metrics);
        let line_w = in_line as f32 * (tile_px + gap) - gap;
        let x = rect.center().x - line_w / 2.0 + col as f32 * (tile_px + gap);
        let y = top + line as f32 * (tile_px + gap);
        homes.push(Rect::from_min_size(egui::pos2(x, y), Vec2::splat(tile_px)));
    }
    let rows_n = count.div_ceil(cols).max(1);
    let area = Rect::from_min_max(
        egui::pos2(rect.min.x, top),
        egui::pos2(
            rect.max.x,
            top + rows_n as f32 * (tile_px + tile_gap(&theme.metrics)) - tile_gap(&theme.metrics),
        ),
    );
    let expanded = row.expanded.filter(|i| *i < count);
    layout(
        &ShadeTileLayoutCx::new(rect, area, cols, expanded, theme, row.tiles),
        homes,
    );
    Some(
        homes
            .iter()
            .filter(|r| r.is_positive())
            .map(|r| r.max.y)
            .fold(top, f32::max),
    )
}
/// Where tile `index` rests and how big it is there: `in_row`, a `tile_px` square — or, with an
/// integrator's layout, the place it gave the tile in `homes`, and `None` where it emptied it.
pub(super) fn tile_home(
    homes: Option<&[Rect]>,
    index: usize,
    in_row: egui::Pos2,
    tile_px: f32,
) -> Option<(egui::Pos2, Vec2)> {
    let Some(homes) = homes else {
        return Some((in_row, Vec2::splat(tile_px)));
    };
    let place = homes.get(index).copied().filter(Rect::is_positive)?;
    Some((place.center(), place.size()))
}
/// How wide the tiles `range` take in their row, each less by how far it has left the row.
/// The gap after the last cell does not count towards the row's width.
pub(super) fn row_width(
    parts: &mut CxParts<'_>,
    range: std::ops::Range<usize>,
    expanded: Option<usize>,
    tween: crate::motion::Tween,
    tile_px: f32,
) -> f32 {
    let mut total = 0.0;
    for i in range.clone() {
        total += (tile_px + tile_gap(&parts.theme.metrics))
            * (1.0 - tile_out(parts, i, expanded, tween));
    }
    if let Some(last) = range.last() {
        total -= tile_gap(&parts.theme.metrics) * (1.0 - tile_out(parts, last, expanded, tween));
    }
    total
}
/// The tile row (plus the Slider expansion row).
pub(super) fn draw_tiles(
    ui: &mut egui::Ui,
    rect: Rect,
    top: f32,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    spots: &mut TileSpots<'_>,
) -> TilesOut {
    let metrics = parts.theme.metrics;
    let tween = parts.theme.motion.switch;
    let mut action = None;
    let mut expanded = row.expanded.filter(|i| *i < row.tiles.len());
    let cols = row.columns.max(1);
    let gap = tile_gap(&metrics);
    let tile_px = tile_width(rect.width(), cols, &metrics);
    let n = row.tiles.len();
    let rows_n = n.div_ceil(cols).max(1);
    // Where the block of tiles ends: under the last row, or under the lowest tile an integrator's
    // layout placed.
    let placed = place_tiles(rect, top, parts.theme, row, spots);
    let block_bottom = placed.unwrap_or(top + (rows_n as f32 - 1.0) * (tile_px + gap) + tile_px);
    let homes = placed.is_some().then_some(spots.homes.as_slice());
    let tile_rects = &mut *spots.drawn;
    tile_rects.clear();
    // The **arrival point** of a tile on its way out = the Slider expansion row's icon place on the left.
    // The row count being fixed, it is known before the tiles are drawn.
    let icon_d = metrics.touch_target * parts.theme.components.shade.note_icon;
    // The expansion row sits a tile gap under the tiles and two in from the panel's sides.
    let slider_top = block_bottom + gap;
    let slot = egui::pos2(
        rect.min.x + 2.0 * gap + icon_d / 2.0,
        slider_top + metrics.widget_height / 2.0,
    );

    // An expanded tile **leaves the row and the tiles left fill its place.** The progress is held per
    // tile, so pressing another Slider tile has one go up and one come down, swapping places.
    //
    // Measuring the width needs the progress first, and the progress needs `parts` borrowed, so it is
    // walked **twice per row**. `AnimationStore::animate` only moves its value at the per-frame `tick`,
    // so calling it twice in the same frame gives the same value (this file is allocation-free per frame,
    // so an array cannot be used).
    let mut cursor_y = top;
    // A tile in motion is drawn **behind** the Slider row — otherwise the row's ground covers it.
    let mut moving: [(usize, Rect, f32); MOVING_MAX] = [(0, Rect::NOTHING, 0.0); MOVING_MAX];
    let mut moving_n = 0_usize;

    for r in 0..rows_n {
        let first = r * cols;
        let last = ((r + 1) * cols).min(n);
        let total = row_width(parts, first..last, expanded, tween, tile_px);
        let mut cursor_x = rect.center().x - total.max(0.0) / 2.0;
        for (index, (quick, state, allowed)) in row.tiles.iter().enumerate().take(last).skip(first)
        {
            let t = tile_out(parts, index, expanded, tween);
            // Where it rests and how big it is there: the row's, or the layout's — whose
            // tiles do not close up behind one on its way out. A place it emptied leaves the tile out.
            let in_row = egui::pos2(
                cursor_x + tile_px * (1.0 - t) / 2.0,
                cursor_y + tile_px / 2.0,
            );
            let Some((home, home_size)) = tile_home(homes, index, in_row, tile_px) else {
                tile_rects.push(Rect::NOTHING);
                continue;
            };
            let size = home_size + (Vec2::splat(icon_d) - home_size) * t;
            let cell = Rect::from_center_size(home + (slot - home) * t, size);
            cursor_x += (tile_px + gap) * (1.0 - t);
            tile_rects.push(cell);
            let resp = ui.interact(
                cell,
                egui::Id::unique(("fairing.tile", index)),
                Sense::click(),
            );
            let live = quick.enabled && *state != TileState::Unavailable;
            if t > MOVING_EPS && moving_n < MOVING_MAX {
                if let Some(hold) = moving.get_mut(moving_n) {
                    *hold = (index, cell, t);
                }
                moving_n += 1;
            } else {
                let tile = (quick, *state, *allowed);
                let paint = CellPaint {
                    pressed: live && resp.is_pointer_button_down_on(),
                    fade: 1.0,
                };
                draw_tile_cell(ui, parts, row, index, tile, cell, paint, &mut spots.brush);
            }
            // On a press already handled as a long press, the release's tap is swallowed — otherwise
            // Wi-Fi turns off and the settings open with it.
            if resp.clicked() && action.is_none() && !row.swallow_tap {
                action = tile_tap(quick, *state, *allowed, live, index, &mut expanded);
            }
        }
        if r + 1 < rows_n {
            cursor_y += tile_px + gap;
        }
    }

    let mut bottom = block_bottom + gap;
    let mut slider_rect = Rect::NOTHING;
    let mut slider_value = None;
    // The Slider expansion row ("2–3 rows when expanded").
    let row_rect = Rect::from_min_size(
        egui::pos2(rect.min.x + 2.0 * gap, slider_top),
        Vec2::new(
            rect.width() - 4.0 * gap,
            expanded_height(row, expanded, metrics),
        ),
    );
    match expanded_row(ui, parts, row, expanded, row_rect) {
        Some(out) => {
            if out.action.is_some() && action.is_none() {
                action = out.action;
            }
            slider_rect = out.rect;
            slider_value = out.value;
            bottom = row_rect.max.y + gap;
        }
        None => expanded = None,
    }

    let moving = moving.get(..moving_n).unwrap_or(&moving);
    draw_moving_tiles(ui, parts, row, moving, &mut spots.brush);

    TilesOut {
        action,
        bottom,
        expanded,
        slider_rect,
        slider_value,
    }
}
/// The expansion row's height. A row an integrator draws has a height of its own — three or four
/// gauges will not sit in one widget's worth of line.
pub(super) fn expanded_height(
    row: &TileRow<'_>,
    expanded: Option<usize>,
    metrics: crate::theme::Metrics,
) -> f32 {
    expanded
        .and_then(|i| row.tiles.get(i))
        .map_or(metrics.widget_height, |(quick, _, _)| match &quick.kind {
            TileKind::Panel { height } => height.max(metrics.widget_height),
            // The row count × a finger's height. The declaration settles the row count, so the height comes from it.
            TileKind::Gauges { rows, .. } => (rows.len().max(1) as f32) * gauge_row_height(metrics),
            _ => metrics.widget_height,
        })
}
pub(super) fn expanded_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    expanded: Option<usize>,
    row_rect: Rect,
) -> Option<SliderOut> {
    let index = expanded?;
    let (quick, state, allowed) = row.tiles.get(index)?;
    if !(*allowed && quick.enabled) {
        return None;
    }
    match &quick.kind {
        TileKind::Slider(key) => Some(draw_slider_row(
            ui,
            parts,
            SliderSlot {
                rect: row_rect,
                index,
                key,
                state: *state,
            },
        )),
        TileKind::Panel { .. } => Some(SliderOut {
            action: None,
            value: None,
            // A drag started in this row is not taken by the shade — a slider can be put inside it.
            rect: draw_panel_row(ui, parts, row, &quick.id, row_rect),
        }),
        // Empty, the row does not open (a tile whose rows were taken away while it was open
        // folds back).
        TileKind::Gauges { rows, .. } if rows.is_empty() => None,
        TileKind::Gauges { rows, label_width } => Some(draw_gauges_row(
            ui,
            parts,
            GaugeRows {
                rect: row_rect,
                tile: index,
                rows,
                label_width: *label_width,
                texts: row.texts,
            },
        )),
        TileKind::Toggle(_) | TileKind::Action(_) | TileKind::Status => None,
    }
}
/// One gauge row's height — one finger. The row count × this is the expansion row's height.
pub(super) fn gauge_row_height(metrics: crate::theme::Metrics) -> f32 {
    metrics.touch_target
}
/// [`TileKind::Gauges`]'s expansion row — **the crate draws it.**
///
/// A row is `name | value | track`, and the declaration sets the name and value columns' widths.
/// The track uses all the width left and its colour differs by row. The icon's place on the left is
/// left empty as it is for [`TileKind::Slider`] — the tile descends into it.
pub(super) fn draw_gauges_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    spec: GaugeRows<'_>,
) -> SliderOut {
    let theme: &Theme = parts.theme;
    let metrics = theme.metrics;
    let icon_d = metrics.touch_target * theme.components.shade.note_icon;
    let gap = metrics.row_height * SLIDER_ICON_GAP;
    let body = Rect::from_min_max(
        egui::pos2(spec.rect.min.x + icon_d + gap, spec.rect.min.y),
        spec.rect.max,
    );
    let mut out = SliderOut {
        action: None,
        value: None,
        rect: body,
    };
    let row_h = gauge_row_height(metrics);
    let base_track = crate::widgets::base_thickness(theme);
    for (i, gauge) in spec.rows.iter().enumerate() {
        let top = row_h.mul_add(i as f32, body.min.y);
        let line = Rect::from_min_max(
            egui::pos2(body.min.x, top),
            egui::pos2(body.max.x, top + row_h),
        );
        if let Some(action) = draw_gauge(ui, parts, &spec, (i, gauge), (line, base_track)) {
            if out.action.is_none() {
                out.action = Some(action);
            }
        }
    }
    out
}
/// One gauge row. On a frame the value changed, it hands back a write action.
pub(super) fn draw_gauge(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    spec: &GaugeRows<'_>,
    (index, gauge): (usize, &Gauge),
    (line, base_track): (Rect, f32),
) -> Option<PanelAction> {
    let theme: &Theme = parts.theme;
    // The value column: the gauge's own width, or `100 %` at the body size.
    let value_w = gauge
        .value_width
        .unwrap_or(theme.metrics.type_scale.body * crate::overlay::tiles::GAUGE_VALUE_EM);
    let mid = line.center().y;
    let mut left = line.min.x;
    if spec.label_width > 0.5 {
        if let Some(g) = galley(spec.texts, TextKey::GaugeLabel(spec.tile, index)) {
            ui.painter().galley(
                egui::pos2(left, mid - g.size().y / 2.0),
                Arc::clone(g),
                theme.color(ColorRole::OnSurface),
            );
        }
        left += spec.label_width;
    }
    if !gauge.unit.is_empty() {
        if let Some(g) = galley(spec.texts, TextKey::GaugeValue(spec.tile, index)) {
            // The value is **right-aligned** — the track's starting point does not wobble as the digit count changes.
            let x = left + value_w - g.size().x - base_track * 0.5;
            ui.painter().galley(
                egui::pos2(x, mid - g.size().y / 2.0),
                Arc::clone(g),
                theme.color(ColorRole::Muted),
            );
        }
        left += value_w;
    }
    let track = Rect::from_min_max(egui::pos2(left, line.min.y), line.max);
    if track.width() < base_track * 2.0 {
        return None;
    }
    let current = gauge_value(parts, gauge);
    let resp = ui.interact(
        track,
        egui::Id::unique(("fairing.tile.gauge", spec.tile, index)),
        if gauge.read_only {
            Sense::hover()
        } else {
            Sense::click_and_drag()
        },
    );
    // It follows the finger, and says so: the page under the shade reads no swipe off this drag.
    crate::drag::claim_if_held(&resp);
    let axis = track.shrink2(Vec2::new(base_track / 2.0, 0.0));
    let pressed = !gauge.read_only && resp.is_pointer_button_down_on();
    let mut value = current;
    let mut action = None;
    if pressed {
        if let Some(pos) = resp.interact_pointer_pos() {
            value = ((pos.x - axis.min.x) / axis.width().max(1.0)).clamp(0.0, 1.0);
            let pct = round_pct(value);
            if pct != round_pct(current) {
                action = Some(PanelAction::Launch(LaunchAction::Set(
                    gauge.key.clone(),
                    SettingValue::Int(pct),
                )));
            }
        }
    }
    // The press swell is the same tween as the built-in slider's (A7).
    let tween = theme.motion.press;
    let grow = parts.animations.animate(
        egui::Id::unique(("fairing.tile.gauge.press", spec.tile, index)),
        f32::from(u8::from(pressed)),
        tween,
        parts.frame,
    );
    let theme: &Theme = parts.theme;
    let dim = if gauge.read_only { 0.55 } else { 1.0 };
    crate::widgets::paint_track(
        ui.painter(),
        track,
        value,
        // `from_theme` first, so the shade inherits the track's edge and its mover from the control
        // vocabulary and only overrides the two colours a gauge actually decides for itself.
        crate::widgets::TrackPaint {
            done: theme.color(gauge.color).gamma_multiply(dim),
            handle: theme.color(ColorRole::OnSurface).gamma_multiply(dim),
            ..crate::widgets::TrackPaint::from_theme(
                theme,
                base_track,
                crate::widgets::swollen(theme, base_track, grow),
            )
        },
    );
    action
}
/// A gauge's value, 0..=1 — it lives in the config as an integer `0..=100`.
pub(super) fn gauge_value(parts: &CxParts<'_>, gauge: &Gauge) -> f32 {
    match parts.settings.get(&gauge.key) {
        Some(SettingValue::Int(v)) => (*v as f32 / 100.0).clamp(0.0, 1.0),
        _ => 0.0,
    }
}
/// [`TileKind::Panel`]'s expansion row — an integrator draws the inside.
///
/// The icon's place on the left is left empty as it is for [`TileKind::Slider`]. That is because
/// **the tile descends into it** (`draw_tiles`'s shared-element transition) — a body covering it
/// would hide the icon.
pub(super) fn draw_panel_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    id: &str,
    rect: Rect,
) -> Rect {
    let metrics = parts.theme.metrics;
    let icon_d = metrics.touch_target * parts.theme.components.shade.note_icon;
    let gap = metrics.row_height * SLIDER_ICON_GAP;
    let body = Rect::from_min_max(egui::pos2(rect.min.x + icon_d + gap, rect.min.y), rect.max);
    let Some(decl) = row.panels.iter().find(|d| d.id() == id) else {
        return body;
    };
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
    child.set_clip_rect(child.clip_rect().intersect(body));
    let pane = PaneInfo {
        rect: body,
        outer: body,
        is_split: false,
        is_focused: false,
        inset_bottom: 0.0,
        instance: InstanceId::NONE,
    };
    let mut cx = parts.cx(pane, None);
    decl.draw_panel(&mut child, &mut cx);
    body
}
/// A tile in motion is drawn **behind the Slider row** — otherwise the row's ground covers it. Its
/// label dims as it goes.
pub(super) fn draw_moving_tiles(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    moving: &[(usize, Rect, f32)],
    brush: &mut TileBrush<'_>,
) {
    for &(index, cell, t) in moving {
        let Some((quick, state, allowed)) = row.tiles.get(index) else {
            continue;
        };
        draw_tile_cell(
            ui,
            parts,
            row,
            index,
            (quick, *state, *allowed),
            cell,
            CellPaint {
                pressed: false,
                fade: 1.0 - t,
            },
            brush,
        );
    }
}
/// How far tile `index` has **left the row** (0 = in place, 1 = at the Slider row's icon place).
pub(super) fn tile_out(
    parts: &mut CxParts<'_>,
    index: usize,
    expanded: Option<usize>,
    tween: crate::motion::Tween,
) -> f32 {
    parts.animations.animate(
        egui::Id::unique(("fairing.tile.out", index)),
        f32::from(u8::from(expanded == Some(index))),
        tween,
        parts.frame,
    )
}
/// The Slider expansion row: a track that **shows whose value it is**. The value follows the finger
/// 1:1 (A7). It raises `Set(key, Int(pct))` only on a frame the integer percentage changes — once
/// the backend or the config takes it, the next frame's `TileState` follows.
///
/// **Its drawing is one with [`TouchSlider`](crate::widgets::TouchSlider).** It used to draw two
/// line segments and a puck here itself with a thickness of `4.0` written in, so changing the
/// `slider.track_ratio` token left the shade alone. Now they share the one
/// `widgets::slider::paint_track`.
///
/// **The icon on the left comes down from the tile.** With only a track there is no telling what is
/// being operated — "what is this bar for?" was actually asked. Sliding the icon from the tile's
/// place to the left of the row joins the two.
pub(super) fn draw_slider_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    slot: SliderSlot<'_>,
) -> SliderOut {
    let theme: &Theme = parts.theme;
    let metrics = theme.metrics;
    let full_rect = slot.rect;
    let current = match slot.state {
        TileState::Value(v) => Some(v.clamp(0.0, 1.0)),
        _ => None,
    };
    // What is left after taking the icon's place off the left is the track.
    let icon_d = metrics.touch_target * theme.components.shade.note_icon;
    let gap = metrics.row_height * SLIDER_ICON_GAP;
    let track_rect = Rect::from_min_max(
        egui::pos2(full_rect.min.x + icon_d + gap, full_rect.min.y),
        full_rect.max,
    );
    let resp = ui.interact(
        track_rect,
        egui::Id::unique(("fairing.tile.slider", slot.index)),
        Sense::click_and_drag(),
    );
    // It follows the finger, and says so: the page under the shade reads no swipe off this drag.
    crate::drag::claim_if_held(&resp);
    let base_track = crate::widgets::base_thickness(theme);
    let axis = track_rect.shrink2(Vec2::new(base_track / 2.0, 0.0));
    let pressed = current.is_some() && resp.is_pointer_button_down_on();
    let mut value = current.unwrap_or(0.0);
    let mut out = SliderOut {
        action: None,
        value: None,
        rect: track_rect,
    };
    if pressed {
        if let Some(pos) = resp.interact_pointer_pos() {
            value = ((pos.x - axis.min.x) / axis.width().max(1.0)).clamp(0.0, 1.0);
            out.value = Some(value);
            let pct = round_pct(value);
            if pct != round_pct(current.unwrap_or(0.0)) {
                out.action = Some(PanelAction::Launch(LaunchAction::Set(
                    slot.key.clone(),
                    SettingValue::Int(pct),
                )));
            }
        }
    }

    let live = current.is_some();
    let (done, todo, handle) = if live {
        (
            theme.color(ColorRole::Primary),
            theme.color(ColorRole::SurfaceVariant),
            theme.color(ColorRole::OnSurface),
        )
    } else {
        let muted = theme.color(ColorRole::Muted);
        (
            muted.gamma_multiply(0.55),
            muted.gamma_multiply(0.22),
            theme.color(ColorRole::ShadeSurface),
        )
    };
    let grow = parts.animations.animate(
        egui::Id::unique(("fairing.tile.slider.press", slot.index)),
        f32::from(u8::from(pressed)),
        theme.motion.press,
        parts.frame,
    );
    let track_h = crate::widgets::swollen(theme, base_track, grow);
    crate::widgets::paint_track(
        ui.painter(),
        track_rect,
        value,
        // As above: the vocabulary's track, with only the three colours this tile chooses replaced.
        crate::widgets::TrackPaint {
            done,
            todo,
            handle,
            ..crate::widgets::TrackPaint::from_theme(theme, base_track, track_h)
        },
    );

    // The value goes above the track, only while it is held. The same convention as `TouchSlider`
    // — and measured off the track's own top, not the row's: at the row's top edge the label
    // stood entirely outside the row and the row's clip cut it in half.
    if grow > 0.01 && live {
        let x = axis.min.x + axis.width() * value;
        let top = track_rect.center().y - track_h * 0.5 - theme.components.slider.label_gap;
        ui.painter().text(
            egui::pos2(x, top),
            egui::Align2::CENTER_BOTTOM,
            format!("{}%", round_pct(value)),
            egui::TextStyle::Small.resolve(ui.style()),
            theme.color(ColorRole::OnSurface).gamma_multiply(grow),
        );
    }
    out
}
/// 0…1 as an integer percentage.
#[expect(
    clippy::cast_possible_truncation,
    reason = "it is clamp(0,1) × 100, so it is inside i64's range"
)]
pub(super) fn round_pct(v: f32) -> i64 {
    (v.clamp(0.0, 1.0) * 100.0).round() as i64
}
