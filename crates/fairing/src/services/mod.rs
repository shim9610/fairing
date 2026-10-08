//! The system backend traits and their implementations.
//!
//! The principle: the UI sees only the traits. Reads are `snapshot()` (a clone of the cache),
//! writes are commands that return immediately, and the result arrives with the next snapshot
//! and a [`Waker`]. **Never block the UI thread**, **no locks** — worker ↔ UI is an `mpsc`
//! channel and the UI side only ever uses `try_recv`.
//!
//! **The implementations are the integrator's**. The crate ships the traits, a `Null*`
//! for each (the default for a backend you do not name) and a `Mock*` simulator for demos and
//! tests — no system code. A device plugs in its own: sysfs, a vendor SDK, D-Bus, a PLC, whatever
//! the hardware speaks. Anything the shell has no slot for goes in as a
//! [custom backend](ServicesBuilder::custom) and takes part in the frame the same way.

pub mod clock;
#[cfg(feature = "mock")]
pub mod mock;
pub mod null;

pub use crate::time::WallTime;
use std::any::Any;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Instant;

/// A thin wrapper around `egui::Context`. When a backend changes state, `wake()` →
/// `request_repaint()`. It is `Clone + Send + Sync`, so it can go to a worker thread. egui's own
/// locking inside `Context` is its thread-safe API and is the one exception to the rule against
/// lock-based shared state.
#[derive(Clone)]
pub struct Waker(egui::Context);

impl Waker {
    /// Build one from a context.
    #[must_use]
    pub fn new(ctx: &egui::Context) -> Self {
        Self(ctx.clone())
    }

    /// Wake the UI thread (request the next frame).
    pub fn wake(&self) {
        self.0.request_repaint();
    }

    /// Wake it after a delay.
    pub fn wake_after(&self, after: std::time::Duration) {
        self.0.request_repaint_after(after);
    }

    /// The wrapped context.
    #[must_use]
    pub fn context(&self) -> &egui::Context {
        &self.0
    }
}

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Waker")
    }
}

/// The backend capability bits (`Capabilities`). Unsupported entries disappear from the UI on their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities(pub u32);

impl Capabilities {
    /// Supports nothing.
    pub const NONE: Self = Self(0);
    /// Provides a battery snapshot.
    pub const BATTERY: Self = Self(1);
    /// Reboot and shutdown commands.
    pub const POWER_CONTROL: Self = Self(1 << 1);
    /// Wi-Fi.
    pub const WIFI: Self = Self(1 << 2);
    /// Bluetooth.
    pub const BLUETOOTH: Self = Self(1 << 3);
    /// The time can be set.
    pub const CLOCK_SET: Self = Self(1 << 4);
    /// Volume — an [`AudioBackend`] that reports it shows `status.volume` and drives `settings.sound`.
    pub const VOLUME: Self = Self(1 << 5);
    /// Brightness — a [`DisplayBackend`] that reports it shows `status.brightness` and the brightness tile.
    pub const BRIGHTNESS: Self = Self(1 << 6);
    /// Ethernet — a [`NetworkBackend`] that reports it shows `status.ethernet`.
    pub const ETHERNET: Self = Self(1 << 7);

    /// Whether it contains another.
    #[must_use]
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The union.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// The kind of a backend error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// This backend does not support it (an absence, not a failure).
    Unsupported,
    /// Already in progress.
    Busy,
    /// Not permitted.
    Denied,
    /// IO.
    Io,
    /// Timed out.
    Timeout,
    /// Anything else.
    Other,
}

/// A backend error. The shell surfaces it as a toast or a notification and the UI does not die.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceError {
    /// The kind.
    pub kind: ErrorKind,
    /// The message.
    pub message: String,
}

impl ServiceError {
    /// `Unsupported`.
    #[must_use]
    pub fn unsupported() -> Self {
        Self {
            kind: ErrorKind::Unsupported,
            message: "unsupported".to_owned(),
        }
    }

    /// A new error.
    #[must_use]
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for ServiceError {}

/// A backend result.
pub type ServiceResult = Result<(), ServiceError>;

/// What every backend shares. `poll` is called on the UI thread every frame and never blocks.
pub trait Backend {
    /// Given once by the shell at startup. Hand it to a worker thread and `wake()` when the state changes.
    fn attach(&mut self, _waker: Waker) {}
    /// Every frame: drain the channel with `try_recv` and refresh the `latest` snapshot.
    fn poll(&mut self) {}
    /// What frame stage 3 actually calls. `now` is the shell's monotonic time
    /// ([`crate::Shell::now`]) — headless it is virtual time (1/60 s at a step), so a
    /// time-dependent backend (a simulated scan delay, say) can be tested with no `sleep`.
    /// The default delegates to [`Backend::poll`]. A backend that watches the
    /// clock implements this and does not use `Instant::now()`.
    fn poll_at(&mut self, _now: Instant) {
        self.poll();
    }
    /// When a time-based state change is due (a simulated scan finishing, say). The shell arms
    /// `request_repaint_after` for then — the way to express "wake me later" with no worker and
    /// no sleep.
    fn next_wake(&self) -> Option<Instant> {
        None
    }
    /// The capabilities.
    fn capabilities(&self) -> Capabilities {
        Capabilities::NONE
    }
}

/// The clock. UTC seconds plus a local offset.
pub trait ClockSource: Backend {
    /// Now.
    fn now(&self) -> WallTime;
    /// Set the time (optional).
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_time(&mut self, _utc_secs: u64) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

/// A battery snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatterySnapshot {
    /// The level, 0–100.
    pub percent: u8,
    /// Charging.
    pub charging: bool,
    /// Time remaining (seconds), or `None` if unknown.
    pub time_to_empty_secs: Option<u64>,
}

/// A power request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerRequest {
    /// Reboot.
    Reboot,
    /// Shut down.
    Shutdown,
    /// Suspend.
    Suspend,
}

/// Power and battery.
pub trait PowerBackend: Backend {
    /// The battery. `None` on a machine without one.
    fn battery(&self) -> Option<BatterySnapshot>;
    /// Reboot, shut down or suspend. Returns immediately.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn request(&mut self, req: PowerRequest) -> ServiceResult;
}

/// The Wi-Fi state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum WifiState {
    /// The radio is off.
    Off,
    /// On, not connected.
    #[default]
    Idle,
    /// Scanning.
    Scanning,
    /// Connecting.
    Connecting,
    /// Connected.
    Connected {
        /// SSID.
        ssid: String,
        /// The strength, 0–4.
        strength: u8,
    },
    /// Failed.
    Failed {
        /// The reason.
        reason: String,
    },
}

/// A scanned network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Network {
    /// SSID.
    pub ssid: String,
    /// The strength, 0–4.
    pub strength: u8,
    /// Secured.
    pub secured: bool,
}

/// A Wi-Fi snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WifiSnapshot {
    /// On.
    pub enabled: bool,
    /// The state.
    pub state: WifiState,
    /// The scan results.
    pub networks: Vec<Network>,
    /// The saved SSIDs.
    pub known: Vec<String>,
}

impl WifiSnapshot {
    /// The strength for the status bar icon. Not connected is 0.
    #[must_use]
    pub fn strength(&self) -> u8 {
        match &self.state {
            WifiState::Connected { strength, .. } => *strength,
            _ => 0,
        }
    }
}

/// Wi-Fi.
pub trait WifiBackend: Backend {
    /// A clone of the snapshot.
    fn snapshot(&self) -> WifiSnapshot;
    /// Whether it is on — a value the status bar asks for **every frame**. The default
    /// implementation clones [`WifiBackend::snapshot`] (a heap allocation), so a backend should
    /// implement this and [`WifiBackend::strength`] directly.
    fn enabled(&self) -> bool {
        self.snapshot().enabled
    }
    /// The connection strength (the same as [`WifiSnapshot::strength`]; 0 when not connected).
    /// Asked every frame — the default clones the snapshot, so implement it directly.
    fn strength(&self) -> u8 {
        self.snapshot().strength()
    }
    /// Turn it on or off.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_enabled(&mut self, on: bool) -> ServiceResult;
    /// Start a scan.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn scan(&mut self) -> ServiceResult;
    /// Connect. The PSK is dropped immediately after it is passed on.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn connect(&mut self, ssid: &str, psk: Option<&str>, hidden: bool) -> ServiceResult;
    /// Disconnect.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn disconnect(&mut self) -> ServiceResult;
    /// Forget a network.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn forget(&mut self, ssid: &str) -> ServiceResult;
}

/// A Bluetooth device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtDevice {
    /// The address.
    pub addr: String,
    /// The name.
    pub name: String,
    /// Paired.
    pub paired: bool,
    /// Connected.
    pub connected: bool,
    /// RSSI.
    pub rssi: Option<i16>,
}

/// A pairing confirmation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRequest {
    /// The address.
    pub addr: String,
    /// The passkey to display.
    pub passkey: Option<u32>,
}

/// A Bluetooth snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BtSnapshot {
    /// On.
    pub enabled: bool,
    /// Discovering.
    pub discovering: bool,
    /// The device list.
    pub devices: Vec<BtDevice>,
    /// A pending pairing request.
    pub pending: Option<PairingRequest>,
}

impl BtSnapshot {
    /// Whether any device is connected (for the status bar icon).
    #[must_use]
    pub fn any_connected(&self) -> bool {
        self.devices.iter().any(|d| d.connected)
    }
}

/// Bluetooth.
pub trait BluetoothBackend: Backend {
    /// A clone of the snapshot.
    fn snapshot(&self) -> BtSnapshot;
    /// Whether it is on — a value the status bar asks for **every frame**. The default
    /// implementation clones [`BluetoothBackend::snapshot`] (a heap allocation), so a backend
    /// should implement this and [`BluetoothBackend::any_connected`] directly.
    fn enabled(&self) -> bool {
        self.snapshot().enabled
    }
    /// Whether any device is connected ([`BtSnapshot::any_connected`]). Implementing it directly is recommended.
    fn any_connected(&self) -> bool {
        self.snapshot().any_connected()
    }
    /// Turn it on or off.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_enabled(&mut self, on: bool) -> ServiceResult;
    /// Turn discovery on or off.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_discovering(&mut self, on: bool) -> ServiceResult;
    /// Pair.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn pair(&mut self, addr: &str) -> ServiceResult;
    /// Respond to a pairing request.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn respond_pairing(&mut self, accept: bool, passkey: Option<u32>) -> ServiceResult;
    /// Connect or disconnect.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn connect(&mut self, addr: &str, on: bool) -> ServiceResult;
    /// Remove.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn remove(&mut self, addr: &str) -> ServiceResult;
}

/// Display information (`DisplayBackend::info`; read-only).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DisplayInfo {
    /// The resolution (px).
    pub size_px: (u32, u32),
    /// The physical size (mm), or `None` if unknown.
    pub physical_mm: Option<(f32, f32)>,
    /// The rotation (degrees: 0, 90, 180, 270).
    pub rotation: u16,
}

/// The display backend. M2 uses only [`DisplayBackend::set_idle_inhibit`] for
/// `keep_awake` ([`crate::ChromePolicy::keep_awake`]) and the brightness for `tile.brightness`.
pub trait DisplayBackend: Backend {
    /// The brightness, 0..=100. `None` if unknown (the item and the tile hide).
    fn brightness(&self) -> Option<u8>;
    /// Set the brightness.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_brightness(&mut self, v: u8) -> ServiceResult;
    /// Stop or resume the idle timer. Failure is silent — the shell only calls it when the policy changes.
    fn set_idle_inhibit(&mut self, on: bool);
    /// The last idle-inhibit state requested. `None` if the backend does not know (the mock remembers, for tests).
    fn idle_inhibited(&self) -> Option<bool> {
        None
    }
    /// Turn the display on or off.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it or it fails.
    fn set_power(&mut self, on: bool) -> ServiceResult;
    /// The resolution, physical size and rotation.
    fn info(&self) -> DisplayInfo {
        DisplayInfo::default()
    }
}

/// The audio output's level and mute ([`AudioBackend::volume`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioSnapshot {
    /// The level, 0..=100.
    pub level: u8,
    /// Muted. The level is kept while muted, so unmuting comes back to it.
    pub muted: bool,
}

/// A short sound the shell asks for. What each one sounds like — or whether it
/// sounds at all — is the backend's to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Cue {
    /// A notification arrived (a new one, not an update to one already shown).
    Notify,
}

/// The audio output. `settings.sound` and `status.volume` go through it.
///
/// With the `Null` default the sound screen still writes `audio.volume` and `audio.muted`, so an
/// integrator who would rather apply them from [`crate::ShellEvent::SettingChanged`] can.
pub trait AudioBackend: Backend {
    /// The level and mute. `None` if unknown (the status item hides). Asked every frame while
    /// `status.volume` is shown, so keep it a copy of a cached value.
    fn volume(&self) -> Option<AudioSnapshot>;
    /// Set the level, 0..=100.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_volume(&mut self, level: u8) -> ServiceResult;
    /// Mute or unmute.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_muted(&mut self, muted: bool) -> ServiceResult;
    /// Play a cue. Optional — the default plays nothing.
    fn play_cue(&mut self, _cue: Cue) {}
}

/// What kind of link an interface is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum IfaceKind {
    /// Wired.
    Ethernet,
    /// Wireless — the radio [`WifiBackend`] drives, listed here for its address.
    Wifi,
    /// Anything else (a USB gadget, a tunnel, a cellular modem).
    #[default]
    Other,
}

/// An IPv4 address with its prefix length (`192.168.0.42/24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Net {
    /// The address.
    pub addr: Ipv4Addr,
    /// The prefix length, 0..=32.
    pub prefix: u8,
}

/// An IPv6 address with its prefix length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv6Net {
    /// The address.
    pub addr: Ipv6Addr,
    /// The prefix length, 0..=128.
    pub prefix: u8,
}

/// One network interface as the backend sees it. Build it with
/// `..Default::default()` so that a field added later does not break the build.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IfaceSnapshot {
    /// The system name (`eth0`, `enp1s0`, `wlan0`).
    pub name: String,
    /// The kind.
    pub kind: IfaceKind,
    /// The link is up (a cable in, an association made).
    pub up: bool,
    /// The IPv4 address, if it has one.
    pub ipv4: Option<Ipv4Net>,
    /// The IPv6 addresses.
    pub ipv6: Vec<Ipv6Net>,
    /// The IPv4 gateway.
    pub gateway: Option<Ipv4Addr>,
    /// The DNS servers.
    pub dns: Vec<IpAddr>,
    /// The hardware address, as the system prints it.
    pub mac: Option<String>,
    /// The address came from DHCP rather than being set by hand.
    pub dhcp: bool,
}

/// How an interface gets its IPv4 address ([`NetworkBackend::configure`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum IpConfig {
    /// From a DHCP server.
    Dhcp,
    /// Set by hand.
    Static {
        /// The address and prefix.
        addr: Ipv4Net,
        /// The gateway.
        gateway: Option<Ipv4Addr>,
        /// The DNS servers.
        dns: Vec<IpAddr>,
    },
}

/// Wired and other interfaces, their addresses and the hostname.
/// `status.ethernet` and `settings.network` go through it.
pub trait NetworkBackend: Backend {
    /// The interfaces, in the order to list them.
    fn interfaces(&self) -> Vec<IfaceSnapshot>;
    /// Whether a wired link is up: `Some(true)` up, `Some(false)` a wired interface with no link,
    /// `None` no wired interface at all (the status item hides). `status.ethernet` asks it every
    /// frame and the default walks [`NetworkBackend::interfaces`] — an allocation — so a backend
    /// should answer it from its cache directly.
    fn ethernet_up(&self) -> Option<bool> {
        let wired: Vec<bool> = self
            .interfaces()
            .iter()
            .filter(|iface| iface.kind == IfaceKind::Ethernet)
            .map(|iface| iface.up)
            .collect();
        (!wired.is_empty()).then(|| wired.contains(&true))
    }
    /// Change how an interface is addressed. Returns at once; the change shows in a later
    /// [`NetworkBackend::interfaces`].
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn configure(&mut self, _iface: &str, _config: IpConfig) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
    /// The hostname, if known.
    fn hostname(&self) -> Option<String> {
        None
    }
    /// Set the hostname.
    ///
    /// # Errors
    /// [`ServiceError`] if the backend does not support it (`Unsupported`) or it fails.
    fn set_hostname(&mut self, _name: &str) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

/// What the device says about itself ([`InfoBackend::device`]). Every field is
/// optional — show what you know. Build it with `..Default::default()`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DeviceInfo {
    /// The model name.
    pub model: Option<String>,
    /// The serial number.
    pub serial: Option<String>,
    /// The firmware or application version.
    pub firmware: Option<String>,
    /// The operating system (`Ubuntu Core 24`).
    pub os: Option<String>,
    /// Seconds since boot.
    pub uptime_secs: Option<u64>,
}

/// The device's own description. `settings.about` shows it.
pub trait InfoBackend: Backend {
    /// The description.
    fn device(&self) -> DeviceInfo;
}

/// A backend of the integrator's own, kept so that it can be handed back by its type.
trait Custom: Backend + Any {}

impl<T: Backend + Any> Custom for T {}

/// The set of backends. Anything unspecified is a `Null*` (the clock is [`clock::SystemClock`]).
pub struct Services {
    /// The clock.
    pub clock: Box<dyn ClockSource>,
    /// Power and battery.
    pub power: Box<dyn PowerBackend>,
    /// Wi-Fi.
    pub wifi: Box<dyn WifiBackend>,
    /// Bluetooth.
    pub bluetooth: Box<dyn BluetoothBackend>,
    /// The display (M2: idle inhibit and brightness).
    pub display: Box<dyn DisplayBackend>,
    /// The audio output.
    pub audio: Box<dyn AudioBackend>,
    /// Wired and other interfaces, and the hostname.
    pub network: Box<dyn NetworkBackend>,
    /// The device's description.
    pub info: Box<dyn InfoBackend>,
    /// The integrator's own backends ([`ServicesBuilder::custom`]).
    custom: Vec<Box<dyn Custom>>,
}

impl Services {
    /// The builder.
    #[must_use]
    pub fn builder() -> ServicesBuilder {
        ServicesBuilder::default()
    }

    /// Exactly what it says — **everything Null**, including the clock
    /// ([`null::NullClock`], fixed at epoch). Use it to keep a headless test off the real wall
    /// clock (virtual time). For the system clock, use the default from
    /// [`Services::builder`] ([`clock::SystemClock`]).
    #[must_use]
    pub fn null() -> Self {
        ServicesBuilder::default().clock(null::NullClock).build()
    }

    /// **A backend of your own, by its type** — what [`ServicesBuilder::custom`] put in. `None`
    /// if there is none of that type.
    #[must_use]
    pub fn custom<T: Backend + 'static>(&self) -> Option<&T> {
        self.custom
            .iter()
            .find_map(|backend| (&**backend as &dyn Any).downcast_ref::<T>())
    }

    /// The same, mutable — to send it a command from a screen
    /// (`cx.services.custom_mut::<Gpio>()`).
    pub fn custom_mut<T: Backend + 'static>(&mut self) -> Option<&mut T> {
        self.custom
            .iter_mut()
            .find_map(|backend| (&mut **backend as &mut dyn Any).downcast_mut::<T>())
    }

    /// Every backend, built-in and custom — for the frame's poll and the startup attach.
    fn each(&mut self) -> impl Iterator<Item = &mut dyn Backend> + '_ {
        let builtin: [&mut dyn Backend; 8] = [
            &mut *self.clock,
            &mut *self.power,
            &mut *self.wifi,
            &mut *self.bluetooth,
            &mut *self.display,
            &mut *self.audio,
            &mut *self.network,
            &mut *self.info,
        ];
        builtin
            .into_iter()
            .chain(self.custom.iter_mut().map(|b| &mut **b as &mut dyn Backend))
    }

    /// Frame stage 3: [`Backend::poll_at`] on every backend. Returns the earliest `next_wake`.
    /// `now` is the shell's monotonic time (virtual, headless).
    pub fn poll(&mut self, now: Instant) -> Option<Instant> {
        self.each()
            .filter_map(|backend| {
                backend.poll_at(now);
                backend.next_wake()
            })
            .min()
    }

    /// Hand the `Waker` to every backend at startup.
    pub fn attach(&mut self, waker: &Waker) {
        for backend in self.each() {
            backend.attach(waker.clone());
        }
    }
}

impl fmt::Debug for Services {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Services").finish_non_exhaustive()
    }
}

/// The [`Services`] builder.
#[derive(Default)]
pub struct ServicesBuilder {
    clock: Option<Box<dyn ClockSource>>,
    power: Option<Box<dyn PowerBackend>>,
    wifi: Option<Box<dyn WifiBackend>>,
    bluetooth: Option<Box<dyn BluetoothBackend>>,
    display: Option<Box<dyn DisplayBackend>>,
    audio: Option<Box<dyn AudioBackend>>,
    network: Option<Box<dyn NetworkBackend>>,
    info: Option<Box<dyn InfoBackend>>,
    custom: Vec<Box<dyn Custom>>,
}

impl ServicesBuilder {
    /// The clock.
    #[must_use]
    pub fn clock(mut self, backend: impl ClockSource + 'static) -> Self {
        self.clock = Some(Box::new(backend));
        self
    }

    /// Power.
    #[must_use]
    pub fn power(mut self, backend: impl PowerBackend + 'static) -> Self {
        self.power = Some(Box::new(backend));
        self
    }

    /// Wi-Fi.
    #[must_use]
    pub fn wifi(mut self, backend: impl WifiBackend + 'static) -> Self {
        self.wifi = Some(Box::new(backend));
        self
    }

    /// Bluetooth.
    #[must_use]
    pub fn bluetooth(mut self, backend: impl BluetoothBackend + 'static) -> Self {
        self.bluetooth = Some(Box::new(backend));
        self
    }

    /// The display (M2: `keep_awake` idle inhibit and `tile.brightness`).
    #[must_use]
    pub fn display(mut self, backend: impl DisplayBackend + 'static) -> Self {
        self.display = Some(Box::new(backend));
        self
    }

    /// The audio output (`settings.sound`, `status.volume`).
    #[must_use]
    pub fn audio(mut self, backend: impl AudioBackend + 'static) -> Self {
        self.audio = Some(Box::new(backend));
        self
    }

    /// Wired and other interfaces, and the hostname (`settings.network`, `status.ethernet`).
    #[must_use]
    pub fn network(mut self, backend: impl NetworkBackend + 'static) -> Self {
        self.network = Some(Box::new(backend));
        self
    }

    /// The device's description (`settings.about`).
    #[must_use]
    pub fn info(mut self, backend: impl InfoBackend + 'static) -> Self {
        self.info = Some(Box::new(backend));
        self
    }

    /// **A backend of your own** — a GPIO line, a PLC link, a card reader, a door sensor:
    /// anything the shell has no slot for. It takes part in the frame like the built-in ones —
    /// [`Backend::attach`] hands it the [`Waker`] at startup, [`Backend::poll_at`] runs every frame
    /// and [`Backend::next_wake`] arms the next repaint — and a screen reaches it by its type with
    /// [`Services::custom`] / [`Services::custom_mut`].
    ///
    /// One per type: a second of the same type replaces the first.
    #[must_use]
    pub fn custom<T: Backend + 'static>(mut self, backend: T) -> Self {
        self.custom
            .retain(|existing| !(&**existing as &dyn Any).is::<T>());
        self.custom.push(Box::new(backend));
        self
    }

    /// Finish. Anything unspecified is Null — **except the clock, which is
    /// [`clock::SystemClock`]** (the device default). For a fixed time use [`Services::null`] or
    /// `.clock(NullClock)`.
    #[must_use]
    pub fn build(self) -> Services {
        Services {
            clock: self
                .clock
                .unwrap_or_else(|| Box::new(clock::SystemClock::default())),
            power: self.power.unwrap_or_else(|| Box::new(null::NullPower)),
            wifi: self.wifi.unwrap_or_else(|| Box::new(null::NullWifi)),
            bluetooth: self
                .bluetooth
                .unwrap_or_else(|| Box::new(null::NullBluetooth)),
            display: self.display.unwrap_or_else(|| Box::new(null::NullDisplay)),
            audio: self.audio.unwrap_or_else(|| Box::new(null::NullAudio)),
            network: self.network.unwrap_or_else(|| Box::new(null::NullNetwork)),
            info: self.info.unwrap_or_else(|| Box::new(null::NullInfo)),
            custom: self.custom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{null::NullClock, Backend, Services, Waker};
    use std::time::{Duration, Instant};

    struct Gpio {
        level: bool,
        due: Option<Instant>,
        attached: u32,
    }

    impl Gpio {
        fn new(level: bool) -> Self {
            Self {
                level,
                due: None,
                attached: 0,
            }
        }
    }

    impl Backend for Gpio {
        fn attach(&mut self, _waker: Waker) {
            self.attached += 1;
        }

        fn next_wake(&self) -> Option<Instant> {
            self.due
        }
    }

    struct Scale;

    impl Backend for Scale {}

    struct Absent;

    impl Backend for Absent {}

    /// Found by its type, one per type: the second `Gpio` replaces the first.
    #[test]
    fn custom_backends_are_found_by_type_and_replaced_by_type() {
        let mut services = Services::builder()
            .custom(Gpio::new(false))
            .custom(Scale)
            .custom(Gpio::new(true))
            .build();
        assert!(services.custom::<Gpio>().is_some_and(|gpio| gpio.level));
        assert!(services.custom::<Scale>().is_some());
        assert!(services.custom::<Absent>().is_none());
        if let Some(gpio) = services.custom_mut::<Gpio>() {
            gpio.level = false;
        }
        assert!(services.custom::<Gpio>().is_some_and(|gpio| !gpio.level));
    }

    /// A custom backend takes part like a built-in one: it gets the `Waker`, and its `next_wake`
    /// is the frame's earliest when nothing else wants waking.
    #[test]
    fn a_custom_backend_is_attached_and_its_wake_counts() {
        let now = Instant::now();
        let due = now + Duration::from_secs(5);
        let mut services = Services::builder()
            .clock(NullClock)
            .custom(Gpio {
                due: Some(due),
                ..Gpio::new(false)
            })
            .build();
        services.attach(&Waker::new(&egui::Context::default()));
        assert_eq!(services.custom::<Gpio>().map(|gpio| gpio.attached), Some(1));
        assert_eq!(services.poll(now), Some(due));
    }
}
