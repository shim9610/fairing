//! Device settings screens and forms.

use super::{
    describe, format_clock, format_offset, format_uptime, held, keys, percent, stored_percent, tr,
    ui, Capabilities, ColorRole, Cx, IconRef, PowerRequest, SettingValue, Ui,
};

/// The `settings.display` screen's body, **without** the scroll wrapper.
///
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
pub fn display_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    let s = cx.strings;
    let info = cx.services.display.info();
    let has_brightness = cx
        .services
        .display
        .capabilities()
        .contains(Capabilities::BRIGHTNESS);

    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
                cx.set_setting(
                    keys::DISPLAY_BRIGHTNESS,
                    SettingValue::Int(i64::from(level)),
                );
            }
        } else {
            ui::info_row(ui, cx, tr!(s, "Brightness"), tr!(s, "Not supported"));
        }
    });

    ui::section_card_with(ui, cx, tr!(s, "Theme"), ui::Deco::new(), |ui, cx| {
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
    });
    ui::note(
        ui,
        cx,
        tr!(s, "Applies to the shell and every built-in screen."),
    );

    ui::section_card_with(ui, cx, tr!(s, "Panel"), ui::Deco::new(), |ui, cx| {
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
    });
}
/// The `settings.sound` screen's body, **without** the scroll wrapper.
///
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
    });

    ui::section_card_with(ui, cx, tr!(s, "Alerts"), ui::Deco::new(), |ui, cx| {
        let mut silent = matches!(
            cx.settings.get(&keys::UI_SILENT.into()),
            Some(SettingValue::Bool(true))
        );
        if ui::switch_row(ui, cx, tr!(s, "Silent"), None, &mut silent, true) {
            cx.set_setting(keys::UI_SILENT, SettingValue::Bool(silent));
        }
    });
    ui::note(
        ui,
        cx,
        tr!(s, "Notifications still appear; only the sound is muted."),
    );
}
/// The `settings.locale` screen's body, **without** the scroll wrapper.
///
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        if let Some(index) = ui::choice_rows(ui, cx, &options, current) {
            if let Some((tag, _)) = locales.get(index) {
                cx.set_setting(keys::UI_LOCALE, SettingValue::Text((*tag).to_owned()));
            }
        }
    });
}
/// The `settings.datetime` screen's body, **without** the scroll wrapper.
///
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
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

    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        if ui::switch_row(ui, cx, tr!(s, "24-hour time"), None, &mut h24, true) {
            cx.set_setting(keys::UI_CLOCK_12H, SettingValue::Bool(!h24));
        }
    });

    ui::section_card_with(ui, cx, tr!(s, "Time source"), ui::Deco::new(), |ui, cx| {
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
    });
    ui::note(
        ui,
        cx,
        tr!(
            s,
            "The clock comes from the integrator's backend; the shell never sets it on its own."
        ),
    );
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        let mut enabled = snapshot.enabled;
        if ui::switch_row(ui, cx, tr!(s, "Bluetooth"), None, &mut enabled, true) {
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
    });
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
        ui::section_card_with(ui, cx, tr!(s, "Paired"), ui::Deco::new(), |ui, cx| {
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
        });
    }
    if found.is_empty() {
        ui::note(ui, cx, tr!(s, "Looking for nearby devices…"));
    } else {
        ui::section_card_with(ui, cx, tr!(s, "Available"), ui::Deco::new(), |ui, cx| {
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
        });
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
    });
    true
}
/// The `settings.power` screen's body, **without** the scroll wrapper.
///
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        if ui::icon_row(ui, cx, IconRef::Builtin("check"), verb, None, None).clicked() {
            // The shell emits PowerRequest; the host decides whether and when to commit the power action.
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
    });
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
        ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
            ui::info_row(ui, cx, tr!(s, "Restart"), tr!(s, "Not supported"));
        });
        return;
    }
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
    });
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
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
pub fn about_body(ui: &mut Ui, cx: &mut Cx<'_>) {
    // Whatever the device's `InfoBackend` knows — with the `Null` default, none of it.
    let s = cx.strings;
    let device = cx.services.info.device();
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
    });

    ui::section(ui, cx, tr!(s, "Open source licences"));
    ui::group_with(ui, cx, ui::Deco::new(), licences);
}
/// Stream the crate table from `THIRD_PARTY.md` as `name version — licence` lines.
///
/// **It does not make a nested scroll** — the outer [`ui::page`] already scrolls, and a second one
/// inside would leave no telling which of them a finger is pushing.
fn licences(ui: &mut Ui, cx: &mut Cx<'_>) {
    const NOTICES: &str = include_str!("../../../THIRD_PARTY.md");
    let s = cx.strings;
    let m = &cx.theme.metrics;
    // Small type from the scale: a share of the row is not a type size.
    let (inset, size) = (m.screen_inset, m.type_scale.small);
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

#[cfg(test)]
mod tests {
    use super::super::en;
    use super::*;
    #[test]
    fn uptime_shows_the_two_largest_units() {
        assert_eq!(format_uptime(59, &en()), "0 min");
        assert_eq!(format_uptime(4 * 3_600 + 12 * 60, &en()), "4 h 12 min");
        assert_eq!(format_uptime(3 * 86_400 + 4 * 3_600 + 59, &en()), "3 d 4 h");
    }
}
