//! Render for `Shell`.

use super::{
    compute_layout, draw_icon_info, draw_prompt, draw_rail, draw_status_bar, draw_status_bar_area,
    input_route, is_user_input, leaves_overlay, overlay_screen, rebind_font_families, toast_anchor,
    AppRef, BarMode, CxParts, DesktopAction, Duration, Edge, EdgeMask, FrameActions, FrameWake,
    Gate, Gesture, GestureFrame, HeadsUpAction, Instant, LateOutput, LaunchAction, Layout,
    LayoutInput, Lifecycle, NavAction, NavStyle, NotificationId, OverlaySlot, OverlaySlotAction,
    OverviewMode, Phase, PolicyDriver, PromptAction, ReleaseRule, RepaintMode, Shell, ShellEvent,
    StatusBarAction, Task, ToastAction, EMERGENCY_GATE, LIFT_HOLD_MIN, LIFT_TRAVEL_MAX_MM,
    LIFT_TRAVEL_MIN_MM, LIFT_TRAVEL_RATIO, LIFT_TRAVEL_SHORT_SIDE, MAX_DT, MIN_WAKE,
    OVERLAY_OPEN_GATE, RECENTS_GATE, SLOW_FRAME, STALE_WAKE_FRAMES, STATUS_BAR_OVERLAY_AREA_ID,
};

impl Shell {
    pub(super) fn frame_inner(&mut self, ui: &mut egui::Ui, app: &mut AppRef<'_>) {
        let started = Instant::now();
        let ctx = ui.ctx().clone();
        // **Nothing is drawn until this crate's font families are live.**
        //
        // egui 0.37 applies `set_fonts` immediately before text layout, and otherwise at the
        // next pass. Building inside a pass therefore draws at once only when the families
        // are already live. A missing family would show replacement glyphs.
        //
        // Wait only during startup. If an integrator replaced the definitions after the
        // builder, rebind our two families after the first wait instead of staying blank.
        if self.frame_no == 0 && !crate::fonts::families_are_live(&ctx) {
            self.font_waits = self.font_waits.saturating_add(1);
            if self.font_waits > 1 {
                if self.font_waits == 2 {
                    log::warn!(
                        "[fonts] the font definitions were replaced after ShellBuilder::build and before the first frame, which unbinds fairing's strong and display families; they are bound again with the regular faces. Pass fonts with ShellBuilder::font / ShellBuilder::fonts instead of Context::set_fonts"
                    );
                }
                rebind_font_families(&ctx);
            }
            ctx.request_repaint();
            return;
        }
        // 0. The scale. Density → `pixels_per_point` → `Metrics`. Fixed for the whole of this frame.
        self.resolve_scale(&ctx, ui.max_rect());
        self.check_added_nav_items();
        // 1. Two clocks. Animations step by `stable_dt`, capped at 50 ms
        // so a stall never jumps one. The shell's monotonic time — every deadline: a temporary
        // unlock, the session timeout, the idle lock, a lockout, a toast's hold — runs on the
        // **real** time between frames. On an idle panel egui reports `stable_dt` as its 1/60 s
        // prediction however long it slept, so deadlines counted in it ran many times slower than
        // the wall clock: a five-minute unlock outlived the operator by hours. Headless,
        // `RawInput::time` is the harness's virtual time, so tests stay deterministic.
        self.frame_no += 1;
        self.check_script(&ctx);
        let dt = ui.input(|i| i.stable_dt).clamp(0.0, MAX_DT);
        self.last_dt = dt;
        let elapsed = ui.input(|i| i.unstable_dt);
        let elapsed = Duration::try_from_secs_f32(elapsed.max(0.0))
            .unwrap_or_else(|_| Duration::from_secs_f32(dt));
        self.advance_frame_time(elapsed);
        let now = self.mono;
        let input_activity =
            ui.input(|i| i.pointer.any_down() || i.events.iter().any(is_user_input));
        self.update_cursor(&ctx, now);
        // State processing may already have run in eframe::App::logic for this pass.
        let wake = self.frame_state(&ctx, input_activity, app);
        // 5.
        let screen = ui.max_rect();
        let wants_keyboard = ctx.egui_wants_keyboard_input();
        self.update_chrome(&ctx, screen, wants_keyboard, now, dt, app);
        // 6.
        let inset = self.osk.inset_bottom();
        let was = self.layout.content;
        self.layout = self.current_layout(screen);
        self.workspace.set_inset_bottom(inset);
        // **Say so when a Pane grows or shrinks**. A screen rotation, a window resize and
        // a transition to a screen with the chrome hidden all come in here. The first frame
        // (`Rect::NOTHING`) is excluded — the instance receives a `Created` first there, so the size can
        // be read from that.
        //
        // The OSK does not shrink `content` (the keyboard does not push the content and only
        // says so through `inset_bottom`). So raising and lowering the keypad does not leak this event.
        let now_content = self.layout.content;
        if was.is_positive() && was.size() != now_content.size() {
            // Each pane's instances hear their own size — the content's, or their share of it.
            self.workspace.content_resized(now_content);
        }
        self.open_rail_home(app);
        if self.region_frame_deferred {
            // egui already hit-tested last pass's widgets. Do not execute UI callbacks
            // against a newly captured press with stale widget ownership. Registering
            // only guards retires stale widgets before the next pass. Logic ran above.
            ctx.stop_dragging();
            self.finish_region_guards(&ctx);
            self.paint_regions(&ctx, now);
            self.finish_frame(
                &ctx,
                FrameActions {
                    desktop: None,
                    nav: None,
                    status: None,
                },
                LateOutput::default(),
                wake,
                app,
            );
            self.paint_display_notice(&ctx);
            self.trim_events();
            ctx.request_repaint();
            return;
        }
        let layout = self.layout;
        let was_home = self.workspace.is_home();
        // 7–9.
        let (out, nav_action, status_action) = self.draw_chrome(ui, &layout, now, app);
        // The screens the overview closed (a card thrown away, "Close all"), and home where that
        // ended the last task.
        for (id, instance) in &out.closed {
            self.events.push(ShellEvent::ScreenClosed {
                id: id.clone(),
                instance: *instance,
            });
        }
        if !out.closed.is_empty() {
            self.note_went_home(was_home);
        }
        // 10–13.
        let late = self.draw_late(&ctx, &layout, screen, now, app);
        self.finish_region_guards(&ctx);
        self.paint_regions(&ctx, now);
        self.finish_frame(
            &ctx,
            FrameActions {
                desktop: out.desktop_action,
                nav: nav_action,
                status: status_action,
            },
            late,
            wake,
            app,
        );
        self.paint_display_notice(&ctx);
        // 16. The events go out through poll_events.
        self.trim_events();
        let elapsed = started.elapsed();
        if elapsed > SLOW_FRAME {
            log::warn!(
                "slow frame #{}: {:.1} ms (the budget is 33 ms)",
                self.frame_no,
                elapsed.as_secs_f64() * 1000.0
            );
        }
    }
    // The ordinary egui adapter uses the compiled bitmap, not text layout/font loading.
    // egui shape submission/tessellation may still allocate; the raw CPU raster path is separate.
    pub(super) fn paint_display_notice(&self, ctx: &egui::Context) {
        let mut status = self.display_status;
        status.merge(self.handle.display_status(false));
        status.merge(self.icons.cache.display_status());
        #[cfg(feature = "osk")]
        status.merge(self.osk.0.display_status());
        status.merge(self.toasts.display_status());
        status.merge(self.heads_up.display_status());
        if !status.is_reduced() {
            return;
        }
        let (size, pixels) = crate::memory::display_warning_tile();
        let viewport = ctx.viewport_rect();
        if !viewport.is_finite() || !viewport.is_positive() {
            return;
        }
        let origin = viewport.left_top() + egui::vec2(4.0, 4.0);
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Tooltip,
                egui::Id::unique("fairing.display_notice"),
            ))
            .with_clip_rect(ctx.viewport_rect());
        painter.rect_filled(
            egui::Rect::from_min_size(origin, egui::vec2(192.0, 24.0)),
            0.0,
            egui::Color32::from_rgb(24, 24, 24),
        );
        for (y, row) in pixels.chunks(size[0]).enumerate() {
            for (x, pixel) in row.iter().enumerate() {
                if *pixel != [255, 208, 64, 255] {
                    continue;
                }
                let x = f32::from(u8::try_from(x).unwrap_or(0));
                let y = f32::from(u8::try_from(y).unwrap_or(0));
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        origin + egui::vec2(x * 2.0, y * 2.0),
                        egui::vec2(2.0, 2.0),
                    ),
                    0.0,
                    egui::Color32::from_rgb(255, 208, 64),
                );
            }
        }
    }
    /// Stage 5: gestures → the policy guard → the overlay → the back gesture → the OSK → advancing the
    /// animations → evict · rebuild → the overlay's boundary lifecycle.
    /// The things that run every frame (animations, transitions, the notification queue). Split out
    /// from [`Self::update_chrome`] — inlined there, one function would go over the length cap.
    pub(super) fn tick_animations(
        &mut self,
        dt: f32,
        now: Instant,
        tokens: &crate::theme::MotionTokens,
    ) {
        self.animations.tick(dt, self.frame_no);
        self.workspace.tick(dt);
        self.desktop.tick(dt);
        self.toasts
            .tick(dt, now, tokens, self.theme.components.toast);
        self.heads_up.tick(dt, now, tokens);
    }
    /// Current shell geometry, shared by gesture placement and final layout.
    pub(super) fn current_layout(&self, screen: egui::Rect) -> Layout {
        compute_layout(&LayoutInput {
            screen,
            policy: self.workspace.chrome_policy(),
            status_height: self.status_bar.enabled.then_some(self.status_bar.height),
            nav_height: self.nav_bar.enabled.then_some(self.nav_bar.height),
            edge_px: self.theme.metrics.edge_px,
            osk_height: (self.osk.inset_bottom() > 0.5).then_some(self.osk.inset_bottom()),
            rail: self.rail_input(),
        })
    }
    pub(super) fn update_chrome(
        &mut self,
        ctx: &egui::Context,
        screen: egui::Rect,
        wants_keyboard: bool,
        now: Instant,
        dt: f32,
        app: &mut AppRef<'_>,
    ) {
        let tokens = self.theme.motion;
        let policy = self.workspace.chrome_policy();
        let modal = self.prompt.is_open();
        // OSK (A5). The prompt is the shell's own, so the screen's keyboard policy does not
        // reach it — a screen that turned the keyboard off must not leave a password card mute.
        let osk_mode = if modal {
            crate::screen::OskMode::Auto
        } else {
            policy.osk
        };
        // The row cap is in millimetres, so it is this frame's resolved value. The keyboard sits
        // above the nav bar, so the room it has is the screen less that bar.
        let nav = if self.nav_bar.enabled && policy.nav_bar != BarMode::Hide {
            self.nav_bar.height
        } else {
            0.0
        };
        self.osk.set_bounds(
            self.theme.metrics.osk_max_key,
            self.theme.metrics.osk_key_gap,
            screen.height() - nav,
        );
        // The keys the focused field asked for — a PIN field gets the digits, a password the
        // letters, whatever the device's layout is. Nothing focused leaves the keys as they are,
        // so a keyboard going down does not change under the finger.
        if let Some(id) = ctx.memory(egui::Memory::focused) {
            self.osk.follow(crate::widgets::Keys::of(ctx, id));
        }
        self.osk
            .update(wants_keyboard, osk_mode, screen.height(), now, dt, &tokens);
        let gesture = self.read_gestures(ctx, screen, &policy, now);
        let gesture = self.behind_the_prompt(gesture);
        self.desktop_presses(ctx, gesture, &tokens);
        let guard = self
            .guard
            .update(&policy, screen, self.gestures.hold(), now);
        if guard.emergency && !modal {
            self.emergency();
        }
        if let Some(on) = self
            .guard
            .apply_keep_awake(&policy, self.services.display.as_mut())
        {
            log::debug!("keep_awake → DisplayBackend::set_idle_inhibit({on})");
        }
        // The hidden entry points' **corner knocks**. Caught from the raw pointer rather than
        // from a widget, so they work whatever a screen draws — on a kiosk screen with all the chrome
        // hidden too. They come **after** the gesture decision, so they do not intercept an edge swipe —
        // only a corner touched and released straight away is a knock. Behind the prompt the
        // corners are glass like everything else.
        if modal {
            self.tap_press = None;
            self.last_screen = screen;
        } else {
            self.poll_knocks(ctx, screen, now, app);
        }
        // The overlay takes the top pull first (the priority: the shade > a screen transition > a page).
        let status_hidden = policy.status_bar == BarMode::Hide && self.status_bar.enabled;
        let allowed = self.access.allows(&OVERLAY_OPEN_GATE);
        let consumed = self.overlay.update(
            gesture,
            overlay_screen(&self.layout),
            self.layout.content,
            status_hidden,
            self.status_bar.height,
            allowed,
            now,
            dt,
            &tokens,
        );
        if consumed {
            // Once the shade starts, the lower priorities (a screen transition, a page
            // swipe) are finished at once — so that two do not run together.
            if matches!(
                gesture,
                Some(Gesture::EdgeSwipe {
                    phase: Phase::Started,
                    ..
                })
            ) {
                self.workspace.settle_transitions();
                self.desktop.settle_page();
            }
        } else {
            self.gesture_back_in_pane(ctx, gesture, screen);
            self.bottom_gestures(ctx, gesture);
        }
        // The touch in a region, every frame to its end — one the regions stopped being in play
        // under is told it was taken.
        self.region_gestures(ctx, gesture, now, app);
        // Advance the animations and transitions.
        self.tick_animations(dt, now, &tokens);
        if let Some(palette) = self.theme_fade.tick(dt) {
            self.theme.palette = palette;
            self.theme_dirty = true;
        }
        // **Outside the crossfade branch.** It used to sit inside it, so the only thing that could
        // re-apply the theme was a palette animation — a metric or a text size changed at run time
        // never reached egui's style.
        if std::mem::take(&mut self.theme_dirty) {
            self.theme.apply(ctx);
        }
        for (id, instance) in self.workspace.evict_expired(now, &tokens) {
            self.events.push(ShellEvent::ScreenClosed { id, instance });
        }
        self.rebuild_registry();
        self.note_cover();
        let shown = self.osk.is_shown();
        if shown != self.osk_was_shown {
            self.osk_was_shown = shown;
            self.events.push(ShellEvent::OskToggled(shown));
        }
        let split = self.workspace.is_split();
        if split != self.split_was {
            self.split_was = split;
            self.events.push(ShellEvent::SplitToggled(split));
        }
        self.status_bar
            .set_unread(u32::try_from(self.notifications.unread()).unwrap_or(u32::MAX));
    }
    /// Stage 5's first step: the gesture engine's reading of this frame. A run of quick switches
    /// ends here too, on a press anywhere but the indicator's edge.
    pub(super) fn read_gestures(
        &mut self,
        ctx: &egui::Context,
        screen: egui::Rect,
        policy: &crate::screen::ChromePolicy,
        now: Instant,
    ) -> Option<Gesture> {
        let layout = self.current_layout(screen);
        let zones = layout.edge_zones;
        let blocked = PolicyDriver::blocked_edges(policy);
        let previous = self.region_zones;
        self.region_zones = self.place_regions(screen, &layout, zones, blocked);
        let changed = previous != self.region_zones;
        let (fresh_press, claimed) = if changed {
            ctx.input(|input| {
                let fresh = input.pointer.primary_pressed();
                (
                    fresh,
                    fresh
                        && crate::drag::press_point(input)
                            .is_some_and(|at| self.region_zones.at(at).is_some()),
                )
            })
        } else {
            (false, false)
        };
        self.region_frame_deferred = input_route::defer_ordinary_ui(changed, fresh_press, claimed);
        // Register current guard geometry before any user widgets are drawn.
        self.guard_regions(ctx);
        let gesture = self.gestures.update(
            ctx,
            &GestureFrame {
                screen,
                edge_zones: zones,
                blocked,
                slide_edges: self.slide_edges(),
                regions: self.region_zones,
                now,
            },
        );
        if self.gesture_nav() {
            let elsewhere = ctx.input(|i| {
                i.pointer.any_pressed()
                    && crate::drag::press_point(i).is_some_and(|at| {
                        zones
                            .get(Edge::Bottom.index())
                            .is_some_and(|z| !z.contains(at))
                    })
            });
            if elsewhere {
                self.workspace.end_quick_switch();
            }
        }
        gesture
    }
    /// The desktop's share of this frame's input: a press over the info popover puts it
    /// away, a long press goes to the cells about to be drawn, and the popover goes the moment
    /// something takes the desktop's place — the shade, the prompt, a transition, the cards.
    pub(super) fn desktop_presses(
        &mut self,
        ctx: &egui::Context,
        gesture: Option<Gesture>,
        tokens: &crate::theme::MotionTokens,
    ) {
        if ctx.input(|i| i.pointer.any_pressed()) {
            self.desktop.note_press(tokens);
        }
        if let Some(Gesture::LongPress { pos }) = gesture {
            self.desktop.hold_at(pos);
        }
        let covered = !self.overlay.is_closed()
            || self.prompt.is_open()
            || self.workspace.is_animating()
            || self.workspace.is_overview_open();
        if covered {
            self.desktop.close_info(tokens);
        }
    }
    /// Rebuild before commands, and again if declarations changed during a frame.
    pub(super) fn rebuild_registry(&mut self) {
        if self.registry.take_dirty() {
            self.desktop.rebuild(&self.registry, &self.config.desktop);
            self.overlay.mark_tiles_dirty();
        }
    }
    /// **Behind the prompt nothing new starts** (A9: the prompt is the top priority). A
    /// swipe that was already running when it opened still gets its end, so the shade or the back
    /// transition can settle instead of hanging mid-drag.
    pub(super) fn behind_the_prompt(&mut self, gesture: Option<Gesture>) -> Option<Gesture> {
        match gesture {
            Some(
                Gesture::EdgeSwipe {
                    phase: Phase::Started,
                    ..
                }
                | Gesture::EdgeSlide {
                    phase: Phase::Started,
                    ..
                },
            ) if self.prompt.is_open() => {
                self.gestures.cancel();
                None
            }
            other => other,
        }
    }
    /// The overlay's `Closed` boundary, and the focused screen's cover: Paused while
    /// anything covers it — the shade leaving `Closed`, or the prompt — and Resumed once
    /// nothing does. One boundary for both, so a prompt over an open shade does not pause twice.
    pub(super) fn note_cover(&mut self) {
        let closed = self.overlay.is_closed();
        if closed != self.overlay_was_closed {
            self.overlay_was_closed = closed;
            if !closed {
                self.notifications.mark_seen();
                self.heads_up.absorb();
            }
            self.events.push(ShellEvent::OverlayToggled(!closed));
        }
        let covered = !closed || self.prompt.is_open();
        if covered != self.focus_covered {
            self.focus_covered = covered;
            self.workspace.set_covered(covered);
            // Under the cards the screens stay paused when the shade or the prompt goes — the
            // overview resumes them itself, when it goes.
            if covered || !self.workspace.is_overview_open() {
                self.workspace.queue_focused(if covered {
                    Lifecycle::Paused
                } else {
                    Lifecycle::Resumed
                });
            }
        }
    }
    /// Whether the nav bar is gestures (`[nav_bar] style = "gesture"`). The gestures
    /// come with the bar: one turned off (`enabled = false`) takes them with it, as it does its
    /// buttons — a screen hiding the bar (`ChromePolicy`) does not.
    pub(super) fn gesture_nav(&self) -> bool {
        self.nav_bar.enabled && matches!(self.nav_bar.style, NavStyle::Gesture { .. })
    }
    /// The edges a slide along them is recognised on: the bottom, where the nav bar is gestures —
    /// the home indicator's left and right.
    pub(super) fn slide_edges(&self) -> EdgeMask {
        if self.gesture_nav() {
            EdgeMask::NONE.with(Edge::Bottom)
        } else {
            EdgeMask::NONE
        }
    }
    /// **The bottom edge's gestures**, where the nav bar is gestures: up is home — the
    /// screen on show following the finger — up and a pause is the recent screens, and along the
    /// home indicator is the task used before or after the one on show.
    pub(super) fn bottom_gestures(&mut self, ctx: &egui::Context, gesture: Option<Gesture>) {
        if !self.gesture_nav() {
            return;
        }
        // Over an open shade the press is the shade's; on the on-screen keyboard's keys — where
        // the edge zone is deeper than the band the keyboard sits on — the keys'.
        let starting = matches!(
            gesture,
            Some(
                Gesture::EdgeSwipe {
                    edge: Edge::Bottom,
                    phase: Phase::Started,
                    ..
                } | Gesture::EdgeSlide {
                    edge: Edge::Bottom,
                    phase: Phase::Started,
                    ..
                }
            )
        );
        if starting && (!self.overlay.is_closed() || self.press_on_keys(ctx)) {
            self.gestures.cancel();
            return;
        }
        match gesture {
            Some(Gesture::EdgeSwipe {
                edge: Edge::Bottom,
                progress,
                velocity,
                phase,
            }) => self.bottom_swipe(ctx, progress, velocity, phase),
            Some(Gesture::SwipeHold { edge: Edge::Bottom }) => self.bottom_hold(),
            Some(Gesture::EdgeSlide {
                edge: Edge::Bottom,
                offset,
                velocity,
                phase,
            }) => self.indicator_slide(offset, velocity, phase),
            _ => {}
        }
    }
    /// Whether this press went down on the on-screen keyboard's keys.
    pub(super) fn press_on_keys(&self, ctx: &egui::Context) -> bool {
        let press = ctx.input(|i| i.pointer.press_origin());
        self.osk.is_shown()
            && self
                .layout
                .osk
                .is_some_and(|keys| press.is_some_and(|at| keys.contains(at)))
    }
    /// Up from the bottom edge. Over a task the screen on show follows the finger (the lift); at
    /// home and over the cards nothing does, and the release alone says what it was. Let go far
    /// enough up, or flung up, it is home; short of that, the screen goes back down.
    pub(super) fn bottom_swipe(
        &mut self,
        ctx: &egui::Context,
        progress: f32,
        velocity: f32,
        phase: Phase,
    ) {
        let tokens = self.theme.motion;
        match phase {
            Phase::Started => {
                self.bottom_held = false;
                if !self.workspace.is_home() && !self.workspace.is_overview_open() {
                    let press = ctx.input(|i| i.pointer.press_origin());
                    if let Some(press) = press {
                        let travel = self.lift_travel();
                        let _ = self.workspace.begin_lift(press, travel);
                    }
                }
                self.follow_lift(ctx, velocity);
            }
            Phase::Moved => self.follow_lift(ctx, velocity),
            Phase::Ended | Phase::Cancelled => {
                if std::mem::take(&mut self.bottom_held) {
                    return;
                }
                let rule = ReleaseRule {
                    snap_ratio: tokens.snap_ratio,
                    fling: tokens.fling_px_s,
                };
                let home =
                    phase == Phase::Ended && rule.confirm(progress / self.lift_travel(), velocity);
                if home {
                    self.home_from_swipe();
                } else {
                    self.workspace.lift_back(&tokens);
                }
            }
        }
    }
    /// The lift follows the finger: its offset from where it went down.
    pub(super) fn follow_lift(&mut self, ctx: &egui::Context, velocity_up: f32) {
        if !self.workspace.is_lifting() {
            return;
        }
        let at = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.press_origin()));
        if let (Some(pos), Some(press)) = at {
            self.workspace.drag_lift(pos - press, velocity_up);
        }
    }
    /// A bottom swipe let go into home: a lifted screen carries on into its icon; anything else
    /// is the home button's.
    pub(super) fn home_from_swipe(&mut self) {
        if !self.workspace.is_lifting() {
            self.home();
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&self.theme.motion);
        }
        let icon_rect = self
            .workspace
            .active_task()
            .and_then(|t| t.root_id().map(str::to_owned))
            .and_then(|id| self.desktop.icon_rect(&id));
        if self.workspace.lift_home(icon_rect, &self.theme.motion) {
            self.events.push(ShellEvent::WentHome);
        }
    }
    /// Up and a pause: the recent screens, as the recents button — reported, gated, and only
    /// where `[workspace] overview` is on — the lifted screen carrying on into its card. A pause
    /// too low down is not one yet: the swipe may still pause again further up. At home and
    /// over a screen that did not lift, "how far up" is the finger's, over the lift's travel.
    pub(super) fn bottom_hold(&mut self) {
        let up = if self.workspace.is_lifting() {
            self.workspace.lift_progress()
        } else {
            self.gestures.swipe_progress().unwrap_or(0.0) / self.lift_travel()
        };
        if up < LIFT_HOLD_MIN {
            return;
        }
        if self.bottom_held || self.workspace.is_overview_open() {
            return;
        }
        self.bottom_held = true;
        self.events.push(ShellEvent::OverviewRequested);
        let tokens = self.theme.motion;
        if !self.config.workspace.overview {
            self.workspace.lift_back(&tokens);
            return;
        }
        if !self.access.allows(&RECENTS_GATE) {
            self.workspace.lift_back(&tokens);
            self.request_unlock(RECENTS_GATE, Some(LaunchAction::OpenOverview));
            return;
        }
        if !self.overlay.is_closed() {
            self.overlay.close(&tokens);
        }
        let mode = OverviewMode::Recents {
            beside: self.may_enter_split(),
        };
        if self.workspace.is_lifting() {
            self.workspace.lift_to_overview(mode, &self.theme);
        } else {
            self.workspace.open_overview(mode, &tokens);
        }
    }
    /// Along the home indicator: the screen on show slides away and the task used before it
    /// (to the right) or after it (to the left) slides in. At home, a slide brings back the task
    /// used last. With nothing to go to, the press is let go.
    pub(super) fn indicator_slide(&mut self, offset: f32, velocity: f32, phase: Phase) {
        let tokens = self.theme.motion;
        let width = self.workspace.focused_pane_rect().width().max(1.0);
        match phase {
            Phase::Started => {
                if !self.workspace.is_home() && !self.workspace.begin_switch(width) {
                    self.gestures.cancel();
                    return;
                }
                self.workspace.drag_switch(offset, velocity);
            }
            Phase::Moved => self.workspace.drag_switch(offset, velocity),
            Phase::Ended | Phase::Cancelled => {
                let rule = ReleaseRule {
                    snap_ratio: tokens.snap_ratio,
                    fling: tokens.fling_px_s,
                };
                let through = phase == Phase::Ended
                    && rule.confirm(offset.abs() / width, velocity * offset.signum());
                if self.workspace.is_switching() {
                    self.workspace.release_switch(through, &tokens);
                } else if through && self.workspace.is_home() {
                    self.resume_most_recent();
                }
            }
        }
    }
    /// The task used last, back on show — from home, zooming out of its icon where it has one.
    pub(super) fn resume_most_recent(&mut self) {
        let Some(index) = self.workspace.most_recent_task() else {
            return;
        };
        let icon_rect = self
            .workspace
            .tasks()
            .get(index)
            .and_then(Task::root_id)
            .and_then(|id| self.desktop.icon_rect(id));
        let _ = self
            .workspace
            .resume_task(index, icon_rect, &self.theme.motion);
    }
    /// How far up a lift reaches the card scale (A11's `D`): 35 % of the
    /// pane's height, between 15 and 60 mm, and no more than 60 % of the screen's short side.
    pub(super) fn lift_travel(&self) -> f32 {
        let pane = self.workspace.focused_pane_rect();
        let mm = self.scale.du_per_mm.max(0.1);
        let short = self.last_screen.width().min(self.last_screen.height());
        (LIFT_TRAVEL_RATIO * pane.height())
            .clamp(LIFT_TRAVEL_MIN_MM * mm, LIFT_TRAVEL_MAX_MM * mm)
            .min(LIFT_TRAVEL_SHORT_SIDE * short)
            .max(1.0)
    }
    /// The back gesture, where the shade did not take it. Split, it is the pane's it starts on,
    /// measured across that pane.
    pub(super) fn gesture_back_in_pane(
        &mut self,
        ctx: &egui::Context,
        gesture: Option<Gesture>,
        screen: egui::Rect,
    ) {
        // Over the cards a back swipe is back: they go down, and nothing under them is popped —
        // the screen behind a card is not on show to be swiped away.
        if self.workspace.is_overview_open() {
            if let Some(Gesture::EdgeSwipe {
                edge,
                phase: Phase::Started,
                ..
            }) = gesture
            {
                if self.back_edges.contains(edge) && !matches!(edge, Edge::Top | Edge::Bottom) {
                    self.gestures.cancel();
                    self.workspace.close_overview(&self.theme.motion);
                }
            }
            return;
        }
        let width = if self.workspace.is_split() {
            if matches!(
                gesture,
                Some(Gesture::EdgeSwipe {
                    phase: Phase::Started,
                    ..
                })
            ) {
                if let Some(at) = ctx.input(crate::drag::press_point) {
                    self.workspace.focus_at(ctx, at);
                }
            }
            self.workspace.focused_pane_rect().width()
        } else {
            screen.width()
        };
        self.gesture_back(gesture, width);
    }
    /// The back gesture (A3, M2): it hands an edge swipe on one of `back_edges` to the workspace.
    /// `p = dx / W`, confirmed at `p ≥ snap_ratio` or `v_x ≥ fling`.
    pub(super) fn gesture_back(&mut self, gesture: Option<Gesture>, width: f32) {
        let Some(Gesture::EdgeSwipe {
            edge,
            progress,
            velocity,
            phase,
        }) = gesture
        else {
            return;
        };
        if !self.back_edges.contains(edge) || matches!(edge, Edge::Top | Edge::Bottom) {
            return;
        }
        let w = width.max(1.0);
        // The new press's own `dx / W`: the workspace adds the `p` it caught a returning screen at
        // (A3 "taking hold again mid-cancel") itself.
        let dx = progress / w;
        match phase {
            Phase::Started => {
                if !self.workspace.begin_gesture_back() {
                    self.gestures.cancel();
                    return;
                }
                self.workspace.drag_gesture_back(dx, velocity / w);
            }
            Phase::Moved => self.workspace.drag_gesture_back(dx, velocity / w),
            Phase::Ended | Phase::Cancelled => {
                // The release goes by the real progress: the caught `p` and the new `dx / W`.
                let p = (self.workspace.gesture_back_grab() + dx).clamp(0.0, 1.0);
                let tokens = self.theme.motion;
                let rule = ReleaseRule {
                    snap_ratio: tokens.snap_ratio,
                    fling: tokens.fling_px_s,
                };
                let confirm = phase == Phase::Ended && rule.confirm(p, velocity);
                // Only what the release itself took off the stack is closed: where the gesture
                // was already ended under the finger, the screen now on show stays open.
                if let Some((id, instance)) = self.workspace.release_gesture_back(confirm, &tokens)
                {
                    self.events.push(ShellEvent::ScreenClosed { id, instance });
                }
            }
        }
    }
    /// The emergency gesture completing: where `chrome.emergency` passes it opens the shade,
    /// otherwise an unlock request — the prompt, in `prompt` mode.
    pub(super) fn emergency(&mut self) {
        let gate = Gate::borrowed(EMERGENCY_GATE);
        self.events.push(ShellEvent::Emergency);
        if self.access.allows(&gate) {
            self.overlay.open(&self.theme.motion);
        } else {
            self.request_unlock(gate, Some(LaunchAction::OpenOverlay));
        }
    }
    /// Stages 7–9: the status bar (a panel, or an Overlay `Area`) · the nav bar · the workspace. It
    /// splits the fields up to make a `CxParts` (the panel closures borrow different fields).
    pub(super) fn draw_chrome(
        &mut self,
        ui: &mut egui::Ui,
        layout: &Layout,
        now: Instant,
        app: &mut AppRef<'_>,
    ) -> (
        crate::workspace::WorkspaceOutput,
        Option<NavAction>,
        Option<StatusBarAction>,
    ) {
        let ctx = ui.ctx().clone();
        let root = ui.max_rect();
        let frame_no = self.frame_no;
        let status_opacity = self.status_opacity();
        let Self {
            status_bar,
            nav_bar,
            workspace,
            desktop,
            services,
            theme,
            access,
            registry,
            animations,
            icons,
            widget_painters,
            settings,
            strings,
            handle,
            requests,
            hidden_remaining,
            status_bar_painter,
            nav_bar_painter,
            nav_bar_layout,
            recents_painters,
            bar_rows,
            ..
        } = self;
        let mut parts = CxParts {
            shell: handle,
            access,
            services,
            settings,
            app: app.as_deref_mut(),
            strings,
            theme,
            icons,
            animations,
            widget_painters,
            requests,
            hidden: hidden_remaining,
            now,
            frame: frame_no,
        };
        let mut nav_action = None;
        let mut status_action = None;
        if let Some(rect) = layout.status {
            status_action = draw_status_bar(
                ui,
                &ctx,
                rect,
                layout.status_overlay,
                status_opacity,
                status_bar,
                &mut parts,
                registry,
                bar_rows,
                status_bar_painter.as_mut(),
            );
        } else {
            status_bar.mark_hidden();
        }
        if let Some(rect) = layout.nav {
            // Back is ignored on the desktop; this shows that as the back button's disabled look.
            nav_bar.back_enabled = !workspace.is_home();
            if nav_bar_painter.is_some() {
                nav_bar.collect_items(registry.nav_items(), parts.access, bar_rows);
            }
            egui::Panel::bottom(egui::Id::unique("fairing.nav_bar.panel"))
                .exact_size(rect.height())
                .resizable(false)
                .show_separator_line(false)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    if let Some(painter) = nav_bar_painter.as_mut() {
                        nav_bar.ui_with(ui, &mut parts, bar_rows, painter);
                    } else {
                        let items = registry.nav_items_mut();
                        nav_action = nav_bar.ui(ui, &mut parts, items, nav_bar_layout.as_mut());
                    }
                });
        } else {
            nav_bar.mark_hidden();
        }
        // The rail is **outside** the workspace — it is drawn every frame and a screen does not cover
        // it. That is the only difference from the grid, and it is all there is to the kiosk layout.
        let rail_action = layout
            .rail
            .and_then(|rect| draw_rail(ui, rect, desktop, workspace, &mut parts));
        let mut out = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                workspace.ui(ui, &mut parts, registry, desktop, layout, recents_painters)
            })
            .inner;
        // A rail tap wins — where the grid was pressed on the same frame, it is hidden behind the rail and out of sight.
        if rail_action.is_some() {
            out.desktop_action = rail_action;
        }
        draw_icon_info(&ctx, root, layout, desktop, &mut parts);
        (out, nav_action, status_action)
    }
    /// The status bar's opacity this frame. The A1 fade means something only in the layout where
    /// the shade covers the status bar (`BarMode::Overlay`); a bar with a band of its own stays sharp
    /// to the end, since the shade comes down below it.
    pub(super) fn status_opacity(&self) -> f32 {
        if self.layout.status_overlay {
            self.overlay.status_bar_opacity()
        } else {
            1.0
        }
    }
    /// Stages 10–13: the OSK → the overlay (the scrim, the panel, the status row/peek, the shield) →
    /// the prompt → the heads-up and the toasts. `root` is the whole screen.
    pub(super) fn draw_late(
        &mut self,
        ctx: &egui::Context,
        layout: &Layout,
        root: egui::Rect,
        now: Instant,
        app: &mut AppRef<'_>,
    ) -> LateOutput {
        let frame_no = self.frame_no;
        let status_opacity = 1.0;
        let screen = overlay_screen(layout);
        let Self {
            status_bar,
            workspace: _,
            services,
            theme,
            access,
            registry,
            animations,
            icons,
            widget_painters,
            settings,
            strings,
            handle,
            requests,
            hidden_remaining,
            status_bar_painter,
            bar_rows,
            overlay,
            osk,
            notifications,
            toasts,
            heads_up,
            gestures,
            guard,
            prompt,
            ..
        } = self;
        let mut parts = CxParts {
            shell: handle,
            access,
            services,
            settings,
            app: app.as_deref_mut(),
            strings,
            theme,
            icons,
            animations,
            widget_painters,
            requests,
            hidden: hidden_remaining,
            now,
            frame: frame_no,
        };
        // 10. While the prompt is up the keyboard is drawn after it instead, above it.
        let modal = prompt.is_open();
        let mut late = LateOutput {
            osk_hide: !modal && osk.ui(ctx, layout.osk, parts.theme, parts.icons),
            ..LateOutput::default()
        };
        // 11.
        #[cfg(feature = "overlay")]
        let tiles = registry.tiles();
        #[cfg(not(feature = "overlay"))]
        let tiles: &[()] = &[];
        let out = overlay.ui(ctx, &mut parts, screen, notifications, tiles);
        late.overlay = out.action;
        if let Some(rect) = out.status_row.or(out.peek) {
            // The status row is panel content, so what is outside the curtain (the panel's visible area) is not revealed yet.
            let clip = out.panel.map_or(rect, |panel| rect.intersect(panel));
            let action = draw_status_bar_area(
                ctx,
                rect,
                clip,
                status_opacity,
                status_bar,
                &mut parts,
                registry,
                bar_rows,
                status_bar_painter.as_mut(),
            );
            if action.is_some() {
                late.status_row_tapped = true;
            }
            if out.status_row.is_some() {
                // egui 0.36.1's `Area::begin` raises a pressed Area to the top and `end_pass` sorts
                // stably — where `fairing.status_bar.overlay` has a history of being made before the
                // panel (a `BarMode::Overlay` screen → a `Hide` screen), the status row can end up below
                // the panel, so it is pinned just above the panel on every pass.
                OverlaySlot::pin_status_row_above_panel(ctx, STATUS_BAR_OVERLAY_AREA_ID);
            }
        }
        gestures.shield(ctx);
        // The emergency gesture's progress ring (the A7 long-press convention) — a draw-only layer that takes no input.
        guard.ui(ctx, parts.theme);
        // 12. The prompt, over everything but the heads-up and the toasts (z-order 5).
        late.prompt = draw_prompt(ctx, prompt, &mut parts, layout, root);
        if modal {
            // The password card's fields want the keyboard, and the keyboard has to be **above**
            // the modal that is taking every press — so it goes up a layer for as long as the
            // prompt is up.
            osk.raise(true);
            late.osk_hide = osk.ui(ctx, layout.osk, parts.theme, parts.icons);
            osk.raise(false);
        }
        // 13.
        let tokens = parts.theme.motion;
        late.heads_up = heads_up.ui(
            ctx,
            screen,
            parts.theme,
            &tokens,
            now,
            parts.icons,
            parts.strings,
            parts.access,
        );
        let anchor = toast_anchor(layout, parts.theme);
        late.toast = toasts.ui(ctx, anchor, parts.theme, parts.icons, parts.strings);
        late
    }
    /// Stages 14–15: handle the outputs and requests, propagate the lifecycle, apply the repaint policy.
    pub(super) fn finish_frame(
        &mut self,
        ctx: &egui::Context,
        actions: FrameActions,
        late: LateOutput,
        wake: FrameWake,
        app: &mut AppRef<'_>,
    ) {
        let FrameActions {
            desktop: desktop_action,
            nav: nav_action,
            status: status_action,
        } = actions;
        // 14.
        match status_action {
            Some(StatusBarAction::ItemTapped(id)) => {
                if let Some(action) = self.status_bar.tap_action(&id).cloned() {
                    self.launch_in(action, app);
                }
            }
            Some(StatusBarAction::Tapped) if self.status_bar.tap_opens_shade => {
                // An open split shade crosses to the panel on the tapped side; anything else
                // toggles, as it always did.
                if !self.overlay.bar_tapped(&self.theme.motion) {
                    self.toggle_overlay(app);
                }
            }
            Some(StatusBarAction::Tapped) | None => {}
        }
        match desktop_action {
            Some(DesktopAction::Tap(id)) => self.launch_decl(&id, app),
            Some(DesktopAction::TapLocked(id)) => {
                let then = self.action_for(&id);
                let gate = self.gate_of(&id);
                self.request_unlock(gate, then);
            }
            // The desktop has already answered it — its info popover is up unless `[desktop]
            // long_press = "none"` — and it goes out either way, for a menu of the
            // device's own.
            Some(DesktopAction::LongPress(id)) => {
                self.events.push(ShellEvent::IconLongPressed { id });
            }
            None => {}
        }
        match nav_action {
            Some(NavAction::Back) => self.back_in(app),
            Some(NavAction::Home) => self.home(),
            // The nav bar's own two unbuilt items are **reported** rather than greyed out — the
            // crate has no overview and no split pane yet, but the device may well have somewhere to
            // go, and a control the shell refuses to pass on is the shell deciding.
            Some(NavAction::Recents) => self.open_overview(),
            Some(NavAction::Split) => self.toggle_split(),
            Some(NavAction::Custom(id)) if id == "osk" => self.osk.toggle(),
            Some(NavAction::Custom(id)) => {
                // A `nav_item` closure takes the tap with its own widget. What arrives here is a
                // tap within the item's area **the closure did not consume**, so the shell has nothing to do.
                log::debug!("tap inside nav_item `{id}` - its closure did not consume it");
            }
            None => {}
        }
        if late.osk_hide {
            self.osk.hide();
        }
        match late.overlay {
            Some(OverlaySlotAction::Launch(action)) => {
                // **Where the action takes you elsewhere, the shade goes up first.** Press settings and
                // have the shade still covering it, and there is no telling the screen even opened.
                // Operating a value (`Set` · `Toggle`), conversely, normally means touching several in a
                // row, so it is left open.
                if leaves_overlay(&action) {
                    self.overlay.close(&self.theme.motion);
                }
                self.launch_in(action, app);
            }
            Some(OverlaySlotAction::TileLongPressed(id)) => {
                // The event is raised whether or not there is somewhere to go — an integrator who wants
                // to put a menu up, or do something else, need only take this.
                let action = self.tile_long_press(&id);
                self.events.push(ShellEvent::TileLongPressed { id });
                if let Some(action) = action {
                    if leaves_overlay(&action) {
                        self.overlay.close(&self.theme.motion);
                    }
                    self.launch_in(action, app);
                }
            }
            Some(OverlaySlotAction::TileLocked(gate)) => {
                self.request_unlock(gate, None);
            }
            Some(OverlaySlotAction::NotificationTapped(id)) => self.notification_tapped(id, app),
            Some(OverlaySlotAction::Dismiss(id)) => {
                self.dismiss_notification(id);
            }
            Some(OverlaySlotAction::ClearAll) => self.notifications.clear(),
            None => {}
        }
        match late.heads_up {
            // A banner over the lock screen is something to read, not a way past it.
            Some(HeadsUpAction::Tapped(id)) if !self.prompt.is_lock_screen() => {
                self.notification_tapped(id, app);
            }
            Some(HeadsUpAction::Tapped(_) | HeadsUpAction::Swiped(_)) | None => {}
        }
        if let Some(ToastAction::Tapped(index)) = late.toast {
            self.toasts.dismiss(index, &self.theme.motion);
        }
        match late.prompt {
            Some(PromptAction::Submit(credential)) => self.submit_credential(credential, app),
            Some(PromptAction::Cancel) => self.cancel_prompt(),
            Some(PromptAction::Continue) => self.continue_from_lock(app),
            None => {}
        }
        self.settle_requests(app);
        let wake = FrameWake {
            next_wake: self.services.next_wake(),
            ..wake
        };
        self.schedule_repaint(ctx, wake);
        // 16. The events go out through poll_events.
    }
    /// Stage 15: the repaint policy.
    pub(super) fn schedule_repaint(&mut self, ctx: &egui::Context, wake: FrameWake) {
        // Request an immediate update for active animation, pending commands/requests
        // or continuous mode. Otherwise choose the next clock, backend or UI deadline.
        // The host may also schedule updates independently of this shell decision.
        let animating = self.is_animating()
            || wake.commands_pending
            || !self.requests.is_empty()
            || self.config.shell.repaint == RepaintMode::Continuous;
        if animating {
            ctx.request_repaint();
            self.stale_wake_frames = 0;
            return;
        }
        let mut after = self.clock_wake();
        if let Some(at) = wake.next_wake {
            // An overdue zero-delay wake would request another immediate update.
            // Apply a delay floor and diagnose repeated overdue backend deadlines.
            let stale = at <= wake.now;
            self.note_stale_wake(stale, wake.now);
            let wake_in = at.saturating_duration_since(wake.now).max(MIN_WAKE);
            after = Some(after.map_or(wake_in, |a| a.min(wake_in)));
        } else {
            self.stale_wake_frames = 0;
        }
        let mut deadline = |at: Option<Duration>| {
            if let Some(d) = at {
                let d = d.max(MIN_WAKE);
                after = Some(after.map_or(d, |a| a.min(d)));
            }
        };
        deadline(
            self.toasts
                .next_deadline()
                .map(|t| t.saturating_duration_since(wake.now)),
        );
        deadline(
            self.heads_up
                .next_deadline()
                .map(|t| t.saturating_duration_since(wake.now)),
        );
        deadline(self.overlay.peek_remaining(wake.now));
        // A session timer running out on a panel nobody touches — a temporary unlock, the
        // timeout, the idle lock — and the lockout's countdown.
        let at = |t: Option<Instant>| t.map(|t| t.saturating_duration_since(wake.now));
        deadline(at(self.access.next_deadline()));
        deadline(at(self.prompt.next_deadline(wake.now)));
        // A stopped screen's `evict_after` running out (checked at stage 5).
        deadline(at(self.workspace.next_eviction()));
        if self.prompt.is_lock_screen() {
            // The lock screen's clock, at the next minute.
            deadline(Some(Duration::from_secs(
                self.services.clock.now().secs_to_next_minute(),
            )));
        }
        // An automatic keyboard counting down to hide after the focus left.
        deadline(at(self.osk.hide_deadline(&self.theme.motion)));
        if self.gestures.hold().is_some()
            || self.gestures.is_active()
            || self.toasts.has_room_for_next()
        {
            // While something is held, the timers are re-evaluated every frame (long presses, the
            // emergency gesture) — and while an edge swipe or a touch in a region is under way: a
            // finger standing still on it sends nothing, and its pause (`SwipeHold`, the recent
            // screens in the gesture navigation, a gesture handle's long gesture,
            // a region's own) is only seen by frames that keep coming. A waiting toast with a free
            // slot comes in on the next frame.
            deadline(Some(Duration::from_millis(16)));
        }
        if let Some(after) = after {
            ctx.request_repaint_after(after);
        }
    }
    /// A notification tap (in the shade's list, or a heads-up): run the action plus raise the event.
    pub(super) fn notification_tapped(&mut self, id: NotificationId, app: &mut AppRef<'_>) {
        if let Some(action) = self.notifications.get(id).and_then(|n| n.action.clone()) {
            // A notification tap follows the same convention — opening a screen has the shade stand aside.
            if leaves_overlay(&action) {
                self.overlay.close(&self.theme.motion);
            }
            self.launch_in(action, app);
        }
        self.events.push(ShellEvent::NotificationTapped(id));
    }
    /// Toggle the shade. Opening it passes the `overlay.open` gate.
    pub(super) fn toggle_overlay(&mut self, app: &mut AppRef<'_>) {
        if self.overlay.is_closed() {
            self.launch_in(LaunchAction::OpenOverlay, app);
        } else {
            self.overlay.close(&self.theme.motion);
        }
    }
    /// The clock boundary for the idle repaint. It has a value only where a clock item was **drawn on
    /// this frame** (waking at the clock's next minute boundary is a rule for when the clock is
    /// visible) — with the status bar off, the chrome policy `Hide`, or the clock invisible through a
    /// gate or a collapse, nothing is woken.
    pub(super) fn clock_wake(&self) -> Option<Duration> {
        // `shows_seconds`, not `== Hms`: the 12-hour spelling of the same shape needs waking just as
        // often. The `ui.clock_12h` setting never changes which of the two it is.
        if self.status_bar.drawn_clock()?.shows_seconds() {
            // WallTime has no fractional-second phase. Request a 500 ms wake to
            // reduce skipped seconds when updates are delayed; host scheduling can
            // still postpone a UI pass.
            return Some(Duration::from_millis(500));
        }
        Some(Duration::from_secs(
            self.services.clock.now().secs_to_next_minute(),
        ))
    }
    /// Count the frames `next_wake` points into the past. Every [`STALE_WAKE_FRAMES`] in a row it warns
    /// once which backend it is.
    pub(super) fn note_stale_wake(&mut self, stale: bool, now: Instant) {
        if !stale {
            self.stale_wake_frames = 0;
            return;
        }
        self.stale_wake_frames += 1;
        if !self.stale_wake_frames.is_multiple_of(STALE_WAKE_FRAMES) {
            return;
        }
        let frames = self.stale_wake_frames;
        self.services.for_each_stale(now, |name| {
            log::warn!(
                "backend `{name}` has had next_wake in the past for {frames} frames - the shell cannot go idle (guide 06 §1)"
            );
        });
    }
}
