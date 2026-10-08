//! Settings — the value model and the built-in settings screens.
//!
//! # Two layers
//!
//! **The value model** ([`SettingKey`], [`SettingValue`], [`SettingsView`], [`keys`]) is outside
//! the feature gate — [`Cx::settings`](crate::Cx::settings) has to always be there. It is one
//! in-memory table, and **the shell writes no files**: a change reaches the integrator as
//! [`ShellEvent::SettingChanged`](crate::ShellEvent::SettingChanged), to store where the device
//! keeps such things, and [`Shell::restore_settings`](crate::Shell::restore_settings) hands the
//! stored values back at start.
//!
//! **The built-in screens** ([`add_all`]) are behind feature `settings`. Turn it off and they
//! leave the build, and the integrator registers their own screens under the same ids.
//!
//! # All of it can be overridden
//!
//! [`Shell::add`](crate::Shell::add) **replaces the same id**, so swapping a
//! built-in screen out needs no special API:
//!
//! ```no_run
//! # fn main() {
//! # let shell: &mut fairing::Shell = todo!();
//! use fairing::screen;
//! use fairing::settings::{add_all, SettingsConfig};
//!
//! add_all(shell, &SettingsConfig::default());          // the built-ins
//! shell.add(screen("settings.wifi", my_wifi_screen));  // one of them replaced with ours
//! # fn my_wifi_screen(_: &mut egui::Ui, _: &mut fairing::Cx<'_>) {}
//! # }
//! ```
//!
//! You can also never call it, or drop screens with [`SettingsConfig::without`]. The shallow
//! routes — colours, metrics, labels — are tabulated in guide 09 §8.
//!
//! # The list itself is yours
//!
//! Which rows `settings.home` offers is a `Vec` you own:
//! [`screens::entries`] gives the built-in one, and [`add_all_with`] takes whatever you make of
//! it — rows out, rows of your own in, in whatever order.
//!
//! # A built-in is a template, not a wall
//!
//! Replacing one is the case above. Keeping one **and** having a variant of it beside the original
//! takes two more routes, and both are open:
//!
//! ```no_run
//! # fn main() {
//! # let shell: &mut fairing::Shell = todo!();
//! use fairing::screen;
//! use fairing::settings::screens;
//!
//! // The whole declaration under an id of your own — body, title, icon, gate, chrome.
//! shell.add(screens::display().with_id("app.display").title("Panel"));
//!
//! // Or its body alone, with rows of your own around it. `page` is yours to apply.
//! shell.add(screen("app.panel", |ui: &mut egui::Ui, cx: &mut fairing::Cx<'_>| {
//!     fairing::layout::page(ui, cx, "app.panel", |ui, cx| {
//!         fairing::layout::section(ui, cx, "Ours");
//!         let _ = fairing::layout::info_row(ui, cx, "Panel", "7 inch");
//!         screens::display_body(ui, cx);
//!     });
//! }));
//! # }
//! ```
//!
//! `tests/m2h_integrator_routes.rs` walks all three.

#[cfg(feature = "settings")]
pub mod screens;

use std::borrow::Cow;
use std::collections::BTreeMap;

/// A setting key (`"display.brightness"`, `"app.heater"`, …). Integrator keys live in the `app.*` namespace.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettingKey(pub Cow<'static, str>);

impl From<&'static str> for SettingKey {
    fn from(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }
}

impl From<String> for SettingKey {
    fn from(value: String) -> Self {
        Self(Cow::Owned(value))
    }
}

/// A setting value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SettingValue {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A string.
    Text(String),
}

/// The read view over settings: one in-memory table of what was set or restored since start, with
/// `ui.clock_12h` seeded from `[status_bar] clock_format`.
#[derive(Debug, Clone, Default)]
pub struct SettingsView {
    values: BTreeMap<SettingKey, SettingValue>,
}

impl SettingsView {
    /// Read a value.
    #[must_use]
    pub fn get(&self, key: &SettingKey) -> Option<&SettingValue> {
        self.values.get(key)
    }

    /// Put a value in — this table only: no gate, no backend, no event. The shell's own paths are
    /// [`Shell::set_setting`](crate::Shell::set_setting) and
    /// [`Shell::restore_settings`](crate::Shell::restore_settings).
    pub fn set(&mut self, key: impl Into<SettingKey>, value: SettingValue) {
        self.values.insert(key.into(), value);
    }
}

/// The built-in setting keys: the ones the shell itself acts on — a backend, the
/// theme, the language, a tile or the policy. A gate name is the key name.
pub mod keys {
    /// Wi-Fi on (`tile.wifi` → `WifiBackend::set_enabled`).
    pub const WIFI_ENABLED: &str = "wifi.enabled";
    /// Bluetooth on (`tile.bluetooth` → `BluetoothBackend::set_enabled`).
    pub const BLUETOOTH_ENABLED: &str = "bluetooth.enabled";
    /// Brightness 0..=100 (`tile.brightness` → `DisplayBackend::set_brightness`).
    pub const DISPLAY_BRIGHTNESS: &str = "display.brightness";
    /// Volume 0..=100 (`tile.volume`, `settings.sound` → `AudioBackend::set_volume`).
    pub const AUDIO_VOLUME: &str = "audio.volume";
    /// Muted (`settings.sound` → `AudioBackend::set_muted`).
    pub const AUDIO_MUTED: &str = "audio.muted";
    /// Dark theme (`tile.theme` → the theme crossfade, A7).
    pub const THEME_DARK: &str = "theme.dark";
    /// Silent: a new notification asks the audio backend for no cue (`Cue::Notify`). The banner
    /// still shows.
    pub const UI_SILENT: &str = "ui.silent";
    /// Airplane mode = every radio off (`tile.airplane`).
    pub const RADIO_AIRPLANE: &str = "radio.airplane";
    /// Rotation lock (`tile.rotation_lock`, where the backend supports it).
    pub const DISPLAY_ROTATION_LOCK: &str = "display.rotation_lock";
    /// **A 12-hour clock** — `false` or absent means 24-hour (the date & time row).
    ///
    /// The built-in `settings.datetime` screen writes it and **the status bar clock reads it**: it
    /// turns whichever shape `[status_bar] clock_format` configured into its other half
    /// ([`ClockFormat::with_hour12`](crate::time::ClockFormat::with_hour12)). It was a
    /// bare string in the settings screen with nothing reading it at all.
    pub const UI_CLOCK_12H: &str = "ui.clock_12h";
    /// **The language** (`"en"`, `"ko"`, or a table of yours). `settings.locale`
    /// writes it and the shell switches its string table on the spot: the next frame is in the
    /// new language. Until something writes it the language is `[shell] locale`;
    /// `cx.strings.locale()` is the one in use either way.
    pub const UI_LOCALE: &str = "ui.locale";
}

/// How the built-in settings screens are registered. Feature `settings`.
///
/// The default is **register everything**, and [`add_all`] drops the ones whose backend is
/// missing.
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// use fairing::settings::{add_all, SettingsConfig};
///
/// // We ship our own Wi-Fi screen, so register everything but the built-in one.
/// add_all(shell, &SettingsConfig::default().without("settings.wifi"));
/// # }
/// ```
#[cfg(feature = "settings")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsConfig {
    /// Screen ids to leave out of the registration.
    excluded: std::collections::BTreeSet<String>,
    /// Whether to put the `settings.home` icon on the desktop. `true` by default.
    ///
    /// Turned off, the screen registers but no icon appears — for putting it on the dock
    /// yourself, or opening it only through a hidden entry point.
    pub home_icon: bool,
    /// Whether to register screens whose backend is missing. `false` by default.
    ///
    /// Turned on, a Wi-Fi entry appears with no Wi-Fi backend and there is nothing behind it.
    /// There is no reason to turn it on except to look at the screens during development.
    pub ignore_capabilities: bool,
}

#[cfg(feature = "settings")]
impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            excluded: std::collections::BTreeSet::new(),
            home_icon: true,
            ignore_capabilities: false,
        }
    }
}

#[cfg(feature = "settings")]
impl SettingsConfig {
    /// Register everything but this screen. Can be called repeatedly.
    #[must_use]
    pub fn without(mut self, id: impl Into<String>) -> Self {
        self.excluded.insert(id.into());
        self
    }

    /// Register only these screens. The opposite of `without`.
    #[must_use]
    pub fn only(ids: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        let keep: std::collections::BTreeSet<String> =
            ids.into_iter().map(|s| s.as_ref().to_owned()).collect();
        let excluded = screens::ALL
            .iter()
            .filter(|id| !keep.contains(**id))
            .map(|id| (*id).to_owned())
            .collect();
        Self {
            excluded,
            ..Self::default()
        }
    }

    /// Without the desktop icon.
    #[must_use]
    pub const fn without_home_icon(mut self) -> Self {
        self.home_icon = false;
        self
    }

    /// Register everything, ignoring what the backends support (for development).
    #[must_use]
    pub const fn ignoring_capabilities(mut self) -> Self {
        self.ignore_capabilities = true;
        self
    }

    /// Whether this id is registered.
    #[must_use]
    pub fn includes(&self, id: &str) -> bool {
        !self.excluded.contains(id)
    }
}

/// **Register the built-in settings screens in one call**. Feature `settings`.
///
/// A gate name is the screen id (the default for
/// [`ScreenDecl::gate`](crate::screen::ScreenDecl::gate)), and which level it takes is the
/// integrator's to assign in `[access.gates]`.
///
/// # No backend, no registration
///
/// The worst outcome is a Wi-Fi entry on a machine with no Wi-Fi backend that says "not
/// supported" once you press it. This checks
/// [`Capabilities`](crate::services::Capabilities) and filters in advance — turn that off with
/// [`SettingsConfig::ignoring_capabilities`].
///
/// # All of it can be overridden
///
/// Call `shell.add(..)` with the same id **after** this function and that one screen is replaced
/// entirely ([`Shell::add`](crate::Shell::add) replaces the same id).
#[cfg(feature = "settings")]
pub fn add_all(shell: &mut crate::Shell, config: &SettingsConfig) {
    add_all_with(shell, config, screens::entries());
}

/// **The same, over a settings list of your own**. Feature `settings`.
///
/// [`screens::entries`] gives the built-in list; take rows out of it, put rows in, move them
/// about, and the `settings.home` this registers draws that list instead. Everything else —
/// which screens are registered, the capability filter, the desktop icon — works exactly as in
/// [`add_all`].
///
/// A row is drawn only where a screen really is registered under its id, so a row of your own
/// wants a screen of your own with it:
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// use fairing::screen;
/// use fairing::icon;
/// use fairing::settings::screens::{entries, SettingsEntry};
/// use fairing::settings::{add_all_with, SettingsConfig};
///
/// let mut list = entries();
/// list.retain(|entry| entry.id != "settings.locale");   // taken out
/// list.push(SettingsEntry::new("app.heater", icon::GAUGE, "Heater")); // put in
///
/// shell.add(screen("app.heater", |ui: &mut egui::Ui, _: &mut fairing::Cx<'_>| {
///     ui.label("heater");
/// }));
/// add_all_with(shell, &SettingsConfig::default(), list);
/// # }
/// ```
#[cfg(feature = "settings")]
pub fn add_all_with(
    shell: &mut crate::Shell,
    config: &SettingsConfig,
    entries: Vec<screens::SettingsEntry>,
) {
    use crate::services::Capabilities;

    // The capabilities are asked of **each backend** — only the Wi-Fi backend knows whether there is Wi-Fi.
    let force = config.ignore_capabilities;
    // Likewise only the authenticator knows whether it manages its entries here.
    let has_admin = force || shell.access_mut().has_admin();
    let services = shell.services();
    let has_wifi = force || services.wifi.capabilities().contains(Capabilities::WIFI);
    let has_bt = force
        || services
            .bluetooth
            .capabilities()
            .contains(Capabilities::BLUETOOTH);
    let has_network = force
        || services
            .network
            .capabilities()
            .contains(Capabilities::ETHERNET);
    let power_caps = services.power.capabilities();
    let has_power = force
        || power_caps.contains(Capabilities::POWER_CONTROL)
        || power_caps.contains(Capabilities::BATTERY);

    // (id, the building function, the capability needed)
    let mut decls: Vec<crate::screen::ScreenDecl> = Vec::new();
    let mut home = screens::home_with(entries);
    if config.home_icon {
        home = home.desktop();
    }
    decls.push(home);
    if has_wifi {
        decls.push(screens::wifi());
    }
    if has_network {
        decls.push(screens::network());
    }
    if has_bt {
        decls.push(screens::bluetooth());
    }
    decls.push(screens::display());
    decls.push(screens::sound());
    decls.push(screens::datetime());
    decls.push(screens::locale());
    if has_admin {
        decls.push(screens::credentials());
    }
    if has_power {
        decls.push(screens::power());
    }
    decls.push(screens::about());

    for decl in decls {
        let id = decl.id().to_owned();
        if config.includes(&id) {
            shell.add(decl);
        }
    }
}
