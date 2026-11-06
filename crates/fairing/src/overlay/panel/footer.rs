//! Footer.

use super::{
    builtin, galley, Arc, ColorRole, CxParts, FooterButton, IconColor, IconRef, IconStyle,
    LaunchAction, NotificationCenter, OverlayPanel, PanelAction, PanelStyle, Rect, Sense, TextKey,
    Theme, TileRow, Vec2,
};

/// The footer's icon buttons (filled from the right). `true` if it was tapped.
pub(super) fn footer_icon_button(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    right: &mut f32,
    footer: Rect,
    id: &'static str,
    icon: &IconRef,
) -> bool {
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let d = style.footer_button;
    // **Here the place widens too**. Unlike the close ×, the footer buttons are separated
    // from their neighbours by only `card_gap` (≈ 8 du), so widening the hit Rect alone would overlap the
    // next button and press the wrong thing. So **the slot** is taken at the touch target and a disc of
    // diameter `d` drawn inside it — the finger settles the place and the token the drawing. The buttons
    // spreading apart by that much is the intended result.
    let slot = d
        .max(parts.theme.metrics.touch_target)
        .min(footer.height().max(d));
    let cell = Rect::from_center_size(
        egui::pos2(*right - slot * 0.5, footer.center().y),
        Vec2::splat(slot),
    );
    *right = cell.min.x - style.card_gap;
    let resp = ui.interact(cell, egui::Id::unique(id), Sense::click());
    let down = resp.is_pointer_button_down_on();
    // An icon floating alone does not show where the pressable place is — a disc beneath it declares it a button.
    ui.painter().circle_filled(
        cell.center(),
        d * 0.5,
        theme.color(if down {
            ColorRole::Primary
        } else {
            ColorRole::SurfaceVariant
        }),
    );
    let icon_px = d * 0.46;
    let icon_style = IconStyle {
        color: IconColor::Role(if down {
            ColorRole::OnPrimary
        } else {
            ColorRole::OnSurface
        }),
        ..IconStyle::sized(icon_px)
    };
    parts.icons.paint(
        ui.painter(),
        Rect::from_center_size(cell.center(), Vec2::splat(icon_px)),
        icon,
        &icon_style,
        theme,
    );
    resp.clicked()
}
/// The footer (the subject's name · the level dot · clear all · lock · settings) plus the bottom handle.
pub(super) fn draw_footer(
    ui: &mut egui::Ui,
    rect: Rect,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    center: &NotificationCenter,
    footer_h: f32,
) -> Option<PanelAction> {
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let mut action = None;
    let footer = Rect::from_min_size(
        egui::pos2(rect.min.x + style.pad, rect.max.y - footer_h - style.gap),
        Vec2::new(rect.width() - style.pad * 2.0, footer_h),
    );
    // The separator between the list and the footer. It keeps the scroll from looking as though it flows under the footer.
    ui.painter().hline(
        footer.min.x..=footer.max.x,
        footer.min.y - style.gap * 0.5,
        egui::Stroke::new(
            theme.control.stroke_hairline,
            theme.color(ColorRole::Outline),
        ),
    );

    // The level's colour dot: Primary at the top level, Muted otherwise. Who is signed in goes with
    // the lock and the settings, so a split shade shows it on the controls and not under the list.
    if row.panel != OverlayPanel::Notifications {
        let session = parts.access.session();
        let top = parts.access.table().top();
        let dot_color = theme.color(if session.subject.level == top {
            ColorRole::Primary
        } else {
            ColorRole::Muted
        });
        let dot_r = style.gap * 0.3;
        ui.painter().circle_filled(
            egui::pos2(footer.min.x + dot_r, footer.center().y),
            dot_r,
            dot_color,
        );
        if let Some(g) = galley(row.texts, TextKey::FooterName) {
            let size = g.size();
            ui.painter().galley(
                egui::pos2(
                    footer.min.x + dot_r * 2.0 + style.card_gap,
                    footer.center().y - size.y * 0.5,
                ),
                Arc::clone(g),
                theme.color(ColorRole::OnSurface),
            );
        }
    }

    // `[overlay] footer` reads left to right as it is drawn, so it is laid out from the right in
    // reverse. **Nothing here is unconditional** — a device with no lock concept, or one that never
    // registered `settings.home`, empties the list and the buttons are gone.
    let mut right = footer.max.x;
    for button in row.footer.iter().rev() {
        match button {
            FooterButton::Settings => {
                if footer_icon_button(
                    ui,
                    parts,
                    &mut right,
                    footer,
                    "fairing.overlay.footer.settings",
                    &builtin::SETTINGS,
                ) {
                    action = Some(PanelAction::Launch(LaunchAction::open("settings.home")));
                }
            }
            FooterButton::Lock => {
                if footer_icon_button(
                    ui,
                    parts,
                    &mut right,
                    footer,
                    "fairing.overlay.footer.lock",
                    &builtin::LOCK,
                ) {
                    action.get_or_insert(PanelAction::Launch(LaunchAction::Lock));
                }
            }
            FooterButton::ClearAll => {
                clear_all_button(ui, parts, &mut right, footer, row, center, &mut action);
            }
        }
    }

    // The handle is drawn at **the curtain's end** (the bottom of what is revealed) — towards the finger
    // while it is being pulled, and at the panel's bottom once open (the clip is the whole panel). A
    // caller that has set no clip draws it at the panel's bottom.
    let edge = ui.clip_rect().max.y.min(rect.max.y);
    // Four hairlines thick: a bar, not a line, at any density.
    let handle_h = theme.control.stroke_hairline * 4.0;
    let handle = Rect::from_center_size(
        egui::pos2(rect.center().x, edge - handle_h * 1.6),
        Vec2::new(theme.metrics.shade_handle_width, handle_h),
    );
    ui.painter()
        .rect_filled(handle, handle_h * 0.5, theme.color(ColorRole::Outline));
    action
}
/// "Clear all", as a pill rather than a bare text link — a link does not show where the pressable
/// place is. Drawn only where there is something to clear.
pub(super) fn clear_all_button(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    right: &mut f32,
    footer: Rect,
    row: &TileRow<'_>,
    center: &NotificationCenter,
    action: &mut Option<PanelAction>,
) {
    // Persistent notifications survive "clear all", so a list of only those has nothing to clear.
    if center.iter().all(|n| n.persistent) {
        return;
    }
    let Some(g) = galley(row.texts, TextKey::ClearAll) else {
        return;
    };
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let size = g.size();
    let h = style.footer_button;
    let w = size.x + style.card_pad * 2.0;
    let clear = Rect::from_center_size(
        egui::pos2(*right - w * 0.5, footer.center().y),
        Vec2::new(w, h),
    );
    *right = clear.min.x - style.card_gap;
    let resp = ui.interact(
        clear,
        egui::Id::unique("fairing.overlay.clear"),
        Sense::click(),
    );
    let down = resp.is_pointer_button_down_on();
    ui.painter().rect_filled(
        clear,
        h * 0.5,
        theme.color(if down {
            ColorRole::Primary
        } else {
            ColorRole::SurfaceVariant
        }),
    );
    ui.painter().galley(
        clear.center() - size * 0.5,
        Arc::clone(g),
        theme.color(if down {
            ColorRole::OnPrimary
        } else {
            ColorRole::Primary
        }),
    );
    if resp.clicked() {
        action.get_or_insert(PanelAction::ClearAll);
    }
}
