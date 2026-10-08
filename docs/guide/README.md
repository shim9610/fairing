# fairing user guide

Working documentation for device integrators. It carries what you need to put `fairing` into your
own device binary, declare your screens, and reshape the shell into your product — nothing else.
How the crate is put together is in [architecture](../architecture.md), and what comes next, with
the limits it has today, is in the [roadmap](../roadmap.md).

Every Rust block on these pages is compiled — and run, unless it is marked `no_run` — by stage 3
of `cargo xtask audit`, which CI runs on every push and pull request: `crates/fairing/src/guide_probe.rs`
turns each page into doctests, with every feature on. So when the source moves, a block that no
longer matches it fails the audit instead of quietly becoming false. The blocks marked `ignore`
are of two kinds: **API outlines** (signatures without bodies, kept in step with the source by
hand) and code that needs something outside this repository (a crate such as `image` or
`zeroize`, or a font file of your own). Every `fairing.toml` fragment was put through
`ShellConfig::from_toml` + `validate` by hand, once. Where the code cannot do something yet, that
is written down as "it cannot".

> **What this guide describes is the code in the repository today.** Where something is planned
> but not in the code, the guide says so on the spot. Nothing has
> been published yet, so there is no migration to describe — see
> [§Coming from before 0.1](#coming-from-before-01).

## Reading order

Integrating for the first time, read in this order.

1. [01 Getting started](01-getting-started.md) — dependencies, features, a minimal app. That
   alone puts a screen up.
2. [02 Screens](02-screens.md) — declare your machine's screens and move between them.
3. [03 Chrome](03-chrome.md) — status bar, nav bar, desktop, shade, notifications, OSK, icons.
4. [07 Config reference](07-config-reference.md) — keep it open once you start writing
   `fairing.toml`.
5. As needed: [04 Customization](04-customization.md) (paint the shell yourself) ·
   [05 Access control](05-access-control.md) (lock screens) · [06 Services](06-services.md)
   (attach real hardware).
6. [08 Troubleshooting](08-troubleshooting.md) — when it does not come up or looks wrong, and at
   deployment and audit time.
7. Just before shipping: [09 Product branding](09-branding.md) — the routes to your machine's
   own character, and **the image specs to prepare**.

Once 01 §4's minimal app and 01 §6's headless harness are in hand, the rest can be read a section
at a time as you need it.

## Task shortcuts

| What you want to do | Page · section |
|---|---|
| Put the first screen up | [01 §4 A minimal app](01-getting-started.md#4-a-minimal-app) |
| Get the built-in settings screens, fonts, the panel's size | [01 §4.1–§4.3](01-getting-started.md#41-the-built-in-settings-screens) |
| React to what the shell did (`ShellEvent`) | [01 §4.5](01-getting-started.md#45-every-shellevent) |
| Add a hidden entry point for a service menu | [05 Hidden entry points](05-access-control.md#hidden-entry-points--the-service-menu) |
| Use our company colour | [09 §0 The decision table](09-branding.md#0-the-30-second-decision-table) |
| Use a background image (requirements) | [09 §2 Background images](09-branding.md#2-background-images--requirements) |
| Change the logo mark | [09 §5 Logo marks](09-branding.md#5-logo-marks--requirements) |
| Remove every trace of the brand | [09 §8.5](09-branding.md#85-checking-that-nothing-of-ours-is-left) |
| Start a new project from a file that runs (`examples/hello.rs`) | [01 §4](01-getting-started.md#4-a-minimal-app) · [01 §5](01-getting-started.md#5-running-the-bundled-examples) |
| Run the bundled examples (`hello` · `demo` · `console` · `kiosk` · `custom_chrome` · `motion_lab` · `palette_sheet`) | [01 §5](01-getting-started.md#5-running-the-bundled-examples) |
| Test my UI with no window and no GPU | [01 §6 Headless](01-getting-started.md#6-testing-your-ui-headless) |
| Add another screen | [02 §1 Five kinds of declaration](02-screens.md#1-five-kinds-of-declaration) |
| Decide where a screen's state lives | [02 §3 Resident and factory](02-screens.md#3-resident-and-factory-screens) |
| Pass a value between screens | [02 §6 Returning a value](02-screens.md#returning-a-value) · [02 §9.1](02-screens.md#91-sharing-app-state-across-screens) |
| Make one screen fullscreen | [02 §7 Per-screen chrome](02-screens.md#7-per-screen-chrome) |
| Put my own item in the status bar | [03 §1.4](03-chrome.md#14-adding-your-own-item) |
| Change the nav bar items | [03 §2.1](03-chrome.md#21-item-kinds) · [03 §2.5](03-chrome.md#25-adding-your-own-item) |
| Place desktop icons, and the dock | [03 §3.1](03-chrome.md#31-putting-icons-on-from-code) · [03 §3.2](03-chrome.md#32-overriding-from-config) |
| **Change the background** | [03 §3.7 Wallpaper](03-chrome.md#37-wallpaper) (flat, gradient, texture) · [04 §5](04-customization.md#5-painting-the-wallpaper-yourself) (procedural) |
| Add shade tiles, and order them | [03 §4.3](03-chrome.md#43-tile-order-and-count) · [03 §4.4](03-chrome.md#44-adding-your-own-tile) |
| Add a tile the crate has no kind for | [03 §4.4c](03-chrome.md#44c-tiles-the-crate-has-no-kind-for) |
| Bind a long press on a tile | [03 §4.4d](03-chrome.md#44d-long-pressing-a-tile) |
| Send notifications or toasts from another thread | [03 §5.3](03-chrome.md#53-sending-from-another-thread) |
| Change the keyboard layout | [03 §6.2](03-chrome.md#62-layouts) |
| Register a custom icon | [03 §8.3](03-chrome.md#83-registering-custom-icons) · to grow the built-in set, [03 §8.4](03-chrome.md#84-adding-a-new-svg-to-the-built-in-set) |
| Change only the colours | [04 §2 `[theme.palette]`](04-customization.md#2-changing-only-the-colours-with-themepalette) |
| Set palette, metrics and motion wholesale in code | [04 §1 Injecting a theme](04-customization.md#1-injecting-a-theme) |
| Paint the status bar or nav bar myself | [04 §3](04-customization.md#3-painting-a-whole-bar-yourself) |
| Paint desktop cells myself | [04 §4](04-customization.md#4-painting-desktop-slots-yourself) |
| Move or redraw the toasts, the banner, the keys, the shade's tiles and panel, the status bar's items | [04 §10](04-customization.md#10-moving-and-redrawing-the-other-pieces) · [04 §10.4](04-customization.md#104-the-shades-tiles-and-panel) · [04 §10.5](04-customization.md#105-the-status-bars-items) |
| Draw the lock screen and the unlock prompt my way | [04 §10.6](04-customization.md#106-the-lock-screen-and-the-unlock-prompt) |
| Draw the recent screens my way | [04 §10.7](04-customization.md#107-the-recent-screens) |
| Draw the widgets my way | [04 §10.8](04-customization.md#108-the-widgets) |
| Turn off the shade, the OSK, gestures | [04 §6.1](04-customization.md#61-what-can-be-turned-off-and-how) |
| Add a gesture of my own | [03 §9.2](03-chrome.md#92-gesture-handles) · [§9.3](03-chrome.md#93-gesture-regions-of-your-own) |
| Tune how the animation feels | [04 §8 Tuning motion](04-customization.md#8-tuning-motion) |
| Lock screens behind levels | [05 §2 Gates](05-access-control.md#2-gates) |
| Attach PIN or card-reader authentication | [05 §5 the prompt](05-access-control.md#5-the-shells-prompt-and-the-authenticator) · [05 §6 routing](05-access-control.md#6-doing-authentication-yourself-with-routing) |
| Develop with no hardware, on mocks | [06 §3 Assembling `Services`](06-services.md#3-assembling-services) |
| Attach real Wi-Fi, power and brightness | [06 §5 Writing a real backend](06-services.md#5-writing-a-real-backend) |
| Find a config key's name or default | [07 §1 The sections](07-config-reference.md#1-the-sections) · [07 §13 A complete example](07-config-reference.md#13-a-complete-example) |
| Configure in code, with no file | [07 §14](07-config-reference.md#14-configuring-in-code-with-no-file) |
| Look up a startup error message | [08 §1 Errors you will actually hit](08-troubleshooting.md#1-errors-you-will-actually-hit) |
| Korean renders as tofu (□) | [08 §6](08-troubleshooting.md#6-korean-renders-as-tofu) |
| It is slow | [08 §8 Checking performance](08-troubleshooting.md#8-checking-performance) |
| Pass the audit (`xtask audit`) | [08 §9](08-troubleshooting.md#9-passing-the-audit) |
| Cross-compile for aarch64, ship a snap | [08 §10](08-troubleshooting.md#10-deployment) |

## The pages

| Page | In one line |
|---|---|
| [01 Getting started](01-getting-started.md) | Requirements · `Cargo.toml` · features · the release profile · a minimal app · running the examples · the headless harness · benches |
| [02 Screens](02-screens.md) | Five kinds of declaration · the `ScreenDecl` builder · resident vs factory · `Cx` · lifecycle · stack navigation · per-screen chrome · common patterns |
| [03 Chrome](03-chrome.md) | Status bar · nav bar · desktop · shade · notifications and toasts · OSK · touch widgets · icons · gestures |
| [04 Customization](04-customization.md) | Injecting a `Theme` · `[theme.palette]` · bar, slot and wallpaper painters · turning parts off · tuning motion · text and translations · layouts and painters for the nav bar, toasts, the banner, the keyboard and the shade's tiles |
| [05 Access control](05-access-control.md) | The level table · gates · `Visibility` · `AccessPolicy` · the unlock prompt and `Authenticator` · the `routing` flow · the session and the lock screen · audit logging |
| [06 Services](06-services.md) | The backend traits · `Capabilities` · assembling `Services` · mocks · writing a real backend · setting values · clocks |
| [07 Config reference](07-config-reference.md) | Every section and key of `fairing.toml`, with defaults, validation rules and a complete example |
| [08 Troubleshooting](08-troubleshooting.md) | Error messages · silent failures · confirming 0 fps at idle · fonts · performance · the 14 audit stages · deployment · FAQ |
| [09 Product branding](09-branding.md) | Image loaders · background, type, icon and mark requirements · the override table · the pre-ship review |

## What exists and what does not

Milestones M0 to M5 are done, and M6 is down to the 0.1.0 release. The milestone table, what
comes after 0.1 and the known limits are in the [roadmap](../roadmap.md). The guide writes down
what is not there as "it cannot" and shows the workaround that exists today.

## Coming from before 0.1

Nothing has been published yet, so this guide has no migration notes: it describes the code as
it stands. Two things a reader of older notes may trip on:

- **Lengths are in du** — density-independent units, egui's points. Touch targets and bar sizes
  are written in millimetres and resolve to du once the panel's size is known
  (`ShellBuilder::physical_mm`, [01 §4](01-getting-started.md#4-a-minimal-app)); until then the
  shell assumes a density and says so in the log.
- **Part of the M2b plan was not built.** Planned only: the generic bars (a `Bar` on any edge,
  rows of bars, bar layouts) and the desktop regions; the `[metrics]`, `[device]`, `[display]`
  and `[scale]` sections; the `diag` feature and its scale reports; a closure over the resolved
  metrics (`metrics_fn`); and `Cx`'s unit helpers (`cx.mm`, `cx.units`) — a screen reads the
  resolved sizes from `cx.theme.metrics`. The shell has the status bar and the nav bar, the dock
  and the icon rail, and its metrics are set in code (`ShellBuilder::metrics_spec`,
  [04 §1](04-customization.md#1-injecting-a-theme)); only the three bar sizes also have TOML keys
  ([03 §1.2](03-chrome.md#12-slot-placement), [03 §2.1](03-chrome.md#21-item-kinds)). The
  [roadmap](../roadmap.md#3-after-01) lists them.
