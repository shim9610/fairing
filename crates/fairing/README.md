# fairing

**A touchscreen shell for embedded devices, built on [egui](https://github.com/emilk/egui).**

The status bar, the pull-down shade with quick-settings tiles and notifications, a
desktop, a screen stack with transitions, a navigation bar, an on-screen keyboard
(numeric, QWERTY and two-set Hangul with composition), device settings screens and
access gates: the parts every kiosk, panel and bench instrument rebuilds, in one
process and one egui frame loop. It is not an OS, an app launcher or a window
manager. You write screens as egui closures; fairing owns everything around them.

```rust
use fairing::{icon, screen, Cx};

fn main() -> fairing::Result<()> {
    let config = fairing::ShellConfig::load("/etc/myapp/fairing.toml")?; // defaults if absent
    fairing::runner::run_shell(fairing::runner::Options::default(), move |ctx| {
        let mut shell = fairing::Shell::builder(config)
            .services(fairing::Services::builder().build())
            .build(ctx)?;
        shell.add(
            screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx| {
                ui.heading("Dashboard");
                if ui.button("Wi-Fi").clicked() {
                    cx.open("settings.wifi");
                }
            })
            .title("Dashboard")
            .icon(icon::GAUGE)
            .desktop(),
        );
        Ok(shell)
    })
}
```

That needs the `runner` feature (`runner-x11` on a Linux desktop), and `egui = "0.36"`
beside fairing for the `egui::Ui` in it (or `use fairing::egui;`). Without the runner,
drive `Shell::frame` from an eframe app you already have.

## What sets it apart

- **Touch targets are physical.** Sizes resolve through panel millimetres, so the same
  code lands correctly on a 4-inch panel and a 27-inch one.
- **One gesture engine.** Edge swipes, flings, long presses and the shade arbitrate in
  one place, so nested scrolling and hand-off behave.
- **No locks on the UI thread.** Threads talk over channels; a build gate enforces it.
- **Nothing is hard-wired.** Every chrome surface is configuration, a declaration or a
  painter you supply, without forking the crate.
- **Your hardware, your backends.** The shell reaches clock, power, Wi-Fi, Bluetooth,
  display, audio, network and device info through traits, and anything else through a
  custom backend it polls each frame. It ships no system code: you plug in what your
  board speaks.
- **Headless tests.** `fairing::testing::Harness` runs frames with no window and no
  GPU, and reports where things were drawn, so tests tap and drag at real positions.

## Features

| Feature | Default | What it adds |
|---|---|---|
| `overlay` | on | The shade: quick-settings tiles, the notification list, the scrim |
| `osk` | on | The on-screen keyboard |
| `settings` | on | The built-in settings screens |
| `brand` | on | The manta mark, the Abyss background, the app tile and the splash |
| `mock` | on | Mock backends — clock, power, Wi-Fi, Bluetooth, display, audio, network and device info — for development |
| `runner` | off | An eframe window bootstrap (`runner::run_shell`) |
| `runner-x11` | off | The same, with X11 as well as Wayland on Linux |
| `chrono` | off | A clock that follows the system time zone and daylight saving |

The touch widgets, the theme tokens and the unit system live in
[`fairing-widgets`](https://crates.io/crates/fairing-widgets) and are re-exported here.

## Platforms

The target is embedded Linux (Ubuntu Core with `ubuntu-frame`, `cage` or `weston`)
with OpenGL ES 2 through eframe's `glow` backend, on x86_64 and aarch64. The crate
also builds for macOS and Windows, which is enough to develop on, but it is only run
and tested on Linux.

## Documentation

- The integrator guide is in the repository's
  [`docs/guide/`](https://github.com/shim9610/fairing/tree/HEAD/docs/guide):
  getting started, screens, chrome, customisation, access control, services, the
  config reference and troubleshooting.
- The API reference is on [docs.rs](https://docs.rs/fairing).
- Screenshots and runnable examples are in the
  [repository README](https://github.com/shim9610/fairing).

## Not yet

- No settings persistence: take `ShellEvent::SettingChanged` and store it yourself.
- Built-in text in English and Korean only; add other languages with
  `ShellBuilder::translations`.

## The recommended profile for a device binary

The workspace holds only libraries and development tools, so it has no
`[profile.release]` (a profile is settled by the root package and would not reach your
binary anyway). In the `Cargo.toml` of the binary that goes onto the device:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
panic = "abort"
strip = true
opt-level = 3      # for a UI, 3 beats "s"; strip wins the size back
```

## Licence

MIT, see [`LICENSE`](LICENSE).
