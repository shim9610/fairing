//! The built-in settings screens. Feature `settings`.
//!
//! # The three depths a manufacturer can adjust at
//!
//! | Depth | How | What changes |
//! |---|---|---|
//! | Shallow | `[theme.palette]` · [`ShellBuilder::palettes`](crate::shell::ShellBuilder::palettes) | **The colours of all of them.** There is no colour literal in this file |
//! | Middle | Individual exclusions via [`SettingsConfig`](super::SettingsConfig) | Which screens exist |
//! | Deep | `shell.add(screen("settings.wifi", ..))` under the same id | That screen **whole** |
//!
//! The deep row holds **everywhere**, the two-pane split included: on a wide screen the right
//! column draws whatever the registry has under the selected id, so a replaced built-in
//! and a screen of the integrator's own reach it by the same road.
//!
//! The dimensions are all `theme.metrics` tokens too, so they grow with a change of glove policy.
//!
//! # Without the backend, it is not registered
//!
//! They are filtered on [`Capabilities`] — the worst outcome is a Wi-Fi entry showing
//! on a device with no Wi-Fi and saying "not supported" once it is pressed.
//!
//! The list follows: a row is drawn only where the screen behind it **is really registered**
//! ([`Cx::has_screen`]). `settings.network` is registered where the network backend reports
//! `ETHERNET`, and `settings.credentials` where the authenticator manages its own entries.

use super::{keys, SettingValue};
use crate::access::{AdminNote, CredentialEntry, CredentialOp, Level, Secret, SecretKind};
use crate::i18n::{tr, tr_key, Strings};
use crate::icons::IconRef;
use crate::layout as ui;
use crate::screen::{screen, Cx, ScreenDecl};
use crate::services::{
    Capabilities, ErrorKind, IfaceKind, IfaceSnapshot, IpConfig, Ipv4Net, PowerRequest,
    ServiceError,
};
use crate::theme::ColorRole;
use egui::Ui;
use std::net::{IpAddr, Ipv4Addr};

/// A real already clamped to 0..=100, as an integer percentage. No truncation, no sign loss.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "it is after clamp(0,100), so it is inside u8's range"
)]
fn percent(value: f32) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

/// A stored percentage as the shell applies it to a backend: a number clamped into 0..=100.
/// `None` for a value that is not a number.
fn stored_percent(value: &SettingValue) -> Option<u8> {
    match value {
        SettingValue::Int(v) => Some(u8::try_from((*v).clamp(0, 100)).unwrap_or(100)),
        #[expect(
            clippy::cast_possible_truncation,
            reason = "it is after clamp(0,100), so it fits an f32"
        )]
        SettingValue::Float(v) => Some(percent(v.clamp(0.0, 100.0) as f32)),
        SettingValue::Bool(_) | SettingValue::Text(_) => None,
    }
}

/// Every screen id this module produces. [`SettingsConfig::only`](super::SettingsConfig::only)
/// uses it to work out the complement.
pub const ALL: &[&str] = &[
    "settings.home",
    "settings.wifi",
    "settings.network",
    "settings.bluetooth",
    "settings.display",
    "settings.sound",
    "settings.datetime",
    "settings.locale",
    "settings.credentials",
    "settings.power",
    "settings.about",
];

/// **One row in the settings list**, leading into a screen.
///
/// The list is a `Vec` of these and the integrator owns it — [`entries`] gives the built-in table,
/// and anything can be taken out of it, put into it or moved within it before it is handed to
/// [`home_with`] or [`add_all_with`](super::add_all_with):
///
/// ```no_run
/// # fn main() {
/// # let shell: &mut fairing::Shell = todo!();
/// use fairing::icon;
/// use fairing::settings::screens::{entries, SettingsEntry};
/// use fairing::settings::{add_all_with, SettingsConfig};
///
/// let mut list = entries();
/// list.retain(|entry| entry.id != "settings.locale");            // one taken out
/// list.push(SettingsEntry::new("app.heater", icon::GAUGE, "Heater")); // one of ours put in
/// add_all_with(shell, &SettingsConfig::default(), list);
/// # }
/// ```
///
/// A row is drawn only where a screen really is registered under its `id`
/// ([`Cx::has_screen`]), so adding one means registering that screen too — `shell.add(screen(
/// "app.heater", ..))`. Registered with `screen`, it also goes up in the right column on a wide
/// screen; registered with `screen_with`, it is pushed instead.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsEntry {
    /// The screen id the row opens.
    pub id: String,
    /// The row's icon.
    pub icon: IconRef,
    /// The row's label.
    pub title: String,
}

impl SettingsEntry {
    /// One row.
    #[must_use]
    pub fn new(id: impl Into<String>, icon: IconRef, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            icon,
            title: title.into(),
        }
    }
}

/// **The built-in settings list**, in its default order. Edit it and hand it to [`home_with`] or
/// [`add_all_with`](super::add_all_with) — see [`SettingsEntry`].
///
/// It names every id in [`ALL`], `settings.home` aside. Ids with no screen behind them are simply
/// not drawn — `settings.credentials` on a device whose authenticator keeps its entries
/// elsewhere costs nothing by being here.
#[must_use]
pub fn entries() -> Vec<SettingsEntry> {
    [
        ("settings.wifi", "wifi", tr_key!("Wi-Fi")),
        ("settings.network", "ethernet", tr_key!("Network")),
        ("settings.bluetooth", "bluetooth", tr_key!("Bluetooth")),
        ("settings.display", "display", tr_key!("Display")),
        ("settings.sound", "volume", tr_key!("Sound")),
        ("settings.datetime", "clock", tr_key!("Date & time")),
        ("settings.locale", "language", tr_key!("Language")),
        ("settings.credentials", "key", tr_key!("Users & access")),
        ("settings.power", "power", tr_key!("Power")),
        ("settings.about", "info", tr_key!("About")),
    ]
    .into_iter()
    .map(|(id, icon, title)| SettingsEntry::new(id, IconRef::Builtin(icon), title))
    .collect()
}

/// The list screen over the built-in list. It shows **only the sub-screens that are registered and
/// pass their gate** (§`shows`).
///
/// Whether to leave a locked entry dimmed or hide it altogether is the same question as
/// [`Visibility`](crate::access::Visibility) on the icon side, and here it **hides** — unlike the
/// icon grid, a list does not show a gap where something is missing, and there is no reason to
/// make anyone press a menu that is not there.
#[must_use]
pub fn home() -> ScreenDecl {
    home_with(entries())
}

/// The list screen over **a list of your own** — [`entries`] edited, or one built from nothing.
///
/// Same id, same title, same icon as [`home`], so it replaces it wherever it is registered. It does
/// **not** put itself on the desktop; add `.desktop()` yourself, or let
/// [`add_all_with`](super::add_all_with) follow `SettingsConfig::home_icon` as [`add_all`](super::add_all)
/// does.
#[must_use]
pub fn home_with(entries: Vec<SettingsEntry>) -> ScreenDecl {
    let entries = dedup_ids(entries);
    screen("settings.home", move |ui: &mut Ui, cx: &mut Cx<'_>| {
        if let Some(width) = ui::split_width(cx) {
            two_pane(ui, cx, width, &entries);
        } else {
            // A narrow screen: only the list is drawn and a tap pushes the screen. The selection is
            // dropped — folding the fold must not leave half of it behind.
            ui.data_mut(|d| d.remove::<String>(selected_id()));
            ui::page(ui, cx, "settings.home", |ui, cx| {
                home_body(ui, cx, &entries);
            });
        }
    })
    .title(tr_key!("Settings"))
    .icon(IconRef::Builtin("settings"))
}

/// Screen brightness · the idle blank · the theme.
#[must_use]
pub fn display() -> ScreenDecl {
    ui::page_screen("settings.display", display_body)
        .title(tr_key!("Display"))
        .icon(IconRef::Builtin("display"))
}

/// Volume · mute · touch sounds. An audio backend that reports `VOLUME` is driven; with the `Null`
/// default the screen holds the settings values for the integrator to apply.
#[must_use]
pub fn sound() -> ScreenDecl {
    ui::page_screen("settings.sound", sound_body)
        .title(tr_key!("Sound"))
        .icon(IconRef::Builtin("volume"))
}

/// Language — the built-in tables and the integrator's. The choice takes effect on the next frame.
#[must_use]
pub fn locale() -> ScreenDecl {
    ui::page_screen("settings.locale", locale_body)
        .title(tr_key!("Language"))
        .icon(IconRef::Builtin("language"))
}

/// The current time · the 24-hour display · the UTC offset. Setting it by hand only when the backend offers `CLOCK_SET`.
#[must_use]
pub fn datetime() -> ScreenDecl {
    ui::page_screen("settings.datetime", datetime_body)
        .title(tr_key!("Date & time"))
        .icon(IconRef::Builtin("clock"))
}

/// Wi-Fi — the enable toggle · the connection card · the scan list.
///
/// A tap on a network is reported as
/// [`ShellEvent::WifiNetworkTapped`](crate::ShellEvent::WifiNetworkTapped) and connecting is the
/// integrator's — asking for a password included, so the crate never holds one.
#[must_use]
pub fn wifi() -> ScreenDecl {
    ui::page_screen("settings.wifi", wifi_body)
        .title(tr_key!("Wi-Fi"))
        .icon(IconRef::Builtin("wifi"))
}

/// Network — each interface's link, addresses and hardware address, how its IPv4 address is
/// set (DHCP or by hand), and the hostname. Everything goes through the
/// [`NetworkBackend`](crate::services::NetworkBackend); editing sits behind the
/// `settings.network.edit` gate.
#[must_use]
pub fn network() -> ScreenDecl {
    ui::page_screen("settings.network", network_body)
        .title(tr_key!("Network"))
        .icon(IconRef::Builtin("ethernet"))
}

/// Bluetooth — the enable and discovery toggles, and the paired / discovered lists.
#[must_use]
pub fn bluetooth() -> ScreenDecl {
    ui::page_screen("settings.bluetooth", bluetooth_body)
        .title(tr_key!("Bluetooth"))
        .icon(IconRef::Builtin("bluetooth"))
}

/// Restart · shut down. **It asks once** — on a device, a shutdown by mistake cannot be taken back.
#[must_use]
pub fn power() -> ScreenDecl {
    ui::page_screen("settings.power", power_body)
        .title(tr_key!("Power"))
        .icon(IconRef::Builtin("power"))
}

/// Device information · the crate version · **the open-source notices**.
///
/// The notices are `include_str!`d straight from `THIRD_PARTY.md` — this is where the MIT /
/// Apache / OFL notice obligations are met inside the device's own UI. Audit stage 10
/// catches the file going stale.
#[must_use]
pub fn about() -> ScreenDecl {
    ui::page_screen("settings.about", about_body)
        .title(tr_key!("About"))
        .icon(IconRef::Builtin("info"))
}

/// `PowerRequest` → the keys of (the card title, the confirm button's label).
const fn describe(request: PowerRequest) -> (&'static str, &'static str) {
    match request {
        PowerRequest::Reboot => (tr_key!("Restart"), tr_key!("Restart now")),
        PowerRequest::Shutdown => (tr_key!("Shut down"), tr_key!("Shut down now")),
        PowerRequest::Suspend => (tr_key!("Suspend"), tr_key!("Suspend now")),
    }
}

/// The card's clock, on **the hour convention the device is set to** ([`keys::UI_CLOCK_12H`]).
/// The screen that writes the setting has to obey it too, or the switch appears to do nothing.
fn format_clock(now: crate::time::WallTime, hour12: bool, strings: &Strings) -> String {
    now.format_with(
        crate::time::ClockFormat::Hms.with_hour12(hour12),
        crate::time::Meridiem::of(strings),
    )
}

/// Offset minutes → `+09:00`.
/// `3 d 4 h`, `4 h 12 min`, `12 min` — the two largest units, which is all an uptime needs.
fn format_uptime(secs: u64, strings: &Strings) -> String {
    let (days, hours, minutes) = (secs / 86_400, secs / 3_600 % 24, secs / 60 % 60);
    let fill = |template: &str| {
        template
            .replace("{d}", &days.to_string())
            .replace("{h}", &hours.to_string())
            .replace("{m}", &minutes.to_string())
    };
    if days > 0 {
        fill(tr!(strings, "{d} d {h} h"))
    } else if hours > 0 {
        fill(tr!(strings, "{h} h {m} min"))
    } else {
        fill(tr!(strings, "{m} min"))
    }
}

fn format_offset(offset_min: i32) -> String {
    let sign = if offset_min < 0 { '-' } else { '+' };
    let abs = offset_min.unsigned_abs();
    format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
}

// ── The screen bodies ─────────────────────────────────────────────────────────
//
// The bodies are `fn`s so that `page_screen` can wrap each in a scroll and make a screen of it. The
// left/right split does **not** reach for them: it asks the registry for the declaration under the
// selected id, which is what lets an integrator's screen appear there at all.

/// Drop a repeated id, keeping the first, and say so.
///
/// The selection is an **id**, so two rows pointing at one screen both light up when either is
/// pressed and the right column takes its heading from whichever comes first — the second row's
/// label never appears. It is an easy list to build by mistake, since [`entries`] already carries
/// every built-in id and pushing "our Wi-Fi screen" onto it is the obvious move. This is the same
/// policy as [`Registry::add`](crate::screen::Registry::add): one id, one entry, and a warning
/// rather than a silent oddity.
fn dedup_ids(entries: Vec<SettingsEntry>) -> Vec<SettingsEntry> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        if seen.insert(entry.id.clone()) {
            out.push(entry);
        } else {
            log::warn!(
                "settings list: `{}` is in it twice - keeping the first row and dropping `{}`",
                entry.id,
                entry.title
            );
        }
    }
    out
}

/// The id of the entry shown on the right on a wide screen. Kept in egui's memory because the
/// selection has to survive between frames and `settings.home`'s body is a plain `fn`.
///
/// **Not `Id::NULL`**, which egui documents as the slot for an application-wide singleton: keyed on
/// `(Id::NULL, TypeId::of::<String>())`, this shared it with any integrator following that advice,
/// and the narrow branch below clears it on every frame it draws.
fn selected_id() -> egui::Id {
    egui::Id::new("fairing.settings.home.selected")
}

/// **The two-pane split**. The list of entries on the left, that entry's screen on
/// the right.
///
/// Unfolding a foldable, or sitting on a wide panel, makes one entry per list row a waste — when
/// there is width to spare, the content goes up on the right alongside. **No screen is pushed**:
/// [`Cx::draw_screen`] draws the registered declaration within this same screen, so the workspace
/// stack is untouched (the workspace's own two panes are another thing).
///
/// Narrowing again drops the selection and returns to the list — folding the fold must not leave
/// half of it behind.
fn two_pane(ui: &mut Ui, cx: &mut Cx<'_>, list_width: f32, entries: &[SettingsEntry]) {
    let s = cx.strings;
    let first = entries
        .iter()
        .find(|e| shows(cx, &e.id) && cx.can_draw_screen(&e.id))
        .map(|e| e.id.clone());
    let selected = ui
        .data(|d| d.get_temp::<String>(selected_id()))
        .filter(|id| shows(cx, id) && cx.can_draw_screen(id))
        .or(first);

    let full = ui.available_rect_before_wrap();
    let split = full.left() + list_width;
    let mut left = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
        full.min,
        egui::pos2(split, full.bottom()),
    )));
    let picked = selected.clone();
    ui::page(&mut left, cx, "settings.home.list", |ui, cx| {
        ui::title(ui, cx, tr!(s, "Settings"));
        for entry in entries {
            if !shows(cx, &entry.id) {
                continue;
            }
            let is_open = picked.as_deref() == Some(entry.id.as_str());
            if ui::list_item(ui, cx, entry.icon.clone(), s.get(&entry.title), is_open).clicked() {
                if cx.can_draw_screen(&entry.id) {
                    // Only the right side changes, within the same screen.
                    ui.data_mut(|d| d.insert_temp(selected_id(), entry.id.clone()));
                } else {
                    // A factory declaration owns no screen to lend, so it is pushed as before.
                    cx.open(&entry.id);
                }
            }
        }
    });

    ui.painter().vline(
        split,
        full.top()..=full.bottom(),
        egui::Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
    );

    let mut right = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
        egui::pos2(split + 1.0, full.top()),
        full.max,
    )));
    if let Some(id) = selected.as_deref() {
        let heading = entries
            .iter()
            .find(|e| e.id == id)
            .map_or(tr!(s, "Settings"), |e| s.get(&e.title));
        ui::title(&mut right, cx, heading);
        // **Whatever is registered under that id** — a built-in, a built-in the integrator replaced,
        // or one of their own. The screen brings its own scroll, so only the heading sits
        // outside it. Before this the right column called a private `fn` from a table of the eight
        // built-in bodies, so nothing an integrator registered could ever appear there.
        //
        // `selected` was filtered on `can_draw_screen`, so this normally draws. It can still come
        // back `false` — the declaration was removed by a screen drawn earlier in this same frame —
        // and an empty column with a heading over it says nothing to the person looking at it.
        if !cx.draw_screen(&mut right, id) {
            ui::note(&mut right, cx, tr!(s, "This screen is not available."));
        }
    } else {
        // Nothing can go on the right: an empty list, everything gated away, or every row pointing
        // at a factory declaration (which is opened, never embedded). Drawing half a screen and
        // leaving the other half blank reads as a bug.
        ui::title(&mut right, cx, tr!(s, "Settings"));
        ui::note(&mut right, cx, tr!(s, "Choose a setting from the list."));
    }
    ui.advance_cursor_after_rect(full);
}

/// Whether a list row is drawn at all: **the screen is registered and its own gate passes**
/// ([`Cx::screen_allowed`]).
///
/// Both halves were wrong before. The entry table is a fixed list of every built-in settings id, but
/// `add_all` registers fewer than that — `settings.credentials` has no screen yet, and Wi-Fi,
/// Network, Bluetooth and Power drop out on a device whose backend does not support them — so rows
/// were drawn that a press could not open. And the gate was checked as
/// `cx.allows(id)`, the **entry id** taken as a gate name; a declaration's gate defaults to its id
/// but `.gate("service")` names another, so a screen locked to a higher level was listed to
/// everyone. Combined with the right column, which draws the selected row with no further asking,
/// that listed screen's contents were painted for a session that could not open it.
fn shows(cx: &Cx<'_>, id: &str) -> bool {
    cx.screen_allowed(id)
}

/// The `settings.home` body (narrow screens).
fn home_body(ui: &mut Ui, cx: &mut Cx<'_>, entries: &[SettingsEntry]) {
    let s = cx.strings;
    ui::title(ui, cx, tr!(s, "Settings"));
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            for entry in entries {
                if !shows(cx, &entry.id) {
                    continue;
                }
                if ui::icon_row(ui, cx, entry.icon.clone(), s.get(&entry.title), None, None)
                    .clicked()
                {
                    cx.open(&entry.id);
                }
            }
        },
    );
}

/// The `settings.display` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
pub fn display_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let s = cx.strings;
    let info = cx.services.display.info();
    let has_brightness = cx
        .services
        .display
        .capabilities()
        .contains(Capabilities::BRIGHTNESS);

    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if has_brightness {
                let mut value = f32::from(cx.services.display.brightness().unwrap_or(50));
                if ui::slider_row(
                    ui,
                    cx,
                    tr!(s, "Brightness"),
                    &mut value,
                    0.0..=100.0,
                    "%",
                    true,
                ) {
                    let level = percent(value);
                    // The backend is the original and the settings value is the copy the UI remembers.
                    let _ = cx.services.display.set_brightness(level);
                    cx.set_setting(
                        keys::DISPLAY_BRIGHTNESS,
                        SettingValue::Int(i64::from(level)),
                    );
                }
            } else {
                ui::info_row(ui, cx, tr!(s, "Brightness"), tr!(s, "Not supported"));
            }
        },
    );

    ui::section_card_with(
        ui,
        cx,
        tr!(s, "Theme"),
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            let dark = matches!(
                cx.settings.get(&keys::THEME_DARK.into()),
                Some(SettingValue::Bool(true))
            );
            if let Some(index) = ui::choice_rows(
                ui,
                cx,
                &[(tr!(s, "Light"), None), (tr!(s, "Dark"), None)],
                usize::from(dark),
            ) {
                cx.set_setting(keys::THEME_DARK, SettingValue::Bool(index == 1));
            }
        },
    );
    ui::note(
        ui,
        cx,
        tr!(s, "Applies to the shell and every built-in screen."),
    );

    ui::section_card_with(
        ui,
        cx,
        tr!(s, "Panel"),
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            ui::info_row(
                ui,
                cx,
                tr!(s, "Resolution"),
                &format!("{} × {}", info.size_px.0, info.size_px.1),
            );
            if let Some(mm) = info.physical_mm {
                ui::info_row(
                    ui,
                    cx,
                    tr!(s, "Physical size"),
                    &format!("{:.0} × {:.0} mm", mm.0, mm.1),
                );
            }
            ui::info_row(ui, cx, tr!(s, "Rotation"), &format!("{}°", info.rotation));
        },
    );
}

/// The `settings.sound` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
pub fn sound_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let s = cx.strings;
    // The backend is the original where there is one. With the `Null` default the setting
    // is all there is — the copy an integrator applies from `SettingChanged` instead.
    let live = if cx
        .services
        .audio
        .capabilities()
        .contains(Capabilities::VOLUME)
    {
        cx.services.audio.volume()
    } else {
        None
    };
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            let mut volume = match live {
                Some(audio) => f32::from(audio.level),
                None => cx
                    .settings
                    .get(&keys::AUDIO_VOLUME.into())
                    .and_then(stored_percent)
                    .map_or(50.0, f32::from),
            };
            if ui::slider_row(
                ui,
                cx,
                tr!(s, "Volume"),
                &mut volume,
                0.0..=100.0,
                "%",
                true,
            ) {
                cx.set_setting(
                    keys::AUDIO_VOLUME,
                    SettingValue::Int(i64::from(percent(volume))),
                );
            }
            let mut muted = live.map_or_else(
                || {
                    matches!(
                        cx.settings.get(&keys::AUDIO_MUTED.into()),
                        Some(SettingValue::Bool(true))
                    )
                },
                |audio| audio.muted,
            );
            if ui::switch_row(ui, cx, tr!(s, "Mute"), None, &mut muted, true) {
                cx.set_setting(keys::AUDIO_MUTED, SettingValue::Bool(muted));
            }
        },
    );

    ui::section_card_with(
        ui,
        cx,
        tr!(s, "Alerts"),
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            let mut silent = matches!(
                cx.settings.get(&keys::UI_SILENT.into()),
                Some(SettingValue::Bool(true))
            );
            if ui::switch_row(ui, cx, tr!(s, "Silent"), None, &mut silent, true) {
                cx.set_setting(keys::UI_SILENT, SettingValue::Bool(silent));
            }
        },
    );
    ui::note(
        ui,
        cx,
        tr!(s, "Notifications still appear; only the sound is muted."),
    );
}

/// The `settings.locale` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
pub fn locale_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    // Every language there is a table for — the built-in ones and the integrator's — each named
    // in its own language, as a language list is. The choice is live: the next frame is in it.
    let strings = cx.strings;
    let locales: Vec<(&str, &str)> = strings.locales().collect();
    let current = locales
        .iter()
        .position(|(tag, _)| *tag == strings.locale())
        .unwrap_or(0);
    let options: Vec<(&str, Option<&str>)> = locales
        .iter()
        .map(|(tag, name)| (*name, Some(*tag)))
        .collect();
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if let Some(index) = ui::choice_rows(ui, cx, &options, current) {
                if let Some((tag, _)) = locales.get(index) {
                    cx.set_setting(keys::UI_LOCALE, SettingValue::Text((*tag).to_owned()));
                }
            }
        },
    );
}

/// The `settings.datetime` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
pub fn datetime_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let s = cx.strings;
    let now = cx.services.clock.now();
    let settable = cx
        .services
        .clock
        .capabilities()
        .contains(Capabilities::CLOCK_SET);
    let mut h24 = !matches!(
        cx.settings.get(&keys::UI_CLOCK_12H.into()),
        Some(SettingValue::Bool(true))
    );
    ui::status_card(
        ui,
        cx,
        &format_clock(now, !h24, s),
        Some(&format!("UTC{}", format_offset(now.offset_min))),
        None,
    );

    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if ui::switch_row(ui, cx, tr!(s, "24-hour time"), None, &mut h24, true) {
                cx.set_setting(keys::UI_CLOCK_12H, SettingValue::Bool(!h24));
            }
        },
    );

    ui::section_card_with(
        ui,
        cx,
        tr!(s, "Time source"),
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            ui::info_row(
                ui,
                cx,
                tr!(s, "Set manually"),
                if settable {
                    tr!(s, "Available")
                } else {
                    tr!(s, "Not supported")
                },
            );
        },
    );
    ui::note(
        ui,
        cx,
        tr!(
            s,
            "The clock comes from the integrator's backend; the shell never sets it on its own."
        ),
    );
}

/// The `settings.wifi` body — **it shows, it does not connect**.
///
/// The radio toggle and the scan button are the screen's, because neither carries a secret nor a
/// choice: they map straight onto [`WifiBackend::set_enabled`](crate::services::WifiBackend::set_enabled)
/// and [`scan`](crate::services::WifiBackend::scan). **Connecting is not.** A tap on a network row
/// leaves through [`ShellEvent::WifiNetworkTapped`](crate::ShellEvent::WifiNetworkTapped) carrying
/// the SSID, whether it is secured and whether a profile is already saved, and the integrator
/// decides what happens — the password, what to ask with, where it is stored, whether a saved
/// network reconnects on a tap at all.
///
/// It reads that way round for one reason: **the shell is a drawing layer and must not own the
/// secret.** [`WifiBackend::connect`](crate::services::WifiBackend::connect) takes the PSK by
/// `&str`, so an integrator can hold the buffer in whatever type they trust — a zeroizing wrapper,
/// say — and hand it straight to the backend without it ever passing through the crate.
/// [`TextField::password`](crate::widgets::TextField::password) is there to type it into and keeps
/// no undo history, but the buffer behind it stays the caller's.
///
/// Earlier this screen connected on a tap for open and saved networks and refused a secured one it
/// had no profile for, leaving a line in the log. On a freshly installed device nothing is saved,
/// so every secured network hit that branch and the screen was unusable — and adding a password
/// prompt meant reimplementing the whole screen, since the tap was already spent.
pub fn wifi_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    use crate::services::WifiState;

    let s = cx.strings;
    let snapshot = cx.services.wifi.snapshot();
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            let mut enabled = snapshot.enabled;
            if ui::switch_row(ui, cx, tr!(s, "Wi-Fi"), None, &mut enabled, true) {
                let _ = cx.services.wifi.set_enabled(enabled);
                cx.set_setting(keys::WIFI_ENABLED, SettingValue::Bool(enabled));
            }
        },
    );
    if !snapshot.enabled {
        ui::note(ui, cx, tr!(s, "Turn Wi-Fi on to see nearby networks."));
        return;
    }

    match &snapshot.state {
        WifiState::Connected { ssid, strength } => ui::status_card(
            ui,
            cx,
            ssid,
            Some(&tr!(s, "Connected · signal {n}/4").replace("{n}", &strength.to_string())),
            Some(ColorRole::Primary),
        ),
        WifiState::Connecting => ui::status_card(ui, cx, tr!(s, "Connecting…"), None, None),
        WifiState::Scanning => ui::status_card(ui, cx, tr!(s, "Scanning…"), None, None),
        WifiState::Failed { reason } => {
            ui::status_card(
                ui,
                cx,
                tr!(s, "Failed"),
                Some(reason),
                Some(ColorRole::Danger),
            );
        }
        WifiState::Off | WifiState::Idle => {}
    }

    ui::section_card_with(
        ui,
        cx,
        tr!(s, "Networks"),
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("refresh"),
                tr!(s, "Scan for networks"),
                None,
                None,
            )
            .clicked()
            {
                let _ = cx.services.wifi.scan();
            }
            for network in &snapshot.networks {
                let known = snapshot.known.iter().any(|saved| saved == &network.ssid);
                let subtitle = match (known, network.secured) {
                    (true, _) => tr!(s, "Saved"),
                    (false, true) => tr!(s, "Secured"),
                    (false, false) => tr!(s, "Open"),
                };
                let trailing = format!("{}/4", network.strength);
                let press = ui::icon_row_with_long_press(
                    ui,
                    cx,
                    IconRef::Builtin("wifi"),
                    &network.ssid,
                    Some(subtitle),
                    Some(&trailing),
                );
                // **Both are reported, neither is acted on.** See this function's docs.
                if press.tapped {
                    cx.wifi_network_tapped(&network.ssid, network.secured, known);
                }
                if press.long_pressed {
                    cx.wifi_network_long_pressed(&network.ssid, known);
                }
            }
        },
    );
    if snapshot.networks.is_empty() {
        ui::note(
            ui,
            cx,
            tr!(s, "No networks found yet. Tap scan to look again."),
        );
    }
}

/// The `settings.network` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
///
/// The interfaces are read from the backend every frame the screen is drawn. An edit is a form
/// held in egui's memory (the body is a plain `fn`, as `settings.power`'s is) and checked with
/// `std::net` before anything reaches the backend; what the backend then says — `Unsupported`
/// included — is shown on the form rather than dropped.
///
/// The form is the drawing screen's alone and lives only while it is drawn for the session that
/// opened it and that session still passes `settings.network.edit`: closed, covered, drawn for
/// another subject or below the gate, the form is gone, and the next person starts from the list.
pub fn network_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    held(ui, cx, "fairing.settings.network.form", |ui, cx, form| {
        if !cx.allows(NETWORK_EDIT) {
            *form = None;
        }
        network_rows(ui, cx, form);
    });
}

/// The gate an edit on `settings.network` sits behind.
const NETWORK_EDIT: &str = "settings.network.edit";

/// A body's state between frames: what it holds, the frame it was last drawn on and whom for.
#[derive(Clone)]
struct Held<T> {
    value: Option<T>,
    drawn: u64,
    subject: crate::access::Subject,
}

/// Run `body` with a value kept in egui's memory for the drawing screen instance, and dropped the
/// first frame the body is not drawn (closed, covered, another entry on the settings home's
/// right) or is drawn for another subject. A plain-`fn` body uses this for what a screen struct
/// would keep in a field, so nothing carries over to the next person or the next visit.
fn held<T: Clone + Send + Sync + 'static>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    salt: &'static str,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>, &mut Option<T>),
) {
    let key = egui::Id::new((salt, cx.pane.instance.0));
    let frame = cx.frame();
    let kept: Option<Held<T>> = ui.data(|d| d.get_temp(key));
    let mut value = kept
        .filter(|kept| frame.saturating_sub(kept.drawn) <= 1 && kept.subject == cx.session.subject)
        .and_then(|kept| kept.value);
    body(ui, cx, &mut value);
    let state = Held {
        value,
        drawn: frame,
        subject: cx.session.subject.clone(),
    };
    ui.data_mut(|d| d.insert_temp(key, state));
}

/// An edit in progress on `settings.network`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct NetworkForm {
    /// What is being edited.
    target: FormTarget,
    /// For an interface: DHCP rather than a hand-set address.
    dhcp: bool,
    /// The address, the prefix length, the gateway and the DNS servers, as typed.
    addr: String,
    prefix: String,
    gateway: String,
    dns: String,
    /// The hostname, as typed.
    hostname: String,
    /// Why the last apply did not go through.
    error: Option<String>,
}

/// What a [`NetworkForm`] edits.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FormTarget {
    /// The IPv4 configuration of the interface with this name.
    Iface(String),
    /// The hostname.
    Hostname,
}

impl NetworkForm {
    /// Start an interface's form from what it has now.
    fn iface(iface: &IfaceSnapshot) -> Self {
        Self {
            target: FormTarget::Iface(iface.name.clone()),
            dhcp: iface.dhcp,
            addr: iface
                .ipv4
                .map(|net| net.addr.to_string())
                .unwrap_or_default(),
            prefix: iface
                .ipv4
                .map_or_else(|| "24".to_owned(), |net| net.prefix.to_string()),
            gateway: iface.gateway.map(|g| g.to_string()).unwrap_or_default(),
            dns: join_addrs(&iface.dns),
            hostname: String::new(),
            error: None,
        }
    }

    /// Start the hostname form.
    fn hostname(current: &str) -> Self {
        Self {
            target: FormTarget::Hostname,
            dhcp: false,
            addr: String::new(),
            prefix: String::new(),
            gateway: String::new(),
            dns: String::new(),
            hostname: current.to_owned(),
            error: None,
        }
    }
}

fn network_rows(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut Option<NetworkForm>) {
    let s = cx.strings;
    if let Some(open) = form.as_mut() {
        let done = match open.target.clone() {
            FormTarget::Iface(name) => ip_form(ui, cx, open, &name),
            FormTarget::Hostname => hostname_form(ui, cx, open),
        };
        if done {
            *form = None;
        }
        return;
    }

    let editable = cx.allows(NETWORK_EDIT);
    let ifaces = cx.services.network.interfaces();
    if ifaces.is_empty() {
        ui::status_card(
            ui,
            cx,
            tr!(s, "No interfaces"),
            Some(tr!(s, "The device reports no network interfaces.")),
            None,
        );
    }
    for iface in &ifaces {
        ui::section(
            ui,
            cx,
            &format!("{} · {}", iface.name, s.get(kind_label(iface.kind))),
        );
        ui::group_with(
            ui,
            cx,
            ui::Deco::new().container(ui::Container::Divided),
            |ui, cx| {
                ui::info_row(
                    ui,
                    cx,
                    tr!(s, "Link"),
                    if iface.up {
                        tr!(s, "Connected")
                    } else {
                        tr!(s, "No link")
                    },
                );
                let ipv4 = iface.ipv4.map_or_else(
                    || tr!(s, "None").to_owned(),
                    |net| {
                        let how = if iface.dhcp { "DHCP" } else { tr!(s, "manual") };
                        format!("{}/{} · {how}", net.addr, net.prefix)
                    },
                );
                ui::info_row(ui, cx, "IPv4", &ipv4);
                if let Some(gateway) = iface.gateway {
                    ui::info_row(ui, cx, tr!(s, "Gateway"), &gateway.to_string());
                }
                if !iface.dns.is_empty() {
                    ui::info_row(ui, cx, "DNS", &join_addrs(&iface.dns));
                }
                for net in &iface.ipv6 {
                    ui::info_row(ui, cx, "IPv6", &format!("{}/{}", net.addr, net.prefix));
                }
                if let Some(mac) = &iface.mac {
                    ui::info_row(ui, cx, tr!(s, "MAC"), mac);
                }
                if editable && ui::nav_row(ui, cx, tr!(s, "Set the IPv4 address"), None).clicked() {
                    *form = Some(NetworkForm::iface(iface));
                }
            },
        );
    }

    if let Some(hostname) = cx.services.network.hostname() {
        ui::section(ui, cx, tr!(s, "This device"));
        ui::group_with(
            ui,
            cx,
            ui::Deco::new().container(ui::Container::Divided),
            |ui, cx| {
                if editable {
                    if ui::nav_row(ui, cx, tr!(s, "Hostname"), Some(&hostname)).clicked() {
                        *form = Some(NetworkForm::hostname(&hostname));
                    }
                } else {
                    ui::info_row(ui, cx, tr!(s, "Hostname"), &hostname);
                }
            },
        );
    }
}

/// One labelled field of a form.
fn form_field(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    text: &mut String,
    hint: &str,
    salt: &'static str,
) {
    ui::section(ui, cx, label);
    crate::widgets::TextField::new(text)
        .hint(hint)
        .id_salt(salt)
        .show(ui, &mut cx.widgets());
}

/// The interface form. `true` when it is finished — applied, or cancelled.
fn ip_form(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut NetworkForm, name: &str) -> bool {
    let s = cx.strings;
    ui::title(ui, cx, &format!("{name} · IPv4"));
    let pick =
        crate::widgets::SegmentedControl::new(&["DHCP", tr!(s, "Manual")], usize::from(!form.dhcp))
            .show(ui, &mut cx.widgets());
    if let Some(picked) = pick.picked {
        form.dhcp = picked == 0;
    }
    if !form.dhcp {
        form_field(
            ui,
            cx,
            tr!(s, "Address"),
            &mut form.addr,
            "192.168.0.10",
            "net.addr",
        );
        form_field(
            ui,
            cx,
            tr!(s, "Prefix length"),
            &mut form.prefix,
            "24",
            "net.prefix",
        );
        form_field(
            ui,
            cx,
            tr!(s, "Gateway"),
            &mut form.gateway,
            tr!(s, "Optional"),
            "net.gateway",
        );
        form_field(
            ui,
            cx,
            tr!(s, "DNS servers"),
            &mut form.dns,
            tr!(s, "Optional, separated by commas"),
            "net.dns",
        );
    }
    finish_form(ui, cx, form, |cx, form| {
        let config = if form.dhcp {
            IpConfig::Dhcp
        } else {
            parse_static(&form.addr, &form.prefix, &form.gateway, &form.dns, s)?
        };
        cx.services
            .network
            .configure(name, config)
            .map_err(|e| describe_service_error(&e, s))
    })
}

/// The hostname form. `true` when it is finished.
fn hostname_form(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut NetworkForm) -> bool {
    let s = cx.strings;
    ui::title(ui, cx, tr!(s, "Hostname"));
    form_field(
        ui,
        cx,
        tr!(s, "Hostname"),
        &mut form.hostname,
        "device-01",
        "net.hostname",
    );
    ui::note(
        ui,
        cx,
        tr!(
            s,
            "Letters, digits and hyphens, up to 63, not starting or ending with a hyphen."
        ),
    );
    finish_form(ui, cx, form, |cx, form| {
        let name = form.hostname.trim();
        if !valid_hostname(name) {
            return Err(tr!(s, "`{name}` is not a valid hostname.").replace("{name}", name));
        }
        cx.services
            .network
            .set_hostname(name)
            .map_err(|e| describe_service_error(&e, s))
    })
}

/// The error card and the Apply / Cancel rows every form ends with. `apply` runs on Apply; its
/// error stays on the form. `true` when the form is finished.
fn finish_form(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    form: &mut NetworkForm,
    apply: impl FnOnce(&mut Cx<'_>, &NetworkForm) -> Result<(), String>,
) -> bool {
    let s = cx.strings;
    if let Some(error) = &form.error {
        ui::status_card(
            ui,
            cx,
            tr!(s, "Not applied"),
            Some(error),
            Some(ColorRole::Danger),
        );
    }
    let (mut apply_pressed, mut cancel_pressed) = (false, false);
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            apply_pressed = ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("check"),
                tr!(s, "Apply"),
                None,
                None,
            )
            .clicked();
            cancel_pressed = ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("close"),
                tr!(s, "Cancel"),
                None,
                None,
            )
            .clicked();
        },
    );
    if cancel_pressed {
        return true;
    }
    if apply_pressed {
        match apply(cx, form) {
            Ok(()) => return true,
            Err(error) => form.error = Some(error),
        }
    }
    false
}

/// Read the manual form. The first field that does not parse is named in the error.
fn parse_static(
    addr: &str,
    prefix: &str,
    gateway: &str,
    dns: &str,
    strings: &Strings,
) -> Result<IpConfig, String> {
    let addr = addr.trim();
    let addr: Ipv4Addr = addr.parse().map_err(|_| {
        tr!(strings, "The address `{addr}` is not an IPv4 address.").replace("{addr}", addr)
    })?;
    let prefix = prefix
        .trim()
        .parse::<u8>()
        .ok()
        .filter(|p| *p <= 32)
        .ok_or_else(|| tr!(strings, "The prefix length is a number from 0 to 32.").to_owned())?;
    let gateway = match gateway.trim() {
        "" => None,
        text => Some(text.parse::<Ipv4Addr>().map_err(|_| {
            tr!(strings, "The gateway `{gateway}` is not an IPv4 address.")
                .replace("{gateway}", text)
        })?),
    };
    let dns = dns
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<IpAddr>().map_err(|_| {
                tr!(strings, "The DNS server `{server}` is not an IP address.")
                    .replace("{server}", part)
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(IpConfig::Static {
        addr: Ipv4Net { addr, prefix },
        gateway,
        dns,
    })
}

/// RFC 1123: one to 63 letters, digits and hyphens, not starting or ending with a hyphen.
fn valid_hostname(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && !name.starts_with('-')
        && !name.ends_with('-')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// What to tell the person at the panel when the backend says no.
fn describe_service_error(error: &ServiceError, strings: &Strings) -> String {
    if error.kind == ErrorKind::Unsupported {
        tr!(strings, "This device does not allow it to be changed here.").to_owned()
    } else {
        error.message.clone()
    }
}

fn kind_label(kind: IfaceKind) -> &'static str {
    match kind {
        IfaceKind::Ethernet => tr_key!("Wired"),
        IfaceKind::Wifi => tr_key!("Wi-Fi"),
        _ => tr_key!("Other"),
    }
}

fn join_addrs(addrs: &[IpAddr]) -> String {
    addrs
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `settings.bluetooth` body.
///
/// A pairing request is **received and drawn**. The passkey is made by the backend
/// — [`BtSnapshot::pending`](crate::services::BtSnapshot::pending) carries it, and the answer goes
/// back through [`respond_pairing`](crate::services::BluetoothBackend::respond_pairing). Showing a
/// number and taking "yes, that matches" collects no secret, so it is the screen's; **entering** a
/// PIN is the other thing, and that stays the integrator's the way a Wi-Fi PSK does.
///
/// Before this, `pair()` was called and the request that came back was never drawn and never
/// answered, so pairing stopped there with nothing on screen.
pub fn bluetooth_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let s = cx.strings;
    let snapshot = cx.services.bluetooth.snapshot();
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            let mut enabled = snapshot.enabled;
            if ui::switch_row(ui, cx, tr!(s, "Bluetooth"), None, &mut enabled, true) {
                let _ = cx.services.bluetooth.set_enabled(enabled);
                cx.set_setting(keys::BLUETOOTH_ENABLED, SettingValue::Bool(enabled));
            }
            if snapshot.enabled {
                let mut discovering = snapshot.discovering;
                if ui::switch_row(
                    ui,
                    cx,
                    tr!(s, "Discoverable"),
                    Some(tr!(s, "Other devices can find this one")),
                    &mut discovering,
                    true,
                ) {
                    let _ = cx.services.bluetooth.set_discovering(discovering);
                }
            }
        },
    );
    if !snapshot.enabled {
        ui::note(ui, cx, tr!(s, "Turn Bluetooth on to pair a device."));
        return;
    }

    // A request waiting takes the screen, the way `settings.power`'s confirmation does — there is
    // one thing to do and the lists below can wait.
    if pairing_card(ui, cx, &snapshot) {
        return;
    }

    let (paired, found): (Vec<_>, Vec<_>) = snapshot.devices.iter().partition(|d| d.paired);
    if !paired.is_empty() {
        ui::section_card_with(
            ui,
            cx,
            tr!(s, "Paired"),
            ui::Deco::new().container(ui::Container::Divided),
            |ui, cx| {
                for device in paired {
                    let subtitle = if device.connected {
                        tr!(s, "Connected")
                    } else {
                        tr!(s, "Not connected")
                    };
                    if ui::icon_row(
                        ui,
                        cx,
                        IconRef::Builtin("bluetooth"),
                        &device.name,
                        Some(subtitle),
                        None,
                    )
                    .clicked()
                    {
                        let _ = cx
                            .services
                            .bluetooth
                            .connect(&device.addr, !device.connected);
                    }
                }
            },
        );
    }
    if found.is_empty() {
        ui::note(ui, cx, tr!(s, "Looking for nearby devices…"));
    } else {
        ui::section_card_with(
            ui,
            cx,
            tr!(s, "Available"),
            ui::Deco::new().container(ui::Container::Divided),
            |ui, cx| {
                for device in found {
                    if ui::icon_row(
                        ui,
                        cx,
                        IconRef::Builtin("bluetooth"),
                        &device.name,
                        None,
                        None,
                    )
                    .clicked()
                    {
                        let _ = cx.services.bluetooth.pair(&device.addr);
                    }
                }
            },
        );
    }
}

/// The pairing request, drawn. `true` when it took the screen.
fn pairing_card(ui: &mut Ui, cx: &mut Cx<'_>, snapshot: &crate::services::BtSnapshot) -> bool {
    let s = cx.strings;
    let Some(request) = &snapshot.pending else {
        return false;
    };
    let name = snapshot
        .devices
        .iter()
        .find(|d| d.addr == request.addr)
        .map_or(request.addr.as_str(), |d| d.name.as_str());
    match request.passkey {
        // The backend's passkey, padded the way the device on the other side shows it.
        Some(passkey) => ui::status_card(
            ui,
            cx,
            &format!("{passkey:06}"),
            Some(&tr!(s, "Check this matches {name}, then confirm.").replace("{name}", name)),
            Some(ColorRole::Primary),
        ),
        None => ui::status_card(
            ui,
            cx,
            name,
            Some(tr!(s, "Confirm to pair with this device.")),
            Some(ColorRole::Primary),
        ),
    }
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("check"),
                tr!(s, "Confirm"),
                None,
                None,
            )
            .clicked()
            {
                // `None` — the shell answers a passkey it was **shown**. A PIN to be typed is the
                // integrator's to collect and pass in here.
                let _ = cx.services.bluetooth.respond_pairing(true, None);
            }
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("close"),
                tr!(s, "Cancel"),
                None,
                None,
            )
            .clicked()
            {
                let _ = cx.services.bluetooth.respond_pairing(false, None);
            }
        },
    );
    true
}

/// The `settings.power` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
pub fn power_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    // The request awaiting confirmation. **It is kept in egui's memory** — the body has to be a plain
    // `fn` for `settings.home` to draw the same function into the right column on a wide screen —
    // and it goes when the screen does: a question left open is not there on the next visit.
    held(ui, cx, "fairing.settings.power.pending", power_rows);
}

/// The one question `settings.power` asks before it acts: the request, its confirm row and Cancel.
fn power_confirm(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    request: PowerRequest,
    pending: &mut Option<PowerRequest>,
) {
    let s = cx.strings;
    let (title, verb) = describe(request);
    let (title, verb) = (s.get(title), s.get(verb));
    ui::status_card(
        ui,
        cx,
        title,
        Some(tr!(s, "This cannot be undone.")),
        Some(ColorRole::Warning),
    );
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if ui::icon_row(ui, cx, IconRef::Builtin("check"), verb, None, None).clicked() {
                // The shell gives the integrator a chance to tidy up through the `PowerRequest` event, and then it runs.
                cx.request_power(request);
                *pending = None;
            }
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("close"),
                tr!(s, "Cancel"),
                None,
                None,
            )
            .clicked()
            {
                *pending = None;
            }
        },
    );
}

/// The actual rows of `settings.power`. `pending` is held and saved by the caller.
fn power_rows(ui: &mut Ui, cx: &mut Cx<'_>, pending: &mut Option<PowerRequest>) {
    let s = cx.strings;
    if let Some(request) = *pending {
        power_confirm(ui, cx, request, pending);
        return;
    }

    if let Some(battery) = cx.services.power.battery() {
        let detail = if battery.charging {
            tr!(s, "Charging")
        } else {
            tr!(s, "On battery")
        };
        ui::status_card(
            ui,
            cx,
            &format!("{}%", battery.percent),
            Some(detail),
            Some(if battery.percent <= 20 {
                ColorRole::Danger
            } else {
                ColorRole::Primary
            }),
        );
    }
    if !cx
        .services
        .power
        .capabilities()
        .contains(Capabilities::POWER_CONTROL)
    {
        ui::group_with(
            ui,
            cx,
            ui::Deco::new().container(ui::Container::Divided),
            |ui, cx| {
                ui::info_row(ui, cx, tr!(s, "Restart"), tr!(s, "Not supported"));
            },
        );
        return;
    }
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("restart"),
                tr!(s, "Restart"),
                None,
                None,
            )
            .clicked()
            {
                *pending = Some(PowerRequest::Reboot);
            }
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("power"),
                tr!(s, "Shut down"),
                None,
                None,
            )
            .clicked()
            {
                *pending = Some(PowerRequest::Shutdown);
            }
        },
    );
    ui::note(
        ui,
        cx,
        tr!(
            s,
            "The shell asks the integrator first — it never powers the device down on its own."
        ),
    );
}

/// The `settings.about` screen's body, **without** the scroll wrapper.
///
/// Public so a built-in can be a starting point rather than a wall: drop it into a screen of
/// your own, or call it and add rows around it. `page` is not applied here - hand it to
/// [`layout::page`](crate::layout::page) as the built-in declaration does, or your screen
/// will not scroll.
pub fn about_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    // Whatever the device's `InfoBackend` knows — with the `Null` default, none of it.
    let s = cx.strings;
    let device = cx.services.info.device();
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            for (label, value) in [
                (tr!(s, "Model"), &device.model),
                (tr!(s, "Firmware"), &device.firmware),
                (tr!(s, "System"), &device.os),
            ] {
                if let Some(value) = value {
                    ui::info_row(ui, cx, label, value);
                }
            }
            ui::info_row(ui, cx, "fairing", env!("CARGO_PKG_VERSION"));
            if cx.allows("settings.about.details") {
                if let Some(serial) = &device.serial {
                    ui::info_row(ui, cx, tr!(s, "Serial number"), serial);
                }
                if let Some(secs) = device.uptime_secs {
                    ui::info_row(ui, cx, tr!(s, "Uptime"), &format_uptime(secs, s));
                }
                let info = cx.services.display.info();
                ui::info_row(
                    ui,
                    cx,
                    tr!(s, "Panel"),
                    &format!("{} × {}", info.size_px.0, info.size_px.1),
                );
            }
        },
    );

    ui::section(ui, cx, tr!(s, "Open source licences"));
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        licences,
    );
}

/// Stream the crate table from `THIRD_PARTY.md` as `name version — licence` lines.
///
/// **It does not make a nested scroll** — the outer [`ui::page`] already scrolls, and a second one
/// inside would leave no telling which of them a finger is pushing.
fn licences(ui: &mut Ui, cx: &mut Cx<'_>) {
    const NOTICES: &str = include_str!("../../THIRD_PARTY.md");
    let s = cx.strings;
    let m = &cx.theme.metrics;
    let (inset, size) = (m.screen_inset, m.row_height * 0.23);
    let color = cx.theme.color(ColorRole::Muted);
    let mut lines = 0usize;
    ui.add_space(m.row_height * 0.2);
    for row in NOTICES.lines() {
        // Only `| name | version | licence | repository |` is walked. The two tables' headers and rules
        // are skipped — the assets table has four columns too, so without the condition its header would
        // come out as a row.
        let cells: Vec<&str> = row.trim().trim_matches('|').split('|').collect();
        let [name, version, licence, ..] = cells.as_slice() else {
            continue;
        };
        let (name, version, licence) = (name.trim(), version.trim(), licence.trim());
        if name.is_empty() || name == "Crate" || name == "Asset" || name.starts_with("---") {
            continue;
        }
        lines += 1;
        ui.horizontal(|ui| {
            ui.add_space(inset);
            ui.label(
                egui::RichText::new(format!("{name} {version} — {licence}"))
                    .size(size)
                    .color(color),
            );
        });
    }
    if lines == 0 {
        ui::info_row(ui, cx, tr!(s, "Notices"), tr!(s, "Unavailable"));
    }
    ui.add_space(m.row_height * 0.2);
}

/// Users & access — the authenticator's own entries, behind the
/// `settings.credentials` gate. Registered only where the authenticator manages its entries here
/// ([`Authenticator::admin`](crate::access::Authenticator::admin)); the reference `PinTable` does
/// not — its file is the source of truth.
///
/// A screen of its own rather than [`credentials_body`] in a page, so that what is typed goes when
/// the screen does: stopped, closed, or the session changes, the form and its secrets are dropped
/// there and then. Back closes an open form before it leaves the screen.
#[must_use]
pub fn credentials() -> ScreenDecl {
    crate::screen::screen_of(CREDENTIALS, CredentialsScreen::default())
        .title(tr_key!("Users & access"))
        .icon(IconRef::Builtin("key"))
}

/// The id, and the page's scroll salt.
const CREDENTIALS: &str = "settings.credentials";

/// The body of [`credentials`]: the entries, and a form to change one or add one.
///
/// **The screen keeps no store of its own.** It draws the shell's last
/// [`CredentialAdmin::list`](crate::access::CredentialAdmin::list) and leaves every change with the
/// shell, which hands it to the authenticator — where an entry lives and how its secret is kept
/// never reach the screen. A form stays up until the authenticator has answered its Apply: what
/// went through closes it, and what did not is said on it with everything typed still there.
///
/// Put in a screen of your own, the form in progress is that screen's alone, and it is dropped the
/// first frame the body is not drawn (closed, covered, another entry on the settings home's right)
/// or is drawn for another subject — the next person never finds a half-filled form to apply.
pub fn credentials_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let key = egui::Id::new((CREDENTIALS, cx.pane.instance.0));
    let mut state: CredentialsState = ui.data_mut(|d| d.remove_temp(key)).unwrap_or_default();
    state.ui(ui, cx);
    ui.data_mut(|d| d.insert_temp(key, state));
}

/// [`credentials`]: the state, and the lifecycle that drops it.
#[derive(Default)]
struct CredentialsScreen(CredentialsState);

impl crate::screen::Screen for CredentialsScreen {
    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx<'_>) {
        let state = &mut self.0;
        ui::page(ui, cx, CREDENTIALS, |ui, cx| state.ui(ui, cx));
    }

    /// Back closes the form first — what was typed goes with it.
    fn on_back(&mut self, _cx: &mut Cx<'_>) -> crate::screen::BackAction {
        if self.0.form.take().is_some() {
            crate::screen::BackAction::Consumed
        } else {
            crate::screen::BackAction::Pop
        }
    }

    fn on_lifecycle(&mut self, ev: crate::screen::Lifecycle, _cx: &mut Cx<'_>) {
        use crate::screen::Lifecycle;
        if matches!(
            ev,
            Lifecycle::Stopped | Lifecycle::Destroyed | Lifecycle::AccessChanged
        ) {
            self.0 = CredentialsState::default();
        }
    }
}

/// `settings.credentials` between frames.
#[derive(Clone, Default)]
struct CredentialsState {
    form: Option<CredentialForm>,
    /// The frame it was last drawn on.
    drawn: Option<u64>,
    /// Whom it was drawn for.
    subject: Option<crate::access::Subject>,
    /// The answer already there when the screen came into view. Only a newer one is shown — an
    /// "Added operator-2" from last week is not news.
    since: u64,
}

impl CredentialsState {
    fn ui(&mut self, ui: &mut Ui, cx: &mut Cx<'_>) {
        let frame = cx.frame();
        let shown = self
            .drawn
            .is_some_and(|last| frame.saturating_sub(last) <= 1)
            && self.subject.as_ref() == Some(&cx.session.subject);
        if !shown {
            // Into view: nothing typed before carries over, and the entries are listed again —
            // on the settings home's right the screen is drawn without ever being opened.
            *self = Self {
                subject: Some(cx.session.subject.clone()),
                since: cx.admin_note().map_or(0, |n| n.seq),
                ..Self::default()
            };
            cx.list_credentials();
            ui.ctx().request_repaint();
        }
        self.drawn = Some(frame);
        credential_rows(ui, cx, &mut self.form, self.since);
    }
}

/// A change in progress on `settings.credentials`. The secrets are what is being typed or drawn,
/// and they go with the form when it closes.
#[derive(Clone, PartialEq, Eq)]
struct CredentialForm {
    /// The entry being changed; `None` adds one.
    editing: Option<CredentialEntry>,
    /// A new entry's id, as typed.
    id: String,
    /// The level chosen, as an index into the level table.
    level: usize,
    /// The kind of secret being set, as an index into the authenticator's kinds.
    kind: usize,
    secret: String,
    confirm: String,
    /// The pattern pad's path while a finger draws it.
    pattern: Vec<u8>,
    /// The first drawing of a new pattern, waiting for the second.
    first: Option<Vec<u8>>,
    /// A pattern drawn twice the same — what Apply sets.
    drawn: Option<Vec<u8>>,
    /// Why the last Apply did not go through, as the form found it.
    error: Option<String>,
    /// Applied, and waiting for the answer: the last answer there was when it went.
    sent: Option<u64>,
    /// What the authenticator answered to an Apply that did not all go through.
    answer: Option<AdminNote>,
}

impl std::fmt::Debug for CredentialForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialForm")
            .field("editing", &self.editing.as_ref().map(|e| &e.id))
            .field("level", &self.level)
            .finish_non_exhaustive()
    }
}

impl CredentialForm {
    fn edit(entry: &CredentialEntry) -> Self {
        Self {
            editing: Some(entry.clone()),
            id: entry.id.clone(),
            level: usize::from(entry.level.0),
            ..Self::add(0)
        }
    }

    /// A new entry starts one level up from the bottom — the bottom needs no credential.
    fn add(levels: usize) -> Self {
        Self {
            editing: None,
            id: String::new(),
            level: usize::from(levels > 1),
            kind: 0,
            secret: String::new(),
            confirm: String::new(),
            pattern: Vec::new(),
            first: None,
            drawn: None,
            error: None,
            sent: None,
            answer: None,
        }
    }

    /// Forget the secret typed or drawn so far — another kind was picked.
    fn clear_secret(&mut self) {
        self.secret.clear();
        self.confirm.clear();
        self.pattern.clear();
        self.first = None;
        self.drawn = None;
        self.error = None;
        self.answer = None;
    }

    /// Hand the calls to the shell and wait for the answer.
    fn send(&mut self, ui: &Ui, cx: &mut Cx<'_>, ops: Vec<CredentialOp>) {
        self.sent = Some(cx.admin_note().map_or(0, |n| n.seq));
        self.error = None;
        self.answer = None;
        cx.credential_ops(ops);
        // The answer is there next frame; nothing else may ask for one.
        ui.ctx().request_repaint();
    }

    /// The answer to the form's own Apply, once it is there: `true` when it all went through.
    /// What did not stays on the form with everything typed, and what did is the starting point
    /// for the next Apply — the entry as listed again.
    fn take_answer(&mut self, cx: &Cx<'_>) -> bool {
        let Some(sent) = self.sent else {
            return false;
        };
        let Some(note) = cx.admin_note().filter(|n| n.seq > sent) else {
            return false;
        };
        self.sent = None;
        if note.refused.is_empty() {
            return true;
        }
        self.answer = Some(note.clone());
        if let Some(id) = self.editing.as_ref().map(|e| e.id.clone()) {
            if let Some(listed) = cx.credentials().iter().find(|e| e.id == id) {
                self.editing = Some(listed.clone());
            }
        }
        false
    }
}

/// A kind of secret as a sentence says it: "PIN", "pattern", "password".
/// The form's words for one kind of secret — whole sentences, so a language never has to fit a
/// noun into a sentence shaped for English (a Korean particle follows the noun's last sound).
struct SecretWords {
    /// The heading over a changed entry's secret.
    new: &'static str,
    /// How to leave the secret as it is.
    keep: &'static str,
    /// Why a new entry cannot go without one.
    needed: &'static str,
}

fn secret_words(kind: SecretKind) -> SecretWords {
    match kind {
        SecretKind::Pin { .. } => SecretWords {
            new: tr_key!("New PIN"),
            keep: tr_key!("Leave both empty to keep the current PIN."),
            needed: tr_key!("A new entry needs a PIN."),
        },
        SecretKind::Pattern { .. } => SecretWords {
            new: tr_key!("New pattern"),
            keep: tr_key!("Draw nothing to keep the current pattern."),
            needed: tr_key!("A new entry needs a pattern."),
        },
        SecretKind::Password => SecretWords {
            new: tr_key!("New password"),
            keep: tr_key!("Leave both empty to keep the current password."),
            needed: tr_key!("A new entry needs a password."),
        },
    }
}

/// The level table's labels, in order.
fn level_labels(cx: &Cx<'_>) -> Vec<String> {
    cx.levels().defs().iter().map(|d| d.label.clone()).collect()
}

fn credential_rows(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut Option<CredentialForm>, since: u64) {
    let s = cx.strings;
    if let Some(open) = form.as_mut() {
        if credential_form(ui, cx, open) {
            *form = None;
            // The list comes up next frame rather than under the form's last one.
            ui.ctx().request_repaint();
        }
        return;
    }
    if let Some(note) = cx.admin_note().filter(|n| n.seq > since).cloned() {
        note_cards(ui, cx, &note);
    }
    let entries = cx.credentials().to_vec();
    let levels = level_labels(cx);
    ui::section(ui, cx, tr!(s, "Entries"));
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if entries.is_empty() {
                ui::info_row(ui, cx, tr!(s, "No entries"), "");
            }
            for entry in &entries {
                let level = levels
                    .get(usize::from(entry.level.0))
                    .map_or("", |label| s.get(label));
                let trailing = if entry.disabled {
                    tr!(s, "{level} · off").replace("{level}", level)
                } else {
                    level.to_owned()
                };
                if !cx.may_grant(entry.level) {
                    // Above the session's own level: listed, not changed.
                    ui::info_row(ui, cx, &entry.label, &trailing);
                } else if ui::nav_row(ui, cx, &entry.label, Some(&trailing)).clicked() {
                    *form = Some(CredentialForm::edit(entry));
                }
            }
        },
    );
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("plus"),
                tr!(s, "Add an entry"),
                None,
                None,
            )
            .clicked()
            {
                *form = Some(CredentialForm::add(grantable(cx, levels.len())));
            }
        },
    );
}

/// What an Apply came to, a card a line: what did not go through, then what did.
fn note_cards(ui: &mut Ui, cx: &mut Cx<'_>, note: &AdminNote) {
    let s = cx.strings;
    for (what, why) in &note.refused {
        // The reason is the shell's key or the authenticator's own words — looked up either way.
        ui::status_card(
            ui,
            cx,
            &what.text(s),
            Some(s.get(why)),
            Some(ColorRole::Danger),
        );
    }
    for done in &note.done {
        ui::status_card(ui, cx, &done.text(s), None, Some(ColorRole::Success));
    }
}

/// How many levels from the bottom the session may give: up to its own.
fn grantable(cx: &Cx<'_>, levels: usize) -> usize {
    (0..levels)
        .take_while(|&i| u16::try_from(i).is_ok_and(|i| cx.may_grant(Level(i))))
        .count()
}

/// The form. `true` when it is finished — applied, removed, or cancelled.
fn credential_form(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut CredentialForm) -> bool {
    if form.take_answer(cx) {
        return true;
    }
    let kinds = cx.secret_kinds().to_vec();
    let kind = kinds
        .get(form.kind)
        .copied()
        .unwrap_or(SecretKind::Password);
    let s = cx.strings;
    let mut levels = level_labels(cx);
    // Only what the session may give is offered.
    levels.truncate(grantable(cx, levels.len()).max(1));
    if let Some(entry) = &form.editing {
        ui::title(ui, cx, &entry.label);
    } else {
        ui::title(ui, cx, tr!(s, "New entry"));
        form_field(ui, cx, tr!(s, "Id"), &mut form.id, "operator-2", "cred.id");
    }
    ui::section(ui, cx, tr!(s, "Level"));
    let options: Vec<(&str, Option<&str>)> = levels.iter().map(|l| (s.get(l), None)).collect();
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            if let Some(picked) = ui::choice_rows(ui, cx, &options, form.level) {
                form.level = picked;
            }
        },
    );
    let words = secret_words(kind);
    let heading = match (form.editing.is_some(), kinds.len() > 1) {
        (true, true) => tr!(s, "New secret"),
        (false, true) => tr!(s, "Secret"),
        (true, false) => s.get(words.new),
        (false, false) => s.get(kind.name()),
    };
    ui::section(ui, cx, heading);
    if kinds.len() > 1 {
        // The authenticator takes more than one kind: the form sets whichever is picked.
        let labels: Vec<&str> = kinds.iter().map(|k| s.get(k.name())).collect();
        let pick =
            crate::widgets::SegmentedControl::new(&labels, form.kind).show(ui, &mut cx.widgets());
        if let Some(picked) = pick.picked {
            form.kind = picked;
            form.clear_secret();
        }
    }
    match kind {
        SecretKind::Pattern { grid, min_points } => {
            pattern_entry(ui, cx, form, grid, min_points);
        }
        SecretKind::Pin { len: 0, max_len } => {
            let hint = tr!(s, "Up to {n} digits").replace("{n}", &max_len.to_string());
            secret_fields(ui, cx, form, &hint);
        }
        SecretKind::Pin { len, .. } => {
            let hint = tr!(s, "{n} digits").replace("{n}", &len.to_string());
            secret_fields(ui, cx, form, &hint);
        }
        SecretKind::Password => secret_fields(ui, cx, form, ""),
    }
    if let Some(entry) = form.editing.clone() {
        ui::note(ui, cx, s.get(words.keep));
        // Removing is the one change that cannot be taken back, so it is held, not tapped.
        let remove = crate::widgets::BigButton::new(tr!(s, "Remove"))
            .kind(crate::widgets::ButtonKind::Danger)
            .long_press(std::time::Duration::from_secs(1))
            .show(ui, &mut cx.widgets());
        if remove.completed && form.sent.is_none() {
            form.send(ui, cx, vec![CredentialOp::Remove { id: entry.id }]);
        }
    }
    credential_foot(ui, cx, form, kind)
}

/// The foot of the form: what the last Apply came to, and Apply and Cancel. `true` when the form
/// is finished.
fn credential_foot(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    form: &mut CredentialForm,
    kind: SecretKind,
) -> bool {
    let s = cx.strings;
    if let Some(error) = &form.error {
        ui::status_card(
            ui,
            cx,
            tr!(s, "Not saved"),
            Some(error),
            Some(ColorRole::Danger),
        );
    }
    if let Some(answer) = form.answer.clone() {
        note_cards(ui, cx, &answer);
    }
    if form.sent.is_some() {
        ui::note(ui, cx, tr!(s, "Saving…"));
    }
    let (mut apply, mut cancel) = (false, false);
    ui::group_with(
        ui,
        cx,
        ui::Deco::new().container(ui::Container::Divided),
        |ui, cx| {
            apply = ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("check"),
                tr!(s, "Apply"),
                None,
                None,
            )
            .clicked();
            cancel = ui::icon_row(
                ui,
                cx,
                IconRef::Builtin("close"),
                tr!(s, "Cancel"),
                None,
                None,
            )
            .clicked();
        },
    );
    if cancel {
        return true;
    }
    if apply && form.sent.is_none() {
        match credential_ops(form, kind, cx.credentials(), s) {
            // Nothing changed: there is nothing to wait for.
            Ok(ops) if ops.is_empty() => return true,
            Ok(ops) => form.send(ui, cx, ops),
            Err(error) => {
                form.error = Some(error);
                form.answer = None;
            }
        }
    }
    false
}

/// The secret and its repeat, typed.
fn secret_fields(ui: &mut Ui, cx: &mut Cx<'_>, form: &mut CredentialForm, hint: &str) {
    let again = tr!(cx.strings, "Again");
    for (text, hint, salt) in [
        (&mut form.secret, hint, "cred.secret"),
        (&mut form.confirm, again, "cred.confirm"),
    ] {
        crate::widgets::TextField::new(text)
            .password(true)
            .hint(hint)
            .id_salt(salt)
            .show(ui, &mut cx.widgets());
    }
}

/// A new pattern, drawn twice: the first drawing waits for the second, the two must match, and a
/// stroke after a match starts over.
fn pattern_entry(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    form: &mut CredentialForm,
    grid: u8,
    min_points: u8,
) {
    let s = cx.strings;
    let step = match (&form.drawn, &form.first) {
        (Some(_), _) => tr!(
            s,
            "Drawn twice the same - Apply saves it. A new stroke starts over."
        ),
        (None, Some(_)) => tr!(s, "Draw it again to confirm."),
        (None, None) => tr!(s, "Draw the new pattern."),
    };
    ui::note(ui, cx, step);
    let target = crate::theme::control_height(&cx.theme.metrics, &cx.theme.control);
    let side = ui.available_width().min(target * 5.0);
    let pad = ui
        .allocate_ui_with_layout(
            egui::Vec2::splat(side),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                crate::widgets::PatternPad::new(&mut form.pattern)
                    .grid(grid)
                    .min_points(min_points)
                    .show(ui, &mut cx.widgets())
            },
        )
        .inner;
    if pad.changed && !form.pattern.is_empty() {
        form.error = None;
        if form.drawn.take().is_some() {
            form.first = None;
        }
    }
    if pad.too_short {
        form.pattern.clear();
        form.error = Some(
            s.get(crate::access::prompt::labels::MIN_DOTS)
                .replace("{n}", &min_points.to_string()),
        );
    }
    if pad.submitted {
        let path = std::mem::take(&mut form.pattern);
        pattern_submitted(form, path, s);
    }
}

/// A finished drawing on the form: the first waits for the second, a match is what Apply sets,
/// and a mismatch starts over.
fn pattern_submitted(form: &mut CredentialForm, path: Vec<u8>, strings: &Strings) {
    match form.first.take() {
        None => form.first = Some(path),
        Some(first) if first == path => form.drawn = Some(path),
        Some(_) => {
            form.error =
                Some(tr!(strings, "The two patterns do not match - draw it again.").to_owned());
        }
    }
}

/// What Apply asks for, checked first: the two secrets match, a PIN is digits and no longer than
/// the authenticator takes, a pattern was drawn twice the same, a new entry has an id nobody has
/// and a secret. The authenticator still has the last word on all of it.
fn credential_ops(
    form: &CredentialForm,
    kind: SecretKind,
    existing: &[CredentialEntry],
    strings: &Strings,
) -> Result<Vec<CredentialOp>, String> {
    let level = Level(u16::try_from(form.level).unwrap_or(0));
    let typed = form.secret.as_str();
    let secret = match kind {
        SecretKind::Pattern { .. } => {
            if form.first.is_some() && form.drawn.is_none() {
                return Err(tr!(strings, "Draw the pattern again to confirm it.").to_owned());
            }
            form.drawn.clone().map(Secret::Pattern)
        }
        SecretKind::Pin { len, max_len } => {
            if typed != form.confirm {
                return Err(tr!(strings, "The two PINs do not match.").to_owned());
            }
            let digits = typed.bytes().all(|b| b.is_ascii_digit());
            if !typed.is_empty() {
                // A fixed length is the only one the prompt can take: it submits on the last digit.
                if len > 0 && (typed.len() != usize::from(len) || !digits) {
                    return Err(
                        tr!(strings, "A PIN is {n} digits.").replace("{n}", &len.to_string())
                    );
                }
                if typed.len() > usize::from(max_len) || !digits {
                    return Err(tr!(strings, "A PIN is 1 to {n} digits.")
                        .replace("{n}", &max_len.to_string()));
                }
            }
            (!typed.is_empty()).then(|| Secret::Pin(typed.to_owned()))
        }
        SecretKind::Password => {
            if typed != form.confirm {
                return Err(tr!(strings, "The two passwords do not match.").to_owned());
            }
            (!typed.is_empty()).then(|| Secret::Password(typed.to_owned()))
        }
    };
    let Some(entry) = &form.editing else {
        let id = form.id.trim();
        if id.is_empty() {
            return Err(tr!(strings, "A new entry needs an id.").to_owned());
        }
        if existing.iter().any(|e| e.id == id) {
            return Err(tr!(strings, "`{id}` is taken.").replace("{id}", id));
        }
        let Some(secret) = secret else {
            return Err(strings.get(secret_words(kind).needed).to_owned());
        };
        return Ok(vec![CredentialOp::Add {
            id: id.to_owned(),
            level,
            secret,
        }]);
    };
    let mut ops = Vec::new();
    if level != entry.level {
        ops.push(CredentialOp::SetLevel {
            id: entry.id.clone(),
            level,
        });
    }
    if let Some(secret) = secret {
        ops.push(CredentialOp::SetSecret {
            id: entry.id.clone(),
            secret,
        });
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::{
        credential_ops, describe_service_error, format_uptime, parse_static, pattern_submitted,
        valid_hostname, CredentialForm,
    };
    use crate::access::{CredentialEntry, CredentialOp, Level, Secret, SecretKind};
    use crate::i18n::Strings;

    /// The English table — the keys as written.
    fn en() -> Strings {
        Strings::new("en")
    }

    const PIN: SecretKind = SecretKind::Pin {
        len: 0,
        max_len: 16,
    };
    const PATTERN: SecretKind = SecretKind::Pattern {
        grid: 3,
        min_points: 4,
    };
    use crate::services::{ErrorKind, IpConfig, Ipv4Net, ServiceError};
    use std::net::{IpAddr, Ipv4Addr};

    /// The manual form parses with `std::net`; gateway and DNS may be left empty.
    #[test]
    fn a_static_form_parses_or_names_the_field() {
        let parsed = parse_static(" 10.0.0.5 ", "8", "10.0.0.1", "10.0.0.1, 1.1.1.1", &en());
        assert_eq!(
            parsed,
            Ok(IpConfig::Static {
                addr: Ipv4Net {
                    addr: Ipv4Addr::new(10, 0, 0, 5),
                    prefix: 8,
                },
                gateway: Some(Ipv4Addr::new(10, 0, 0, 1)),
                dns: vec![
                    IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
                    IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
                ],
            })
        );
        assert!(matches!(
            parse_static("10.0.0.5", "24", "", "", &en()),
            Ok(IpConfig::Static { gateway: None, ref dns, .. }) if dns.is_empty()
        ));
        let address = parse_static("10.0.0", "24", "", "", &en());
        assert!(address.is_err_and(|e| e.contains("address `10.0.0`")));
        let prefix = parse_static("10.0.0.5", "33", "", "", &en());
        assert!(prefix.is_err_and(|e| e.contains("prefix length")));
        let gateway = parse_static("10.0.0.5", "24", "router", "", &en());
        assert!(gateway.is_err_and(|e| e.contains("gateway `router`")));
        let dns = parse_static("10.0.0.5", "24", "", "1.1.1.1, nope", &en());
        assert!(dns.is_err_and(|e| e.contains("DNS server `nope`")));
    }

    /// RFC 1123 labels: letters, digits and hyphens, 1–63, no hyphen at either end.
    #[test]
    fn hostnames_follow_rfc_1123() {
        assert!(valid_hostname("fairing-demo"));
        assert!(valid_hostname("a"));
        assert!(valid_hostname(&"x".repeat(63)));
        assert!(!valid_hostname(""));
        assert!(!valid_hostname(&"x".repeat(64)));
        assert!(!valid_hostname("-lead"));
        assert!(!valid_hostname("trail-"));
        assert!(!valid_hostname("has space"));
        assert!(!valid_hostname("dotted.name"));
    }

    /// `Unsupported` reads as a plain sentence; anything else carries the backend's message.
    #[test]
    fn backend_errors_read_as_sentences() {
        assert_eq!(
            describe_service_error(&ServiceError::unsupported(), &en()),
            "This device does not allow it to be changed here."
        );
        assert_eq!(
            describe_service_error(
                &ServiceError::new(ErrorKind::Denied, "read-only image"),
                &en()
            ),
            "read-only image"
        );
    }

    #[test]
    fn uptime_shows_the_two_largest_units() {
        assert_eq!(format_uptime(59, &en()), "0 min");
        assert_eq!(format_uptime(4 * 3_600 + 12 * 60, &en()), "4 h 12 min");
        assert_eq!(format_uptime(3 * 86_400 + 4 * 3_600 + 59, &en()), "3 d 4 h");
    }

    fn entry(id: &str, level: u16) -> CredentialEntry {
        CredentialEntry {
            id: id.to_owned(),
            label: id.to_owned(),
            level: Level(level),
            disabled: false,
        }
    }

    /// The form checks what it can before the authenticator sees anything.
    #[test]
    fn the_credential_form_checks_before_it_asks() {
        let existing = [entry("ada", 1)];
        let mut add = CredentialForm::add(3);
        add.id = "bob".to_owned();
        add.secret = "4321".to_owned();
        add.confirm = "4320".to_owned();
        assert!(
            credential_ops(&add, PIN, &existing, &en()).is_err(),
            "a mismatch"
        );
        add.confirm = "4321".to_owned();
        let ops = credential_ops(&add, PIN, &existing, &en());
        assert!(matches!(
            ops.as_deref(),
            Ok([CredentialOp::Add { id, level: Level(1), .. }]) if id == "bob"
        ));
        add.secret = "43a1".to_owned();
        add.confirm = "43a1".to_owned();
        assert!(
            credential_ops(&add, PIN, &existing, &en()).is_err(),
            "a letter in a PIN"
        );
        assert!(
            credential_ops(&add, SecretKind::Password, &existing, &en()).is_ok(),
            "but fine in a password"
        );
        add.id = "ada".to_owned();
        assert!(
            credential_ops(&add, SecretKind::Password, &existing, &en()).is_err(),
            "a taken id"
        );
    }

    /// Editing asks only for what changed — a level, a secret, both, or nothing.
    #[test]
    fn an_edit_asks_only_for_what_changed() {
        let ada = entry("ada", 1);
        let mut edit = CredentialForm::edit(&ada);
        assert_eq!(credential_ops(&edit, PIN, &[], &en()), Ok(Vec::new()));
        edit.level = 2;
        edit.secret = "9999".to_owned();
        edit.confirm = "9999".to_owned();
        let ops = credential_ops(&edit, PIN, &[], &en()).unwrap_or_default();
        assert!(matches!(
            ops.as_slice(),
            [
                CredentialOp::SetLevel {
                    level: Level(2),
                    ..
                },
                CredentialOp::SetSecret { .. }
            ]
        ));
        assert!(
            !format!("{ops:?}").contains("9999"),
            "no secret in a debug line"
        );
    }

    /// A PIN no longer than the authenticator takes, and a pattern drawn twice the same.
    #[test]
    fn the_form_follows_the_kind_of_secret() {
        let mut add = CredentialForm::add(3);
        add.id = "bob".to_owned();
        add.secret = "1234567".to_owned();
        add.confirm = "1234567".to_owned();
        let six = SecretKind::Pin { len: 0, max_len: 6 };
        assert!(
            credential_ops(&add, six, &[], &en()).is_err(),
            "seven over six"
        );
        add.secret.pop();
        add.confirm.pop();
        assert!(credential_ops(&add, six, &[], &en()).is_ok());
        // Where the prompt takes exactly four, six digits could never be entered there.
        let four = SecretKind::Pin { len: 4, max_len: 4 };
        assert!(
            credential_ops(&add, four, &[], &en()).is_err(),
            "six where four"
        );
        add.secret.truncate(4);
        add.confirm.truncate(4);
        assert!(credential_ops(&add, four, &[], &en()).is_ok());
        add.secret.truncate(3);
        add.confirm.truncate(3);
        assert!(
            credential_ops(&add, four, &[], &en()).is_err(),
            "three where four"
        );

        let mut add = CredentialForm::add(3);
        add.id = "bob".to_owned();
        assert!(
            credential_ops(&add, PATTERN, &[], &en()).is_err(),
            "nothing drawn"
        );
        add.first = Some(vec![0, 1, 2, 5]);
        assert!(
            credential_ops(&add, PATTERN, &[], &en()).is_err(),
            "drawn once, not confirmed"
        );
        add.first = None;
        add.drawn = Some(vec![0, 1, 2, 5]);
        let ops = credential_ops(&add, PATTERN, &[], &en()).unwrap_or_default();
        assert!(matches!(
            ops.as_slice(),
            [CredentialOp::Add { secret: Secret::Pattern(dots), .. }] if dots == &[0, 1, 2, 5]
        ));
        // Drawn once, then again: a match is set, a mismatch starts over.
        let mut form = CredentialForm::add(3);
        pattern_submitted(&mut form, vec![0, 1, 2, 5], &en());
        assert_eq!(form.first.as_deref(), Some(&[0, 1, 2, 5][..]));
        pattern_submitted(&mut form, vec![0, 1, 2, 4], &en());
        assert!(form.first.is_none() && form.drawn.is_none() && form.error.is_some());
        pattern_submitted(&mut form, vec![0, 1, 2, 5], &en());
        pattern_submitted(&mut form, vec![0, 1, 2, 5], &en());
        assert_eq!(form.drawn.as_deref(), Some(&[0, 1, 2, 5][..]));
        // Editing with nothing drawn keeps the pattern and asks for nothing.
        let mut edit = CredentialForm::edit(&entry("ada", 1));
        edit.kind = 1;
        assert_eq!(credential_ops(&edit, PATTERN, &[], &en()), Ok(Vec::new()));
    }
}
