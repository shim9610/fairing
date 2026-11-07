//! Gestures for `Workspace`.

use super::{
    root_key, HomeFrame, InstanceId, Lifecycle, Lift, MotionTokens, Overview, OverviewMode, Pos2,
    QuickOrder, Rect, StackTransition, Switch, Task, Workspace, WorkspaceView, LIFT_SPLIT_FADE,
};

impl Workspace {
    /// Start the back gesture (A3, M2): only with a stack of 2 or more and no transition running.
    /// On starting, `Paused` on the one leaving (the top). `true` if it started.
    ///
    /// **Mid-cancel it is taken hold of where it stands** (A3, "taking hold again during a
    /// cancelled gesture continues from the current p"). The `dx / W` of the
    /// [`Workspace::drag_gesture_back`] that follows is added to the `p` it was caught at — the
    /// finger starts again from the screen's left edge but the screen carries on from where it was.
    ///
    /// **It does not start at the root (a stack of 1).** Popping to the root ends the task and goes
    /// home, which is an A2 close (the icon zoom), whose render mapping is nothing like
    /// A3's horizontal 1:1 following, so the finger cannot be carried straight over. On a `false`
    /// the shell releases this press with [`GestureEngine::cancel`] so the widget below receives it —
    /// the back button and `back()` still go home.
    ///
    /// [`GestureEngine::cancel`]: crate::gesture::GestureEngine::cancel
    pub(crate) fn begin_gesture_back(&mut self) -> bool {
        // Under the cards nothing is on show to be swiped away.
        if self.overview.is_some() {
            return false;
        }
        if let Some(p) = self.stack.regrab_back() {
            self.back_grab = p;
            if let Some(top) = self.active_task_mut().and_then(Task::top_mut) {
                top.queue(Lifecycle::Paused);
            }
            return true;
        }
        if self.view != WorkspaceView::Tasks || self.is_animating() {
            return false;
        }
        let Some(task) = self.active_task_mut() else {
            return false;
        };
        if task.len() < 2 {
            return false;
        }
        if let Some(top) = task.top_mut() {
            top.queue(Lifecycle::Paused);
        }
        self.back_grab = 0.0;
        self.stack.begin_back();
        true
    }
    /// Gesture progress: `p = clamp(dx / W, 0, 1)`, `v_p = v_x / W`. On a gesture taken hold of
    /// again, the `p` it was caught at is added.
    pub(crate) fn drag_gesture_back(&mut self, p: f32, v_p: f32) {
        self.stack
            .drag_back((self.back_grab + p).clamp(0.0, 1.0), v_p);
    }
    /// The `p` the back gesture under way was taken hold of again at (A3 re-entry; 0 means a fresh
    /// gesture, or none under way).
    #[doc(hidden)]
    #[must_use]
    pub fn gesture_back_grab(&self) -> f32 {
        self.back_grab
    }
    /// The release: confirmed at `p ≥ 0.33`, or at `v_x ≥ fling` (the caller decides and hands it
    /// over as `confirm`). Confirmed, the top comes off the stack as the outgoing instance;
    /// cancelled, it is restored with `Resumed`.
    ///
    /// The (declaration id, instance id) of what came off the stack. `None` for a cancel, and for
    /// a release whose gesture something else already ended (a `back` from elsewhere mid-drag):
    /// nothing was closed by it.
    pub fn release_gesture_back(
        &mut self,
        confirm: bool,
        tokens: &MotionTokens,
    ) -> Option<(String, InstanceId)> {
        if !matches!(
            self.stack,
            StackTransition::DraggingBack {
                confirmed: None,
                ..
            }
        ) {
            return None;
        }
        // Let go, the gesture is over: the next one starts fresh unless it catches this one.
        self.back_grab = 0.0;
        let outgoing = if confirm {
            self.active_task_mut().and_then(Task::pop)
        } else {
            if let Some(top) = self.active_task_mut().and_then(Task::top_mut) {
                top.queue(Lifecycle::Resumed);
            }
            None
        };
        let closed = outgoing.as_ref().map(|o| (o.decl_id().to_owned(), o.id()));
        // The outgoing slot takes len so it does not collide with the new top (len−1).
        let base = Self::slot_base(self.focus);
        let outgoing = outgoing.map(|mut o| {
            let len = self.active_task().map_or(0, Task::len);
            o.set_layer_slot(base.saturating_add(u32::try_from(len).unwrap_or(u32::MAX)));
            o
        });
        // The width W is the focused pane's, as last drawn (= the content's with one pane — the same
        // value as the shell's `dx / W`). Where not one frame has been drawn yet (a headless unit test),
        // 1 — the old absolute decision as it was.
        let width = self.focused_pane_rect().width();
        let width = if width.is_finite() && width > 1.0 {
            width
        } else {
            1.0
        };
        self.stack
            .release_back(confirm, outgoing, tokens.spring, tokens.reduce, width);
        if tokens.reduce {
            self.settle_stack();
        }
        closed
    }
    /// **Start the lift**: the screen on show follows a finger up from the bottom edge,
    /// shrinking as it rises — to the overview's card scale `travel` du up. Only over a task with
    /// nothing moving; `Paused` on the screens on show. `true` where it started.
    pub(crate) fn begin_lift(&mut self, press: Pos2, travel: f32) -> bool {
        if self.view != WorkspaceView::Tasks
            || self.overview.is_some()
            || self.lift.is_some()
            || self.switch.is_some()
            || self.is_animating()
            || self.active_task().is_none()
        {
            return false;
        }
        self.queue_focused(Lifecycle::Paused);
        self.lift = Some(Lift::new(press, travel));
        true
    }
    /// The lifting finger is `offset` du from where it went down, rising at `velocity_up` du/s.
    pub(crate) fn drag_lift(&mut self, offset: egui::Vec2, velocity_up: f32) {
        if let Some(lift) = self.lift.as_mut() {
            lift.drag(offset, velocity_up);
        }
    }
    /// Whether a lift is on — the finger's, or springing back.
    #[must_use]
    pub fn is_lifting(&self) -> bool {
        self.lift.is_some()
    }
    /// How far up the lift is, `0..=1` (0 with none).
    #[doc(hidden)]
    #[must_use]
    pub fn lift_progress(&self) -> f32 {
        self.lift.as_ref().map_or(0.0, Lift::progress)
    }
    /// The lift let go short of home: the screen springs back down, and is `Resumed` there.
    pub(crate) fn lift_back(&mut self, tokens: &MotionTokens) {
        if let Some(lift) = self.lift.as_mut() {
            lift.release(tokens.spring, tokens.reduce);
            if tokens.reduce {
                self.finish_lift();
            }
        }
    }
    /// The lift let go into home: the A2 close carries on from where the finger left the screen
    /// (one pane), or from the panes as they stand (two). `false` with no lift.
    pub(crate) fn lift_home(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) -> bool {
        let Some(lift) = self.lift.take() else {
            return false;
        };
        self.layers_dirty = true;
        let from = self
            .split
            .is_none()
            .then(|| lift.rect(self.focused_pane_rect()));
        self.leave_for_home(icon_rect, from, tokens);
        true
    }
    /// The lift held: the overview comes up, the screen carrying on from where the finger has it
    /// into its card. It was paused when the lift began, so it is not paused again.
    pub(crate) fn lift_to_overview(&mut self, mode: OverviewMode, theme: &crate::theme::Theme) {
        let Some(lift) = self.lift.take() else {
            return;
        };
        self.layers_dirty = true;
        let tokens = theme.motion;
        let current = self.active_task().and_then(root_key);
        let mut overview = Overview::open(current, mode, false, &tokens);
        if self.split.is_none() && !tokens.reduce {
            overview.carry_from(lift.rect(self.last_content));
        }
        self.overview = Some(overview);
    }
    /// The lift is back down: the screens on show go on.
    pub(super) fn finish_lift(&mut self) {
        if self.lift.take().is_some() {
            self.layers_dirty = true;
            self.queue_focused(Lifecycle::Resumed);
        }
    }
    /// The lift over this frame's home values: one pane scales about the press point
    /// and moves with the finger; two fade where they stand, as under the overview.
    pub(super) fn lifted(&self, mut home: HomeFrame) -> HomeFrame {
        let Some(lift) = self.lift.as_ref() else {
            return home;
        };
        if self.split.is_some() {
            home.screen_opacity *= 1.0 - LIFT_SPLIT_FADE * lift.progress();
        } else {
            home.screen_scale *= lift.scale();
            home.screen_pivot = Some(lift.pivot());
            home.screen_shift += lift.shift();
        }
        home
    }
    /// A launch, back or close takes over: a lift or a switch under way is dropped where it is —
    /// the screen on show back in place and going on.
    pub(super) fn drop_gestures(&mut self) {
        if self.lift.take().is_some() {
            self.layers_dirty = true;
            self.queue_focused(Lifecycle::Resumed);
        }
        if let Some(switch) = self.switch.take() {
            self.layers_dirty = true;
            if switch.confirmed() == Some(true) {
                // Let go through already: it lands now.
                self.switch = Some(switch);
                self.finish_switch();
            } else {
                self.queue_focused(Lifecycle::Resumed);
            }
        }
    }
    /// The tasks' root ids by when they were last used, the one on show first.
    pub(super) fn tasks_by_recency(&self) -> Vec<InstanceId> {
        let mut order: Vec<usize> = (0..self.tasks.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.tasks.get(i).map(Task::last_active)));
        if let Some(current) = self.pane_task_index() {
            order.retain(|&i| i != current);
            order.insert(0, current);
        }
        order
            .into_iter()
            .filter_map(|i| self.tasks.get(i).and_then(root_key))
            .collect()
    }
    /// The task used last — what a slide along the indicator at home brings back.
    #[must_use]
    pub fn most_recent_task(&self) -> Option<usize> {
        (0..self.tasks.len()).max_by_key(|&i| self.tasks.get(i).map(Task::last_active))
    }
    /// **Start the quick switch**: the screen on show slides with a finger along the
    /// indicator, and the task used before it (to the right) or after it (to the left) slides in
    /// beside it. One pane, over a task, with nothing moving and another task to go to. `Paused`
    /// on the screen on show; `true` where it started.
    pub(crate) fn begin_switch(&mut self, width: f32) -> bool {
        if self.view != WorkspaceView::Tasks
            || self.split.is_some()
            || self.overview.is_some()
            || self.lift.is_some()
            || self.switch.is_some()
            || self.is_animating()
        {
            return false;
        }
        let Some(from) = self.active_task().and_then(root_key) else {
            return false;
        };
        // A run goes on while it stands on the task on show; anything else began a new one.
        if self.quick.as_ref().and_then(QuickOrder::current) != Some(from) {
            self.quick = Some(QuickOrder::new(self.tasks_by_recency()));
        }
        let (older, newer) = self.quick.as_ref().map_or((None, None), |run| {
            run.neighbours(|key| self.task_by_key(key).is_some())
        });
        if older.is_none() && newer.is_none() {
            return false;
        }
        self.queue_focused(Lifecycle::Paused);
        self.switch = Some(Switch::new(from, older, newer, width));
        true
    }
    /// The sliding finger is `offset` du along the indicator from where it went down, at
    /// `velocity` du/s.
    pub(crate) fn drag_switch(&mut self, offset: f32, velocity: f32) {
        if let Some(switch) = self.switch.as_mut() {
            switch.drag(offset, velocity);
        }
    }
    /// The slide let go: through to the task coming in where `through` (and there is one that
    /// way), or back.
    pub(crate) fn release_switch(&mut self, through: bool, tokens: &MotionTokens) {
        if let Some(switch) = self.switch.as_mut() {
            switch.release(through, tokens.spring, tokens.reduce);
            if tokens.reduce {
                self.finish_switch();
            }
        }
    }
    /// Whether a quick switch is on — the finger's, or landing.
    #[must_use]
    pub fn is_switching(&self) -> bool {
        self.switch.is_some()
    }
    /// How far the quick switch has slid, as a fraction of the pane's width (positive to the
    /// right; 0 with none).
    #[doc(hidden)]
    #[must_use]
    pub fn switch_progress(&self) -> f32 {
        self.switch.as_ref().map_or(0.0, Switch::progress)
    }
    /// The run of quick switches is over — the screen on show was touched, or something else
    /// moved on. The next slide starts from the tasks as they were last used.
    pub(crate) fn end_quick_switch(&mut self) {
        self.quick = None;
    }
    /// The switch has landed: through, the task that came in is on show (`Resumed`) and the one
    /// that went is `Stopped`; back, the one on show goes on.
    pub(super) fn finish_switch(&mut self) {
        let Some(switch) = self.switch.take() else {
            return;
        };
        self.layers_dirty = true;
        let incoming = switch
            .incoming()
            .filter(|_| switch.confirmed() == Some(true));
        let Some((key, index)) = incoming.and_then(|key| Some((key, self.task_by_key(key)?)))
        else {
            self.queue_focused(Lifecycle::Resumed);
            return;
        };
        if let Some(previous) = self
            .task_by_key(switch.from)
            .and_then(|i| self.tasks.get_mut(i))
        {
            for instance in previous.iter_mut() {
                instance.queue(Lifecycle::Stopped);
            }
        }
        self.set_pane_task(Some(index));
        if let Some(task) = self.tasks.get_mut(index) {
            task.touch();
            if let Some(top) = task.top_mut() {
                top.queue(Lifecycle::Resumed);
            }
        }
        // Its icon is not where it came from; home finds it again.
        self.origin = None;
        if let Some(run) = self.quick.as_mut() {
            run.step_to(key);
        }
    }
}
