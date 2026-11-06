//! Atomic publication. All fields participate in one sequentially consistent order.

use super::session_rules::{self, Observation};
#[cfg(fairing_loom)]
use loom::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(not(fairing_loom))]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

pub(crate) struct GrantCell {
    pub(crate) base: AtomicU64,
    pub(crate) temporary: AtomicU64,
}

pub(crate) struct Shared {
    revision: AtomicU64,
    revoked: AtomicBool,
    until: AtomicU64,
    grants: Vec<GrantCell>,
}

impl Shared {
    pub(crate) fn new(until: u64, grants: &[(u64, u64)]) -> Self {
        Self {
            revision: AtomicU64::new(0),
            revoked: AtomicBool::new(false),
            until: AtomicU64::new(until),
            grants: grants
                .iter()
                .map(|&(base, temporary)| GrantCell {
                    base: AtomicU64::new(base),
                    temporary: AtomicU64::new(temporary),
                })
                .collect(),
        }
    }

    /// A controller is the sole writer; neither this method nor its state is public API.
    pub(crate) fn publish(&self, now: u64, until: u64, grants: &[(u64, u64)]) -> bool {
        if grants.len() != self.grants.len() {
            return false;
        }
        let before = self.revision.load(Ordering::SeqCst);
        let state = session_rules::ControlState {
            revision: before,
            revoked: self.revoked.load(Ordering::SeqCst),
        };
        let next = session_rules::transition(
            state,
            session_rules::ControlEvent::Publish {
                now,
                until: self.until.load(Ordering::SeqCst),
            },
        );
        if next.revoked {
            self.revoke();
            return false;
        }
        let after = next.revision;
        self.revision.store(before + 1, Ordering::SeqCst);
        self.until.store(until, Ordering::SeqCst);
        for (cell, &(base, temporary)) in self.grants.iter().zip(grants) {
            cell.base.store(base, Ordering::SeqCst);
            cell.temporary.store(temporary, Ordering::SeqCst);
        }
        self.revision.store(after, Ordering::SeqCst);
        true
    }

    pub(crate) fn revoke(&self) {
        let state = session_rules::ControlState {
            revision: self.revision.load(Ordering::SeqCst),
            revoked: self.revoked.load(Ordering::SeqCst),
        };
        let next = session_rules::transition(state, session_rules::ControlEvent::Revoke);
        self.revoked.store(next.revoked, Ordering::SeqCst);
    }

    pub(crate) fn observe(&self, now: u64) -> Observation {
        let before = self.revision.load(Ordering::SeqCst);
        let until = self.until.load(Ordering::SeqCst);
        let revoked = self.revoked.load(Ordering::SeqCst);
        let after = self.revision.load(Ordering::SeqCst);
        session_rules::observe(revoked, before, after, now, until)
    }

    pub(crate) fn allows(&self, index: usize, now: u64, expected: u64) -> bool {
        let before = self.revision.load(Ordering::SeqCst);
        let Some(cell) = self.grants.get(index) else {
            return false;
        };
        let base = cell.base.load(Ordering::SeqCst);
        let temporary = cell.temporary.load(Ordering::SeqCst);
        let until = self.until.load(Ordering::SeqCst);
        let revoked = self.revoked.load(Ordering::SeqCst);
        let after = self.revision.load(Ordering::SeqCst);
        let live = before == expected
            && matches!(
                session_rules::observe(revoked, before, after, now, until),
                Observation::Active(_)
            );
        session_rules::permission_allows(live, now, base, temporary)
    }

    pub(crate) fn next_deadline(&self, now: u64) -> Option<u64> {
        let before = self.revision.load(Ordering::SeqCst);
        let until = self.until.load(Ordering::SeqCst);
        let mut next = (until > now && until != u64::MAX).then_some(until);
        for cell in &self.grants {
            for at in [
                cell.base.load(Ordering::SeqCst),
                cell.temporary.load(Ordering::SeqCst),
            ] {
                if at > now && at != u64::MAX {
                    next = Some(next.map_or(at, |old| old.min(at)));
                }
            }
        }
        let after = self.revision.load(Ordering::SeqCst);
        if self.revoked.load(Ordering::SeqCst) || before != after || !before.is_multiple_of(2) {
            Some(now)
        } else {
            next
        }
    }
}
