//! The built-in settings screens. Feature `settings`.
//!
//! # The three depths a manufacturer can adjust at
//!
//! | Depth | How | What changes |
//! |---|---|---|
//! | Shallow | `[theme.palette]` · [`ShellBuilder::palettes`](crate::shell::ShellBuilder::palettes) | Theme colour roles used by built-in screens |
//! | Middle | Individual exclusions via [`SettingsConfig`](super::SettingsConfig) | Which screens exist |
//! | Deep | `shell.add(screen("settings.wifi", ..))` under the same id | That screen **whole** |
//!
//! The deep row holds **everywhere**, the two-pane split included: on a wide screen the right
//! column draws whatever the registry has under the selected id, so a replaced built-in
//! and a screen of the integrator's own reach it by the same road.
//!
//! Built-in geometry uses theme metrics. Metrics expressed in finger units change
//! with the finger-size policy; text and fixed-size content have separate inputs.
//!
//! # Without the backend, it is not registered
//!
//! Registration filters screens using [`Capabilities`] unless that filter is
//! explicitly disabled in `SettingsConfig`.
//!
//! The list follows: a row is drawn only where the screen behind it **is really registered**
//! ([`Cx::has_screen`]). `settings.network` is registered where the network backend reports
//! `ETHERNET`, and `settings.credentials` where the authenticator manages its own entries.
mod device;
pub use device::{
    about_body, bluetooth_body, datetime_body, display_body, locale_body, power_body, sound_body,
};
mod network;
pub use network::{network_body, wifi_body};
mod credentials;
pub use credentials::{credentials, credentials_body};

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
/// application's, including credential prompting and profile handling.
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
/// The notices table is included from `THIRD_PARTY.md`. Audit stage 10 checks
/// that file against the repository's dependency inventory.
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

/// Format uptime as days/hours, hours/minutes, or minutes: `3 d 4 h`, `4 h 12 min`, `12 min`.
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

/// Offset minutes → `+09:00`.
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
    egui::Id::unique("fairing.settings.home.selected")
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
        egui::Stroke::new(
            cx.theme.control.stroke_hairline,
            cx.theme.color(ColorRole::Outline),
        ),
    );

    let mut right = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(
        egui::pos2(split + 1.0, full.top()),
        full.max,
    )));
    // **The two titles on one line.** The list's title sits under the top gap its `page` opens,
    // and the right column's title is drawn outside a page — the screen under it brings its own
    // scroll — so without the same gap here "Wi-Fi" sat higher than "Settings" beside it.
    right.add_space(cx.theme.metrics.row_height * ui::CARD_GAP);
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
/// The lookup uses the registered declaration's gate, including an explicit
/// `.gate(...)` override, rather than treating the entry ID as the grant ID.
fn shows(cx: &Cx<'_>, id: &str) -> bool {
    cx.screen_allowed(id)
}

/// The `settings.home` body (narrow screens).
fn home_body(ui: &mut Ui, cx: &mut Cx<'_>, entries: &[SettingsEntry]) {
    let s = cx.strings;
    ui::title(ui, cx, tr!(s, "Settings"));
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        for entry in entries {
            if !shows(cx, &entry.id) {
                continue;
            }
            if ui::icon_row(ui, cx, entry.icon.clone(), s.get(&entry.title), None, None).clicked() {
                cx.open(&entry.id);
            }
        }
    });
}

/// A body's state between frames: what it holds, the frame it was last drawn on and whom for.
#[derive(Clone)]
struct Held<T> {
    value: Option<T>,
    drawn: u64,
    subject: crate::access::Subject,
}

/// Run `body` with a value kept in egui's memory for the drawing screen instance.
/// Discard the saved value on the next body call after a gap in drawing or a changed subject. A plain-`fn` body
/// uses this retained value for form state between consecutive eligible calls.
fn held<T: Clone + Send + Sync + 'static>(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    state_id: &'static str,
    body: impl FnOnce(&mut Ui, &mut Cx<'_>, &mut Option<T>),
) {
    let key = egui::Id::unique((state_id, cx.pane.instance.0));
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

/// One labelled field of a form.
fn form_field(
    ui: &mut Ui,
    cx: &mut Cx<'_>,
    label: &str,
    text: &mut String,
    hint: &str,
    field_id: &'static str,
) {
    ui::section(ui, cx, label);
    crate::widgets::TextField::new(text)
        .hint(hint)
        .id_salt(field_id)
        .show(ui, &mut cx.widgets());
}

#[cfg(test)]
/// The English table — the keys as written.
fn en() -> Strings {
    Strings::new("en")
}
