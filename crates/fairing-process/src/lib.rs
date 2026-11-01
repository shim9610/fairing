#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(test),
    forbid(
        clippy::disallowed_methods,
        clippy::disallowed_macros,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::todo,
        clippy::unimplemented
    )
)]
#[cfg(not(target_os = "linux"))]
compile_error!("fairing-process currently supports Linux with process isolation");

mod host;
mod wire;
mod worker;

pub use host::{Config, Fault, Host, State, Stats, WorkerCommand};
pub use worker::{Frame, RejectedFrame, RenderToken, Snapshot, Worker};

/// Largest model snapshot, in bytes. Larger views should page/filter in the work engine.
pub const MAX_SNAPSHOT_BYTES: usize = 4096;
/// Largest individual UI action. Payloads are never interpreted as authorization.
pub const MAX_ACTION_BYTES: usize = 256;
/// Largest number of actions committed together after a UI pass returns.
pub const MAX_FRAME_ACTIONS: usize = 8;

/// Bounded, redacted API errors; reporting an error never allocates a wrapper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Bounded queue is full; no acceptance is implied.
    Full,
    /// Data exceeds a fixed protocol bound.
    TooLarge,
    /// Peer or background transport is unavailable.
    Disconnected,
    /// Renderer generation or authority context is no longer current.
    Stale,
    /// A counter cannot advance without reuse.
    Exhausted,
    /// Malformed protocol data.
    Protocol,
    /// The supplied configuration cannot establish the requested bounds.
    Config,
    /// Operating-system I/O failed.
    Io,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Full => "renderer queue full",
            Self::TooLarge => "renderer payload too large",
            Self::Disconnected => "renderer disconnected",
            Self::Stale => "stale renderer context",
            Self::Exhausted => "renderer counter exhausted",
            Self::Protocol => "invalid renderer protocol",
            Self::Config => "invalid renderer configuration",
            Self::Io => "renderer I/O failed",
        })
    }
}
impl std::error::Error for Error {}

/// A UI intent from the current process generation and authority context.
/// The work engine must still authorize and validate its meaning before acting.
pub struct Action {
    generation: u64,
    epoch: u64,
    sequence: u64,
    model_sequence: u64,
    len: usize,
    data: [u8; MAX_ACTION_BYTES],
}
impl Action {
    /// Bounded uninterpreted payload. Debug never prints it.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.data.get(..self.len).unwrap_or_default()
    }
    /// Generation that emitted this intent.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    /// Work-engine authority context echoed by the renderer.
    #[must_use]
    pub const fn context(&self) -> u64 {
        self.epoch
    }
    /// Monotonic action sequence within the generation.
    #[must_use]
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
    /// Snapshot the intent was derived from. Validate its business revision in the engine.
    #[must_use]
    pub const fn model_sequence(&self) -> u64 {
        self.model_sequence
    }
}
impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("generation", &self.generation)
            .field("context", &self.epoch)
            .field("sequence", &self.sequence)
            .field("model_sequence", &self.model_sequence)
            .field("length", &self.len)
            .finish_non_exhaustive()
    }
}

#[expect(
    clippy::disallowed_types,
    reason = "private bounded transports; public callers only use try APIs"
)]
type Sender<T> = std::sync::mpsc::SyncSender<T>; // sync-check: allow: private bounded transport, never capacity-waits
#[expect(
    clippy::disallowed_types,
    reason = "private receiver; no blocking receive API is exposed"
)]
type Receiver<T> = std::sync::mpsc::Receiver<T>;
fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    std::sync::mpsc::sync_channel(capacity) // sync-check: allow: fixed internal mailbox, only try_send is used
}
fn notify(wake: &std::os::unix::net::UnixStream) {
    use std::io::Write;
    let mut stream = wake;
    let _ = stream.write(&[1]); // Nonblocking stream; a full wake buffer already contains a wake-up.
}
fn wake_pair() -> Result<
    (
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    ),
    Error,
> {
    let (read, write) = std::os::unix::net::UnixStream::pair().map_err(|_| Error::Io)?;
    read.set_nonblocking(true).map_err(|_| Error::Io)?;
    write.set_nonblocking(true).map_err(|_| Error::Io)?;
    Ok((read, write))
}
