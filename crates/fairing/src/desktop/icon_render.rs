//! Icon render.

use super::{
    builtin, scale_about, Arc, BadgeAnchor, BadgeTone, Color32, ColorRole, CountBadge,
    DesktopAction, DesktopCtx, IconColor, IconSlot, IconStyle, LabelLegibility, LayoutJob, Rect,
    Sense, SlotCx, SlotPaint, Theme, Visibility, ICON_SHADOW_DROP, SHADOW_ALPHA, SHADOW_DROP,
};

/// One cell: the gate filter → the icon plus the label plus the badge → the tap. It hands back what
/// happened.
///
/// The hit test, the press decision, the gate filter and recording the A2 origin Rect are **always
/// the shell's**. Where there is an integrator [`SlotPainter`], only the drawing is handed over —
/// the press tint is the painter's too.
pub(super) fn draw_slot(
    ui: &mut egui::Ui,
    cell: Rect,
    slot: &IconSlot,
    in_dock: bool,
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) -> Option<DesktopAction> {
    // The gate is settled when the slot is placed (`IconSlot::resolve_gate`). `None` happens only where
    // an integrator cleared it through `slots_mut()`, so it is taken by the name decision — neither path
    // builds a `Gate` per frame.
    let allowed = match &slot.gate {
        Some(gate) => cx.access.allows(gate),
        None => cx.access.allows_name(&slot.id),
    };
    // Hidden: on a failed gate, as though it were not there at all.
    if !allowed && slot.visibility == Visibility::Hidden {
        return None;
    }
    let metrics = cx.theme.metrics;
    let key = egui::Id::unique(("fairing.desktop.slot", &slot.id));
    let response = ui.interact(cell, key, Sense::click());
    // A `Response`'s press is the previous pass's snapshot and so is a frame late. The raw input is read
    // alongside it so it answers on the very frame of the press (A7, "≤ 16 ms").
    let down = !paint.drag_active
        && !paint.spent
        && (response.is_pointer_button_down_on()
            || paint
                .raw_press
                .is_some_and(|(origin, now)| cell.contains(origin) && cell.contains(now)));
    if down {
        *paint.pressed = Some(key);
    }
    paint.info.see(&slot.id, cell);
    let scale = paint.press.scale_for(key, down, &cx.theme.motion);

    let icon_size = metrics
        .icon_size
        .min(cell.width() * metrics.desktop_icon_ratio);
    // **The larger** of the measured row height and the token formula is used. The measurement alone
    // gets denser than today depending on the font; the formula alone cuts the bottom off the text with
    // a fallback font.
    let line_h = paint
        .label_row_h
        .max(metrics.desktop_label_size * metrics.desktop_label_line);
    let label_h = f32::from(paint.label_lines) * line_h;
    let content_h = icon_size + metrics.desktop_label_gap + label_h;
    let top = cell.min.y + ((cell.height() - content_h) / 2.0).max(metrics.desktop_content_min_pad);
    let icon_rect = Rect::from_center_size(
        egui::pos2(cell.center().x, top + icon_size / 2.0),
        egui::vec2(icon_size, icon_size),
    );
    // What is recorded is the Rect before the press (so the A2 origin does not wobble mid-press), in
    // **the settled page's** coordinates with the page-swipe offset taken out (A4).
    if let Some(dx) = paint.record_dx {
        if let Some(record) = paint.record(&slot.id) {
            record.rect = Some(icon_rect.translate(egui::vec2(-dx, 0.0)));
        }
    }
    let drawn_icon = scale_about(icon_rect, cell.center(), scale);
    if let Some(painter) = paint.slot_painter.as_deref_mut() {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(cell));
        painter(
            &mut child,
            SlotCx {
                cell,
                icon: icon_rect,
                pressed_icon: drawn_icon,
                slot,
                scale,
                pressed: down,
                allowed,
                in_dock,
                theme: cx.theme,
                icons: &mut *cx.icons,
                now: cx.now,
                strings: cx.strings,
            },
        );
    } else {
        draw_slot_content(
            ui, cell, drawn_icon, icon_rect, slot, allowed, down, scale, cx, paint,
        );
    }

    if paint.drag_active {
        // A4's "no icon tap mid-drag" — a press past the slop is taken by the page.
        return None;
    }
    if response.clicked() && !paint.spent {
        return Some(if allowed {
            DesktopAction::Tap(slot.id.clone())
        } else {
            DesktopAction::TapLocked(slot.id.clone())
        });
    }
    // The gesture engine's long press, not egui's `long_touched()`: that one needs real touch
    // events and waits out `max_click_duration` (0.8 s), so it never fired on a panel that
    // delivers touch as the mouse (the quick tiles had the same problem).
    if paint.held.is_some_and(|pos| cell.contains(pos)) {
        *paint.long = Some(cell);
        return Some(DesktopAction::LongPress(slot.id.clone()));
    }
    None
}
/// The built-in cell rendering: the press tint → the icon → the padlock → the badge → the label.
#[allow(clippy::too_many_arguments)] // These are the values drawing one cell needs (grouping them buys nothing).
pub(super) fn draw_slot_content(
    ui: &egui::Ui,
    cell: Rect,
    drawn_icon: Rect,
    icon_rect: Rect,
    slot: &IconSlot,
    allowed: bool,
    down: bool,
    scale: f32,
    cx: &mut DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) {
    let metrics = cx.theme.metrics;
    let painter = ui.painter().clone();
    // A7: it answers on the first frame with the tint, and the scale shrinks over 80 ms.
    if down {
        painter.rect_filled(
            cell.shrink(metrics.desktop_press_inset),
            metrics.corner_radius,
            cx.theme.color(ColorRole::Pressed),
        );
    }
    // The stroke is a token in 24-grid units (2.0 by default — the same as `IconStyle`'s default before the promotion).
    let style = IconStyle {
        stroke: Some(metrics.desktop_icon_stroke),
        ..IconStyle::sized(drawn_icon.width()).enabled(allowed)
    };
    // The icons get the same prescription as the labels — a white stroked icon disappears before a label
    // does on a bright background (the wrench over the white belly of the manta original does).
    if cx.legibility == LabelLegibility::Shadow {
        let drop = (drawn_icon.width() * ICON_SHADOW_DROP).max(1.0);
        let shadow = IconStyle {
            color: IconColor::Fixed(shadow_color(cx.theme)),
            ..style
        };
        cx.icons.paint(
            &painter,
            drawn_icon.translate(egui::vec2(0.0, drop)),
            &slot.icon,
            &shadow,
            cx.theme,
        );
    }
    if !cx
        .icons
        .paint(&painter, drawn_icon, &slot.icon, &style, cx.theme)
    {
        // An empty generated set, or a name it does not know: the placeholder circle.
        painter.circle_stroke(
            drawn_icon.center(),
            drawn_icon.width() / 2.0 - 2.0,
            egui::Stroke::new(2.0, style.resolve_color(cx.theme)),
        );
    }
    if !allowed {
        draw_lock(&painter, drawn_icon, cx);
    }
    draw_badge(&painter, drawn_icon, slot, cx);
    draw_label(&painter, cell, icon_rect, slot, allowed, scale, cx, paint);
}
/// A failed gate plus `Visibility::Locked`: the padlock badge.
pub(super) fn draw_lock(painter: &egui::Painter, icon: Rect, cx: &mut DesktopCtx<'_>) {
    let lock = cx.theme.metrics.desktop_lock_size;
    // The border padding is added **to the outer circle only**. Growing `lock` itself would grow `inner`
    // and `IconStyle` below with it and have the padlock fill the circle (which is what happened once
    // during the promotion, and the pixel comparison caught it).
    let ring = lock + cx.theme.metrics.desktop_lock_ring_pad;
    let badge = Rect::from_center_size(icon.right_bottom(), egui::vec2(ring, ring));
    painter.circle_filled(
        badge.center(),
        badge.width() / 2.0,
        cx.theme.color(ColorRole::Surface),
    );
    let inner = Rect::from_center_size(badge.center(), egui::vec2(lock, lock));
    let style = IconStyle::sized(lock).color(crate::icons::IconColor::Role(ColorRole::Warning));
    if !cx
        .icons
        .paint(painter, inner, &builtin::LOCK, &style, cx.theme)
    {
        painter.circle_filled(
            badge.center(),
            cx.theme.metrics.desktop_badge_dot_r,
            cx.theme.color(ColorRole::Warning),
        );
    }
}
/// The `Count` / `Dot` / `Text` badge.
pub(super) fn draw_badge(
    painter: &egui::Painter,
    icon: Rect,
    slot: &IconSlot,
    cx: &DesktopCtx<'_>,
) {
    let Some(badge) = &slot.badge else {
        return;
    };
    // Through the widget rather than a pill and a galley of its own. The pair it replaces wrote
    // `OnPrimary` on `Danger`, which measures **2.79** in base dark — a count the operator is
    // meant to read, 38 % under the 4.5 a word needs. `CountBadge` carries the ink per tone, so
    // the digits here go from white to near-black in three of the four palettes. That is the fix.
    // The rect it returns is the space it occupied, halo included; a dock icon has nothing to
    // keep clear of it, so it is dropped here rather than threaded through the cell layout.
    let _occupied = CountBadge::new(badge.as_value())
        .tone(BadgeTone::Alert)
        .paint_over(painter, cx.theme, icon, BadgeAnchor::TopEnd);
}
/// The label (wrapped over `label_lines` lines, with `…` where it overflows). The galley is rebuilt only when the width changes.
#[allow(clippy::too_many_arguments)] // An internal helper that takes the painter, the geometry and the state in one go.
pub(super) fn draw_label(
    painter: &egui::Painter,
    cell: Rect,
    icon: Rect,
    slot: &IconSlot,
    allowed: bool,
    scale: f32,
    cx: &DesktopCtx<'_>,
    paint: &mut SlotPaint<'_>,
) {
    let lines = paint.label_lines;
    let width = paint.label_width;
    // Recompute the icon transform from the current rectangle each frame.
    let mut job = LayoutJob::simple(
        cx.strings.get(&slot.label).to_owned(),
        egui::FontId::proportional(cx.theme.metrics.desktop_label_size),
        Color32::PLACEHOLDER,
        width,
    );
    job.halign = egui::Align::Center;
    job.wrap.max_rows = usize::from(lines);
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    let galley = &galley;
    let color = if allowed {
        cx.theme.color(ColorRole::OnSurface)
    } else {
        cx.theme.color(ColorRole::Muted)
    };
    let anchor = egui::pos2(
        cell.center().x,
        icon.max.y + cx.theme.metrics.desktop_label_gap,
    );
    let pos = cell.center() + (anchor - cell.center()) * scale;
    // The shadow comes **first** — it stops the label being lost on a bright background (a photograph, an
    // art plate). egui has no blur, so it is approximated with one dark copy offset by a step. Offset by
    // two or more it reads double, and a higher alpha makes the label look thick on a dark background.
    if cx.legibility == LabelLegibility::Shadow {
        let drop = (cx.theme.metrics.desktop_label_size * SHADOW_DROP).max(1.0);
        painter.galley(
            pos + egui::vec2(0.0, drop),
            Arc::clone(galley),
            shadow_color(cx.theme),
        );
    }
    painter.galley(pos, Arc::clone(galley), color);
}
/// The label shadow's colour. It takes [`ColorRole::Scrim`]'s hue but pins the alpha to the shadow's
/// own — a palette with a faint scrim must not make the shadow disappear.
pub(super) fn shadow_color(theme: &Theme) -> Color32 {
    let scrim = theme.color(ColorRole::Scrim);
    Color32::from_rgba_unmultiplied(scrim.r(), scrim.g(), scrim.b(), SHADOW_ALPHA)
}
/// The actual height of one label line (du). egui's line height is the maximum across **the whole
/// font family**, so with a CJK fallback font loaded it comes out larger than the formula
/// `desktop_label_size × desktop_label_line`. It is called once per frame — the call takes the
/// context's write lock, so it must not be called per cell.
pub(super) fn label_row_h(ctx: &egui::Context, theme: &Theme) -> f32 {
    let font = egui::FontId::proportional(theme.metrics.desktop_label_size);
    ctx.fonts_mut(|fonts| fonts.row_height(&font))
}
