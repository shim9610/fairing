//! Network settings screens and forms.

use super::{
    form_field, held, keys, tr, tr_key, ui, ColorRole, Cx, ErrorKind, IconRef, IfaceKind,
    IfaceSnapshot, IpAddr, IpConfig, Ipv4Addr, Ipv4Net, ServiceError, SettingValue, Strings, Ui,
};

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
/// [`WifiBackend::connect`](crate::services::WifiBackend::connect) borrows the PSK
/// as `&str`. The application owns the input buffer; credential handling and any
/// retention by the backend remain application responsibilities.
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
        let mut enabled = snapshot.enabled;
        if ui::switch_row(ui, cx, tr!(s, "Wi-Fi"), None, &mut enabled, true) {
            cx.set_setting(keys::WIFI_ENABLED, SettingValue::Bool(enabled));
        }
    });
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

    ui::section_card_with(ui, cx, tr!(s, "Networks"), ui::Deco::new(), |ui, cx| {
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
    });
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
/// Use this body inside an application-owned screen, optionally with additional
/// rows. It supplies no scroll wrapper; use [`layout::page`](crate::layout::page)
/// or another scrolling container when needed.
///
/// The interfaces are read from the backend every frame the screen is drawn. An edit is a form
/// held in egui's memory (the body is a plain `fn`, as `settings.power`'s is) and checked with
/// `std::net` before anything reaches the backend; what the backend then says — `Unsupported`
/// included — is shown on the form rather than dropped.
///
/// Form state is keyed to the drawing screen instance. On the next body call,
/// a gap in drawing or a changed subject discards the saved form. A denied
/// `settings.network.edit` check clears it during that call.
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
        ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
        });
    }

    if let Some(hostname) = cx.services.network.hostname() {
        ui::section(ui, cx, tr!(s, "This device"));
        ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
            if editable {
                if ui::nav_row(ui, cx, tr!(s, "Hostname"), Some(&hostname)).clicked() {
                    *form = Some(NetworkForm::hostname(&hostname));
                }
            } else {
                ui::info_row(ui, cx, tr!(s, "Hostname"), &hostname);
            }
        });
    }
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
    ui::group_with(ui, cx, ui::Deco::new(), |ui, cx| {
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
    });
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

#[cfg(test)]
mod tests {
    use super::super::en;
    use super::*;
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
}
