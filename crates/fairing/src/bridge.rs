//! Optional transport contracts between UI code and application-owned logic.
//!
//! Implement these traits for the transport your application chooses: ownership-moving
//! channels, shared buffers, lock-free queues, TCP adapters or another mechanism.
//! Fairing does not create a transport, spawn a thread or install a recovery policy.
//!
//! The two directions are independent. Message and error types belong to the adapter.
//! There are no `Clone`, `Send`, `Sync` or `'static` bounds. A message may therefore be
//! an owned buffer, a shared handle or a borrow whose lifetime the caller controls.
//! Moving a buffer through this interface does not require copying its contents;
//! allocation, serialization and progress guarantees depend on the implementation.
//!
//! UI callers should use adapters that return promptly instead of waiting for capacity,
//! a message, a lock or network progress. A TCP implementation can expose an application-
//! managed outbox and receive buffer. Thread placement, wakeups, capacities, batching,
//! reconnects and retries remain application decisions.
//!
//! # Using an application-supplied adapter
//!
//! A screen can capture its adapter or access it through [`crate::Cx::app_mut`].
//! This interface does not replace [`crate::ShellHandle`]'s internal command transport.
//!
//! ```
//! use fairing::bridge::{CommandSink, UpdateSource};
//!
//! fn submit<P: CommandSink<Vec<u8>>>(
//!     port: &mut P,
//!     buffer: Vec<u8>,
//! ) -> Result<(), (Vec<u8>, P::Error)> {
//!     port.try_send(buffer)
//! }
//!
//! fn next<P: UpdateSource<Vec<u8>>>(
//!     port: &mut P,
//! ) -> Result<Option<Vec<u8>>, P::Error> {
//!     port.try_recv()
//! }
//! ```
//!
//! These are integration contracts, not a security sandbox. They do not prevent
//! application callbacks from using other captured resources.

/// Submit application-defined commands through a caller-selected transport.
///
/// Implementations used by UI code must return without waiting for transport progress.
/// Neither this trait nor Rust's type system enforces lock freedom or execution time.
/// It is usable through a trait object with a specified message and error type.
pub trait CommandSink<Command> {
    /// Adapter-defined admission or transport error.
    type Error;

    /// Try to submit one command, moving its value into the adapter on success.
    ///
    /// `Ok(())` reports the adapter's acceptance, not business execution or remote
    /// acknowledgement. Define those protocol results in application message types.
    ///
    /// # Errors
    ///
    /// Return the original command together with the adapter's error. Returning
    /// ownership permits refusal handling without a `Clone` bound. It does not prove
    /// that a remote peer observed nothing; delivery ambiguity and retry safety are
    /// part of the adapter's protocol contract.
    fn try_send(&mut self, command: Command) -> Result<(), (Command, Self::Error)>;
}

/// Receive application-defined updates through a caller-selected transport.
///
/// This direction does not require a command sender. The caller decides how many
/// updates to attempt in one UI pass; no draining loop is installed by the library.
pub trait UpdateSource<Update> {
    /// Adapter-defined receive or connection error.
    type Error;

    /// Try to receive one update without waiting for the next one.
    ///
    /// `Ok(None)` means that no update is currently available, not that the
    /// connection is permanently closed. `Ok(Some(update))` moves the adapter's
    /// update value to the caller without requiring it to implement `Clone`.
    ///
    /// # Errors
    ///
    /// The adapter reports connection closure and other receive failures using its
    /// own error type. Whether a particular error is terminal belongs to that contract.
    fn try_recv(&mut self) -> Result<Option<Update>, Self::Error>;
}
