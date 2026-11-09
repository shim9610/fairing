# fairing user guide

Integration documentation for using `fairing` in a device application: registering
screens, configuring the shell and connecting application-owned backends.
Component ownership and execution flow are in [system architecture](../architecture.md).
Authentication, token authority and protected admission are in
[security architecture](../security-architecture.md). Both include SVG diagrams.
Planned work and current limits are in the [roadmap](../roadmap.md).

## Choose a shell mode

| Integration | Start here |
|---|---|
| Ordinary UI with trusted application callbacks and optional legacy level gates | [01 Getting started](01-getting-started.md), then [02 Screens](02-screens.md) |
| Issuer-controlled sessions with typed requirements checked at managed UI entry points | [Session tokens](../session-tokens.md), then [typed registration](../session-tokens.md#typed-permission-requirements) and the [`protected` example](../../crates/fairing/examples/protected.rs) |
| Existing application updating from an earlier checkout | [10 Migration](10-migration.md) |

Choose the mode before copying registration examples. Chapters 01–04 primarily
use ordinary `Shell`; their untyped hooks cannot be copied unchanged into
`ProtectedShell`. Both modes can use the runners and headless harness. Authentication,
permission assignment and device backends are supplied by the application.

## Reading order

Integrating for the first time, read in this order.

1. [01 Getting started](01-getting-started.md) — dependencies, features, a minimal app. That
   alone puts a screen up.
2. [02 Screens](02-screens.md) — declare your machine's screens and move between them.
3. [03 Chrome](03-chrome.md) — status bar, nav bar, desktop, shade, notifications, OSK, icons.
4. [07 Config reference](07-config-reference.md) — keep it open once you start writing
   `fairing.toml`.
5. As needed: [04 Customization](04-customization.md) (paint the shell yourself) ·
   [05 Access control](05-access-control.md) (sessions, permissions and lock screens) · [06 Services](06-services.md)
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
| Use our company colour | [09 §0 Branding options](09-branding.md#0-branding-options) |
| Use a background image (requirements) | [09 §2 Background images](09-branding.md#2-background-images--requirements) |
| Change the logo mark | [09 §5 Logo marks](09-branding.md#5-logo-marks--requirements) |
| Customize the branded surfaces | [09 §8.5](09-branding.md#85-checking-that-nothing-of-ours-is-left) |
| Start a new project from a file that runs (`examples/hello.rs`) | [01 §4](01-getting-started.md#4-a-minimal-app) · [01 §5](01-getting-started.md#5-running-the-bundled-examples) |
| Run the bundled examples (`hello` · `protected` · `demo` · `console` · `kiosk` · `custom_chrome` · `motion_lab` · `palette_sheet`) | [01 §5](01-getting-started.md#5-running-the-bundled-examples) |
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
| Use issuer-controlled roles, revocation and independent device/web login | [Session tokens](../session-tokens.md) |
| Register typed screens and nested permission-aware UI | [Typed requirements](../session-tokens.md#typed-permission-requirements) |
| Verify protected entry routes or application permission metadata | [Common mediation contract](../protected-contracts.md) · [Application checker](../permission-metadata-check.md) |
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
| Separate UI commands and business updates through your own transport | [06 Application-owned UI/business boundary](06-services.md#application-owned-uibusiness-boundary) |
| Handle a callback panic or replace a quarantined shell | [08 Runtime faults and replacement](08-troubleshooting.md#runtime-faults-and-replacement) |
| Update an app written against an earlier checkout | [10 Migration](10-migration.md) |

## The pages

| Page | In one line |
|---|---|
| [01 Getting started](01-getting-started.md) | Requirements · `Cargo.toml` · features · the release profile · a minimal app · running the examples · the headless harness · benches |
| [02 Screens](02-screens.md) | Five kinds of declaration · the `ScreenDecl` builder · resident vs factory · `Cx` · lifecycle · stack navigation · per-screen chrome · common patterns |
| [03 Chrome](03-chrome.md) | Status bar · nav bar · desktop · shade · notifications and toasts · OSK · touch widgets · icons · gestures |
| [04 Customization](04-customization.md) | Injecting a `Theme` · `[theme.palette]` · bar, slot and wallpaper painters · turning parts off · tuning motion · text and translations · layouts and painters for the nav bar, toasts, the banner, the keyboard and the shade's tiles |
| [05 Access control](05-access-control.md) | Legacy levels, gates and prompts · `Authenticator` and routing · session/lock behavior · issuer-controlled sessions and typed UI links |
| [06 Services](06-services.md) | The backend traits · `Capabilities` · assembling `Services` · mocks · writing a real backend · setting values · clocks |
| [07 Config reference](07-config-reference.md) | Every section and key of `fairing.toml`, with defaults, validation rules and a complete example |
| [08 Troubleshooting](08-troubleshooting.md) | Error messages · silent failures · idle repaint behavior · fonts · performance · the 14 audit stages · deployment · FAQ |
| [09 Product branding](09-branding.md) | Image loaders · background, type, icon and mark requirements · the override table · the pre-ship review |
| [10 Migration](10-migration.md) | Request/session tokens · permission values and protected hooks · deferred factories/opaque screen ownership · command/display admission · transport and recovery |

## Examples and compatibility

This guide describes the current checkout before the first published release.
The [migration guide](10-migration.md) compares it with the earlier `f4ca28c`
checkout; the [roadmap](../roadmap.md) separates implemented features from plans.

Rust blocks are compiled by the repository's guide doctest gate unless marked
`ignore`; `no_run` blocks are compiled without execution. Ignored blocks are API
outlines or examples that need external crates or assets. An API outline lists
selected members, not a complete constructible definition.

The Rust doctest gate does not check TOML blocks. Cargo snippets belong in the
application manifest. Device-configuration snippets belong in `fairing.toml` and
need validation as part of the complete configuration with `ShellConfig::from_toml`
and shell construction. See [Contributing](../../CONTRIBUTING.md#keeping-the-documents-up-to-date)
for repository checks and [release checks](../release-checks.md) for package validation.
