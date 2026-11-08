//! Nonblocking, preallocated transport with capacity reservations shared by all producers.

use super::Inbox;
#[cfg(fairing_loom)]
use loom::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::fmt;
use std::sync::mpsc::TryRecvError;
#[cfg(not(fairing_loom))]
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

/// Largest admitted queue capacity. Construction still requires sufficient heap memory.
pub const MAX_BOUNDED_CAPACITY: usize = 65_536;

/// Invalid or unrepresentable bounded queue capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueCapacityError {
    /// The rejected capacity.
    pub requested: usize,
}

impl fmt::Display for QueueCapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "queue capacity {} must be 1..={MAX_BOUNDED_CAPACITY} with a representable slot layout",
            self.requested
        )
    }
}
impl std::error::Error for QueueCapacityError {}

/// Why a nonblocking enqueue was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueErrorKind {
    /// All slots, including producers' reservations, are occupied.
    Full,
    /// The receiver has been dropped.
    Disconnected,
}
impl fmt::Display for QueueErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "queue full",
            Self::Disconnected => "queue disconnected",
        })
    }
}
impl std::error::Error for QueueErrorKind {}

/// Rejection preserving ownership without allocating an error wrapper.
/// Debug/Display never print the message, which may contain credentials or patient data.
pub struct TrySendError<T> {
    kind: QueueErrorKind,
    message: T,
}
impl<T> TrySendError<T> {
    pub(super) fn disconnected(message: T) -> Self {
        Self {
            kind: QueueErrorKind::Disconnected,
            message,
        }
    }
    /// The rejection reason.
    #[must_use]
    pub const fn kind(&self) -> QueueErrorKind {
        self.kind
    }
    /// Recover the rejected message for a caller-defined retry/discard policy.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.message
    }
}
impl<T> fmt::Debug for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.kind, f)
    }
}
impl<T> fmt::Display for TrySendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.kind, f)
    }
}
impl<T> std::error::Error for TrySendError<T> {}

/// Slot admission or fallible payload construction failed. No error wrapper allocation.
pub enum TryBuildSendError<E> {
    /// No slot is available, or the receiver is closed. The factory was not called.
    Queue(QueueErrorKind),
    /// The payload factory returned its own error; the slot was returned.
    Build(E),
}
impl<E> fmt::Debug for TryBuildSendError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Queue(kind) => fmt::Debug::fmt(kind, f),
            Self::Build(_) => f.write_str("payload construction failed"),
        }
    }
}
impl<E> fmt::Display for TryBuildSendError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}
impl<E> std::error::Error for TryBuildSendError<E> {}

/// Individually sampled counters; concurrent changes can occur between fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStats {
    /// Fixed slot count.
    pub capacity: usize,
    /// Queued messages plus producers currently constructing reserved payloads.
    pub outstanding: usize,
    /// Largest simultaneous reservation count.
    pub high_water: usize,
    /// Capacity rejections, saturating at `usize::MAX`.
    pub rejected_full: usize,
    /// Whether a receiver still exists.
    pub receiver_open: bool,
}

struct Budget {
    capacity: usize,
    used: AtomicUsize,
    peak: AtomicUsize,
    rejected: AtomicUsize,
    open: AtomicBool,
}
impl Budget {
    fn stats(&self) -> QueueStats {
        QueueStats {
            capacity: self.capacity,
            outstanding: self.used.load(Ordering::Acquire),
            high_water: self.peak.load(Ordering::Relaxed),
            rejected_full: self.rejected.load(Ordering::Relaxed),
            receiver_open: self.open.load(Ordering::Acquire),
        }
    }
}

struct Permit(Arc<Budget>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.used.fetch_sub(1, Ordering::AcqRel);
    }
}
struct Envelope<T> {
    message: T,
    _permit: Permit,
}
impl<T> Envelope<T> {
    fn into_inner(self) -> T {
        self.message
    }
}

#[cfg(not(fairing_loom))]
#[expect(
    clippy::disallowed_types,
    reason = "private transport; only try_send is exposed"
)]
type Transport<T> = std::sync::mpsc::SyncSender<Envelope<T>>; // sync-check: allow: private transport exposes only try_send
#[cfg(fairing_loom)]
type Transport<T> = loom::sync::mpsc::Sender<Envelope<T>>;

/// A cloneable sender with no capacity-waiting API.
/// Clones share one budget; cloning cannot increase capacity.
pub struct BoundedSender<T> {
    tx: Transport<T>,
    budget: Arc<Budget>,
}
impl<T> Clone for BoundedSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            budget: Arc::clone(&self.budget),
        }
    }
}
impl<T> fmt::Debug for BoundedSender<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedSender")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}
impl<T> BoundedSender<T> {
    fn reserve(&self) -> Result<Permit, QueueErrorKind> {
        if !self.budget.open.load(Ordering::Acquire) {
            return Err(QueueErrorKind::Disconnected);
        }
        let mut used = self.budget.used.load(Ordering::Acquire);
        loop {
            if used >= self.budget.capacity {
                if !self.budget.open.load(Ordering::Acquire) {
                    return Err(QueueErrorKind::Disconnected);
                }
                let mut rejected = self.budget.rejected.load(Ordering::Relaxed);
                while rejected != usize::MAX {
                    match self.budget.rejected.compare_exchange(
                        rejected,
                        rejected + 1,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(actual) => rejected = actual,
                    }
                }
                return Err(QueueErrorKind::Full);
            }
            match self.budget.used.compare_exchange(
                used,
                used + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    self.budget.peak.fetch_max(used + 1, Ordering::Relaxed);
                    return Ok(Permit(Arc::clone(&self.budget)));
                }
                Err(actual) => used = actual,
            }
        }
    }

    fn publish(&self, envelope: Envelope<T>) -> Result<(), TrySendError<T>> {
        #[cfg(not(fairing_loom))]
        {
            self.tx.try_send(envelope).map_err(|error| {
                let (kind, envelope) = match error {
                    std::sync::mpsc::TrySendError::Full(e) => (QueueErrorKind::Full, e),
                    std::sync::mpsc::TrySendError::Disconnected(e) => {
                        (QueueErrorKind::Disconnected, e)
                    }
                };
                TrySendError {
                    kind,
                    message: envelope.into_inner(),
                }
            })
        }
        #[cfg(fairing_loom)]
        {
            // Loom 0.7 has no sync_channel. The same production reservation protocol
            // bounds its model channel; native/Miri tests exercise the real sync_channel.
            self.tx.send(envelope).map_err(|e| TrySendError {
                kind: QueueErrorKind::Disconnected,
                message: e.0.into_inner(),
            })
        }
    }

    /// Try to enqueue without waiting for queue capacity; return the reason and message on refusal.
    /// A message's owned String/Vec payloads can allocate independently of slot storage.
    ///
    /// # Errors
    /// Full or disconnected. No accepted message is evicted.
    pub fn try_send(&self, message: T) -> Result<(), TrySendError<T>> {
        let permit = match self.reserve() {
            Ok(p) => p,
            Err(kind) => return Err(TrySendError { kind, message }),
        };
        self.publish(Envelope {
            message,
            _permit: permit,
        })
    }

    /// Reserve a slot before calling the payload factory. A full/closed queue does
    /// not call it. If the receiver closes concurrently, a factory may already be running.
    /// An unwinding factory automatically returns its reservation.
    ///
    /// # Errors
    /// Full or disconnected. A payload rejected after construction is dropped.
    pub fn try_send_with(&self, make: impl FnOnce() -> T) -> Result<(), QueueErrorKind> {
        let permit = self.reserve()?;
        self.publish(Envelope {
            message: make(),
            _permit: permit,
        })
        .map_err(|error| error.kind())
    }

    /// Reserve capacity before a fallible factory. Factory failure returns its slot.
    ///
    /// # Errors
    /// Queue admission failure or the factory's error; no message is enqueued on either.
    pub fn try_send_fallible_with<E>(
        &self,
        make: impl FnOnce() -> Result<T, E>,
    ) -> Result<(), TryBuildSendError<E>> {
        let permit = self.reserve().map_err(TryBuildSendError::Queue)?;
        let message = make().map_err(TryBuildSendError::Build)?;
        self.publish(Envelope {
            message,
            _permit: permit,
        })
        .map_err(|error| TryBuildSendError::Queue(error.kind()))
    }

    /// Sample capacity/usage/overload counters without touching message contents.
    #[must_use]
    pub fn stats(&self) -> QueueStats {
        self.budget.stats()
    }
}

/// The bounded receiving end. Capacity is returned when a message is received or discarded.
pub struct BoundedInbox<T> {
    rx: Inbox<Envelope<T>>,
    budget: Arc<Budget>,
}
impl<T> fmt::Debug for BoundedInbox<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoundedInbox")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}
impl<T> Drop for BoundedInbox<T> {
    fn drop(&mut self) {
        self.budget.open.store(false, Ordering::Release);
    }
}
impl<T> BoundedInbox<T> {
    /// Refuse future admission without dropping accepted messages or invoking their destructors.
    /// A factory already running may still finish; callers must stop dispatch after a fault.
    pub fn close_admission(&self) {
        self.budget.open.store(false, Ordering::Release);
    }

    /// Try to receive a message, returning its capacity reservation when one is received.
    ///
    /// # Errors
    /// Empty or disconnected; never waits.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        self.rx.try_recv().map(Envelope::into_inner)
    }
    /// Drain at most capacity messages, stopping earlier if empty.
    /// A producer refilling the queue cannot turn this into an unbounded loop.
    pub fn drain(&self) -> impl Iterator<Item = T> + '_ {
        self.drain_up_to(self.budget.capacity)
    }
    /// Drain at most limit messages, stopping earlier if empty.
    pub fn drain_up_to(&self, limit: usize) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok()).take(limit)
    }
    /// Includes producers currently preparing a reserved payload.
    #[must_use]
    pub fn stats(&self) -> QueueStats {
        self.budget.stats()
    }
}

#[cfg(all(test, not(fairing_loom)))]
#[allow(
    clippy::unwrap_used,
    reason = "test assertions retain the failing operation"
)]
mod tests {
    use super::{Inbox, QueueErrorKind, MAX_BOUNDED_CAPACITY};
    use std::cell::Cell;

    #[test]
    fn capacity_is_shared_rejections_preserve_payload_and_receiving_returns_slots() {
        let (a, inbox) = Inbox::bounded(2).unwrap();
        let b = a.clone();
        a.try_send("private one".to_owned()).unwrap();
        b.try_send("private two".to_owned()).unwrap();
        let rejected = a.try_send("private three".to_owned()).unwrap_err();
        assert_eq!(rejected.kind(), QueueErrorKind::Full);
        assert!(!format!("{rejected:?}").contains("private"));
        assert_eq!(rejected.into_inner(), "private three");
        assert_eq!(a.stats().outstanding, 2);
        assert_eq!(a.stats().high_water, 2);
        assert_eq!(a.stats().rejected_full, 1);
        assert_eq!(inbox.try_recv().unwrap(), "private one");
        b.try_send("replacement".to_owned()).unwrap();
        assert_eq!(
            inbox.drain().collect::<Vec<_>>(),
            ["private two", "replacement"]
        );
        assert_eq!(a.stats().outstanding, 0);
    }

    #[test]
    fn factories_do_not_run_on_full_or_closed_queues_and_close_releases_messages() {
        let (tx, inbox) = Inbox::bounded(1).unwrap();
        let calls = Cell::new(0);
        tx.try_send(7).unwrap();
        assert_eq!(
            tx.try_send_with(|| {
                calls.set(calls.get() + 1);
                8
            }),
            Err(QueueErrorKind::Full)
        );
        assert_eq!(calls.get(), 0);
        drop(inbox);
        assert_eq!(tx.stats().outstanding, 0);
        assert!(!tx.stats().receiver_open);
        assert_eq!(
            tx.try_send_with(|| {
                calls.set(calls.get() + 1);
                9
            }),
            Err(QueueErrorKind::Disconnected)
        );
        assert_eq!(calls.get(), 0);
    }

    #[test]
    #[allow(
        clippy::panic,
        reason = "synthetic unwinding verifies reservation cleanup"
    )]
    fn an_unwinding_or_reentrant_factory_does_not_leak_its_reservation() {
        let (tx, inbox) = Inbox::<u8>::bounded(1).unwrap();
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = tx.try_send_with(|| panic!("synthetic factory failure"));
        }));
        assert!(failure.is_err());
        assert_eq!(tx.stats().outstanding, 0);
        tx.try_send_with(|| {
            assert_eq!(tx.try_send(2).unwrap_err().kind(), QueueErrorKind::Full);
            1
        })
        .unwrap();
        assert_eq!(inbox.try_recv().unwrap(), 1);
    }

    #[test]
    fn close_during_factory_returns_a_disconnected_error_and_releases_the_slot() {
        let (tx, inbox) = Inbox::<u8>::bounded(1).unwrap();
        assert_eq!(
            tx.try_send_with(|| {
                drop(inbox);
                3
            }),
            Err(QueueErrorKind::Disconnected)
        );
        assert_eq!(tx.stats().outstanding, 0);
    }

    #[test]
    fn draining_has_a_work_bound_even_when_the_sender_refills() {
        let (tx, inbox) = Inbox::bounded(2).unwrap();
        tx.try_send(0).unwrap();
        let received: Vec<_> = inbox
            .drain()
            .inspect(|&n| {
                tx.try_send(n + 1).unwrap();
            })
            .collect();
        assert_eq!(received, [0, 1]);
        assert_eq!(inbox.try_recv().unwrap(), 2);
    }

    #[test]
    fn invalid_capacities_are_rejected_before_channel_construction() {
        assert!(Inbox::<u8>::bounded(0).is_err());
        assert!(Inbox::<u8>::bounded(MAX_BOUNDED_CAPACITY + 1).is_err());
        assert!(Inbox::<u8>::bounded(usize::MAX).is_err());
    }
}

pub(super) fn pair<T>(
    capacity: usize,
) -> Result<(BoundedSender<T>, BoundedInbox<T>), QueueCapacityError> {
    if !(1..=MAX_BOUNDED_CAPACITY).contains(&capacity)
        || std::alloc::Layout::array::<Envelope<T>>(capacity).is_err()
    {
        return Err(QueueCapacityError {
            requested: capacity,
        });
    }
    Ok(pair_validated(capacity))
}

/// Construct a transport after capacity/layout validation, or for a fixed crate-owned
/// message type and capacity whose layout is covered by the default-channel tests.
pub(super) fn pair_validated<T>(capacity: usize) -> (BoundedSender<T>, BoundedInbox<T>) {
    #[cfg(not(fairing_loom))]
    let (tx, rx) = std::sync::mpsc::sync_channel(capacity); // sync-check: allow: preallocated transport exposes only try_send
    #[cfg(fairing_loom)]
    let (tx, rx) = loom::sync::mpsc::channel();
    let budget = Arc::new(Budget {
        capacity,
        used: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        rejected: AtomicUsize::new(0),
        open: AtomicBool::new(true),
    });
    (
        BoundedSender {
            tx,
            budget: Arc::clone(&budget),
        },
        BoundedInbox {
            rx: Inbox { rx },
            budget,
        },
    )
}
