//! **The names most apps reach for, in one import**.
//!
//! ```no_run
//! use fairing::prelude::*;
//!
//! fn dashboard() -> ScreenDecl {
//!     screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
//!         ui.heading("Dashboard");
//!         if ui.button("Hello").clicked() {
//!             cx.shell.toast("Hello");
//!         }
//!     })
//!     .title("Dashboard")
//!     .icon(icon::GAUGE)
//!     .desktop()
//! }
//!
//! fn handle(shell: &mut Shell) {
//!     for event in shell.poll_events() {
//!         match event {
//!             ShellEvent::ScreenOpened { id, instance } => println!("{id} opened ({instance:?})"),
//!             ShellEvent::SettingChanged { key, value } => println!("{key:?} = {value:?}"),
//!             ShellEvent::PowerRequest(request) => println!("power: {request:?}"),
//!             _ => {}
//!         }
//!     }
//! }
//! # let _ = (dashboard, handle);
//! ```
//!
//! What is in it: building and running the shell, declaring screens and items, what a screen is
//! handed, the events with everything they carry, the look (built-in icons, palette roles, units),
//! the [`layout`] and [`widgets`] modules, and egui itself. Every name here is also at its own
//! path, and none means anything different for being imported this way.
//!
//! **Left out on purpose:** [`crate::Result`]. Its single parameter would hide the standard
//! two-parameter `Result` in every module that imports this; write `fairing::Result` where you
//! want it.

pub use crate::egui;
pub use crate::{
    action, nav_item, screen, screen_of, screen_with, status_item, AccessEvent, BarMode,
    ChromePolicy, ColorRole, Cx, Dim, Error, IconRef, InstanceId, LaunchAction, Lifecycle,
    Notification, NotificationId, OskMode, PowerRequest, Screen, ScreenDecl, Services, SettingKey,
    SettingValue, Shell, ShellBuilder, ShellConfig, ShellEvent, ShellHandle, Slot, Span, Theme,
    Toast,
};
pub use crate::{icon, layout, widgets};
#[cfg(feature = "overlay")]
pub use crate::{tile, TileKind};
