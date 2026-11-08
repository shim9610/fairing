//! The `Mock*` simulator backends (feature `mock`). For the demos and the headless tests.
//!
//! **Nonblocking channel controls.** Each message-based backend hands out a [`MockControl`] (a nonblocking
//! sender) and a scenario thread or a test sends messages to it. Each connection defaults
//! to 1,024 slots and 256 messages per poll; custom inbox settings can select explicit
//! unbounded storage. The backend's `poll_at()` processes a finite batch of
//! them on the UI thread with `try_recv` and refreshes `latest`. The sending side wakes the UI
//! with [`Waker::wake`] after an accepted send; controls do not capture a waker.
//! Time-based simulation (scan/connect delays and battery draining) compares `Instant`s
//! inside `poll_at()` rather than sleeping, and announces the next
//! moment with `next_wake()`. The only time it ever reads is **the monotonic time the shell hands
//! to `poll_at(now)`** (`Instant::now()` is banned) — which makes headless virtual time the
//! logical time as it stands.

use super::{
    AudioBackend, AudioSnapshot, Backend, BatterySnapshot, BluetoothBackend, BtDevice, BtSnapshot,
    Capabilities, ClockSource, Cue, DeviceInfo, DisplayBackend, DisplayInfo, ErrorKind, IfaceKind,
    IfaceSnapshot, InfoBackend, IpConfig, Ipv4Net, Network, NetworkBackend, PowerBackend,
    PowerRequest, ServiceError, ServiceResult, Waker, WallTime, WifiBackend, WifiSnapshot,
    WifiState,
};
use crate::inbox::{
    ConfiguredInbox, ConfiguredSender, Inbox, InboxConfig, QueueCapacityError, TrySendError,
};
use std::fmt;
use std::net::{IpAddr, Ipv4Addr};
use std::time::{Duration, Instant};

/// The handle that sends messages to a mock backend. `Clone + Send`.
pub struct MockControl<M> {
    tx: ConfiguredSender<M>,
}

impl<M> fmt::Debug for MockControl<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MockControl").finish_non_exhaustive()
    }
}

impl<M> Clone for MockControl<M> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<M> MockControl<M> {
    /// Send without waiting. `false` when full or disconnected; use `try_send` to
    /// distinguish the reason and recover the message for a caller-selected policy.
    pub fn send(&self, message: M) -> bool {
        self.try_send(message).is_ok()
    }

    /// Send without waiting, preserving rejected message ownership.
    ///
    /// # Errors
    /// Full bounded storage or a disconnected backend.
    pub fn try_send(&self, message: M) -> Result<(), TrySendError<M>> {
        self.tx.try_send(message)
    }
}

struct MockInbox<M> {
    rx: ConfiguredInbox<M>,
    limit: usize,
    remaining: usize,
    now: Option<Instant>,
    pending: Option<Instant>,
}
impl<M> MockInbox<M> {
    fn new(rx: ConfiguredInbox<M>, limit: usize) -> Self {
        Self {
            rx,
            limit,
            remaining: 0,
            now: None,
            pending: None,
        }
    }
    fn start_poll(&mut self, now: Instant) -> usize {
        self.now = Some(now);
        self.pending = None;
        self.remaining = self.limit;
        self.limit
    }
    fn try_recv(&mut self) -> Result<M, std::sync::mpsc::TryRecvError> {
        if self.remaining == 0 {
            return Err(std::sync::mpsc::TryRecvError::Empty);
        }
        let message = self.rx.try_recv()?;
        self.remaining -= 1;
        if self.remaining == 0 {
            self.pending = self.now;
        }
        Ok(message)
    }
    fn set_limit(&mut self, limit: usize) {
        self.limit = limit;
        self.rx.set_batch_limit(limit);
        self.pending = if limit == 0 { None } else { self.now };
    }
    fn next_wake(&self, timed: Option<Instant>) -> Option<Instant> {
        match (self.pending, timed) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (pending, timed) => pending.or(timed),
        }
    }
}

fn default_channel<M>() -> (MockControl<M>, MockInbox<M>) {
    let (tx, rx) = Inbox::mock_default();
    (
        MockControl { tx },
        MockInbox::new(rx, InboxConfig::default().batch_limit),
    )
}
fn channel<M>(config: InboxConfig) -> Result<(MockControl<M>, MockInbox<M>), QueueCapacityError> {
    let (tx, rx) = Inbox::configured(config)?;
    Ok((MockControl { tx }, MockInbox::new(rx, config.batch_limit)))
}

/// The deadline of a mock operation that takes time. **It is fixed on the first `poll_at`** — so
/// that a `scan()` / `set_discovering()` called outside a frame does not reach for `Instant::now()`
/// (a backend sees only the monotonic time the shell hands it). Before it is fixed
/// `next_wake` is `None` too, and the first `poll_at` pins it at `now + delay`.
#[derive(Debug, Clone, Copy)]
struct Deadline {
    due: Option<Instant>,
    delay: Duration,
}

impl Deadline {
    /// With `now` already known, it is fixed on the spot.
    fn new(delay: Duration, now: Option<Instant>) -> Self {
        Self {
            due: now.map(|now| now + delay),
            delay,
        }
    }

    /// The deadline (called only inside `poll_at` — if it is not fixed, it is fixed here).
    fn resolve(&mut self, now: Instant) -> Instant {
        *self.due.get_or_insert(now + self.delay)
    }
}

/// [`MockClock`] messages.
#[derive(Debug, Clone, Copy)]
pub enum ClockMsg {
    /// Change the time.
    Set(WallTime),
}

/// The mock clock. `fixed` stands still; otherwise it follows the shell's time from the first `poll_at` on.
pub struct MockClock {
    base: WallTime,
    started: Option<Instant>,
    /// The time of the last `poll_at`.
    now: Option<Instant>,
    fixed: bool,
    rx: MockInbox<ClockMsg>,
    control: MockControl<ClockMsg>,
}

impl MockClock {
    /// A stopped clock (for tests).
    #[must_use]
    pub fn fixed(at: WallTime) -> Self {
        Self::new(at, true)
    }

    /// A running clock (for the demos).
    #[must_use]
    pub fn running(from: WallTime) -> Self {
        Self::new(from, false)
    }

    /// A fixed clock with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn fixed_with_inbox(at: WallTime, config: InboxConfig) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(at, true, channel(config)?))
    }

    /// A running clock with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn running_with_inbox(
        from: WallTime,
        config: InboxConfig,
    ) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(from, false, channel(config)?))
    }

    fn new(base: WallTime, fixed: bool) -> Self {
        Self::from_channel(base, fixed, default_channel())
    }

    fn from_channel(
        base: WallTime,
        fixed: bool,
        pair: (MockControl<ClockMsg>, MockInbox<ClockMsg>),
    ) -> Self {
        let (control, rx) = pair;
        Self {
            base,
            started: None,
            now: None,
            fixed,
            rx,
            control,
        }
    }

    /// Set the maximum messages processed by each poll. Zero pauses queue consumption;
    /// a positive limit resumes it without replacing controls or accepted messages.
    pub fn set_inbox_batch_limit(&mut self, limit: usize) {
        self.rx.set_limit(limit);
    }

    /// The control handle.
    #[must_use]
    pub fn control(&self) -> MockControl<ClockMsg> {
        self.control.clone()
    }
}

impl Backend for MockClock {
    fn poll_at(&mut self, now: Instant) {
        self.now = Some(now);
        self.started.get_or_insert(now);
        for _ in 0..self.rx.start_poll(now) {
            let Ok(msg) = self.rx.try_recv() else {
                break;
            };
            match msg {
                ClockMsg::Set(at) => {
                    self.base = at;
                    self.started = Some(now);
                }
            }
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        self.rx.next_wake(None)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::CLOCK_SET
    }
}

impl ClockSource for MockClock {
    fn now(&self) -> WallTime {
        if self.fixed {
            return self.base;
        }
        let elapsed = match (self.started, self.now) {
            (Some(started), Some(now)) => now.saturating_duration_since(started).as_secs(),
            _ => 0,
        };
        WallTime {
            utc_secs: self.base.utc_secs + elapsed,
            offset_min: self.base.offset_min,
        }
    }

    fn set_time(&mut self, utc_secs: u64) -> ServiceResult {
        self.base.utc_secs = utc_secs;
        self.started = self.now;
        Ok(())
    }
}

/// [`MockPower`] messages.
#[derive(Debug, Clone, Copy)]
pub enum PowerMsg {
    /// Change the battery state.
    Battery(BatterySnapshot),
}

/// Mock power. The drain simulation is `drain_per_min` (% per minute).
pub struct MockPower {
    latest: BatterySnapshot,
    /// The drain in % per minute (0 = stopped).
    pub drain_per_min: u8,
    last_drain: Option<Instant>,
    requests: Vec<PowerRequest>,
    rx: MockInbox<PowerMsg>,
    control: MockControl<PowerMsg>,
    waker: Option<Waker>,
}

impl MockPower {
    /// The initial level and charging state.
    #[must_use]
    pub fn new(percent: u8, charging: bool) -> Self {
        Self::from_channel(percent, charging, default_channel())
    }

    /// Construct with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn with_inbox(
        percent: u8,
        charging: bool,
        config: InboxConfig,
    ) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(percent, charging, channel(config)?))
    }

    fn from_channel(
        percent: u8,
        charging: bool,
        pair: (MockControl<PowerMsg>, MockInbox<PowerMsg>),
    ) -> Self {
        let (control, rx) = pair;
        Self {
            latest: BatterySnapshot {
                percent,
                charging,
                time_to_empty_secs: None,
            },
            drain_per_min: 0,
            last_drain: None,
            requests: Vec::new(),
            rx,
            control,
            waker: None,
        }
    }

    /// Set the maximum messages processed by each poll. Zero pauses queue consumption;
    /// a positive limit resumes it without replacing controls or accepted messages.
    pub fn set_inbox_batch_limit(&mut self, limit: usize) {
        self.rx.set_limit(limit);
    }

    /// The control handle.
    #[must_use]
    pub fn control(&self) -> MockControl<PowerMsg> {
        self.control.clone()
    }

    /// The power requests received so far (for test assertions).
    #[must_use]
    pub fn requests(&self) -> &[PowerRequest] {
        &self.requests
    }
}

impl Backend for MockPower {
    fn attach(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    fn poll_at(&mut self, now: Instant) {
        for _ in 0..self.rx.start_poll(now) {
            let Ok(PowerMsg::Battery(b)) = self.rx.try_recv() else {
                break;
            };
            self.latest = b;
        }
        let last = *self.last_drain.get_or_insert(now);
        // At 0 % there is nothing left to take off — waking every minute after that is a waste (the repaint policy).
        if self.drain_per_min > 0
            && self.latest.percent > 0
            && now.saturating_duration_since(last) >= Duration::from_mins(1)
        {
            self.last_drain = Some(now);
            self.latest.percent = self.latest.percent.saturating_sub(self.drain_per_min);
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        self.rx.next_wake(match self.last_drain {
            Some(last) if self.drain_per_min > 0 && self.latest.percent > 0 => {
                Some(last + Duration::from_mins(1))
            }
            _ => None,
        })
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::BATTERY.union(Capabilities::POWER_CONTROL)
    }
}

impl PowerBackend for MockPower {
    fn battery(&self) -> Option<BatterySnapshot> {
        Some(self.latest)
    }

    fn request(&mut self, req: PowerRequest) -> ServiceResult {
        self.requests.push(req);
        Ok(())
    }
}

/// [`MockWifi`] messages.
#[derive(Debug, Clone)]
pub enum WifiMsg {
    /// Change the list of scan results.
    Networks(Vec<Network>),
    /// Change the state.
    State(WifiState),
    /// Turn it on or off.
    Enabled(bool),
}

/// Mock Wi-Fi: a list of fake APs, a scan delay, and connect delay / failure scenarios.
pub struct MockWifi {
    latest: WifiSnapshot,
    /// How long a scan takes.
    pub scan_delay: Duration,
    /// How long a connection takes.
    pub connect_delay: Duration,
    pending: Option<(Deadline, Pending)>,
    /// The time of the last `poll_at` — what `scan` and `connect` reckon their deadlines from.
    now: Option<Instant>,
    rx: MockInbox<WifiMsg>,
    control: MockControl<WifiMsg>,
    waker: Option<Waker>,
}

#[derive(Debug, Clone)]
enum Pending {
    Scan,
    Connect { ssid: String },
}

/// The connection-failure scenario: the reason is chosen by the SSID prefix (a convention for the
/// tests and the demos). `None` means success.
fn wifi_failure_reason(ssid: &str) -> Option<&'static str> {
    if ssid.starts_with("fail-auth") {
        Some("wrong password")
    } else if ssid.starts_with("fail-timeout") {
        Some("connection timed out")
    } else if ssid.starts_with("fail") {
        Some("could not connect")
    } else {
        None
    }
}

impl MockWifi {
    /// Three fake APs by default.
    #[must_use]
    pub fn new() -> Self {
        Self::from_channel(default_channel())
    }

    /// Construct with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn with_inbox(config: InboxConfig) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(channel(config)?))
    }

    fn from_channel(pair: (MockControl<WifiMsg>, MockInbox<WifiMsg>)) -> Self {
        let (control, rx) = pair;
        let networks = vec![
            Network {
                ssid: "fairing-lab".to_owned(),
                strength: 4,
                secured: true,
            },
            Network {
                ssid: "guest".to_owned(),
                strength: 2,
                secured: false,
            },
            Network {
                ssid: "fail-net".to_owned(),
                strength: 3,
                secured: true,
            },
        ];
        Self {
            latest: WifiSnapshot {
                enabled: true,
                state: WifiState::Idle,
                networks,
                known: Vec::new(),
            },
            scan_delay: Duration::from_millis(1500),
            connect_delay: Duration::from_millis(1200),
            pending: None,
            now: None,
            rx,
            control,
            waker: None,
        }
    }

    /// Set the maximum messages processed by each poll. Zero pauses queue consumption;
    /// a positive limit resumes it without replacing controls or accepted messages.
    pub fn set_inbox_batch_limit(&mut self, limit: usize) {
        self.rx.set_limit(limit);
    }

    /// The control handle.
    #[must_use]
    pub fn control(&self) -> MockControl<WifiMsg> {
        self.control.clone()
    }
}

impl Default for MockWifi {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for MockWifi {
    fn attach(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    fn poll_at(&mut self, now: Instant) {
        self.now = Some(now);
        for _ in 0..self.rx.start_poll(now) {
            let Ok(msg) = self.rx.try_recv() else {
                break;
            };
            match msg {
                WifiMsg::Networks(list) => self.latest.networks = list,
                WifiMsg::State(state) => self.latest.state = state,
                // As `set_enabled`: what was in flight is called off.
                WifiMsg::Enabled(on) => {
                    self.latest.enabled = on;
                    self.latest.state = if on { WifiState::Idle } else { WifiState::Off };
                    self.pending = None;
                }
            }
        }
        // Borrow Pending for the expiry check rather than cloning its SSID on every waiting poll.
        // Completing a scan or connection can still allocate strings or grow the known-network list.
        let due = self.pending.as_mut().map(|(due, _)| due.resolve(now));
        if due.is_some_and(|due| now >= due) {
            if let Some((_, pending)) = self.pending.take() {
                self.latest.state = match pending {
                    // A scan from a connected radio left the connection standing.
                    Pending::Scan => match &self.latest.state {
                        WifiState::Scanning => WifiState::Idle,
                        other => other.clone(),
                    },
                    Pending::Connect { ssid } => {
                        if let Some(reason) = wifi_failure_reason(&ssid) {
                            WifiState::Failed {
                                reason: reason.to_owned(),
                            }
                        } else {
                            let strength = self
                                .latest
                                .networks
                                .iter()
                                .find(|n| n.ssid == ssid)
                                .map_or(3, |n| n.strength);
                            if !self.latest.known.contains(&ssid) {
                                self.latest.known.push(ssid.clone());
                            }
                            WifiState::Connected { ssid, strength }
                        }
                    }
                };
            }
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        self.rx
            .next_wake(self.pending.as_ref().and_then(|(due, _)| due.due))
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::WIFI
    }
}

impl WifiBackend for MockWifi {
    fn snapshot(&self) -> WifiSnapshot {
        self.latest.clone()
    }

    fn enabled(&self) -> bool {
        self.latest.enabled
    }

    fn strength(&self) -> u8 {
        self.latest.strength()
    }

    fn set_enabled(&mut self, on: bool) -> ServiceResult {
        self.latest.enabled = on;
        self.latest.state = if on { WifiState::Idle } else { WifiState::Off };
        self.pending = None;
        Ok(())
    }

    fn scan(&mut self) -> ServiceResult {
        if !self.latest.enabled {
            return Err(ServiceError::new(super::ErrorKind::Denied, "wifi off"));
        }
        if self.pending.is_some() {
            return Err(ServiceError::new(super::ErrorKind::Busy, "busy"));
        }
        // A connected radio scans without dropping its connection.
        if !matches!(self.latest.state, WifiState::Connected { .. }) {
            self.latest.state = WifiState::Scanning;
        }
        self.pending = Some((Deadline::new(self.scan_delay, self.now), Pending::Scan));
        Ok(())
    }

    fn connect(&mut self, ssid: &str, _psk: Option<&str>, hidden: bool) -> ServiceResult {
        // A backend drops the PSK the moment it is handed over and never logs it — this simulator
        // does not store it either.
        if !self.latest.enabled {
            return Err(ServiceError::new(super::ErrorKind::Denied, "wifi off"));
        }
        // A hidden network ("adding a hidden network") need not be in the scan list — that is what
        // the hidden flag means. Anything else has to be scanned or already known.
        let known_or_scanned = self.latest.known.iter().any(|k| k == ssid)
            || self.latest.networks.iter().any(|n| n.ssid == ssid);
        if !hidden && !known_or_scanned {
            return Err(ServiceError::new(
                super::ErrorKind::Other,
                "not in the scan list - for a hidden network, connect with hidden=true",
            ));
        }
        self.latest.state = WifiState::Connecting;
        self.pending = Some((
            Deadline::new(self.connect_delay, self.now),
            Pending::Connect {
                ssid: ssid.to_owned(),
            },
        ));
        Ok(())
    }

    fn disconnect(&mut self) -> ServiceResult {
        self.latest.state = WifiState::Idle;
        self.pending = None;
        Ok(())
    }

    fn forget(&mut self, ssid: &str) -> ServiceResult {
        self.latest.known.retain(|k| k != ssid);
        Ok(())
    }
}

/// [`MockBluetooth`] messages.
#[derive(Debug, Clone)]
pub enum BtMsg {
    /// Change the device list.
    Devices(Vec<BtDevice>),
    /// Turn it on or off.
    Enabled(bool),
}

/// Mock Bluetooth. At first only one paired device is in sight, and a new one (`Headset`) turns up
/// `discover_delay` after `set_discovering(true)` (the discovery-delay simulation).
pub struct MockBluetooth {
    latest: BtSnapshot,
    /// How long after discovery starts a new device turns up.
    pub discover_delay: Duration,
    /// The devices not discovered yet (moved into `devices` one at a time as they are found).
    undiscovered: Vec<BtDevice>,
    /// When the next device turns up (fixed on the first `poll_at`).
    discover_due: Option<Deadline>,
    /// The time of the last `poll_at` — what `pair` and `set_discovering` reckon their deadlines from.
    now: Option<Instant>,
    rx: MockInbox<BtMsg>,
    control: MockControl<BtMsg>,
    waker: Option<Waker>,
}

impl MockBluetooth {
    /// One paired device (`Scanner`) plus one that only turns up through discovery (`Headset`).
    #[must_use]
    pub fn new() -> Self {
        Self::from_channel(default_channel())
    }

    /// Construct with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn with_inbox(config: InboxConfig) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(channel(config)?))
    }

    fn from_channel(pair: (MockControl<BtMsg>, MockInbox<BtMsg>)) -> Self {
        let (control, rx) = pair;
        Self {
            latest: BtSnapshot {
                enabled: true,
                discovering: false,
                devices: vec![BtDevice {
                    addr: "AA:BB:CC:00:00:01".to_owned(),
                    name: "Scanner".to_owned(),
                    paired: true,
                    connected: true,
                    rssi: Some(-50),
                }],
                pending: None,
            },
            discover_delay: Duration::from_millis(800),
            undiscovered: vec![BtDevice {
                addr: "AA:BB:CC:00:00:02".to_owned(),
                name: "Headset".to_owned(),
                paired: false,
                connected: false,
                rssi: Some(-70),
            }],
            discover_due: None,
            now: None,
            rx,
            control,
            waker: None,
        }
    }

    /// Set the maximum messages processed by each poll. Zero pauses queue consumption;
    /// a positive limit resumes it without replacing controls or accepted messages.
    pub fn set_inbox_batch_limit(&mut self, limit: usize) {
        self.rx.set_limit(limit);
    }

    /// The control handle.
    #[must_use]
    pub fn control(&self) -> MockControl<BtMsg> {
        self.control.clone()
    }

    /// The radio on or off. Off stops discovery: nothing more is found until it is on and
    /// discovering again.
    fn switch(&mut self, on: bool) {
        self.latest.enabled = on;
        if !on {
            self.latest.discovering = false;
            self.discover_due = None;
        }
    }
}

impl Default for MockBluetooth {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for MockBluetooth {
    fn attach(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    fn poll_at(&mut self, now: Instant) {
        self.now = Some(now);
        for _ in 0..self.rx.start_poll(now) {
            let Ok(msg) = self.rx.try_recv() else {
                break;
            };
            match msg {
                BtMsg::Devices(list) => self.latest.devices = list,
                BtMsg::Enabled(on) => self.switch(on),
            }
        }
        // Once the deadline has passed it **must** be tidied away — combining the two conditions would
        // have `next_wake` point into the past for ever with no devices left to hand out, and the shell
        // could never go idle.
        let due = self.discover_due.as_mut().map(|due| due.resolve(now));
        if due.is_some_and(|due| now >= due) {
            if self.undiscovered.is_empty() {
                self.discover_due = None;
            } else {
                self.latest.devices.push(self.undiscovered.remove(0));
                self.discover_due = (!self.undiscovered.is_empty())
                    .then(|| Deadline::new(self.discover_delay, Some(now)));
            }
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        self.rx.next_wake(self.discover_due.and_then(|due| due.due))
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::BLUETOOTH
    }
}

impl BluetoothBackend for MockBluetooth {
    fn snapshot(&self) -> BtSnapshot {
        self.latest.clone()
    }

    fn enabled(&self) -> bool {
        self.latest.enabled
    }

    fn any_connected(&self) -> bool {
        self.latest.any_connected()
    }

    fn set_enabled(&mut self, on: bool) -> ServiceResult {
        self.switch(on);
        Ok(())
    }

    fn set_discovering(&mut self, on: bool) -> ServiceResult {
        if on && !self.latest.enabled {
            return Err(ServiceError::new(super::ErrorKind::Denied, "bluetooth off"));
        }
        self.latest.discovering = on;
        self.discover_due = (on && !self.undiscovered.is_empty())
            .then(|| Deadline::new(self.discover_delay, self.now));
        Ok(())
    }

    /// It does not pair straight away — like a real pairing-confirmation flow, it puts a
    /// [`PairingRequest`](crate::services::PairingRequest) up and waits for `respond_pairing`
    /// (with a fixed passkey; it is a simulator, so it is the same every time).
    fn pair(&mut self, addr: &str) -> ServiceResult {
        if !self.latest.devices.iter().any(|d| d.addr == addr) {
            return Err(ServiceError::new(super::ErrorKind::Other, "unknown device"));
        }
        self.latest.pending = Some(super::PairingRequest {
            addr: addr.to_owned(),
            passkey: Some(123_456),
        });
        Ok(())
    }

    fn respond_pairing(&mut self, accept: bool, passkey: Option<u32>) -> ServiceResult {
        let Some(request) = self.latest.pending.take() else {
            return Err(ServiceError::new(
                super::ErrorKind::Other,
                "no pairing request is waiting",
            ));
        };
        if !accept {
            return Ok(());
        }
        if let (Some(expected), Some(given)) = (request.passkey, passkey) {
            if expected != given {
                self.latest.pending = Some(request);
                return Err(ServiceError::new(
                    super::ErrorKind::Denied,
                    "passkey mismatch",
                ));
            }
        }
        if let Some(d) = self
            .latest
            .devices
            .iter_mut()
            .find(|d| d.addr == request.addr)
        {
            d.paired = true;
        }
        Ok(())
    }

    fn connect(&mut self, addr: &str, on: bool) -> ServiceResult {
        if let Some(d) = self.latest.devices.iter_mut().find(|d| d.addr == addr) {
            d.connected = on;
            Ok(())
        } else {
            Err(ServiceError::new(super::ErrorKind::Other, "unknown device"))
        }
    }

    fn remove(&mut self, addr: &str) -> ServiceResult {
        self.latest.devices.retain(|d| d.addr != addr);
        Ok(())
    }
}

/// The mock display (M2): a brightness value in memory plus a memory of the idle inhibit. The `keep_awake` test looks at it.
#[derive(Debug, Clone, Copy)]
pub struct MockDisplay {
    brightness: u8,
    idle_inhibit: bool,
    power: bool,
    info: DisplayInfo,
}

impl MockDisplay {
    /// Brightness `brightness` (0..=100), the idle inhibit off, 1024×600.
    #[must_use]
    pub fn new(brightness: u8) -> Self {
        Self {
            brightness: brightness.min(100),
            idle_inhibit: false,
            power: true,
            info: DisplayInfo {
                size_px: (1024, 600),
                physical_mm: None,
                rotation: 0,
            },
        }
    }

    /// Whether the screen is on.
    #[must_use]
    pub fn powered(&self) -> bool {
        self.power
    }
}

impl Default for MockDisplay {
    fn default() -> Self {
        Self::new(70)
    }
}

impl Backend for MockDisplay {
    fn capabilities(&self) -> Capabilities {
        Capabilities::BRIGHTNESS
    }
}

impl DisplayBackend for MockDisplay {
    fn brightness(&self) -> Option<u8> {
        Some(self.brightness)
    }

    fn set_brightness(&mut self, v: u8) -> ServiceResult {
        self.brightness = v.min(100);
        Ok(())
    }

    fn set_idle_inhibit(&mut self, on: bool) {
        self.idle_inhibit = on;
    }

    fn idle_inhibited(&self) -> Option<bool> {
        Some(self.idle_inhibit)
    }

    fn set_power(&mut self, on: bool) -> ServiceResult {
        self.power = on;
        Ok(())
    }

    fn info(&self) -> DisplayInfo {
        self.info
    }
}

/// [`MockAudio`] messages.
#[derive(Debug, Clone, Copy)]
pub enum AudioMsg {
    /// The level or the mute changed outside the UI — the hardware volume keys, say.
    Volume(AudioSnapshot),
}

/// Mock audio: a level and a mute in memory, and a record of the cues the shell asked for.
pub struct MockAudio {
    latest: AudioSnapshot,
    cues: Vec<Cue>,
    rx: MockInbox<AudioMsg>,
    control: MockControl<AudioMsg>,
}

impl MockAudio {
    /// The initial level (0..=100), unmuted.
    #[must_use]
    pub fn new(level: u8) -> Self {
        Self::from_channel(level, default_channel())
    }

    /// Construct with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn with_inbox(level: u8, config: InboxConfig) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(level, channel(config)?))
    }

    fn from_channel(level: u8, pair: (MockControl<AudioMsg>, MockInbox<AudioMsg>)) -> Self {
        let (control, rx) = pair;
        Self {
            latest: AudioSnapshot {
                level: level.min(100),
                muted: false,
            },
            cues: Vec::new(),
            rx,
            control,
        }
    }

    /// Set the maximum messages processed by each poll. Zero pauses queue consumption;
    /// a positive limit resumes it without replacing controls or accepted messages.
    pub fn set_inbox_batch_limit(&mut self, limit: usize) {
        self.rx.set_limit(limit);
    }

    /// The control handle.
    #[must_use]
    pub fn control(&self) -> MockControl<AudioMsg> {
        self.control.clone()
    }

    /// The cues asked for so far (for test assertions).
    #[must_use]
    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }
}

impl Default for MockAudio {
    fn default() -> Self {
        Self::new(60)
    }
}

impl Backend for MockAudio {
    fn poll_at(&mut self, now: Instant) {
        for _ in 0..self.rx.start_poll(now) {
            let Ok(AudioMsg::Volume(snapshot)) = self.rx.try_recv() else {
                break;
            };
            self.latest = snapshot;
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        self.rx.next_wake(None)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::VOLUME
    }
}

impl AudioBackend for MockAudio {
    fn volume(&self) -> Option<AudioSnapshot> {
        Some(self.latest)
    }

    fn set_volume(&mut self, level: u8) -> ServiceResult {
        self.latest.level = level.min(100);
        Ok(())
    }

    fn set_muted(&mut self, muted: bool) -> ServiceResult {
        self.latest.muted = muted;
        Ok(())
    }

    fn play_cue(&mut self, cue: Cue) {
        self.cues.push(cue);
    }
}

/// [`MockNetwork`] messages.
#[derive(Debug, Clone)]
pub enum NetworkMsg {
    /// A cable went in or came out.
    Link {
        /// The interface name.
        iface: String,
        /// The link is up.
        up: bool,
    },
}

/// Mock network: a wired `eth0` on DHCP and a `wlan0`, and a hostname. `configure` takes effect
/// at once — a static address is used as given, and going back to DHCP hands out the demo lease
/// again.
pub struct MockNetwork {
    ifaces: Vec<IfaceSnapshot>,
    hostname: String,
    rx: MockInbox<NetworkMsg>,
    control: MockControl<NetworkMsg>,
}

/// The address the mock's DHCP "server" hands out.
const MOCK_LEASE: Ipv4Net = Ipv4Net {
    addr: Ipv4Addr::new(192, 168, 0, 42),
    prefix: 24,
};

/// The mock's router, which is also its DNS server.
const MOCK_ROUTER: Ipv4Addr = Ipv4Addr::new(192, 168, 0, 1);

impl MockNetwork {
    /// `eth0` up on DHCP (`192.168.0.42/24`), `wlan0` with no address, hostname `fairing-demo`.
    #[must_use]
    pub fn new() -> Self {
        Self::from_channel(default_channel())
    }

    /// Construct with connection-specific storage and per-poll limits.
    ///
    /// # Errors
    /// Invalid bounded capacity or an unrepresentable queue layout.
    pub fn with_inbox(config: InboxConfig) -> Result<Self, QueueCapacityError> {
        Ok(Self::from_channel(channel(config)?))
    }

    fn from_channel(pair: (MockControl<NetworkMsg>, MockInbox<NetworkMsg>)) -> Self {
        let (control, rx) = pair;
        Self {
            ifaces: vec![
                IfaceSnapshot {
                    name: "eth0".to_owned(),
                    kind: IfaceKind::Ethernet,
                    up: true,
                    ipv4: Some(MOCK_LEASE),
                    gateway: Some(MOCK_ROUTER),
                    dns: vec![IpAddr::V4(MOCK_ROUTER)],
                    mac: Some("a4:5e:60:11:22:33".to_owned()),
                    dhcp: true,
                    ..IfaceSnapshot::default()
                },
                IfaceSnapshot {
                    name: "wlan0".to_owned(),
                    kind: IfaceKind::Wifi,
                    mac: Some("a4:5e:60:44:55:66".to_owned()),
                    dhcp: true,
                    ..IfaceSnapshot::default()
                },
            ],
            hostname: "fairing-demo".to_owned(),
            rx,
            control,
        }
    }

    /// Set the maximum messages processed by each poll. Zero pauses queue consumption;
    /// a positive limit resumes it without replacing controls or accepted messages.
    pub fn set_inbox_batch_limit(&mut self, limit: usize) {
        self.rx.set_limit(limit);
    }

    /// The control handle.
    #[must_use]
    pub fn control(&self) -> MockControl<NetworkMsg> {
        self.control.clone()
    }
}

impl Default for MockNetwork {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for MockNetwork {
    fn poll_at(&mut self, now: Instant) {
        for _ in 0..self.rx.start_poll(now) {
            let Ok(NetworkMsg::Link { iface, up }) = self.rx.try_recv() else {
                break;
            };
            if let Some(found) = self.ifaces.iter_mut().find(|i| i.name == iface) {
                found.up = up;
            }
        }
    }

    fn next_wake(&self) -> Option<Instant> {
        self.rx.next_wake(None)
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::ETHERNET
    }
}

impl NetworkBackend for MockNetwork {
    fn interfaces(&self) -> Vec<IfaceSnapshot> {
        self.ifaces.clone()
    }

    fn ethernet_up(&self) -> Option<bool> {
        let mut wired = self
            .ifaces
            .iter()
            .filter(|i| i.kind == IfaceKind::Ethernet)
            .peekable();
        wired.peek()?;
        Some(wired.any(|i| i.up))
    }

    fn configure(&mut self, iface: &str, config: IpConfig) -> ServiceResult {
        let Some(found) = self.ifaces.iter_mut().find(|i| i.name == iface) else {
            return Err(ServiceError::new(
                ErrorKind::Other,
                format!("no interface `{iface}`"),
            ));
        };
        match config {
            IpConfig::Dhcp => {
                found.dhcp = true;
                found.ipv4 = Some(MOCK_LEASE);
                found.gateway = Some(MOCK_ROUTER);
                found.dns = vec![IpAddr::V4(MOCK_ROUTER)];
            }
            IpConfig::Static { addr, gateway, dns } => {
                found.dhcp = false;
                found.ipv4 = Some(addr);
                found.gateway = gateway;
                found.dns = dns;
            }
        }
        Ok(())
    }

    fn hostname(&self) -> Option<String> {
        Some(self.hostname.clone())
    }

    fn set_hostname(&mut self, name: &str) -> ServiceResult {
        name.clone_into(&mut self.hostname);
        Ok(())
    }
}

/// Mock device information. The uptime counts from the first frame, in the shell's time.
pub struct MockInfo {
    info: DeviceInfo,
    first: Option<Instant>,
    last: Option<Instant>,
}

impl MockInfo {
    /// A demo panel: a model, a serial, this crate's version as the firmware, and a made-up OS.
    #[must_use]
    pub fn new() -> Self {
        Self {
            info: DeviceInfo {
                model: Some("fairing demo panel".to_owned()),
                serial: Some("FD-0001".to_owned()),
                firmware: Some(env!("CARGO_PKG_VERSION").to_owned()),
                os: Some("Mock OS".to_owned()),
                uptime_secs: None,
            },
            first: None,
            last: None,
        }
    }
}

impl Default for MockInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for MockInfo {
    fn poll_at(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }
}

impl InfoBackend for MockInfo {
    fn device(&self) -> DeviceInfo {
        let uptime_secs = self
            .first
            .zip(self.last)
            .map(|(first, last)| last.saturating_duration_since(first).as_secs());
        DeviceInfo {
            uptime_secs,
            ..self.info.clone()
        }
    }
}

/// The demo bundle: a running clock · a battery at 72 % · Wi-Fi · Bluetooth · a display
/// (brightness 70) · audio at 60 % · a wired `eth0` · a device description.
#[must_use]
pub fn services() -> super::Services {
    super::Services::builder()
        .clock(MockClock::running(WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 540,
        }))
        .power(MockPower::new(72, false))
        .wifi(MockWifi::new())
        .bluetooth(MockBluetooth::new())
        .display(MockDisplay::default())
        .audio(MockAudio::default())
        .network(MockNetwork::new())
        .info(MockInfo::new())
        .build()
}

#[cfg(test)]
mod tests {
    use super::{
        AudioMsg, MockAudio, MockBluetooth, MockInfo, MockNetwork, MockPower, MockWifi, NetworkMsg,
        WifiMsg,
    };
    use crate::services::{
        AudioBackend, AudioSnapshot, Backend, BatterySnapshot, BluetoothBackend, ErrorKind,
        InfoBackend, IpConfig, Ipv4Net, NetworkBackend, PowerBackend, WifiBackend, WifiState,
    };
    use std::net::Ipv4Addr;
    use std::time::{Duration, Instant};

    #[test]
    fn control_messages_arrive_on_poll_without_blocking() {
        let mut wifi = MockWifi::new();
        let control = wifi.control();
        assert!(control.send(WifiMsg::Networks(Vec::new())));
        assert_eq!(wifi.snapshot().networks.len(), 3);
        let t0 = Instant::now();
        wifi.poll_at(t0);
        assert_eq!(wifi.snapshot().networks.len(), 0);
        assert!(wifi.scan().is_ok());
        assert_eq!(wifi.snapshot().state, WifiState::Scanning);
        assert!(wifi.next_wake().is_some());
        // Only the time `poll_at` handed over is looked at: unchanged just before the delay, Idle after it.
        wifi.poll_at(t0 + wifi.scan_delay / 2);
        assert_eq!(wifi.snapshot().state, WifiState::Scanning);
        wifi.poll_at(t0 + wifi.scan_delay);
        assert_eq!(wifi.snapshot().state, WifiState::Idle);
    }

    /// A `scan()` called before any frame (before the first `poll_at`) reckons its deadline from
    /// **the first `poll_at`'s time** rather than from real time (a backend does not use
    /// `Instant::now()`).
    #[test]
    fn scan_before_first_poll_anchors_deadline_to_shell_time() {
        let mut wifi = MockWifi::new();
        assert!(wifi.scan().is_ok());
        assert!(
            wifi.next_wake().is_none(),
            "before the shell's time arrives there is no wake time to set"
        );
        // The shell's time may be a value unrelated to real time (headless virtual time).
        let t0 = Instant::now() + Duration::from_hours(1);
        wifi.poll_at(t0);
        assert_eq!(wifi.next_wake(), Some(t0 + wifi.scan_delay));
        assert_eq!(wifi.snapshot().state, WifiState::Scanning);
        wifi.poll_at(t0 + wifi.scan_delay);
        assert_eq!(wifi.snapshot().state, WifiState::Idle);
        assert!(wifi.next_wake().is_none());
    }

    /// Once a deadline has passed with nothing left to hand out, `next_wake` must be cleared — a
    /// `next_wake` left pointing into the past keeps the shell from going idle.
    #[test]
    fn bluetooth_clears_a_past_due_even_with_nothing_left_to_discover() {
        let mut bt = MockBluetooth::new();
        let t0 = Instant::now();
        bt.poll_at(t0);
        assert!(bt.set_discovering(true).is_ok());
        bt.poll_at(t0 + bt.discover_delay);
        assert!(bt.next_wake().is_none());
        // Starting discovery again with the list emptied stands no deadline at all.
        assert!(bt.set_discovering(false).is_ok());
        assert!(bt.set_discovering(true).is_ok());
        assert!(bt.next_wake().is_none());
        bt.poll_at(t0 + bt.discover_delay * 3);
        assert!(bt.next_wake().is_none());
    }

    #[test]
    fn unscanned_ssid_needs_hidden_flag_but_then_connects_and_is_remembered() {
        let mut wifi = MockWifi::new();
        let t0 = Instant::now();
        wifi.poll_at(t0);
        // An SSID in neither the scan list nor the known ones — refused without hidden.
        assert!(wifi.connect("shadow-net", Some("secret"), false).is_err());
        assert!(wifi.connect("shadow-net", Some("secret"), true).is_ok());
        wifi.poll_at(t0 + wifi.connect_delay);
        assert_eq!(
            wifi.snapshot().state,
            WifiState::Connected {
                ssid: "shadow-net".to_owned(),
                strength: 3,
            }
        );
        assert!(wifi.snapshot().known.contains(&"shadow-net".to_owned()));
    }

    #[test]
    fn connect_failure_reason_depends_on_ssid_prefix() {
        let mut wifi = MockWifi::new();
        let t0 = Instant::now();
        wifi.poll_at(t0);
        assert!(wifi.connect("fail-auth-net", None, true).is_ok());
        wifi.poll_at(t0 + wifi.connect_delay);
        assert_eq!(
            wifi.snapshot().state,
            WifiState::Failed {
                reason: "wrong password".to_owned()
            }
        );
    }

    #[test]
    fn bluetooth_discovery_reveals_second_device_after_delay() {
        let mut bt = MockBluetooth::new();
        let t0 = Instant::now();
        bt.poll_at(t0);
        assert_eq!(
            bt.snapshot().devices.len(),
            1,
            "at first only the paired one shows"
        );
        assert!(bt.set_discovering(true).is_ok());
        assert!(bt.next_wake().is_some());
        bt.poll_at(t0 + bt.discover_delay / 2);
        assert_eq!(bt.snapshot().devices.len(), 1, "not yet, before the delay");
        bt.poll_at(t0 + bt.discover_delay);
        assert_eq!(
            bt.snapshot().devices.len(),
            2,
            "a new device found after the delay"
        );
        assert!(
            bt.next_wake().is_none(),
            "with no device left to find it does not wake"
        );
    }

    #[test]
    fn bluetooth_pairing_needs_a_matching_response() {
        let mut bt = MockBluetooth::new();
        bt.poll_at(Instant::now());
        assert!(bt.set_discovering(true).is_ok());
        bt.poll_at(Instant::now() + bt.discover_delay);
        let addr = "AA:BB:CC:00:00:02";
        assert!(bt.pair(addr).is_ok());
        assert!(
            !bt.snapshot()
                .devices
                .iter()
                .any(|d| d.addr == addr && d.paired),
            "only the request is raised — it does not pair straight away"
        );
        assert!(bt.snapshot().pending.is_some());
        // A wrong passkey is refused and the request stays.
        assert!(bt.respond_pairing(true, Some(0)).is_err());
        assert!(bt.snapshot().pending.is_some());
        assert!(bt.respond_pairing(true, Some(123_456)).is_ok());
        assert!(bt
            .snapshot()
            .devices
            .iter()
            .any(|d| d.addr == addr && d.paired));
        assert!(bt.snapshot().pending.is_none());
    }

    #[test]
    fn battery_drain_crosses_20_percent_and_stops_waking_at_zero() {
        let mut power = MockPower::new(22, false);
        power.drain_per_min = 5;
        let t0 = Instant::now();
        power.poll_at(t0);
        power.poll_at(t0 + Duration::from_mins(1));
        assert_eq!(
            power.battery().map(|b| b.percent),
            Some(17),
            "22 - 5, it crosses the 20 % boundary"
        );
        for i in 2..=4 {
            power.poll_at(t0 + Duration::from_mins(i));
        }
        assert_eq!(power.battery().map(|b| b.percent), Some(2));
        power.poll_at(t0 + Duration::from_mins(5));
        assert_eq!(
            power.battery().map(|b| b.percent),
            Some(0),
            "saturating_sub, never negative"
        );
        assert!(
            power.next_wake().is_none(),
            "at 0 % there is nothing left to take off, so it does not wake"
        );
    }

    /// Mock scenarios are deterministic: replaying the same `now` sequence gives the same
    /// snapshot.
    #[test]
    fn same_now_sequence_yields_the_same_snapshot() {
        fn run(times: &[Instant]) -> (WifiState, Option<BatterySnapshot>) {
            let mut wifi = MockWifi::new();
            let mut power = MockPower::new(50, false);
            power.drain_per_min = 3;
            for &t in times {
                wifi.poll_at(t);
                power.poll_at(t);
            }
            assert!(wifi.scan().is_ok());
            for &t in times {
                wifi.poll_at(t + wifi.scan_delay);
            }
            (wifi.snapshot().state, power.battery())
        }
        let t0 = Instant::now();
        let times = [
            t0,
            t0 + Duration::from_millis(16),
            t0 + Duration::from_mins(1),
        ];
        let a = run(&times);
        let b = run(&times);
        assert_eq!(a, b);
    }

    /// The volume keys move the level outside the UI; the next poll picks it up.
    #[test]
    fn audio_takes_commands_and_outside_changes() {
        let mut audio = MockAudio::new(60);
        assert!(audio.set_muted(true).is_ok());
        assert_eq!(
            audio.volume(),
            Some(AudioSnapshot {
                level: 60,
                muted: true
            })
        );
        let control = audio.control();
        assert!(control.send(AudioMsg::Volume(AudioSnapshot {
            level: 20,
            muted: false,
        })));
        audio.poll_at(Instant::now());
        assert_eq!(audio.volume().map(|a| a.level), Some(20));
    }

    /// A static address is used as given, DHCP hands the lease back, an unknown name is an error,
    /// and a cable pulled out is seen on the next poll.
    #[test]
    fn network_configures_and_follows_the_cable() {
        let mut network = MockNetwork::new();
        assert_eq!(network.ethernet_up(), Some(true));
        let addr = Ipv4Net {
            addr: Ipv4Addr::new(10, 0, 0, 5),
            prefix: 8,
        };
        let config = IpConfig::Static {
            addr,
            gateway: None,
            dns: Vec::new(),
        };
        assert!(network.configure("eth0", config).is_ok());
        let eth0 = network.interfaces().into_iter().next();
        assert!(eth0.is_some_and(|i| i.ipv4 == Some(addr) && !i.dhcp));
        assert!(network.configure("eth0", IpConfig::Dhcp).is_ok());
        assert!(network.interfaces().first().is_some_and(|i| i.dhcp));
        let missing = network.configure("eth9", IpConfig::Dhcp);
        assert_eq!(missing.map_err(|e| e.kind), Err(ErrorKind::Other));
        assert!(network.control().send(NetworkMsg::Link {
            iface: "eth0".to_owned(),
            up: false,
        }));
        network.poll_at(Instant::now());
        assert_eq!(network.ethernet_up(), Some(false));
    }

    /// The uptime is the shell's time since the first frame.
    #[test]
    fn info_counts_uptime_in_shell_time() {
        let mut info = MockInfo::new();
        assert_eq!(info.device().uptime_secs, None);
        let t0 = Instant::now();
        info.poll_at(t0);
        info.poll_at(t0 + Duration::from_secs(90));
        assert_eq!(info.device().uptime_secs, Some(90));
        assert!(info.device().model.is_some());
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "mock channel contract assertions")]
mod inbox_tests {
    use super::*;
    use crate::inbox::{InboxCapacity, QueueErrorKind};

    fn default_capacity_is_shared<M>(control: &MockControl<M>, make: impl Fn() -> M) {
        // Validate the fixed-default layout for every concrete crate-owned message type.
        assert!(Inbox::<M>::configured(InboxConfig::default()).is_ok());
        for _ in 0..1024 {
            assert!(control.try_send(make()).is_ok());
        }
        let rejected = control.clone().try_send(make()).unwrap_err();
        assert_eq!(rejected.kind(), QueueErrorKind::Full);
        assert!(!control.send(make()));
    }

    #[test]
    fn all_default_mock_channels_have_a_finite_shared_capacity() {
        default_capacity_is_shared(&MockClock::fixed(WallTime::default()).control(), || {
            ClockMsg::Set(WallTime::default())
        });
        default_capacity_is_shared(&MockPower::new(50, false).control(), || {
            PowerMsg::Battery(BatterySnapshot {
                percent: 1,
                charging: false,
                time_to_empty_secs: None,
            })
        });
        default_capacity_is_shared(&MockWifi::new().control(), || WifiMsg::Enabled(true));
        default_capacity_is_shared(&MockBluetooth::new().control(), || BtMsg::Enabled(true));
        default_capacity_is_shared(&MockAudio::new(50).control(), || {
            AudioMsg::Volume(AudioSnapshot {
                level: 1,
                muted: false,
            })
        });
        default_capacity_is_shared(&MockNetwork::new().control(), || NetworkMsg::Link {
            iface: "eth0".into(),
            up: true,
        });
    }

    #[test]
    fn default_poll_budget_limits_a_large_accepted_backlog() {
        let mut clock = MockClock::fixed(WallTime::default());
        let control = clock.control();
        for utc_secs in 1..=300 {
            control
                .try_send(ClockMsg::Set(WallTime {
                    utc_secs,
                    offset_min: 0,
                }))
                .unwrap();
        }
        let now = Instant::now();
        clock.poll_at(now);
        assert_eq!(clock.now().utc_secs, 256);
        assert_eq!(clock.next_wake(), Some(now));
        clock.poll_at(now);
        assert_eq!(clock.now().utc_secs, 300);
        assert_eq!(clock.next_wake(), None);
    }

    #[test]
    fn batches_preserve_fifo_and_schedule_their_own_continuation() {
        let mut clock = MockClock::fixed_with_inbox(
            WallTime::default(),
            InboxConfig {
                capacity: InboxCapacity::Bounded(3),
                batch_limit: 1,
            },
        )
        .unwrap();
        let control = clock.control();
        for utc_secs in [10, 20, 30] {
            control
                .try_send(ClockMsg::Set(WallTime {
                    utc_secs,
                    offset_min: 0,
                }))
                .unwrap();
        }
        let start = Instant::now();
        for (i, utc_secs) in [10, 20, 30].into_iter().enumerate() {
            let now = start + Duration::from_secs(u64::try_from(i).unwrap());
            clock.poll_at(now);
            assert_eq!(clock.now().utc_secs, utc_secs);
            assert_eq!(clock.next_wake(), Some(now));
        }
        // A full batch may request one conservative extra poll, even when now empty.
        clock.poll_at(start + Duration::from_secs(3));
        assert_eq!(clock.next_wake(), None);
        assert!(control.try_send(ClockMsg::Set(WallTime::default())).is_ok());
    }

    #[test]
    fn unlimited_storage_does_not_make_polling_unlimited() {
        let mut wifi = MockWifi::with_inbox(InboxConfig {
            capacity: InboxCapacity::Unbounded,
            batch_limit: 1,
        })
        .unwrap();
        let control = wifi.control();
        control.try_send(WifiMsg::Enabled(false)).unwrap();
        for _ in 0..1100 {
            control.try_send(WifiMsg::Enabled(true)).unwrap();
        }
        let now = Instant::now();
        wifi.poll_at(now);
        assert!(!wifi.enabled());
        assert_eq!(wifi.next_wake(), Some(now));
        wifi.poll_at(now);
        assert!(wifi.enabled());
    }

    #[test]
    fn zero_budget_pauses_without_spinning_and_can_resume_with_the_same_control() {
        let mut audio = MockAudio::with_inbox(
            25,
            InboxConfig {
                capacity: InboxCapacity::Bounded(1),
                batch_limit: 0,
            },
        )
        .unwrap();
        let control = audio.control();
        control
            .try_send(AudioMsg::Volume(AudioSnapshot {
                level: 80,
                muted: true,
            }))
            .unwrap();
        let now = Instant::now();
        audio.poll_at(now);
        assert_eq!(audio.volume().unwrap().level, 25);
        assert_eq!(audio.next_wake(), None);
        audio.set_inbox_batch_limit(1);
        assert_eq!(audio.next_wake(), Some(now));
        audio.poll_at(now);
        assert_eq!(audio.volume().unwrap().level, 80);
        assert!(control
            .try_send(AudioMsg::Volume(AudioSnapshot {
                level: 20,
                muted: false
            }))
            .is_ok());
    }

    #[test]
    fn queue_rejection_preserves_owned_payload_and_disconnects_explicitly() {
        let wifi = MockWifi::with_inbox(InboxConfig {
            capacity: InboxCapacity::Bounded(1),
            batch_limit: 1,
        })
        .unwrap();
        let control = wifi.control();
        control.try_send(WifiMsg::Enabled(true)).unwrap();
        let networks = vec![Network {
            ssid: "owned test network".into(),
            strength: 1,
            secured: false,
        }];
        let storage = networks.as_ptr();
        let error = control.try_send(WifiMsg::Networks(networks)).unwrap_err();
        assert_eq!(error.kind(), QueueErrorKind::Full);
        let rejected = error.into_inner();
        assert!(matches!(&rejected, WifiMsg::Networks(_)));
        if let WifiMsg::Networks(returned) = rejected {
            assert_eq!(returned.as_ptr(), storage);
        }
        drop(wifi);
        assert_eq!(
            control
                .try_send(WifiMsg::Enabled(false))
                .unwrap_err()
                .kind(),
            QueueErrorKind::Disconnected
        );
    }

    #[test]
    fn batch_backlog_does_not_starve_simulated_deadlines() {
        let mut wifi = MockWifi::with_inbox(InboxConfig {
            capacity: InboxCapacity::Bounded(4),
            batch_limit: 1,
        })
        .unwrap();
        assert!(wifi.scan().is_ok());
        for _ in 0..4 {
            wifi.control()
                .try_send(WifiMsg::Networks(Vec::new()))
                .unwrap();
        }
        let now = Instant::now();
        wifi.poll_at(now);
        assert_eq!(wifi.next_wake(), Some(now));
        wifi.poll_at(now + wifi.scan_delay);
        assert_eq!(wifi.snapshot().state, WifiState::Idle);
        assert_eq!(wifi.next_wake(), Some(now + wifi.scan_delay));
    }

    #[test]
    fn power_bluetooth_and_network_apply_only_one_message_per_poll() {
        let config = InboxConfig {
            capacity: InboxCapacity::Bounded(2),
            batch_limit: 1,
        };
        let mut power = MockPower::with_inbox(50, false, config).unwrap();
        let mut bluetooth = MockBluetooth::with_inbox(config).unwrap();
        let mut network = MockNetwork::with_inbox(config).unwrap();
        for (percent, enabled) in [(10, false), (90, true)] {
            power
                .control()
                .try_send(PowerMsg::Battery(BatterySnapshot {
                    percent,
                    charging: false,
                    time_to_empty_secs: None,
                }))
                .unwrap();
            bluetooth
                .control()
                .try_send(BtMsg::Enabled(enabled))
                .unwrap();
            network
                .control()
                .try_send(NetworkMsg::Link {
                    iface: "eth0".into(),
                    up: enabled,
                })
                .unwrap();
        }
        let now = Instant::now();
        for (percent, enabled) in [(10, false), (90, true)] {
            power.poll_at(now);
            bluetooth.poll_at(now);
            network.poll_at(now);
            assert_eq!(power.battery().unwrap().percent, percent);
            assert_eq!(bluetooth.snapshot().enabled, enabled);
            assert_eq!(network.ethernet_up(), Some(enabled));
            assert_eq!(power.next_wake(), Some(now));
            assert_eq!(bluetooth.next_wake(), Some(now));
            assert_eq!(network.next_wake(), Some(now));
        }
        power.poll_at(now);
        bluetooth.poll_at(now);
        network.poll_at(now);
        assert_eq!(power.next_wake(), None);
        assert_eq!(bluetooth.next_wake(), None);
        assert_eq!(network.next_wake(), None);
    }

    #[test]
    fn all_configurable_constructors_reject_invalid_capacity() {
        let invalid = InboxConfig {
            capacity: InboxCapacity::Bounded(0),
            batch_limit: 1,
        };
        assert!(MockClock::fixed_with_inbox(WallTime::default(), invalid).is_err());
        assert!(MockClock::running_with_inbox(WallTime::default(), invalid).is_err());
        assert!(MockPower::with_inbox(50, false, invalid).is_err());
        assert!(MockWifi::with_inbox(invalid).is_err());
        assert!(MockBluetooth::with_inbox(invalid).is_err());
        assert!(MockAudio::with_inbox(50, invalid).is_err());
        assert!(MockNetwork::with_inbox(invalid).is_err());
    }
}
