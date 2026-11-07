//! Render for `Overlay`.

use super::{
    backdrop, paint_ground, panel, pin_leaving_under_panel, pin_panel_above_scrim, scrim, Backdrop,
    ColorRole, CxParts, Footers, Glass, Grab, NotificationCenter, Overlay, OverlayAction,
    OverlayFrame, OverlayLayout, OverlayReveal, PanelAction, PendingShot, Plate, Pointer, Rect,
    TileBrush, TileDecl, TileRow, TileSpots, PANEL_AREA_ID, PANEL_LEAVING_AREA_ID, SHOT_PATIENCE,
};

impl Overlay {
    /// Stage 11: the scrim plus the panel. Closed, it draws nothing (it only reports the peek row).
    #[expect(
        clippy::too_many_lines,
        reason = "it ties the scrim, the panel and the layer order together inside one frame"
    )]
    pub(crate) fn ui(
        &mut self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        screen: Rect,
        center: &NotificationCenter,
        custom: &[TileDecl],
    ) -> Option<OverlayAction> {
        if self.tiles_dirty {
            self.rebuild_tiles(custom);
        }
        let tokens = parts.theme.motion;
        let now = parts.now;
        let pointer = ctx.input(|i| Pointer {
            pressed: i.pointer.primary_pressed(),
            down: i.pointer.primary_down(),
            pos: i.pointer.interact_pos(),
            origin: crate::drag::press_point(i),
            velocity: i.pointer.velocity(),
            delta: i.pointer.delta(),
        });
        if pointer.pressed {
            if let Some(p) = pointer.origin.or(pointer.pos) {
                self.last_press = Some(p);
            }
        }
        let prev = self.frame;
        self.frame = OverlayFrame::default();
        // The finger over the panel (after the engine's stage 5). The release of a drag caught while closed is handled here too.
        self.update_grab(&pointer, prev.panel, &tokens);
        // **At the stop, a press anywhere else closes it**. There is no scrim at the
        // stop — that is the point of stopping there — so nothing was there to take the tap
        // that closes a full shade, and a shade pulled down to toggle Wi-Fi stayed over the top of
        // the screen through everything the hand did next (user report). The press is not taken:
        // it lands on the page as it would have, and the shade folds away behind it.
        if self.two_step && self.shade.at_detent() && pointer.pressed && !self.edge_active {
            let on_panel = pointer
                .origin
                .or(pointer.pos)
                .is_some_and(|p| prev.panel.is_some_and(|r| r.contains(p)));
            if !on_panel {
                self.dismissed = true;
            }
        }
        if self.shade.is_peeking(now) && self.shade.y() <= 0.5 {
            self.frame.peek = Some(Rect::from_min_size(
                screen.min,
                egui::vec2(screen.width(), self.status_height),
            ));
        }
        if self.shade.y() <= 0.5 {
            // A removal the operator already confirmed is not lost to the shade closing over
            // its animation: what was flying out (or done and waiting) is raised now.
            let owed = self.pending.take().or_else(|| self.flying.take());
            self.reset_panel_state();
            return owed;
        }
        self.settle_swipe(center, &tokens);
        self.refresh_tile_states(
            parts.services,
            parts.settings,
            parts.access,
            parts.theme.dark,
        );
        self.refresh_texts(ctx, parts, center, screen.width());
        // The scrim — or, for a card, only its catch for the tap outside: a card lies on
        // an undimmed page, and a tap beside it closes it without a scrim to take it.
        let tapped_outside = match self.reveal {
            OverlayReveal::Curtain => scrim::show(ctx, screen, self.scrim_alpha(), parts.theme),
            OverlayReveal::Card => !self.shade.at_detent() && scrim::catch(ctx, screen),
        };
        if tapped_outside {
            self.dismissed = true;
        }
        self.frost_backdrop(
            ctx,
            prev.panel.is_none(),
            parts.theme.color(ColorRole::ShadeSurface),
        );
        let plate = self.plate(screen, &pointer, parts.theme, prev.panel.is_none());
        self.last_plate = Some(plate);
        // **Crossing between the split panels**: the panel crossed to comes in as a
        // first open does, and what is left of the other dissolves where it stood, under it.
        let fading = self
            .fade
            .map(|(from, from_plate)| (from, from_plate, self.fade_t.value()));
        let status_row = self.status_hidden.then_some(self.status_height);
        if let Some(sh) = status_row {
            self.frame.status_row = Some(Rect::from_min_size(
                plate.content.min,
                egui::vec2(plate.content.width(), sh),
            ));
        }
        // The list drag-scrolls only on a gesture the list owns (or with nothing pressed). While the
        // engine, the shade or a row owns it, it is `Never` and the `ScrollArea` adds no delta.
        let drag_scroll = !self.edge_active && matches!(self.grab, Grab::None | Grab::List);
        let scroll_to = self.scroll_to.take();
        let swiping = self.swiping.map(|id| (id, self.swipe_x.value()));
        let collapsing = self.collapsing.map(|id| (id, self.collapse.value()));
        let Self {
            tiles,
            tile_rects,
            leaving_tile_rects,
            tile_layout,
            tile_painter,
            panel_painter,
            reveal,
            tile_homes,
            tile_columns,
            texts,
            expanded,
            dismiss_button,
            swallow_tap,
            footer,
            footer_notes,
            footer_controls,
            panel: shown,
            reliefs,
            ..
        } = self;
        let [panel_relief, leaving_relief] = reliefs;
        let reveal = *reveal;
        let footers = Footers {
            all: footer,
            notes: footer_notes,
            controls: footer_controls,
        };
        let row = TileRow {
            tiles,
            columns: *tile_columns,
            texts,
            expanded: *expanded,
            panels: custom,
            dismiss_button: *dismiss_button,
            footer: footers.of(*shown),
            swiping,
            collapsing,
            swallow_tap: *swallow_tap,
            drag_scroll,
            scroll_to,
            panel: *shown,
        };
        let out = egui::Area::new(egui::Id::unique(PANEL_AREA_ID))
            .order(egui::Order::Foreground)
            .fixed_pos(plate.visible.min)
            .default_size(plate.visible.size())
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                // The clip = the part shown, plus the room a card's soft edge feathers into. The
                // content is laid out into `plate.content` (the full height), so what is not yet
                // revealed below (the footer and the handle's places) is neither drawn nor
                // pressable (egui's hit Rect is the intersection with the clip).
                // Never above the overlay's own screen: a card on its way down comes out from
                // under a visible status bar rather than over it, so the clock and the alarms stay
                // readable through the whole arrival.
                ui.set_clip_rect(plate.visible.expand(plate.bleed).intersect(screen));
                // The Area's size = the part shown. Otherwise egui records this layer as small (or as
                // large as below the curtain) and `layer_id_at(the press point)` picks the scrim (a panel
                // tap leaking into the scrim) or the panel eats a tap below the curtain.
                ui.set_min_size(plate.visible.size());
                let ground = paint_ground(
                    ui,
                    &plate,
                    parts.theme,
                    panel_relief,
                    panel_painter.as_mut(),
                    (*shown, reveal, false),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(plate.content));
                child.set_clip_rect(plate.visible.intersect(screen));
                child.set_opacity(plate.content_alpha);
                panel::show(
                    &mut child,
                    plate.content,
                    parts,
                    &row,
                    center,
                    TileSpots {
                        drawn: tile_rects,
                        layout: tile_layout.as_mut(),
                        homes: tile_homes,
                        brush: TileBrush {
                            painter: tile_painter.as_mut(),
                            ground,
                        },
                    },
                    status_row,
                )
            })
            .inner;
        pin_panel_above_scrim(ctx);
        if let Some((from, from_plate, t)) = fading {
            // What is left of the panel being crossed from: drawn as it was, fading, and taking no
            // presses — the panel crossed to has them from the frame it was asked for.
            let leaving = Plate {
                fill: from_plate.fill.gamma_multiply(1.0 - t),
                soft: None,
                content_alpha: from_plate.content_alpha * (1.0 - t),
                arrival: from_plate.arrival * (1.0 - t),
                glass: from_plate.glass.map(|g| Glass {
                    alpha: g.alpha * (1.0 - t),
                    ..g
                }),
                ..from_plate
            };
            let old_row = TileRow {
                footer: footers.of(from),
                swiping: None,
                collapsing: None,
                drag_scroll: false,
                scroll_to: None,
                panel: from,
                ..row
            };
            egui::Area::new(egui::Id::unique(PANEL_LEAVING_AREA_ID))
                .order(egui::Order::Foreground)
                .fixed_pos(leaving.visible.min)
                .default_size(leaving.visible.size())
                .constrain(false)
                .fade_in(false)
                .interactable(false)
                .show(ctx, |ui| {
                    ui.set_clip_rect(leaving.visible.expand(leaving.bleed).intersect(screen));
                    let ground = paint_ground(
                        ui,
                        &leaving,
                        parts.theme,
                        leaving_relief,
                        panel_painter.as_mut(),
                        (from, reveal, true),
                    );
                    let mut child =
                        ui.new_child(egui::UiBuilder::new().max_rect(from_plate.content));
                    child.set_clip_rect(leaving.visible.intersect(screen));
                    child.set_opacity(leaving.content_alpha);
                    let _ = panel::show(
                        &mut child,
                        from_plate.content,
                        parts,
                        &old_row,
                        center,
                        TileSpots {
                            drawn: leaving_tile_rects,
                            layout: tile_layout.as_mut(),
                            homes: tile_homes,
                            brush: TileBrush {
                                painter: tile_painter.as_mut(),
                                ground,
                            },
                        },
                        status_row,
                    );
                });
            pin_leaving_under_panel(ctx);
        } else if self.layout == OverlayLayout::Split {
            // Kept up, empty, while a split shade is open: egui lays a new `Area` out unseen on
            // its first frame, and the outgoing panel vanishing for that frame read as a blink
            // at the start of every crossing.
            egui::Area::new(egui::Id::unique(PANEL_LEAVING_AREA_ID))
                .order(egui::Order::Foreground)
                .fixed_pos(screen.min)
                .constrain(false)
                .fade_in(false)
                .interactable(false)
                .show(ctx, |_| {});
        }
        self.list_offset = out.list_offset;
        self.list_rect = out.list_rect;
        self.close_hit = out.close_hit;
        self.slider_rect = out.slider_rect;
        self.expanded = out.expanded;
        // A long press is matched against **the tile places drawn this frame**. It comes while the finger
        // is still down, so the screen changes before the release and "what I pressed opened" is felt in
        // the hand.
        let long_press = self.take_tile_long_press();
        if pointer.pressed {
            self.swallow_tap = false;
            if let Grab::Pending { row, .. } = &mut self.grab {
                *row = out.pressed_row;
            }
            // **Whether it can be pushed right to open** — only where that notification carries somewhere
            // to go. Settled once on the frame of the press and used through that whole drag.
            self.swipe_open = out
                .pressed_row
                .and_then(|id| center.get(id))
                .is_some_and(|n| n.action.is_some());
        }
        // **What the × removes goes out the same way as a swipe.** A button has no finger velocity, so it
        // is pushed left by a tween rather than a spring (`toast_out` — the motion of a notification
        // leaving). On the frame it is all the way out, `settle_swipe` raises the `Dismiss`.
        //
        // **Only what the panel raised this frame** is intercepted. `self.pending` is the result the swipe
        // already flew out and handed over, and catching it again here would fly the same row out twice.
        if let Some(PanelAction::Dismiss(id)) = out.action {
            if self.flying.is_none() && !tokens.reduce {
                let w = self.list_rect.width().max(1.0);
                self.swiping = Some(id);
                self.swipe_x.set_range(-w, 0.0);
                self.swipe_x.snap(0.0);
                self.swipe_x.to(-w, tokens.toast_out);
                self.flying = Some(PanelAction::Dismiss(id));
                return None;
            }
        }
        long_press.or(out.action).or_else(|| self.pending.take())
    }
    /// **The card's frosted backdrop**: asked of the runner on a card's first visible
    /// frame, frosted and uploaded when it answers — a frame or two later, while the card is still
    /// arriving — and kept until the shade shuts. A runner that never answers is waited on for
    /// [`SHOT_PATIENCE`] frames, and the card stays solid.
    ///
    /// **A frost that no longer matches is put down**, and the card goes solid until it next
    /// opens: the theme switched under it (the dark mode tile is on the controls card, and a dark
    /// page behind a frost of the light one washes the card out) or the window resized (the frost
    /// no longer lines up). It is not taken again — the card is over the page by then, and a
    /// second screenshot would frost the card into its own backdrop. A theme switch repaints
    /// everything anyway, so the card going solid with it does not show as a change of its own.
    ///
    /// **A panel painter takes the frost's place**: it draws the ground and is handed no
    /// backdrop, so with one the screenshot is never asked for — frosting it would be work for
    /// nothing.
    pub(super) fn frost_backdrop(
        &mut self,
        ctx: &egui::Context,
        first: bool,
        surface: egui::Color32,
    ) {
        if self.reveal != OverlayReveal::Card
            || self.card_glass >= 1.0
            || self.panel_painter.is_some()
        {
            return;
        }
        let viewport = ctx.viewport_rect();
        if self
            .backdrop
            .as_ref()
            .is_some_and(|b| b.surface != surface || b.viewport != viewport)
            || self
                .pending_shot
                .as_ref()
                .is_some_and(|shot| shot.surface != surface || shot.viewport != viewport)
        {
            self.put_down_glass();
            return;
        }
        if first && self.backdrop.is_none() {
            let pixels = viewport.size() * ctx.pixels_per_point();
            let area = f64::from(pixels.x).ceil() * f64::from(pixels.y).ceil();
            if !pixels.x.is_finite()
                || !pixels.y.is_finite()
                || pixels.x <= 0.0
                || pixels.y <= 0.0
                || !area.is_finite()
                || area > backdrop::MAX_SOURCE_PIXELS as f64
            {
                self.put_down_glass();
                return;
            }
            let (tx, rx) = crate::inbox::Inbox::pair();
            self.pending_shot = Some(PendingShot {
                rx,
                viewport,
                surface,
            });
            let wake = ctx.clone();
            ctx.request_screenshot(move |image| {
                let _ = tx.send(image);
                wake.request_repaint();
            });
            self.shot_wait = SHOT_PATIENCE;
            return;
        }
        if self.shot_wait == 0 || self.backdrop.is_some() {
            self.pending_shot = None;
            return;
        }
        let shot = self
            .pending_shot
            .as_ref()
            .and_then(|shot| shot.rx.try_recv().ok());
        let Some(image) = shot else {
            self.shot_wait -= 1;
            if self.shot_wait == 0 {
                self.pending_shot = None;
            }
            return;
        };
        let Ok(frosted) = backdrop::frost(&image) else {
            self.put_down_glass();
            return;
        };
        drop(image);
        let texture = ctx.load_texture(
            "fairing.overlay.backdrop",
            frosted,
            egui::TextureOptions::LINEAR,
        );
        self.backdrop = Some(Backdrop {
            texture,
            viewport,
            surface,
        });
        self.shot_wait = 0;
        self.pending_shot = None;
    }
}
