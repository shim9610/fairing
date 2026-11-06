# fairing

**A touchscreen shell for embedded devices, built on [egui](https://github.com/emilk/egui).**

The status bar, the pull-down shade with quick-settings tiles and notifications, a
desktop, a screen stack with transitions, a navigation bar, an on-screen keyboard
(numeric, QWERTY and two-set Hangul with composition), device settings screens and
access gates: the parts every kiosk, panel and bench instrument rebuilds, in one
process and one egui frame loop. It is not an OS, an app launcher or a window
manager. You write screens as egui closures; fairing owns everything around them.

```toml
[dependencies]
fairing = { version = "0.1", features = ["runner-x11"] }
```

```rust,no_run
use fairing::{egui, icon, screen, Cx};

fn main() -> fairing::Result<()> {
    let config = fairing::ShellConfig::load("/etc/myapp/fairing.toml")?; // defaults if absent
    fairing::runner::run_shell(fairing::runner::Options::default(), move |ctx| {
        let mut shell = fairing::Shell::builder(config)
            .services(fairing::Services::builder().build())
            .build(ctx)?;
        fairing::settings::add_all(&mut shell, &fairing::settings::SettingsConfig::default());
        shell.add(
            screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx| {
                ui.heading("Dashboard");
                if ui.button("Settings").clicked() {
                    cx.open("settings.home");
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

That needs the `runner` feature (`runner-x11` on a Linux desktop) and the default
`settings` feature. It uses the `fairing::egui` re-export, so no direct egui
dependency is needed. Without the runner, connect `Shell::logic*` and `Shell::frame*`
from an eframe app you already have.

## What sets it apart

- **Touch targets are physical.** Sizes resolve through panel millimetres, so the same
  code lands correctly on a 4-inch panel and a 27-inch one.
- **One gesture engine.** Edge swipes, flings, long presses and the shade arbitrate in
  one place, so nested scrolling and hand-off behave.
- **Non-waiting queue APIs.** UI code does not wait for messages or queue capacity.
  The audit rejects application-level locks and blocking calls in product UI sources;
  standard channel internals may synchronize.
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
| `brand` | on | The manta mark and the Abyss procedural background |
| `mock` | on | Mock backends — clock, power, Wi-Fi, Bluetooth, display, audio, network and device info — for development |
| `runner` | off | An eframe window bootstrap (`runner::run_shell`) |
| `runner-x11` | off | The same, with X11 as well as Wayland on Linux |
| `chrono` | off | A clock that follows the system time zone and daylight saving |

The touch widgets, the theme tokens and the unit system live in
[`fairing-widgets`](https://crates.io/crates/fairing-widgets) and are re-exported here.

## Platforms

The target is embedded Linux (Ubuntu Core with `ubuntu-frame`, `cage` or `weston`)
with OpenGL ES 2 through eframe's `glow` backend, on x86_64 and aarch64. The crate
uses eframe's platform support for desktop development. The full audit and graphical
tours run on Linux. Selected CPU display and runtime-fault tests also run on Windows;
the complete Windows runner and macOS are not validated by those checks.

## Documentation

- The [migration guide](https://github.com/shim9610/fairing/blob/HEAD/docs/guide/10-migration.md)
  covers source/API and default changes for applications using an earlier checkout.
- The integrator guide is in the repository's
  [`docs/guide/`](https://github.com/shim9610/fairing/tree/HEAD/docs/guide):
  getting started, screens, chrome, customisation, access control, services, the
  config reference and troubleshooting.
- The API reference is on [docs.rs](https://docs.rs/fairing).
- Screenshots and runnable examples are in the
  [repository README](https://github.com/shim9610/fairing).

## By design, and not yet

- The crate writes no files. A changed setting comes out as `ShellEvent::SettingChanged`;
  keep it where your device keeps such things and hand it back with
  `Shell::restore_settings` at the next start.
- Built-in text in English and Korean only; add other languages with
  `ShellBuilder::translations`.
- The known limits are listed in the
  [roadmap](https://github.com/shim9610/fairing/blob/HEAD/docs/roadmap.md#4-known-limits).

## The recommended profile for a device binary

The workspace holds only libraries and development tools, so it has no
`[profile.release]` (a profile is settled by the root package and would not reach your
binary anyway). In the `Cargo.toml` of the binary that goes onto the device:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
panic = "unwind"
strip = true
opt-level = 3      # for a UI, 3 beats "s"; strip wins the size back
```

Use `panic = "unwind"` for runtime callback containment. `panic = "abort"` bypasses
recovery and terminates the process on a panic; allocator aborts are not unwinds.


## Licence

MIT, see [`LICENSE`](LICENSE).

## Memory limits

`ShellBuilder::command_queue_capacity` (default 1,024) and
`max_commands_per_update` (default 256) bound command admission and work count.
`ShellHandle::try_send` distinguishes full/closed and preserves rejected ownership;
`try_send_with` constructs payloads only after reserving space.
`send` returns `false` for either full or closed queues; void helpers discard rejected
commands without logging. Use `try_send` when refusal or ownership matters.
`Inbox::bounded` supports worker channels, and `memory::FixedPool<T, N>` stores
objects inline with stale/foreign handle rejection.

Display admission uses fallible buffer growth, hard notification retention including
persistent items, and a bounded waiting toast queue (default 64). Use `try_toast`,
`try_notify` and the models' `try_new` constructors for explicit errors. Full or
failed optional display admission does not evict accepted commands.

The production `fixed-heap` feature was withdrawn: egui and remaining infallible
Rust paths can abort on OOM. The unpublished allocator is now a separate diagnostic
with no dependency edge from either product crate. Whole-renderer continued
operation under complete exhaustion remains unsupported. See the
[memory guide](https://github.com/shim9610/fairing/blob/HEAD/docs/memory-limits.md).


Display pressure uses bounded inline histories and borrowed-image/bitmap-warning
CPU raster functions under `fairing::memory`. Shell pending toasts default to
KeepLatest; `toast_overflow_policy(RejectNew)` restores refusal. Omission status
shows a compiled warning until acknowledged. These owned-path guarantees do not
establish whole stock-egui frame/GPU recovery from complete heap exhaustion.

## Application-owned transport and recovery

`bridge::CommandSink` and `UpdateSource` let an application select its command/update
transport without `Clone`, `Send`, `Sync` or `'static` bounds on messages. Fairing does
not choose a queue, start a worker, or install retries through these traits. They do
not replace `ShellHandle`'s internal transport or sandbox callback access.

With unwinding enabled, runtime callback faults quarantine ordinary shell dispatch.
`Shell::restart_with` accepts a host-built replacement, and `RecoveryUi` plus runner
recovery callbacks expose optional guidance and host policy. Old handles remain
disconnected. Shell replacement does not reset the egui context/GPU or undo business
side effects. See the [recovery guide](https://github.com/shim9610/fairing/blob/HEAD/docs/memory-limits.md#runtime-panics-and-sensitive-ui-state).

Issuer-controlled device sessions use explicit operation grants and split controller/token
capabilities. See [session tokens](https://github.com/shim9610/fairing/blob/main/docs/session-tokens.md)
for multiple roles, revocation and verification scope.

`ShellBuilder::build_protected` requires token mode and returns a `ProtectedShell`
whose declarations carry `Protected::new(value, declaration)` requirements. Your
own enum implements `Permission::gate(self)`; Fairing compares that value with the
current token. Implement `PermissionBound` for external screens/elements and use
`add_screen` / `add_elements` for library-owned dispatch. Element trees filter before
measurement/layout and recheck authority before paint, preserving UI/app/backend
access inside admitted callbacks. See the session guide for typed grants, stable
metadata, factory/lifecycle behavior and the callback-effect boundary.
