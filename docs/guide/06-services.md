# 06. Services — wiring real hardware

## 0. The rules

Screens and chrome know nothing about hardware. The Wi-Fi screen sees a
`WifiBackend` trait and cannot tell whether `nmcli`, D-Bus or a mock sits behind
it. Four rules hold that up.

1. **The UI only sees traits.** Concrete types are chosen once, when you assemble
   `Services`.
2. **Snapshot plus command.** Reads go through `snapshot()` — a cheap clone of a
   cache. Writes are commands that return immediately; the result arrives in the
   next snapshot, and the `Waker` tells the UI to look.
3. **Channels, never locks.** No `Mutex` or `RwLock` inside a backend. If there is
   a worker thread, it gets one pair of `mpsc` channels (commands out, results in)
   and the UI-side `poll()` only ever calls `try_recv`.
4. **Never block the UI thread.** `poll()`, `poll_at()` and every command method
   (`set_enabled`, `scan`, `connect`, …) must return immediately. Anything slow
   belongs to a worker thread or to a state machine inside the backend.

This page covers the traits, the implementations and the assembly that exist in the crate
today.

**The implementations are yours.** fairing ships the traits, a `Null*` default for each
and a `Mock*` simulator — no system code. Your device speaks sysfs, a
vendor SDK, D-Bus or a PLC protocol; you write the backend against the trait, and
anything the shell has no slot for goes in as a custom backend (§5.1).

## 1. The traits

They live in `fairing::services` and all have `Backend` as a supertrait.

```rust,ignore
pub trait Backend {
    fn attach(&mut self, waker: Waker) {}                 // once, at start-up
    fn poll(&mut self) {}                                 // every frame
    fn poll_at(&mut self, now: Instant) { self.poll(); }  // what actually gets called
    fn next_wake(&self) -> Option<Instant> { None }       // when to be woken next
    fn capabilities(&self) -> Capabilities { Capabilities::NONE }
}
```

The `now` in `poll_at` is **the shell's monotonic clock** (`Shell::now()`), not
`Instant::now()`. Headless tests advance that clock virtually, so a time-based
simulation — a scan delay, say — can be verified with no `sleep`. The flip side:
a backend that calls `Instant::now()` internally throws that away.

Eight traits, one for each kind of hardware the built-in UI shows:

| Trait | What it covers | Key methods |
|---|---|---|
| `ClockSource` | Wall clock | `now() -> WallTime`, and optionally `set_time(utc_secs) -> ServiceResult` (defaults to `Unsupported`) |
| `PowerBackend` | Battery and power | `battery() -> Option<BatterySnapshot>`, `request(PowerRequest) -> ServiceResult` (`Reboot`, `Shutdown`, `Suspend`) |
| `WifiBackend` | Wi-Fi | `snapshot() -> WifiSnapshot`, `enabled()`, `strength()`, `set_enabled`, `scan`, `connect(ssid, psk, hidden)`, `disconnect`, `forget(ssid)` |
| `BluetoothBackend` | Bluetooth | `snapshot() -> BtSnapshot`, `enabled()`, `any_connected()`, `set_enabled`, `set_discovering`, `pair`, `respond_pairing`, `connect`, `remove` |
| `DisplayBackend` | Brightness and screen power | `brightness() -> Option<u8>`, `set_brightness`, `set_idle_inhibit(bool)`, `idle_inhibited() -> Option<bool>`, `set_power`, `info() -> DisplayInfo` |
| `AudioBackend` | Volume and cues | `volume() -> Option<AudioSnapshot>`, `set_volume(u8)`, `set_muted(bool)`, and optionally `play_cue(Cue)` |
| `NetworkBackend` | Interfaces, addresses, hostname | `interfaces() -> Vec<IfaceSnapshot>`, `ethernet_up() -> Option<bool>`, and optionally `configure(iface, IpConfig)`, `hostname()`, `set_hostname` |
| `InfoBackend` | What the device says about itself | `device() -> DeviceInfo` — model, serial, firmware, OS, uptime, each optional |

The default `enabled()`, `strength()` and `any_connected()` clone `snapshot()` and
pick a field out of it — a heap allocation. The status bar asks for those **every
frame**, so a real backend is better off implementing all three directly.

`DisplayBackend` drives `keep_awake` (idle inhibition, see
`ChromePolicy::keep_awake` in [03 Chrome](03-chrome.md)), the `tile.brightness`
slider and `status.brightness`. `set_power` is on the trait but nothing in the shell
calls it yet. `AudioBackend` drives `settings.sound` and `status.volume`, and is asked
for `Cue::Notify` when a new notification arrives (unless `ui.silent` is on).
`NetworkBackend` drives `status.ethernet` — `ethernet_up()` is asked every frame, so
answer it from a cache — and `settings.network`: the interface cards, an IPv4 form (DHCP or a
manual address, checked with `std::net` before `configure` is called) and the hostname, the
editing behind the `settings.network.edit` gate. The screen is registered when the backend
reports `Capabilities::ETHERNET`. `InfoBackend` fills `settings.about`.

Wi-Fi and Bluetooth state types, in brief:

```rust,ignore
pub enum WifiState { Off, Idle, Scanning, Connecting, Connected { ssid: String, strength: u8 }, Failed { reason: String } }
pub struct WifiSnapshot { pub enabled: bool, pub state: WifiState, pub networks: Vec<Network>, pub known: Vec<String> }
pub struct BtSnapshot { pub enabled: bool, pub discovering: bool, pub devices: Vec<BtDevice>, pub pending: Option<PairingRequest> }
```

A PSK handed to `connect` is the backend's business and ends there — neither the
shell nor any UI state keeps a copy.

### Connecting is yours

The built-in `settings.wifi` screen **shows the networks and does not connect to
them.** The radio toggle and the scan button are the screen's, because neither
carries a secret nor a choice. A tap on a network row leaves as an event and
nothing else happens:

```rust
# use fairing::ShellEvent;
# struct App { password_sheet: Option<String> }
# impl App {
# fn on_events(&mut self, shell: &mut fairing::Shell) {
for event in shell.poll_events() {
    if let ShellEvent::WifiNetworkTapped { ssid, secured, known } = event {
        if secured && !known {
            self.password_sheet = Some(ssid);   // your own sheet, your own buffer
        } else {
            let _ = shell.services_mut().wifi.connect(&ssid, None, false);
        }
    }
}
# }
# }
```

**Take this event or the list does nothing when tapped.** That is the one thing
to remember here, and it is deliberate: whether to ask for a password, what to
ask with, where the profile is stored and whether a saved network should
reconnect on a single tap are decisions about your device, not about a shell.
Nothing stops you from connecting on every tap if that suits the machine.

It also keeps the secret away from the crate entirely. `connect` takes
`psk: Option<&str>` and [`TextField`](04-customization.md) borrows a
`&mut String`, so the buffer is yours from end to end — hold it in a zeroizing
type if you want one, and reserve its capacity up front so growing it does not
leave an unzeroed copy behind:

```rust,ignore
let mut psk = zeroize::Zeroizing::new(String::with_capacity(64)); // WPA2 tops out at 63
TextField::new(&mut psk).password(true).hint("Password").show(ui, &mut cx.widgets());
```

`fairing` does not depend on `zeroize` and does not need to — it never owns the
string. What it does owe you is that the widget forgets: `TextField::password(true)`
clears the `TextEdit` undo history every frame, because egui otherwise keeps up
to a hundred plaintext snapshots in its own memory under the widget's id, where
no wrapper of yours can reach them.

**Holding** a row reports separately, and that is where forgetting a network
goes:

```rust
# use fairing::ShellEvent;
# fn on_event(shell: &mut fairing::Shell, event: ShellEvent) {
# match event {
ShellEvent::WifiNetworkLongPressed { ssid, known } if known => {
    let _ = shell.services_mut().wifi.forget(&ssid);
}
# _ => {}
# }
# }
```

One press never means two things — the release that ends a hold does not also
arrive as `WifiNetworkTapped`.

Adding a hidden network needs nothing from the crate at all: collect the SSID
and the password in your own screen and call
`shell.services_mut().wifi.connect(&ssid, psk, true)`.

### Pairing a Bluetooth device

Here the crate does draw the interaction, because there is no secret in it. The
backend raises the request and picks the passkey; the screen shows the number so
it can be checked against the other device, and Confirm answers with
`respond_pairing(true, None)`:

```rust,ignore
pub struct PairingRequest { pub addr: String, pub passkey: Option<u32> }
// -> BtSnapshot::pending, drawn by settings.bluetooth
fn respond_pairing(&mut self, accept: bool, passkey: Option<u32>) -> ServiceResult;
```

Showing a number and taking "yes, that matches" collects nothing. A PIN the user
has to **type** is the other case, and that one is yours the same way a Wi-Fi
PSK is — collect it in your own sheet and pass it as the `passkey` argument.

## 2. `Capabilities` — declaring what works

```rust,ignore
pub struct Capabilities(pub u32);
impl Capabilities {
    pub const NONE: Self;
    pub const BATTERY: Self;
    pub const POWER_CONTROL: Self;
    pub const WIFI: Self;
    pub const BLUETOOTH: Self;
    pub const CLOCK_SET: Self;
    pub const VOLUME: Self;       // an AudioBackend reports it: status.volume, settings.sound
    pub const BRIGHTNESS: Self;   // a DisplayBackend reports it: status.brightness, the tile
    pub const ETHERNET: Self;     // a NetworkBackend reports it: status.ethernet
}
```

Anything backed by `Capabilities::NONE` — every `Null*` backend except the clock —
disappears from the UI. It is treated as absence, not as an error. So with the `Null`
defaults `status.ethernet`, `status.volume` and `status.brightness` stay hidden
whatever you put in `[status_bar]`, and they appear once your backend reports the bit.

## 3. Assembling `Services`

```rust
fn null_everything() -> fairing::Services {
    // Even the clock is a fixed epoch - for headless tests that must not see the wall clock.
    fairing::Services::null()
}

fn defaults() -> fairing::Services {
    fairing::Services::builder().build() // real clock, null everything else
}
```

The builder swaps in only what you name; everything else stays `Null*` — **except
the clock**, which defaults to `clock::SystemClock`, because a device booting
without a clock is the stranger outcome. For a deterministic fixed time, either
say `.clock(NullClock)` or use `Services::null()`.

```rust
use fairing::services::clock::SystemClock;
use fairing::services::mock::{MockBluetooth, MockDisplay, MockPower, MockWifi};

fn mixed() -> fairing::Services {
    fairing::Services::builder()
        .clock(SystemClock::with_offset(540)) // UTC+9, no DST - see §8
        .power(MockPower::new(85, false))
        .wifi(MockWifi::new())
        .bluetooth(MockBluetooth::new())
        .display(MockDisplay::new(70))
        .build()
}
```

## 4. Developing against mocks

The `mock` feature (on by default) puts five simulators in
`fairing::services::mock`: `MockClock`, `MockPower`, `MockWifi`, `MockBluetooth`
and `MockDisplay`. All of them are **channel-based and lock-free** — each hands
out a `MockControl<M>` (an `mpsc::Sender` wrapper, `Clone + Send`), a scenario
thread or a test posts messages to it, and the backend's `poll()` drains them with
`try_recv`.

```rust
use fairing::services::mock::{MockPower, MockWifi, WifiMsg};
use fairing::services::{Services, WifiState};

fn scenario() -> (Services, fairing::services::mock::MockControl<WifiMsg>) {
    let wifi = MockWifi::new();
    let control = wifi.control(); // Clone + Send - hand it to another thread if you like

    let mut power = MockPower::new(85, false);
    power.drain_per_min = 3; // 3 % a minute; it schedules its own repaints through next_wake

    let services = Services::builder().wifi(wifi).power(power).build();

    // Inject state from anywhere - a test body, a scenario thread.
    control.send(WifiMsg::State(WifiState::Connected {
        ssid: "demo-ap".to_owned(),
        strength: 3,
    }));

    (services, control)
}
```

`fairing::services::mock::services()` bundles a running clock, a 72 % battery,
Wi-Fi, Bluetooth and a display at 70 % brightness in one call — handy for demos.

**Deterministic time.** Time-based scenarios such as scan and connect delays never
touch `Instant::now()`. The first `poll_at(now)` fixes the deadline at
`now + delay`, and later `poll_at` calls compare against the time they are given.
A headless test only has to advance virtual time frame by frame to reproduce "the
scan finishes 50 ms later" with no `sleep` — `mock_wifi_scan_completes_via_next_wake`
in `tests/m1_services.rs` is the worked example. The backend reports the
deadline through `next_wake()`, the shell arms `request_repaint_after` for it, and
no worker or polling loop is needed.

The real pattern for waking the UI from another thread is in `examples/demo.rs` —
a scenario thread that jitters the Wi-Fi strength every four seconds.

```rust
# use std::time::Duration;
# use fairing::services::mock::{MockWifi, WifiMsg};
# use fairing::services::WifiState;
# fn spawn(shell: &fairing::Shell, wifi: &MockWifi) {
# let wifi_control = wifi.control();
let waker = shell.handle().waker().clone(); // Clone + Send
std::thread::spawn(move || loop {
    std::thread::sleep(Duration::from_secs(4));
    // ... work out the new strength ...
#   let new_state = WifiState::Idle;
    if !wifi_control.send(WifiMsg::State(new_state)) {
        break; // the backend is already gone
    }
    waker.wake(); // ask the UI thread for another frame
});
# }
```

That thread is allowed to `sleep` — **it is not the UI thread**. What is banned is
blocking the UI thread inside `poll`, `poll_at` or a command method, not sleeping
in a worker.

## 5. Writing a real backend

An implementation that keeps to §0 usually looks like "a worker thread, two
channel pairs, and a `Waker`". The backend struct lives on the UI thread; the
heavy work — reading device files, shelling out to `nmcli`, calling D-Bus — is the
worker's.

```rust
use fairing::services::{
    Backend, Capabilities, ServiceError, ServiceResult, Waker, WifiBackend, WifiSnapshot, WifiState,
};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Instant;

enum FromWorker {
    Snapshot(WifiSnapshot),
}

enum ToWorker {
    SetEnabled(bool),
    Scan,
    Connect { ssid: String, psk: Option<String>, hidden: bool },
    Disconnect,
    Forget(String),
}

pub struct RealWifi {
    latest: WifiSnapshot,
    from_worker: Receiver<FromWorker>,
    to_worker: Sender<ToWorker>,
}

impl RealWifi {
    #[must_use]
    pub fn spawn() -> Self {
        let (to_worker_tx, to_worker_rx) = mpsc::channel::<ToWorker>();
        let (from_worker_tx, from_worker_rx) = mpsc::channel::<FromWorker>();

        thread::spawn(move || {
            // This is the one thread allowed to block - nmcli, D-Bus and driver calls go here.
            while let Ok(cmd) = to_worker_rx.recv() {
                let snapshot = match cmd {
                    ToWorker::SetEnabled(on) => WifiSnapshot { enabled: on, ..WifiSnapshot::default() },
                    ToWorker::Scan => WifiSnapshot { state: WifiState::Scanning, ..WifiSnapshot::default() },
                    ToWorker::Connect { ssid, .. } => WifiSnapshot {
                        state: WifiState::Connected { ssid, strength: 3 },
                        ..WifiSnapshot::default()
                    },
                    ToWorker::Disconnect | ToWorker::Forget(_) => WifiSnapshot::default(),
                };
                if from_worker_tx.send(FromWorker::Snapshot(snapshot)).is_err() {
                    break; // the UI-side RealWifi was dropped
                }
                // In production, also call the Waker::wake() you kept from attach().
            }
        });

        Self {
            latest: WifiSnapshot::default(),
            from_worker: from_worker_rx,
            to_worker: to_worker_tx,
        }
    }

    fn send(&self, cmd: ToWorker) -> ServiceResult {
        self.to_worker
            .send(cmd)
            .map_err(|_| ServiceError::new(fairing::services::ErrorKind::Io, "the worker died"))
    }
}

impl Backend for RealWifi {
    fn attach(&mut self, _waker: Waker) {
        // Hand waker.clone() to the worker thread and it can wake the UI the moment
        // state changes - no polling latency.
    }

    fn poll(&mut self) {
        while let Ok(FromWorker::Snapshot(s)) = self.from_worker.try_recv() {
            self.latest = s;
        }
    }

    fn poll_at(&mut self, _now: Instant) {
        self.poll(); // a backend that ignores time need not implement poll_at at all
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::WIFI
    }
}

impl WifiBackend for RealWifi {
    fn snapshot(&self) -> WifiSnapshot {
        self.latest.clone()
    }
    fn set_enabled(&mut self, on: bool) -> ServiceResult {
        self.send(ToWorker::SetEnabled(on))
    }
    fn scan(&mut self) -> ServiceResult {
        self.send(ToWorker::Scan)
    }
    fn connect(&mut self, ssid: &str, psk: Option<&str>, hidden: bool) -> ServiceResult {
        self.send(ToWorker::Connect { ssid: ssid.to_owned(), psk: psk.map(str::to_owned), hidden })
    }
    fn disconnect(&mut self) -> ServiceResult {
        self.send(ToWorker::Disconnect)
    }
    fn forget(&mut self, ssid: &str) -> ServiceResult {
        self.send(ToWorker::Forget(ssid.to_owned()))
    }
}
```

Three rules to keep:

1. **`RealWifi` itself, on the UI thread, has no lock.** `latest` changes only in
   `poll()`, and only the shell calls that, only on the UI thread.
2. **Command methods post to the channel and return.** `connect()` does not wait
   for the connection; completion shows up as a changed `snapshot()` after the
   next `poll()`.
3. **When the worker changes state, call `Waker::wake()`.** Without it the screen
   does not refresh until the next input — egui repaints reactively by default
   (see `repaint` in [01 Getting started](01-getting-started.md)).

Plug it in with `Services::builder().wifi(RealWifi::spawn()).build()`. Not one
line of the Wi-Fi screen changes.

### 5.1 Anything else: a custom backend

A door switch, a card reader, a PLC link, a scale — the shell has no slot for them,
and it does not need one. `ServicesBuilder::custom` takes any `Backend`, and from then
on it is treated like the built-in ones: `attach` hands it the `Waker` at start-up,
`poll_at` runs every frame and `next_wake` arms the next repaint. A screen reaches it
by its type with `cx.services.custom::<T>()` (or `custom_mut` to send it a command).
One per type — a second of the same type replaces the first.

```rust
use fairing::inbox::Inbox;
use fairing::services::{Backend, Waker};
use fairing::{screen, Cx, Services, Shell};
use std::sync::mpsc::Sender;

/// A door switch on a GPIO line, watched by a worker thread.
pub struct DoorSensor {
    edges: Inbox<bool>,          // the UI side can only `try_recv` — it cannot block
    worker: Option<Sender<bool>>,
    open: bool,
}

impl DoorSensor {
    pub fn new() -> Self {
        let (worker, edges) = Inbox::pair();
        Self { edges, worker: Some(worker), open: false }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }
}

impl Backend for DoorSensor {
    // The worker starts once there is a Waker to hand it.
    fn attach(&mut self, waker: Waker) {
        if let Some(tx) = self.worker.take() {
            std::thread::spawn(move || {
                // ... block on the GPIO line here; on each edge:
                if tx.send(true).is_ok() {
                    waker.wake(); // ask the UI thread for a frame
                }
            });
        }
    }

    fn poll(&mut self) {
        for open in self.edges.drain() {
            self.open = open;
        }
    }
}

fn services() -> Services {
    Services::builder().custom(DoorSensor::new()).build()
}

fn add_door_screen(shell: &mut Shell) {
    shell.add(screen("door", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        if let Some(door) = cx.services.custom::<DoorSensor>() {
            ui.label(if door.is_open() { "Door open" } else { "Door closed" });
        }
    }));
}
```

A status bar item or a tile of your own reads it the same way, through its `cx`.

## 6. Hiding what the hardware cannot do

The status bar and the tiles ask `capabilities()` every frame and decide whether
to draw.

```rust,ignore
// the status bar, from measure_builtin in crates/fairing/src/chrome/status_bar.rs
StatusItem::Wifi => parts.services.wifi.capabilities().contains(Capabilities::WIFI).then_some(style.size),
StatusItem::Battery => parts.services.power.battery().map(|_| style.size * 1.4), // the value itself decides
```

Battery presence is decided by `battery()` returning `Some`, not by
`Capabilities::BATTERY`. A mains-only device with no battery still reports
`POWER_CONTROL` for reboot and shutdown, and the battery item simply does not
appear.

For "usable if supported, dimmed if not" — the rotation-lock tile — pull it into a
helper:

```rust,ignore
pub fn rotation_lock_available(display: Capabilities) -> bool {
    display != Capabilities::NONE
}
```

Report exactly what your backend supports. Over-report and the UI appears but
every command fails with `Unsupported`; under-report and working features hide.

## 7. Reading and writing settings

```rust,ignore
pub struct SettingKey(pub Cow<'static, str>);   // "display.brightness", "app.heater" …
#[non_exhaustive]                                // match with a `_` arm: more kinds may come
pub enum SettingValue { Bool(bool), Int(i64), Float(f64), Text(String) }
```

Built-in keys are constants in `fairing::settings::keys`. Put your own under
`app.*` so they cannot collide.

| Constant | String | Backend it reaches |
|---|---|---|
| `keys::WIFI_ENABLED` | `"wifi.enabled"` | `WifiBackend::set_enabled` |
| `keys::BLUETOOTH_ENABLED` | `"bluetooth.enabled"` | `BluetoothBackend::set_enabled` |
| `keys::DISPLAY_BRIGHTNESS` | `"display.brightness"` | `DisplayBackend::set_brightness` (0..=100) |
| `keys::RADIO_AIRPLANE` | `"radio.airplane"` | Inverts Wi-Fi and Bluetooth together |
| `keys::THEME_DARK` | `"theme.dark"` | The theme crossfade — no backend, internal to the shell |
| `keys::AUDIO_VOLUME` | `"audio.volume"` | `AudioBackend::set_volume` (0..=100) |
| `keys::AUDIO_MUTED` | `"audio.muted"` | `AudioBackend::set_muted` |
| `keys::UI_SILENT` | `"ui.silent"` | None; a new notification asks for no `Cue::Notify` while it is on |
| `keys::DISPLAY_ROTATION_LOCK` | `"display.rotation_lock"` | None; §6's `rotation_lock_available` only controls how it draws |
| `keys::UI_CLOCK_12H` | `"ui.clock_12h"` | None; **the status bar clock reads it** — see §8 |

Write through `ShellHandle::set_setting`, from any thread.

```rust
fn set_brightness(shell: &fairing::Shell) {
    use fairing::settings::{keys, SettingValue};
    shell.handle().set_setting(keys::DISPLAY_BRIGHTNESS, SettingValue::Int(80));
}
```

**The gate is the key name.** Assign `[access.gates] "display.brightness" = "operator"`
and anything below that level cannot change the brightness — the value stays put
and you get
`ShellEvent::Access(AccessEvent::UnlockRequested { gate, then: Some(LaunchAction::Set(key, value)) })`
instead ([05 §2.3](05-access-control.md#23-where-enforcement-happens)).

A write that passes the gate does three things in order: update the in-memory
`SettingsView`, call the matching backend if it is a built-in key
(`apply_setting_to_backend`, the table above), and emit
`ShellEvent::SettingChanged { key, value }`.

Read inside a screen with `cx.settings.get(&key)`; `Cx::settings` is a read-only
view.

### 7.1 Keeping settings across a restart

**The shell writes no files.** Where a device keeps its settings — a file, a database, an
EEPROM, a PLC — and how often and how safely it writes them is the device's business. The shell hands you both ends:

- **Saving**: every change arrives as `ShellEvent::SettingChanged { key, value }`. Store it.
- **Restoring**: `Shell::restore_settings` takes what you stored and puts it back — in the
  table, at the backends, in the theme and the language. It checks no gate, since the values
  passed one when they were set, and emits no `SettingChanged`, since they are stored already.
  The theme switches at once, without the crossfade, and `radio.airplane` is applied after the
  rest and only ever turns the radios off.

```rust
use fairing::settings::{SettingKey, SettingValue};
use fairing::{Shell, ShellEvent};

/// Where the device keeps its settings — a file, a database, an EEPROM.
struct Store;

impl Store {
    fn save(&mut self, _key: &SettingKey, _value: &SettingValue) {}
    fn load(&self) -> Vec<(SettingKey, SettingValue)> {
        Vec::new()
    }
}

/// Once, right after the shell is built.
fn restore(shell: &mut Shell, store: &Store) {
    shell.restore_settings(store.load());
}

/// Every frame, with the rest of your event handling.
fn on_events(shell: &mut Shell, store: &mut Store) {
    for event in shell.poll_events() {
        if let ShellEvent::SettingChanged { key, value } = event {
            store.save(&key, &value);
        }
    }
}
```

Two things to mind on the saving side:

- **A slider sends one event per step.** Writing the file on every event writes it dozens of
  times a drag; keep the latest value per key and write after a pause.
- **Not on the UI thread.** File IO inside the frame stalls it. Hand the write to a thread of
  your own over a channel.

## 8. Clocks and time zones

```rust,ignore
pub struct WallTime { pub utc_secs: u64, pub offset_min: i32 }  // local = UTC + offset_min minutes
```

There are two clock backends.

| Backend | Feature | Behaviour |
|---|---|---|
| `clock::SystemClock` | always | `SystemTime` plus a **fixed offset you supply**. No dependencies, and **no DST** — in a region with summer time it is an hour out twice a year unless you update the offset yourself |
| `clock::ChronoClock` | `chrono` | Asks the OS timezone database for the offset **at that moment**, so March and November correct themselves. Costs three crates in the tree |

```rust
use fairing::services::clock::SystemClock;

fn kst_clock() -> SystemClock {
    SystemClock::with_offset(540) // UTC+9, in minutes
}
```

```rust
use fairing::services::clock::ChronoClock;   // needs features = ["chrono"]

let services = fairing::Services::builder().clock(ChronoClock::new()).build();
```

The offset has no `fairing.toml` key — pass it in code. Neither clock implements
`ClockSource::set_time`, so both return the default `Err(Unsupported)`: changing
the system clock is a privilege question, and that belongs to your own backend.

### 8.1 Connecting the time source later

`Services::builder().clock(..)` is the connection before the shell exists.
`Shell::set_clock` is the same connection afterwards, for a device that learns
its time from a PLC, a GPS receiver or an NTP sync rather than at startup. The
status bar, the shade and `settings.datetime` all read `services.clock`, so one
call moves them together.

```rust
fn go_to_kst(shell: &mut fairing::Shell) {
    use fairing::services::clock::SystemClock;
    shell.set_clock(SystemClock::with_offset(540));
}
```

The shell never sets the clock itself — it asks yours what the time is.

### 8.2 12- or 24-hour

Two separate decisions, deliberately.

| Decision | Whose | Where |
|---|---|---|
| The clock's **shape** — minutes, seconds, the date in front | The integrator's; it is part of how the status bar is laid out | `[status_bar] clock_format`: `"hm"`, `"hms"`, `"date_hm"` and the 12-hour spellings `"hm12"`, `"hms12"`, `"date_hm12"` |
| The **hour convention** — 12 or 24 | The device owner's, and it changes while the shell runs | The `keys::UI_CLOCK_12H` setting, which the built-in `settings.datetime` screen's "24-hour time" switch writes |

The status bar takes the shape you configured and applies the setting to it each
frame (`ClockFormat::with_hour12`), so `"hms"` becomes `"9:30:00 AM"` the moment
someone throws the switch, and back again. Write the setting yourself to follow a
device policy:

```rust
fn use_12_hour(shell: &fairing::Shell) {
    use fairing::settings::{keys, SettingValue};
    shell.handle().set_setting(keys::UI_CLOCK_12H, SettingValue::Bool(true));
}
```

For headless tests and deterministic demos, use `MockClock`.

```rust
use fairing::services::mock::MockClock;
use fairing::services::WallTime;

fn fixed_2024() -> MockClock {
    MockClock::fixed(WallTime { utc_secs: 1_704_069_000, offset_min: 540 }) // stopped
}

fn running_2024() -> MockClock {
    MockClock::running(WallTime { utc_secs: 1_704_069_000, offset_min: 540 }) // follows the shell clock
}
```

`fixed` never moves; `running` starts at the time of the first `poll_at` and
follows the shell's monotonic clock. Either one can be jumped to an arbitrary time
by sending `ClockMsg::Set(WallTime)` to the `MockControl<ClockMsg>` from
`control()`.
