//! Overview flow for `Workspace`.

use super::{
    root_key, Card, DrawnCard, Instance, InstanceId, Lifecycle, MotionTokens, Overview,
    OverviewMode, Picked, Rect, Task, Workspace, WorkspaceView,
};

impl Workspace {
    /// Whether the overview is up.
    #[must_use]
    pub fn is_overview_open(&self) -> bool {
        self.overview.is_some()
    }
    /// The session changed under the overview: its cards get split buttons where a split may
    /// come up now, and lose them where it may not.
    pub(crate) fn set_overview_beside(&mut self, beside: bool) {
        let over_tasks = self.view == WorkspaceView::Tasks;
        if let Some(overview) = self.overview.as_mut() {
            overview.set_beside(beside && over_tasks);
        }
    }
    /// Whether the overview is up as the split control's picker — a card goes beside the pane.
    #[must_use]
    pub fn is_overview_picking(&self) -> bool {
        self.overview.as_ref().is_some_and(Overview::is_picker)
    }
    /// Where the overview's cards were drawn last frame — for a tour or a test pressing them.
    #[doc(hidden)]
    #[must_use]
    pub fn overview_cards_drawn(&self) -> &[DrawnCard] {
        self.overview.as_ref().map_or(&[], Overview::drawn)
    }
    /// Where the overview's "Close all" was drawn last frame.
    #[doc(hidden)]
    #[must_use]
    pub fn overview_close_all_rect(&self) -> Option<Rect> {
        self.overview.as_ref().and_then(Overview::close_all_rect)
    }
    /// **Bring the overview up** (A10): over the task on show, whose screen shrinks into
    /// its card, or over the desktop. `mode` says what the cards are for — the recent screens
    /// (with split buttons where a split may come up) or the split control's picker. The screens
    /// on show are `Paused` while it covers them.
    pub(crate) fn open_overview(&mut self, mode: OverviewMode, tokens: &MotionTokens) {
        if self.overview.is_some() {
            return;
        }
        self.settle();
        let over_tasks = self.view == WorkspaceView::Tasks;
        let current = over_tasks
            .then(|| self.active_task().and_then(root_key))
            .flatten();
        if over_tasks {
            self.queue_focused(Lifecycle::Paused);
        }
        // Over the desktop there is no pane on show for a card to go beside.
        let mode = match mode {
            OverviewMode::Recents { beside } => OverviewMode::Recents {
                beside: beside && over_tasks,
            },
            OverviewMode::Picker => OverviewMode::Picker,
        };
        self.overview = Some(Overview::open(current, mode, !over_tasks, tokens));
        self.layers_dirty = true;
    }
    /// The overview goes back to what was on show (back, a tap past the cards).
    pub(crate) fn close_overview(&mut self, tokens: &MotionTokens) {
        let done = self
            .overview
            .as_mut()
            .is_some_and(|o| (tokens.reduce || !o.is_returning()) && o.go_back(tokens));
        if done {
            self.finish_overview_return();
        }
    }
    /// The overview is gone, back to what it covered: those screens are `Resumed`. A card thrown
    /// away and still flying ends its task all the same.
    pub(super) fn finish_overview_return(&mut self) {
        let thrown = self.overview.as_mut().and_then(Overview::take_thrown);
        self.overview = None;
        self.layers_dirty = true;
        if let Some(key) = thrown {
            let closed = self.close_card(key);
            self.overview_closed.extend(closed);
        }
        if self.view == WorkspaceView::Tasks {
            self.queue_focused(Lifecycle::Resumed);
        }
    }
    /// The overview gone at once — something else takes over the motion.
    pub(super) fn drop_overview(&mut self) {
        // A card thrown away and still flying ends its task all the same.
        let thrown = self.overview.as_mut().and_then(Overview::take_thrown);
        self.overview = None;
        self.layers_dirty = true;
        if let Some(key) = thrown {
            let closed = self.close_card(key);
            self.overview_closed.extend(closed);
        }
    }
    /// The task whose root instance is `key`.
    pub(super) fn task_by_key(&self, key: InstanceId) -> Option<usize> {
        self.tasks
            .iter()
            .position(|task| root_key(task) == Some(key))
    }
    /// The cards, in the order the overview shows them: the task on show first, then the rest by
    /// when they were last used. The picker shows only the tasks that can go beside the focused
    /// screen.
    pub(super) fn overview_cards(
        &self,
        access: &crate::access::Access,
        registry: &crate::screen::Registry,
    ) -> Vec<Card> {
        let Some(overview) = self.overview.as_ref() else {
            return Vec::new();
        };
        let shown: Vec<usize> = if self.view == WorkspaceView::Tasks {
            self.panes.iter().filter_map(|p| p.task).collect()
        } else {
            Vec::new()
        };
        let table = access.table();
        let bottom = table.get(table.bottom()).map(|d| d.label.clone());
        let mut order: Vec<usize> = (0..self.tasks.len()).collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.tasks.get(i).map(Task::last_active)));
        if let Some(current) = overview.current().and_then(|key| self.task_by_key(key)) {
            order.retain(|&i| i != current);
            order.insert(0, current);
        }
        if overview.is_picker() {
            // The picker offers only what can really go beside the focused screen.
            let fits = self.beside_candidates();
            order.retain(|i| fits.contains(i));
        }
        let can_beside = overview.has_beside() && self.view == WorkspaceView::Tasks;
        order
            .into_iter()
            .filter_map(|index| {
                let task = self.tasks.get(index)?;
                let root = task.iter().next()?;
                let top = task.top()?;
                let gate = registry
                    .screen(top.decl_id())
                    .map(crate::screen::ScreenDecl::gate_name);
                let badge = gate
                    .and_then(|gate| access.hint(&gate))
                    .filter(|label| Some(label) != bottom.as_ref());
                let beside =
                    can_beside && !shown.contains(&index) && self.can_split(top.split_support());
                Some(Card {
                    key: root.id(),
                    title: root.title().to_owned(),
                    icon: root.icon().cloned(),
                    last_active: task.last_active(),
                    badge,
                    beside,
                })
            })
            .collect()
    }
    /// End the task behind card `key`: every instance closed, the carousel closing the gap.
    /// Returns what closed, for `ScreenClosed`.
    pub(super) fn close_card(&mut self, key: InstanceId) -> Vec<(String, InstanceId)> {
        let Some(index) = self.task_by_key(key) else {
            return Vec::new();
        };
        let ids: Vec<InstanceId> = self
            .tasks
            .get(index)
            .map(|task| task.iter().map(Instance::id).collect())
            .unwrap_or_default();
        let tokens = self.tokens;
        let closed = self.close_where(|instance| ids.contains(&instance.id()), &tokens);
        let left = self.tasks.len();
        if let Some(overview) = self.overview.as_mut() {
            overview.forget_current(key);
            overview.closed(index, left);
        }
        closed
    }
    /// Act on what the overview was asked. Returns what closed.
    pub(super) fn apply_overview(&mut self, picked: Picked) -> Vec<(String, InstanceId)> {
        let tokens = self.tokens;
        match picked {
            Picked::Dismiss => {
                self.close_overview(&tokens);
                Vec::new()
            }
            Picked::Close(key) => self.close_card(key),
            Picked::CloseAll => {
                let tokens = self.tokens;
                let closed = self.close_where(|_| true, &tokens);
                self.drop_overview();
                closed
            }
            Picked::Beside(key) => {
                // The cards go only once the task is really beside; where it no longer fits (the
                // content changed under the cards) they stay up rather than vanish on a tap.
                if let Some(index) = self.task_by_key(key) {
                    if self.show_in_other_pane(index, &tokens) {
                        self.drop_overview();
                        // Everything the cards covered is on show again.
                        self.queue_focused(Lifecycle::Resumed);
                    } else {
                        log::info!("overview: that screen does not fit beside this one here");
                    }
                }
                Vec::new()
            }
            Picked::Resume(key) => {
                self.resume_card(key, &tokens);
                Vec::new()
            }
        }
    }
    /// A card tapped: a task on show goes back to its pane (the overview's way back); any other
    /// comes forward — over a task, through the A2 fallback as a screen opened with no icon does
    /// (A10); in a split, into the focused pane.
    pub(super) fn resume_card(&mut self, key: InstanceId, tokens: &MotionTokens) {
        let Some(index) = self.task_by_key(key) else {
            return;
        };
        let on_show = (self.view == WorkspaceView::Tasks)
            .then(|| self.panes.iter().position(|p| p.task == Some(index)))
            .flatten();
        if let Some(pane) = on_show {
            if pane != self.focus {
                self.focus = pane.min(self.panes.len().saturating_sub(1));
            }
            self.close_overview(tokens);
            return;
        }
        self.drop_overview();
        if self.view == WorkspaceView::Tasks && self.split.is_some() {
            // The other pane's screen was covered too; it is on show again.
            let other = 1 - self.focus.min(1);
            if let Some(top) = self.task_of_pane_mut(other).and_then(Task::top_mut) {
                top.queue(Lifecycle::Resumed);
            }
            self.resume_task(index, None, tokens);
            return;
        }
        if self.view == WorkspaceView::Tasks {
            // Home first, at once — then the A2 fallback opens the card's task.
            self.view = WorkspaceView::Home;
            self.finish_home(true);
        }
        self.resume_task(index, None, tokens);
    }
}
