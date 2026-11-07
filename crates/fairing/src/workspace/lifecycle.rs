//! Lifecycle for `Workspace`.

use super::{
    pane_info, screen_rect, transition, CxParts, Easing, HomeTransition, Instance, InstanceId,
    Instant, Lifecycle, MotionTokens, PaneId, Phase, Rect, SplitSupport, StackTransition,
    TSTransform, Task, Tween, Workspace, WorkspaceView,
};

impl Workspace {
    /// Finish every running transition at once — called by the shell when it starts something of
    /// **higher priority** (the shade). The same thing a new launch / back / close
    /// does before starting.
    pub(crate) fn settle_transitions(&mut self) {
        self.settle();
    }
    /// Finish every running transition at once ("there is no queue"). The interruption rule:
    /// a new launch / back / close mid-transition settles here and then starts. The exception
    /// (A2's reverse continuation) is handled directly by [`Workspace::go_home`] and
    /// [`Workspace::resume_task`].
    pub(super) fn settle(&mut self) {
        self.settle_home();
        self.settle_stack();
        self.settle_split();
        self.drop_gestures();
    }
    /// Land a split on its way in or out, and a divider still springing (the same "no queue" rule).
    pub(super) fn settle_split(&mut self) {
        let Some(split) = self.split.as_mut() else {
            return;
        };
        if split.is_dragging() || !split.is_moving() {
            return;
        }
        let phase = split.phase();
        // Run it out: the longest step a tick takes, until nothing moves (a spring may take a
        // second or two of steps; the guard is only against one that never settles).
        let mut left = None;
        let mut guard = 0;
        while split.is_moving() && guard < 4096 {
            left = left.or(split.tick(crate::motion::MAX_DT));
            guard += 1;
        }
        match (phase, left) {
            (_, Some(pane)) | (Phase::Leaving(pane), None) => self.finish_leaving(pane),
            (Phase::Entering(_), None) => self.finish_entering(),
            (Phase::Split, None) => {}
        }
    }
    pub(super) fn settle_home(&mut self) {
        if self.home.is_active() {
            let closing = matches!(self.home, HomeTransition::Closing { .. });
            self.home.finish();
            self.finish_home(closing);
        }
    }
    pub(super) fn settle_stack(&mut self) {
        let clearing = self.is_clearing();
        if clearing {
            self.finish_clearing();
        }
        if self.stack.is_active() {
            if let Some(out) = self.stack.finish() {
                self.bury(out);
            }
        } else if !clearing {
            return;
        }
        self.finish_stack();
    }
    /// Send the instances being cleared by the clear-top cross-fade to the graveyard (`Destroyed`).
    pub(super) fn finish_clearing(&mut self) {
        self.clear_fade.snap(1.0);
        for instance in std::mem::take(&mut self.clearing) {
            self.bury(instance);
        }
    }
    /// Start an A2 open (immediately under reduce). It records the `origin`.
    pub(super) fn start_home_open(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        self.origin = icon_rect;
        if tokens.reduce {
            self.finish_home(false);
        } else {
            self.home.open(icon_rect, tokens.home_open);
        }
    }
    /// Start an A2 close (immediately under reduce). Mid-`Opening` it reverses from the same `t`.
    pub(super) fn start_home_close(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        if tokens.reduce {
            self.finish_home(true);
        } else {
            self.home.close(icon_rect, tokens.home_close);
        }
    }
    /// Open a new instance. From home, the root of a new task plus an A2; in Tasks, a push plus an
    /// A3. A transition in progress is finished first (there is no queue).
    pub fn open(&mut self, instance: Instance, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        self.settle();
        // A screen that is never split takes the whole content: the split ends first.
        if self.split.is_some() && instance.split_support() == SplitSupport::No {
            self.collapse_to_focused();
        }
        if self.view == WorkspaceView::Home || self.active_task().is_none() {
            self.tasks.push(Task::new(instance));
            let index = self.tasks.len() - 1;
            self.set_pane_task(Some(index));
            self.view = WorkspaceView::Tasks;
            self.start_home_open(icon_rect, tokens);
        } else {
            if let Some(task) = self.active_task_mut() {
                // At the start the side leaving (losing focus) gets `Paused` only — `Stopped` comes at the end.
                if let Some(top) = task.top_mut() {
                    top.queue(Lifecycle::Paused);
                }
                task.push(instance);
            }
            if tokens.reduce {
                self.finish_stack();
            } else {
                self.stack.push(tokens.push);
            }
        }
    }
    /// Bring a task whose root is `decl_id` to the front, if there is one (an icon re-tapped from
    /// home). To search whole stacks, [`Workspace::find_task`] plus [`Workspace::resume_task`].
    #[cfg(test)]
    pub(crate) fn resume_task_with_root(
        &mut self,
        decl_id: &str,
        icon_rect: Option<Rect>,
        tokens: &MotionTokens,
    ) -> bool {
        match self.tasks.iter().position(|t| t.root_id() == Some(decl_id)) {
            Some(index) => self.resume_task(index, icon_rect, tokens),
            None => false,
        }
    }
    /// Bring task `index` to the front (a `Single` screen already in another task brings that task
    /// forward).
    ///
    /// - From home, an A2 open. Where it is **the very task being A2-closed**, it reverses from the
    ///   same `t` and opens again (the A2 interruption rule — after the `Paused` already sent, a
    ///   `Resumed` at the end).
    /// - Switching to another task in the Tasks view gives the previous task a `Paused` on top then
    ///   `Stopped` throughout, and the new task's top a `Resumed` — **immediately** (the
    ///   overview's own motion is A10's).
    /// - The stack is unchanged (clear-top happens only when the screen is in the current task).
    ///   Already at the front, it does nothing and returns `true`; out of range, `false`.
    pub(crate) fn resume_task(
        &mut self,
        index: usize,
        icon_rect: Option<Rect>,
        tokens: &MotionTokens,
    ) -> bool {
        if index >= self.tasks.len() {
            return false;
        }
        self.settle_stack();
        let from_home = self.view == WorkspaceView::Home;
        let previous = self.pane_task_index();
        if !from_home && previous == Some(index) {
            return true;
        }
        // Shown in the other pane already: it only takes the focus.
        if !from_home && self.split.is_some() {
            let other = 1 - self.focus.min(1);
            if self.panes.get(other).and_then(|p| p.task) == Some(index) {
                self.focus_pane(other);
                return true;
            }
        }
        let closing_same = from_home
            && matches!(self.home, HomeTransition::Closing { .. })
            && previous == Some(index);
        if !closing_same {
            self.settle_home();
        }
        if let Some(prev) = previous
            .filter(|_| !from_home)
            .and_then(|p| self.tasks.get_mut(p))
        {
            if let Some(top) = prev.top_mut() {
                top.queue(Lifecycle::Paused);
            }
            for instance in prev.iter_mut() {
                instance.queue(Lifecycle::Stopped);
            }
        }
        self.set_pane_task(Some(index));
        if let Some(task) = self.tasks.get_mut(index) {
            task.touch();
        }
        self.view = WorkspaceView::Tasks;
        if from_home {
            // On `closing_same`, `HomeTransition::open` carries on from `1 − t`.
            self.start_home_open(icon_rect, tokens);
        } else {
            self.origin = icon_rect.or(self.origin);
            self.finish_home(false);
        }
        true
    }
    /// If `decl_id` is inside the current task, pop everything above it and resume (`Single`'s
    /// clear-top). A transition in progress is finished first.
    ///
    /// The screens cleared away disappear in a **cross-fade** over `tokens.clear_top` (160 ms
    /// `CubicOut`). Whichever was on top gets a `Paused` at the start, all of them
    /// a `Destroyed` at the end, and the new top a `Resumed` at the end ("notifications
    /// during a transition come at the end of it"). Immediately under `reduce`.
    ///
    /// The tokens are last frame's, cached by [`Workspace::ui`] — the shell hands this entry point
    /// none.
    pub(crate) fn clear_top_to(&mut self, decl_id: &str) -> bool {
        if self
            .active_task()
            .is_none_or(|task| task.find(decl_id).is_none())
        {
            return false;
        }
        self.settle();
        let Some(task) = self.active_task_mut() else {
            return false;
        };
        if task.find(decl_id).is_none() {
            return false;
        }
        let popped = task.clear_top(decl_id);
        if popped.is_empty() {
            // Already at the top — nothing to clear away, so no transition either.
            if let Some(top) = task.top_mut() {
                top.queue(Lifecycle::Resumed);
            }
            return true;
        }
        for (i, mut instance) in popped.into_iter().enumerate() {
            if i == 0 {
                instance.queue(Lifecycle::Paused);
            }
            self.clearing.push(instance);
        }
        let tween = self.tokens.clear_top;
        if self.tokens.reduce || tween.duration.is_zero() {
            self.finish_clearing();
            self.finish_stack();
        } else {
            self.clear_fade.snap(0.0);
            self.clear_fade.to(
                1.0,
                Tween {
                    duration: tween.duration,
                    easing: Easing::Linear,
                },
            );
            self.layers_dirty = true;
        }
        true
    }
    /// How many instances are being cleared by the clear-top cross-fade (0 = it is not running).
    #[must_use]
    pub fn clearing(&self) -> usize {
        self.clearing.len()
    }
    /// The current opacity of the screen being cleared, `1 → 0`. `None` while it is not running.
    #[must_use]
    pub fn clear_top_alpha(&self) -> Option<f32> {
        self.is_clearing()
            .then(|| transition::clear_top_alpha(self.clear_fade.value(), &self.tokens))
    }
    /// Pop the top (back, `cx.finish`). With the stack emptied, it goes home (an A2 close). `true` if
    /// a pop happened. The one leaving gets a `Paused` at the start and a `Destroyed` at the end; the
    /// new top a `Resumed` at the end.
    pub fn pop(&mut self, tokens: &MotionTokens) -> bool {
        self.settle();
        let base = Self::slot_base(self.focus);
        let Some(task) = self.active_task_mut() else {
            return false;
        };
        if task.len() <= 1 {
            // Popped to the root: the task ends and it goes home.
            return self.close_active_task(tokens);
        }
        let Some(mut outgoing) = task.pop() else {
            return false;
        };
        outgoing.queue(Lifecycle::Paused);
        // The outgoing one is drawn off the stack, so its slot is stated (= the length after the pop, different from the new top's).
        let depth = u32::try_from(task.len()).unwrap_or(u32::MAX);
        outgoing.set_layer_slot(base.saturating_add(depth));
        if tokens.reduce {
            self.bury(outgoing);
            self.finish_stack();
        } else {
            self.stack.pop(outgoing, tokens.pop);
        }
        true
    }
    /// Close a particular instance (`cx.finish`). On the top it is the same as a pop; otherwise it is
    /// removed quietly (with no animation — a transition in progress is finished).
    pub(crate) fn close_instance(&mut self, id: InstanceId, tokens: &MotionTokens) -> bool {
        if self
            .active_task()
            .and_then(Task::top)
            .is_some_and(|t| t.id() == id)
        {
            return self.pop(tokens);
        }
        !self
            .close_where(|instance| instance.id() == id, tokens)
            .is_empty()
    }
    /// The home button: the task stays alive (`Stopped`) and can be returned to from recents. An A2
    /// close (the fallback with no icon Rect). **Mid-A2-open** it reverses from the same `t` (A2).
    /// Already home (a close in progress included), it does nothing.
    pub(crate) fn go_home(&mut self, icon_rect: Option<Rect>, tokens: &MotionTokens) {
        if self.overview.is_some() {
            // Home from the overview: straight there — the screens are already in their cards.
            self.drop_overview();
            if self.view == WorkspaceView::Tasks {
                self.view = WorkspaceView::Home;
                self.finish_home(true);
            }
            return;
        }
        if self.view == WorkspaceView::Home {
            return;
        }
        self.settle_stack();
        self.settle_split();
        self.drop_gestures();
        // Split, both panes go with it: the focused one closes on its icon, the other fades
        // with the screens (A2), and the split ends once home is reached.
        self.queue_focused(Lifecycle::Paused);
        self.leave_for_home(icon_rect, None, tokens);
    }
    /// The A2 close to home, from the pane — or from `from`, a lifted screen let go.
    pub(super) fn leave_for_home(
        &mut self,
        icon_rect: Option<Rect>,
        from: Option<Rect>,
        tokens: &MotionTokens,
    ) {
        self.view = WorkspaceView::Home;
        let target = icon_rect.or(self.origin);
        self.origin = target;
        match from {
            Some(from) if !tokens.reduce => self.home.close_from(target, from, tokens.home_close),
            _ => self.start_home_close(target, tokens),
        }
    }
    /// End the active task (popping to the root, an Overview swipe). The task comes straight off the
    /// stack (it is in neither `find` nor `tasks`) and is drawn as `Workspace::closing_task` only
    /// through the A2 close, then `Destroyed` at the end of the transition. The origin is the last
    /// icon Rect the shell handed over (`Workspace::origin`).
    pub(crate) fn close_active_task(&mut self, tokens: &MotionTokens) -> bool {
        self.close_active_task_with(self.origin, tokens)
    }
    /// Hand [`Workspace::close_active_task`] the A2 close's origin (the icon Rect) directly.
    pub(crate) fn close_active_task_with(
        &mut self,
        icon_rect: Option<Rect>,
        tokens: &MotionTokens,
    ) -> bool {
        self.settle();
        let Some(index) = self.pane_task_index() else {
            return false;
        };
        if index >= self.tasks.len() {
            return false;
        }
        let mut task = self.tasks.remove(index);
        self.shift_panes_after_remove(index);
        if let Some(top) = task.top_mut() {
            top.queue(Lifecycle::Paused);
        }
        // Split, the task ends with its pane: it slides out and the other pane fills the content
        // ("a stack emptied ends the split"). Home is not where this goes.
        if self.split.is_some() && self.panes.len() == 2 {
            let leaving = self.focus;
            self.leaving_task = Some(task);
            self.start_leaving(leaving, tokens);
            return true;
        }
        self.view = WorkspaceView::Home;
        self.origin = None;
        if tokens.reduce {
            self.bury_task(task);
            self.finish_home(true);
        } else {
            self.closing_task = Some(task);
            self.home.close(icon_rect, tokens.home_close);
        }
        true
    }
    /// The resident screen of a declaration that is being replaced or removed goes to its open
    /// instance, wherever that is — on a stack, leaving through a transition or waiting for its
    /// `Destroyed` — so what that instance hears from now on reaches the screen it ran, not
    /// whatever the registry holds under the id next. Without an instance it is dropped here.
    pub(crate) fn adopt_resident(&mut self, decl_id: &str, screen: crate::screen::ScreenHandle) {
        let outgoing = match &mut self.stack {
            StackTransition::Popping { outgoing, .. }
            | StackTransition::DraggingBack { outgoing, .. } => outgoing.as_deref_mut(),
            StackTransition::Idle | StackTransition::Pushing { .. } => None,
        };
        let mut all = self
            .tasks
            .iter_mut()
            .chain(self.closing_task.iter_mut())
            .chain(self.leaving_task.iter_mut())
            .flat_map(Task::iter_mut)
            .chain(outgoing)
            .chain(self.graveyard.iter_mut());
        if let Some(instance) = all.find(|i| i.decl_id() == decl_id && i.is_resident()) {
            instance.adopt(screen);
        }
    }
    /// `remove(id)`: end every instance of that declaration. No animation. With none at all, a
    /// transition in progress is left alone.
    pub(crate) fn close_decl(&mut self, decl_id: &str, tokens: &MotionTokens) {
        let _closed = self.close_where(|instance| instance.decl_id() == decl_id, tokens);
    }
    /// End every instance matching a condition (an instance failing its gate on a session downgrade,
    /// `evict_after`). No animation. It hands back the (declaration id, instance id) pairs closed —
    /// the shell raises `ScreenClosed`. With none matching, a transition in progress is left alone.
    /// Where the instance leaving through a transition matches, only the transition is finished (it
    /// is already closed, so it is not in the list).
    ///
    /// **A transition in progress is cut short only when the foreground (the active Pane's task,
    /// mid-transition) is what closes.** A background task's `evict_after` expiring, or another
    /// task's tidying up, must not snap the foreground's A2 / A3 to its end — only a user command
    /// interrupts a transition, and memory policy stays out of sight.
    pub fn close_where(
        &mut self,
        mut pred: impl FnMut(&Instance) -> bool,
        tokens: &MotionTokens,
    ) -> Vec<(String, InstanceId)> {
        // Of the active task only the top two are drawn — the one on show and the one a push or
        // a pop slides it over. An instance stopped deeper down goes without touching the motion.
        let active_index = self.pane_task_index();
        let in_active = active_index
            .and_then(|i| self.tasks.get(i))
            .into_iter()
            .flat_map(|task| task.iter().skip(task.len().saturating_sub(2)))
            .any(&mut pred);
        let in_stacks = in_active || self.tasks.iter().flat_map(Task::iter).any(&mut pred);
        let in_transit = self.closing_task.iter().flat_map(Task::iter).any(&mut pred)
            || self.leaving_task.iter().flat_map(Task::iter).any(&mut pred)
            || matches!(&self.stack, StackTransition::Popping { outgoing: Some(out), .. } if pred(out));
        if !in_stacks && !in_transit {
            return Vec::new();
        }
        if in_active || in_transit {
            self.settle();
        }
        let mut closed = Vec::new();
        if !in_stacks {
            return closed;
        }
        // Every task on show — the focused pane's, and the other pane's while split.
        let shown: Vec<usize> = if self.view == WorkspaceView::Tasks {
            self.panes.iter().filter_map(|p| p.task).collect()
        } else {
            Vec::new()
        };
        let mut changed_shown: Vec<usize> = Vec::new();
        let mut buried = Vec::new();
        for (index, task) in self.tasks.iter_mut().enumerate() {
            if !task.iter().any(&mut pred) {
                // A task with no match is left alone (`last_active` too).
                continue;
            }
            let was_top = task.top().map(Instance::id);
            let mut all = Vec::new();
            while let Some(instance) = task.pop() {
                all.push(instance);
            }
            all.reverse();
            for mut instance in all {
                if pred(&instance) {
                    closed.push((instance.decl_id().to_owned(), instance.id()));
                    if shown.contains(&index) && was_top == Some(instance.id()) {
                        instance.queue(Lifecycle::Paused);
                    }
                    buried.push(instance);
                } else {
                    task.push(instance);
                }
            }
            // Only a task on show whose top changed has a screen newly on show.
            if shown.contains(&index) && task.top().map(Instance::id) != was_top {
                changed_shown.push(index);
            }
        }
        for instance in buried {
            self.bury(instance);
        }
        // The new top gets a `Resumed`. **Only where something really came out of a task on show** — a
        // background task being tidied up must not make a lifecycle event on the foreground screen (and
        // mid-transition it would throw the order out as well).
        let on_top = self.on_show();
        for index in changed_shown {
            if let Some(top) = self.tasks.get_mut(index).and_then(Task::top_mut) {
                top.queue(on_top);
            }
        }
        self.prune_empty_tasks(tokens);
        closed
    }
    /// Frame stage 5: end the `Stopped` spawned instances whose `evict_after` has passed
    /// (`Instance::evict_due`). `now` is the shell's time.
    pub(crate) fn evict_expired(
        &mut self,
        now: Instant,
        tokens: &MotionTokens,
    ) -> Vec<(String, InstanceId)> {
        self.close_where(|instance| instance.evict_due(now), tokens)
    }
    /// The earliest moment [`Workspace::evict_expired`] has something to end, so that an idle
    /// panel wakes for it.
    #[must_use]
    pub(crate) fn next_eviction(&self) -> Option<Instant> {
        self.tasks
            .iter()
            .flat_map(Task::iter)
            .filter_map(Instance::evict_at)
            .min()
    }
    pub(super) fn prune_empty_tasks(&mut self, _tokens: &MotionTokens) {
        let mut index = 0;
        while index < self.tasks.len() {
            if self.tasks.get(index).is_some_and(Task::is_empty) {
                self.tasks.remove(index);
                self.shift_panes_after_remove(index);
            } else {
                index += 1;
            }
        }
        // A pane whose task has gone takes the split with it: the other pane fills the content.
        self.collapse_to_live_pane();
        if self.view == WorkspaceView::Tasks && self.active_task().is_none() {
            // The active task has gone entirely — home, with no animation.
            self.view = WorkspaceView::Home;
            self.origin = None;
            self.finish_home(true);
        }
    }
    pub(super) fn bury(&mut self, mut instance: Instance) {
        instance.queue(Lifecycle::Destroyed);
        self.graveyard.push(instance);
    }
    pub(super) fn bury_task(&mut self, mut task: Task) {
        while let Some(instance) = task.pop() {
            self.bury(instance);
        }
    }
    /// Handle the end of a home transition: on an open, `Resumed` on the top; on a close, `Stopped`
    /// throughout the task (or `Destroyed` for a task ended by a root pop).
    pub(super) fn finish_home(&mut self, closing: bool) {
        self.layers_dirty = true;
        let index = self.pane_task_index();
        if closing {
            if let Some(task) = index.and_then(|i| self.tasks.get_mut(i)) {
                for instance in task.iter_mut() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
            // Split, the other pane's task went home too: stopped, and one pane again.
            if self.split.is_some() {
                let other = 1 - self.focus.min(1);
                if let Some(task) = self
                    .panes
                    .get(other)
                    .and_then(|p| p.task)
                    .and_then(|i| self.tasks.get_mut(i))
                {
                    for instance in task.iter_mut() {
                        if !instance.is_stopped() {
                            instance.queue(Lifecycle::Stopped);
                        }
                    }
                }
                if let Some(task) = self.leaving_task.take() {
                    self.bury_task(task);
                }
                self.panes.truncate(1);
                if let Some(pane) = self.panes.first_mut() {
                    pane.id = PaneId(0);
                }
                self.split = None;
                self.focus = 0;
            }
            self.set_pane_task(None);
            if let Some(task) = self.closing_task.take() {
                self.bury_task(task);
            }
        } else if let Some(top) = index
            .and_then(|i| self.tasks.get_mut(i))
            .and_then(Task::top_mut)
        {
            top.queue(Lifecycle::Resumed);
        }
    }
    /// Handle the end of a stack transition: `Resumed` on the top, `Stopped` below it.
    pub(super) fn finish_stack(&mut self) {
        self.layers_dirty = true;
        let on_top = self.on_show();
        if let Some(task) = self.active_task_mut() {
            let len = task.len();
            for (i, instance) in task.iter_mut().enumerate() {
                if i + 1 == len {
                    instance.queue(on_top);
                } else if !instance.is_stopped() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
        }
    }
    /// Advance the frame: move the transition values by `dt` and wrap up any that finished.
    pub fn tick(&mut self, dt: f32) {
        let closing = matches!(self.home, HomeTransition::Closing { .. });
        if self.home.tick(dt) {
            self.finish_home(closing);
        }
        if self.lift.as_mut().is_some_and(|lift| lift.tick(dt)) {
            self.finish_lift();
        }
        if self.switch.as_mut().is_some_and(|switch| switch.tick(dt)) {
            self.finish_switch();
        }
        if let Some(done) = self.stack.tick(dt) {
            if let Some(out) = done {
                self.bury(out);
            }
            self.finish_stack();
        }
        if self.is_clearing() && !self.clear_fade.tick(dt) {
            self.finish_clearing();
            self.finish_stack();
        }
        self.tick_split(dt);
        if let Some(ticked) = self.overview.as_mut().map(|o| o.tick(dt)) {
            if ticked.returned {
                self.finish_overview_return();
            }
            if let Some(key) = ticked.thrown {
                let closed = self.close_card(key);
                self.overview_closed.extend(closed);
            }
        }
    }
    /// Advance the split (A8): an enter finished resumes the pane that came in, a leave finished
    /// takes its pane out, and a split come to rest after a change sizes its panes once.
    pub(super) fn tick_split(&mut self, dt: f32) {
        let Some(split) = self.split.as_mut() else {
            return;
        };
        let phase = split.phase();
        let left = split.tick(dt);
        let entered = matches!(phase, Phase::Entering(_)) && split.phase() == Phase::Split;
        let resize = split.take_resize();
        if let Some(pane) = left {
            self.finish_leaving(pane);
            return;
        }
        if entered {
            self.finish_entering();
        }
        if resize {
            self.queue_split_resize();
        }
    }
    /// Frame stage 14: lifecycle propagation. It drains every instance's queue (the one leaving
    /// through a transition included) into `on_lifecycle` and drops any instance that received a
    /// `Destroyed`. Each instance's `Cx` sees the rect of the pane it is in.
    pub(crate) fn flush_lifecycle(
        &mut self,
        parts: &mut CxParts<'_>,
        registry: &mut crate::screen::Registry,
    ) {
        let rect = self.last_content;
        let inset = self.inset_bottom;
        let split = self.split.is_some();
        // Where each task's screens are: the pane showing it, or the whole content.
        let mut placed: Vec<(usize, Rect)> = Vec::new();
        if split {
            for (pane, p) in self.panes.iter().enumerate() {
                if let (Some(task), Some(laid)) = (p.task, self.pane_laid.get(pane)) {
                    if laid.is_positive() {
                        placed.push((task, *laid));
                    }
                }
            }
        }
        let metrics = parts.theme.metrics;
        for (index, task) in self.tasks.iter_mut().enumerate() {
            let outer = placed
                .iter()
                .find(|(t, _)| *t == index)
                .map_or(rect, |(_, r)| *r);
            let shown = placed.iter().any(|(t, _)| *t == index);
            for instance in task.iter_mut() {
                let inner = screen_rect(outer, &instance.chrome(), &metrics);
                let pane = pane_info(outer, inner, instance.id(), false, inset, split && shown);
                let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
                instance.flush_lifecycle(&mut cx);
            }
        }
        let in_transit = self
            .closing_task
            .iter_mut()
            .chain(self.leaving_task.iter_mut())
            .flat_map(Task::iter_mut)
            .chain(self.clearing.iter_mut());
        for instance in in_transit {
            let inner = screen_rect(rect, &instance.chrome(), &metrics);
            let pane = pane_info(rect, inner, instance.id(), false, inset, false);
            let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
            instance.flush_lifecycle(&mut cx);
        }
        if let StackTransition::Popping {
            outgoing: Some(outgoing),
            ..
        }
        | StackTransition::DraggingBack {
            outgoing: Some(outgoing),
            ..
        } = &mut self.stack
        {
            let inner = screen_rect(rect, &outgoing.chrome(), &metrics);
            let pane = pane_info(rect, inner, outgoing.id(), false, inset, false);
            let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
            outgoing.flush_lifecycle(&mut cx);
        }
        for mut instance in self.graveyard.drain(..) {
            let inner = screen_rect(rect, &instance.chrome(), &metrics);
            let pane = pane_info(rect, inner, instance.id(), false, inset, false);
            let mut cx = parts.cx_in(pane, None, Some(&mut *registry));
            instance.flush_lifecycle(&mut cx);
            parts
                .shell
                .waker()
                .context()
                .set_transform_layer(instance.layer_id(), TSTransform::IDENTITY);
        }
    }
}
