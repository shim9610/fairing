//! Work-engine endpoint. Only the supervisor thread owns process and stream I/O.
use crate::{
    channel, notify, wake_pair,
    wire::{self, Decoder, Encoder, Kind, Packet},
    Action, Error, Receiver, Sender, MAX_ACTION_BYTES, MAX_SNAPSHOT_BYTES,
};
use std::{
    ffi::OsString,
    os::unix::{net::UnixStream, process::ExitStatusExt},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

/// Trusted renderer executable and bounded startup arguments; no shell is invoked.
pub struct WorkerCommand {
    program: PathBuf,
    args: Vec<OsString>,
    inherit_stderr: bool,
}
impl WorkerCommand {
    /// An application-owned worker executable. Its entry point must call `Worker::connect` first.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            inherit_stderr: false,
        }
    }
    /// Add a trusted startup argument (at most 16, each at most 1024 bytes).
    /// # Errors
    /// Bounds exceeded or startup allocation failed.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Result<Self, Error> {
        let arg = arg.into();
        if self.args.len() >= 16 || arg.len() > 1024 {
            return Err(Error::TooLarge);
        }
        self.args.try_reserve(1).map_err(|_| Error::Io)?;
        self.args.push(arg);
        Ok(self)
    }
    /// Forward worker diagnostics to the host's stderr. Disabled by default.
    #[must_use]
    pub fn inherit_stderr(mut self) -> Self {
        self.inherit_stderr = true;
        self
    }
}
impl std::fmt::Debug for WorkerCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerCommand")
            .field("argument_count", &self.args.len())
            .finish_non_exhaustive()
    }
}
/// Renderer bounds. The address-space limit is virtual memory, not a GPU-memory limit.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// `RLIMIT_AS` for this child only; applied before the bootstrap permit is sent.
    pub address_space_bytes: u64,
    /// Time allowed for initial UI/model readiness.
    pub startup_timeout: Duration,
    /// Time allowed for an in-flight snapshot to be applied.
    pub snapshot_timeout: Duration,
    /// Minimum delay between process starts; failure backoff doubles up to two seconds.
    pub restart_delay: Duration,
    /// Automatic retries after failures. Exhaustion quarantines this host until recreated.
    pub max_restarts: u32,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            address_space_bytes: 256 * 1024 * 1024,
            startup_timeout: Duration::from_secs(5),
            snapshot_timeout: Duration::from_secs(2),
            restart_delay: Duration::from_millis(100),
            max_restarts: 3,
        }
    }
}
impl Config {
    fn validate(self) -> Result<(), Error> {
        if self.address_space_bytes < 16 * 1024 * 1024
            || self.address_space_bytes > 1024 * 1024 * 1024 * 1024
            || self.startup_timeout < Duration::from_millis(10)
            || self.startup_timeout > Duration::from_secs(60)
            || self.snapshot_timeout < Duration::from_millis(10)
            || self.snapshot_timeout > Duration::from_secs(60)
            || self.restart_delay < Duration::from_millis(10)
            || self.restart_delay > Duration::from_secs(2)
            || self.max_restarts > 32
        {
            return Err(Error::Config);
        }
        Ok(())
    }
}
/// Individually sampled renderer state; device work remains owned by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum State {
    /// Waiting for the first/current-context snapshot.
    Waiting = 0,
    /// Starting and awaiting readiness.
    Starting = 1,
    /// CPU-side model/UI readiness acknowledged; not a presentation guarantee.
    Running = 2,
    /// Waiting out bounded restart backoff.
    Backoff = 3,
    /// Previous child is being stopped/reaped.
    Stopping = 4,
    /// Failure retry budget exhausted.
    Quarantined = 5,
    /// Supervisor stopped, or this endpoint exhausted its identity counters.
    Stopped = 6,
}
/// Last failure class. Payloads and OS diagnostic text are not logged by this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Fault {
    /// No failure recorded.
    None = 0,
    /// Executable/process startup failed.
    Spawn = 1,
    /// Child resource limits could not be established.
    Limits = 2,
    /// Peer disconnected or I/O failed.
    Transport = 3,
    /// Malformed or out-of-order protocol.
    Protocol = 4,
    /// Child exited.
    Exit = 5,
    /// Startup or snapshot deadline expired.
    Timeout = 6,
    /// Work engine did not drain its bounded action queue.
    ActionOverflow = 7,
    /// A monotonic counter was exhausted.
    Exhausted = 8,
}
struct Shared {
    state: AtomicU8,
    fault: AtomicU8,
    generation: AtomicU64,
    epoch: AtomicU64,
    starts: AtomicU64,
    failures: AtomicU32,
    rejected: AtomicU64,
    last_ready: AtomicU64,
    pid: AtomicU32,
    exit_signal: AtomicI32,
    stop: AtomicBool,
}
impl Shared {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(State::Waiting as u8),
            fault: AtomicU8::new(0),
            generation: AtomicU64::new(0),
            epoch: AtomicU64::new(1),
            starts: AtomicU64::new(0),
            failures: AtomicU32::new(0),
            rejected: AtomicU64::new(0),
            last_ready: AtomicU64::new(0),
            pid: AtomicU32::new(0),
            exit_signal: AtomicI32::new(0),
            stop: AtomicBool::new(false),
        }
    }
    fn state(&self, state: State) {
        self.state.store(state as u8, Ordering::Release);
    }
}
/// Read-only counters, sampled separately rather than as an atomic snapshot.
#[derive(Debug, Clone, Copy)]
pub struct Stats {
    /// Current lifecycle state.
    pub state: State,
    /// Most recent failure.
    pub last_fault: Fault,
    /// Renderer generation, never reused within this work-engine process lifetime.
    pub generation: u64,
    /// Work-engine authority context.
    pub context: u64,
    /// Number of process starts.
    pub starts: u64,
    /// Number of observed failures.
    pub failures: u32,
    /// Stale actions refused by transport/context checks.
    pub rejected_actions: u64,
    /// Last applied snapshot sequence, not a GPU presentation receipt.
    pub last_ready_sequence: u64,
    /// Child PID if one is owned.
    pub child_pid: u32,
    /// Most recent child termination signal, or zero for normal exit/no observation.
    pub last_exit_signal: i32,
}
/// Work-engine handle. Publishing and receiving never wait for renderer I/O or restart.
/// Context invalidation stops the old worker and requires a fresh snapshot; it also
/// rejects already queued intents from the old authority context.
pub struct Host {
    tx: Sender<Packet>,
    rx: Receiver<Action>,
    wake: UnixStream,
    shared: Arc<Shared>,
    next_sequence: u64,
    epoch: u64,
}
impl Host {
    fn exhausted(&self) -> Error {
        self.shared
            .fault
            .store(Fault::Exhausted as u8, Ordering::Release);
        self.shared.stop.store(true, Ordering::Release);
        self.shared.state(State::Stopped);
        notify(&self.wake);
        Error::Exhausted
    }
    /// Start a supervisor thread; a renderer is launched only after the first snapshot.
    /// # Errors
    /// Invalid bounds, socket/thread startup failure.
    pub fn new(command: WorkerCommand, config: Config) -> Result<Self, Error> {
        config.validate()?;
        let (tx, commands) = channel(1);
        let (events, rx) = channel(16);
        let (wake_read, wake) = wake_pair()?;
        let shared = Arc::new(Shared::new());
        let actor_shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("fairing-render-host".into())
            .stack_size(2 * 1024 * 1024)
            .spawn(move || actor(command, config, commands, events, wake_read, actor_shared))
            .map_err(|_| Error::Io)?;
        Ok(Self {
            tx,
            rx,
            wake,
            shared,
            next_sequence: 0,
            epoch: 1,
        })
    }
    /// Queue a bounded read-only model. Snapshots may coalesce; they are not device commands.
    /// # Errors
    /// Too large, full mailbox, closed/quarantined host or sequence exhaustion.
    pub fn try_publish(&mut self, data: &[u8]) -> Result<u64, Error> {
        if data.len() > MAX_SNAPSHOT_BYTES {
            return Err(Error::TooLarge);
        }
        if self.shared.stop.load(Ordering::Acquire)
            || matches!(self.stats().state, State::Stopped | State::Quarantined)
        {
            return Err(Error::Disconnected);
        }
        let sequence = self
            .next_sequence
            .checked_add(1)
            .ok_or_else(|| self.exhausted())?;
        let mut packet = Packet::empty(Kind::Snapshot, 0, self.epoch, sequence);
        packet.set(data)?;
        self.tx.try_send(packet).map_err(|error| match error {
            std::sync::mpsc::TrySendError::Full(_) => Error::Full,
            std::sync::mpsc::TrySendError::Disconnected(_) => Error::Disconnected,
        })?;
        self.next_sequence = sequence;
        notify(&self.wake);
        Ok(sequence)
    }
    /// Invalidate session/sample authority before processing more UI intents.
    /// The old renderer is stopped; publish a fresh current-context model to restart it.
    /// # Errors
    /// Authority counter exhausted: the endpoint stops rather than recycling an identity.
    pub fn invalidate_context(&mut self) -> Result<u64, Error> {
        let epoch = self.epoch.checked_add(1).ok_or_else(|| self.exhausted())?;
        self.epoch = epoch;
        self.shared.state(State::Stopping);
        self.shared.epoch.store(epoch, Ordering::Release);
        notify(&self.wake);
        Ok(epoch)
    }
    /// Retrieve one current-context intent. The caller must independently authorize it.
    /// Stale queued intents are discarded, with at most the fixed queue capacity inspected.
    pub fn try_action(&mut self) -> Option<Action> {
        if self.shared.stop.load(Ordering::Acquire) {
            return None;
        }
        for _ in 0..16 {
            let action = self.rx.try_recv().ok()?;
            let stats = self.stats();
            if stats.state == State::Running
                && action.generation == stats.generation
                && action.epoch == self.epoch
                && self.shared.state.load(Ordering::Acquire) == State::Running as u8
                && self.shared.generation.load(Ordering::Acquire) == action.generation
            {
                return Some(action);
            }
            let _ = self.shared.rejected.fetch_add(1, Ordering::Relaxed);
        }
        None
    }
    /// Read individually sampled counters without I/O or payload access.
    #[must_use]
    pub fn stats(&self) -> Stats {
        let state = match self.shared.state.load(Ordering::Acquire) {
            0 => State::Waiting,
            1 => State::Starting,
            2 => State::Running,
            3 => State::Backoff,
            4 => State::Stopping,
            5 => State::Quarantined,
            _ => State::Stopped,
        };
        let fault = match self.shared.fault.load(Ordering::Relaxed) {
            0 => Fault::None,
            1 => Fault::Spawn,
            2 => Fault::Limits,
            3 => Fault::Transport,
            4 => Fault::Protocol,
            5 => Fault::Exit,
            6 => Fault::Timeout,
            7 => Fault::ActionOverflow,
            _ => Fault::Exhausted,
        };
        Stats {
            state,
            last_fault: fault,
            generation: self.shared.generation.load(Ordering::Acquire),
            context: self.shared.epoch.load(Ordering::Acquire),
            starts: self.shared.starts.load(Ordering::Relaxed),
            failures: self.shared.failures.load(Ordering::Relaxed),
            rejected_actions: self.shared.rejected.load(Ordering::Relaxed),
            last_ready_sequence: self.shared.last_ready.load(Ordering::Acquire),
            child_pid: self.shared.pid.load(Ordering::Relaxed),
            last_exit_signal: self.shared.exit_signal.load(Ordering::Relaxed),
        }
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        notify(&self.wake);
    }
}
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
fn generation() -> Result<u64, Error> {
    let mut current = NEXT_GENERATION.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1).ok_or(Error::Exhausted)?;
        match NEXT_GENERATION.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return Ok(current),
            Err(actual) => current = actual,
        }
    }
}
struct Running {
    child: Child,
    socket: UnixStream,
    decoder: Decoder,
    encoder: Option<Encoder>,
    generation: u64,
    epoch: u64,
    hello: bool,
    ready: bool,
    start_deadline: Instant,
    inflight: Option<(u64, Instant)>,
    last_sent: u64,
    last_ready: u64,
    last_action: u64,
    reaped: bool,
}
impl Running {
    fn spawn(command: &WorkerCommand, config: Config, epoch: u64) -> Result<Self, Fault> {
        let generation = generation().map_err(|_| Fault::Exhausted)?;
        let start_deadline = Instant::now()
            .checked_add(config.startup_timeout)
            .ok_or(Fault::Timeout)?;
        let encoder = Some(
            Encoder::new(Packet::empty(Kind::Start, generation, epoch, 0))
                .map_err(|_| Fault::Protocol)?,
        );
        let (parent, child_socket) = UnixStream::pair().map_err(|_| Fault::Spawn)?;
        let child_input = child_socket.try_clone().map_err(|_| Fault::Spawn)?;
        let mut launch = Command::new(&command.program);
        launch
            .args(&command.args)
            .env_clear()
            .env("FAIRING_RENDERER_WORKER", "1");
        // Explicit GUI/locale variables only. Do not inherit backend/cloud credentials by default.
        for key in [
            "DISPLAY",
            "XAUTHORITY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "XDG_SESSION_TYPE",
            "LANG",
            "LC_ALL",
            "HOME",
            "LIBGL_ALWAYS_SOFTWARE",
            "LP_NUM_THREADS",
        ] {
            if let Some(value) = std::env::var_os(key) {
                launch.env(key, value);
            }
        }
        launch
            .stdin(Stdio::from(std::os::fd::OwnedFd::from(child_input)))
            .stdout(Stdio::from(std::os::fd::OwnedFd::from(child_socket)))
            .stderr(if command.inherit_stderr {
                Stdio::inherit()
            } else {
                Stdio::null()
            });
        let mut child = launch.spawn().map_err(|_| Fault::Spawn)?;
        drop(launch);
        let pid = i32::try_from(child.id())
            .ok()
            .and_then(rustix::process::Pid::from_raw);
        let Some(pid) = pid else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Fault::Limits);
        };
        let limited = rustix::process::prlimit(
            Some(pid),
            rustix::process::Resource::As,
            rustix::process::Rlimit {
                current: Some(config.address_space_bytes),
                maximum: Some(config.address_space_bytes),
            },
        )
        .and_then(|_| {
            rustix::process::prlimit(
                Some(pid),
                rustix::process::Resource::Core,
                rustix::process::Rlimit {
                    current: Some(0),
                    maximum: Some(0),
                },
            )
        });
        if limited.is_err() || parent.set_nonblocking(true).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Fault::Limits);
        }
        Ok(Self {
            child,
            socket: parent,
            decoder: Decoder::new(),
            encoder,
            generation,
            epoch,
            hello: false,
            ready: false,
            start_deadline,
            inflight: None,
            last_sent: 0,
            last_ready: 0,
            last_action: 0,
            reaped: false,
        })
    }
    fn expired(&self, now: Instant) -> bool {
        (!self.ready && now >= self.start_deadline)
            || self.inflight.is_some_and(|(_, at)| now >= at)
    }
    #[allow(
        clippy::too_many_lines,
        reason = "one bounded protocol turn checks deadlines before every peer message"
    )]
    fn pump(
        &mut self,
        latest: Option<&Packet>,
        config: Config,
        shared: &Shared,
        events: &Sender<Action>,
    ) -> Result<(), Fault> {
        if self.expired(Instant::now()) {
            return Err(Fault::Timeout);
        }
        if let Some(encoder) = self.encoder.as_mut() {
            if encoder
                .write(&mut self.socket)
                .map_err(|_| Fault::Transport)?
            {
                self.encoder = None;
            }
        }
        for _ in 0..16 {
            let Some(packet) = self.decoder.read(&mut self.socket).map_err(|error| {
                if error == Error::Disconnected {
                    Fault::Exit
                } else {
                    Fault::Protocol
                }
            })?
            else {
                break;
            };
            if self.expired(Instant::now()) {
                return Err(Fault::Timeout);
            }
            if packet.generation != self.generation || packet.epoch != self.epoch {
                return Err(Fault::Protocol);
            }
            match packet.kind {
                Kind::Hello if !self.hello && packet.sequence == 0 && packet.len == 0 => {
                    self.hello = true;
                }
                Kind::Ready if self.hello && packet.len == 0 => {
                    if packet.sequence == 0 && self.last_sent == 0 {
                        self.ready = true;
                    } else if self.inflight.is_some_and(|(seq, _)| seq == packet.sequence) {
                        self.inflight = None;
                        self.last_ready = packet.sequence;
                        self.ready = true;
                        shared.last_ready.store(packet.sequence, Ordering::Release);
                    } else if packet.sequence != self.last_ready {
                        return Err(Fault::Protocol);
                    }
                    if self.ready {
                        shared.state(State::Running);
                    }
                }
                Kind::Action if self.ready => {
                    let expected = self.last_action.checked_add(1).ok_or(Fault::Exhausted)?;
                    if packet.sequence != expected
                        || packet.len < 8
                        || packet.len > MAX_ACTION_BYTES + 8
                    {
                        return Err(Fault::Protocol);
                    }
                    self.last_action = expected;
                    let frame = u64::from_le_bytes(
                        packet
                            .data
                            .get(..8)
                            .ok_or(Fault::Protocol)?
                            .try_into()
                            .map_err(|_| Fault::Protocol)?,
                    );
                    if frame == 0
                        || frame != self.last_ready
                        || shared.epoch.load(Ordering::Acquire) != self.epoch
                    {
                        let _ = shared.rejected.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    let payload = packet.data.get(8..packet.len).ok_or(Fault::Protocol)?;
                    let mut action = Action {
                        generation: self.generation,
                        epoch: self.epoch,
                        sequence: packet.sequence,
                        model_sequence: frame,
                        len: payload.len(),
                        data: [0; MAX_ACTION_BYTES],
                    };
                    action
                        .data
                        .get_mut(..payload.len())
                        .ok_or(Fault::Protocol)?
                        .copy_from_slice(payload);
                    events.try_send(action).map_err(|_| Fault::ActionOverflow)?;
                }
                _ => return Err(Fault::Protocol),
            }
        }
        if self.hello && self.encoder.is_none() && self.inflight.is_none() {
            if let Some(latest) =
                latest.filter(|p| p.epoch == self.epoch && p.sequence > self.last_sent)
            {
                let mut packet = latest.clone();
                packet.generation = self.generation;
                self.last_sent = packet.sequence;
                self.inflight = Some((
                    packet.sequence,
                    Instant::now()
                        .checked_add(config.snapshot_timeout)
                        .ok_or(Fault::Timeout)?,
                ));
                self.encoder = Some(Encoder::new(packet).map_err(|_| Fault::Protocol)?);
            }
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.reaped = true;
                shared
                    .exit_signal
                    .store(status.signal().unwrap_or(0), Ordering::Release);
                Err(Fault::Exit)
            }
            Ok(None) => Ok(()),
            Err(error) => {
                if error.raw_os_error() == Some(rustix::io::Errno::CHILD.raw_os_error()) {
                    // An external SIGCHLD policy already reaped it: never signal a recycled PID.
                    self.reaped = true;
                }
                Err(Fault::Exit)
            }
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
        }
    }
}
struct StopOnExit(Arc<Shared>);
impl Drop for StopOnExit {
    fn drop(&mut self) {
        self.0.state(State::Stopped);
        self.0.pid.store(0, Ordering::Release);
    }
}
fn retry_delay(config: Config, failures: u32) -> Duration {
    config
        .restart_delay
        .saturating_mul(
            1u32.checked_shl(failures.saturating_sub(1).min(5))
                .unwrap_or(32),
        )
        .min(Duration::from_secs(2))
}
#[allow(
    clippy::too_many_lines,
    clippy::needless_pass_by_value,
    reason = "the supervisor exclusively owns process lifecycle and transport resources"
)]
fn actor(
    command: WorkerCommand,
    config: Config,
    commands: Receiver<Packet>,
    events: Sender<Action>,
    mut wake: UnixStream,
    shared: Arc<Shared>,
) {
    let _stop = StopOnExit(Arc::clone(&shared));
    let mut running: Option<Running> = None;
    let mut retiring: Option<Running> = None;
    let mut latest: Option<Packet> = None;
    let mut epoch = shared.epoch.load(Ordering::Acquire);
    let mut ready_at = Instant::now();
    let mut failures = 0u32;
    let mut last_start: Option<Instant> = None;
    loop {
        wire::drain_wake(&mut wake);
        for _ in 0..2 {
            match commands.try_recv() {
                Ok(packet) if packet.epoch == shared.epoch.load(Ordering::Acquire) => {
                    latest = Some(packet);
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let now = Instant::now();
        let stopping = shared.stop.load(Ordering::Acquire);
        let next_epoch = shared.epoch.load(Ordering::Acquire);
        if next_epoch != epoch {
            epoch = next_epoch;
            latest = latest.filter(|p| p.epoch == epoch);
            shared.last_ready.store(0, Ordering::Release);
            if let Some(mut old) = running.take() {
                shared.state(State::Stopping);
                let _ = old.child.kill();
                retiring = Some(old);
            }
            ready_at = last_start
                .and_then(|at| at.checked_add(config.restart_delay))
                .unwrap_or(now);
        }
        if stopping {
            latest = None;
            if let Some(mut old) = running.take() {
                shared.state(State::Stopping);
                let _ = old.child.kill();
                retiring = Some(old);
            }
        }
        if let Some(old) = retiring.as_mut() {
            let reaped = if old.reaped {
                true
            } else {
                match old.child.try_wait() {
                    Ok(Some(status)) => {
                        shared
                            .exit_signal
                            .store(status.signal().unwrap_or(0), Ordering::Release);
                        true
                    }
                    Err(error)
                        if error.raw_os_error()
                            == Some(rustix::io::Errno::CHILD.raw_os_error()) =>
                    {
                        true
                    }
                    _ => false,
                }
            };
            if reaped {
                old.reaped = true;
                retiring = None;
                shared.pid.store(0, Ordering::Release);
            }
        }
        if stopping && retiring.is_none() {
            break;
        }
        if let Some(active) = running.as_mut() {
            if let Err(fault) = active.pump(latest.as_ref(), config, &shared, &events) {
                shared.state(State::Stopping);
                shared.fault.store(fault as u8, Ordering::Release);
                failures = failures.saturating_add(1);
                shared.failures.store(failures, Ordering::Release);
                let delay = retry_delay(config, failures);
                ready_at = Instant::now().checked_add(delay).unwrap_or(now);
                if let Some(mut old) = running.take() {
                    if !old.reaped {
                        let _ = old.child.kill();
                    }
                    retiring = Some(old);
                }
            }
        }
        if running.is_none() && retiring.is_none() && !stopping {
            if failures > config.max_restarts {
                shared.state(State::Quarantined);
            } else if latest.is_none() {
                shared.state(State::Waiting);
            } else if now < ready_at {
                shared.state(State::Backoff);
            } else {
                shared.state(State::Starting);
                match Running::spawn(&command, config, epoch) {
                    Ok(child) => {
                        shared.generation.store(child.generation, Ordering::Release);
                        shared.pid.store(child.child.id(), Ordering::Release);
                        shared.last_ready.store(0, Ordering::Release);
                        let _ = shared.starts.fetch_add(1, Ordering::Relaxed);
                        last_start = Some(Instant::now());
                        running = Some(child);
                    }
                    Err(fault) => {
                        shared.fault.store(fault as u8, Ordering::Release);
                        failures = failures.saturating_add(1);
                        shared.failures.store(failures, Ordering::Release);
                        ready_at = Instant::now()
                            .checked_add(retry_delay(config, failures))
                            .unwrap_or(now);
                    }
                }
            }
        }
        let mut delay = Duration::from_millis(250);
        if let Some(active) = running.as_ref() {
            if !active.ready {
                delay = delay.min(
                    active
                        .start_deadline
                        .saturating_duration_since(Instant::now()),
                );
            }
            if let Some((_, at)) = active.inflight {
                delay = delay.min(at.saturating_duration_since(Instant::now()));
            }
        } else if retiring.is_none() && latest.is_some() && failures <= config.max_restarts {
            delay = delay.min(ready_at.saturating_duration_since(Instant::now()));
        }
        let wait = wire::wait(
            running.as_ref().map(|r| &r.socket),
            &wake,
            running.as_ref().is_some_and(|r| r.encoder.is_some()),
            delay,
        );
        if wait.is_err() {
            shared
                .fault
                .store(Fault::Transport as u8, Ordering::Release);
            shared.stop.store(true, Ordering::Release);
        }
    }
    shared.state(State::Stopped);
    shared.pid.store(0, Ordering::Release);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    fn test_host() -> (Host, Sender<Action>) {
        let (tx, _commands) = channel(1);
        let (events, rx) = channel(16);
        let (_read, wake) = wake_pair().unwrap();
        let shared = Arc::new(Shared::new());
        shared.state(State::Running);
        shared.generation.store(10, Ordering::Release);
        (
            Host {
                tx,
                rx,
                wake,
                shared,
                next_sequence: 0,
                epoch: 1,
            },
            events,
        )
    }
    fn action(generation: u64, epoch: u64) -> Action {
        Action {
            generation,
            epoch,
            sequence: 1,
            model_sequence: 1,
            len: 6,
            data: [0; MAX_ACTION_BYTES],
        }
    }
    #[test]
    fn queued_actions_fail_closed_immediately_after_authority_change() {
        let (mut host, events) = test_host();
        for _ in 0..16 {
            events.try_send(action(10, 1)).unwrap();
        }
        host.invalidate_context().unwrap();
        assert!(host.try_action().is_none());
        assert_eq!(host.stats().rejected_actions, 16);
        events.try_send(action(10, 2)).unwrap();
        assert!(host.try_action().is_none());
        host.shared.generation.store(11, Ordering::Release);
        host.shared.state(State::Running);
        events.try_send(action(11, 2)).unwrap();
        assert!(host.try_action().is_some());
    }
    #[test]
    fn stopped_or_replaced_workers_cannot_release_queued_intents() {
        let (mut host, events) = test_host();
        events.try_send(action(9, 1)).unwrap();
        events.try_send(action(10, 1)).unwrap();
        assert_eq!(host.try_action().unwrap().generation(), 10);
        events.try_send(action(10, 1)).unwrap();
        host.shared.state(State::Stopping);
        assert!(host.try_action().is_none());
    }
    #[test]
    fn identity_exhaustion_and_invalid_limits_never_silently_relax_bounds() {
        let (mut host, _) = test_host();
        host.next_sequence = u64::MAX;
        assert_eq!(host.try_publish(b"data"), Err(Error::Exhausted));
        host.epoch = u64::MAX;
        assert_eq!(host.invalidate_context(), Err(Error::Exhausted));
        assert_eq!(host.stats().state, State::Stopped);
        assert_eq!(
            host.try_publish(b"after exhaustion"),
            Err(Error::Disconnected)
        );
        assert!(Config {
            address_space_bytes: u64::MAX,
            ..Config::default()
        }
        .validate()
        .is_err());
        assert!(Config {
            startup_timeout: Duration::ZERO,
            ..Config::default()
        }
        .validate()
        .is_err());
        assert!(Config {
            max_restarts: 33,
            ..Config::default()
        }
        .validate()
        .is_err());
    }
}
