//! Rail render for `DesktopView`.

use super::{
    paint_rail_item, rail_radius, Access, ColorRole, DesktopAction, DesktopCtx, DesktopView, Rect,
    Visibility, RAIL_GAP,
};

impl DesktopView {
    /// The **first openable** icon id on the rail.
    ///
    /// In a rail layout the state of being at home is awkward — there is no kiosk with only a menu and
    /// nothing beside it. The shell uses this to open the first screen automatically.
    #[must_use]
    pub(crate) fn first_rail_entry(&self, access: &Access) -> Option<&str> {
        self.entries
            .iter()
            .map(|e| &e.slot)
            .find(|slot| slot.gate.as_ref().is_none_or(|gate| access.allows(gate)))
            .map(|slot| slot.id.as_str())
    }
    /// **The icon rail** — the icons drawn in one vertical column (the kiosk layout).
    ///
    /// # What is different from the grid
    ///
    /// [`DesktopView::ui`]'s grid is the home screen, and an opened screen **covers** it (A2). The
    /// rail is the opposite — the shell draws it **every frame** and a screen opens only in the area
    /// left over ([`Layout::rail`](crate::shell::Layout::rail)). The menu down the left of a shop kiosk
    /// is exactly this shape: the categories are always in view and only the content changes.
    ///
    /// So there is no wallpaper, no page turning and no dock. It draws as many as fit in one column and
    /// scrolls vertically past that.
    ///
    /// `open` is the declaration id of the screen open right now — that entry is highlighted. Without
    /// seeing which of the list is open, a rail is just a row of buttons.
    pub(crate) fn rail_ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        open: Option<&str>,
        cx: &mut DesktopCtx<'_>,
    ) -> Option<DesktopAction> {
        let m = cx.theme.metrics;
        ui.painter()
            .rect_filled(rect, 0.0, cx.theme.color(ColorRole::Surface));
        // One solid line between the rail and the content. Colour alone does not divide the two areas —
        // depending on the palette, Surface and Background can be close (as happened with the settings cards).
        let border_x = if rect.center().x < ui.max_rect().center().x {
            rect.max.x
        } else {
            rect.min.x
        };
        ui.painter().vline(
            border_x,
            rect.top()..=rect.bottom(),
            egui::Stroke::new(
                cx.theme.control.stroke_hairline,
                cx.theme.color(ColorRole::Outline),
            ),
        );

        let mut action = None;
        let inset = m.screen_inset;
        let item_h = m.icon_cell.max(m.touch_target);
        let gap = m.row_height * RAIL_GAP;
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink2(egui::vec2(inset * 0.5, 0.0)))
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );
        // The rail is drawn on the shell's own layer, which `layer_id_at` may not count as an area:
        // a long press is the rail's where nothing else is over the spot.
        let layer = ui.layer_id();
        let held = self.held.filter(|pos| {
            !self.spent
                && rect.contains(*pos)
                && ui.ctx().layer_id_at(*pos).is_none_or(|at| at == layer)
        });
        let spent = self.spent;
        let mut long = None;
        let Self { entries, info, .. } = self;
        egui::ScrollArea::vertical()
            .id_salt("fairing.desktop.rail")
            .max_height(rect.height())
            .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                ui.add_space(gap);
                for entry in entries.iter() {
                    let slot = &entry.slot;
                    let allowed = slot.gate.as_ref().is_none_or(|gate| cx.access.allows(gate));
                    if !allowed && slot.visibility == Visibility::Hidden {
                        continue;
                    }
                    let (item, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), item_h),
                        egui::Sense::click(),
                    );
                    info.see(&slot.id, item);
                    if response.clicked() && !spent {
                        action = Some(if allowed {
                            DesktopAction::Tap(slot.id.clone())
                        } else {
                            DesktopAction::TapLocked(slot.id.clone())
                        });
                    }
                    if held.is_some_and(|pos| item.contains(pos)) {
                        long = Some(item);
                        action = Some(DesktopAction::LongPress(slot.id.clone()));
                    }
                    let is_open = open == Some(slot.id.as_str());
                    if is_open || (response.is_pointer_button_down_on() && !spent) {
                        ui.painter().rect_filled(
                            item.shrink(2.0),
                            egui::CornerRadius::same(rail_radius(m.corner_radius)),
                            cx.theme.color(if is_open {
                                ColorRole::SurfaceVariant
                            } else {
                                ColorRole::Pressed
                            }),
                        );
                    }
                    paint_rail_item(ui, item, slot, allowed, is_open, cx);
                    ui.add_space(gap);
                }
            });
        if let (Some(DesktopAction::LongPress(id)), Some(cell)) = (&action, long) {
            self.answer_long_press(id, cell, &cx.theme.motion);
        }
        action
    }
}
