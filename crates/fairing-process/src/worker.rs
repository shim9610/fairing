//! Renderer endpoint. Bootstrap runs before egui initialization; runtime I/O is off the UI thread.
use crate::{
    channel, notify, wake_pair,
    wire::{self, Decoder, Encoder, Kind, Packet},
    Error, Receiver, Sender, MAX_ACTION_BYTES, MAX_FRAME_ACTIONS, MAX_SNAPSHOT_BYTES,
};
use std::{
    os::{fd::AsFd, unix::net::UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

/// Opaque model identity. It must accompany intents derived from that model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderToken {
    generation: u64,
    epoch: u64,
    sequence: u64,
}
impl RenderToken {
    /// Owning renderer generation.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
    /// Work-engine authority context.
    #[must_use]
    pub const fn context(self) -> u64 {
        self.epoch
    }
    /// Read-only model sequence.
    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }
}
/// Read-only bounded model. Apply it before constructing a frame with its token.
pub struct Snapshot {
    token: RenderToken,
    len: usize,
    data: [u8; MAX_SNAPSHOT_BYTES],
}
impl Snapshot {
    /// Uninterpreted model bytes; the renderer must validate its application schema.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.data.get(..self.len).unwrap_or_default()
    }
    /// Identity to use only after this snapshot has been applied.
    #[must_use]
    pub const fn token(&self) -> RenderToken {
        self.token
    }
}
impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("token", &self.token)
            .field("length", &self.len)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy)]
struct Intent {
    len: usize,
    data: [u8; MAX_ACTION_BYTES],
}
/// Bounded intents staged during a UI pass. Commit only after the pass returns.
/// This receipt means CPU-side model/UI completion, never compositor presentation.
pub struct Frame {
    token: RenderToken,
    len: usize,
    actions: [Intent; MAX_FRAME_ACTIONS],
}
impl Frame {
    fn new(token: RenderToken) -> Self {
        Self {
            token,
            len: 0,
            actions: [Intent {
                len: 0,
                data: [0; MAX_ACTION_BYTES],
            }; MAX_FRAME_ACTIONS],
        }
    }
    /// Stage one intent without allocation. A frame without an applied model cannot act.
    /// # Errors
    /// No model, oversized intent or the fixed per-frame count limit.
    pub fn try_action(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.token.sequence == 0 {
            return Err(Error::Stale);
        }
        if bytes.len() > MAX_ACTION_BYTES {
            return Err(Error::TooLarge);
        }
        let action = self.actions.get_mut(self.len).ok_or(Error::Full)?;
        action
            .data
            .get_mut(..bytes.len())
            .ok_or(Error::TooLarge)?
            .copy_from_slice(bytes);
        action.len = bytes.len();
        self.len += 1;
        Ok(())
    }
}
impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("token", &self.token)
            .field("actions", &self.len)
            .finish_non_exhaustive()
    }
}
/// Unaccepted frame ownership, returned without allocating an error wrapper.
pub struct RejectedFrame {
    kind: Error,
    frame: Frame,
}
impl RejectedFrame {
    /// Failure reason.
    #[must_use]
    pub const fn kind(&self) -> Error {
        self.kind
    }
    /// Recover for a caller-owned bounded retry; stale frames must be discarded.
    #[must_use]
    pub fn into_frame(self) -> Frame {
        self.frame
    }
}
impl std::fmt::Debug for RejectedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.kind, f)
    }
}
impl std::fmt::Display for RejectedFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.kind, f)
    }
}
impl std::error::Error for RejectedFrame {}
type Wake = Arc<dyn Fn() + Send + Sync>;
struct Shared {
    closed: AtomicBool,
}
struct CloseOnExit(Arc<Shared>);
impl Drop for CloseOnExit {
    fn drop(&mut self) {
        self.0.closed.store(true, Ordering::Release);
    }
}
/// Renderer-side non-waiting API. stdout is reserved exclusively for framed IPC.
/// Core device state and authorization must stay in the host process.
pub struct Worker {
    generation: u64,
    epoch: u64,
    incoming: Receiver<Snapshot>,
    outgoing: Sender<Frame>,
    hooks: Sender<Wake>,
    wake: UnixStream,
    shared: Arc<Shared>,
}
impl Worker {
    /// Receive the bootstrap permit before creating egui or parsing application data.
    /// This is the one startup wait; runtime I/O uses a separate thread.
    /// # Errors
    /// Wrong role, malformed bootstrap, resource setup or thread/socket failure.
    pub fn connect() -> Result<Self, Error> {
        if std::env::var_os("FAIRING_RENDERER_WORKER").as_deref() != Some(std::ffi::OsStr::new("1"))
        {
            return Err(Error::Config);
        }
        let descriptor = std::io::stdin()
            .as_fd()
            .try_clone_to_owned()
            .map_err(|_| Error::Io)?;
        let mut socket = UnixStream::from(descriptor);
        // OwnedFd conversion is safe; a non-socket stdin fails normal stream operations.
        let mut decoder = Decoder::new();
        let start = decoder.read(&mut socket)?.ok_or(Error::Protocol)?;
        if start.kind != Kind::Start || start.sequence != 0 || start.len != 0 {
            return Err(Error::Protocol);
        }
        rustix::process::set_parent_process_death_signal(Some(rustix::process::Signal::KILL))
            .map_err(|_| Error::Io)?;
        socket.set_nonblocking(true).map_err(|_| Error::Io)?;
        let (snapshots, incoming) = channel(1);
        let (outgoing, frames) = channel(4);
        let (hooks, hook_rx) = channel(1);
        let (wake_read, wake) = wake_pair()?;
        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
        });
        let io_shared = Arc::clone(&shared);
        let generation = start.generation;
        let epoch = start.epoch;
        std::thread::Builder::new()
            .name("fairing-render-io".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || {
                worker_loop(
                    socket,
                    wake_read,
                    frames,
                    snapshots,
                    hook_rx,
                    io_shared,
                    (generation, epoch),
                );
            })
            .map_err(|_| Error::Io)?;
        Ok(Self {
            generation,
            epoch,
            incoming,
            outgoing,
            hooks,
            wake,
            shared,
        })
    }
    /// Set an egui repaint wake-up hook after context creation. Called on the I/O thread.
    /// # Errors
    /// Hook mailbox full, transport closed, or hook allocation failed outside this method.
    pub fn set_waker(&self, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(), Error> {
        self.hooks.try_send(wake).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(_) => Error::Full,
            std::sync::mpsc::TrySendError::Disconnected(_) => Error::Disconnected,
        })?;
        notify(&self.wake);
        Ok(())
    }
    /// Receive a prepared model immediately; never waits for the host.
    pub fn try_snapshot(&mut self) -> Option<Snapshot> {
        self.incoming.try_recv().ok()
    }
    /// Stage a UI pass. With no model, readiness may be reported but intents are refused.
    /// # Errors
    /// Closed connection or a token from another renderer/context.
    pub fn frame(&self, token: Option<RenderToken>) -> Result<Frame, Error> {
        if self.is_closed() {
            return Err(Error::Disconnected);
        }
        let token = token.unwrap_or(RenderToken {
            generation: self.generation,
            epoch: self.epoch,
            sequence: 0,
        });
        if token.generation != self.generation || token.epoch != self.epoch {
            return Err(Error::Stale);
        }
        Ok(Frame::new(token))
    }
    /// Commit staged intents after successful UI/model processing. Full preserves ownership.
    /// # Errors
    /// Full/closed transport or stale token. Acceptance is not work-engine authorization.
    #[allow(
        clippy::result_large_err,
        reason = "preserve bounded frame ownership inline without allocating during refusal"
    )]
    pub fn try_commit(&mut self, frame: Frame) -> Result<(), RejectedFrame> {
        if self.is_closed() {
            return Err(RejectedFrame {
                kind: Error::Disconnected,
                frame,
            });
        }
        if frame.token.generation != self.generation || frame.token.epoch != self.epoch {
            return Err(RejectedFrame {
                kind: Error::Stale,
                frame,
            });
        }
        self.outgoing.try_send(frame).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(frame) => RejectedFrame {
                kind: Error::Full,
                frame,
            },
            std::sync::mpsc::TrySendError::Disconnected(frame) => RejectedFrame {
                kind: Error::Disconnected,
                frame,
            },
        })?;
        notify(&self.wake);
        Ok(())
    }
    /// Connection state, sampled without I/O.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }
    /// Owning process generation.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }
    /// Owning work-engine authority context.
    #[must_use]
    pub const fn context(&self) -> u64 {
        self.epoch
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        notify(&self.wake);
    }
}
struct Batch {
    frame: Frame,
    index: usize,
}
impl Batch {
    fn next(&mut self, serial: &mut u64) -> Result<Option<Packet>, Error> {
        if self.index == 0 {
            self.index = 1;
            return Ok(Some(Packet::empty(
                Kind::Ready,
                self.frame.token.generation,
                self.frame.token.epoch,
                self.frame.token.sequence,
            )));
        }
        let action_index = self.index - 1;
        if action_index >= self.frame.len {
            return Ok(None);
        }
        let action = self
            .frame
            .actions
            .get(action_index)
            .ok_or(Error::Protocol)?;
        let next = serial.checked_add(1).ok_or(Error::Exhausted)?;
        let mut packet = Packet::empty(
            Kind::Action,
            self.frame.token.generation,
            self.frame.token.epoch,
            next,
        );
        let prefix = self.frame.token.sequence.to_le_bytes();
        packet
            .data
            .get_mut(..8)
            .ok_or(Error::Protocol)?
            .copy_from_slice(&prefix);
        packet
            .data
            .get_mut(8..8 + action.len)
            .ok_or(Error::Protocol)?
            .copy_from_slice(action.data.get(..action.len).ok_or(Error::Protocol)?);
        packet.len = 8 + action.len;
        *serial = next;
        self.index += 1;
        Ok(Some(packet))
    }
}
#[allow(
    clippy::needless_pass_by_value,
    reason = "the I/O thread owns channels for its entire lifetime"
)]
fn worker_loop(
    mut socket: UnixStream,
    mut wake: UnixStream,
    frames: Receiver<Frame>,
    snapshots: Sender<Snapshot>,
    hooks: Receiver<Wake>,
    shared: Arc<Shared>,
    identity: (u64, u64),
) {
    let (generation, epoch) = identity;
    let _close = CloseOnExit(Arc::clone(&shared));
    let mut hook: Option<Wake> = None;
    let mut run = || -> Result<(), Error> {
        let mut decoder = Decoder::new();
        let mut encoder = Some(Encoder::new(Packet::empty(
            Kind::Hello,
            generation,
            epoch,
            0,
        ))?);
        let mut batch: Option<Batch> = None;
        let mut serial = 0;
        let mut last_snapshot = 0;
        while !shared.closed.load(Ordering::Acquire) {
            wire::drain_wake(&mut wake);
            if let Ok(next) = hooks.try_recv() {
                hook = Some(next);
                if let Some(wake) = &hook {
                    wake();
                }
            }
            if let Some(writing) = encoder.as_mut() {
                if writing.write(&mut socket)? {
                    encoder = None;
                }
            }
            let mut completed_batch = false;
            if encoder.is_none() {
                if batch.is_none() {
                    batch = frames
                        .try_recv()
                        .ok()
                        .map(|frame| Batch { frame, index: 0 });
                }
                if let Some(current) = batch.as_mut() {
                    if let Some(packet) = current.next(&mut serial)? {
                        encoder = Some(Encoder::new(packet)?);
                    } else {
                        batch = None;
                        completed_batch = true;
                    }
                }
            }
            for _ in 0..16 {
                let Some(packet) = decoder.read(&mut socket)? else {
                    break;
                };
                if packet.kind != Kind::Snapshot
                    || packet.generation != generation
                    || packet.epoch != epoch
                    || packet.sequence <= last_snapshot
                {
                    return Err(Error::Protocol);
                }
                last_snapshot = packet.sequence;
                let snapshot = Snapshot {
                    token: RenderToken {
                        generation,
                        epoch,
                        sequence: packet.sequence,
                    },
                    len: packet.len,
                    data: packet.data,
                };
                snapshots.try_send(snapshot).map_err(|_| Error::Protocol)?;
                if let Some(wake) = &hook {
                    wake();
                }
            }
            // An already staged batch must not wait for another external wake to be spooled.
            if encoder.is_none() && (batch.is_some() || completed_batch) {
                continue;
            }
            wire::wait(
                Some(&socket),
                &wake,
                encoder.is_some(),
                Duration::from_secs(60),
            )?;
        }
        Ok(())
    };
    let _ = run();
    shared.closed.store(true, Ordering::Release);
    if let Some(wake) = hook {
        wake();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;
    #[test]
    fn intents_require_a_model_and_refusal_preserves_staged_data() {
        let token = RenderToken {
            generation: 1,
            epoch: 1,
            sequence: 0,
        };
        assert_eq!(Frame::new(token).try_action(b"act"), Err(Error::Stale));
        let token = RenderToken {
            sequence: 5,
            ..token
        };
        let mut frame = Frame::new(token);
        assert_eq!(
            frame.try_action(&[0; MAX_ACTION_BYTES + 1]),
            Err(Error::TooLarge)
        );
        for _ in 0..MAX_FRAME_ACTIONS {
            frame.try_action(b"patient-secret").unwrap();
        }
        assert_eq!(frame.try_action(b"overflow"), Err(Error::Full));
        assert_eq!(frame.len, MAX_FRAME_ACTIONS);
        assert!(!format!("{frame:?}").contains("secret"));
        let rejected = RejectedFrame {
            kind: Error::Full,
            frame,
        };
        assert!(!format!("{rejected:?}").contains("secret"));
        assert_eq!(rejected.into_frame().actions[0].len, 14);
    }
    #[test]
    fn a_completed_pass_precedes_its_intents_and_serials_never_wrap() {
        let token = RenderToken {
            generation: 3,
            epoch: 4,
            sequence: 5,
        };
        let mut frame = Frame::new(token);
        frame.try_action(b"A").unwrap();
        frame.try_action(b"B").unwrap();
        let mut batch = Batch { frame, index: 0 };
        let mut serial = u64::MAX - 1;
        let ready = batch.next(&mut serial).unwrap().unwrap();
        assert_eq!(ready.kind, Kind::Ready);
        assert_eq!(ready.sequence, 5);
        let first = batch.next(&mut serial).unwrap().unwrap();
        assert_eq!(first.kind, Kind::Action);
        assert_eq!(first.sequence, u64::MAX);
        assert_eq!(&first.data[..9], &[5, 0, 0, 0, 0, 0, 0, 0, b'A']);
        assert!(matches!(batch.next(&mut serial), Err(Error::Exhausted)));
        assert_eq!(serial, u64::MAX);
    }
    #[test]
    fn multiple_committed_frames_need_no_second_external_wake() {
        let (parent, child) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        child.set_nonblocking(true).unwrap();
        let (wake_read, wake) = wake_pair().unwrap();
        let (tx, frames) = channel(4);
        let (snapshots, _rx) = channel(1);
        let (_hooks, hook_rx) = channel(1);
        let shared = Arc::new(Shared {
            closed: AtomicBool::new(false),
        });
        for seq in 0..4 {
            tx.try_send(Frame::new(RenderToken {
                generation: 1,
                epoch: 1,
                sequence: seq,
            }))
            .unwrap();
        }
        let io_shared = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            worker_loop(
                child,
                wake_read,
                frames,
                snapshots,
                hook_rx,
                io_shared,
                (1, 1),
            );
        });
        notify(&wake);
        let mut parent = parent;
        let mut decoder = Decoder::new();
        assert_eq!(
            decoder.read(&mut parent).unwrap().unwrap().kind,
            Kind::Hello
        );
        for seq in 0..4 {
            let packet = decoder.read(&mut parent).unwrap().unwrap();
            assert_eq!(packet.kind, Kind::Ready);
            assert_eq!(packet.sequence, seq);
        }
        shared.closed.store(true, Ordering::Release);
        notify(&wake);
        thread.join().unwrap();
    }
}
