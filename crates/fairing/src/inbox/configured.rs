//! Connection-specific capacity and per-pass processing limits.
use super::{BoundedInbox, BoundedSender, Inbox, QueueCapacityError, TrySendError};
use std::sync::mpsc::TryRecvError;

const DEFAULT_CAPACITY: usize = 1024;
const DEFAULT_BATCH_LIMIT: usize = 256;

/// Storage policy for an individual connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxCapacity {
    /// Reject sends when this many slots are occupied.
    Bounded(usize),
    /// Explicitly permit unbounded storage for a trusted, rate-controlled producer.
    Unbounded,
}

/// Connection configuration. Storage and processing limits are independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboxConfig {
    /// Storage policy. Unbounded storage can exhaust process memory.
    pub capacity: InboxCapacity,
    /// Maximum items returned by one `ConfiguredInbox::drain` call.
    /// Zero pauses draining; individual `try_recv` calls remain available.
    pub batch_limit: usize,
}
impl Default for InboxConfig {
    fn default() -> Self {
        Self {
            capacity: InboxCapacity::Bounded(DEFAULT_CAPACITY),
            batch_limit: DEFAULT_BATCH_LIMIT,
        }
    }
}

/// Nonblocking sender using the connection's selected capacity policy.
pub enum ConfiguredSender<T> {
    /// Bounded admission, including lazy payload construction through the contained sender.
    Bounded(BoundedSender<T>),
    /// Unbounded storage. Sending may allocate.
    Unbounded(super::Sender<T>),
}
impl<T> Clone for ConfiguredSender<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Bounded(tx) => Self::Bounded(tx.clone()),
            Self::Unbounded(tx) => Self::Unbounded(tx.clone()),
        }
    }
}
impl<T> ConfiguredSender<T> {
    /// Send without waiting for queue capacity, preserving rejected message ownership.
    ///
    /// # Errors
    /// Full bounded storage or a disconnected receiver.
    pub fn try_send(&self, message: T) -> Result<(), TrySendError<T>> {
        match self {
            Self::Bounded(tx) => tx.try_send(message),
            Self::Unbounded(tx) => tx
                .send(message)
                .map_err(|error| TrySendError::disconnected(error.0)),
        }
    }
}

enum Transport<T> {
    Bounded(BoundedInbox<T>),
    Unbounded(Inbox<T>),
}

/// Receiver with a finite, connection-specific default batch limit.
pub struct ConfiguredInbox<T> {
    transport: Transport<T>,
    batch_limit: usize,
}
impl<T> ConfiguredInbox<T> {
    /// Receive one item without waiting.
    ///
    /// # Errors
    /// Empty or disconnected queue.
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match &self.transport {
            Transport::Bounded(rx) => rx.try_recv(),
            Transport::Unbounded(rx) => rx.try_recv(),
        }
    }
    /// Process at most the configured batch limit, even while producers keep sending.
    pub fn drain(&self) -> impl Iterator<Item = T> + '_ {
        self.drain_up_to(self.batch_limit)
    }
    /// Process at most `limit` items, stopping early if empty.
    pub fn drain_up_to(&self, limit: usize) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok()).take(limit)
    }
    /// Change the default per-pass limit. Zero pauses default draining.
    pub fn set_batch_limit(&mut self, limit: usize) {
        self.batch_limit = limit;
    }
}
impl<T> Inbox<T> {
    /// Fixed default for the crate's six concrete mock message enums. Public generic
    /// construction still validates the selected capacity and message layout.
    #[allow(
        dead_code,
        reason = "used by optional mock backends; Inbox is also included by standalone verification"
    )]
    pub(crate) fn mock_default() -> (ConfiguredSender<T>, ConfiguredInbox<T>) {
        let (tx, rx) = super::bounded::pair_validated(DEFAULT_CAPACITY);
        (
            ConfiguredSender::Bounded(tx),
            ConfiguredInbox {
                transport: Transport::Bounded(rx),
                batch_limit: DEFAULT_BATCH_LIMIT,
            },
        )
    }

    /// Build a connection with explicit storage and processing policies.
    ///
    /// # Errors
    /// An invalid bounded capacity. Construction still requires sufficient heap memory.
    pub fn configured(
        config: InboxConfig,
    ) -> Result<(ConfiguredSender<T>, ConfiguredInbox<T>), QueueCapacityError> {
        let (tx, transport) = match config.capacity {
            InboxCapacity::Bounded(capacity) => {
                let (tx, rx) = Self::bounded(capacity)?;
                (ConfiguredSender::Bounded(tx), Transport::Bounded(rx))
            }
            InboxCapacity::Unbounded => {
                let (tx, rx) = Self::pair();
                (ConfiguredSender::Unbounded(tx), Transport::Unbounded(rx))
            }
        };
        Ok((
            tx,
            ConfiguredInbox {
                transport,
                batch_limit: config.batch_limit,
            },
        ))
    }
}

#[cfg(all(test, not(fairing_loom)))]
#[allow(
    clippy::unwrap_used,
    reason = "test assertions retain the failing operation"
)]
mod tests {
    use super::*;
    use crate::inbox::QueueErrorKind;
    #[test]
    fn storage_and_batch_policies_are_independent() {
        for capacity in [InboxCapacity::Bounded(3), InboxCapacity::Unbounded] {
            let (tx, mut rx) = Inbox::configured(InboxConfig {
                capacity,
                batch_limit: 1,
            })
            .unwrap();
            for i in 0..3 {
                tx.try_send(i).unwrap();
            }
            if matches!(capacity, InboxCapacity::Bounded(_)) {
                let error = tx.try_send(9).unwrap_err();
                assert_eq!(error.kind(), QueueErrorKind::Full);
                assert_eq!(error.into_inner(), 9);
            }
            assert_eq!(rx.drain().collect::<Vec<_>>(), [0]);
            rx.set_batch_limit(0);
            assert_eq!(rx.drain().next(), None);
            rx.set_batch_limit(2);
            assert_eq!(rx.drain().collect::<Vec<_>>(), [1, 2]);
            drop(rx);
            let error = tx.try_send(10).unwrap_err();
            assert_eq!(error.kind(), QueueErrorKind::Disconnected);
            assert_eq!(error.into_inner(), 10);
        }
    }
    #[test]
    fn replenishing_a_queue_does_not_extend_a_batch() {
        let (tx, rx) = Inbox::configured(InboxConfig {
            capacity: InboxCapacity::Unbounded,
            batch_limit: 7,
        })
        .unwrap();
        tx.try_send(0).unwrap();
        let mut count = 0;
        for value in rx.drain() {
            count += 1;
            tx.try_send(value + 1).unwrap();
        }
        assert_eq!(count, 7);
        assert_eq!(rx.try_recv(), Ok(7));
    }
}
