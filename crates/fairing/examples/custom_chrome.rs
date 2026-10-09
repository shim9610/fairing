//! An example of **an integrator redrawing the whole shell without touching the crate**
//! (M2a). `cargo run -p fairing --features runner-x11 --example custom_chrome -- --size=1024x600`.
//!
//! What is changed here, and the hook for it:
//!
//! | Changed | Hook |
//! |---|---|
//! | The palette, metrics and motion | A [`fairing::Theme`] built in code and injected (`[shell] theme` and `[theme.palette]` ignored) |
//! | The whole status bar | [`ShellBuilder::status_bar_painter`](fairing::ShellBuilder::status_bar_painter) — the device name on the left, the clock and the Wi-Fi strength on the right |
//! | The nav bar | `[nav_bar] enabled = false` — back is **a button inside the screen** (`cx.finish()`) |
//! | The desktop cells | [`ShellBuilder::slot_painter`](fairing::ShellBuilder::slot_painter) — the icons as round badges |
//! | The wallpaper | [`Wallpaper::Painter`](fairing::Wallpaper::Painter) — a procedural grid |
//! | The dock | None (`[desktop] dock = []`) |
//! | A quick-settings tile (declared) | [`TileKind::Gauges`](fairing::overlay::TileKind::Gauges) — three hopper level gauges. **Only the row count, the names, the colours and whether they can be operated are written down; the crate does the drawing** |
//! | A quick-settings tile (drawn) | [`overlay::tile_panel`](fairing::overlay::tile_panel) — an axis jog pad that fits no standard shape. The inside of the row is ours whole |
//!
//! What the shell goes on doing: the layout Rects, the gate decisions, the hit testing and press
//! decisions, the A2/A3 transitions, the repaint policy. A painter **only draws**.
//!
//! # Tour mode (`--tour <dir>`)
//!
//! ```text
//! xvfb-run -a -s "-screen 0 1024x600x24" env LIBGL_ALWAYS_SOFTWARE=1 \
//!   cargo run -p fairing --features runner-x11 --example custom_chrome -- --tour target/tour
//! ```
//! It leaves four frames (`01-custom` the desktop · `02-custom-tiles` the shade · `03-custom-gauges`
//! the three declared gauges · `04-custom-panel` the hand-drawn jog pad) and closes the window. The
//! driver and the PNG encoder are shared with `demo.rs` in `examples/common/`.

mod common;

use common::{Act, Expect, Side, Spot};
use egui::{Color32, Rect, Stroke};
use fairing::config::{DesktopConfig, NavBarConfig, OverlayConfig, ShellConfig, StatusBarConfig};
use fairing::icons::{IconColor, IconStyle};
use fairing::overlay::{tile, tile_panel, Gauge, TileKind};
use fairing::runner::{self, Options};
use fairing::services::mock::{MockClock, MockPower, MockWifi, WifiMsg};
use fairing::services::{Services, WallTime, WifiState};
use fairing::settings::SettingValue;
use fairing::theme::{MetricsSpec, Palette};
use fairing::time::ClockFormat;
use fairing::unit::{Dim, Span};
use fairing::{icon, screen, BarCx, ColorRole, Cx, SlotCx, Theme, Wallpaper};

/// The default window size.
const SIZE: (f32, f32) = (
    fairing::testing::DEFAULT_SIZE.x,
    fairing::testing::DEFAULT_SIZE.y,
);

/// The device's name — drawn on the left of the status bar. The on-screen text is English as in the
/// demo: `default_fonts` has no CJK, so Hangul comes out as □ (an integrator loads a font
/// with `ShellBuilder::fonts`).
const DEVICE: &str = "LAB-7 CONTROL";

/// The tour's script: the desktop · the shade · the place our own tile opens.
const PLAN: &[Act] = &[
    Act::Settle,
    Act::Wait(15), // time for the font atlas and the first repaint to settle
    Act::Shot("01-custom.png"),
    // Pull the shade down — `tile.hopper` sits between two built-in tiles.
    Act::Press(Spot::Edge(Side::Top, 0.01)),
    Act::DragTo {
        to: Spot::Page(0.01, 0.45),
        frames: 10,
    },
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Shot("02-custom-tiles.png"),
    Act::Expect(Expect::ShadeOpen),
    // Pressing `tile.hopper` — by its label, wherever the row put it — opens three gauge rows
    // **exactly as declared** (the row count, the colours, the names, the units).
    Act::Tap(Spot::Text("Hoppers")),
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::Text("Resin A")),
    Act::Expect(Expect::Text("Solvent")),
    Act::Shot("03-custom-gauges.png"),
    // `tile.jog` fits no standard shape, so it takes a whole row. (The hopper has left the row and
    // the tiles left behind have filled the gap, so Jog has come to the middle.)
    Act::Tap(Spot::Text("Jog")),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("04-custom-panel.png"),
    Act::Wait(2),
];

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size = common::arg_size(&args);
    let tour = common::arg_tour(&args);
    let options = Options {
        fullscreen: tour.is_none() && size.is_none(),
        title: "fairing custom chrome".to_owned(),
        size: Some(size.unwrap_or(SIZE)),
    };
    match tour {
        Some(dir) => common::run_tour(options, dir, PLAN, |ctx| Ok((build(ctx)?, ()))),
        None => runner::run_shell(options, build),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// The theme and the config
// ─────────────────────────────────────────────────────────────────────────────

/// A theme built in code. It settles the palette and the metrics whole rather than using
/// `[theme.palette]` — injecting one has `[shell] theme` ignored, with one warning logged.
fn theme() -> Theme {
    let mut theme = Theme::dark();
    theme.palette = Palette {
        background: Color32::from_rgb(0x06, 0x11, 0x0d),
        surface: Color32::from_rgb(0x0d, 0x1f, 0x18),
        surface_variant: Color32::from_rgb(0x14, 0x2e, 0x24),
        on_surface: Color32::from_rgb(0xd6, 0xf5, 0xe4),
        muted: Color32::from_rgb(0x5f, 0x8a, 0x76),
        primary: Color32::from_rgb(0x2f, 0xe0, 0x9b),
        ..Palette::dark()
    };
    theme
}

/// The chrome's sizes, **in the hand's units**: a bar three quarters of a finger, icons a finger
/// across in cells of two and a third, a screen inset of three millimetres. Resolved against the
/// panel's density and the finger policy by the shell, so a gloved finger or a denser panel
/// gets the same chrome at the same physical size — where a block of `Metrics` literals (44,
/// 132, 56, 20 du) gave one panel's sizes to every panel.
fn metrics() -> MetricsSpec {
    MetricsSpec {
        status_bar_height: Span::fixed(Dim::finger(0.75)).reanchored(),
        icon_cell: Span::fixed(Dim::finger(2.3)).reanchored(),
        icon_size: Span::fixed(Dim::finger(1.0)).reanchored(),
        screen_inset: Span::fixed(Dim::mm(3.2)).reanchored(),
        ..MetricsSpec::default()
    }
}

/// A config literal — settled in code, with no file. No dock, no nav bar.
fn config() -> ShellConfig {
    ShellConfig {
        status_bar: StatusBarConfig {
            // The slot lists settle only **what to show**. The layout and the drawing are the
            // painter's, and the gate decisions are the shell's and arrive through `BarCx::items()`.
            left: Vec::new(),
            center: Vec::new(),
            right: vec!["status.clock".to_owned(), "status.wifi".to_owned()],
            ..StatusBarConfig::default()
        },
        nav_bar: NavBarConfig {
            enabled: false,
            ..NavBarConfig::default()
        },
        desktop: DesktopConfig {
            columns: 4,
            rows: 2,
            dock: Vec::new(),
            ..DesktopConfig::default()
        },
        overlay: OverlayConfig {
            // **A tile the crate knows nothing of is slotted in.** The list can mix built-in ids and
            // declaration ids, and the order is settled here — `tile.hopper` and `tile.jog` are ours.
            // A built-in tile not in the list (the theme, the lock …) does not stand at all.
            tiles: vec![
                "tile.wifi".to_owned(),
                "tile.hopper".to_owned(),
                "tile.jog".to_owned(),
                "tile.brightness".to_owned(),
            ],
            tile_columns: 4,
            ..OverlayConfig::default()
        },
        ..ShellConfig::default()
    }
}

/// The Mock backends (unlike the demo, with no scenario thread — for the tour's reproducibility).
fn services() -> Services {
    let wifi = MockWifi::new();
    // It starts connected. The command sits in the channel and takes effect on the first `poll`.
    let _ = wifi.control().send(WifiMsg::State(WifiState::Connected {
        ssid: "fairing-lab".to_owned(),
        strength: 3,
    }));
    Services::builder()
        .clock(MockClock::running(WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 540,
        }))
        .power(MockPower::new(72, false))
        .wifi(wifi)
        .build()
}

/// Build the shell once the window exists (the `Waker`).
fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
    let theme = theme();
    let wallpaper = grid_wallpaper(&theme);
    let mut shell = fairing::Shell::builder(config())
        .theme(theme)
        .metrics_spec(metrics())
        .services(services())
        .status_bar_painter(status_bar_painter())
        .slot_painter(slot_painter)
        .build(ctx)?;
    shell.desktop_mut().set_wallpaper(wallpaper);
    add_screens(&mut shell);
    add_hopper_tile(&mut shell);
    add_jog_tile(&mut shell);
    Ok(shell)
}

// ─────────────────────────────────────────────────────────────────────────────
// The painters
// ─────────────────────────────────────────────────────────────────────────────

/// Draw the whole status bar. All the shell hands over is `bar.rect` (the layout), `bar.items()` (the
/// item list with the gate decisions made) and `bar.cx` (the backend snapshots, the theme, the
/// handle).
///
/// The clock string is overwritten in a buffer the closure holds, through
/// [`WallTime::format_into`](fairing::services::WallTime::format_into), so no heap allocation is made
/// per frame (the same method as the built-in status bar).
fn status_bar_painter() -> impl FnMut(&mut egui::Ui, &mut BarCx<'_>) {
    let mut clock = String::new();
    move |ui: &mut egui::Ui, bar: &mut BarCx<'_>| {
        let theme = bar.cx.theme;
        let painter = ui.painter().clone();
        painter.rect_filled(bar.rect, 0.0, theme.color(ColorRole::Surface));
        // An accent line along the bottom — our device's own marking, which the built-in status bar has not.
        painter.hline(
            bar.rect.x_range(),
            bar.rect.max.y - 1.0,
            Stroke::new(2.0, theme.color(ColorRole::Primary)),
        );
        let pad = theme.metrics.status_edge_pad;
        let font = egui::FontId::proportional(16.0);
        painter.text(
            egui::pos2(bar.rect.min.x + pad, bar.rect.center().y),
            egui::Align2::LEFT_CENTER,
            DEVICE,
            font.clone(),
            theme.color(ColorRole::OnSurface),
        );
        // Stacked backwards from the right. Where an item fails its gate, the shell says so with `live() == false`.
        let mut x = bar.rect.max.x - pad;
        if bar.item("status.wifi").is_some_and(|item| item.live()) {
            let level = bar.cx.services.wifi.strength().min(4);
            let text = if bar.cx.services.wifi.enabled() {
                format!("Wi-Fi {level}/4")
            } else {
                "Wi-Fi off".to_owned()
            };
            let galley = painter.layout_no_wrap(text, font.clone(), theme.color(ColorRole::Muted));
            x -= galley.size().x;
            painter.galley(
                egui::pos2(x, bar.rect.center().y - galley.size().y / 2.0),
                galley,
                theme.color(ColorRole::Muted),
            );
            x -= pad;
        }
        if bar.item("status.clock").is_some_and(|item| item.live()) {
            clock.clear();
            bar.cx
                .services
                .clock
                .now()
                .format_into(&mut clock, ClockFormat::Hm);
            let galley =
                painter.layout_no_wrap(clock.clone(), font, theme.color(ColorRole::Primary));
            x -= galley.size().x;
            painter.galley(
                egui::pos2(x, bar.rect.center().y - galley.size().y / 2.0),
                galley,
                theme.color(ColorRole::Primary),
            );
        }
    }
}

/// One desktop cell: the icon drawn as a round badge. What the shell hands over is the cell Rect, the
/// declaration, the press scale and whether the gate passed — the hit test and the press decision are
/// already made.
// Taking it by value is the `SlotPainter` contract (`SlotCx` carries a `&mut IconSet`).
#[allow(clippy::needless_pass_by_value)]
fn slot_painter(ui: &mut egui::Ui, slot: SlotCx<'_>) {
    let theme = slot.theme;
    let painter = ui.painter().clone();
    let center = slot.pressed_icon.center();
    let radius = slot.pressed_icon.width() * 0.62;
    let fill = if slot.pressed {
        theme.color(ColorRole::SurfaceVariant)
    } else {
        theme.color(ColorRole::Surface)
    };
    let ring = if slot.allowed {
        theme.color(ColorRole::Primary)
    } else {
        theme.color(ColorRole::Muted)
    };
    painter.circle_filled(center, radius, fill);
    painter.circle_stroke(center, radius, Stroke::new(2.0, ring));
    let inner = Rect::from_center_size(center, egui::Vec2::splat(radius));
    let style = IconStyle::sized(inner.width())
        .color(IconColor::Role(ColorRole::OnSurface))
        .enabled(slot.allowed);
    let _ = slot
        .icons
        .paint(&painter, inner, &slot.slot.icon, &style, theme);
    // The label goes below the badge, in the panel's language. (On a real device the galley would be cached as the built-in rendering does — the example keeps it simple.)
    painter.text(
        egui::pos2(center.x, slot.cell.max.y - 14.0),
        egui::Align2::CENTER_CENTER,
        slot.strings.get(&slot.slot.label),
        egui::FontId::proportional(14.0),
        if slot.allowed {
            theme.color(ColorRole::OnSurface)
        } else {
            theme.color(ColorRole::Muted)
        },
    );
}

/// A procedural grid wallpaper (`Wallpaper::Painter`). The colours are taken from the theme
/// beforehand and captured — the callback does not receive the theme.
fn grid_wallpaper(theme: &Theme) -> Wallpaper {
    let background = theme.color(ColorRole::Background);
    let line = theme.color(ColorRole::SurfaceVariant);
    Wallpaper::Painter(Box::new(move |painter: &egui::Painter, rect: Rect| {
        painter.rect_filled(rect, 0.0, background);
        let step = 44.0;
        let stroke = Stroke::new(1.0, line);
        let mut x = rect.min.x;
        while x <= rect.max.x {
            painter.vline(x, rect.y_range(), stroke);
            x += step;
        }
        let mut y = rect.min.y;
        while y <= rect.max.y {
            painter.hline(rect.x_range(), y, stroke);
            y += step;
        }
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Slotting **something the crate knows nothing of** into the quick settings
// ─────────────────────────────────────────────────────────────────────────────

/// A hopper's name · config key · colour · initial level (%). A concept only this device has, and the crate knows nothing of it.
const HOPPERS: [(&str, &str, ColorRole, i64); 3] = [
    ("Resin A", "hopper.a", ColorRole::Primary, 72),
    ("Resin B", "hopper.b", ColorRole::Warning, 41),
    // The solvent is a value a sensor reads, so it is not operated — it is drawn dimmed and takes no drag.
    ("Solvent", "hopper.c", ColorRole::Success, 88),
];

/// The row to leave read-only (a sensor value).
const READ_ONLY: &str = "hopper.c";

/// The tile that opens three gauge rows — **only declared; the crate does the drawing.**
///
/// It fits none of the built-in kinds (toggle, slider, action, display), but the crate settles the
/// shape it is written in: per row a config key · a name · a colour · a unit, and for the whole row
/// the name column's width. The finger height, the presses, the track thickness and the value display
/// come from the same conventions as the built-in slider.
fn add_hopper_tile(shell: &mut fairing::Shell) {
    shell.add(
        tile(
            "tile.hopper",
            TileKind::Gauges {
                rows: HOPPERS
                    .iter()
                    .map(|(name, key, color, _)| {
                        Gauge::new(*key, *name)
                            .color(*color)
                            .read_only(*key == READ_ONLY)
                    })
                    .collect(),
                label_width: 96.0,
            },
        )
        .label("Hoppers")
        .icon(icon::GAUGE),
    );
    for (_, key, _, level) in HOPPERS {
        shell.set_setting(
            fairing::settings::SettingKey::from(key),
            SettingValue::Int(level),
        );
    }
}

/// Something that fits no standard shape **takes a whole row** — here, an axis jog pad. The crate
/// gives only the tile's shape and the open/close transition, and does not touch the inside of the row.
fn add_jog_tile(shell: &mut fairing::Shell) {
    // The tile body is `FnMut`, so the pad's position is a plain `mut` capture.
    let mut at = egui::vec2(0.0, 0.0);
    shell.add(
        tile_panel("tile.jog", 132.0, move |ui, cx| {
            let (pad, resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width().min(320.0), 120.0),
                egui::Sense::click_and_drag(),
            );
            if let Some(pos) = resp.interact_pointer_pos() {
                let rel = (pos - pad.center()) / (pad.size() * 0.5);
                at = egui::vec2(rel.x.clamp(-1.0, 1.0), rel.y.clamp(-1.0, 1.0));
            }
            let painter = ui.painter();
            painter.rect_stroke(
                pad,
                8.0,
                Stroke::new(1.0, cx.theme.color(ColorRole::Outline)),
                egui::StrokeKind::Inside,
            );
            let knob = pad.center() + at * (pad.size() * 0.5 - egui::vec2(14.0, 14.0));
            painter.circle_filled(knob, 14.0, cx.theme.color(ColorRole::Primary));
        })
        .label("Jog")
        .icon(icon::SPLIT),
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// The screens — with no nav bar, back is a button inside the screen
// ─────────────────────────────────────────────────────────────────────────────

/// The back row at the top of a screen. The regular path on a device with the nav bar turned off
/// (no disabled items here: there is no bar at all and the screen
/// answers for it).
fn back_row(ui: &mut egui::Ui, cx: &mut Cx<'_>, title: &str) {
    ui.horizontal(|ui| {
        if ui.button("<  Back").clicked() {
            cx.finish();
        }
        ui.add_space(12.0);
        ui.heading(title);
    });
    ui.separator();
}

/// The four desktop icons (no dock).
fn add_screens(shell: &mut fairing::Shell) {
    shell.add(
        screen("dashboard", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            back_row(ui, cx, "Dashboard");
            if let Some(battery) = cx.services.power.battery() {
                ui.label(format!("battery {} %", battery.percent));
            }
            ui.label(format!("wifi strength {}", cx.services.wifi.strength()));
            if ui.button("open valve screen").clicked() {
                cx.open("valve");
            }
        })
        .title("Dashboard")
        .icon(icon::GAUGE)
        .desktop(),
    );
    shell.add(
        screen("valve", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            back_row(ui, cx, "Valve");
            // A screen stacked on top — the back button runs the A3 pop.
            ui.label("pushed on the stack; the Back button pops it (A3)");
        })
        .title("Valve")
        .icon(icon::WRENCH)
        .desktop(),
    );
    shell.add(
        screen("log", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            back_row(ui, cx, "Log");
            for line in 1..=6 {
                ui.label(format!("{line:02}:00  ok"));
            }
        })
        .title("Log")
        .icon(icon::CHART)
        .desktop(),
    );
    shell.add(
        screen("about", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            back_row(ui, cx, "About");
            ui.label(format!("{DEVICE} - fairing {}", fairing::VERSION));
            // The status bar, the cells and the wallpaper are all integrator painters.
            ui.label("status bar, slots and wallpaper are all integrator painters");
        })
        .title("About")
        .icon(icon::INFO)
        .desktop(),
    );
}
