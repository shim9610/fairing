# fairing

**A touchscreen shell for embedded devices, built on [egui](https://github.com/emilk/egui).**

The status bar, the pull-down shade with quick-settings tiles and notifications, a
desktop, a screen stack with transitions, a navigation bar, an on-screen keyboard
(numeric, QWERTY and two-set Hangul with composition), device settings screens and
access gates: the parts every kiosk, panel and bench instrument rebuilds, in one
process. Your application owns business state, credential verification and device
backends. Register screens as egui closures or custom `Screen` implementations;
Fairing coordinates their layout, lifecycle and dispatch in its runner or your host.

Development toward 0.1.0; the crates have not been published yet. Use a reviewed
repository revision until the first release:

```toml
[dependencies]
fairing = { git = "https://github.com/shim9610/fairing", features = ["runner-x11"] }
```

The following app uses the ordinary `Shell` API with no login required by its
default configuration. For typed, issuer-controlled UI, see **Sessions and
protected UI** below.

```rust,no_run
use fairing::{egui, icon, screen, Cx};

#[derive(Default)]
struct AppState { completed: usize }

fn main() -> fairing::Result<()> {
    let config = fairing::ShellConfig::load("/etc/myapp/fairing.toml")?; // defaults if absent
    fairing::runner::run_app(fairing::runner::Options::default(), move |ctx| {
        let mut shell = fairing::Shell::builder(config)
            .services(fairing::Services::builder().build())
            .physical_mm(154.0, 86.0) // replace with the panel's visible dimensions
            .build(ctx)?;
        fairing::settings::add_all(&mut shell, &fairing::settings::SettingsConfig::default());
        shell.add(
            screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx| {
                ui.heading("Dashboard");
                if let Some(state) = cx.app_mut::<AppState>() {
                    if ui.button("Complete operation").clicked() {
                        state.completed += 1;
                    }
                    ui.label(format!("Completed: {}", state.completed));
                }
                if ui.button("Settings").clicked() {
                    cx.open("settings.home");
                }
            })
            .title("Dashboard")
            .icon(icon::GAUGE)
            .desktop(),
        );
        Ok((shell, AppState::default()))
    })
}
```

That needs the `runner` feature (`runner-x11` on a Linux desktop) and the default
`settings` feature. It uses the `fairing::egui` re-export, so no direct egui
dependency is needed. Without the runner, connect `Shell::logic*` and `Shell::frame*`
from an eframe app you already have. `run_app` owns the state returned alongside
the shell and lends it through `Cx::app` / `app_mut`; use `logic_with` and
`frame_with` with the same state in a custom host. `run_shell` is the counterpart
without application state, and the `_with` runner variants deliver shell events.

## What sets it apart

- **Touch targets are physical.** Sizes resolve through panel millimetres, so the same
  code lands correctly on a 4-inch panel and a 27-inch one.
- **One gesture engine.** Edge swipes, flings, long presses and the shade arbitrate in
  one place, so nested scrolling and hand-off behave.
- **Non-waiting queue APIs.** UI code does not wait for messages or queue capacity.
  The audit rejects application-level locks and blocking calls in product UI sources;
  standard channel internals may synchronize.
- **Customizable UI.** Use configuration and declarations; ordinary shells also
  accept global painters. Protected custom UI uses typed elements.
- **Your hardware, your backends.** The shell reaches clock, power, Wi-Fi, Bluetooth,
  display, audio, network and device info through traits, and anything else through a
  custom backend it polls each frame. It ships no system code: you plug in what your
  board speaks.
- **Headless tests.** `fairing::testing::Harness` runs frames with no window and no
  GPU, and reports where things were drawn, so tests tap and drag at real positions.

## Sessions and protected UI

Credential verification belongs to your `Authenticator`. It keeps the non-cloneable
`SessionController` and returns a read-only `SessionToken` to Fairing through a
request-correlated `AuthOutcome::Session`. Roles expand to explicit independent
grants; separate device and web sessions use separate controller/token pairs.
`PinTable` is a plaintext reference authenticator for the legacy level flow.

Issue permissions with `SessionGrant::base_for`, `temporary_for` or `inactive_for`.
The issuance catalog is fixed; `SessionController::replace_grants` can change only
catalogued gates and removes omitted rights. Revocation or dropping the controller
is terminal and wakes the shell. Grant fields are private; use their constructors
and `gate()`, `is_base()` and `temporary_until()` readers.

`ShellBuilder::build_protected` requires `access.session_tokens = true` and returns
`ProtectedShell`. Define your enum or value struct with `Permission::gate(self)`,
then register `Protected::new(value, declaration)`. External
`Screen + PermissionBound` implementations use `add_screen`; `ElementTree` uses
`add_elements`. Trees omit denied children before measurement/layout and recheck
live authority before paint. A missing, revoked or expired token denies protected
work; declaring a permission never grants it. A screen requirement guards the
screen callback, not each arbitrary egui widget inside it. Use a separately
protected action for an independent operation, or permission-bound elements for
independent layout/rendering requirements.

Protected construction rejects untyped global painters/layouts, image loaders and
procedural wallpapers. There is no mutable conversion to ordinary `Shell`.
Admitted callbacks retain original UI/app/backend references and callback-local
extension stores. `runner::run_shell*`/`run_app*` and `testing::Harness::from_builder`
accept both shell modes. Use `settings::add_all_protected` for built-ins with
application-owned permission values. Screen-view grants do not grant setting
writes; provide setting-key grants for `Cx::set_setting` / queued `set_setting`
separately. Observe layout, settings, session and decisions through
`ProtectedShell::observe`, which returns `shell::ShellView` without component/backend loans.

See [session tokens](https://github.com/shim9610/fairing/blob/HEAD/docs/session-tokens.md)
for a complete typed registration example, issuer lifecycle and nested elements.
The [common mediation contract](https://github.com/shim9610/fairing/blob/HEAD/docs/protected-contracts.md)
connects the proved admission kernel to compiler-discovered protected entries.
Credential verification, metadata classification and arbitrary admitted callback
effects remain integration responsibilities.

## Configuration and runtime changes

Configuration sections in `fairing::config` and runner `Options` use `Default` / constructors and
consuming `configure` closures; they are non-exhaustive. Define executable
screens/actions/items in code and use TOML to configure placement, appearance and
gates.

Ordinary `Shell` exposes named operations instead of mutable component loans:
`set_wallpaper`, `set_desktop_page`, `set_nav_style`, `set_nav_height` and
`set_status_items`. Edit the owned value from `status_item_spec` or `metrics_spec`
and commit with `set_status_item_spec` or `set_metrics_spec` so derived layout is
invalidated correctly.

Implement device backend traits through `Services::builder`, then replace backends
with `set_services`, `set_clock` or the backend-specific setters. Replacement
attaches the new waker before publication. `request_service(ServiceRequest::...)`
is an ordinary trusted-host hardware API; user setting writes must use setting
dispatch. Protected hosts expose neither this method nor mutable backend access.

The builder supplies a system clock and Null device backends; `Services::null()`
also uses a fixed Null clock. Backend workers, transport and persistent storage
belong to the application. Consume `ShellEvent::SettingChanged`; ordinary hosts can
use `Shell::restore_settings` for trusted startup restoration.

See [host API decisions](https://github.com/shim9610/fairing/blob/HEAD/docs/host-api-decisions.md)
for the earlier-to-current API mapping and
[the protected integration example](https://github.com/shim9610/fairing/blob/HEAD/crates/fairing/examples/protected.rs)
for authenticator, app-state and typed settings wiring.

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
[`fairing-widgets`](https://github.com/shim9610/fairing/tree/HEAD/crates/fairing-widgets) and are re-exported here.

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
- Build the API reference with `cargo doc -p fairing --all-features --no-deps --open`.
  [docs.rs](https://docs.rs/fairing) will host it after publication.
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

Display pressure uses bounded inline histories and borrowed-image/bitmap-warning
CPU raster functions under `fairing::memory`. Shell pending toasts default to
KeepLatest; `toast_overflow_policy(RejectNew)` restores refusal. Omission status
shows a compiled warning until acknowledged.

The production `fixed-heap` feature was withdrawn because egui and remaining
infallible Rust allocations can abort on out-of-memory (OOM) failures. The allocator
is now an unpublished diagnostic with no product dependency edge. Whole-renderer
continued operation under complete exhaustion is unsupported; see the
[memory guide](https://github.com/shim9610/fairing/blob/HEAD/docs/memory-limits.md).

## Application-owned transport and recovery

`bridge::CommandSink` and `UpdateSource` let an application select its command/update
transport without `Clone`, `Send`, `Sync` or `'static` bounds on messages. Fairing does
not choose a queue, start a worker, or install retries through these traits. They do
not replace `ShellHandle`'s internal transport or sandbox callback access.

With unwinding enabled, runtime callback faults quarantine dispatch in both shell
modes. `Shell::restart_with` accepts an ordinary replacement;
`ProtectedShell::restart_with` requires a validated protected replacement. Failed
replacement stays quarantined. `RecoveryUi`, `run_shell_with_recovery` and
`run_app_with_recovery` expose optional guidance and host policy. Old handles remain
disconnected. Shell replacement does not reset the egui context/GPU or undo business
side effects. See the [recovery guide](https://github.com/shim9610/fairing/blob/HEAD/docs/memory-limits.md#runtime-panics-and-sensitive-ui-state).

## Licence

MIT, see [`LICENSE`](LICENSE).
