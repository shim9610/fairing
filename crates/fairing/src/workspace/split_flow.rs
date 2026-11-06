//! Split flow for `Workspace`.

use super::{
    pane_of, split, HeldBack, Instance, InstanceId, Lifecycle, MotionTokens, Pane, PaneId, Phase,
    Pos2, Rect, Split, SplitAxis, SplitBlocker, SplitSupport, StackTransition, Task, Workspace,
    WorkspaceView,
};

impl Workspace {
    /// The task pane `pane` shows (the leaving one included, while its pane slides out).
    pub(super) fn task_of_pane(&self, pane: usize) -> Option<&Task> {
        if self.leaving_pane() == Some(pane) {
            if let Some(task) = self.leaving_task.as_ref() {
                return Some(task);
            }
        }
        self.panes
            .get(pane)
            .and_then(|p| p.task)
            .and_then(|i| self.tasks.get(i))
    }
    /// [`Workspace::task_of_pane`], mutable.
    pub(super) fn task_of_pane_mut(&mut self, pane: usize) -> Option<&mut Task> {
        if self.leaving_pane() == Some(pane) && self.leaving_task.is_some() {
            return self.leaving_task.as_mut();
        }
        let index = self.panes.get(pane).and_then(|p| p.task)?;
        self.tasks.get_mut(index)
    }
    /// The pane on its way out, while one is.
    pub(super) fn leaving_pane(&self) -> Option<usize> {
        match self.split.as_ref().map(Split::phase) {
            Some(Phase::Leaving(pane)) => Some(pane),
            _ => None,
        }
    }
    /// Whether two panes are up — or one of them is on its way in or out.
    #[must_use]
    pub fn is_split(&self) -> bool {
        self.split.is_some()
    }
    /// The focused pane, as an index: 0 is the first pane (left, or top), 1 the second.
    #[must_use]
    pub fn focused_pane(&self) -> usize {
        self.focus
    }
    /// The task pane `pane` shows.
    #[must_use]
    pub fn pane_task(&self, pane: usize) -> Option<&Task> {
        self.task_of_pane(pane)
    }
    /// Where pane `pane` was last drawn, before the screen inset.
    #[must_use]
    pub fn pane_rect(&self, pane: usize) -> Option<Rect> {
        let rect = if self.split.is_some() {
            *self.pane_rects.get(pane)?
        } else if pane == 0 {
            self.last_content
        } else {
            return None;
        };
        rect.is_positive().then_some(rect)
    }
    /// Current pane geometry for input placement before the workspace is drawn.
    pub(crate) fn focused_pane_in(&self, content: Rect) -> Rect {
        pane_of(self.pane_frame(content).laid, self.focus)
    }
    /// Where the focused pane's screen was last laid out — the content with one pane.
    pub(crate) fn focused_pane_rect(&self) -> Rect {
        if self.split.is_some() {
            if let Some(rect) = self.pane_laid.get(self.focus).filter(|r| r.is_positive()) {
                return *rect;
            }
        }
        self.last_content
    }
    /// The divider's place along the axis, as a share of the content, while split.
    #[must_use]
    pub fn split_ratio(&self) -> Option<f32> {
        self.split.as_ref().map(Split::ratio)
    }
    /// Where the divider's handle was last drawn: the band, a touch target across.
    #[must_use]
    pub fn divider_rect(&self) -> Option<Rect> {
        self.divider_handle
            .is_positive()
            .then_some(self.divider_handle)
    }
    /// The axis the panes divide along, for this content.
    #[must_use]
    pub fn split_axis(&self) -> SplitAxis {
        self.axis_for(self.last_content)
    }
    pub(super) fn axis_for(&self, content: Rect) -> SplitAxis {
        self.axis_rule
            .unwrap_or_else(|| SplitAxis::for_content(content))
    }
    /// `[workspace] split_axis` — `None` follows the content's shape.
    pub(crate) fn set_split_axis(&mut self, rule: Option<SplitAxis>) {
        self.axis_rule = rule;
    }
    /// The least length along the axis each pane keeps in `content` — from its top screen's
    /// `SplitSupport`. A screen that is never split (it can only be here by being pushed after the
    /// split began) gets the default.
    pub(super) fn pane_mins(&self, content: Rect, axis: SplitAxis) -> [f32; 2] {
        let target = self.split_metrics.1;
        let min = |pane: usize| {
            let support = self
                .task_of_pane(pane)
                .and_then(Task::top)
                .map_or(SplitSupport::Yes, Instance::split_support);
            split::min_len(support, content, axis, target)
                .or_else(|| split::min_len(SplitSupport::Yes, content, axis, target))
                .unwrap_or(0.0)
        };
        [min(0), min(1)]
    }
    /// Move the focus to pane `pane` — a press on it. A transition in progress lands
    /// first; nothing else changes: both panes stay `Resumed`, only `PaneInfo::is_focused` and
    /// whose chrome policy rules move.
    pub(crate) fn focus_pane(&mut self, pane: usize) {
        if pane == self.focus || pane >= self.panes.len() {
            return;
        }
        if !matches!(self.split.as_ref().map(Split::phase), Some(Phase::Split)) {
            return;
        }
        self.settle();
        self.focus = pane;
    }
    /// Take a back gesture the finger is still dragging out of the way, while the other pane acts
    /// on its own: focusing that pane for a moment settles every transition, and the user's drag
    /// must not be one of them. `None` where no drag is under way.
    pub(crate) fn hold_back_gesture(&mut self) -> Option<HeldBack> {
        if !matches!(
            self.stack,
            StackTransition::DraggingBack {
                confirmed: None,
                ..
            }
        ) {
            return None;
        }
        Some(HeldBack {
            stack: std::mem::replace(&mut self.stack, StackTransition::Idle),
            pane: self.focus,
            top: self.active_task().and_then(Task::top).map(Instance::id),
        })
    }
    /// Put back what [`Workspace::hold_back_gesture`] took: the finger carries on where it was.
    /// Where the pane under it changed in the meantime the drag is over, and the screen it was
    /// dragging, still on top, is told `Resumed`.
    pub(crate) fn resume_back_gesture(&mut self, held: HeldBack) {
        let top = self.active_task().and_then(Task::top).map(Instance::id);
        if self.focus == held.pane
            && top == held.top
            && matches!(self.stack, StackTransition::Idle)
            && self.view == WorkspaceView::Tasks
            && self.overview.is_none()
        {
            self.stack = held.stack;
            return;
        }
        if let Some(instance) = held.top.and_then(|id| self.instance_mut(id)) {
            instance.queue(Lifecycle::Resumed);
        }
    }
    /// Focus the pane showing `instance` — a screen asking for something from its own pane.
    pub(crate) fn focus_pane_of(&mut self, instance: InstanceId) {
        if self.split.is_none() {
            return;
        }
        let pane = (0..self.panes.len()).find(|&pane| {
            self.task_of_pane(pane)
                .is_some_and(|task| task.instance(instance).is_some())
        });
        if let Some(pane) = pane {
            self.focus_pane(pane);
        }
    }
    /// The pane showing instance `instance` — `None` where it is in neither (or with one pane, in
    /// a task not on show).
    pub(crate) fn pane_holding(&self, instance: InstanceId) -> Option<usize> {
        (0..self.panes.len()).find(|&pane| {
            self.task_of_pane(pane)
                .is_some_and(|task| task.instance(instance).is_some())
        })
    }
    /// A screen in the pane the user is not in opened something on its own — a timer, an answer
    /// arriving — and the push went to its pane, which took the focus to do it. The push finishes
    /// at once (stack transitions are drawn in the focused pane) and the focus goes back to
    /// `pane`, where the user is: their keyboard, bars and back button stay theirs (the
    /// focus is the pane of the last input).
    pub(crate) fn return_focus(&mut self, pane: usize) {
        if self.split.is_none() || pane >= self.panes.len() || pane == self.focus {
            return;
        }
        self.settle();
        self.focus_pane(pane);
    }
    /// Focus the pane under `at` — where the press landed on a pane's own screen. A press on
    /// anything drawn over the panes (the shade, the on-screen keyboard, the unlock prompt, the
    /// overview's cards, a toast) or on the divider's handle leaves the focus where it is: typing
    /// on the keyboard over the other pane must not hand that pane the focus.
    pub(crate) fn focus_at(&mut self, ctx: &egui::Context, at: Pos2) {
        if self.split.is_none() || self.overview.is_some() || self.divider_handle.contains(at) {
            return;
        }
        // The topmost layer that takes input there, as last frame drew it.
        let Some(layer) = ctx.layer_id_at(at) else {
            return;
        };
        let on_a_pane = (0..self.panes.len()).any(|pane| {
            self.task_of_pane(pane)
                .and_then(Task::top)
                .is_some_and(|top| top.layer_id() == layer)
        });
        if !on_a_pane {
            return;
        }
        if let Some(pane) = self.pane_rects.iter().position(|r| r.contains(at)) {
            self.focus_pane(pane);
        }
    }
    /// Whether a split can hold with a screen of `incoming` support in the other pane:
    /// in the Tasks view, the focused screen allowing it (`ChromePolicy::allow_split` and its own
    /// `SplitSupport`), and a ratio in this content at which both keep their minimums.
    #[must_use]
    pub fn can_split(&self, incoming: SplitSupport) -> bool {
        self.entering_ratio(incoming).is_some()
    }
    /// The ratio a split with `incoming` in the other pane would start at — an even split, or as
    /// near as both minimums allow. `None` where no split can hold.
    pub(super) fn entering_ratio(&self, incoming: SplitSupport) -> Option<f32> {
        if self.view != WorkspaceView::Tasks
            || matches!(
                self.split.as_ref().map(Split::phase),
                Some(Phase::Leaving(_))
            )
        {
            return None;
        }
        let top = self.focused()?;
        if !top.chrome().allow_split {
            return None;
        }
        let content = self.last_content;
        if !content.is_positive() {
            return None;
        }
        let axis = self.axis_for(content);
        let (divider, target) = self.split_metrics;
        let kept = split::min_len(top.split_support(), content, axis, target)?;
        let coming = split::min_len(incoming, content, axis, target)?;
        // The focused pane keeps its side; the other side is the incoming screen's.
        let mins = if self.split.is_some() && self.focus == 1 {
            [coming, kept]
        } else {
            [kept, coming]
        };
        let (lo, hi) = split::bounds(content, axis, divider, mins)?;
        let ratio = self.split.as_ref().map_or(0.5, Split::ratio);
        Some(ratio.clamp(lo, hi))
    }
    /// **Open `instance` in the other pane** (`cx.open_in_other_pane`): with one pane, the
    /// second pane of a new split — the pane already there keeps the first half and the new one
    /// slides in (A8); with two, a push onto the pane not focused. Focus moves to it. The caller
    /// asks [`Workspace::can_split`] first; where no split can hold this opens in the same pane.
    pub(crate) fn open_in_other_pane(&mut self, instance: Instance, tokens: &MotionTokens) {
        let Some(ratio) = self.entering_ratio(instance.split_support()) else {
            self.open(instance, None, tokens);
            return;
        };
        self.settle();
        if self.split.is_some() {
            self.focus = 1 - self.focus;
            self.open(instance, None, tokens);
            return;
        }
        self.tasks.push(Task::new(instance));
        let index = self.tasks.len() - 1;
        self.enter_split(index, ratio, tokens);
    }
    /// **Show task `task` in the other pane** — the Overview's "split" and a launch that found its
    /// screen in a task of its own. With two panes the other pane's task goes to the background
    /// (`Paused`, then `Stopped`) and this one comes forward (`Resumed`), at once. `false` where it
    /// is the focused task itself, or no split can hold.
    pub(crate) fn show_in_other_pane(&mut self, task: usize, tokens: &MotionTokens) -> bool {
        if task >= self.tasks.len() || self.pane_task_index() == Some(task) {
            return false;
        }
        let support = self
            .tasks
            .get(task)
            .and_then(Task::top)
            .map_or(SplitSupport::Yes, Instance::split_support);
        let Some(ratio) = self.entering_ratio(support) else {
            return false;
        };
        self.settle();
        if self.split.is_none() {
            self.enter_split(task, ratio, tokens);
            return true;
        }
        let other = 1 - self.focus;
        self.focus = other;
        if self.pane_task_index() == Some(task) {
            return true;
        }
        if let Some(old) = self.active_task_mut() {
            if let Some(top) = old.top_mut() {
                top.queue(Lifecycle::Paused);
            }
            for instance in old.iter_mut() {
                instance.queue(Lifecycle::Stopped);
            }
        }
        self.set_pane_task(Some(task));
        if let Some(shown) = self.tasks.get_mut(task) {
            shown.touch();
            if let Some(top) = shown.top_mut() {
                top.queue(Lifecycle::Resumed);
            }
        }
        self.layers_dirty = true;
        true
    }
    /// The tasks that can go beside the focused screen now, the one used last first: not on show
    /// in either pane, with a top screen whose `SplitSupport` fits next to the focused one.
    pub(super) fn beside_candidates(&self) -> Vec<usize> {
        let shown: Vec<usize> = if self.view == WorkspaceView::Tasks {
            self.panes.iter().filter_map(|p| p.task).collect()
        } else {
            Vec::new()
        };
        let mut fits: Vec<usize> = self
            .tasks
            .iter()
            .enumerate()
            .filter(|(index, _)| !shown.contains(index))
            .filter(|(_, task)| {
                task.top()
                    .is_some_and(|top| self.can_split(top.split_support()))
            })
            .map(|(index, _)| index)
            .collect();
        fits.sort_by_key(|&i| std::cmp::Reverse(self.tasks.get(i).map(Task::last_active)));
        fits
    }
    /// Why a split cannot come up beside the focused screen now — `None` where one can. The split
    /// control says this instead of doing nothing.
    pub(crate) fn split_blocker(&self) -> Option<SplitBlocker> {
        if self.view != WorkspaceView::Tasks || self.focused().is_none() {
            return Some(SplitBlocker::NothingOnShow);
        }
        // The smallest partner a screen can name: if even that does not fit, nothing will.
        let smallest = SplitSupport::MinSize(egui::Vec2::ZERO);
        if !self.can_split(smallest) {
            return Some(SplitBlocker::Refused);
        }
        if self.beside_candidates().is_empty() {
            return Some(SplitBlocker::NothingFits);
        }
        None
    }
    /// Something is being opened from outside the cards — a launch, a notification, a command from
    /// another thread: the overview gets out of the way at once, so what opens is on show rather
    /// than hidden under it, and the screens it covered are on show again.
    pub(crate) fn clear_overview(&mut self) {
        if self.overview.is_none() {
            return;
        }
        self.drop_overview();
        if self.view == WorkspaceView::Tasks {
            self.queue_focused(Lifecycle::Resumed);
        }
    }
    /// One pane becomes two: task `task` in the second pane, sliding in (A8). Its top is
    /// `Resumed` once it is in.
    pub(super) fn enter_split(&mut self, task: usize, ratio: f32, tokens: &MotionTokens) {
        if let Some(shown) = self.tasks.get_mut(task) {
            shown.touch();
        }
        self.panes.truncate(1);
        self.panes.push(Pane {
            id: PaneId(1),
            task: Some(task),
        });
        self.focus = 1;
        self.split = Some(Split::enter(1, ratio, tokens));
        self.layers_dirty = true;
        if tokens.reduce {
            self.finish_entering();
        }
    }
    /// The split is in: the pane that came in is `Resumed`.
    pub(super) fn finish_entering(&mut self) {
        if let Some(top) = self.task_of_pane_mut(1).and_then(Task::top_mut) {
            top.queue(Lifecycle::Resumed);
        }
    }
    /// **Back to one pane**, keeping pane `keep` (by the split control, or the divider pushed to
    /// an end). The other pane slides out (A8); its task stays alive in the background — `Paused`
    /// now, `Stopped` once it is out. `false` with one pane, or a pane already leaving.
    pub(crate) fn unsplit(&mut self, keep: usize, tokens: &MotionTokens) -> bool {
        if keep > 1
            || !matches!(
                self.split.as_ref().map(Split::phase),
                Some(Phase::Split | Phase::Entering(_))
            )
        {
            return false;
        }
        self.settle();
        let leaving = 1 - keep;
        if let Some(top) = self.task_of_pane_mut(leaving).and_then(Task::top_mut) {
            top.queue(Lifecycle::Paused);
        }
        self.start_leaving(leaving, tokens);
        true
    }
    /// Start pane `leaving` sliding out; the focus is the other pane's from now.
    pub(super) fn start_leaving(&mut self, leaving: usize, tokens: &MotionTokens) {
        self.focus = 1 - leaving;
        let done = self
            .split
            .as_mut()
            .is_some_and(|split| split.leave(leaving, tokens));
        self.layers_dirty = true;
        if done {
            self.finish_leaving(leaving);
        }
    }
    /// Pane `leaving` is out: one pane again. A task that ended with it (a root pop) is buried;
    /// one that stays alive is `Stopped`. The pane left fills the content, and says so.
    pub(super) fn finish_leaving(&mut self, leaving: usize) {
        let pane = (leaving < self.panes.len()).then(|| self.panes.remove(leaving));
        if let Some(task) = self.leaving_task.take() {
            self.bury_task(task);
        } else if let Some(task) = pane
            .and_then(|p| p.task)
            .and_then(|i| self.tasks.get_mut(i))
        {
            for instance in task.iter_mut() {
                if !instance.is_stopped() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
        }
        self.split = None;
        self.focus = 0;
        if let Some(pane) = self.panes.first_mut() {
            pane.id = PaneId(0);
        }
        self.layers_dirty = true;
        self.queue_pane_resize(0, self.last_content);
    }
    /// One pane again, at once — a pane's task is gone (a session downgrade, an eviction): the
    /// other pane fills the content with no animation, as `close_where` closes.
    pub(super) fn collapse_to_live_pane(&mut self) {
        if self.split.is_none() {
            return;
        }
        let live = self.panes.iter().position(|p| p.task.is_some());
        let both = self.panes.iter().all(|p| p.task.is_some());
        if both {
            return;
        }
        let keep = live.unwrap_or(0);
        let kept = self.panes.get(keep).copied();
        self.panes.clear();
        self.panes.push(Pane {
            id: PaneId(0),
            task: kept.and_then(|p| p.task),
        });
        if let Some(task) = self.leaving_task.take() {
            self.bury_task(task);
        }
        self.split = None;
        self.focus = 0;
        self.layers_dirty = true;
        self.queue_pane_resize(0, self.last_content);
    }
    /// Back to one pane at once and with no animation, keeping the focused pane — the Home view
    /// coming back, or a screen that is never split arriving in a pane.
    pub(super) fn collapse_to_focused(&mut self) {
        if self.split.is_none() {
            return;
        }
        let kept = self.panes.get(self.focus).copied();
        let other = 1 - self.focus.min(1);
        if let Some(task) = self.leaving_task.take() {
            self.bury_task(task);
        } else if let Some(task) = self
            .panes
            .get(other)
            .and_then(|p| p.task)
            .and_then(|i| self.tasks.get_mut(i))
        {
            if let Some(top) = task.top_mut() {
                top.queue(Lifecycle::Paused);
            }
            for instance in task.iter_mut() {
                if !instance.is_stopped() {
                    instance.queue(Lifecycle::Stopped);
                }
            }
        }
        self.panes.clear();
        self.panes.push(Pane {
            id: PaneId(0),
            task: kept.and_then(|p| p.task),
        });
        self.split = None;
        self.focus = 0;
        self.layers_dirty = true;
        self.queue_pane_resize(0, self.last_content);
    }
    /// Queue `Resized` on every instance of pane `pane`'s task, at `rect`'s size.
    pub(super) fn queue_pane_resize(&mut self, pane: usize, rect: Rect) {
        if !rect.is_positive() {
            return;
        }
        let size = rect.size();
        if let Some(task) = self.task_of_pane_mut(pane) {
            for instance in task.iter_mut() {
                instance.tell_size(size);
            }
        }
    }
    /// The split came to rest after a change: each pane's instances hear their new size, once
    /// (the end of a tween, the divider's release).
    pub(super) fn queue_split_resize(&mut self) {
        let content = self.last_content;
        let Some(split) = self.split.as_ref() else {
            return;
        };
        if !content.is_positive() {
            return;
        }
        let axis = self.axis_for(content);
        let [first, second, _] = split::rest(content, axis, split.ratio(), self.split_metrics.0);
        self.queue_pane_resize(0, first);
        self.queue_pane_resize(1, second);
    }
    /// Initial geometry for lifecycle work before the first UI pass. A later actual size
    /// is still reported by `content_resized`.
    pub(crate) fn seed_content(&mut self, content: Rect) {
        if !self.last_content.is_positive() {
            self.last_content = content;
        }
    }
    /// The content resized (a rotation, a window, the chrome hiding): every pane's instances hear
    /// their new size — the whole content with one pane, each pane's share with two.
    pub(crate) fn content_resized(&mut self, content: Rect) {
        self.last_content = content;
        if self.split.is_none() {
            for instance in self.tasks.iter_mut().flat_map(Task::iter_mut) {
                instance.tell_size(content.size());
            }
            return;
        }
        // The background tasks take the whole content; the two shown take their shares.
        let shown: Vec<usize> = self.panes.iter().filter_map(|p| p.task).collect();
        for (index, task) in self.tasks.iter_mut().enumerate() {
            if shown.contains(&index) {
                continue;
            }
            for instance in task.iter_mut() {
                instance.tell_size(content.size());
            }
        }
        self.queue_split_resize();
    }
    /// Every instance at rest in a pane of the size it was last told — or told now. The tweens
    /// tell their panes at their end, but a task also changes size without one: put into
    /// the other pane, resumed into a split, sent to the background when its pane leaves. Asked
    /// once a frame with nothing moving; an instance already told its size hears nothing.
    pub(super) fn sync_sizes(&mut self) {
        let content = self.last_content;
        if !content.is_positive() || self.is_animating() {
            return;
        }
        if self.split.as_ref().is_some_and(Split::is_dragging) {
            return;
        }
        let mut shown: [Option<(usize, Rect)>; 2] = [None; 2];
        if self.view == WorkspaceView::Tasks {
            match self.split.as_ref() {
                Some(split) if split.phase() == Phase::Split => {
                    let axis = self.axis_for(content);
                    let [first, second, _] =
                        split::rest(content, axis, split.ratio(), self.split_metrics.0);
                    for (slot, (pane, rect)) in shown.iter_mut().zip([(0, first), (1, second)]) {
                        *slot = self.panes.get(pane).and_then(|p| p.task).map(|t| (t, rect));
                    }
                }
                Some(_) => return,
                None => {
                    shown[0] = self.pane_task_index().map(|t| (t, content));
                }
            }
        }
        for (index, task) in self.tasks.iter_mut().enumerate() {
            let size = shown
                .iter()
                .flatten()
                .find(|(t, _)| *t == index)
                .map_or(content.size(), |(_, rect)| rect.size());
            for instance in task.iter_mut() {
                instance.keep_size(size);
            }
        }
    }
    /// Pull the Pane indices in after task `index` has gone.
    pub(super) fn shift_panes_after_remove(&mut self, index: usize) {
        for pane in &mut self.panes {
            pane.task = match pane.task {
                Some(i) if i == index => None,
                Some(i) if i > index => Some(i - 1),
                other => other,
            };
        }
    }
}
