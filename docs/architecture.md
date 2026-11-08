# fairing architecture

This page is the map of the crate: what sits where, how one frame runs, how threads talk to the
shell, and the rules the code keeps. It describes the code in the repository today. How to use
each part is in the [integrator guide](guide/README.md); what is planned is in the
[roadmap](roadmap.md).

## 1. Layers

```text
┌─────────────────────────────────────────────────────────────────┐
│ Your device binary                                              │
│   your screens · your backends · fairing.toml · your theme      │
├─────────────────────────────────────────────────────────────────┤
│ fairing                 the shell                               │
│   shell · chrome · overlay · osk · notify · desktop · workspace │
│   screen · gesture · access · settings · services · i18n        │
│   elements · layout · config · time · inbox · brand · testing   │
├─────────────────────────────────────────────────────────────────┤
│ fairing-widgets         the element layer                       │
│   unit · theme · motion · icons · fonts · widgets · drag        │
├─────────────────────────────────────────────────────────────────┤
│ runner (optional)       an eframe window, glow, Wayland or X11  │
├─────────────────────────────────────────────────────────────────┤
│ egui / epaint                                                   │
├─────────────────────────────────────────────────────────────────┤
│ compositor · GL driver · kernel                                 │
└─────────────────────────────────────────────────────────────────┘
```

The shell only sees **traits** for the device: Wi-Fi, Bluetooth, display, power, audio, network,
device information and the clock. It ships no system code. A backend is yours, written against
sysfs, a vendor SDK, D-Bus or whatever your device speaks. The crate gives each trait a `Null`
implementation and, with the `mock` feature, a `Mock` simulator for development
([guide 06](guide/06-services.md)).

## 2. The workspace

| Crate | Release role | What it holds |
|---|---|---|
| `fairing` | Planned public crate | The shell. It re-exports `fairing-widgets` and `egui`, so one dependency is enough |
| `fairing-widgets` | Planned public crate | Units, the theme, motion, icons, fonts and the touch widgets. A control can be worked on here without the shell |
| `fairing-fixed-heap` | Unpublished diagnostic | Isolated allocation-failure diagnostic; no product dependency; one scoped unsafe arena claim |
| `xtask` | Unpublished tool | The audit gate, the icon compiler, the license report and the other maintenance commands |

The first crates.io release has not been published yet.

`fairing` depends on `egui`, `serde`, `toml` and `log`, plus `eframe` behind `runner` and `chrono`
behind `chrono`. Every other dependency, a new asset included, goes through an approval list
that the audit checks (`deps.allow`, `deny.toml`).

egui and eframe use the 0.37 API. The optional runner keeps GL, Wayland, bundled fonts and
OS clipboard support explicit, with X11 added by `runner-x11`. For an overdue redraw,
eframe can call `App::logic` while a Wayland compositor withholds frame callbacks. It
does not run `App::ui` or force a frame to be presented. `run_shell*` and `run_app*` call
`Shell::logic*` there: commands, backend snapshots, access expiry/authentication, stopped
screen eviction and lifecycle requests continue on the event-loop thread. Their `ui`
callback draws without repeating the same state update. The closure-only `runner::run`
has no shell to tick; a custom host must connect `Shell::logic*` itself.

### Features

| Feature | Default | What it adds |
|---|---|---|
| `overlay` | on | The shade: quick-settings tiles, the notification list, the scrim |
| `osk` | on | The on-screen keyboard |
| `settings` | on | The built-in settings screens. The setting values are always there |
| `brand` | on | The manta mark and the Abyss background |
| `mock` | on | Mock backends for development |
| `runner` | off | An eframe window bootstrap, `runner::run_shell` |
| `runner-x11` | off | The same, with X11 as well as Wayland |
| `chrono` | off | A clock that follows the system time zone and daylight saving |

A large part comes out whole behind its feature. Turned off, it is not compiled at all.

## 3. Modules

The public module paths below are stable facades. Private implementation files
separate coordination from subsystem work:

- `shell`: authentication/session handling, setting dispatch, navigation, state
  updates and rendering; `mod.rs` retains the shell's state and public facade.
- `workspace`: lifecycle, split/overview coordination, gestures and rendering;
  the inline test suite lives in `workspace/tests.rs`.
- `settings/screens`: device pages, network forms and credential administration,
  with shared retained-form ownership in the parent module.
- `overlay`: transitions and rendering, with panel tiles, notifications and footer
  rendering in separate files.
- `desktop`: page state, grid/dock/icon rendering and the rail.
- `layout`: containers, rows and fitting functions re-exported from `layout`.
- `widgets/dropdown`: trigger painting, popup behavior and shared painting helpers.

These file boundaries do not change the state owner or the frame/deadline order.
Method implementation modules use parent-scoped visibility for cross-file helpers;
they do not expose new application APIs.

| Module | What it does |
|---|---|
| `shell` | `Shell`, `ProtectedShell`, `ShellBuilder`, `ShellHandle` and `ShellEvent`. Runs the frame below |
| `elements` | Permission-aware leaf/row/column trees with library-owned filtering, measurement, layout and paint dispatch |
| `screen` | `Screen`, `ScreenDecl`, `Cx`, `ChromePolicy`, the lifecycle. A screen is a closure or a type you register |
| `workspace` | The screen stack, transitions, two panes with a divider, the recent screens |
| `desktop` | The icon grid, pages, the dock and the icon rail, the wallpaper |
| `chrome` | The status bar and the nav bar, their items, layouts and painters |
| `overlay` | The shade, its detents, tiles and notification list |
| `osk` | The on-screen keyboard: numeric, QWERTY and two-set Hangul |
| `notify` | Notifications, toasts and heads-up banners |
| `inbox` | Non-waiting channel receive APIs, bounded admission and connection-specific storage/work limits |
| `bridge` | Optional application-implemented command/update transport traits; no transport or thread policy |
| `recovery` | Fault guidance, shell-replacement errors and panic-payload cleanup diagnostics |
| `memory` | Re-exported fixed pools, fallible strings, display windows and CPU raster helpers |
| `gesture` | One gesture engine for edges, flings, long presses, handles and your own regions |
| `access` | Legacy levels/gates, issuer-controlled controller/token sessions, typed permission bindings, common admission kernel, credential prompts and `Authenticator` |
| `settings` | Setting keys and values, and the built-in settings screens |
| `services` | The backend traits, `Null` and `Mock`, `Services`, `Waker` |
| `i18n` | The string table, with English and Korean built in, switched live |
| `layout` | Screen layout helpers: grids, groups, action bars, tab bars |
| `config` | `ShellConfig`, read from `fairing.toml` and validated |
| `time` | Clocks and wall time |
| `brand` | The mark and the procedural background |
| `testing` | `Harness`: frames with no window and no GPU |
| `runner` | The optional window bootstrap |

### Protected execution

Ordinary `build` permits untyped host customization. `build_protected` requires
token mode, rejects untyped builder hooks before setup and returns a facade with
no mutable legacy-shell conversion. User permission values map to issuer grant
identifiers; `Protected` declarations and `PermissionBound` screens retain those
requirements. `ElementTree` owns nested layout and omits denied children before
measurement; it invalidates stale plans and checks live authority before paint.

The authenticator owns the mutable `SessionController`; the shell reads its
`SessionToken`. Revocation/publication wakes the host, and access is observed during
logic-only updates as well as rendering and protected-operation admission. The
[session guide](session-tokens.md) describes issuer and clock assumptions. Admitted
application bodies retain raw UI/app/backend access.

The [common contract](protected-contracts.md) generates opaque payload storage and
operation adapters. Source-bound kernel proofs and compiler-MIR routing checks
connect library-managed protected entries to one admission boundary. The
[metadata checker](permission-metadata-check.md) separately checks supported
workspace permission getters/mappers. These are distinct contracts.

`runner::run_shell*`/`run_app*` and `testing::Harness::from_builder` accept ordinary
or protected shells through the sealed `HostedShell` interface. It delegates
frame/logic/event/fault operations without exposing registration or conversion.
`ProtectedShell::restart_with` only accepts a protected replacement and uses the
same close-before-build/fail-quarantined lifecycle as the ordinary shell.
`settings::add_all_protected` maps each built-in screen ID to an application-owned
permission value; backend writes retain their separate operation gates.
`ShellView` exposes supported state and diagnostic queries without component loans.

## 4. One frame

For a frame-only host, `Shell::frame(ui)` runs this sequence. Shell/state runners run
state stages 1–4 in `Shell::logic*` first, including when no UI follows; the UI pass
records fresh activity and reuses that state update before gestures and drawing.
Eviction and lifecycle requests can also finish during logic-only calls.

| # | Step | What happens |
|---|---|---|
| 1 | Time, declarations and expiry | Advance time, rebuild declarations, observe integrator subject replacements, enforce overdue access deadlines and revalidate open gates before commands or input |
| 2 | Commands | Receive and execute at most `max_commands_per_update` queued commands (default 256); request continuation for remaining work |
| 3 | Services | Pin snapshots, confirm outstanding Bool commands and send queued targets, gather backend wake-ups |
| 4 | Access | Revalidate policy after backend updates, record input and poll the authenticator |
| 5 | Input | The gesture engine, the guard, the shade, the back gesture, the keyboard, every animation |
| 6 | Layout | The rects for this frame from the focused screen's chrome policy and the keyboard |
| 7–9 | Draw | The status bar, the nav bar, then the desktop or the screens |
| 10–13 | Layers | The keyboard, the shade, the unlock prompt or lock screen, then toasts and banners |
| 14 | Outputs | Launches, back, home, actions from the shade and notifications, lifecycle events |
| 15 | Repaint | A repaint while anything moves, otherwise one at the next moment something is due |
| 16 | Events | You take them with `poll_events` |

The order is a contract. A screen sees a layout that is final for the frame, and what a screen
asks for through `Cx` while it draws, such as opening another screen, is handled later
in the same frame. `Cx::shell` sends through `ShellHandle` and waits for a subsequent
state update and its command budget.

## 5. Threads

- **The UI is one thread.** `Shell` does not need to be `Send`.
- **UI shared state uses channels and atomics.** The audit bans application-level
  Mutex/RwLock/Condvar and blocking calls in UI sources. Standard channels may
  synchronize internally; the isolated diagnostic allocator uses a spinlock. There is
  no wait-free or hard real-time guarantee.
- **Command admission is bounded.** All handles share 1,024 slots by default and
  an update executes at most 256 commands. Builder options adjust both. Full
  rejection preserves ownership through `try_send`; accepted work is not evicted.
  [Memory limits](memory-limits.md) explains factories, metrics, fixed storage,
  newest-sample display windows and allocation-free CPU preview/warning primitives.
  There is no renderer process/IPC layer. The ordinary egui path is still allocating.
- **UI queue operations do not wait for work or capacity.** They use `try_recv` and
  immediate admission rather than blocking receive/join/sleep calls. Runtime callbacks
  and transports supplied by the application must return promptly; the audit does not
  enforce that on external code. Startup configuration/font loading can read files.
  A worker thread may wait on its own queue.
- **`ShellHandle` is `Clone + Send`.** Any thread can launch a screen, go back or home, post a
  notification or a toast, change a setting or ask for an unlock. Sending wakes the UI.
- **Backends ask to be woken.** A backend holds a `Waker` and calls `wake()` when its state
  changes. Something due later, such as a scan finishing, is `next_wake()`, not a timer thread.
- **Logic-only work uses wall time.** egui retains the last UI input during `run_logic`.
  The shell uses a shared monotonic wall timeline once `Shell::logic*` is used, so stale
  input cannot renew a session and returning to UI cannot count the hidden interval twice.
  A host using only `Shell::frame*` retains egui's input timeline for headless simulation.
  Hidden scheduling follows backend/access/eviction deadlines and bounded queued work;
  animations, held pointers and continuous repaint do not keep it spinning.
- **Screenshot callbacks only deliver data.** A per-request `Inbox` carries the image back to
  the UI thread and the callback asks for a repaint. Closing a card, changing its theme or
  viewport, or timing out drops that inbox so a late image cannot complete a later request.

Applications can implement `bridge::CommandSink` and `UpdateSource` for ownership-moving
channels, shared buffers, lock-free queues or network adapters. The traits impose no
`Clone`, `Send`, `Sync` or `'static` bound and do not replace `ShellHandle`'s internal
queue. Adapters own their admission/delivery semantics; the host owns thread placement,
wakeups, retries and UI replacement. See [the boundary guide](guide/06-services.md#application-owned-uibusiness-boundary).

## 6. State

**The crate writes no files.** It reads `fairing.toml` once at start, and the files your code
hands it, such as fonts. What should outlive a restart is yours to keep, wherever your device
keeps such things:

- A changed setting comes out as `ShellEvent::SettingChanged`. Hand it back at the next start
  with `Shell::restore_settings`.
- Credentials are your `Authenticator`'s to store.

## 7. Repaint and power

In reactive mode, the shell requests updates for input, backend wakeups, animations,
queued work and scheduled deadlines. With none pending it requests no continuous
redraws; this is the idle **0 fps** condition. A visible clock still schedules updates
according to its minute/second format. Continuous mode and application repaint requests
can keep drawing independently. A screen with `keep_awake` asks the display backend
to inhibit its idle timer. How to confirm 0 fps at idle is in
[guide 08](guide/08-troubleshooting.md).

## 8. Errors

- **UI product code rejects explicit panic paths.** `unwrap`, `expect`, `panic!`
  and explicit termination paths are denied by its Clippy configuration; indexing is
  checked by the warning-denying audit. Dependencies, integrator
  callbacks and infallible allocation can still terminate a process; whole-renderer exhaustion is not universally recoverable. The diagnostic allocator
  is not selected by either product library.
- A backend error comes back as a value, and the shell shows it as a toast or a notification.
- Invalid configuration values fail at start with `Error::Config`; unreadable or
  invalid filesystem paths return `Error::Io`. An absent file deliberately selects
  defaults, including legacy access mode. A deployment requiring a provisioned
  security configuration must enforce its presence; `ShellConfig::load` is not a
  required-file API. `build_protected` separately requires token mode.
- Logging goes through the `log` facade. The logger is yours.
- **Runtime unwind faults quarantine the shell.** Borrowed ownership is restored before
  forwarding an unwind to the shell boundary. Dispatch, event delivery and admission
  stop there; the host may replace the shell with `restart_with` and opt into recovery
  guidance. Replacement does not reset egui/GPU state or roll back business effects.
  [Memory limits](memory-limits.md#runtime-panics-and-sensitive-ui-state) defines the
  exact unwind, output and payload-cleanup scope.

## 9. Testing and the audit

`testing::Harness` runs whole frames on `egui::Context` with virtual time, so a test taps and
drags where the shell actually drew, with no window, no GPU and no sleeping. The library's tests,
the examples and the guide's non-ignored Rust blocks run under `cargo xtask audit`, which also checks
formatting, lints, docs, dependencies, licenses, locks and blocking calls. CI runs it with
`--strict` on pull requests and pushes to `main`, and on a weekly schedule
([guide 08 §9](guide/08-troubleshooting.md#9-passing-the-audit)).

## 10. Principles

These are the rules the design keeps coming back to.

1. **Any touchscreen device is a target.** There is no list of target devices, so no default
   assumes one.
2. **Every way of touching is a target:** capacitive and resistive panels, bare fingers and
   gloves. Sizes are set in millimetres. The default is a bare 9 mm finger and text read at
   hand-held distance, the density of a phone or a tablet; a device worked with gloves says so,
   and every target grows with it.
3. **Backends belong to the integrator.** The crate defines the traits and ships no system code.
4. **Customization happens in code.** Every part can be turned on, turned off or redrawn without
   forking the crate.
5. **Non-waiting UI APIs.** Threads exchange messages; UI code does not wait for messages or capacity.
6. **One general mechanism beats a one-off effect.** Implement a trait, and the library does the
   rest.
7. **Review the public surface.** Reachability lints reject unreachable public items;
   compatibility also depends on public fields, exhaustive enums and exposed concrete
   types. See [API evolution](api-contracts.md#public-surface-and-evolution).
8. **Defaults are chosen with care and stay adjustable** through the API.
9. **The crate writes no files of its own.**
10. **Executable documentation examples are checked.** Ignored integration outlines
    and platform-dependent examples need their stated external setup; compilation
    alone does not establish their visible behavior.

## 11. The control rules

Every built-in control keeps these. The widget sources cite them by number.

| Rule | What it says |
|---|---|
| 1.1 | The slot is a full touch target on both axes, whatever size the control is drawn at |
| 1.3 | Inside a row, the row owns the hit rect. A control drawn into it senses nothing of its own |
| 1.4 | A disabled control lets the tap through to whatever is beneath it |
| 1.5 | A press only ever grows the painted shape. The allocated rect never moves |
| 3.1 | A control's identifying boundary meets the 3.0 contrast floor of WCAG 1.4.11. `Outline` is a divider, never a control's edge |
| 4.1 | The pill shape is kept for movers that ride a track, so it stays the cue that something travels |
| 7.1 | Colour alone is never the channel. A state also shows in shape or fill |
