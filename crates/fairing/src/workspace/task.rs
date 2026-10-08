//! A task — one pane's instance stack.
//!
//! At the bottom is the root screen (the one an icon opened) and above it whatever was opened
//! from there. Back pops. A task's name and icon are the root screen's.

use super::{Instance, InstanceId};
use std::time::Instant;

/// An instance stack.
#[derive(Debug)]
pub struct Task {
    stack: Vec<Instance>,
    last_active: Instant,
}

impl Task {
    /// Start with a root instance.
    #[must_use]
    pub fn new(root: Instance) -> Self {
        Self {
            stack: vec![root],
            last_active: Instant::now(),
        }
    }

    /// The root declaration id (the task's identity).
    #[doc(hidden)]
    #[must_use]
    pub fn root_id(&self) -> Option<&str> {
        self.stack.first().map(Instance::decl_id)
    }

    /// The top (what is visible).
    #[must_use]
    pub fn top(&self) -> Option<&Instance> {
        self.stack.last()
    }

    /// The top (mutable).
    pub(crate) fn top_mut(&mut self) -> Option<&mut Instance> {
        self.stack.last_mut()
    }

    /// push.
    pub fn push(&mut self, instance: Instance) {
        self.stack.push(instance);
        self.last_active = Instant::now();
    }

    /// Pop. `None` when empty.
    pub fn pop(&mut self) -> Option<Instance> {
        self.last_active = Instant::now();
        self.stack.pop()
    }

    /// The depth.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Whether it is empty (that is, the task has ended).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Find by declaration id. Where the same declaration is stacked several times
    /// (`LaunchMode::Multi`), it gives **the bottom-most** (opened first) — the same basis as
    /// `clear_top`.
    #[must_use]
    pub fn find(&self, decl_id: &str) -> Option<&Instance> {
        self.stack.iter().find(|i| i.decl_id() == decl_id)
    }

    /// Find by instance id.
    #[must_use]
    pub fn instance(&self, id: InstanceId) -> Option<&Instance> {
        self.stack.iter().find(|i| i.id() == id)
    }

    /// Find by instance id (mutable).
    pub(crate) fn instance_mut(&mut self, id: InstanceId) -> Option<&mut Instance> {
        self.stack.iter_mut().find(|i| i.id() == id)
    }

    /// Pop everything above a declaration id's instance (clear-top, Single-1).
    ///
    /// - The basis is **the bottom-most** (first opened) instance. In `[a, b, a]`,
    ///   `clear_top("a")` takes the upper `b, a` off and leaves `[a]` — Single means "go back to
    ///   the one that is alive".
    /// - It returns the popped instances, **top first**. Notifying `Destroyed` in that order
    ///   cleans up from the one nearest the screen.
    /// - If that declaration is not in the stack, it does nothing and returns an empty `Vec`.
    /// - Already at the top, an empty `Vec` (the stack is unchanged).
    pub fn clear_top(&mut self, decl_id: &str) -> Vec<Instance> {
        let Some(pos) = self.stack.iter().position(|i| i.decl_id() == decl_id) else {
            return Vec::new();
        };
        let mut popped: Vec<Instance> = self.stack.drain(pos + 1..).collect();
        popped.reverse();
        popped
    }

    /// Remove every instance of a declaration id, wherever it is in the stack (`shell.remove(id)`
    /// or a declaration being replaced). The order returned is **top first**, as in `clear_top`.
    /// The relative order of the remaining instances is unchanged.
    #[cfg(test)]
    pub(crate) fn remove_decl(&mut self, decl_id: &str) -> Vec<Instance> {
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for instance in self.stack.drain(..) {
            if instance.decl_id() == decl_id {
                removed.push(instance);
            } else {
                kept.push(instance);
            }
        }
        self.stack = kept;
        removed.reverse();
        removed
    }

    /// Iterate the stack (bottom to top).
    pub fn iter(&self) -> impl Iterator<Item = &Instance> {
        self.stack.iter()
    }

    /// Iterate the stack (mutable).
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Instance> {
        self.stack.iter_mut()
    }

    /// The last active time (for ordering in Overview, and the split control's "the task used
    /// last").
    #[must_use]
    pub fn last_active(&self) -> Instant {
        self.last_active
    }

    /// Refresh the active time.
    pub fn touch(&mut self) {
        self.last_active = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::{Instance, InstanceId, Task};
    use crate::screen::{screen_with, Cx, Screen, ScreenDecl};
    use crate::workspace::InstanceScreen;

    /// A screen that draws nothing.
    struct Blank;

    impl Screen for Blank {
        fn ui(&mut self, _ui: &mut egui::Ui, _cx: &mut Cx<'_>) {}
    }

    fn decl(id: &str) -> ScreenDecl {
        screen_with(id, || Blank)
    }

    fn instance(decl: &ScreenDecl, id: u64) -> Instance {
        Instance::new(InstanceId(id), decl, InstanceScreen::Owned(Box::new(Blank)))
    }

    /// Build a stack from `[(declaration id, instance id)]` (bottom to top).
    fn task(entries: &[(&str, u64)]) -> Task {
        let mut iter = entries.iter();
        let Some((root_id, root_instance)) = iter.next() else {
            return Task::new(instance(&decl("root"), 0));
        };
        let mut task = Task::new(instance(&decl(root_id), *root_instance));
        for (id, n) in iter {
            task.push(instance(&decl(id), *n));
        }
        task
    }

    fn ids(instances: &[Instance]) -> Vec<u64> {
        instances.iter().map(|i| i.id().0).collect()
    }

    fn stack_ids(task: &Task) -> Vec<u64> {
        task.iter().map(|i| i.id().0).collect()
    }

    /// Single-instance clear-top: everything above the basis instance comes off, returned top first.
    #[test]
    fn clear_top_pops_everything_above_and_returns_top_first() {
        let mut t = task(&[("a", 1), ("b", 2), ("c", 3)]);
        let popped = t.clear_top("a");
        assert_eq!(ids(&popped), vec![3, 2], "from the top down");
        assert_eq!(stack_ids(&t), vec![1]);
        assert_eq!(t.len(), 1);
    }

    /// With the same declaration twice, **the bottom-most** (the first) is the basis.
    #[test]
    fn clear_top_uses_the_bottom_most_match() {
        let mut t = task(&[("a", 1), ("b", 2), ("a", 3), ("c", 4)]);
        let popped = t.clear_top("a");
        assert_eq!(ids(&popped), vec![4, 3, 2]);
        assert_eq!(stack_ids(&t), vec![1]);
        assert_eq!(t.find("a").map(|i| i.id().0), Some(1));
    }

    /// A missing declaration, or one already on top, does nothing.
    #[test]
    fn clear_top_is_a_no_op_when_nothing_is_above() {
        let mut t = task(&[("a", 1), ("b", 2)]);
        assert!(t.clear_top("nope").is_empty());
        assert_eq!(stack_ids(&t), vec![1, 2]);
        assert!(t.clear_top("b").is_empty(), "already on top");
        assert_eq!(stack_ids(&t), vec![1, 2]);
    }

    /// `remove_decl` takes every match wherever it is, and keeps the order of what is left.
    #[test]
    fn remove_decl_takes_every_match_top_first() {
        let mut t = task(&[("a", 1), ("b", 2), ("a", 3), ("c", 4)]);
        let removed = t.remove_decl("a");
        assert_eq!(ids(&removed), vec![3, 1], "from the top down");
        assert_eq!(
            stack_ids(&t),
            vec![2, 4],
            "the order of what is left is unchanged"
        );
        assert!(t.remove_decl("nope").is_empty());
        assert_eq!(stack_ids(&t), vec![2, 4]);
    }

    /// Removing the root as well empties the task (that is, the task ends).
    #[test]
    fn remove_decl_can_empty_the_task() {
        let mut t = task(&[("a", 1), ("a", 2)]);
        let removed = t.remove_decl("a");
        assert_eq!(ids(&removed), vec![2, 1]);
        assert!(t.is_empty());
        assert_eq!(t.root_id(), None);
        assert!(t.top().is_none());
    }

    /// push/pop and lookups.
    #[test]
    fn push_pop_and_lookup() {
        let mut t = task(&[("a", 1)]);
        assert_eq!(t.root_id(), Some("a"));
        assert_eq!(t.top().map(|i| i.id().0), Some(1));
        t.push(instance(&decl("b"), 2));
        assert_eq!(t.top().map(|i| i.id().0), Some(2));
        assert_eq!(t.root_id(), Some("a"), "the root does not change");
        assert_eq!(t.len(), 2);
        assert_eq!(t.instance(InstanceId(1)).map(|i| i.id().0), Some(1));
        assert_eq!(t.instance_mut(InstanceId(2)).map(|i| i.id().0), Some(2));
        assert!(t.instance(InstanceId(99)).is_none());
        assert_eq!(t.pop().map(|i| i.id().0), Some(2));
        assert_eq!(t.pop().map(|i| i.id().0), Some(1));
        assert!(t.pop().is_none());
        assert!(t.is_empty());
    }

    /// `touch` moves the active time forward (for ordering in Overview).
    #[test]
    fn touch_moves_last_active_forward() {
        let mut t = task(&[("a", 1)]);
        let before = t.last_active();
        t.touch();
        assert!(t.last_active() >= before);
        let after_touch = t.last_active();
        t.push(instance(&decl("b"), 2));
        assert!(
            t.last_active() >= after_touch,
            "a push moves the active time too"
        );
    }
}
