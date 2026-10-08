//! The notification centre: an in-memory list, dedup, the unread count and
//! grouping by source.
//!
//! The list is **newest first**, and notifications with the same `source` stay together as one
//! group ([`NotificationCenter::groups`]). A new notification brings its whole source group to
//! the front — one machine emitting notifications back to back does not scatter them among
//! other sources.
//!
//! A gated notification is redacted **here** ("where it is enforced = the
//! notification centre"): when [`shows_content`] is `false`, the drawing side draws one line of
//! [`crate::overlay::panel::labels::HIDDEN_NOTIFICATION`] instead of the title and body.

use super::model::{Notification, NotificationId};
use crate::access::Access;

/// The result of [`NotificationCenter::push`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CenterChange {
    /// A new notification (a heads-up candidate).
    Added,
    /// The same id updated (no heads-up).
    Updated,
}

/// Whether the gate lets the **content be shown**. Always `true` with no gate.
///
/// Failing it, the list hides the title, body and progress and draws one redacted row. The
/// notification itself stays — it does not hide the fact that "there are some".
#[must_use]
pub fn shows_content(notification: &Notification, access: &Access) -> bool {
    notification
        .gate
        .as_ref()
        .is_none_or(|gate| access.allows(gate))
}

/// The notification centre.
#[derive(Debug, Default)]
pub struct NotificationCenter {
    items: Vec<Notification>,
    unread: usize,
    max_items: usize,
    /// Whether it has already reported that the cap cannot be met (everything is persistent) — the same warning does not go out every frame.
    warned_full: bool,
}

impl NotificationCenter {
    /// Keeps at most `max_items`. 0 is unlimited.
    #[must_use]
    pub fn new(max_items: usize) -> Self {
        Self {
            items: Vec::new(),
            unread: 0,
            max_items,
            warned_full: false,
        }
    }

    /// Insert. The same id updates **in place** ([`CenterChange::Updated`]), and a new one goes
    /// at the front of its source group (with the group moving to the front of the list as a
    /// whole). Over the cap, the oldest **non-persistent** ones are dropped first.
    pub fn push(&mut self, notification: Notification) -> CenterChange {
        if let Some(index) = self.items.iter().position(|n| n.id == notification.id) {
            let same_source = self
                .items
                .get(index)
                .is_some_and(|old| old.source == notification.source);
            if same_source {
                if let Some(slot) = self.items.get_mut(index) {
                    *slot = notification;
                }
            } else {
                // A change of source finds the group again and goes into it — a refresh, not a new notification.
                self.items.remove(index);
                self.insert_grouped(notification);
            }
            return CenterChange::Updated;
        }
        self.insert_grouped(notification);
        self.unread += 1;
        self.enforce_cap();
        CenterChange::Added
    }

    /// Dismiss. A persistent notification refuses (`false`).
    pub fn dismiss(&mut self, id: NotificationId) -> bool {
        match self.items.iter().position(|n| n.id == id) {
            Some(pos) if !self.items.get(pos).is_some_and(|n| n.persistent) => {
                self.items.remove(pos);
                self.unread = self.unread.min(self.items.len());
                true
            }
            _ => false,
        }
    }

    /// Dismiss every non-persistent one ("clear all").
    pub fn clear(&mut self) {
        self.items.retain(|n| n.persistent);
        self.unread = 0;
        self.warned_full = false;
    }

    /// Opening the shade takes the unread count to zero.
    pub(crate) fn mark_seen(&mut self) {
        self.unread = 0;
    }

    /// The unread count (the status bar badge).
    #[must_use]
    pub fn unread(&self) -> usize {
        self.unread
    }

    /// The total count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether it is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Newest first.
    pub fn iter(&self) -> impl Iterator<Item = &Notification> {
        self.items.iter()
    }

    /// Look one up.
    #[must_use]
    pub fn get(&self, id: NotificationId) -> Option<&Notification> {
        self.items.iter().find(|n| n.id == id)
    }

    /// The source groups (newest first). Returns each run of the same `source` as
    /// `(source, notifications)`. A notification with an empty `source` is not a group, so it
    /// comes out on its own. No allocation.
    pub fn groups(&self) -> impl Iterator<Item = (&str, &[Notification])> {
        self.items
            .chunk_by(|a, b| !a.source.is_empty() && a.source == b.source)
            .filter_map(|chunk| Some((chunk.first()?.source.as_str(), chunk)))
    }

    /// How many notifications one source has (for putting "3" on a group header). No allocation.
    #[must_use]
    pub fn group_len(&self, source: &str) -> usize {
        if source.is_empty() {
            return 0;
        }
        self.items.iter().filter(|n| n.source == source).count()
    }

    /// How many notifications have **their content hidden** from this session (the redacted row count). No allocation.
    #[must_use]
    pub fn hidden(&self, access: &Access) -> usize {
        self.items
            .iter()
            .filter(|n| !shows_content(n, access))
            .count()
    }

    /// The retention cap (0 = unlimited).
    #[must_use]
    pub fn max_items(&self) -> usize {
        self.max_items
    }

    /// Insert a new notification at the front of its source group. If the group exists, it is
    /// rotated to the front of the list first — so the group stays together as the entry goes in
    /// and the list is newest-first by group.
    fn insert_grouped(&mut self, notification: Notification) {
        if !notification.source.is_empty() {
            if let Some((start, end)) = self.group_range(&notification.source) {
                if let Some(prefix) = self.items.get_mut(..end) {
                    prefix.rotate_right(end - start);
                }
            }
        }
        self.items.insert(0, notification);
    }

    /// The `[start, end)` indices of the `source` group. `None` if there is none.
    fn group_range(&self, source: &str) -> Option<(usize, usize)> {
        let start = self.items.iter().position(|n| n.source == source)?;
        let end = self
            .items
            .iter()
            .skip(start)
            .position(|n| n.source != source)
            .map_or(self.items.len(), |offset| start + offset);
        Some((start, end))
    }

    /// The cap rule: drop the oldest **non-persistent** ones first. If everything is persistent
    /// and nothing can be dropped, report it once (overflowing quietly leaves
    /// nothing but "why does this keep piling up").
    fn enforce_cap(&mut self) {
        if self.max_items == 0 {
            return;
        }
        while self.items.len() > self.max_items {
            let Some(pos) = self.items.iter().rposition(|n| !n.persistent) else {
                if !self.warned_full {
                    self.warned_full = true;
                    log::warn!(
                        "[notify] persistent notifications went past max_items = {} - nothing can be dropped, so they keep piling up",
                        self.max_items
                    );
                }
                break;
            };
            self.items.remove(pos);
        }
        self.unread = self.unread.min(self.items.len());
    }
}

#[cfg(test)]
mod tests {
    use super::{shows_content, CenterChange, NotificationCenter};
    use crate::access::{Access, Gate};
    use crate::config::AccessConfig;
    use crate::notify::{Notification, NotificationId};

    fn note(id: u64, source: &str) -> Notification {
        Notification::new(NotificationId(id), "t").source(source)
    }

    #[test]
    fn dedup_and_unread() {
        let mut c = NotificationCenter::new(0);
        assert_eq!(
            c.push(Notification::new(NotificationId(1), "a")),
            CenterChange::Added
        );
        assert_eq!(
            c.push(Notification::new(NotificationId(1), "b")),
            CenterChange::Updated
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c.unread(), 1);
        c.mark_seen();
        assert_eq!(c.unread(), 0);
        assert!(c.dismiss(NotificationId(1)));
        assert!(c.is_empty());
    }

    #[test]
    fn persistent_survives_dismiss_and_overflow() {
        let mut c = NotificationCenter::new(2);
        c.push(Notification::new(NotificationId(1), "p").persistent());
        c.push(Notification::new(NotificationId(2), "b"));
        c.push(Notification::new(NotificationId(3), "c"));
        assert_eq!(c.len(), 2);
        assert!(c.get(NotificationId(1)).is_some(), "a persistent one stays");
        assert!(!c.dismiss(NotificationId(1)));
    }

    /// With everything at the cap persistent there is nothing to drop — it stays over and warns once.
    #[test]
    fn cap_keeps_persistent_and_clamps_unread() {
        let mut c = NotificationCenter::new(2);
        for id in 1..=4 {
            c.push(Notification::new(NotificationId(id), "p").persistent());
        }
        assert_eq!(c.len(), 4, "there is no non-persistent one to drop");
        assert_eq!(c.unread(), 4);
        let mut c = NotificationCenter::new(2);
        for id in 1..=5 {
            c.push(Notification::new(NotificationId(id), "n"));
        }
        assert_eq!(c.len(), 2);
        assert_eq!(
            c.unread(),
            2,
            "a dropped notification leaves the unread count too"
        );
    }

    /// The same source stays together, and a new notification brings that group to the front.
    #[test]
    fn same_source_notifications_stay_grouped() {
        let mut c = NotificationCenter::new(0);
        c.push(note(1, "printer"));
        c.push(note(2, "oven"));
        c.push(note(3, "printer"));
        let ids: Vec<u64> = c.iter().map(|n| n.id.0).collect();
        assert_eq!(
            ids,
            vec![3, 1, 2],
            "the printer group comes first, newest first inside it"
        );
        let groups: Vec<(&str, usize)> = c.groups().map(|(s, g)| (s, g.len())).collect();
        assert_eq!(groups, vec![("printer", 2), ("oven", 1)]);
        assert_eq!(c.group_len("printer"), 2);
        assert_eq!(c.group_len(""), 0);
        // With no source there is no group — they come out one at a time.
        c.push(Notification::new(NotificationId(4), "x"));
        c.push(Notification::new(NotificationId(5), "y"));
        assert_eq!(c.groups().count(), 4);
    }

    /// An update that changes the source moves it to the new group without changing the count or the unread total.
    #[test]
    fn update_moves_between_groups_without_counting_twice() {
        let mut c = NotificationCenter::new(0);
        c.push(note(1, "a"));
        c.push(note(2, "b"));
        assert_eq!(c.push(note(1, "b")), CenterChange::Updated);
        assert_eq!(c.len(), 2);
        assert_eq!(c.unread(), 2);
        let groups: Vec<(&str, usize)> = c.groups().map(|(s, g)| (s, g.len())).collect();
        assert_eq!(groups, vec![("b", 2)]);
    }

    /// A notification that fails its gate stays in the list with its content hidden.
    #[test]
    fn gated_notification_hides_its_content() -> crate::Result<()> {
        let cfg = AccessConfig {
            levels: vec!["viewer".to_owned(), "admin".to_owned()],
            default_gate: Some("top".to_owned()),
            ..AccessConfig::default()
        };
        let access = Access::from_config(&cfg)?;
        let open = Notification::new(NotificationId(1), "visible");
        let secret = Notification::new(NotificationId(2), "secret").gate(Gate::borrowed("secret"));
        assert!(shows_content(&open, &access));
        assert!(!shows_content(&secret, &access));
        let mut c = NotificationCenter::new(0);
        c.push(open);
        c.push(secret);
        assert_eq!(c.len(), 2);
        assert_eq!(c.hidden(&access), 1);
        Ok(())
    }
}
