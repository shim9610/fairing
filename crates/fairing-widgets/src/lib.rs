//! Touch widgets, theme tokens and a physical unit system for `egui` — the element layer
//! behind the [fairing](https://docs.rs/fairing) touchscreen shell, usable on its own in any
//! egui app.
//!
//! Every control is sized for a finger on a real panel: sizes resolve through millimetres and
//! the viewing distance ([`unit`](mod@unit)), not pixels.
//!
//! # Using a widget
//!
//! A widget is drawn with a [`WidgetCx`]: the theme, the icon set and the animation store,
//! which your app keeps between frames.
//!
//! ```
//! use fairing_widgets::icons::IconSet;
//! use fairing_widgets::motion::AnimationStore;
//! use fairing_widgets::theme::Theme;
//! use fairing_widgets::widgets::{Stepper, Switch};
//! use fairing_widgets::WidgetCx;
//!
//! let theme = Theme::light();
//! let mut icons = IconSet::new();
//! let mut anims = AnimationStore::new();
//! let (mut on, mut count) = (true, 3);
//!
//! # let ctx = egui::Context::default();
//! # let output = ctx.run_ui(egui::RawInput::default(), |ui| {
//! // Inside your egui update, where you have a `ui`:
//! let mut cx = WidgetCx {
//!     theme: &theme,
//!     icons: &mut icons,
//!     anims: &mut anims,
//!     anim_scope: egui::Id::new("my-app"),
//!     frame: 0,
//!     inset_bottom: 0.0,
//!     painters: None,
//! };
//! if Switch::new(&mut on).show(ui, &mut cx).changed() {
//!     // `on` was flipped.
//! }
//! let _ = Stepper::new(&mut count).show(ui, &mut cx);
//! # });
//! # output.drop_without_applying_deltas();
//! ```
//!
//! # Why this is a crate of its own
//!
//! Everything here answers one question — *what does a control look like, and how big is it on
//! this panel* — and nothing here knows the shell exists. Measured before the split, the widgets
//! reached outside themselves for exactly three things (`theme`, `icons` and an animation call);
//! every other reference to the shell was a doc link. Keeping that boundary as a crate boundary is
//! what stops shell concepts leaking back into element code, and it lets the element design be
//! worked on in its own cycle without touching the shell at all.
//!
//! # What is in here
//!
//! - [`unit`](mod@unit) — the `du` / `mm` / finger unit system and the [`Scale`](unit::Scale) that resolves it.
//! - [`motion`] — tweens, springs and drag arithmetic.
//! - [`drag`] — whose drag it is: a control that follows the finger says so, and a gesture read
//!   off the raw pointer leaves that drag alone.
//! - [`theme`] — the palette roles, the metric tokens and their physical spec.
//! - [`icons`] — the built-in icon set and the polyline cache.
//! - [`fonts`] — font installation, including the bold face the weight axis needs.
//! - [`widgets`] — the touch controls.
//!
//! The shell re-exports all of it, so integrator code says `fairing_widgets::…` as it always did.

#![forbid(unsafe_code)]
// A `pub` item has to be reachable from the crate root — anything else is `pub(crate)`.
// The audit's clippy stage denies warnings, so this keeps the public surface to what is meant.
// It sits here rather than in the workspace lints because the examples, tests and xtask are
// binaries, where nothing is reachable and the lint would only flag their helpers.
#![warn(unreachable_pub)]

pub mod config;
mod cx;
pub mod drag;
pub mod error;
pub mod fit;
pub mod fonts;
pub mod icons;
pub mod motion;
pub mod theme;
pub mod unit;
pub mod widgets;

pub use cx::WidgetCx;
pub use error::{Error, Result};
