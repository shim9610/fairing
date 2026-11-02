//! State processing shared by UI frames and eframe's logic-only callbacks.

use super::{
    compute_layout, AppRef, FrameWake, LayoutInput, Shell, ShellEvent, LIFECYCLE_ROUNDS, MIN_WAKE,
};
use std::time::{Duration, Instant};

pub(super) struct LogicState {
    prepared: Option<PreparedLogic>,
}

struct PreparedLogic {
    wake: FrameWake,
    pass: u64,
    input_time: u64,
}

impl Shell {
    /// Process commands, backends and access deadlines without drawing or consuming input.
    ///
    /// Call this from `eframe::App::logic`, before each corresponding [`Self::frame`],
    /// including when no UI pass follows. The built-in shell/state runners do this for you.
    /// The following UI pass reuses the state update, rather than polling backends twice.
    ///
    /// This opts into a wall-clock timeline: egui leaves input time and events unchanged
    /// during logic-only calls. Hosts using only `frame` retain egui's input timeline,
    /// including deterministic `RawInput::time` in headless tests.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.logic_with(ctx, &mut ());
    }

    /// [`Self::logic`] with the app state lent to actions and lifecycle callbacks.
    /// Pair with [`Self::frame_with`] using the same state.
    pub fn logic_with(&mut self, ctx: &egui::Context, app: &mut dyn std::any::Any) {
        if self.faulted {
            return;
        }
        self.contain_fault(|shell| shell.logic_at(ctx, &mut Some(app), Instant::now()));
        if self.faulted {
            crate::runtime::blank_faulted_frame(ctx);
        }
    }

    pub(super) fn logic_at(&mut self, ctx: &egui::Context, app: &mut AppRef<'_>, wall: Instant) {
        self.advance_wall_time(wall);
        let wake = self.poll_state(false, app);
        self.seed_background_layout(ctx);
        // State/lifecycle work must finish even if a compositor never allows another UI pass.
        for (id, instance) in self.workspace.evict_expired(self.mono, &self.theme.motion) {
            self.events.push(ShellEvent::ScreenClosed { id, instance });
        }
        self.settle_requests(app);
        self.trim_events();
        let wake = FrameWake {
            next_wake: self.services.next_wake(),
            ..wake
        };
        self.schedule_logic(ctx, wake);
        if let Some(state) = self.logic_state.as_mut() {
            state.prepared = Some(PreparedLogic {
                wake,
                pass: ctx.cumulative_pass_nr(),
                input_time: ctx.input(|i| i.time.to_bits()),
            });
        }
    }

    fn seed_background_layout(&mut self, ctx: &egui::Context) {
        if self.layout.content.is_positive() {
            return;
        }
        // Created can precede the first UI pass. Give it the current viewport (or the
        // builder's fallback) instead of Rect::NOTHING. A different UI size then emits
        // Resized, rather than silently retaining an invalid initial size forever.
        let screen = ctx
            .input(|i| i.viewport().inner_rect)
            .filter(egui::Rect::is_positive)
            .map_or(self.last_screen, |r| {
                egui::Rect::from_min_size(egui::Pos2::ZERO, r.size())
            });
        self.layout = compute_layout(&LayoutInput {
            screen,
            policy: self.workspace.chrome_policy(),
            status_height: self.status_bar.enabled.then_some(self.status_bar.height),
            nav_height: self.nav_bar.enabled.then_some(self.nav_bar.height),
            edge_px: self.theme.metrics.edge_px,
            osk_height: None,
            rail: self.rail_input(),
        });
        self.workspace.seed_content(self.layout.content);
    }

    fn advance_wall_time(&mut self, wall: Instant) {
        self.logic_state
            .get_or_insert(LogicState { prepared: None });
        let elapsed = wall.saturating_duration_since(self.clock_wall);
        self.clock_wall = self.clock_wall.max(wall);
        self.mono = self.mono.checked_add(elapsed).unwrap_or(self.mono);
    }

    pub(super) fn advance_frame_time(&mut self, elapsed: Duration) {
        if self.logic_state.is_some() {
            // Never add the UI gap again after logic-only calls already accounted for it.
            self.advance_wall_time(Instant::now());
        } else {
            self.mono = self.mono.checked_add(elapsed).unwrap_or(self.mono);
            self.clock_wall = Instant::now();
        }
    }

    pub(super) fn frame_state(
        &mut self,
        ctx: &egui::Context,
        input_activity: bool,
        app: &mut AppRef<'_>,
    ) -> FrameWake {
        let prepared = self.logic_state.as_mut().and_then(|s| s.prepared.take());
        if let Some(prepared) = prepared.filter(|p| {
            p.pass == ctx.cumulative_pass_nr() && p.input_time == ctx.input(|i| i.time.to_bits())
        }) {
            // Integrator event handlers may change the subject/policy after App::logic.
            // Recheck before drawing, and record only this UI pass's fresh input.
            self.rebuild_registry();
            self.observe_subject_change(app);
            self.expire_access(self.mono);
            if input_activity {
                self.access.record_activity(self.mono);
            }
            FrameWake {
                now: self.mono,
                ..prepared.wake
            }
        } else {
            self.poll_state(input_activity, app)
        }
    }

    fn poll_state(&mut self, input_activity: bool, app: &mut AppRef<'_>) -> FrameWake {
        let now = self.mono;
        self.rebuild_registry();
        self.observe_subject_change(app);
        self.expire_access(now);
        let commands_pending = self.drain_commands(app);
        let next_wake = self.services.poll(now);
        self.reconcile_pending_bools();
        self.tick_access(now, input_activity, app);
        FrameWake {
            next_wake,
            now,
            commands_pending,
        }
    }

    pub(super) fn settle_requests(&mut self, app: &mut AppRef<'_>) {
        self.process_requests(app);
        self.flush_lifecycle(app);
        for _ in 0..LIFECYCLE_ROUNDS {
            if self.requests.is_empty() {
                break;
            }
            self.process_requests(app);
            self.flush_lifecycle(app);
        }
    }

    fn schedule_logic(&mut self, ctx: &egui::Context, wake: FrameWake) {
        // No animation, old pointer state, visual countdown or continuously repainting
        // screen may keep a hidden window spinning. Workers still wake it via Waker.
        let mut after = (wake.commands_pending || !self.requests.is_empty()).then_some(MIN_WAKE);
        if let Some(at) = wake.next_wake {
            self.note_stale_wake(at <= wake.now, wake.now);
        } else {
            self.stale_wake_frames = 0;
        }
        for at in [
            wake.next_wake,
            self.access.next_deadline(),
            self.workspace.next_eviction(),
        ]
        .into_iter()
        .flatten()
        {
            let delay = at.saturating_duration_since(wake.now).max(MIN_WAKE);
            after = Some(after.map_or(delay, |other| other.min(delay)));
        }
        if let Some(after) = after {
            ctx.request_repaint_after(after);
        }
    }
}
