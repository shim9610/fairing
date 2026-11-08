//! Page state for `DesktopView`.

use super::{
    apply_dock_order, apply_overrides, carry_badges, ensure_page, place_auto, slot_entries, Badge,
    DesktopConfig, DesktopPlacement, DesktopView, IconRecord, IconSlot, Metrics, MotionTokens,
    Page, PageDrag, PageSwipe, Rect, Registry, MAX_COLUMNS, MAX_ROWS,
};

impl DesktopView {
    /// Rebuild the pages and the dock from the registry's declarations plus the config overrides.
    ///
    /// The order is ① build the candidates from the declarations (screens then actions, only those
    /// with an icon), ② lay the `[[desktop.pages]]` overrides (label, icon, `locked`, `col`/`row`) on
    /// top, ③ settle the dock from the `desktop.dock` list and the `.dock()` declarations, then ④
    /// place what is left, positioned ones first and then in declaration order. An id that went into
    /// the dock is not placed on the grid again (the same id drawn twice makes `icon_rect`
    /// ambiguous). An id that was never declared warns and is ignored.
    pub(crate) fn rebuild(&mut self, registry: &Registry, cfg: &DesktopConfig) {
        let mut entries = slot_entries(registry);
        carry_badges(&self.entries, &mut entries);
        apply_overrides(&mut entries, cfg);
        apply_dock_order(&mut entries, cfg);
        self.entries = entries;
        self.fitted = None;
        self.place_all();
        self.swipe.configure(self.pages.len(), self.swipe_width());
        self.swipe
            .snap(self.page_index.min(self.pages.len().saturating_sub(1)));
    }
    /// Place [`Self::entries`] on the current grid (`columns × rows`). They are placed again from the
    /// original list so that the overrides are not lost when a screen size change alters the automatic
    /// grid.
    pub(super) fn place_all(&mut self) {
        let Self {
            columns,
            rows,
            entries,
            pages,
            dock,
            page_index,
            records,
            label_width,
            ..
        } = self;
        let (columns, rows) = ((*columns).max(1), (*rows).max(1));
        pages.clear();
        // The placements are values the config and the integrator settled, so a rebuild must not clear them — only the slots are emptied.
        dock.slots.clear();
        for entry in entries.iter_mut() {
            entry.placed = false;
        }
        // 1) The dock (the config list's order, then the `.dock()` declarations' order).
        let mut order: Vec<usize> = (0..entries.len()).collect();
        order.sort_by_key(|i| {
            entries
                .get(*i)
                .and_then(|e| e.dock_order)
                .unwrap_or(usize::MAX)
        });
        for index in order {
            let Some(entry) = entries.get_mut(index) else {
                continue;
            };
            if entry.dock_order.is_some() && dock.push(entry.slot.clone()) {
                entry.placed = true;
            }
        }
        // 2) The positioned ones.
        for entry in entries.iter_mut() {
            if entry.placed {
                continue;
            }
            let DesktopPlacement::At(p) = entry.placement else {
                continue;
            };
            ensure_page(pages, usize::from(p.page), columns, rows);
            let put = pages
                .get_mut(usize::from(p.page))
                .is_some_and(|page| page.put(p.col, p.row, entry.slot.clone()));
            if put {
                entry.placed = true;
            } else {
                log::warn!(
                    "desktop: the slot for `{}` (page {}, col {}, row {}) is taken or outside the grid - placing it automatically",
                    entry.slot.id, p.page, p.col, p.row
                );
            }
        }
        // 3) The rest, in declaration order.
        for entry in entries.iter_mut() {
            if entry.placed || entry.placement == DesktopPlacement::None {
                continue;
            }
            place_auto(
                pages,
                entry.page_hint.unwrap_or(0),
                columns,
                rows,
                entry.slot.clone(),
            );
            entry.placed = true;
        }
        if pages.is_empty() {
            pages.push(Page::new(columns, rows));
        }
        *page_index = (*page_index).min(pages.len() - 1);
        // The record list is built only here (the render path only refreshes values).
        records.clear();
        for slot in pages
            .iter()
            .flat_map(|p| p.slots().map(|(_, _, s)| s))
            .chain(dock.slots.iter())
        {
            records.push(IconRecord {
                id: slot.id.clone(),
                rect: None,
            });
        }
        *label_width = 0.0;
    }
    /// Every page's icons (the dock excluded).
    #[must_use]
    pub fn icons(&self) -> Vec<&IconSlot> {
        self.pages
            .iter()
            .flat_map(|p| p.slots().map(|(_, _, s)| s))
            .collect()
    }
    /// The dock's slots.
    #[must_use]
    pub fn dock(&self) -> &[IconSlot] {
        self.dock.slots()
    }
    /// The pages.
    #[must_use]
    pub fn pages(&self) -> &[Page] {
        &self.pages
    }
    /// The current page.
    #[must_use]
    pub fn page(&self) -> usize {
        self.page_index
    }
    /// Move to a page (out of range is ignored). It settles at once — to go by the spring, [`Self::swipe_to`].
    pub fn set_page(&mut self, index: usize) {
        if index < self.pages.len() {
            self.page_index = index;
            self.swipe.configure(self.pages.len(), self.swipe_width());
            self.swipe.snap(index);
        }
    }
    /// Move to a page by the spring, as an indicator tap does (A4). Under `reduce` it settles at once.
    pub fn swipe_to(&mut self, index: usize, tokens: &MotionTokens) {
        if index < self.pages.len() {
            self.swipe.configure(self.pages.len(), self.swipe_width());
            if tokens.reduce {
                self.swipe.snap(index);
                self.page_index = index;
            } else {
                self.swipe.go_to(index, &tokens.page);
            }
        }
    }
    /// The real page position (A4's `pos`). At rest it is `page() as f32`.
    #[doc(hidden)]
    #[must_use]
    pub fn page_pos(&self) -> f32 {
        self.swipe.pos()
    }
    /// The page swipe's state.
    #[must_use]
    pub fn swipe(&self) -> &PageSwipe {
        &self.swipe
    }
    /// Settle a running page spring at once — called by the shell when it starts something of **higher
    /// priority** (the shade). While a finger is driving the page it does nothing (that
    /// is not the same finger, and the release rules decide it).
    pub(crate) fn settle_page(&mut self) {
        if self.swipe.is_dragging() {
            return;
        }
        let page = self.swipe.page().min(self.pages.len().saturating_sub(1));
        self.swipe.snap(page);
        self.page_index = page;
    }
    /// Frame stage 5: advance the page spring. On settling it brings `page_index` into line. `true` while it is moving.
    pub fn tick(&mut self, dt: f32) -> bool {
        self.info.tick(dt);
        let moving = self.swipe.tick(dt);
        if !moving && !self.swipe.is_dragging() {
            let page = self.swipe.page().min(self.pages.len().saturating_sub(1));
            self.page_index = page;
        }
        moving
    }
    /// One page's width `W` — last frame's grid width (1 before the first frame).
    pub(super) fn swipe_width(&self) -> f32 {
        self.page_width.max(1.0)
    }
    /// The A4 finger binding. Pressed within the grid, with a horizontal movement past the slop and
    /// **greater than the vertical**, it drives the page. `true` while it is active — that frame's icon
    /// presses and taps are cancelled.
    ///
    /// `raw` is [`raw_press`]'s result (where it went down, where it is now). On the frame of the
    /// release it is `None`, and [`PageSwipe::release`] is called there — egui's `velocity()` is still
    /// valid on that frame (the 100 ms window regression).
    pub(super) fn drive_page_swipe(
        &mut self,
        ui: &egui::Ui,
        grid: Rect,
        raw: Option<(egui::Pos2, egui::Pos2)>,
        tokens: &MotionTokens,
    ) -> bool {
        let vx = ui.input(|i| i.pointer.velocity().x);
        let Some((origin, now)) = raw else {
            if self.drag.active {
                let target = self.swipe.release(vx, &tokens.page);
                if tokens.reduce {
                    self.swipe.snap(target);
                }
                self.page_index = target;
            }
            self.drag = PageDrag::default();
            return false;
        };
        if self.drag.origin.is_none() && self.pages.len() > 1 && grid.contains(origin) {
            self.drag.origin = Some(origin);
        }
        let Some(start) = self.drag.origin else {
            return false;
        };
        let delta = now - start;
        if !self.drag.active && delta.x.abs() > tokens.slop_px && delta.x.abs() > delta.y.abs() {
            self.drag.active = true;
            self.swipe.begin();
        }
        if self.drag.active {
            self.swipe.drag(delta.x, vx);
        }
        self.drag.active
    }
    /// The icon Rect drawn last frame (the A2 icon zoom's origin). `None` where it is not on the
    /// current page or the dock, or is `Hidden`. The press scale is not applied, so the A2 origin does
    /// not wobble.
    #[must_use]
    pub fn icon_rect(&self, id: &str) -> Option<Rect> {
        self.records
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.rect)
    }
    /// Whether the press feedback (A7) is running. It becomes `true` on **the very frame** of the
    /// press (the tint comes up at once) and `false` once the scale has come back to 1 after the
    /// release.
    #[must_use]
    pub fn is_pressed(&self) -> bool {
        self.press.key.is_some()
    }
    /// The tap Rect of a page indicator dot drawn last frame. With only one page the indicator is
    /// not drawn, so this returns `None`. Pages also support horizontal swipes.
    #[must_use]
    pub fn page_indicator_rect(&self, index: usize) -> Option<Rect> {
        self.indicator_rects
            .get(index)
            .copied()
            .filter(Rect::is_positive)
    }
    /// Refresh a badge. `false` where there is no such id.
    pub fn set_badge(&mut self, id: &str, badge: Option<&Badge>) -> bool {
        let mut found = false;
        for entry in &mut self.entries {
            if entry.slot.id == id {
                entry.slot.badge = badge.cloned();
                found = true;
            }
        }
        for page in &mut self.pages {
            for slot in page.slots_mut() {
                if slot.id == id {
                    slot.badge = badge.cloned();
                    found = true;
                }
            }
        }
        for slot in &mut self.dock.slots {
            if slot.id == id {
                slot.badge = badge.cloned();
                found = true;
            }
        }
        found
    }
    /// Work the automatic grid (`columns` / `rows = 0`) out from the content's width and height. A
    /// change in the value places everything again from the same declaration list — which happens once,
    /// on a screen rotation or a split.
    ///
    /// One page needs no indicator, so the grid is first fitted to the whole area; only where that
    /// still takes two pages or more is the indicator's room taken out. Fitted the other way round,
    /// a desktop that once had two pages would keep a second page one page could hold.
    pub(super) fn fit_grid(&mut self, rect: Rect, metrics: &Metrics) {
        if !self.auto_columns && !self.auto_rows {
            return;
        }
        let cell = metrics.icon_cell.max(1.0);
        let alone = self.grid_size(self.grid_rect_with(rect, metrics, false), cell);
        let shared = self.grid_size(self.grid_rect_with(rect, metrics, true), cell);
        if self.fitted == Some((alone, shared)) {
            return;
        }
        self.fitted = Some((alone, shared));
        self.set_grid_size(alone);
        if self.pages.len() > 1 {
            self.set_grid_size(shared);
        }
    }
    /// The automatic grid's `(columns, rows)` on `grid`, a fixed count kept as it is.
    pub(super) fn grid_size(&self, grid: Rect, cell: f32) -> (u8, u8) {
        let columns = if self.auto_columns {
            ((grid.width() / cell).floor() as i32).clamp(1, i32::from(MAX_COLUMNS)) as u8
        } else {
            self.columns
        };
        let rows = if self.auto_rows {
            ((grid.height() / cell).floor() as i32).clamp(1, i32::from(MAX_ROWS)) as u8
        } else {
            self.rows
        };
        (columns, rows)
    }
    /// Take `(columns, rows)` on, placing everything again where it changed.
    pub(super) fn set_grid_size(&mut self, (columns, rows): (u8, u8)) {
        if (columns, rows) != (self.columns, self.rows) {
            self.columns = columns;
            self.rows = rows;
            self.place_all();
        }
    }
}
