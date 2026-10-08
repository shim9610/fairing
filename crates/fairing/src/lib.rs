//! `fairing` — a touchscreen shell for embedded devices, built on `egui`.
//!
//! The status bar, the pull-down shade with quick-settings tiles and notifications
//! ([`overlay`]), the desktop, a screen stack with transitions, the navigation bar, an
//! on-screen keyboard with numeric, QWERTY and two-set Hangul layouts ([`osk`]), toasts and
//! heads-up banners ([`notify`]), device settings screens and access gates — the parts every
//! kiosk, panel and bench instrument rebuilds, in one process and one `egui` frame loop.
//!
//! It is not an OS, an app launcher or a window manager. A screen is a closure or a [`Screen`]
//! you register, and fairing owns everything around it: the layout, touch targets sized in
//! millimetres ([`unit`](mod@unit)), one gesture engine for edges, flings and long presses
//! ([`gesture`]), the transitions, the gates and the repaint policy. Every chrome surface can be
//! reconfigured, declared or repainted without forking the crate, and [`testing::Harness`] runs
//! frames with no window and no GPU, so a test taps and drags where the shell actually drew.
//!
//! The [integrator guide](https://github.com/shim9610/fairing/tree/HEAD/docs/guide) in the
//! repository is the place to start. How the crate is put together is in
//! [architecture](https://github.com/shim9610/fairing/blob/HEAD/docs/architecture.md), and
//! what comes next is in the
//! [roadmap](https://github.com/shim9610/fairing/blob/HEAD/docs/roadmap.md).
//!
//! # Features
//!
//! | Feature | Default | What it adds |
//! |---|---|---|
//! | `overlay` | on | The shade: quick-settings tiles, the notification list, the scrim |
//! | `osk` | on | The on-screen keyboard |
//! | `settings` | on | The built-in settings screens |
//! | `brand` | on | The manta mark (a ribbon mesh) and the Abyss procedural background. The finished app-icon, splash and wallpaper images are files in `assets/brand/`, outside the crate |
//! | `mock` | on | Mock backends for Wi-Fi, Bluetooth, display and power, for development |
//! | `runner` | off | An eframe window bootstrap, `runner::run_shell` |
//! | `runner-x11` | off | The same, with X11 as well as Wayland on Linux |
//! | `chrono` | off | A clock that follows the system time zone and daylight saving |
//!
//! # Example
//!
//! ```no_run
//! use fairing::{screen, icon, Cx};
//!
//! let ctx = egui::Context::default();
//! let mut shell = fairing::Shell::new(
//!     fairing::ShellConfig::default(),
//!     fairing::Services::builder().build(), // SystemClock for the clock, Null for the rest (Services::null() is all-Null)
//!     &ctx,
//! )?;
//! shell.add(
//!     screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx| {
//!         ui.heading("Dashboard");
//!         if ui.button("Hello").clicked() {
//!             cx.shell.toast("Hello");
//!         }
//!     })
//!     .title("Dashboard")
//!     .icon(icon::GAUGE)
//!     .desktop()
//!     .dock(),
//! );
//! # Ok::<(), fairing::Error>(())
//! ```

#![forbid(unsafe_code)]
// A `pub` item has to be reachable from the crate root — anything else is `pub(crate)`.
// The audit's clippy stage denies warnings, so this keeps the public surface to what is meant.
// It sits here rather than in the workspace lints because the examples, tests and xtask are
// binaries, where nothing is reachable and the lint would only flag their helpers.
#![warn(unreachable_pub)]
// On docs.rs every feature-gated item carries its feature (`runner`, `osk`, `overlay`, …) as a
// badge; elsewhere the attribute is inert.
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod access;
#[cfg(feature = "brand")]
pub mod brand;
pub mod chrome;
pub mod config;
pub mod desktop;
pub mod gesture;
pub mod i18n;
pub mod inbox;
pub mod layout;
pub mod notify;
pub mod prelude;
pub mod screen;
pub mod services;
pub mod settings;
pub mod shell;
pub mod testing;
pub mod time;
pub mod workspace;

#[cfg(feature = "osk")]
pub mod osk;
#[cfg(feature = "overlay")]
pub mod overlay;

#[cfg(feature = "runner")]
pub mod runner;

/// **egui, re-exported verbatim**.
///
/// Screen closures receive an `&mut egui::Ui`, so the integrator has to get hold of egui one way
/// or another. Left to take a direct dependency, a version mismatch links **two copies and the
/// types stop matching** — and that failure is hard to diagnose, because the compiler says `Ui`
/// is not `Ui`. Going through this re-export (`fairing::egui::Ui`) makes a mismatch impossible.
pub use egui;

/// **The element layer, re-exported verbatim.**
///
/// [`unit`](mod@unit), [`motion`], [`theme`], [`icons`], [`fonts`], [`widgets`] and [`drag`]
/// live in [`fairing_widgets`], so that the design of a control can be worked on without the
/// shell in the way. They are re-exported here under the names they have always
/// had, so integrator code does not change: `fairing::unit::Dim` is `fairing_widgets::unit::Dim`.
pub use fairing_widgets;
pub use fairing_widgets::{drag, error, fonts, icons, motion, theme, unit, widgets};

pub use access::{AccessEvent, Gate, Level, LevelTable, Subject, Visibility};
pub use chrome::{
    nav_item, status_item, BarCx, BarItem, BarKind, NavLayoutCx, Slot, StatusLayoutCx,
};
pub use config::ShellConfig;
pub use desktop::{SlotCx, Wallpaper};
pub use error::{Error, Result};
pub use fonts::{FontSet, FontSource};
pub use icons::builtin as icon;
pub use icons::IconRef;
pub use notify::{Notification, NotificationId, Toast};
#[cfg(feature = "overlay")]
pub use overlay::{tile, tile_panel, Gauge, TileKind};
pub use screen::{
    action, screen, screen_of, screen_with, BackAction, BarMode, ChromePolicy, Cx, LaunchAction,
    LaunchMode, Lifecycle, OskMode, PaneInfo, Screen, ScreenDecl, ScreenValue, SplitSupport,
};
pub use services::{PowerRequest, Services};
pub use settings::{SettingKey, SettingValue};
pub use shell::{Layout, Shell, ShellBuilder, ShellEvent, ShellHandle};
pub use theme::{ColorRole, Theme};
pub use unit::{Dim, Scale, ScalePolicy, Span};
pub use workspace::InstanceId;

/// The version string from this crate's `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// The guide doctest gate: every Rust block in docs/guide is compiled — and run, unless it is
// marked `no_run` — as a doctest of this module. The guide is written against the whole feature
// set (it shows the runner, chrono, the shade and the keyboard), so the gate needs every feature
// on: the audit's stage 3 and CI run it, and a plain `cargo test` skips it.
#[cfg(all(
    doctest,
    feature = "mock",
    feature = "overlay",
    feature = "osk",
    feature = "brand",
    feature = "settings",
    feature = "runner",
    feature = "chrono"
))]
mod guide_probe;
