//! Trigger paint for `Dropdown<'a>`.

use super::{
    chevron, chevron_at, paint_skin, paint_words, round_u8, text_width, truncated, Color32,
    CornerRadius, Cx, Dropdown, DropdownLook, DropdownPart, Elevation, Face, FieldLook, Ink, Pos2,
    PressedIf, Rect, Skin, Stroke, StrokeKind, Tokens, Trigger, Vec2, Words, MAX_ROWS,
};

impl Dropdown<'_> {
    /// Hand `part` at `rect` to the dropdown painter, where there is one, and say
    /// whether it drew it. `ground` is the panel's: it goes in as the built-in one and comes back
    /// as the painter's, for the fades of a list that scrolls.
    pub(super) fn painted(
        &self,
        painter: &egui::Painter,
        cx: &mut Cx<'_>,
        part: DropdownPart,
        rect: Rect,
        ground: &mut Color32,
    ) -> bool {
        let Some(custom) = cx.painters.as_deref_mut().and_then(|p| p.dropdown.as_mut()) else {
            return false;
        };
        let (text, hint) = match part {
            DropdownPart::Option { index, .. } => (
                self.options.get(index).copied().unwrap_or_default(),
                self.hint(index),
            ),
            _ => (self.shown(), None),
        };
        let mut look = DropdownLook {
            part,
            rect,
            trigger: self.trigger,
            opener: self.opener,
            label: self.label,
            text,
            hint,
            enabled: self.enabled,
            ground: *ground,
            theme: cx.theme,
            icons: &mut *cx.icons,
        };
        custom(painter, &mut look);
        *ground = look.ground;
        true
    }
    /// The open panel's ground — the skin, or a painter's — and the colour it is, which the list
    /// fades into. `shown` is what shows of it, `unroll` how far it has opened.
    pub(super) fn ground(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        skin: &Skin,
        shown: Rect,
        unroll: f32,
    ) -> Color32 {
        let mut ground = skin.ground;
        let part = DropdownPart::Panel {
            shown,
            unroll,
            close: None,
        };
        if !self.painted(ui.painter(), cx, part, skin.rect, &mut ground) {
            paint_skin(ui.painter(), cx, skin);
        }
        ground
    }
    /// The chosen option's text, or an em dash where the index has run off the end.
    pub(super) fn shown(&self) -> &str {
        self.options.get(*self.selected).copied().unwrap_or("—")
    }
    /// The hint on row `index`, where there is one and it says something.
    pub(super) fn hint(&self, index: usize) -> Option<&str> {
        self.hints.get(index).copied().filter(|h| !h.is_empty())
    }
    /// How many rows any opener lays out.
    pub(super) fn rows(&self) -> u16 {
        u16::try_from(self.options.len())
            .unwrap_or(u16::MAX)
            .min(MAX_ROWS)
    }
    // ── The trigger ────────────────────────────────────────────────────────────────────────

    /// The closed control's size. A button, a field and a tile take the width they are given; a
    /// chip and an inline value take what their words need.
    pub(super) fn trigger_size(&self, ui: &egui::Ui, t: &Tokens) -> Vec2 {
        let avail = ui.available_width();
        match self.trigger {
            Trigger::Button => Vec2::new(avail, t.target),
            Trigger::Field(_) => {
                let h = if self.label.is_some() {
                    t.target + t.small_h
                } else {
                    t.target
                };
                Vec2::new(avail, h)
            }
            Trigger::Tile => {
                let inner = t.small_h + t.line_gap + t.heading_h;
                Vec2::new(avail, t.tile.max(t.inset.mul_add(2.0, inner)))
            }
            Trigger::Inline => {
                let words = self.words_width(ui, t, &t.body);
                let w = t.inset.mul_add(0.5, words) + t.gap + t.mark + t.inset * 0.5;
                Vec2::new(w.min(avail), t.target)
            }
            Trigger::Chip => {
                let words = self.words_width(ui, t, &t.button);
                // A chip's side padding is its radius, as `Chip` has it.
                let w = t.control_radius.mul_add(2.0, words) + t.gap + t.mark;
                Vec2::new(w.min(avail), t.target)
            }
        }
    }
    /// The width of the label (where there is one) and the value, in `font`, with the gap
    /// between them.
    pub(super) fn words_width(&self, ui: &egui::Ui, t: &Tokens, font: &egui::FontId) -> f32 {
        let value = text_width(ui, self.shown(), font);
        self.label
            .map_or(value, |l| text_width(ui, l, font) + t.gap + value)
    }
    pub(super) fn paint_trigger(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
    ) {
        match self.trigger {
            Trigger::Button => self.paint_button(ui, cx, t, face, ink),
            Trigger::Field(look) => self.paint_field(ui, cx, t, face, ink, look),
            Trigger::Inline => self.paint_inline(ui, cx, t, face, ink),
            Trigger::Tile => self.paint_tile(ui, cx, t, face, ink),
            Trigger::Chip => self.paint_chip(ui, cx, t, face, ink),
        }
    }
    /// The current value on a button face, and a chevron that turns over when it is open.
    pub(super) fn paint_button(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
    ) {
        if face.container {
            let radius = CornerRadius::same(round_u8(t.control_radius));
            let painter = ui.painter();
            painter.rect_filled(face.rect, radius, ink.face.pressed_if(face.pressed, ink));
            painter.rect_stroke(
                face.rect,
                radius,
                Stroke::new(t.stroke_edge, ink.edge),
                StrokeKind::Inside,
            );
        }
        let words = Words {
            at: Pos2::new(face.rect.min.x + t.inset, face.rect.center().y),
            gap: t.gap,
            max_width: t.inset.mul_add(-1.5, face.rect.width()) - t.mark - t.gap,
            font: &t.body,
            label: self.label,
            value: self.shown(),
            value_colour: ink.label,
        };
        paint_words(ui, words, ink);
        chevron(ui, cx, t, face, ink.chevron);
    }
    /// A form field: the label floats small at the top, the value sits under it.
    pub(super) fn paint_field(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
        look: FieldLook,
    ) {
        let rect = face.rect;
        let radius = round_u8(t.control_radius);
        let (fill, corners) = match look {
            FieldLook::Outlined => (ink.field, CornerRadius::same(radius)),
            // Material's filled field: rounded where it meets the page, square where it meets
            // its own line.
            FieldLook::Filled => (
                ink.face,
                CornerRadius {
                    nw: radius,
                    ne: radius,
                    sw: 0,
                    se: 0,
                },
            ),
            FieldLook::Underlined => (Color32::TRANSPARENT, CornerRadius::ZERO),
        };
        if face.container {
            let painter = ui.painter();
            if fill != Color32::TRANSPARENT || face.pressed {
                painter.rect_filled(rect, corners, fill.pressed_if(face.pressed, ink));
            }
            // Open, the edge is the focus ring: an open field is a field being written to.
            let edge = if face.open {
                Stroke::new(t.stroke_mark, ink.focus)
            } else {
                Stroke::new(t.stroke_edge, ink.outline)
            };
            match look {
                FieldLook::Outlined => {
                    painter.rect_stroke(rect, corners, edge, StrokeKind::Inside);
                }
                FieldLook::Filled | FieldLook::Underlined => {
                    let y = rect.max.y - edge.width * 0.5;
                    painter
                        .line_segment([Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)], edge);
                }
            }
        }
        let pad = t.inset * 0.5;
        let max_width = t.inset.mul_add(-1.5, rect.width()) - t.mark - t.gap;
        let mut band = rect;
        if let Some(label) = self.label {
            let galley = truncated(ui, label, t.small.clone(), ink.hint, max_width);
            ui.painter().galley(
                Pos2::new(rect.min.x + t.inset, rect.min.y + pad),
                galley,
                ink.hint,
            );
            band.min.y = rect.min.y + pad + t.small_h;
        }
        let words = Words {
            at: Pos2::new(rect.min.x + t.inset, band.center().y),
            gap: t.gap,
            max_width,
            font: &t.body,
            label: None,
            value: self.shown(),
            value_colour: ink.label,
        };
        paint_words(ui, words, ink);
        chevron(ui, cx, t, face, ink.chevron);
    }
    /// The value in running text: no face, only the pressed tint while it is held.
    pub(super) fn paint_inline(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
    ) {
        if face.pressed && face.container {
            let radius = CornerRadius::same(round_u8(t.control_radius));
            ui.painter().rect_filled(face.rect, radius, ink.pressed);
        }
        let pad = t.inset * 0.5;
        let words = Words {
            at: Pos2::new(face.rect.min.x + pad, face.rect.center().y),
            gap: t.gap,
            max_width: pad.mul_add(-2.0, face.rect.width()) - t.mark - t.gap,
            font: &t.body,
            label: self.label,
            value: self.shown(),
            value_colour: ink.label,
        };
        paint_words(ui, words, ink);
        chevron_at(
            ui,
            cx,
            t,
            Pos2::new(face.rect.max.x - pad - t.mark * 0.5, face.rect.center().y),
            face.open,
            ink.chevron,
        );
    }
    /// A caption over a value set in the heading size, on a raised tile.
    pub(super) fn paint_tile(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
    ) {
        let rect = face.rect;
        if face.container {
            let radius = CornerRadius::same(round_u8(t.card_radius));
            crate::theme::paint_elevation(ui.painter(), cx.theme, rect, radius, Elevation::Raised);
            let painter = ui.painter();
            painter.rect_filled(rect, radius, ink.face.pressed_if(face.pressed, ink));
            painter.rect_stroke(
                rect,
                radius,
                Stroke::new(t.stroke_edge, ink.edge),
                StrokeKind::Inside,
            );
        }
        let max_width = t.inset.mul_add(-1.5, rect.width()) - t.mark - t.gap;
        if let Some(label) = self.label {
            let galley = truncated(ui, label, t.small.clone(), ink.hint, max_width);
            ui.painter().galley(
                Pos2::new(rect.min.x + t.inset, rect.min.y + t.inset),
                galley,
                ink.hint,
            );
        }
        let value = truncated(ui, self.shown(), t.heading.clone(), ink.label, max_width);
        let size = value.size();
        ui.painter().galley(
            Pos2::new(rect.min.x + t.inset, rect.max.y - t.inset - size.y),
            value,
            ink.label,
        );
        chevron(ui, cx, t, face, ink.chevron);
    }
    /// A chip: the unselected chip's face closed, the selected chip's face while it is open.
    pub(super) fn paint_chip(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        face: Face,
        ink: Ink,
    ) {
        let radius = face
            .corners
            .unwrap_or_else(|| CornerRadius::same(round_u8(t.control_radius)));
        let (fill, text) = if face.open {
            (ink.mark, ink.on_mark)
        } else {
            (ink.face, ink.label)
        };
        // An open chip's accent face is its content — the head of the open panel wears it too.
        if face.container || face.open {
            let painter = ui.painter();
            painter.rect_filled(face.rect, radius, fill.pressed_if(face.pressed, ink));
            if !face.open {
                painter.rect_stroke(
                    face.rect,
                    radius,
                    Stroke::new(t.stroke_edge, ink.edge),
                    StrokeKind::Inside,
                );
            }
        }
        let pad = t.control_radius;
        let words = Words {
            at: Pos2::new(face.rect.min.x + pad, face.rect.center().y),
            gap: t.gap,
            max_width: pad.mul_add(-2.0, face.rect.width()) - t.mark - t.gap,
            font: &t.button,
            label: self.label,
            value: self.shown(),
            value_colour: text,
        };
        // An open chip's muted label would vanish on the accent: the whole line takes the
        // accent's own text colour.
        let mut ink = ink;
        if face.open {
            ink.hint = ink.on_mark;
        }
        paint_words(ui, words, ink);
        chevron_at(
            ui,
            cx,
            t,
            Pos2::new(face.rect.max.x - pad - t.mark * 0.5, face.rect.center().y),
            face.open,
            if face.open { ink.on_mark } else { ink.chevron },
        );
    }
}
