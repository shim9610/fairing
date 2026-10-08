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
│   layout · config · time · inbox · brand · testing              │
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

| Crate | Published | What it holds |
|---|---|---|
| `fairing` | yes | The shell. It re-exports `fairing-widgets` and `egui`, so one dependency is enough |
| `fairing-widgets` | yes | Units, the theme, motion, icons, fonts and the touch widgets. A control can be worked on here without the shell |
| `xtask` | no | The audit gate, the icon compiler, the license report and the other maintenance commands |

`fairing` depends on `egui`, `serde`, `toml` and `log`, plus `eframe` behind `runner` and `chrono`
behind `chrono`. Every other dependency, a new asset included, goes through an approval list
that the audit checks (`deps.allow`, `deny.toml`).

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

| Module | What it does |
|---|---|
| `shell` | `Shell`, `ShellBuilder`, `ShellHandle` and `ShellEvent`. Runs the frame below |
| `screen` | `Screen`, `ScreenDecl`, `Cx`, `ChromePolicy`, the lifecycle. A screen is a closure or a type you register |
| `workspace` | The screen stack, transitions, two panes with a divider, the recent screens |
| `desktop` | The icon grid, pages, the dock and the icon rail, the wallpaper |
| `chrome` | The status bar and the nav bar, their items, layouts and painters |
| `overlay` | The shade, its detents, tiles and notification list |
| `osk` | The on-screen keyboard: numeric, QWERTY and two-set Hangul |
| `notify` | Notifications, toasts and heads-up banners |
| `inbox` | The notification store the shade reads |
| `gesture` | One gesture engine for edges, flings, long presses, handles and your own regions |
| `access` | Levels, gates, the session, the unlock prompt, the lock screen, `Authenticator` |
| `settings` | Setting keys and values, and the built-in settings screens |
| `services` | The backend traits, `Null` and `Mock`, `Services`, `Waker` |
| `i18n` | The string table, with English and Korean built in, switched live |
| `layout` | Screen layout helpers: grids, groups, action bars, tab bars |
| `config` | `ShellConfig`, read from `fairing.toml` and validated |
| `time` | Clocks and wall time |
| `brand` | The mark and the procedural background |
| `testing` | `Harness`: frames with no window and no GPU |
| `runner` | The optional window bootstrap |

## 4. One frame

Everything happens inside `Shell::frame(ui)`, once per frame, in a fixed order:

| # | Step | What happens |
|---|---|---|
| 1 | Time | `now` from the real time between frames; `dt` is capped at 50 ms |
| 2 | Commands | The `ShellHandle` queue is drained with `try_recv` |
| 3 | Services | Backend snapshots are pinned and their next wake-ups gathered |
| 4 | Access | Temporary unlocks expire, the session times out, the idle lock runs |
| 5 | Input | The gesture engine, the guard, the shade, the back gesture, the keyboard, every animation |
| 6 | Layout | The rects for this frame from the focused screen's chrome policy and the keyboard |
| 7–9 | Draw | The status bar, the nav bar, then the desktop or the screens |
| 10–13 | Layers | The keyboard, the shade, the unlock prompt or lock screen, then toasts and banners |
| 14 | Outputs | Launches, back, home, actions from the shade and notifications, lifecycle events |
| 15 | Repaint | A repaint while anything moves, otherwise one at the next moment something is due |
| 16 | Events | You take them with `poll_events` |

The order is a contract. A screen sees a layout that is final for the frame, and what a screen
asks for while it draws, such as opening another screen, is handled later in the same frame.

## 5. Threads

- **The UI is one thread.** `Shell` does not need to be `Send`.
- **No locks.** No `Mutex`, `RwLock`, `Condvar`, `OnceLock` or the like, in any crate of the
  workspace. Threads talk over `std::sync::mpsc` channels and atomics. The audit fails a build
  that brings a lock in.
- **The UI thread never blocks.** It uses `try_recv`, never `recv`, `join`, `sleep` or file IO.
  A worker thread may wait on its own queue.
- **`ShellHandle` is `Clone + Send`.** Any thread can launch a screen, go back or home, post a
  notification or a toast, change a setting or ask for an unlock. Sending wakes the UI.
- **Backends ask to be woken.** A backend holds a `Waker` and calls `wake()` when its state
  changes. Something due later, such as a scan finishing, is `next_wake()`, not a timer thread.

## 6. State

**The crate writes no files.** It reads `fairing.toml` once at start, and the files your code
hands it, such as fonts. What should outlive a restart is yours to keep, wherever your device
keeps such things:

- A changed setting comes out as `ShellEvent::SettingChanged`. Hand it back at the next start
  with `Shell::restore_settings`.
- Credentials are your `Authenticator`'s to store.

## 7. Repaint and power

The shell draws only when something happens: input, a backend's `wake()`, or an animation in
progress. While something moves it draws at the display's rate, and the moment it stops the shell
drops to **0 fps**. The clock asks for the next minute boundary only when it is on screen. A
screen with `keep_awake` holds the display's idle timer off. How to confirm 0 fps at idle is in
[guide 08](guide/08-troubleshooting.md).

## 8. Errors

- **Library code does not panic.** `unwrap`, `expect`, `panic!` and unchecked indexing are
  denied by the workspace lints.
- A backend error comes back as a value, and the shell shows it as a toast or a notification.
- A configuration that cannot be right fails at start with `Error::Config`. Access control is
  never silently opened.
- Logging goes through the `log` facade. The logger is yours.

## 9. Testing and the audit

`testing::Harness` runs whole frames on `egui::Context` with virtual time, so a test taps and
drags where the shell actually drew, with no window, no GPU and no sleeping. The library's tests,
the examples and every Rust block in the guide run under `cargo xtask audit`, which also checks
formatting, lints, docs, dependencies, licenses, locks and blocking calls. CI runs it with
`--strict` on every push ([guide 08 §9](guide/08-troubleshooting.md#9-passing-the-audit)).

## 10. Principles

These are the rules the design keeps coming back to.

1. **Any touchscreen device is a target.** There is no list of target devices, so no default
   assumes one.
2. **Every way of touching is a target:** capacitive and resistive panels, bare fingers and
   gloves. Sizes are set in millimetres.
3. **Backends belong to the integrator.** The crate defines the traits and ships no system code.
4. **Customization happens in code.** Every part can be turned on, turned off or redrawn without
   forking the crate.
5. **No locks.** Threads talk over channels, and the UI thread never blocks.
6. **One general mechanism beats a one-off effect.** Implement a trait, and the library does the
   rest.
7. **The public surface is deliberate.** Nothing is public by accident.
8. **Defaults are chosen with care and stay adjustable** through the API.
9. **The crate writes no files of its own.**
10. **Code shown in the docs compiles,** and the audit checks it.

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
