//! Render for `DesktopView`.

use super::{
    carve_around, draw_dock, draw_grid, draw_indicator, label_row_h, paint_fitted, raw_press, veil,
    Arc, Color32, DesktopAction, DesktopCtx, DesktopView, DockPlacement, LabelLegibility,
    LongPressMode, Metrics, MotionTokens, Rect, SlotPaint, Theme, Wallpaper, FULL_UV,
};

impl DesktopView {
    /// Draw the desktop in `rect`. Slot records and text galleys are reused where their
    /// inputs are unchanged. Cache rebuilds, egui and custom drawing can still allocate.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        cx: &mut DesktopCtx<'_>,
    ) -> Option<DesktopAction> {
        self.paint_wallpaper(ui.painter(), rect, cx.theme);
        // The veil goes **just above the wallpaper and below the icons** (guide 09 §2.2). Laid over the
        // icons it flattens the labels with them.
        if self.legibility == LabelLegibility::Veil {
            ui.painter().rect_filled(rect, 0.0, veil(cx.theme));
        }
        let metrics = cx.theme.metrics;
        // The automatic grid is reckoned on the area with the dock and the indicator taken out. A change
        // of placement can change the page count, so the area is worked out once more.
        self.fit_grid(rect, &metrics);
        // Advance the press animation. No repaint is requested here — the shell's stage 15 decides it
        // together, through `DesktopView::is_animating`.
        self.press.tick(cx.now);
        let grid = self.grid_rect(rect, &metrics);
        let cell = metrics
            .icon_cell
            .min(grid.width() / f32::from(self.columns.max(1)))
            .min(grid.height() / f32::from(self.rows.max(1)))
            .max(1.0);
        self.refresh_label_width(cell, metrics.desktop_label_pad);

        // A spent press neither presses an icon nor drives the page.
        let raw_press = if self.spent {
            None
        } else {
            raw_press(ui, Self::layer_id())
        };
        // A long press is the desktop's where the desktop is what the finger is on — not under the
        // shade, the prompt or a screen.
        let held = self
            .held
            .filter(|pos| !self.spent && ui.ctx().layer_id_at(*pos) == Some(Self::layer_id()));
        let mut long = None;
        // A4: a horizontal drag over the grid drives the page. The width `W` is the grid's.
        self.page_width = grid.width();
        // The rubber band is taken again from the current theme every frame — holding the value from
        // construction leaves the pull-back resistance at the end on its default however `[motion]
        // page.rubber` changes.
        self.swipe.set_rubber(cx.theme.motion.page.rubber);
        self.swipe.configure(self.pages.len(), grid.width());
        let drag_active = self.drive_page_swipe(ui, grid, raw_press, &cx.theme.motion);
        let mut action = None;
        let mut pressed = None;
        let goto_page;
        {
            let Self {
                pages,
                dock,
                records,
                page_index,
                columns,
                rows,
                label_lines,
                label_width,
                press,
                indicator_rects,
                slot_painter,
                swipe,
                info,
                spent,
                ..
            } = self;
            for record in records.iter_mut() {
                record.rect = None;
            }
            let mut paint = SlotPaint {
                records,
                press,
                label_lines: *label_lines,
                label_width: *label_width,
                label_row_h: label_row_h(ui.ctx(), cx.theme),
                pressed: &mut pressed,
                raw_press,
                drag_active,
                record_dx: Some(0.0),
                slot_painter: slot_painter.as_mut(),
                held,
                spent: *spent,
                info,
                long: &mut long,
            };
            // Only the two pages (`floor` and `ceil`) are drawn, at their x offsets. What is off-screen
            // is cut by the `Ui`'s clip (the content) — making more child `Ui`s would grow the `UiStack`
            // per frame.
            let mut drawn = usize::MAX;
            for (index, x) in swipe.visible() {
                if index == drawn {
                    continue;
                }
                drawn = index;
                let Some(page) = pages.get(index) else {
                    continue;
                };
                // The A2 origin Rect is recorded in **the settled page's** own coordinates (so the icon
                // zoom's origin does not wobble along mid-swipe).
                paint.record_dx = (index == *page_index).then_some(x);
                let shifted = grid.translate(egui::vec2(x, 0.0));
                if let Some(a) =
                    draw_grid(ui, page, shifted, cell, (*columns, *rows), cx, &mut paint)
                {
                    action = Some(a);
                }
            }
            paint.record_dx = Some(0.0);
            if let Some(a) = draw_dock(ui, dock, rect, cell, cx, &mut paint) {
                action = Some(a);
            }
            goto_page = draw_indicator(ui, rect, dock, pages.len(), swipe, cx, indicator_rects);
        }
        if let Some(index) = goto_page {
            self.swipe_to(index, &cx.theme.motion);
        }
        if pressed.is_none() {
            // With no cell pressed this frame, the release animation is run to the end.
            self.press.release(&cx.theme.motion);
        }
        if let (Some(DesktopAction::LongPress(id)), Some(cell)) = (&action, long) {
            self.answer_long_press(id, cell, &cx.theme.motion);
        }
        action
    }
    /// A long press on the icon `id`, drawn in `cell`, was the desktop's: its press is
    /// spent, so its release launches nothing, and the info popover comes up unless
    /// `[desktop] long_press = "none"`.
    pub(super) fn answer_long_press(&mut self, id: &str, cell: Rect, tokens: &MotionTokens) {
        self.spent = true;
        if self.long_press == LongPressMode::Info {
            self.info.open(id, cell, tokens);
        }
    }
    /// The gesture engine reported a long press at `pos` this frame (frame stage 5). The cells
    /// drawn this frame decide whose it is.
    pub(crate) fn hold_at(&mut self, pos: egui::Pos2) {
        self.held = Some(pos);
    }
    /// A press went down this frame (frame stage 5). Over an open popover it puts the popover away,
    /// and it is spent; any other press starts afresh.
    pub(crate) fn note_press(&mut self, tokens: &MotionTokens) {
        if self.info.is_open() {
            self.info.close(tokens);
            self.spent = true;
        } else {
            self.spent = false;
        }
    }
    /// Put the info popover away (it fades where it is).
    pub(crate) fn close_info(&mut self, tokens: &MotionTokens) {
        self.info.close(tokens);
    }
    /// Whether the info popover is up and taking presses.
    pub(crate) fn info_open(&self) -> bool {
        self.info.is_open()
    }
    /// **Probe**: the icon whose info popover is up. `None` while it is closed or on its
    /// way out.
    #[doc(hidden)]
    #[must_use]
    pub fn info_icon(&self) -> Option<&str> {
        self.info.id()
    }
    /// **Probe**: the info popover's card as last drawn, while it is up.
    #[doc(hidden)]
    #[must_use]
    pub fn info_rect(&self) -> Option<Rect> {
        self.info.card()
    }
    /// Draw the info popover (frame stage 9, after the workspace and the rail) — the shield over
    /// `root` while it is up, the card inside `bounds`. A long press this frame did not match is
    /// dropped here.
    pub(crate) fn info_ui(
        &mut self,
        ctx: &egui::Context,
        root: Rect,
        bounds: Rect,
        cx: &mut DesktopCtx<'_>,
    ) {
        self.held = None;
        let Self { info, entries, .. } = self;
        let slot = info
            .subject()
            .and_then(|id| entries.iter().find(|e| e.slot.id == id))
            .map(|e| &e.slot);
        info.ui(ctx, root, bounds, slot, cx);
    }
    /// Whether an icon press (A7) is running. The shell folds it into
    /// [`crate::Shell::is_animating`] so that the repaint policy is settled in one place — the nav bar's
    /// presses use the shell's [`crate::motion::AnimationStore`], but the desktop holds state per cell
    /// and uses a store of its own.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.press.is_animating() || self.swipe.is_animating() || self.info.is_animating()
    }
    /// A change in cell width throws the label galleys away (they are rebuilt at the next layout).
    pub(super) fn refresh_label_width(&mut self, cell: f32, pad: f32) {
        let width = (cell - pad).max(pad);
        if (self.label_width - width).abs() > 0.5 {
            self.label_width = width;
        }
    }
    /// The area the grid may use (with the dock and the indicator taken out). The indicator takes space
    /// only with **two or more pages**.
    pub(super) fn grid_rect(&self, rect: Rect, metrics: &Metrics) -> Rect {
        self.grid_rect_with(rect, metrics, self.pages.len() > 1)
    }
    /// [`Self::grid_rect`] with the indicator's room taken out or not, whatever the page count.
    pub(super) fn grid_rect_with(&self, rect: Rect, metrics: &Metrics, indicator: bool) -> Rect {
        let indicator = if indicator {
            metrics.page_indicator_height
        } else {
            0.0
        };
        // The indicator is always at the bottom. A dock against an edge takes from that edge, and one
        // crossing the desktop divides the content and leaves **the wider side**.
        let mut out = Rect::from_min_max(
            rect.min,
            egui::pos2(rect.max.x, (rect.max.y - indicator).max(rect.min.y)),
        );
        let t = self.dock.band_thickness(metrics);
        if t > 0.0 {
            out = match self.dock.placement() {
                DockPlacement::Edge(crate::gesture::Edge::Bottom) => Rect::from_min_max(
                    out.min,
                    egui::pos2(out.max.x, (out.max.y - t).max(out.min.y)),
                ),
                DockPlacement::Edge(crate::gesture::Edge::Top) => Rect::from_min_max(
                    egui::pos2(out.min.x, (out.min.y + t).min(out.max.y)),
                    out.max,
                ),
                DockPlacement::Edge(crate::gesture::Edge::Left) => Rect::from_min_max(
                    egui::pos2((out.min.x + t).min(out.max.x), out.min.y),
                    out.max,
                ),
                DockPlacement::Edge(crate::gesture::Edge::Right) => Rect::from_min_max(
                    out.min,
                    egui::pos2((out.max.x - t).max(out.min.x), out.max.y),
                ),
                DockPlacement::Band { axis, .. } => {
                    carve_around(out, self.dock.rect_in(rect, metrics), axis)
                }
            };
        }
        out
    }
    /// The label legibility correction (guide 09 §2.2).
    #[must_use]
    pub const fn legibility(&self) -> LabelLegibility {
        self.legibility
    }
    /// Change the label legibility correction. Used together with swapping the wallpaper at runtime —
    /// `Shadow` on changing to a photo background, `None` on coming back to a flat colour.
    pub const fn set_legibility(&mut self, legibility: LabelLegibility) {
        self.legibility = legibility;
    }
    /// The wallpaper. [`Wallpaper::Gradient`] caches its mesh for reuse while its inputs remain unchanged.
    pub(super) fn paint_wallpaper(&mut self, painter: &egui::Painter, rect: Rect, theme: &Theme) {
        // **The pair is resolved before the match**, so it costs one line rather than a second copy
        // of every arm below it.
        match self.wallpaper.for_theme(theme.dark) {
            // Already picked above; a pair inside a pair keeps the inner one.
            Wallpaper::Themed { dark, light } => {
                let inner = if theme.dark { dark } else { light };
                log::warn!("[wallpaper] a Themed pair inside a Themed pair: {inner:?}");
            }
            Wallpaper::Solid(role) => {
                painter.rect_filled(rect, 0.0, theme.color(*role));
            }
            Wallpaper::Fixed(color) => {
                painter.rect_filled(rect, 0.0, *color);
            }
            Wallpaper::Gradient { top, bottom } => {
                let hit = self
                    .gradient
                    .as_ref()
                    .filter(|(r, t, b, _)| *r == rect && t == top && b == bottom);
                let mesh = if let Some((_, _, _, mesh)) = hit {
                    Arc::clone(mesh)
                } else {
                    let mut mesh = egui::Mesh::default();
                    mesh.colored_vertex(rect.left_top(), *top);
                    mesh.colored_vertex(rect.right_top(), *top);
                    mesh.colored_vertex(rect.right_bottom(), *bottom);
                    mesh.colored_vertex(rect.left_bottom(), *bottom);
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(0, 2, 3);
                    let mesh = Arc::new(mesh);
                    self.gradient = Some((rect, *top, *bottom, Arc::clone(&mesh)));
                    mesh
                };
                painter.add(egui::Shape::mesh(mesh));
            }
            Wallpaper::Texture { id } => {
                painter.image(
                    *id,
                    rect,
                    FULL_UV,
                    // Not a role colour but **the multiplicative identity** — it draws the texture in
                    // its original colours.
                    Color32::WHITE,
                );
            }
            Wallpaper::TextureFit {
                id,
                source_size,
                fit,
            } => paint_fitted(painter, rect, theme, *id, *source_size, *fit),
            Wallpaper::Owned {
                texture,
                source_size,
                fit,
                // `texture.id()` is `#[inline]` and takes no lock — the only `TextureHandle` method
                // that may be called from the paint loop.
            } => paint_fitted(painter, rect, theme, texture.id(), *source_size, *fit),
            Wallpaper::Painter(cb) => cb(painter, rect),
            Wallpaper::ThemedPainter(cb) => cb(painter, rect, theme),
        }
    }
}
