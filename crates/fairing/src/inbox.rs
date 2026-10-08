//! [`Inbox`] — the receiving end of a channel with **the blocking half taken away**.
//!
//! The UI thread may not block. `try_recv` is the whole of the contract, and everything else
//! `std::sync::mpsc::Receiver` offers — `recv`, `recv_timeout`, `recv_deadline`, `iter`, and being
//! iterated as `for msg in rx` — waits for a message to arrive.
//!
//! Five of those six are named in `clippy.toml`'s `disallowed-methods`, which resolves the receiver
//! **by type** and so cannot be walked past with an alias. The sixth cannot be: `for msg in rx`
//! desugars to `IntoIterator::into_iter`, and banning that would ban every `for` loop in the
//! workspace. `xtask sync-check`'s text scan cannot see it either — `for msg in rx` and
//! `for msg in slice` are the same shape.
//!
//! So the last one is closed by **construction** instead of by a lint. `Inbox` owns the receiver,
//! hands out `try_recv` and a non-blocking [`Inbox::drain`], and never lends the receiver out. There
//! is no expression you can write that blocks.

use std::sync::mpsc::{Sender, TryRecvError};

/// The receiving end of an mpsc channel that **cannot block**.
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
#[derive(Debug)]
pub struct Inbox<T> {
    // The one place in the workspace a bare `Receiver` is allowed to live. It is private and never
    // handed out, which is what makes the guarantee above hold.
    #[expect(
        clippy::disallowed_types,
        reason = "Inbox is the wrapper that makes the blocking half unreachable"
    )]
    rx: std::sync::mpsc::Receiver<T>,
}

impl<T> Inbox<T> {
    /// A channel: the ordinary [`Sender`], and the non-blocking receiving end.
    #[must_use]
    pub fn pair() -> (Sender<T>, Self) {
        let (tx, rx) = std::sync::mpsc::channel();
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

    /// Everything that has arrived **so far**, in order. Ends the moment the queue is empty rather
    /// than waiting for the next message, which is what separates it from `Receiver::iter`.
    pub fn drain(&self) -> impl Iterator<Item = T> + '_ {
        std::iter::from_fn(|| self.try_recv().ok())
    }
}

#[cfg(test)]
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
