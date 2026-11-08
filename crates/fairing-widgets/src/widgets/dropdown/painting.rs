//! Painting.

use super::{
    builtin, Color32, CornerRadius, Cx, Elevation, Face, IconColor, IconRef, IconStyle, Ink,
    LayoutJob, Pos2, Rect, Skin, Stroke, StrokeKind, TextWrapping, Tokens, Vec2, Words,
};

/// Where both fit, neither is cut. Where they do not, the value keeps what it needs up to half
/// the room and the muted label is cut to the rest — the value is what the control is for.
pub(super) fn paint_words(ui: &egui::Ui, words: Words<'_>, ink: Ink) {
    let mut x = words.at.x;
    let mut room = words.max_width;
    if let Some(label) = words.label {
        let label_w = text_width(ui, label, words.font);
        let value_w = text_width(ui, words.value, words.font);
        let value_take = value_w
            .min((room - label_w - words.gap).max(room * 0.5))
            .min(room);
        let label_room = (room - value_take - words.gap).max(0.0);
        let galley = truncated(ui, label, words.font.clone(), ink.hint, label_room);
        let size = galley.size();
        ui.painter()
            .galley(Pos2::new(x, words.at.y - size.y * 0.5), galley, ink.hint);
        let step = size.x + words.gap;
        x += step;
        room -= step;
    }
    let galley = truncated(
        ui,
        words.value,
        words.font.clone(),
        words.value_colour,
        room,
    );
    let size = galley.size();
    ui.painter().galley(
        Pos2::new(x, words.at.y - size.y * 0.5),
        galley,
        words.value_colour,
    );
}
/// The chevron at the trigger's right edge, half an inset in: a glyph carries its own optical
/// padding where a word does not, and Material's exposed dropdown pads its trailing icon 12 dp
/// against the text's 16. On a narrow field the difference is a value shown whole.
pub(super) fn chevron(ui: &egui::Ui, cx: &mut Cx<'_>, t: &Tokens, face: Face, colour: Color32) {
    chevron_at(
        ui,
        cx,
        t,
        Pos2::new(
            face.rect.max.x - t.inset * 0.5 - t.mark * 0.5,
            face.rect.center().y,
        ),
        face.open,
        colour,
    );
}
/// A chevron that turns over when the list is open — drawn, not rotated: the crate ships both
/// glyphs, and turning one over would have to guess a pivot that the icon's own bounding box
/// does not carry.
pub(super) fn chevron_at(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    t: &Tokens,
    at: Pos2,
    open: bool,
    colour: Color32,
) {
    let icon = if open {
        builtin::CHEVRON_UP
    } else {
        builtin::CHEVRON_DOWN
    };
    icon_at(ui, cx, &icon, at, t.mark, colour);
}
/// One glyph, `side` square, centred on `at`.
pub(super) fn icon_at(
    ui: &egui::Ui,
    cx: &mut Cx<'_>,
    icon: &IconRef,
    at: Pos2,
    side: f32,
    colour: Color32,
) {
    // Sized to the glyph it draws: the stroke follows the size, and `default()` is 24 whatever
    // the glyph is.
    let style = IconStyle::sized(side).color(IconColor::Fixed(colour));
    let rect = Rect::from_center_size(at, Vec2::splat(side));
    cx.icons.paint(ui.painter(), rect, icon, &style, cx.theme);
}
/// The top two corners of `c`, the bottom two square.
pub(super) const fn top_corners(c: CornerRadius) -> CornerRadius {
    CornerRadius {
        nw: c.nw,
        ne: c.ne,
        sw: 0,
        se: 0,
    }
}
/// The bottom two corners of `c`, the top two square.
pub(super) const fn bottom_corners(c: CornerRadius) -> CornerRadius {
    CornerRadius {
        nw: 0,
        ne: 0,
        sw: c.sw,
        se: c.se,
    }
}
/// The skin itself: its elevation, its ground, its edge and its line.
pub(super) fn paint_skin(painter: &egui::Painter, cx: &Cx<'_>, skin: &Skin) {
    crate::theme::paint_elevation(
        painter,
        cx.theme,
        skin.rect,
        skin.corners,
        Elevation::Floating,
    );
    painter.rect_filled(skin.rect, skin.corners, skin.ground);
    if let Some(stroke) = skin.stroke {
        painter.rect_stroke(skin.rect, skin.corners, stroke, StrokeKind::Inside);
    }
    if let Some(line) = skin.underline {
        let y = skin.rect.max.y - line.width * 0.5;
        painter.line_segment(
            [Pos2::new(skin.rect.min.x, y), Pos2::new(skin.rect.max.x, y)],
            line,
        );
    }
}
/// The hairline between the head and the rows, on whichever side the rows are: the head is not a
/// row.
pub(super) fn divider(painter: &egui::Painter, t: &Tokens, skin: &Skin, head: Rect, ink: Ink) {
    let y = if skin.below {
        head.max.y - t.hairline * 0.5
    } else {
        head.min.y + t.hairline * 0.5
    };
    painter.line_segment(
        [Pos2::new(head.min.x, y), Pos2::new(head.max.x, y)],
        Stroke::new(t.hairline, ink.outline),
    );
}
/// The panel's own body: its elevation and its opaque ground.
pub(super) fn paint_panel(
    painter: &egui::Painter,
    cx: &Cx<'_>,
    rect: Rect,
    corners: CornerRadius,
    ground: Color32,
) {
    crate::theme::paint_elevation(painter, cx.theme, rect, corners, Elevation::Floating);
    painter.rect_filled(rect, corners, ground);
}
/// The corners a row's pressed fill rounds: the ones it shares with the list.
pub(super) fn row_corners(index: usize, last: usize, radius: CornerRadius) -> CornerRadius {
    match (index == 0, index == last) {
        (true, true) => radius,
        (true, false) => CornerRadius {
            nw: radius.nw,
            ne: radius.ne,
            sw: 0,
            se: 0,
        },
        (false, true) => CornerRadius {
            nw: 0,
            ne: 0,
            sw: radius.sw,
            se: radius.se,
        },
        (false, false) => CornerRadius::ZERO,
    }
}
/// The cut edges of a scrolling list say so: a fade of the list's own ground at whichever end
/// has more rows behind it. `scroll` is the offset and the content height.
pub(super) fn edge_fades(
    painter: &egui::Painter,
    rect: Rect,
    ground: Color32,
    depth: f32,
    scroll: (f32, f32),
) {
    let (offset, content_h) = scroll;
    if offset > 1.0 {
        fade(
            painter,
            Rect::from_min_size(rect.min, Vec2::new(rect.width(), depth)),
            ground,
            true,
        );
    }
    if offset + rect.height() + 1.0 < content_h {
        fade(
            painter,
            Rect::from_min_size(
                Pos2::new(rect.min.x, rect.max.y - depth),
                Vec2::new(rect.width(), depth),
            ),
            ground,
            false,
        );
    }
}
/// The width of `text` on one line in `font`.
pub(super) fn text_width(ui: &egui::Ui, text: &str, font: &egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER)
        .size()
        .x
}
/// One line, cut with an ellipsis where it would run past `max_width`.
pub(super) fn truncated(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    colour: Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = TextWrapping::truncate_at_width(max_width.max(8.0));
    ui.painter().layout_job(job)
}
/// A band of `colour` fading to clear — solid at the top when `from_top`, at the bottom when
/// not — as a two-triangle mesh, the way the chip lane fades.
pub(super) fn fade(painter: &egui::Painter, band: Rect, colour: Color32, from_top: bool) {
    if band.height() <= 0.0 || band.width() <= 0.0 {
        return;
    }
    let clear = Color32::TRANSPARENT;
    let (top, bottom) = if from_top {
        (colour, clear)
    } else {
        (clear, colour)
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(band.left_top(), top);
    mesh.colored_vertex(band.right_top(), top);
    mesh.colored_vertex(band.right_bottom(), bottom);
    mesh.colored_vertex(band.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}
