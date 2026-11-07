//! Popup for `Dropdown<'a>`.

use super::{
    bottom_corners, builtin, dismissed, divider, edge_fades, icon_at, no_match_row, paint_panel,
    panel, place_list, round_u8, row_corners, scroll_rows, search_edit, sheet_close, shield,
    text_width, top_corners, truncated, CornerRadius, Cx, Dropdown, DropdownPart, Face, FieldLook,
    GridShape, Ink, Open, Pick, Placement, Pos2, PressedIf, Rect, RowSlot, Sense, Skin, Stroke,
    StrokeKind, Tokens, Trigger, ValueSlot, Vec2, FADE_ROWS, GRID_FRACTION, HINT_FRACTION,
    LIST_FRACTION, MAX_ROWS, SHEET_FRACTION,
};

impl Dropdown<'_> {
    // ── The openers ────────────────────────────────────────────────────────────────────────

    /// The widest row's text, in the body face: the option and, where it has one, its hint.
    pub(super) fn widest_row(&self, ui: &egui::Ui, t: &Tokens) -> f32 {
        self.options
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let option = text_width(ui, o, &t.body);
                self.hint(i)
                    .map_or(option, |h| option + t.gap + text_width(ui, h, &t.body))
            })
            .fold(0.0, f32::max)
    }
    /// The width an anchored list wants: as wide as the widest row asks, within the bound — the
    /// trigger's width is a floor, not the list's size, and an ellipsis is the last resort rather
    /// than the first.
    pub(super) fn list_width(&self, ui: &egui::Ui, t: &Tokens, place: &Placement) -> f32 {
        let text_x = t.inset + t.mark + t.gap;
        (self.widest_row(ui, t) + text_x + t.inset)
            .max(place.trigger.width())
            .min(place.bounds.width())
    }
    /// **The look the open panel wears** — the trigger's own container, so the panel and the
    /// trigger make one silhouette. `list` is where the rows go, under or over the
    /// trigger.
    pub(super) fn skin(&self, t: &Tokens, ink: Ink, trigger: Rect, list: Rect) -> Skin {
        let below = list.min.y >= trigger.max.y - 0.5;
        let radius = round_u8(match self.trigger {
            Trigger::Tile => t.card_radius,
            _ => t.control_radius,
        });
        let all = CornerRadius::same(radius);
        let top = CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        };
        let edge = Stroke::new(t.stroke_edge, ink.edge);
        // An open field is a field being written to: its edge is the focus ring, and the ring
        // runs round the whole of what it opened into.
        let focus = Stroke::new(t.stroke_mark, ink.focus);
        let (corners, ground, stroke, underline) = match self.trigger {
            Trigger::Button | Trigger::Tile => (all, ink.list, Some(edge), None),
            Trigger::Field(FieldLook::Outlined) => (all, ink.field, Some(focus), None),
            // Material's filled field: rounded where it meets the page, square where it meets
            // its own line — and the line is now under the last row.
            Trigger::Field(FieldLook::Filled) => (top, ink.list, None, Some(focus)),
            Trigger::Field(FieldLook::Underlined) => {
                (CornerRadius::ZERO, ink.list, None, Some(focus))
            }
            Trigger::Inline | Trigger::Chip => (all, ink.list, None, None),
        };
        let (head, tail) = if below {
            (top_corners(corners), bottom_corners(corners))
        } else {
            (bottom_corners(corners), top_corners(corners))
        };
        Skin {
            rect: trigger.union(list),
            corners,
            ground,
            stroke,
            underline,
            head,
            tail,
            below,
        }
    }
    /// The trigger drawn again as the panel's first row, over the page's own copy: the same
    /// words on the panel's ground, the chevron turned over, and a tap on it closes the panel as
    /// a tap on the trigger always did. Returns whether it was tapped.
    pub(super) fn head(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        open: &Open,
    ) -> bool {
        let Open { skin, trigger, ink } = *open;
        let rect = skin.head_rect(trigger);
        let response = ui.interact(rect, id.with("head"), Sense::click());
        let pressed = response.is_pointer_button_down_on();
        let head = DropdownPart::Trigger {
            open: true,
            pressed,
            head: true,
        };
        if self.painted(ui.painter(), cx, head, rect, &mut skin.ground.clone()) {
            return response.clicked();
        }
        if pressed && !matches!(self.trigger, Trigger::Chip) {
            ui.painter().rect_filled(rect, skin.head, ink.pressed);
        }
        let face = Face {
            rect,
            open: true,
            pressed,
            container: false,
            corners: Some(skin.head),
        };
        self.paint_trigger(ui, cx, t, face, ink);
        divider(ui.painter(), t, &skin, rect, ink);
        response.clicked()
    }
    /// The head as a search field: the label where the style keeps it, the query typed where the
    /// value was — the value itself is the field's hint, so what is chosen now stays readable
    /// while another is looked for — and the search glyph in the chevron's place.
    pub(super) fn search_head(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        open: &Open,
        query: &mut String,
    ) {
        let Open { skin, trigger, ink } = *open;
        let rect = skin.head_rect(trigger);
        let chip = matches!(self.trigger, Trigger::Chip);
        let head = DropdownPart::Trigger {
            open: true,
            pressed: false,
            head: true,
        };
        // A painter draws the head; the query is typed into the `TextEdit` over it either way.
        let painted = self.painted(ui.painter(), cx, head, rect, &mut skin.ground.clone());
        if chip && !painted {
            ui.painter().rect_filled(rect, skin.head, ink.mark);
        }
        let text = if chip { ink.on_mark } else { ink.label };
        let slot = self.value_slot(ui, t, rect, (ink, !painted));
        if !painted {
            icon_at(
                ui,
                cx,
                &builtin::SEARCH,
                Pos2::new(slot.mark_x, slot.line.center().y),
                t.mark,
                if chip { ink.on_mark } else { ink.chevron },
            );
        }
        let edit = Rect::from_min_max(
            Pos2::new(slot.left, slot.line.min.y),
            Pos2::new(slot.mark_x - t.mark * 0.5 - t.gap, slot.line.max.y),
        );
        search_edit(ui, t, id, edit, query, self.shown(), text);
        if !painted {
            divider(ui.painter(), t, &skin, rect, ink);
        }
    }
    /// Where the value goes on a head of this trigger's style, with the label painted where the
    /// style keeps it — `paint` false leaves it to a painter: the line the value sits on, the x
    /// it starts at, and the centre of the mark at the right.
    pub(super) fn value_slot(
        &self,
        ui: &egui::Ui,
        t: &Tokens,
        rect: Rect,
        (ink, paint): (Ink, bool),
    ) -> ValueSlot {
        let chip = matches!(self.trigger, Trigger::Chip);
        let hint = if chip { ink.on_mark } else { ink.hint };
        let (pad, font) = match self.trigger {
            Trigger::Inline => (t.inset * 0.5, &t.body),
            Trigger::Chip => (t.control_radius, &t.button),
            Trigger::Button | Trigger::Field(_) | Trigger::Tile => (t.inset, &t.body),
        };
        let mark_x = match self.trigger {
            Trigger::Inline | Trigger::Chip => rect.max.x - pad - t.mark * 0.5,
            Trigger::Button | Trigger::Field(_) | Trigger::Tile => {
                rect.max.x - t.inset * 0.5 - t.mark * 0.5
            }
        };
        let max_width = pad.mul_add(-2.0, rect.width()) - t.mark - t.gap;
        let mut left = rect.min.x + pad;
        let line = match self.trigger {
            Trigger::Field(_) => {
                let top = t.inset * 0.5;
                let mut band = rect;
                if let Some(label) = self.label {
                    if paint {
                        let galley = truncated(ui, label, t.small.clone(), hint, max_width);
                        ui.painter()
                            .galley(Pos2::new(left, rect.min.y + top), galley, hint);
                    }
                    band.min.y = rect.min.y + top + t.small_h;
                }
                band
            }
            Trigger::Tile => {
                if let Some(label) = self.label.filter(|_| paint) {
                    let galley = truncated(ui, label, t.small.clone(), hint, max_width);
                    ui.painter()
                        .galley(Pos2::new(left, rect.min.y + t.inset), galley, hint);
                }
                Rect::from_min_max(
                    Pos2::new(rect.min.x, rect.max.y - t.inset - t.heading_h),
                    Pos2::new(rect.max.x, rect.max.y - t.inset),
                )
            }
            Trigger::Button | Trigger::Inline | Trigger::Chip => {
                if let Some(label) = self.label {
                    let galley = truncated(ui, label, font.clone(), hint, max_width * 0.5);
                    let size = galley.size();
                    if paint {
                        ui.painter().galley(
                            Pos2::new(left, rect.center().y - size.y * 0.5),
                            galley,
                            hint,
                        );
                    }
                    left += size.x + t.gap;
                }
                rect
            }
        };
        ValueSlot { line, left, mark_x }
    }
    /// The anchored list. Returns what the frame decided.
    pub(super) fn list(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let wanted = t.target * f32::from(self.rows());
        let width = self.list_width(ui, t, &place);
        let (rows, scrolls) = place_list(place.trigger, place.bounds, wanted, width, LIST_FRACTION);
        let skin = self.skin(t, ink, place.trigger, rows);
        let open = Open {
            skin,
            trigger: place.trigger,
            ink,
        };

        // The shield first, so the panel is the one on top.
        let shield_tapped = shield(ui, id, place.bounds, None);
        let shown = place
            .bounds
            .intersect(skin.unrolled(place.trigger, place.unroll));
        let mut pick = Pick::None;
        let last = self.options.len().saturating_sub(1);
        panel(ui, id, skin.rect, shown, |ui| {
            let ground = self.ground(ui, cx, &skin, shown, place.unroll);
            if self.head(ui, cx, t, id, &open) {
                pick = Pick::Dismissed;
            }
            let out = scroll_rows(ui, id, rows, scrolls, |ui| {
                for index in 0..self.options.len() {
                    let slot = RowSlot {
                        width: rows.width(),
                        index,
                        corners: row_corners(index, last, skin.tail),
                    };
                    if self.row(ui, cx, t, slot, ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), rows, ground, t.target * FADE_ROWS, out);
            }
        });
        dismissed(ui, pick, shield_tapped, &place)
    }
    /// The grid: finger-sized cells in a near-square block, anchored like the list.
    pub(super) fn grid(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let widest = self
            .options
            .iter()
            .map(|o| text_width(ui, o, &t.body))
            .fold(0.0, f32::max);
        let cell_w = t.gap.mul_add(2.0, widest).max(t.target);
        let mut shape = GridShape::fit(self.rows(), cell_w, t.target, t.gap, place.bounds.width());
        // Under a trigger wider than the block, the cells widen to fill it: the block then lines
        // up with what opened it instead of hanging off its left end.
        shape.stretch_to(place.trigger.width());
        let width = shape.width().min(place.bounds.width());
        let (rows, scrolls) = place_list(
            place.trigger,
            place.bounds,
            shape.height(),
            width,
            GRID_FRACTION,
        );
        let skin = self.skin(t, ink, place.trigger, rows);
        let open = Open {
            skin,
            trigger: place.trigger,
            ink,
        };

        let shield_tapped = shield(ui, id, place.bounds, None);
        let shown = place
            .bounds
            .intersect(skin.unrolled(place.trigger, place.unroll));
        let mut pick = Pick::None;
        panel(ui, id, skin.rect, shown, |ui| {
            let ground = self.ground(ui, cx, &skin, shown, place.unroll);
            if self.head(ui, cx, t, id, &open) {
                pick = Pick::Dismissed;
            }
            let out = scroll_rows(ui, id, rows, scrolls, |ui| {
                let (block, _) =
                    ui.allocate_exact_size(Vec2::new(rows.width(), shape.height()), Sense::hover());
                for index in 0..self.options.len().min(usize::from(MAX_ROWS)) {
                    let cell = shape.cell(block.min, index);
                    if self.grid_cell(ui, cx, t, id, (cell, index), ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), rows, ground, t.target * FADE_ROWS, out);
            }
        });
        dismissed(ui, pick, shield_tapped, &place)
    }
    /// One cell: the chosen one wears the accent, the rest the control face. Returns whether it
    /// was picked.
    pub(super) fn grid_cell(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        (cell, index): (Rect, usize),
        ink: Ink,
    ) -> bool {
        let response = ui.interact(cell, id.with(("cell", index)), Sense::click());
        let on = index == *self.selected;
        let part = DropdownPart::Option {
            index,
            selected: on,
            pressed: response.is_pointer_button_down_on(),
            cell: true,
        };
        if self.painted(ui.painter(), cx, part, cell, &mut ink.list.clone()) {
            return response.clicked();
        }
        let radius = CornerRadius::same(round_u8(t.control_radius));
        let fill = if on { ink.mark } else { ink.face };
        let painter = ui.painter();
        painter.rect_filled(
            cell,
            radius,
            fill.pressed_if(response.is_pointer_button_down_on(), ink),
        );
        if !on {
            painter.rect_stroke(
                cell,
                radius,
                Stroke::new(t.stroke_edge, ink.edge),
                StrokeKind::Inside,
            );
        }
        let colour = if on { ink.on_mark } else { ink.label };
        let galley = truncated(
            ui,
            self.options.get(index).copied().unwrap_or_default(),
            t.body.clone(),
            colour,
            t.gap.mul_add(-2.0, cell.width()),
        );
        let size = galley.size();
        ui.painter()
            .galley(cell.center() - size * 0.5, galley, colour);
        response.clicked()
    }
    /// The sheet: full width at the bottom of the pane, a scrim over the rest.
    pub(super) fn sheet(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let bounds = place.bounds;
        let wanted = t.target * (f32::from(self.rows()) + 1.0);
        let height = wanted.min(bounds.height() * SHEET_FRACTION).max(0.0);
        let scrolls = height + 0.5 < wanted;
        let rect = Rect::from_min_max(Pos2::new(bounds.min.x, bounds.max.y - height), bounds.max);
        let k = place.unroll.clamp(0.0, 1.0);
        // Slid up from below the pane's edge rather than clipped: a sheet moves as one thing.
        let shown = rect.translate(Vec2::new(0.0, height * (1.0 - k)));
        let radius = round_u8(t.card_radius);
        let corners = CornerRadius {
            nw: radius,
            ne: radius,
            sw: 0,
            se: 0,
        };

        let shield_tapped = shield(ui, id, bounds, Some(ink.scrim.gamma_multiply(k)));
        let mut pick = Pick::None;
        panel(ui, id, shown, bounds, |ui| {
            let mut ground = ink.list;
            let part = DropdownPart::Panel {
                shown,
                unroll: k,
                close: Some(sheet_close(shown, t)),
            };
            let painted = self.painted(ui.painter(), cx, part, shown, &mut ground);
            if !painted {
                paint_panel(ui.painter(), cx, shown, corners, ink.list);
            }
            if self.sheet_header(ui, cx, t, id, (shown, painted), ink) {
                pick = Pick::Dismissed;
            }
            let body =
                Rect::from_min_max(Pos2::new(shown.min.x, shown.min.y + t.target), shown.max);
            let out = scroll_rows(ui, id, body, scrolls, |ui| {
                for index in 0..self.options.len() {
                    let slot = RowSlot {
                        width: body.width(),
                        index,
                        corners: CornerRadius::ZERO,
                    };
                    if self.row(ui, cx, t, slot, ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), body, ground, t.target * FADE_ROWS, out);
            }
        });
        dismissed(ui, pick, shield_tapped, &place)
    }
    /// The sheet's first row: its title at the left, a close button at the right — drawn here
    /// unless a painter drew the sheet (`painted`), pressed either way. Returns whether the close
    /// was pressed.
    pub(super) fn sheet_header(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        (sheet, painted): (Rect, bool),
        ink: Ink,
    ) -> bool {
        let header = Rect::from_min_size(sheet.min, Vec2::new(sheet.width(), t.target));
        let close = sheet_close(sheet, t);
        let response = ui.interact(close, id.with("close"), Sense::click());
        if painted {
            return response.clicked();
        }
        if response.is_pointer_button_down_on() {
            ui.painter().rect_filled(
                close,
                CornerRadius::same(round_u8(t.control_radius)),
                ink.pressed,
            );
        }
        if let Some(label) = self.label {
            let galley = truncated(
                ui,
                label,
                t.button.clone(),
                ink.label,
                header.width() - t.inset - t.target,
            );
            let size = galley.size();
            ui.painter().galley(
                Pos2::new(header.min.x + t.inset, header.center().y - size.y * 0.5),
                galley,
                ink.label,
            );
        }
        icon_at(ui, cx, &builtin::CLOSE, close.center(), t.mark, ink.chevron);
        // The line under the header: the title is not a row.
        let y = header.max.y - t.hairline * 0.5;
        ui.painter().line_segment(
            [Pos2::new(header.min.x, y), Pos2::new(header.max.x, y)],
            Stroke::new(t.hairline, ink.outline),
        );
        response.clicked()
    }
    /// The search: the head turned into a field, the matches under it in the same panel.
    pub(super) fn search(
        &self,
        ui: &egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        id: egui::Id,
        place: Placement,
        ink: Ink,
    ) -> Pick {
        let query_id = id.with("query");
        let mut query = crate::interaction::load::<String>(
            ui.ctx(),
            query_id,
            self.enabled && ui.is_enabled(),
            crate::interaction::Lifetime::Mounted,
        )
        .unwrap_or_default();
        let matches = self.matches(&query);
        let shown_rows = u16::try_from(matches.len())
            .unwrap_or(u16::MAX)
            .clamp(1, MAX_ROWS);
        let wanted = t.target * f32::from(shown_rows);
        let width = self.list_width(ui, t, &place);
        let (rows, scrolls) = place_list(place.trigger, place.bounds, wanted, width, LIST_FRACTION);
        let skin = self.skin(t, ink, place.trigger, rows);
        let open = Open {
            skin,
            trigger: place.trigger,
            ink,
        };

        let shield_tapped = shield(ui, id, place.bounds, None);
        let shown = place
            .bounds
            .intersect(skin.unrolled(place.trigger, place.unroll));
        let mut pick = Pick::None;
        let last = matches.len().saturating_sub(1);
        panel(ui, id, skin.rect, shown, |ui| {
            let ground = self.ground(ui, cx, &skin, shown, place.unroll);
            self.search_head(ui, cx, t, id, &open, &mut query);
            let out = scroll_rows(ui, id, rows, scrolls, |ui| {
                if matches.is_empty() {
                    no_match_row(ui, t, rows.width(), ink, self.no_match);
                }
                for (n, index) in matches.iter().copied().enumerate() {
                    let slot = RowSlot {
                        width: rows.width(),
                        index,
                        corners: row_corners(n, last, skin.tail),
                    };
                    if self.row(ui, cx, t, slot, ink) {
                        pick = Pick::At(index);
                    }
                }
            });
            if scrolls {
                edge_fades(ui.painter(), rows, ground, t.target * FADE_ROWS, out);
            }
        });
        crate::interaction::store(ui.ctx(), query_id, query);
        dismissed(ui, pick, shield_tapped, &place)
    }
    /// The options that contain `query`, case-insensitively, in their own order. An empty query
    /// matches everything.
    pub(super) fn matches(&self, query: &str) -> Vec<usize> {
        let needle = query.trim().to_lowercase();
        self.options
            .iter()
            .enumerate()
            .filter(|(_, o)| needle.is_empty() || o.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .take(usize::from(MAX_ROWS))
            .collect()
    }
    /// One option: a tick where it is the chosen one, the text, and its hint at the right.
    /// Returns whether it was picked.
    pub(super) fn row(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        t: &Tokens,
        slot: RowSlot,
        ink: Ink,
    ) -> bool {
        let RowSlot {
            width,
            index,
            corners,
        } = slot;
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, t.target), Sense::click());
        let on = index == *self.selected;
        let part = DropdownPart::Option {
            index,
            selected: on,
            pressed: response.is_pointer_button_down_on(),
            cell: false,
        };
        if self.painted(ui.painter(), cx, part, rect, &mut ink.list.clone()) {
            return response.clicked();
        }
        if response.is_pointer_button_down_on() {
            // The first and last rows round the corners they share with the list, so the
            // pressed fill does not poke out of it.
            ui.painter().rect_filled(rect, corners, ink.pressed);
        }
        if on {
            icon_at(
                ui,
                cx,
                &builtin::CHECK,
                Pos2::new(rect.min.x + t.inset + t.mark * 0.5, rect.center().y),
                t.mark,
                ink.mark,
            );
        }
        // Every row leaves room for the tick whether it wears one or not, so the list does not
        // shuffle sideways as the choice moves.
        let text_x = rect.min.x + t.inset + t.mark + t.gap;
        let mut right = rect.max.x - t.inset;
        if let Some(hint) = self.hint(index) {
            let galley = truncated(ui, hint, t.body.clone(), ink.hint, width * HINT_FRACTION);
            let size = galley.size();
            right -= size.x;
            ui.painter().galley(
                Pos2::new(right, rect.center().y - size.y * 0.5),
                galley,
                ink.hint,
            );
            right -= t.gap;
        }
        let colour = if on { ink.mark } else { ink.label };
        let galley = truncated(
            ui,
            self.options.get(index).copied().unwrap_or_default(),
            t.body.clone(),
            colour,
            right - text_x,
        );
        let size = galley.size();
        ui.painter().galley(
            Pos2::new(text_x, rect.center().y - size.y * 0.5),
            galley,
            colour,
        );
        response.clicked()
    }
}
