<p align="center">
  <img src="docs/images/social-preview.png" alt="fairing: a touchscreen shell for embedded devices, built on egui" width="100%">
</p>

<p align="center">
  <a href="https://github.com/shim9610/fairing/actions/workflows/ci.yml"><img src="https://github.com/shim9610/fairing/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.95%2B-orange.svg" alt="Rust 1.95+">
  <a href="https://github.com/emilk/egui"><img src="https://img.shields.io/badge/egui-0.37-8a2be2.svg" alt="egui 0.37"></a>
  <img src="https://img.shields.io/badge/UI_crates_unsafe-forbidden-success.svg" alt="unsafe forbidden in UI crates">
</p>

# fairing

A touchscreen shell for embedded devices, built on [egui](https://github.com/emilk/egui).

Fairing provides the UI around your device application: status and navigation bars,
a desktop, screen stacks, quick settings, notifications, an on-screen keyboard and
device settings. Your screens and hardware backends run in the same process and
egui frame loop. Touch sizes resolve from physical panel dimensions; screens,
chrome and widgets share theme and motion tokens.

**Development toward 0.1.0; the crates are not published yet.** This documentation
describes the current checkout. Start with the [integrator guide](docs/guide/README.md),
or read the [migration guide](docs/guide/10-migration.md) when updating an existing app.

<table>
  <tr>
    <td width="50%"><img src="docs/images/demo.gif" alt="Animated: on the desktop an icon zooms open into its screen, a second screen pushes over it and pops off, and home closes the screen back into its icon"></td>
    <td width="50%"><img src="docs/images/console-shade.gif" alt="Animated: on a bench console the notifications card and then the controls card are pulled open from the top edge, each floating on a frosted copy of the page"></td>
  </tr>
  <tr>
    <td>An icon zooms open, a screen pushes over it and pops off, home closes it (<code>demo</code>)</td>
    <td>A split shade: each half pulled open as a card on frosted glass (<code>console</code>)</td>
  </tr>
  <tr>
    <td><img src="docs/images/shade-curtain.png" alt="The shade drawn down as a curtain, with quick-settings tiles above a notification list"></td>
    <td><img src="docs/images/console.png" alt="A bench console: a rail down the left joined to the status bar, a progress ring and action tiles"></td>
  </tr>
  <tr>
    <td>The same shade as a curtain: tiles, notifications, a footer (<code>demo</code>)</td>
    <td>A bench console built from <code>layout</code> and <code>widgets</code> calls only (<code>console</code>)</td>
  </tr>
  <tr>
    <td><img src="docs/images/hangul-keyboard.png" alt="The on-screen keyboard's two-set Hangul layout composing a syllable in a text field"></td>
    <td><img src="docs/images/settings.png" alt="The built-in Wi-Fi settings screen with a switch and a network list"></td>
  </tr>
  <tr>
    <td>Two-set Hangul with composition on the on-screen keyboard (<code>demo</code>)</td>
    <td>One of the built-in settings screens (<code>demo</code>)</td>
  </tr>
</table>

## What is included

| Area | Capabilities |
|---|---|
| Shell | Desktop grid and dock, screen stacks, lifecycle events, transitions, two panes and recent-screen cards |
| Chrome | Status/nav bars, pull-down quick settings and notifications, toasts and heads-up banners |
| Input | Touch gestures, edge handles, custom gesture regions, numeric/QWERTY/two-set Hangul keyboard |
| Device integration | Clock, power, Wi-Fi, Bluetooth, display, audio, network and device-information traits, null backends and optional mocks |
| Access | Legacy level gates, credential prompts and lock screens; opt-in issuer-controlled sessions and typed protected UI |
| Layout and widgets | Screen containers, cards, grids, rails and tabs; 23 touch widgets, physical units, vector icons and theme tokens |
| Host integration | Bounded command admission, application-owned command/update transports, caught-unwind quarantine and host-controlled shell replacement |

[`fairing-widgets`](crates/fairing-widgets/README.md) can be used on its own in an
egui application. Fairing re-exports it and egui, so a shell application needs
only the `fairing` dependency.

## Quick start

For a Linux desktop, enable `runner-x11`; a Wayland device can use `runner`.
Pin a reviewed Git revision when integrating a device build.

```toml
[dependencies]
fairing = { git = "https://github.com/shim9610/fairing", features = ["runner-x11"] }
```

This minimal app uses the ordinary `Shell` API. Its default configuration does
not require a login; typed session enforcement is a separate choice below.

```rust,no_run
use fairing::{egui, icon, screen, Cx, Services, Shell, ShellConfig};

fn main() -> fairing::Result<()> {
    fairing::runner::run_shell(fairing::runner::Options::default(), |ctx| {
        let mut shell = Shell::builder(ShellConfig::default())
            .services(Services::builder().build())
            .physical_mm(154.0, 86.0)
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

The [standalone `hello` example](crates/fairing/examples/hello.rs) also shows custom
status/nav items. Run it with:

```sh
cargo run -p fairing --example hello --features runner-x11
```

Set `physical_mm` to the panel's visible dimensions for calibrated touch targets.
Load your product fonts with `ShellBuilder::fonts` or `font`; egui's fallback faces
do not contain Hangul. Without the runner, connect `logic`/`logic_with` and
`frame`/`frame_with` in your own host, then drain `poll_events` after either update.
[Getting started](docs/guide/01-getting-started.md) covers fonts, configuration,
features and hosting.

## Sessions and protected UI

Choose the integration boundary explicitly:

| API | Application registration | Authority |
|---|---|---|
| `ShellBuilder::build` | Ordinary declarations and trusted custom hooks | Legacy level gates by default; token mode can be enabled separately |
| `ShellBuilder::build_protected` | Typed `Protected` declarations, `add_screen` and `add_elements`; no mutable legacy-shell conversion | Requires token mode and checks current issuer grants before protected callbacks |

Your authenticator verifies credentials and owns a non-cloneable
`SessionController`. Fairing holds its read-only `SessionToken`. The issuer expands
roles into independent grants, updates them or terminally revokes the session.
Use separate controller/token pairs for device and web login state.
`PinTable` is a plaintext reference authenticator for the legacy flow; use your
own `Authenticator` for production credential verification.

Define your own permission enum and map its values to token grant identifiers:

```rust
use fairing::{action, egui, screen, Gate, ProtectedShell, Shell, ShellConfig};
use fairing::access::{Permission, Protected};

#[derive(Clone, Copy)]
enum PermissionId { ViewResults, ExportResults }

impl Permission for PermissionId {
    fn gate(self) -> Gate {
        Gate::borrowed(match self {
            Self::ViewResults => "results.view",
            Self::ExportResults => "results.export",
        })
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<ProtectedShell> {
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    let mut shell = Shell::builder(config).build_protected(ctx)?;
    shell.add(Protected::new(
        PermissionId::ViewResults,
        screen("results", |ui, _cx| { ui.heading("Results"); })
            .icon(fairing::icon::GAUGE)
            .desktop(),
    ));
    shell.add(Protected::new(
        PermissionId::ExportResults,
        action("export", |cx| { cx.shell.toast("Export requested"); }),
    ));
    Ok(shell)
}
```

This registers requirements; it does not issue a session. Until the authenticator
returns a valid `AuthOutcome::Session(token)`, protected work is denied. Issue
matching grants with `SessionGrant::base_for(value)` after authentication; the
[session guide](docs/session-tokens.md) covers issuance, asynchronous request
correlation, role expansion, expiry and logout.

For permission-dependent content inside a screen, implement `PermissionBound` and
`elements::Element`, then register an `ElementTree` with `add_elements`. Fairing
omits denied children before measurement/layout and rechecks authority before
paint. Custom `Screen + PermissionBound` implementations use `add_screen`.
The [typed integration example](docs/session-tokens.md#typed-permission-requirements)
shows nested UI and independent operation permissions.

The `runner::run_shell*`/`run_app*` helpers accept both shell modes through a sealed
host interface. `testing::Harness::from_builder` also accepts `build_protected`.
Register built-in settings with `settings::add_all_protected`, supplying your
permission mapping for each screen ID. Run the development integration example
with `cargo run -p fairing --example protected --features runner-x11`; its public
DEMO marker demonstrates session wiring and must be replaced by a real verifier.
Protected construction rejects untyped global painters/layouts, image loaders and
procedural wallpapers; put custom protected UI in typed declarations or elements.

From a repository checkout, run the protected example's GUI smoke tour with:

```sh
cargo run --locked -p fairing --example protected --features runner-x11 -- --tour target/tours/protected --size=1024x600
```

It checks a rejected development marker, login, an authorized operation, logout
with protected content hidden and a stale button click blocked, then reauthentication.
It writes four screenshots and exits nonzero on a failed check. `tools/tours.sh`
includes this flow alongside the gallery tours. This smoke test complements the
kernel proofs and headless tests; it does not cover every role change, revocation
or expiry history.

The [common mediation contract](docs/protected-contracts.md) proves the shared
admission kernel and automatically checks compiler-discovered protected entry
routes. An admitted callback retains raw egui, app and backend access; credential
verification, permission classification and arbitrary callback effects remain
integration responsibilities. This is a UI execution boundary within one process.

## Customization and backends

- **Configuration:** choose placement, visibility, gate assignments, palette and
  motion in `fairing.toml`.
- **Declarations:** add/replace/remove screens, actions, status/nav items and tiles
  by identifier. Protected registration carries a permission value.
- **Drawing:** supply themes and metrics; ordinary shells also accept global
  painter/layout hooks. Protected custom drawing uses typed elements.
- **Hardware:** implement backend traits or register a custom backend. Backends
  publish snapshots and wake the UI; the application owns workers and transport.
- **Persistence:** consume `ShellEvent::SettingChanged`, save values in your own
  storage and restore them at startup. Fairing writes no files itself.

See [customization](docs/guide/04-customization.md),
[services](docs/guide/06-services.md) and the
[configuration reference](docs/guide/07-config-reference.md).

Command admission defaults to 1,024 shared slots and 256 commands per update.
Use `try_send` to recover rejected ownership. Optional display work has separate
bounds and fallible APIs. Caught callback unwinds quarantine the shell; the host
chooses recovery. These mechanisms do not establish whole-renderer recovery from
heap exhaustion; the fixed-heap crate is an isolated unpublished diagnostic.
[Memory and recovery](docs/memory-limits.md) describes the limits and APIs.

## Examples

```sh
cargo run -p fairing --example demo --features runner-x11 -- --size=1024x600
cargo run -p fairing --example console --features runner-x11 -- --size=1280x800
```

| Example | Focus |
|---|---|
| `hello` | A minimal standalone app |
| `demo` | Shell features, settings, keyboard and legacy unlock prompts |
| `console` | Bench instrument, rail layout and split shade |
| `kiosk` | Café ordering, portrait/counter/compact layouts and live language switching |
| `custom_chrome` | Custom bar, desktop cells, wallpaper and tiles |
| `motion_lab` | Interactive motion-token tuning and TOML export |
| `palette_sheet` | Widget palette comparison |
| `bench` | Headless frame-time scenarios |

The gallery comes from the example tours. [Contributing](CONTRIBUTING.md#rebuilding-the-readmes-animations)
explains screenshot and GIF regeneration.

## Verification

`fairing::testing::Harness` drives actual egui frames without a window or GPU and
reports rendered geometry for pointer-driven tests. The repository also checks:

| Check | Evidence |
|---|---|
| Strict audit | Formatting, Clippy, workspace/guide doctests, API docs, dependency approvals, generated files, aarch64 type-check and blocking-call scan |
| API and UI regressions | Native/generated histories, retained widget state, independent expectations and production mutation controls |
| Feature profiles | Complete workspace tests and strict Clippy with minimal, default, default UI without Mock and all features |
| Graphical tours | Real-window gallery flows and the protected login/operation/logout/reauthentication smoke tour |
| Protected execution | Source-bound Kani/Verus kernel proofs, compiler-derived routing checks and extension/bypass controls |
| Additional assurance | Miri, Loom concurrency scenarios, sanitizer fuzzing, coverage and dependency inventories |
| Release archives | Extracted-package tests and an external consumer for minimal/default/all features |
| Code scanning | GitHub-managed CodeQL for Rust, Python and Actions |

```sh
cargo run -p xtask --locked -- audit --strict
```

[Rust assurance](docs/rust-assurance.md), the [API contract map](docs/api-contracts.md)
and [release checks](docs/release-checks.md) describe commands, evidence and scope.
Verification is tied to source/configuration; it is not a certification of arbitrary
application code or all possible UI histories.

## Platforms and current limits

The target is embedded Linux on x86_64/aarch64 with OpenGL ES 2 through eframe's
`glow` backend. CI runs the full audit and graphical tours on Linux and type-checks
aarch64; it does not validate a physical aarch64 device. Selected CPU display and
runtime-fault checks also run on Windows. The complete Windows runner and macOS
are not validated. The declared minimum Rust version is 1.95; repository development uses 1.99.

Built-in text is English/Korean; supply translations and fonts for your product.
The keyboard has numeric, QWERTY and two-set Hangul layouts. There is no accessibility
tree or right-to-left layout. See the [roadmap](docs/roadmap.md) for further limits.

## Documentation and workspace

| Resource | Purpose |
|---|---|
| [Integrator guide](docs/guide/README.md) | Installation, screens, chrome, access, backends and deployment |
| [Migration](docs/guide/10-migration.md) | Source and behavior changes from the earlier checkout |
| [Architecture](docs/architecture.md) | State ownership, frame order and module map |
| [Session contract](docs/session-tokens.md) | Issuer/session lifecycle and typed UI integration |
| [Protected mediation](docs/protected-contracts.md) | Shared kernel and automatic route verification |
| [Application metadata checker](docs/permission-metadata-check.md) | Check your workspace's permission declarations |

Build the local API reference with `cargo doc -p fairing --all-features --no-deps --open`.
docs.rs will host it after publication. The [changelog](CHANGELOG.md) records changes.

`fairing` and `fairing-widgets` are the product crates. `xtask` owns repository
maintenance; `verification` is an unpublished assurance workspace.
`fairing-fixed-heap` is a diagnostic with no product dependency edge.

## Security and licence

Report vulnerabilities privately through [SECURITY.md](SECURITY.md).
MIT; see [LICENSE](LICENSE) and [third-party notices](THIRD_PARTY.md).
