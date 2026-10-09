# 01. Getting started

From installation to your first screen. Follow this page to the end and you have a
desktop with two icons and a shell that moves between them.

`fairing` is a shell that sits on top of egui. It owns the status bar, the shade,
the desktop, the navigation bar and the gestures; you draw the inside of each
screen with egui. How it is put together is in
[architecture](../architecture.md).

## 1. Requirements

| Item | Value | Notes |
|---|---|---|
| Rust | 1.95 or newer | The crates' MSRV (`rust-version`). To work on fairing itself, `rust-toolchain.toml` at the repository root pins 1.99 and rustup fetches it |
| Edition | 2021 | |
| egui | `0.36` | `fairing` asks for egui 0.36 (any 0.36.x) and re-exports it as `fairing::egui`, so depending on egui directly is **optional** — if you do, ask for the same minor, `egui = "0.36"` |
| OS | Linux | The main target is Ubuntu Core with `ubuntu-frame` (a Wayland kiosk); `cage` and `weston` are meant to work too. None of the three has been tried on a real device yet — development and CI run under X11 |
| Graphics | OpenGL ES 2 or newer (Mesa/EGL) | The runner uses eframe's `glow` backend. wgpu is not used |

If you use the runner, a feature picks the windowing backend.

| Environment | Feature |
|---|---|
| Device (Wayland kiosk) | `runner` |
| Desktop development, X11 included | `runner-x11` — a superset of `runner` plus `eframe/x11`, so the Wayland backend is still in |

After installing `rustup`, check the repository once:

```sh
rustup show                     # installs and selects 1.99 from rust-toolchain.toml
cargo test -p fairing --all-features
```

No Korean font ships with the crate — they run to several megabytes and subsetting
is a policy call for the integrator. egui's `default_fonts` has no CJK, so Hangul
renders as tofu until you add a font with `ShellBuilder::fonts`; see
[08 §6](08-troubleshooting.md#6-korean-renders-as-tofu). Korean **input** (the
two-set layout) is a separate switch: `[osk] layout = "hangul"`.

## 2. Adding it to Cargo.toml

`fairing` is not published on crates.io, so depend on it by path or git.

```toml
[dependencies]
# not on crates.io until 0.1.0 is published, so from git for now
fairing = { git = "https://github.com/shim9610/fairing", features = ["runner"] }
# or the repository checked out next door
# fairing = { path = "../fairing/crates/fairing", features = ["runner"] }

# egui itself is optional.
# Leave it out and use `fairing::egui` - a version mismatch becomes impossible.
# Put it in and it must be the same minor as fairing's, or you link two copies
# of egui and the types stop matching.
# egui = "0.36"
```

Screen closures take `&mut egui::Ui`, so the egui types have to be reachable.
Either of these works:

```rust
use fairing::egui;          // the re-export - nothing to mismatch
// use egui;                // a direct dependency, with `egui = "0.36"` in Cargo.toml
```

### Feature list

Straight from `crates/fairing/Cargo.toml`.

| Feature | Default | What it brings in |
|---|:---:|---|
| `mock` | ✓ | `fairing::services::mock` — `MockClock`, `MockPower`, `MockWifi`, `MockBluetooth`, `MockDisplay`. Used by the examples and the headless tests |
| `overlay` | ✓ | `fairing::overlay` — the pull-down shade, quick-settings tiles, the scrim. The `tile()` declaration lives here too |
| `osk` | ✓ | `fairing::osk` — the on-screen keyboard (numpad, QWERTY, Hangul), `TextEdit` injection, the bottom inset |
| `brand` | ✓ | `fairing::brand` — the manta mark (a ribbon mesh) and the Abyss procedural background. **Turning it off is not how you remove the branding** — branding is opt-in, so not switching it on is enough ([09](09-branding.md)) |
| `settings` | ✓ | The eleven built-in settings screens and `settings::add_all` ([§4.1](#41-the-built-in-settings-screens)). The setting keys and values screens use are there either way |
| `runner` | | `fairing::runner` — an eframe-based fullscreen bootstrap (Wayland + glow) |
| `runner-x11` | | `runner` plus `eframe/x11` |
| `chrono` | | `fairing::services::clock::ChronoClock` — a clock that asks the OS timezone database every frame, so daylight saving corrects itself. Costs three crates in the tree; the default `SystemClock` is a fixed offset and does not know about DST |

The default set is `["mock", "overlay", "osk", "brand", "settings"]`. A device with a hardware
keyboard and no need for a shade can drop them all:

```toml
fairing = { path = "…", default-features = false, features = ["runner"] }
```

A subsystem you switch off leaves the build entirely. The `fairing::overlay` and
`fairing::osk` paths disappear along with the `tile()` declaration, so code that
uses them needs a `#[cfg(feature = "overlay")]` or has to go.

## 3. The device binary profile

The `fairing` workspace has no `[profile.release]`. Cargo takes the profile from
the root package, so anything a library writes never reaches the integrator's
binary. Put it in the `Cargo.toml` of whatever you ship:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
opt-level = 3      # for a UI, 3 beats "s"; strip wins the size back
```

## 4. A minimal app

Config → `Services` → `Shell` → screen declarations → run. The shell is built
**after** the window exists: `Shell::new` takes an `egui::Context` and wraps it in
a `Waker` so other threads can wake the UI
([architecture §5](../architecture.md#5-threads)).

```rust,no_run
use fairing::egui; // the re-export, as §2 says - no egui line in Cargo.toml needed
use fairing::runner::{self, Options};
use fairing::{icon, screen, Cx, Services, Shell, ShellConfig};

fn main() -> fairing::Result<()> {
    // 1) Config. A missing file is not an error - it means defaults.
    let config = ShellConfig::load("/etc/mydevice/fairing.toml")?;

    // 2) Window options. Ship devices with Options::default() (fullscreen);
    //    only pass a size when you are looking at it on a desktop.
    let options = Options {
        fullscreen: false,
        title: "mydevice".to_owned(),
        size: Some((1024.0, 600.0)),
    };

    // 3) Build the shell once the window exists. The runner hands you ctx.
    runner::run_shell(options, move |ctx| {
        // For Korean, add a font with Shell::builder(..).fonts(..) - see 08 §6.
        let services = Services::builder().build(); // real clock, null everything else
        let mut shell = Shell::new(config, services, ctx)?;

        shell.add(
            screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx| {
                ui.heading("Dashboard");
                if ui.button("counter").clicked() {
                    cx.open("counter");
                }
            })
            .title("Dashboard")
            .icon(icon::GAUGE)
            .desktop(),
        );

        shell.add(
            screen("counter", |ui: &mut egui::Ui, cx: &mut Cx| {
                ui.heading("Counter");
                if ui.button("back").clicked() {
                    cx.finish();
                }
            })
            .title("Counter")
            .icon(icon::ACTIVITY)
            .desktop(),
        );

        Ok(shell)
    })
}
```

What each piece does:

| Piece | What it does | More |
|---|---|---|
| `ShellConfig::load(path)` | Reads `fairing.toml`. A missing file means defaults; a bad value is `Error::Config` | [07 Config reference](07-config-reference.md) |
| `Services::builder().build()` | The backend bundle. Only the clock is real (`SystemClock`); power, Wi-Fi, Bluetooth and display are `Null` | [06 Services](06-services.md) |
| `Shell::new(config, services, ctx)` | A thin wrapper over `Shell::builder(config).services(services).build(ctx)` | [04 Customization](04-customization.md) |
| `screen(id, closure)` | One screen declaration. The closure *is* the screen | [02 Screens](02-screens.md) |
| `.icon(..).desktop()` | Puts it on the desktop. Without an icon it does not go up | [02 Screens](02-screens.md) |
| `cx.open(id)` / `cx.finish()` | Push onto the stack / pop yourself | [02 Screens](02-screens.md) |

The imports can be one line: `use fairing::prelude::*;` brings in the names above and the others
most apps reach for — the declarations, `Cx`, the events, the palette roles, the built-in icons,
the `layout` and `widgets` modules and `egui` itself. It leaves out `fairing::Result`, whose one
parameter would hide the standard `Result`. `examples/hello.rs` is this app with the prelude, the
built-in settings, a status bar item and a nav bar item, standing alone so you can copy it.

### 4.1 The built-in settings screens

`cx.open("settings.home")` opens a screen with that id, so one has to be there. The built-in
settings screens go in with one call (feature `settings`, on by default): the home list,
display, sound, date and time, language and about always; Wi-Fi, Bluetooth, network and power
when a backend reports that capability; users and access when your authenticator manages its
entries.

```rust
# fn add(shell: &mut fairing::Shell) {
use fairing::settings::{add_all, SettingsConfig};

add_all(shell, &SettingsConfig::default());
# }
```

Without it, `cx.open("settings.home")` only logs that the screen was never declared. So does
`cx.open("settings.wifi")` on a device with no Wi-Fi backend — with the default `Null`
backends, a Wi-Fi screen could only say "not supported", so it is left out.
`SettingsConfig::default().ignoring_capabilities()` puts every screen in regardless. Narrowing
the list, replacing one screen and adding your own are in
[09 §8.3](09-branding.md#83-the-built-in-settings-screens--three-depths).

### 4.2 Fonts

The library carries no font, on purpose: which faces a device shows, and under what licence, is
the device's call. Until you give it faces, it draws with egui's built-in face: **one weight,
Latin letters only** — screen and row titles come out in the regular weight, and Hangul or `₩`
come out as boxes. The shell logs a warning at startup when there is no bold, and when its own
language's letters are missing. A device image usually has the faces already; the crate finds
them, and you decide what to load:

```rust
use fairing::fonts::{korean_font, strong_font, FontFamilies, FontPriority};
use fairing::FontSource;

# fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
let mut builder = fairing::Shell::builder(fairing::ShellConfig::default());
// Korean, from the system font directories. It joins the regular faces as a fallback.
if let Some(path) = korean_font() {
    builder = builder.font(FontSource::from_path("ko", path)?);
}
// A bold for the titles - first in the strong family, so it is the face they draw with.
if let Some(path) = strong_font() {
    builder = builder.font(
        FontSource::from_path("bold", path)?
            .families(FontFamilies::Strong)
            .priority(FontPriority::First),
    );
}
let shell = builder.build(ctx)?;
# Ok(shell)
# }
```

A device that ships its own files uses `FontSource::from_static(name, include_bytes!(..))`
instead; the examples carry Noto Sans KR that way (`assets/fonts/`). Boxes in place of Hangul
are [08 §6](08-troubleshooting.md#6-korean-renders-as-tofu).

### 4.3 The panel's size

Touch targets and bar sizes are written in millimetres (a 9 mm finger, a 7 mm status bar), so
the shell has to know how big a pixel is. Tell it the panel's visible area:

```rust
# fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
let shell = fairing::Shell::builder(fairing::ShellConfig::default())
    .physical_mm(154.0, 86.0) // a 7-inch 1024×600 panel
    .build(ctx)?;
# Ok(shell)
# }
```

A display backend that reports `DisplayInfo::physical_mm` does the same, and the builder's value
wins over it. With neither, the shell assumes about 6.3 pixels per millimetre and logs a warning:
a touch target is then 57 px on every panel — oversized on a coarse one, under a fingertip on a
dense one.

The finger is a bare one by default, and the text is sized for a panel read at hand-held
distance — a phone's density, where a row is one finger tall. A device worked with gloves or a
stylus says so, and every touch target, row and bar grows with it; a panel read standing up
moves the text on its own:

```rust
# fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
use fairing::unit::ScalePolicy;
let shell = fairing::Shell::builder(fairing::ShellConfig::default())
    .scale_policy(ScalePolicy::gloved().with_viewing_distance_mm(600.0))
    .build(ctx)?;
# Ok(shell)
# }
```

### 4.4 When you are not using the runner

If you already have your own `eframe::App` or another egui host, skip the `runner`
feature and call `Shell::frame(ui)` once per frame.

`ShellEvent` is not a reason to come here. `runner::run_shell` does drop events
into `log::debug!`, but **`runner::run_shell_with(options, build, on_event)`**
takes a handler in that spot — `run_shell` is the thin wrapper that supplies a
default one.

```rust
# fn host(shell: &mut fairing::Shell, ui: &mut egui::Ui) {
// Once per frame, hand the shell one root Ui.
shell.frame(ui);
for event in shell.poll_events() {
    // ScreenOpened / ScreenClosed / WentHome / SettingChanged / Access(..) …
}
# }
```

To build the shell on the first frame instead, use `runner::run` and take the
context from `ui.ctx()`. `examples/motion_lab.rs` has that shape.

### 4.5 Every `ShellEvent`

`poll_events()` (or `run_shell_with`'s handler) hands you what the shell did, in order. Most are
reports — the shell has already acted — and a few are requests that only you can carry out.

| Event | When | What it asks of you |
|---|---|---|
| `ScreenOpened { id, instance }` | A screen opened, from home or pushed | — |
| `ScreenClosed { id, instance }` | A screen closed: popped, removed, or its task ended | — |
| `WentHome` | The shell went to the desktop | — |
| `HiddenEntry { id }` | A hidden entry point's knock completed ([05](05-access-control.md#hidden-entry-points--the-service-menu)) | — |
| `Access(AccessEvent)` | Every unlock request, grant, refusal, lockout and change of subject ([05 §8](05-access-control.md#8-audit-logging-with-accessevent)) | In `routing` mode, `UnlockRequested` is yours to answer |
| `PowerRequest(PowerRequest)` | Shutdown, reboot or suspend was asked for | Clean up, then `Shell::commit_power` — the shell does not act on its own |
| `SettingChanged { key, value }` | A setting changed | Store it if it should survive a restart: the shell writes no files, and `Shell::restore_settings` puts what you stored back ([06 §7.1](06-services.md#71-keeping-settings-across-a-restart)) |
| `DeclRemoved(id)` | A declaration, a built-in status item or a tile was removed | — |
| `NotificationTapped(id)` | A notification was tapped; its `action` has already been launched | — |
| `NotificationDismissed(id)` | A notification was dismissed, by the user or `dismiss_notification` | — |
| `OverlayToggled(open)` | The shade finished opening (`true`) or closing (`false`) | — |
| `OskToggled(shown)` | The keyboard was shown or hidden | — |
| `TileLongPressed { id }` | A quick-settings tile was held ([03 §4.4d](03-chrome.md#44d-long-pressing-a-tile)) | Open its detail, if it has one |
| `IconLongPressed { id }` | A desktop icon was held; the info popover shows unless `[desktop] long_press = "none"` ([03 §3.8](03-chrome.md#38-holding-an-icon-the-info-popover)) | — |
| `WifiNetworkTapped { ssid, secured, known }` | A network row on `settings.wifi` was tapped | Connect — joining a network is the device's ([06 §1](06-services.md#connecting-is-yours)) |
| `WifiNetworkLongPressed { ssid, known }` | A network row was held | Offer to forget it, for instance |
| `LockRequested` | The lock control, `tile.lock`, `LaunchAction::Lock`, or the idle lock | In `prompt` mode with an authenticator the shell locks; elsewhere, what locking means is yours |
| `LogoutRequested` | Log out was asked for — `LaunchAction::Logout`, `ShellHandle::logout`, a tap on the `status.lock` padlock | In `prompt` mode with an authenticator the shell returns to the starting subject; elsewhere it is yours (`ShellHandle::set_subject`) |
| `OverviewRequested` | The recent-screens control was used | Only with `[workspace] overview = false`: show your own |
| `SplitRequested` | The split control was used | Only with `[workspace] split = false` |
| `SplitToggled(two)` | Two panes came up (`true`) or the workspace went back to one | — |
| `Emergency` | The emergency gesture completed; the shade opened if the gate passed | — |

The enum is `#[non_exhaustive]`, so keep a `_ => {}` arm.

## 5. Running the bundled examples

None of them needs real hardware: `hello` runs on the null backends, the rest on the mock ones.

| Example | Command | What it shows |
|---|---|---|
| `hello` | `cargo run -p fairing --features runner-x11 --example hello` | §4 as a file: one screen on the desktop, the built-in settings, a status bar item and a nav bar item, in a 1024×600 window. It shares nothing with the other examples, so it is the one to copy into a new project |
| `demo` | `cargo run -p fairing --features runner-x11 --example demo -- --size=1024x600` | The whole shell: twelve icons over two pages, the status bar, the shade, a `Form` screen that raises the OSK, the `Widgets` and `Notify` galleries, edge-swipe back, and a scenario thread waking the UI through the `Waker` |
| `console` | `cargo run -p fairing --features runner-x11,mock --example console -- --size=1280x800` | A bench instrument rather than a phone: a rail down the left joined to the status bar, five pages with their own transitions, and the shade split into two floating cards |
| `kiosk` | `cargo run -p fairing --features runner-x11,mock --example kiosk -- --size=1080x2560 --panel-mm=380x900` | A production-shaped café ordering flow, laid out and styled almost entirely by the crate — what it still draws itself is listed in its header. `--layout=portrait\|counter\|compact` puts the same code on three devices, and a switch on its attract screen changes the language of every screen at run time (`--lang=ko` starts it in Korean) |
| `custom_chrome` | `cargo run -p fairing --features runner-x11 --example custom_chrome -- --size=1024x600` | Replacing the chrome without touching the crate: a `Theme` built in code, a hand-drawn status bar, circular desktop badges, a procedural wallpaper, no nav bar, a gauges tile the crate draws from a declaration, and a jog-pad tile it knows nothing about |
| `motion_lab` | `cargo run -p fairing --features runner-x11 --example motion_lab -- --size=1024x600` | Every `[motion]` token on a slider, replaying the shade, page swipes, push/pop and icon zoom. A frame-time graph sits top right (p50, p95, a 33 ms rule) and the tuned values export as TOML |
| `palette_sheet` | `cargo run -p fairing --release --features runner-x11,mock --example palette_sheet -- --tour out/ --panel-mm=305x381` | One frame with every control on it, for comparing palettes (`--preset=base\|abyss\|linen`, `--theme=dark\|light`) |

Drop `--size` and they open fullscreen; `hello` sets its window in code. Every example but
`hello` needs `mock`, and `hello` needs `settings`; `custom_chrome` also needs `overlay`, and
`motion_lab` `overlay` and `osk`. All of those are default features, so normally there is nothing
to pass beyond the runner.

### `--tour` screenshot mode

`demo`, `console`, `kiosk`, `custom_chrome` and `palette_sheet` drive themselves,
write PNGs and close the window. They put the shell into each state through its own API (`launch`, `back`,
`home`) plus a synthetic finger, then send `egui::ViewportCommand::Screenshot` —
nobody has to sit and watch.

```sh
xvfb-run -a -s "-screen 0 1024x600x24" env LIBGL_ALWAYS_SOFTWARE=1 \
  cargo run -p fairing --features runner-x11 --example demo -- --tour target/tour
```

The environment variable `FAIRING_TOUR_DIR` works in place of `--tour <dir>`.
Mid-transition shots read the progress every frame and fire on the closest one, so
`motion.reduce` has to be off for those.

**A tour is a check as well as a camera.** A script presses what it names rather than where
that was last seen — `Act::Tap(Spot::Text("Alerts"))` finds the label on the glass this frame,
so a row that moves with the type scale or the finger is still the row pressed; a gesture starts
at an edge (`Spot::Edge(Side::Top, 0.5)`) or at a share of the content area (`Spot::Page(0.5,
0.8)`), and a drag goes to a spot (`Act::DragTo`), never a distance in pixels. There is no way to
write a window coordinate: that vocabulary is what let a tap go stale when the rows changed
height. A wait for a span of time is on the clock (`Act::WaitMs`), not a frame count. A script
also says what
it expects to be looking at before each picture: `Act::Expect(Expect::Text("Lamp hours"))`,
`Expect::Screen("settings.wifi")`, `Expect::ShadeOpen`, `Expect::OskUp`; `Act::Until(.., frames)`
is the same with patience, for what arrives after an animation the shell does not own. A step
that cannot find its target or an expectation that does not hold is logged with what *was*
there, the script goes on so one run lists every miss, and the example exits non-zero at the end.
`tools/tours.sh` runs every tour that way, stills only, and CI runs it on every push.

Add `--record` and the tour runs on a fixed 60 Hz clock: each frame is 1/60 s after
the one before, however long it took to draw, so a software rasteriser records as
smoothly as a GPU. The animated stretches of the script are written as frames, 30 a
second, each into a folder of its own (`demo-desktop/0000.png`, …). Stitch them into
a GIF or a video with any tool. `tools/make_gif.py` turns one folder into a GIF (it
needs Pillow), and `tools/readme-gifs.sh` rebuilds the images in the repository's
README that way, end to end.

## 6. Testing your UI headless

`fairing::testing::Harness` runs frames through `egui::Context::run_ui` with no
window and no GPU. It needs no dev-dependency, and time is virtual — 1/60 s per
frame — so nothing sleeps.

```rust
use fairing::testing::{single_level_access, test_shell};
use fairing::{icon, screen, Cx};

fn dashboard_opens_when_its_icon_is_tapped() -> fairing::Result<()> {
    // test_shell builds a harness on null backends (fixed clock) with motion.reduce = true.
    let mut h = test_shell(single_level_access(), |shell| {
        shell.add(
            screen("dashboard", |ui: &mut egui::Ui, _cx: &mut Cx| {
                ui.heading("Dashboard");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;

    h.frames(2); // the desktop has to be drawn once before an icon has a Rect
    let Some(rect) = h.shell.desktop().icon_rect("dashboard") else {
        return Err(fairing::Error::Config("the icon was never laid out".to_owned()));
    };

    h.tap(rect.center());
    assert!(h.shell.workspace().find("dashboard").is_some());
    assert!(!h.shell.workspace().is_home());
    Ok(())
}

fn main() -> fairing::Result<()> {
    dashboard_opens_when_its_icon_is_tapped()
}
```

Under `tests/`, put `#[test]` on the function. A test returning `fairing::Result`
passes as it is.

### Harness API

| Building | Meaning |
|---|---|
| `Harness::new(config, services)` | Your own config and backends. Use it when you want the animations to actually run |
| `Harness::from_builder(\|ctx\| ..)` | Wraps a shell built through `Shell::builder`, so theme injection and painter hooks are in play |
| `testing::test_shell(config, setup)` | Null backends with `motion.reduce = true` forced, so "two frames and it has landed" holds |
| `.with_size(w, h)` | Screen size (default `DEFAULT_SIZE`, 1024×600) |

| Frames | Meaning |
|---|---|
| `frame()` | One frame; virtual time advances 1/60 s |
| `frames(n)` / `run_for(seconds)` | Several frames / a span of time |
| `frame_shapes()` | The shapes actually drawn this frame |
| `now()` / `time()` / `frames` (field) | The shell's monotonic clock / the next `RawInput.time` / how many frames have run |

| Input | Meaning |
|---|---|
| `tap(pos)` | Press frame, release frame, settle frame (three in total) |
| `drag(from, to, steps)` | Press, move over `steps` frames, release |
| `hold(pos, frames)` | Hold without releasing (long press, emergency gesture) |
| `fling(from, step, frames)` | Push `step` per frame, then release. At 60 Hz that is `step × 60` px/s |
| `wheel(pos, delta)` | Pixel scroll. Scrolling up is `delta.y > 0` |
| `type_text(text)` | `Event::Text` — the same path the OSK injects on |
| `press` / `release` / `move_to` / `push_event` | The layer below |

| By label | Meaning |
|---|---|
| `tap_text(label)?` | A tap on the one text reading `label` this frame. An error naming what *is* on the glass when nothing does, or where each is when more than one does |
| `text_rect(label)?` | Where that text is — for a press, a drag or a long press that starts there |
| `texts()` | Every text drawn this frame with its rectangle (one scrolled out of its area is left out) |

Do not re-derive coordinates, and do not write them down: a number that is right for today's
row height is wrong the day the rows change, and a tap on nothing is silent. Find a row by
its label, or ask the shell for the rectangle it drew last frame:

| Ask | Signature |
|---|---|
| Desktop and dock icons | `shell.desktop().icon_rect(id: &str) -> Option<Rect>` |
| Status bar items | `shell.status_bar().item_rect(id: &str) -> Option<Rect>` |
| Nav bar items | `shell.nav_bar().item_rect(item: &NavItem) -> Option<Rect>` (`NavItem::Back`, `Home`, `Custom(id)`) |
| Quick-settings tiles | `shell.overlay().tile_rect(id: &str) -> Option<Rect>` |
| On-screen keyboard keys | `shell.osk().key_rect(label: &str) -> Option<Rect>` |

Real uses live in `crates/fairing/tests/m1_*.rs`, `m2_*.rs` and `m7_tap_by_label.rs`.

## 7. Measuring your device

`examples/bench` runs scenarios through the harness with no window and prints
frame-time p50, p95 and max to stdout. Run it on the device itself: the shell assumes no
hardware of its own, so how fast it draws on yours is yours to measure.

```sh
cargo run -p fairing --release --example bench
```

`--release` is the baseline. Debug numbers are a different order of magnitude — do
not mix them. The scenarios are: idle desktop, icon zoom (A2), push/pop (A3), a
200-widget screen (idle and drag-scrolling), home, shade drag (A1), page swipe (A4)
and OSK reveal (A5).

Keep each run with your device's notes: the hardware, the commit hash, the command, the
conditions (resolution, icon count, `motion.reduce`) and the table of numbers `bench` prints. Compare
runs on the same device: a p50 more than 30 % above an earlier one counts as a regression. p95
wobbles run to run on shared machines, so re-run two or three times before calling it.

Per-frame heap allocations are not counted. A counting allocator needs
`unsafe impl GlobalAlloc`, and the workspace is `unsafe_code = "forbid"`. The idle
desktop p50 is the proxy.

## 8. Where to go next

| What you want | Page |
|---|---|
| Several screens, moving between them, where state lives, lifecycle | [02 Screens](02-screens.md) |
| Status bar, nav bar, shade, notifications, OSK | [03 Chrome](03-chrome.md) |
| Themes and painters — changing the chrome itself | [04 Customization](04-customization.md) |
| Locking screens behind levels and gates | [05 Access control](05-access-control.md) |
| Wiring real Wi-Fi, power and display backends | [06 Services](06-services.md) |
| Every `fairing.toml` key | [07 Config reference](07-config-reference.md) |
| It will not start, or it looks wrong | [08 Troubleshooting](08-troubleshooting.md) |
