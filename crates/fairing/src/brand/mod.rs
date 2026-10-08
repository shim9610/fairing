//! Branding — the manta mark and the Abyss procedural background. Feature `brand`.
//!
//! **Rule G**: this module holds **only drawing code that uses egui**. The
//! types deserialised from config ([`crate::config::AbyssTier`]), the palette presets
//! ([`crate::theme::Preset`]) and the [`Wallpaper`](crate::desktop::Wallpaper) variants all live
//! outside the gate, so that turning `brand` off still parses the config and keeps the palette.
//!
//! **The brand is opt-in**. Write nothing and nothing is installed — the
//! default configuration has neither the manta nor the abyss in it.

mod abyss;
mod manta;
mod noise;
mod ribbon;

pub use crate::config::AbyssTier;
pub use abyss::{abyss_wallpaper, Abyss, AbyssColors, AbyssParams};
pub use manta::{manta_painter, MantaLod, MantaStyle, MantaTones};
pub use ribbon::Cubic;
