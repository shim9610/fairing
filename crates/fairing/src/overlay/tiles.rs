//! Quick-settings tiles. The built-in `tile.*` and the integrator's
//! `tile(id, kind)` declarations.
//!
//! Tiles are filtered by their gate (`tile.<name>`), and a `Locked` one gets a
//! padlock badge and an unlock prompt on tap. A tap goes through `Shell::launch` as a
//! [`crate::LaunchAction`] — a `Toggle(key)`'s gate is the key name, so it is checked twice
//! (the render filter is presentation; the shell enforces).
//!
//! **The state-reading rules** (applied by [`super::Overlay`] every frame, with no allocation):
//! for keys with a backend snapshot (`wifi.enabled`, `bluetooth.enabled`,
//! `display.brightness`) the backend is the truth; everything else is the `SettingsView` value
//! (`audio.volume` is an in-memory value in M2, defaulting to 0.5). `display.brightness` is
//! [`TileState::Unavailable`] when the backend returns `None`, and `display.rotation_lock` is
//! `Unavailable` when [`rotation_lock_available`] is false. `Action` draws as `Off`, and
//! `Status` draws dimmed as `Unavailable`.
//!
//! **A Slider tile** opens an expanded row (a track and a thumb) under the tile row on tap, and
//! the value follows the finger 1:1 — the drawing is in [`super::panel`] and the expanded state
//! is held by [`super::Overlay`].

use crate::access::Gate;
use crate::i18n::{tr_key, LabelKey};
use crate::icons::{builtin, IconRef};
use crate::screen::{Cx, LaunchAction};
use crate::services::Capabilities;
use crate::settings::{keys, SettingKey};
use crate::theme::ColorRole;
use std::cell::RefCell;

/// Whether to enable `tile.rotation_lock` ("where the backend supports it"). It
/// lives only when the display backend reports at least one capability (that is, real display
/// control is attached) — `NullDisplay` reports none, so it is dimmed. There is no rotation
/// capability of its own: a display backend that controls anything counts.
#[must_use]
pub fn rotation_lock_available(display: Capabilities) -> bool {
    display != Capabilities::NONE
}

/// The kind of a tile.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TileKind {
    /// A boolean setting toggle.
    Toggle(SettingKey),
    /// A 0..=100 slider (in an expanded row).
    Slider(SettingKey),
    /// An action (opening settings, locking, …).
    Action(LaunchAction),
    /// A read-only display.
    Status,
    /// **Several gauge rows open.** The declaration sets how many rows there are and each row's
    /// text, colour and widths, and **the crate does the drawing** — the finger height, the
    /// press feedback, the track thickness and the value readout follow the same contract as the
    /// built-in slider.
    ///
    /// Each row's value is read and written as a 0..=100 integer in the [`Gauge::key`] setting.
    ///
    /// ```no_run
    /// # use fairing::overlay::{tile, Gauge, TileKind};
    /// # use fairing::ColorRole;
    /// tile(
    ///     "tile.hopper",
    ///     TileKind::Gauges {
    ///         rows: vec![
    ///             Gauge::new("hopper.a", "Resin A").color(ColorRole::Primary),
    ///             Gauge::new("hopper.b", "Resin B").color(ColorRole::Warning),
    ///             Gauge::new("hopper.c", "Solvent").color(ColorRole::Success),
    ///         ],
    ///         label_width: 96.0,
    ///     },
    /// );
    /// ```
    Gauges {
        /// The rows — **exactly this many** appear. Empty means the row does not open.
        rows: Vec<Gauge>,
        /// The name column's width (du). `0` draws no names and widens the track by that much.
        label_width: f32,
    },
    /// **The integrator draws the expanded row.** This is where something the crate knows
    /// nothing about goes straight into the quick-settings panel — several gauges, a small
    /// chart, a machine-specific control, anything.
    ///
    /// The tile itself looks like any other (a puck and a label), and pressing it opens a row in
    /// the same place as [`TileKind::Slider`], with the tile dropping into the left of that row.
    /// The only difference is who draws inside it.
    ///
    /// The body is given with [`tile_panel`].
    Panel {
        /// The row's height (du). One widget row is `metrics.widget_height`.
        height: f32,
    },
}

/// One row of a [`TileKind::Gauges`].
///
/// The value lives in the [`Gauge::key`] setting as a `0..=100` integer — the same contract as
/// the built-in brightness and volume sliders, so the write gate is the key name in exactly the
/// same way.
#[derive(Debug, Clone, PartialEq)]
pub struct Gauge {
    /// The setting key the value lives in.
    pub key: SettingKey,
    /// The name written on the left. Not drawn when `label_width` is `0`.
    pub label: LabelKey,
    /// The track colour (a palette role). It can differ per row.
    pub color: ColorRole,
    /// The unit after the value (`"%"`, `"°C"`). Empty draws no value readout at all.
    pub unit: String,
    /// The value column's width (du). Unused when `unit` is empty.
    pub value_width: Option<f32>,
    /// Read-only — it cannot be touched and only shows its value.
    pub read_only: bool,
}

/// The default for [`Gauge::value_width`] as a multiple of the body type size — the width
/// `100 %` fits in at any type scale (it was a fixed 56 du, which a larger scale overran).
pub(crate) const GAUGE_VALUE_EM: f32 = 3.5;

impl Gauge {
    /// Draw the `key` setting under the name `label`. Defaults: the `Primary` colour, a `%` unit, and touchable.
    #[must_use]
    pub fn new(key: impl Into<SettingKey>, label: impl Into<LabelKey>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            color: ColorRole::Primary,
            unit: "%".to_owned(),
            value_width: None,
            read_only: false,
        }
    }

    /// The track colour.
    #[must_use]
    pub fn color(mut self, color: ColorRole) -> Self {
        self.color = color;
        self
    }

    /// The unit after the value. An empty string draws no value readout.
    #[must_use]
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// The value column's width (du).
    #[must_use]
    pub const fn value_width(mut self, width: f32) -> Self {
        self.value_width = Some(width);
        self
    }

    /// Read-only.
    #[must_use]
    pub const fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
}

/// A tile's value (a snapshot for rendering).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TileState {
    /// Off.
    Off,
    /// On.
    On,
    /// A slider value, 0..=1.
    Value(f32),
    /// The backend does not support it (dimmed).
    Unavailable,
}

/// A tile.
#[derive(Debug, Clone, PartialEq)]
pub struct QuickTile {
    /// id (`tile.wifi` …).
    pub id: String,
    /// The icon.
    pub icon: IconRef,
    /// The label.
    pub label: LabelKey,
    /// The kind.
    pub kind: TileKind,
    /// The gate. Fixed at declaration time (defaulting to `id`) — the render filter borrows it every frame, with no allocation.
    pub gate: Gate,
    /// On or off (from config or code).
    pub enabled: bool,
    /// **Where a long press goes** (without one, a long press goes nowhere).
    ///
    /// Holding the Wi-Fi tile to reach Wi-Fi settings, the way Android's quick settings do, is a
    /// common expectation. Which screen that is, **only the integrator knows** (the crate has no
    /// such screen id). Attached or not, the long press itself goes out as
    /// [`ShellEvent::TileLongPressed`](crate::ShellEvent::TileLongPressed), so take that to do
    /// something other than opening a screen (a menu, a dialog).
    pub long_press: Option<LaunchAction>,
    /// **Where the tile's lit state is read from**, for the kinds that have none of their own.
    /// Set with [`TileDecl::lit_by`].
    pub lit_by: Option<SettingKey>,
}

impl QuickTile {
    /// The gate (defaulting to the id). Borrowed — zero heap allocations per frame.
    #[must_use]
    pub fn gate_name(&self) -> &Gate {
        &self.gate
    }
}

/// A `tile(id, kind)` declaration ("the other declarations have the same shape").
pub struct TileDecl {
    tile: QuickTile,
    /// The body of a [`TileKind::Panel`]'s expanded row. It is a `RefCell` because the drawing
    /// side only ever **borrows** the declaration — passing it as `&mut`, the way status bar and
    /// nav items do, would turn the whole panel drawing path into mutable references
    /// (bans locks; `RefCell` is the UI-thread idiom).
    body: Option<RefCell<PanelBody>>,
}

/// A [`TileKind::Panel`] body — a closure shaped like a screen's body.
type PanelBody = Box<dyn FnMut(&mut egui::Ui, &mut Cx<'_>)>;

impl std::fmt::Debug for TileDecl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TileDecl")
            .field("tile", &self.tile)
            .field("body", &self.body.is_some())
            .finish()
    }
}

/// A tile declaration. `shell.add(tile("heater", TileKind::Toggle("app.heater".into())).icon(icon::FLAME))`.
#[must_use]
pub fn tile(id: impl Into<String>, kind: TileKind) -> TileDecl {
    let id = id.into();
    TileDecl {
        body: None,
        tile: QuickTile {
            label: id.clone(),
            gate: Gate::from(&id),
            id,
            icon: builtin::SETTINGS,
            kind,
            enabled: true,
            long_press: None,
            lit_by: None,
        },
    }
}

/// A tile declaration that **draws its own** expanded row.
///
/// This is how something the crate does not have goes into the quick-settings panel. The tile
/// looks like any other (a puck and a label), and pressing it opens a `height` du row in the
/// same place as [`TileKind::Slider`]. Inside that row everything is the integrator's — it
/// receives the same `(&mut Ui, &mut Cx)` a screen's body does.
///
/// The icon space on the left of the row is **filled by the tile dropping into it**, so the body
/// gets a rect with that much taken off.
///
/// ```no_run
/// # use fairing::overlay::tile_panel;
/// # use fairing::widgets::TouchSlider;
/// # fn add(shell: &mut fairing::Shell, level: &'static std::cell::Cell<f32>) {
/// shell.add(
///     tile_panel("hopper", 180.0, move |ui, cx| {
///         let mut v = level.get();
///         if TouchSlider::new(&mut v, 0.0..=100.0).show(ui, &mut cx.widgets()).changed() {
///             level.set(v);
///         }
///     })
///     .label("Hoppers"),
/// );
/// # }
/// ```
#[must_use]
pub fn tile_panel(
    id: impl Into<String>,
    height: f32,
    ui: impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static,
) -> TileDecl {
    let mut decl = tile(id, TileKind::Panel { height });
    decl.body = Some(RefCell::new(Box::new(ui)));
    decl
}

impl TileDecl {
    /// Draw the expanded row's body. Does nothing if there is no body (the declaration is not a
    /// [`TileKind::Panel`]) or it is already borrowed.
    pub(crate) fn draw_panel(&self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let Some(body) = self.body.as_ref() else {
            return;
        };
        if let Ok(mut body) = body.try_borrow_mut() {
            body(ui, cx);
        }
    }

    /// The icon.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.tile.icon = icon;
        self
    }

    /// The label.
    #[must_use]
    pub fn label(mut self, label: impl Into<LabelKey>) -> Self {
        self.tile.label = label.into();
        self
    }

    /// The gate.
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.tile.gate = gate.into();
        self
    }

    /// On or off.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.tile.enabled = enabled;
        self
    }

    /// Where a long press goes ([`QuickTile::long_press`]).
    ///
    /// ```no_run
    /// # use fairing::overlay::{tile, TileKind};
    /// # use fairing::{settings::SettingKey, LaunchAction};
    /// tile("wifi", TileKind::Toggle(SettingKey::from("net.wifi.enabled")))
    ///     .long_press(LaunchAction::open("settings.wifi"));
    /// ```
    #[must_use]
    pub fn long_press(mut self, action: LaunchAction) -> Self {
        self.tile.long_press = Some(action);
        self
    }

    /// **Light the tile from a setting**.
    ///
    /// [`TileKind::Action`], [`Status`](TileKind::Status), [`Panel`](TileKind::Panel) and
    /// [`Gauges`](TileKind::Gauges) have no on/off of their own — a tile that opens a screen is not
    /// a switch — so the crate drew them in the off colour and there was nowhere to say otherwise. A
    /// conveyor that is running, a heater at temperature, a door that is open: the device knows, the
    /// crate cannot, and this is where it says so.
    ///
    /// The tile is lit while the key holds `Bool(true)`, and unlit for anything else or nothing at
    /// all. Write it from wherever the truth is, on any thread:
    ///
    /// ```no_run
    /// # fn f(shell: &mut fairing::Shell) {
    /// use fairing::overlay::{tile, TileKind};
    /// use fairing::settings::SettingValue;
    /// use fairing::{icon, LaunchAction};
    ///
    /// shell.add(
    ///     tile("tile.conveyor", TileKind::Action(LaunchAction::open("app.conveyor")))
    ///         .icon(icon::GAUGE)
    ///         .label("Conveyor")
    ///         .lit_by("app.conveyor.running"),
    /// );
    /// // …from the machine's own poll loop, whichever thread it runs on:
    /// shell.handle().set_setting("app.conveyor.running", SettingValue::Bool(true));
    /// # }
    /// ```
    ///
    /// [`Toggle`](TileKind::Toggle) and [`Slider`](TileKind::Slider) already read their own key, so
    /// `lit_by` on one of those is ignored with a warning — two sources for one tile is a bug
    /// waiting to happen, not a feature.
    ///
    /// **The key is a gate name** like every other setting, so with a
    /// `[access] default_gate` in force the write is refused unless `[access.gates]` names the key
    /// at a level the writer holds. A machine-state key is not something the person at the panel
    /// sets, so give it the lowest level and leave it there.
    #[must_use]
    pub fn lit_by(mut self, key: impl Into<SettingKey>) -> Self {
        if matches!(self.tile.kind, TileKind::Toggle(_) | TileKind::Slider(_)) {
            log::warn!(
                "tile `{}`: lit_by is ignored on a Toggle or Slider - the tile's own key already says whether it is on",
                self.tile.id
            );
            return self;
        }
        self.tile.lit_by = Some(key.into());
        self
    }

    /// id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.tile.id
    }

    /// The gate name.
    #[must_use]
    pub fn gate_name(&self) -> &Gate {
        self.tile.gate_name()
    }

    /// The tile.
    #[must_use]
    pub fn tile(&self) -> &QuickTile {
        &self.tile
    }
}

/// The built-in tile ids. `split_screen` is the split control.
pub const BUILTIN_IDS: &[&str] = &[
    "tile.wifi",
    "tile.bluetooth",
    "tile.brightness",
    "tile.volume",
    "tile.lock",
    "tile.theme",
    "tile.airplane",
    "tile.rotation_lock",
    "tile.split_screen",
    "tile.settings",
];

/// The built-in settings screen a **long press** on a built-in tile goes to. The same pairing as
/// Android's quick settings — hold Wi-Fi and you get Wi-Fi settings. With no pairing, or with
/// that screen not registered, nothing happens.
///
/// A [`TileDecl::long_press`] on the declaration wins over this (the ladder).
#[must_use]
pub(crate) fn builtin_long_press(id: &str) -> Option<&'static str> {
    Some(match id {
        "tile.wifi" => "settings.wifi",
        "tile.bluetooth" => "settings.bluetooth",
        // The brightness and the theme are both adjusted from the display settings.
        "tile.brightness" | "tile.theme" => "settings.display",
        "tile.volume" => "settings.sound",
        // `tile.settings` already has the settings home as its tap — pressing and holding to go to the same place means nothing.
        _ => return None,
    })
}

/// A built-in tile's definition. `None` for an unknown id.
#[must_use]
pub fn builtin(id: &str) -> Option<QuickTile> {
    let (icon, label, kind) = match id {
        "tile.wifi" => (
            builtin::WIFI,
            tr_key!("Wi-Fi"),
            TileKind::Toggle(SettingKey::from(keys::WIFI_ENABLED)),
        ),
        "tile.bluetooth" => (
            builtin::BLUETOOTH,
            tr_key!("Bluetooth"),
            TileKind::Toggle(SettingKey::from(keys::BLUETOOTH_ENABLED)),
        ),
        "tile.brightness" => (
            builtin::BRIGHTNESS,
            tr_key!("Brightness"),
            TileKind::Slider(SettingKey::from(keys::DISPLAY_BRIGHTNESS)),
        ),
        "tile.volume" => (
            builtin::VOLUME,
            tr_key!("Volume"),
            TileKind::Slider(SettingKey::from(keys::AUDIO_VOLUME)),
        ),
        "tile.lock" => (
            builtin::LOCK,
            tr_key!("Lock"),
            TileKind::Action(LaunchAction::Lock),
        ),
        "tile.theme" => (
            builtin::MOON,
            tr_key!("Dark"),
            TileKind::Toggle(SettingKey::from(keys::THEME_DARK)),
        ),
        "tile.airplane" => (
            builtin::AIRPLANE,
            tr_key!("Airplane"),
            TileKind::Toggle(SettingKey::from(keys::RADIO_AIRPLANE)),
        ),
        "tile.rotation_lock" => (
            builtin::RESTART,
            tr_key!("Rotation"),
            TileKind::Toggle(SettingKey::from(keys::DISPLAY_ROTATION_LOCK)),
        ),
        "tile.split_screen" => (
            builtin::SPLIT,
            tr_key!("Split"),
            TileKind::Action(LaunchAction::ToggleSplit),
        ),
        "tile.settings" => (
            builtin::SETTINGS,
            tr_key!("Settings"),
            TileKind::Action(LaunchAction::open("settings.home")),
        ),
        _ => return None,
    };
    Some(QuickTile {
        id: id.to_owned(),
        icon,
        label: label.to_owned(),
        kind,
        gate: Gate::from(id),
        enabled: true,
        // A built-in tile leaves its destination empty too — a settings screen's id is the integrator's,
        // not ours (the ladder: a crate that invents screen names leaves the integrator unable to change them).
        long_press: None,
        lit_by: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{builtin, tile, TileKind, BUILTIN_IDS};

    #[test]
    fn builtin_ids_all_resolve() {
        for id in BUILTIN_IDS {
            assert!(builtin(id).is_some(), "{id}");
        }
        assert!(builtin("tile.nope").is_none());
    }

    #[test]
    fn decl_defaults_gate_to_id() {
        let d = tile("heater", TileKind::Status).label("Heater");
        assert_eq!(d.gate_name().as_str(), "heater");
        assert_eq!(d.tile().label, "Heater");
    }

    #[test]
    fn rotation_lock_needs_a_display_backend() {
        use crate::services::Capabilities;
        assert!(!super::rotation_lock_available(Capabilities::NONE));
        assert!(super::rotation_lock_available(Capabilities::BRIGHTNESS));
    }
}
