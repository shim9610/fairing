//! Notifications.

use super::{
    builtin, galley, notification_row_h, Arc, CardOut, ColorRole, CxParts, IconColor, IconRef,
    IconStyle, ListOut, Notification, NotificationCenter, PanelAction, PanelStyle, Rect, Sense,
    TextKey, Theme, TileRow, Vec2, HINT_GROUND,
};

/// The notification list (inside the scroll area).
pub(super) fn draw_list(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    center: &NotificationCenter,
    row: &TileRow<'_>,
) -> ListOut {
    let theme: &Theme = parts.theme;
    let metrics = theme.metrics;
    let mut out = ListOut {
        action: None,
        pressed: None,
        close_hit: Rect::NOTHING,
    };
    let pressed_now = ui.input(|i| i.pointer.primary_pressed());
    if center.is_empty() {
        let (empty, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), metrics.notification_row_height),
            Sense::hover(),
        );
        if let Some(g) = galley(row.texts, TextKey::NoNotifications) {
            let size = g.size();
            ui.painter().galley(
                empty.center() - size * 0.5,
                Arc::clone(g),
                theme.color(ColorRole::Muted),
            );
        }
    }
    for item in center.iter() {
        // A row mid-collapse: the card is already off-screen, so only **the space revealed** (the
        // throw-away marking) is left. It has to collapse together with the place for it to read as "the
        // discard slot closing" — the marking vanishing first and the place shrinking after looks like two
        // separate actions. It takes no input: a 1 px row taking a tap would open the notification just
        // removed.
        if let Some((_, scale)) = row.collapsing.filter(|(id, _)| *id == item.id) {
            let h = metrics.notification_row_height * scale;
            if h <= 0.5 {
                continue;
            }
            let (slot, _) =
                ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
            if ui.is_rect_visible(slot) {
                let style = PanelStyle::new(&metrics, theme.components.shade);
                let home = slot.shrink2(Vec2::new(style.pad, style.card_gap * 0.5));
                swipe_hint(ui, parts, item, home, -slot.width(), &style);
            }
            continue;
        }
        // The row is a touch target and a gap — and, where the title and body ask for more,
        // their two lines with the card's padding round them. The tokens are hand-sized and the
        // text is eye-sized, and at a coarse finger the finger's row was shorter than the
        // eye's two lines: the body sat on the card's bottom edge, half outside it.
        let style = PanelStyle::new(&metrics, theme.components.shade);
        let text_h = {
            let title = galley(row.texts, TextKey::Title(item.id)).map_or(0.0, |g| g.size().y);
            let body = galley(row.texts, TextKey::Body(item.id)).map_or(0.0, |g| g.size().y);
            title
                + if body > 0.0 {
                    style.card_gap * 0.5 + body
                } else {
                    0.0
                }
        };
        let row_h = notification_row_h(&metrics, &style)
            .max(text_h + style.card_gap + 2.0 * style.card_pad);
        let (row_rect, resp) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::click());
        if !ui.is_rect_visible(row_rect) {
            continue;
        }
        // Gate condensation is enforced at the notification centre alone — a row whose gate fails
        // hides its title, body and progress (the `allowed` branch of `draw_notification_row`).
        let allowed = crate::notify::shows_content(item, parts.access);
        let dx = row
            .swiping
            .filter(|(id, _)| *id == item.id)
            .map_or(0.0, |(_, x)| x);
        if resp.clicked() && out.action.is_none() {
            out.action = Some(PanelAction::NotificationTapped(item.id));
        }
        if !item.persistent && pressed_now && resp.contains_pointer() {
            out.pressed = Some(item.id);
        }
        let card = draw_notification_row(ui, parts, row, item, row_rect, dx, allowed);
        if out.close_hit == Rect::NOTHING {
            out.close_hit = card.close_hit;
        }
        if card.closed && allowed {
            out.action = Some(PanelAction::Dismiss(item.id));
        }
    }
    out
}
/// Draw **the outcome** in the space a swipe reveals — throwing it away on the left, opening it on
/// the right.
///
/// The direction decides the meaning, so the drawing has to split too. Were both the same grey,
/// which way the finger is pushing would not connect to what happens.
///
/// The right side appears **only on a notification with somewhere to go** (`action`). Without one,
/// `Overlay` does not pull to the right in the first place.
pub(super) fn swipe_hint(
    ui: &egui::Ui,
    parts: &mut CxParts<'_>,
    item: &Notification,
    home: Rect,
    dx: f32,
    style: &PanelStyle,
) {
    if dx.abs() < 0.5 {
        return;
    }
    let theme: &Theme = parts.theme;
    let to_open = dx > 0.0;
    if to_open && item.action.is_none() {
        return;
    }
    let (role, icon) = if to_open {
        (ColorRole::Primary, &builtin::CHEVRON_RIGHT)
    } else {
        (ColorRole::Danger, &builtin::TRASH)
    };
    let tint = theme.color(role);
    let painter = ui.painter().with_clip_rect(home);
    painter.rect_filled(home, style.radius, tint.gamma_multiply(HINT_GROUND));
    // The icon goes at **the end being revealed** — pushed left it is the right end, and the same the other way.
    let d = theme.metrics.touch_target * theme.components.shade.note_icon;
    let inset = style.card_pad + d / 2.0;
    let cx_ = if to_open {
        home.left() + inset
    } else {
        home.right() - inset
    };
    let icon_style = IconStyle {
        color: IconColor::Role(role),
        ..IconStyle::sized(d)
    };
    parts.icons.paint(
        &painter,
        Rect::from_center_size(egui::pos2(cx_, home.center().y), Vec2::splat(d)),
        icon,
        &icon_style,
        theme,
    );
}
/// One notification card (the icon disc · the title · the body · the progress · the close).
///
/// The structure divides into three — the icon column on the left, the text column in the middle,
/// the close column on the right. The text starts on **one vertical line** and the icon's optical
/// centre is matched to the title's line (not to the card's centre — with a long body the icon would
/// drift away from the title, blurring which text it belongs to).
#[expect(
    clippy::too_many_lines,
    reason = "it lays out one card's three columns in one place"
)]
pub(super) fn draw_notification_row(
    ui: &mut egui::Ui,
    parts: &mut CxParts<'_>,
    row: &TileRow<'_>,
    item: &Notification,
    row_rect: Rect,
    dx: f32,
    allowed: bool,
) -> CardOut {
    let theme: &Theme = parts.theme;
    let style = PanelStyle::new(&theme.metrics, theme.components.shade);
    let home = row_rect.shrink2(Vec2::new(style.pad, style.card_gap * 0.5));
    let card = home.translate(Vec2::new(dx, 0.0));
    let painter = ui.painter().with_clip_rect(row_rect);
    // **What is about to happen** is laid down in the place the card has moved off first, and the card goes over it.
    swipe_hint(ui, parts, item, home, dx, &style);
    painter.rect_filled(card, style.radius, theme.color(ColorRole::SurfaceVariant));

    let heading = if allowed {
        galley(row.texts, TextKey::Title(item.id))
    } else {
        galley(row.texts, TextKey::Hidden)
    };
    let body = allowed
        .then(|| galley(row.texts, TextKey::Body(item.id)))
        .flatten();
    let title_h = heading.map_or(0.0, |g| g.size().y);
    let body_h = body.map_or(0.0, |g| g.size().y);
    let line_gap = style.card_gap * 0.5;
    let text_h = title_h + if body_h > 0.0 { line_gap + body_h } else { 0.0 };
    let text_top = card.center().y - text_h * 0.5;

    // The icon: a disc centred on **the whole text block**, not on the title's line.
    //
    // It was `text_top + title_h * 0.5` — the title's own centre — which is right for a one-line
    // notification and visibly high on a two-line one, where it sits above the optical centre of
    // the title-and-body pair. `text_h` was already computed three lines up for exactly this block;
    // the badge simply was not using it.
    let puck_d = style.note_icon;
    let puck = Rect::from_center_size(
        egui::pos2(
            card.min.x + style.card_pad + puck_d * 0.5,
            text_top + text_h * 0.5,
        ),
        Vec2::splat(puck_d),
    );
    painter.circle_filled(
        puck.center(),
        puck_d * 0.5,
        theme.color(ColorRole::ShadeSurface),
    );
    let icon_px = puck_d * 0.52;
    let icon_style = IconStyle {
        color: IconColor::Role(if allowed {
            item.level.role()
        } else {
            ColorRole::Muted
        }),
        ..IconStyle::sized(icon_px)
    };
    // A gated row shows the bell and nothing else — the shape is part of the content it is hiding.
    // An icon the caller set is borrowed, never cloned per frame; the fallbacks are both
    // `Builtin`, so neither allocates.
    let fallback: IconRef = if allowed {
        item.level.icon()
    } else {
        builtin::BELL
    };
    let icon: &IconRef = match (&item.icon, allowed) {
        (Some(icon), true) => icon,
        _ => &fallback,
    };
    parts.icons.paint(
        &painter,
        Rect::from_center_size(puck.center(), Vec2::splat(icon_px)),
        icon,
        &icon_style,
        theme,
    );

    // With `[notify] dismiss_button = false` the × is not drawn at all — the only ways to dismiss are a
    // left swipe and "clear all". Not drawing it widens the text column by that much.
    let closable = row.dismiss_button && !item.persistent && allowed;
    let close_d = style.close;
    let text_x = puck.max.x + style.card_pad;
    let text_right = if closable {
        card.max.x - style.card_pad - close_d
    } else {
        card.max.x - style.card_pad
    };
    let text_painter = painter.with_clip_rect(Rect::from_min_max(
        egui::pos2(text_x, card.min.y),
        egui::pos2(text_right, card.max.y),
    ));
    let mut y = text_top;
    if let Some(g) = heading {
        text_painter.galley(
            egui::pos2(text_x, y),
            Arc::clone(g),
            theme.color(ColorRole::OnSurface),
        );
        y += title_h + line_gap;
    }
    if let Some(g) = body {
        text_painter.galley(
            egui::pos2(text_x, y),
            Arc::clone(g),
            theme.color(ColorRole::Muted),
        );
    }
    if allowed {
        if let Some(p) = item.progress {
            let h = theme.components.heads_up.progress_h;
            let bar = Rect::from_min_size(
                egui::pos2(text_x, card.max.y - style.card_pad * 0.6 - h),
                Vec2::new((text_right - text_x).max(0.0), h),
            );
            painter.rect_filled(bar, h * 0.5, theme.color(ColorRole::Outline));
            let done = Rect::from_min_size(bar.min, Vec2::new(bar.width() * p.clamp(0.0, 1.0), h));
            painter.rect_filled(done, h * 0.5, theme.color(ColorRole::Primary));
        }
    }
    if !closable {
        return CardOut {
            closed: false,
            close_hit: Rect::NOTHING,
        };
    }
    let close = Rect::from_center_size(
        egui::pos2(card.max.x - style.card_pad - close_d * 0.5, card.center().y),
        Vec2::splat(close_d),
    );
    // **The size drawn and the place pressed are separated** — the same prescription as a grid
    // cell's `rect` and `visual`.
    //
    // The close × is drawn at a multiple of `shade.close` (0.72), which comes to 9.4 mm under a gloved
    // finger — short of `finger_hard_mm` (10.1 mm). Growing the drawing would bury the notification card
    // in ×s, so **the drawing is left as it is and only the hit Rect** widens to the touch target. It
    // widens leftwards and covers the body text, but the text is not pressable, and between the two the
    // close is the smaller than a card tap (opening the notification), so it is right that it takes the place.
    let hit_d = close_d
        .max(parts.theme.metrics.touch_target)
        .min(card.height());
    let close_hit = Rect::from_center_size(close.center(), Vec2::splat(hit_d))
        .intersect(card)
        .translate(Vec2::new(-dx, 0.0));
    let close_resp = ui.interact(
        close_hit,
        egui::Id::unique(("fairing.notif.x", item.id.0)),
        Sense::click(),
    );
    if close_resp.hovered() || close_resp.is_pointer_button_down_on() {
        painter.circle_filled(
            close.center(),
            close_d * 0.5,
            theme.color(ColorRole::Pressed),
        );
    }
    let x_px = close_d * 0.42;
    let x_style = IconStyle {
        color: IconColor::Role(ColorRole::Muted),
        ..IconStyle::sized(x_px)
    };
    parts.icons.paint(
        &painter,
        Rect::from_center_size(close.center(), Vec2::splat(x_px)),
        &builtin::CLOSE,
        &x_style,
        theme,
    );
    CardOut {
        closed: close_resp.clicked(),
        close_hit,
    }
}
