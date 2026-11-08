//! Channel receiving APIs without capacity- or message-waiting methods.
//!
//! [`Inbox`] owns its receiver and exposes `try_recv` and draining iterators. It
//! provides no receiver loan or `IntoIterator` implementation that would expose
//! `Receiver`'s waiting iterator. Individual receives use `try_recv`; an unlimited
//! `drain` can still continue while producers keep adding messages. Use a finite
//! batch at UI/service boundaries. Standard-channel synchronization is upstream.

#[cfg(fairing_loom)]
use loom::sync::mpsc;
use mpsc::Sender;
#[cfg(not(fairing_loom))]
use std::sync::mpsc;
use std::sync::mpsc::TryRecvError;

#[path = "inbox/bounded.rs"]
mod bounded;
#[path = "inbox/configured.rs"]
mod configured;
pub use bounded::{
    BoundedInbox, BoundedSender, QueueCapacityError, QueueErrorKind, QueueStats, TryBuildSendError,
    TrySendError, MAX_BOUNDED_CAPACITY,
};
pub use configured::{ConfiguredInbox, ConfiguredSender, InboxCapacity, InboxConfig};

impl<T> Inbox<T> {
    /// A preallocated bounded transport exposing no blocking send or receive.
    ///
    /// A full queue returns the original message; it never evicts another message.
    /// Use the lazy sender to reserve capacity before constructing a payload.
    ///
    /// # Errors
    /// Zero, excessive capacity, or a slot layout that cannot be represented.
    pub fn bounded(
        capacity: usize,
    ) -> Result<(BoundedSender<T>, BoundedInbox<T>), QueueCapacityError> {
        bounded::pair(capacity)
    }
}

/// The receiving end of an mpsc channel without a message-waiting receive API.
///
/// ```
/// use fairing::inbox::Inbox;
/// let (tx, inbox) = Inbox::pair();
/// let _ = tx.send(1u8);
/// let _ = tx.send(2);
/// assert_eq!(inbox.drain().collect::<Vec<_>>(), vec![1, 2]);
/// // Empty is `None` at once — it never waits for the next one.
/// assert_eq!(inbox.drain().next(), None);
/// ```
///
/// The blocking spellings are **not reachable**. `for msg in inbox` is the one a lint cannot ban
/// (it desugars to `IntoIterator::into_iter`, which every `for` loop uses), so `Inbox` simply does
/// not implement `IntoIterator`:
///
/// ```compile_fail
/// use fairing::inbox::Inbox;
/// let (_tx, inbox) = Inbox::<u8>::pair();
/// for _msg in inbox {}   // the loop that would wait for the next message
/// ```
///
/// Nor is the receiver lent out, so nothing can be called on it:
///
/// ```compile_fail
/// use fairing::inbox::Inbox;
/// let (_tx, inbox) = Inbox::<u8>::pair();
/// let _ = inbox.recv();  // no such method — `try_recv` and `drain` are the whole of it
/// ```
#[cfg_attr(not(fairing_loom), derive(Debug))]
pub struct Inbox<T> {
    // The one place in the workspace a bare `Receiver` is allowed to live. It is private and never
    // handed out, which is what makes the guarantee above hold.
    #[expect(
        clippy::disallowed_types,
        reason = "Inbox is the wrapper that makes the blocking half unreachable"
    )]
    rx: mpsc::Receiver<T>,
}

impl<T> Inbox<T> {
    /// A channel: the ordinary [`Sender`], and the non-blocking receiving end.
    #[must_use]
    pub fn pair() -> (Sender<T>, Self) {
        let (tx, rx) = mpsc::channel();
        (tx, Self { rx })
    }

    /// The next message, or why there is not one. Never waits.
    ///
    /// # Errors
    /// [`TryRecvError::Empty`] where nothing has arrived, [`TryRecvError::Disconnected`] where every
    /// sender is gone.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.rx.try_recv()
    }

    /// At most `limit` currently available items. Continued production cannot extend the batch.
    pub fn drain_up_to(&self, limit: usize) -> impl Iterator<Item = T> + '_ {
        self.drain().take(limit)
    }

    /// Available items, in order. This is not a snapshot and has no batch limit.
    /// Prefer `drain_up_to` or a configured inbox at service boundaries.
    ///
    /// Ends the moment the queue is empty rather
    /// than waiting for the next message, which is what separates it from `Receiver::iter`.
    pub fn drain(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }
}

#[cfg(all(test, not(fairing_loom)))]
mod tests {
    use super::Inbox;

    /// `drain` stops at the end of the queue instead of waiting — the whole point of the type.
    #[test]
    fn drain_ends_at_the_end_of_the_queue() {
        let (tx, inbox) = Inbox::pair();
        for n in 0..3u8 {
            assert!(tx.send(n).is_ok());
        }
        assert_eq!(inbox.drain().collect::<Vec<_>>(), vec![0, 1, 2]);
        assert_eq!(inbox.drain().count(), 0, "a second pass finds nothing");
        assert!(tx.send(9).is_ok());
        assert_eq!(inbox.drain().collect::<Vec<_>>(), vec![9], "and it refills");
    }

    /// With every sender dropped it is empty, not stuck.
    #[test]
    fn a_closed_channel_drains_empty() {
        let (tx, inbox) = Inbox::<u8>::pair();
        assert!(tx.send(1).is_ok());
        drop(tx);
        assert_eq!(inbox.drain().collect::<Vec<_>>(), vec![1]);
        assert!(inbox.try_recv().is_err());
    }
}
