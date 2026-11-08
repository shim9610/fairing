//! Render for `Workspace`.

use super::{
    a3_mapping, draw_clearing, draw_instance, draw_outgoing, paint_divider, paint_ground, painters,
    pane_of, raises_outgoing, root_key, scale_about, screen_of, split, transition, warm_slot,
    CxParts, DesktopAction, DesktopCtx, DesktopView, DividerAction, DividerLook, Draw, DrawParams,
    HomeFrame, HomeTransition, Layout, Lift, MotionTokens, Overview, PaneFrame, PaneLook, Phase,
    RecentsPainters, Rect, Released, Seat, Split, SplitAxis, StackTransition, TSTransform, Task,
    Workspace, WorkspaceOutput, WorkspaceView, DIVIDER_ID, PANE_SLOTS, WARM_SLOTS,
};

impl Workspace {
    /// Frame stage 9: coordinate desktop or pane rendering through egui Areas.
    /// egui rendering and application callbacks can allocate during this call.
    pub(crate) fn ui(
        &mut self,
        ui: &mut egui::Ui,
        parts: &mut CxParts<'_>,
        registry: &mut crate::screen::Registry,
        desktop: &mut DesktopView,
        layout: &Layout,
        painters: &mut RecentsPainters,
    ) -> WorkspaceOutput {
        let ctx = ui.ctx().clone();
        let content = layout.content;
        self.last_content = content;
        let tokens = parts.theme.motion;
        // `clear_top_to` takes no tokens as an argument — they are cached here.
        self.set_motion(&tokens);
        self.split_metrics = (
            parts.theme.metrics.split_divider,
            parts.theme.metrics.touch_target,
        );
        let mut out = WorkspaceOutput::default();

        // A press moves the focus to the pane under it — before anything is drawn, so the
        // screens drawn this frame already know which of them has it.
        if self.split.is_some() && !self.blocks_input() {
            let press = ctx.input(|i| {
                i.pointer
                    .any_pressed()
                    .then(|| crate::drag::press_point(i))
                    .flatten()
            });
            if let Some(at) = press {
                self.focus_at(&ctx, at);
            }
        }
        self.hold_split(content);
        if self.layers_dirty {
            self.layers_dirty = false;
            self.reset_layer_transforms(&ctx);
        }
        // The sizing pass is run through even the slot that may be used next frame.
        self.warm_layer_slots(&ctx, content);
        let frame = self.pane_frame(content);
        self.pane_rects = frame.visible;
        self.pane_laid = frame.laid;
        // A2 maps onto the focused pane — the whole content with one — and the lift over it.
        let focused_pane = pane_of(frame.visible, self.focus);
        let home = self.lifted(self.home_frame(focused_pane, &tokens));

        // The desktop layer — painted plain under the overview over a task, where it is the
        // backdrop the screen shrinks against (A10).
        out.desktop_action = self.desktop_ui(
            &ctx,
            parts,
            desktop,
            content,
            &home,
            painters.ground.as_mut(),
        );

        // The Pane layer(s), then the divider over them — or, under the overview, the screen on
        // show shrinking into its card (A10), and nothing at all once the cards cover the content.
        let look = self.overview.as_ref().map(|o| {
            let deck = o.deck(content, parts.theme);
            (o.is_full(), o.screen_look(&deck))
        });
        match look {
            Some((true, _)) => {}
            Some((false, look)) => {
                let mut shrinking = home;
                // Split, the panes fade where they stand; one pane shrinks into its card.
                if self.split.is_none() {
                    shrinking.screen_scale *= look.scale;
                    shrinking.screen_pivot = Some(look.pivot);
                    shrinking.screen_shift += look.shift;
                }
                shrinking.screen_opacity *= look.opacity;
                self.draw_panes(&ctx, parts, registry, content, &frame, &shrinking, &tokens);
            }
            None => {
                self.draw_panes(&ctx, parts, registry, content, &frame, &home, &tokens);
                self.divider_ui(&ctx, parts.theme, content, &frame);
            }
        }
        if look.is_some() {
            self.divider_handle = Rect::NOTHING;
        }
        self.end_stray_divider_drag(&ctx, content);
        if self.overview.is_some() {
            let cards = self.overview_cards(parts.access, registry);
            let picked = self
                .overview
                .as_mut()
                .and_then(|overview| overview.ui(&ctx, parts, content, &cards, &tokens, painters));
            if let Some(picked) = picked {
                let closed = self.apply_overview(picked);
                out.closed.extend(closed);
            }
        }
        out.closed.append(&mut self.overview_closed);
        // A3's "blocking input: a shield mid-tween" (M1 had `interactable(false)` alone). The back
        // gesture, where the finger is leading, is excluded — the gesture engine already has the same
        // `Area` up throughout it (shell stage 11), which would draw it twice in one frame.
        if self.needs_shield() {
            let screen = screen_of(layout);
            if screen.is_positive() {
                let _ = crate::gesture::shield::show(&ctx, screen);
                // Always the top within Foreground (it does not lean on the registration order).
                ctx.move_to_top(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::unique(crate::gesture::shield::AREA_ID),
                ));
            }
        }
        self.sync_sizes();
        out
    }
    /// The desktop layer, where it shows: the home view, A2 flying over it, or the recent screens'
    /// ground — the backdrop the screen shrinks against under the overview over a task (A10), plain
    /// or a painter's. Returns what was tapped on it, where it takes input.
    pub(super) fn desktop_ui(
        &self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        desktop: &mut DesktopView,
        content: Rect,
        home: &HomeFrame,
        ground: Option<&mut painters::RecentsGroundPainter>,
    ) -> Option<DesktopAction> {
        // The plain backdrop the screen shrinks against: under the overview over a task, and
        // under a lift — the overview it may become has the same one.
        let backdrop_only =
            (self.overview.is_some() || self.lift.is_some()) && self.view == WorkspaceView::Tasks;
        let show_desktop =
            self.view == WorkspaceView::Home || self.home.is_active() || backdrop_only;
        if !show_desktop {
            ctx.set_transform_layer(DesktopView::layer_id(), TSTransform::IDENTITY);
            return None;
        }
        let transform = scale_about(content.center(), home.desktop_scale);
        ctx.set_transform_layer(DesktopView::layer_id(), transform);
        let interactable = !self.home.is_active() && self.overview.is_none();
        let action = egui::Area::new(DesktopView::area_id())
            .order(egui::Order::Background)
            .fixed_pos(content.min)
            .default_size(content.size())
            .constrain(false)
            .fade_in(false)
            .interactable(interactable)
            .show(ctx, |ui| {
                ui.set_clip_rect(content);
                ui.set_min_size(content.size());
                if backdrop_only {
                    // Whole from the start: the screen shrinking into its card covers it.
                    let over = painters::RecentsOver::Task;
                    paint_ground(ui.painter(), parts.theme, ground, content, over, 1.0);
                    return None;
                }
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content));
                child.set_opacity(home.desktop_opacity);
                let mut dcx = DesktopCtx {
                    theme: parts.theme,
                    access: parts.access,
                    icons: &mut *parts.icons,
                    now: parts.now,
                    legibility: desktop.legibility(),
                    strings: parts.strings,
                };
                desktop.ui(&mut child, content, &mut dcx)
            })
            .inner;
        action.filter(|_| interactable)
    }
    /// Whether a tween is running and input has to be blocked (A3 · A2, "blocking input"). The
    /// back gesture, where a finger is driving the value (`confirmed == None`), is **input itself**
    /// and is not blocked.
    pub(super) fn needs_shield(&self) -> bool {
        if self.home.is_active() {
            return true;
        }
        // A lift or a switch let go moves on its own; while the finger has it, the gesture
        // engine's shield is up already.
        if self.lift.as_ref().is_some_and(Lift::is_released)
            || self
                .switch
                .as_ref()
                .is_some_and(|s| s.confirmed().is_some())
        {
            return true;
        }
        if self.split.as_ref().is_some_and(Split::is_tweening) {
            return true;
        }
        if self.overview.as_ref().is_some_and(Overview::is_tweening) {
            return true;
        }
        if self.is_clearing() {
            return true;
        }
        match self.stack {
            StackTransition::Idle => false,
            StackTransition::DraggingBack { confirmed, .. } => confirmed.is_some(),
            StackTransition::Pushing { .. } | StackTransition::Popping { .. } => true,
        }
    }
    /// Tidy up the layer transforms left after a transition. The slots are renumbered by stack depth
    /// (before drawing) and all those layers put back to `IDENTITY` — whichever are drawn will be set
    /// again straight away.
    pub(super) fn reset_layer_transforms(&mut self, ctx: &egui::Context) {
        for pane in 0..self.panes.len().max(1) {
            let base = Self::slot_base(pane);
            if let Some(task) = self.task_of_pane_mut(pane) {
                for (depth, instance) in task.iter_mut().enumerate() {
                    let slot = base.saturating_add(u32::try_from(depth).unwrap_or(u32::MAX));
                    instance.set_layer_slot(slot);
                    ctx.set_transform_layer(instance.layer_id(), TSTransform::IDENTITY);
                }
            }
        }
    }
    /// The slot warm-up. An egui `Area` with no `AreaState` runs its first frame as a
    /// sizing pass (`ui_builder.invisible()`) and throws that frame's output away whole — which is
    /// what leaves an incoming screen blank on the frame a transition starts. `Areas::set_state` is
    /// `pub(crate)`, so the state cannot be planted; instead **an empty `Area` is drawn once
    /// beforehand** to consume that pass first. Exactly once per slot, and nothing is drawn on that
    /// frame.
    ///
    /// [`WARM_SLOTS`] of them are run through on the first frame, and as the stack deepens the
    /// **next slot to be used** (`depth + 1`) is run through in advance — a launch or back request is
    /// handled at frame stage 14 and that instance is first drawn on the following frame, so this
    /// frame's warm-up is always a step ahead.
    pub(super) fn warm_layer_slots(&mut self, ctx: &egui::Context, content: Rect) {
        let depth_of = |ws: &Self, pane: usize| ws.task_of_pane(pane).map_or(0, Task::len);
        let split = self.split.is_some();
        // The first pane's slots also serve a second pane left alone — it moves to them.
        let first = depth_of(self, 0)
            .max(self.closing_task.as_ref().map_or(0, Task::len))
            .max(if split { depth_of(self, 1) } else { 0 });
        let need = |depth: usize, least: u32| {
            u32::try_from(depth)
                .unwrap_or(u32::MAX)
                .saturating_add(1)
                .max(least)
                .min(PANE_SLOTS)
        };
        // The second pane's only while it is up: a pane coming in starts outside the content, so
        // its first frame — the one a sizing pass would blank — is not on the glass anyway.
        let wanted = [
            need(first, WARM_SLOTS),
            if split { need(depth_of(self, 1), 0) } else { 0 },
        ];
        for (pane, want) in wanted.into_iter().enumerate() {
            let base = Self::slot_base(pane);
            while self.warmed_slots.get(pane).is_some_and(|w| *w < want) {
                let Some(warmed) = self.warmed_slots.get_mut(pane) else {
                    break;
                };
                let slot = base.saturating_add(*warmed);
                *warmed = warmed.saturating_add(1);
                warm_slot(ctx, slot, content);
            }
        }
    }
    /// This frame's A2 values ([`HomeFrame::IDLE`] where there are none).
    pub(super) fn home_frame(&self, content: Rect, tokens: &MotionTokens) -> HomeFrame {
        match self.home {
            HomeTransition::Idle => HomeFrame::IDLE,
            HomeTransition::Opening {
                t,
                icon_rect: Some(icon),
            } => HomeFrame::card(&transition::a2_open(t.value(), icon, content, tokens)),
            HomeTransition::Closing {
                t,
                icon_rect: Some(icon),
                from,
            } => {
                // A lifted screen let go into home: the card starts where the finger
                // left the screen, and the screen shrinks inside it as it fades.
                let m = transition::a2_close(t.value(), icon, from.unwrap_or(content), tokens);
                let mut frame = HomeFrame::card(&m);
                if from.is_some() {
                    frame.fit_screen(m.card_rect, content);
                }
                frame
            }
            HomeTransition::Opening { t, icon_rect: None } => {
                HomeFrame::fallback(&transition::a2_fallback_open(t.value(), tokens))
            }
            HomeTransition::Closing {
                t,
                icon_rect: None,
                from,
            } => {
                let m = transition::a2_fallback_close(t.value(), tokens);
                let mut frame = HomeFrame::fallback(&m);
                if let Some(from) = from {
                    let shrunk =
                        Rect::from_center_size(from.center(), from.size() * m.screen_scale);
                    frame.fit_screen(shrunk, content);
                }
                frame
            }
        }
    }
    /// Hold the split inside what its panes allow (a screen with a larger minimum came to the top,
    /// the content resized under it). Where no ratio holds both any more, back to one pane.
    pub(super) fn hold_split(&mut self, content: Rect) {
        let Some(phase) = self.split.as_ref().map(Split::phase) else {
            return;
        };
        if !content.is_positive() {
            return;
        }
        let axis = self.axis_for(content);
        // A screen that never shares came to the top of a pane — a task resumed into it, a pop
        // that uncovered it: one pane again, the focused one (a push of one ends the split as it
        // opens, `Workspace::open`). Without this it would sit in half the content at a minimum
        // it never named.
        let target = self.split_metrics.1;
        let refused = (0..self.panes.len()).any(|pane| {
            self.task_of_pane(pane)
                .and_then(Task::top)
                .is_some_and(|top| {
                    split::min_len(top.split_support(), content, axis, target).is_none()
                })
        });
        if refused && phase == Phase::Split {
            self.collapse_to_focused();
            return;
        }
        let mins = self.pane_mins(content, axis);
        match split::bounds(content, axis, self.split_metrics.0, mins) {
            Some(bounds) => {
                if let Some(split) = self.split.as_mut() {
                    split.keep_within(Some(bounds));
                }
            }
            None if phase == Phase::Split => self.collapse_to_focused(),
            None => {}
        }
    }
    /// This frame's pane rects: the content with one pane, the split's geometry with two.
    pub(super) fn pane_frame(&self, content: Rect) -> PaneFrame {
        let Some(split) = self.split.as_ref() else {
            return PaneFrame::single(content);
        };
        let axis = self.axis_for(content);
        let mins = self.pane_mins(content, axis);
        let g = split.geometry(content, axis, self.split_metrics.0, mins);
        PaneFrame {
            visible: g.visible,
            laid: g.laid,
            squeeze: g.squeeze,
            divider: Some((g.divider, g.divider_alpha)),
            other: Some(1 - self.focus.min(1)),
        }
    }
    /// The height the OSK covers of a pane at `visible` — the part of the content's bottom inset
    /// that reaches up into it.
    pub(super) fn pane_inset(&self, visible: Rect, content: Rect) -> f32 {
        if !visible.is_positive() {
            return 0.0;
        }
        (self.inset_bottom - (content.max.y - visible.max.y)).max(0.0)
    }
    /// The Pane (stack) render: mid-transition it draws two instances on offset layers
    /// (A3). The layer slots are lent out here by stack depth (a finite set), from each pane's base.
    /// During an A2, the placeholder card is drawn first inside the top instance's `Area` (the
    /// card's colour is the declaration's `background` or surface, and the icon the task root's),
    /// and the real screen is drawn over it from `screen_opacity > 0` (an open's `t ≥ 0.5`) on.
    ///
    /// **Split, the other pane is drawn still** — its top, in its own rect — and every transition
    /// is the focused pane's, inside its rect. Each screen is cut at its pane's edge on
    /// the glass, so a push slides within its pane and not across the other.
    #[allow(clippy::too_many_arguments)] // The frame's parts, and the three things worked out for it.
    #[allow(clippy::too_many_lines)] // One pane's draw as it always was, and the other pane's before it.
    pub(super) fn draw_panes(
        &mut self,
        ctx: &egui::Context,
        parts: &mut CxParts<'_>,
        registry: &mut crate::screen::Registry,
        content: Rect,
        frame: &PaneFrame,
        home: &HomeFrame,
        tokens: &MotionTokens,
    ) {
        let closing = matches!(self.home, HomeTransition::Closing { .. });
        if !(self.view == WorkspaceView::Tasks || closing) {
            return;
        }
        let focus = self.focus.min(1);
        let laid = pane_of(frame.laid, focus);
        // An A2 card flies between the pane and its icon, so it is cut only at the content's edge.
        let clip = if home.card.is_some() {
            content
        } else {
            pane_of(frame.visible, focus)
        };
        let insets = [
            self.pane_inset(frame.visible[0], content),
            self.pane_inset(frame.visible[1], content),
        ];
        let inset = pane_of(insets, focus);
        let split = self.split.is_some();
        let width = laid.width();
        let a3 = a3_mapping(&self.stack, width, tokens);
        let animating = self.blocks_input();
        let pushing = matches!(self.stack, StackTransition::Pushing { .. });
        // The top is on its way right with the finger (an unconfirmed gesture, or a cancel returning).
        let back_top_moves = matches!(
            self.stack,
            StackTransition::DraggingBack { outgoing: None, .. }
        );
        let popping = matches!(self.stack, StackTransition::Popping { .. })
            || matches!(
                self.stack,
                StackTransition::DraggingBack {
                    outgoing: Some(_),
                    ..
                }
            );
        // The clear-top cross-fade: only the top being cleared fades out by alpha — it is drawn in
        // place over the screen below, so its layer is raised as the outgoing one's is.
        let clear_alpha = self.clear_top_alpha();
        let raise_outgoing = raises_outgoing(&self.stack) || clear_alpha.is_some();
        let bases = [Self::slot_base(0), Self::slot_base(1)];
        let leaving = self.leaving_pane();
        // The quick switch: the screen on show slides with the finger and the next
        // task's comes in beside it, on the second pane's slots — one pane only.
        let (switch_out, switch_in) = match self.switch.as_ref() {
            Some(switch) if !split => {
                let (out, inn) = switch.offsets();
                (out, switch.incoming().map(|key| (key, inn)))
            }
            _ => (0.0, None),
        };
        let Self {
            tasks,
            panes,
            stack,
            closing_task,
            clearing,
            leaving_task,
            ..
        } = self;

        let draw = &mut Draw { parts, registry };

        // The other pane: its top, still — or sliding with its pane on the way in or out.
        if let Some(other) = frame.other {
            let task = if leaving == Some(other) && leaving_task.is_some() {
                leaving_task.as_mut()
            } else {
                panes
                    .get(other)
                    .and_then(|p| p.task)
                    .and_then(|i| tasks.get_mut(i))
            };
            if let Some(task) = task {
                let base = pane_of(bases, other);
                for (depth, instance) in task.iter_mut().enumerate() {
                    instance.set_layer_slot(
                        base.saturating_add(u32::try_from(depth).unwrap_or(u32::MAX)),
                    );
                }
                if let Some(top) = task.top_mut() {
                    let params = DrawParams {
                        inset_bottom: pane_of(insets, other),
                        x: 0.0,
                        opacity: home.screen_opacity,
                        scale: 1.0,
                        pivot: None,
                        shift: egui::Vec2::ZERO,
                        dim: pane_of(frame.squeeze, other) * split::SQUEEZE_DIM,
                        shadow: 0.0,
                        focused: !animating,
                        raise: false,
                        clip: pane_of(frame.visible, other),
                        seat: Seat::Beside,
                    };
                    draw_instance(
                        ctx,
                        draw,
                        top,
                        pane_of(frame.laid, other),
                        &params,
                        None,
                        None,
                    );
                }
            }
        }

        if let Some((key, x_in)) = switch_in {
            if let Some(task) = tasks.iter_mut().find(|t| root_key(t) == Some(key)) {
                let depth = u32::try_from(task.len().saturating_sub(1)).unwrap_or(u32::MAX);
                if let Some(top) = task.top_mut() {
                    top.set_layer_slot(pane_of(bases, 1).saturating_add(depth));
                    let params = DrawParams {
                        inset_bottom: inset,
                        x: x_in,
                        opacity: 1.0,
                        scale: 1.0,
                        pivot: None,
                        shift: egui::Vec2::ZERO,
                        dim: 0.0,
                        shadow: 0.0,
                        focused: false,
                        raise: false,
                        clip,
                        seat: Seat::Alone,
                    };
                    draw_instance(ctx, draw, top, laid, &params, None, None);
                }
            }
        }

        let look = PaneLook { clip, split, inset };
        draw_outgoing(ctx, draw, stack, laid, &a3, home, &look);
        draw_clearing(ctx, draw, clearing, laid, clear_alpha, &look);

        let pane_task = panes
            .get(focus)
            .and_then(|p| p.task)
            .and_then(|i| tasks.get_mut(i));
        let Some(task) = pane_task.or(closing_task.as_mut()) else {
            return;
        };
        let len = task.len();
        let base = pane_of(bases, focus);
        for (depth, instance) in task.iter_mut().enumerate() {
            instance.set_layer_slot(base.saturating_add(u32::try_from(depth).unwrap_or(u32::MAX)));
        }
        if (pushing || back_top_moves) && len >= 2 {
            if let Some(below) = task.iter_mut().nth(len - 2) {
                let params = DrawParams {
                    inset_bottom: inset,
                    x: a3.x_out,
                    opacity: home.screen_opacity,
                    scale: home.screen_scale,
                    pivot: home.screen_pivot,
                    shift: home.screen_shift,
                    dim: a3.dim,
                    shadow: 0.0,
                    focused: false,
                    raise: false,
                    clip,
                    seat: Seat::of(split),
                };
                draw_instance(ctx, draw, below, laid, &params, None, None);
            }
        }
        // The top (plus the root, for the card's icon). The two are different slots, so they can be borrowed at once.
        let mut iter = task.iter_mut();
        let Some(root) = iter.next() else {
            return;
        };
        let (root_icon, top) = match iter.last() {
            Some(top) => (root.icon(), top),
            None => (None, root),
        };
        let x = if pushing || back_top_moves {
            a3.x_in
        } else if popping {
            a3.x_out
        } else {
            switch_out
        };
        let squeeze = pane_of(frame.squeeze, focus) * split::SQUEEZE_DIM;
        let params = DrawParams {
            inset_bottom: inset,
            x,
            opacity: home.screen_opacity,
            scale: home.screen_scale,
            pivot: home.screen_pivot,
            shift: home.screen_shift,
            dim: if popping {
                a3.dim.max(squeeze)
            } else {
                squeeze
            },
            shadow: if pushing || back_top_moves {
                a3.shadow
            } else {
                0.0
            },
            focused: !animating,
            raise: !raise_outgoing,
            clip,
            seat: Seat::of(split),
        };
        draw_instance(ctx, draw, top, laid, &params, home.card.as_ref(), root_icon);
    }
    /// The divider: the band between the panes with a grip at its middle, the focused
    /// pane's outline, and the handle a finger moves it by — a drag follows 1:1, a release springs
    /// to where both panes keep their minimums or closes the pane it was pushed into, a double
    /// tap evens it out.
    ///
    /// Two `Area`s: what is drawn is in one that is click-through, so that it can cover the
    /// content without taking a press from the panes, and the handle is a second, no bigger than
    /// the handle — its interactable `Area` participates in egui hit testing within that rect.
    pub(super) fn divider_ui(
        &mut self,
        ctx: &egui::Context,
        theme: &crate::theme::Theme,
        content: Rect,
        frame: &PaneFrame,
    ) {
        let Some((band, alpha)) = frame.divider else {
            self.divider_handle = Rect::NOTHING;
            return;
        };
        let axis = self.axis_for(content);
        let target = theme.metrics.touch_target;
        let grow = ((target - axis.along(band.size())) / 2.0).max(0.0);
        let handle = match axis {
            SplitAxis::SideBySide => band.expand2(egui::vec2(grow, 0.0)),
            SplitAxis::Stacked => band.expand2(egui::vec2(0.0, grow)),
        };
        let resting = matches!(self.split.as_ref().map(Split::phase), Some(Phase::Split));
        let live = resting
            && self.view == WorkspaceView::Tasks
            && !self.home.is_active()
            && !self.stack.is_active()
            && !self.is_clearing();
        self.divider_handle = if live { handle } else { Rect::NOTHING };
        let dragging = self.split.as_ref().is_some_and(Split::is_dragging);

        // What is drawn: the band, the grip, the focus outline.
        let chrome = egui::Id::unique(DIVIDER_ID);
        let focus = pane_of(frame.visible, self.focus);
        paint_divider(
            ctx,
            theme,
            content,
            DividerLook {
                band,
                alpha,
                axis,
                dragging,
                focus: resting.then_some(focus),
            },
        );
        if !live {
            return;
        }
        // The handle.
        let handle_id = chrome.with("handle");
        ctx.move_to_top(egui::LayerId::new(egui::Order::Background, handle_id));
        let action = egui::Area::new(handle_id)
            .order(egui::Order::Background)
            .fixed_pos(handle.min)
            .default_size(handle.size())
            .constrain(false)
            .fade_in(false)
            .show(ctx, |ui| {
                let response = ui.interact(
                    handle,
                    handle_id.with("grip"),
                    egui::Sense::click_and_drag(),
                );
                crate::drag::claim_if_held(&response);
                if response.double_clicked() {
                    Some(DividerAction::Even)
                } else if response.drag_stopped() {
                    Some(DividerAction::Release)
                } else if response.dragged() || response.is_pointer_button_down_on() {
                    response.interact_pointer_pos().map(DividerAction::Drag)
                } else {
                    None
                }
            })
            .inner;
        if let Some(action) = action {
            self.apply_divider(ctx, action, content, axis);
        }
    }
    /// A divider drag lives only while its handle is drawn and a finger is down. The handle stops
    /// being drawn while anything else moves (a push in either pane, home, the overview — any of
    /// them can arrive from another thread mid-drag), and then it never hears the finger lift:
    /// left alone, the divider would stay "under a finger" for good — a pane squeezed past its
    /// minimum and dimmed, the panes never told `Resized`. So the drag is let go here, where it
    /// stands, as a lift would have.
    pub(super) fn end_stray_divider_drag(&mut self, ctx: &egui::Context, content: Rect) {
        if !self.split.as_ref().is_some_and(Split::is_dragging) {
            return;
        }
        let held = ctx.input(|i| i.pointer.any_down()) && self.divider_handle.is_positive();
        if held {
            return;
        }
        let axis = self.axis_for(content);
        self.apply_divider(ctx, DividerAction::LetGo, content, axis);
    }
    /// Act on what the divider's handle was asked.
    pub(super) fn apply_divider(
        &mut self,
        ctx: &egui::Context,
        action: DividerAction,
        content: Rect,
        axis: SplitAxis,
    ) {
        let len = axis.along(content.size()).max(1.0);
        let mins = self.pane_mins(content, axis);
        let bounds = split::bounds(content, axis, self.split_metrics.0, mins);
        let tokens = self.tokens;
        match action {
            DividerAction::Drag(at) => {
                let ratio = (axis.coord(at) - axis.coord(content.min)) / len;
                let velocity = ctx.input(|i| axis.along(i.pointer.velocity())) / len;
                if let Some(split) = self.split.as_mut() {
                    split.drag(ratio, velocity);
                }
            }
            DividerAction::Release | DividerAction::LetGo => {
                let dismiss = matches!(action, DividerAction::Release);
                let released = self
                    .split
                    .as_mut()
                    .map(|split| split.release(bounds, tokens.spring, tokens.reduce, len, dismiss));
                if let Some(Released::Close(pane)) = released {
                    self.unsplit(1 - pane, &tokens);
                }
            }
            DividerAction::Even => {
                if let Some(split) = self.split.as_mut() {
                    split.even(bounds, &tokens);
                }
            }
        }
    }
}
