//! The headless frame-time bench. `cargo run -p fairing --release --example bench`.
//!
//! It runs the Mock services plus synthetic input scenarios and prints the p50/p95 frame times. The
//! scenarios are the M1 scope: idle desktop · the icon zoom (A2 open/close
//! repeated over 240 frames) · push/pop (A3) repeated · a 200-widget screen (idle plus a touch drag
//! scroll) · home (an A2 close), plus M2's shade drag over 200 frames (A1) · the page swipe (A4, 12
//! icons = 4 × 2 over two pages) · the OSK appearing plus key taps (A5, feature `osk`) — and
//! **`layout` with the built-in settings screens**. On CI (x86) it is not the absolute figures that
//! matter but a regression against the baseline — and the baseline is `--release`.
//!
//! # Why the `layout` and settings scenarios were added
//!
//! `200-widget screen` is 200 rows of `ui.label` plus `ui.button` stacked up, so it measures the cost
//! of **egui's primitive widgets**. But a screen written with this crate does not look like that —
//! neither the eleven built-in settings screens nor the rewritten `demo` display screen; they are all
//! [`fairing::layout`]'s cards, rows and grids. None of that path was in the bench, so fixing `Grid`
//! or `group` moved no numbers.
//!
//! So two were added.
//!
//! - **A `layout` screen** — `page` + `section` × 5 + `group` (20 rows) + `Grid` (12 cells). Idle and
//!   a drag scroll are measured separately. The per-frame cost of `Grid`'s column arithmetic and the
//!   card backgrounds shows up here.
//! - **The built-in settings screens** — `settings.home` (the left/right split) · `settings.wifi` (a
//!   switch plus a list) · scrolling `settings.about` (the open-source notices — the longest of the
//!   built-in screens). They are screens an integrator attaches in one line, so the cost here is the
//!   cost the integrator pays.
//!
//! **Heap allocations per frame** are not counted here: a counting allocator needs `unsafe impl
//! GlobalAlloc`, and the workspace lint is `unsafe_code = "forbid"`, which the examples inherit.
//! Instead, idle desktop's p50 is the proxy for an allocation regression (more allocations move the
//! p50 first). The known allocations on the render path: one `Vec<Pos2>` per icon subpath of three
//! or more points (epaint's `PathShape` wants an owned `Vec`), and one each for the parametric
//! bluetooth and volume icons.

#![allow(clippy::print_stdout)] // stdout is right for bench results (a library only logs).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // Percentile and pixel arithmetic.

use fairing::services::mock;
use fairing::testing::{single_level_access, Harness};
use fairing::{icon, screen, Cx, LaunchAction, Lifecycle};
use std::time::Instant;

/// The icons put on the desktop (id, icon). `widgets` goes only in the dock, so the grid has 11 plus
/// `form` = 12 — two pages on a 4 × 2 grid.
const ICONS: [(&str, fairing::IconRef); 12] = [
    ("widgets", icon::GAUGE),
    ("camera", icon::CAMERA),
    ("chart", icon::CHART),
    ("cpu", icon::CPU),
    ("thermo", icon::THERMOMETER),
    ("wrench", icon::WRENCH),
    ("folder", icon::FOLDER),
    ("display", icon::DISPLAY),
    ("keyboard", icon::KEYBOARD),
    ("language", icon::LANGUAGE),
    ("activity", icon::ACTIVITY),
    ("shield", icon::SHIELD),
];

fn percentile(samples: &mut [f64], p: f64) -> f64 {
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((samples.len() as f64 - 1.0) * p).round() as usize;
    samples.get(index).copied().unwrap_or(0.0)
}

fn measure(
    name: &str,
    harness: &mut Harness,
    frames: usize,
    mut step: impl FnMut(&mut Harness, usize),
) {
    let mut samples = Vec::with_capacity(frames);
    let mut animated = 0usize;
    for i in 0..frames {
        step(harness, i);
        let start = Instant::now();
        harness.frame();
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        animated += usize::from(harness.shell.is_animating());
    }
    println!(
        "{name:<32} frames {frames:>4}  p50 {:6.3} ms  p95 {:6.3} ms  max {:6.3} ms  animating {animated:>3}",
        percentile(&mut samples, 0.5),
        percentile(&mut samples, 0.95),
        percentile(&mut samples, 1.0),
    );
}

/// The `layout` display screen — **the representative shape** of a screen written with this crate.
///
/// Five card groups (20 rows) plus a 12-cell grid. Unlike the 200-widget screen, the backgrounds,
/// corners, role colours and touch targets all go through tokens, so fixing a token moves these
/// numbers first.
fn layout_screen(ui: &mut egui::Ui, cx: &mut Cx<'_>, state: &std::cell::Cell<(bool, f32)>) {
    let (mut on, mut value) = state.get();
    fairing::layout::page(ui, cx, "bench.layout", |ui, cx| {
        fairing::layout::title(ui, cx, "Layout bench");
        fairing::layout::status_card(ui, cx, "Connected", Some("Wi-Fi 4/4"), None);
        for g in 0..5 {
            fairing::layout::section(ui, cx, "Group");
            fairing::layout::group(ui, cx, |ui, cx| {
                let _ =
                    fairing::layout::switch_row(ui, cx, "Toggle", Some("Subtitle"), &mut on, true);
                let _ = fairing::layout::slider_row(
                    ui,
                    cx,
                    "Values",
                    &mut value,
                    0.0..=100.0,
                    " %",
                    true,
                );
                let _ = fairing::layout::info_row(ui, cx, "Read only", "Values");
                let _ = fairing::layout::nav_row(ui, cx, "Enter", Some("Right"));
            });
            if g == 0 {
                fairing::layout::note(
                    ui,
                    cx,
                    "One line of caption under the card. It wraps to two when narrow.",
                );
            }
        }
        fairing::layout::section(ui, cx, "Grid");
        fairing::layout::Grid::new(2.0, 2.0)
            .deco(fairing::layout::Deco::new().visual_inset(3.0))
            .show(ui, cx, &GRID_ITEMS, |ui, cx, cell| {
                ui.painter().text(
                    cell.visual.center(),
                    egui::Align2::CENTER_CENTER,
                    cell.item,
                    egui::FontId::proportional(cx.theme.metrics.row_height * 0.24),
                    cx.theme.color(fairing::ColorRole::Muted),
                );
            });
    });
    state.set((on, value));
}

/// The grid's twelve cells.
const GRID_ITEMS: [&str; 12] = [
    "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten", "Eleven",
    "Twelve",
];

/// A point `fx` across and `fy` down the harness's screen — the scenarios are written as shares
/// of the glass, so a different `--size` drives the same gestures.
fn at(h: &Harness, fx: f32, fy: f32) -> egui::Pos2 {
    let r = h.screen_rect();
    r.min + egui::vec2(fx * r.width(), fy * r.height())
}

/// The `layout` screen: idle plus a touch drag scroll.
fn run_layout(harness: &mut Harness) {
    harness.shell.handle().launch(LaunchAction::open("layout"));
    harness.frames(20);
    measure("layout screen idle", harness, 60, |_, _| {});
    measure("layout drag scroll", harness, 60, |h, i| match i {
        0 => h.press(at(h, 0.5, 0.83)),
        59 => h.release(at(h, 0.5, 0.05)),
        _ => h.move_to(at(h, 0.5, 0.83 - 0.013 * i as f32)),
    });
    harness.shell.handle().home();
    harness.frames(20);
}

/// The built-in settings screens (feature `settings`). They are screens an integrator attaches
/// with one line of `add_all`, so the cost here is the cost the integrator pays.
#[cfg(feature = "settings")]
fn run_settings(harness: &mut Harness) {
    // `settings.home` splits left and right at a width of 1024 (`split_width`) — the list and the body
    // are both drawn in one frame.
    measure("settings.home idle", harness, 60, |h, i| {
        if i == 0 {
            h.shell.handle().launch(LaunchAction::open("settings.home"));
        }
    });
    measure("settings.wifi idle", harness, 60, |h, i| {
        if i == 0 {
            h.shell.handle().launch(LaunchAction::open("settings.wifi"));
        }
    });
    // `settings.about` carries the open-source notices and is the longest of the built-in screens — the scrolling cost is here.
    harness
        .shell
        .handle()
        .launch(LaunchAction::open("settings.about"));
    harness.frames(20);
    measure("settings.about drag scroll", harness, 60, |h, i| match i {
        0 => h.press(at(h, 0.68, 0.83)),
        59 => h.release(at(h, 0.68, 0.05)),
        _ => h.move_to(at(h, 0.68, 0.83 - 0.013 * i as f32)),
    });
    harness.shell.handle().home();
    harness.frames(30);
}

/// With feature `settings` off the screens do not exist at all.
#[cfg(not(feature = "settings"))]
fn run_settings(_harness: &mut Harness) {}

/// The M2 scenarios: the shade drag over 200 frames (A1) · the page swipe (A4).
fn run_m2(harness: &mut Harness) {
    // M2: the shade drag over 200 frames — pulled down from the top edge over 100 frames, released
    // (the spring), then pushed back up to close. A per-frame allocation regression shows in the p50.
    measure(
        "shade drag (A1, 200 frames)",
        harness,
        200,
        |h, i| match i {
            0 => h.press(at(h, 0.01, 0.013)),
            1..=99 => h.move_to(at(h, 0.01, 0.013 + 0.0067 * i as f32)),
            100 => h.release(at(h, 0.01, 0.68)),
            130 => h.press(at(h, 0.01, 0.67)),
            131..=169 => h.move_to(at(h, 0.01, 0.67 - 0.0167 * (i - 130) as f32)),
            170 => h.release(at(h, 0.01, 0.013)),
            _ => {}
        },
    );
    harness.frames(30);
    // M2: the page swipe (A4) — a horizontal fling over the grid, there and back.
    measure("page swipe (A4)", harness, 120, |h, i| match i % 60 {
        0 => h.press(at(h, 0.68, 0.5)),
        1..=10 => h.move_to(at(h, 0.68 - 0.03 * (i % 60) as f32, 0.5)),
        11 => h.release(at(h, 0.39, 0.5)),
        30 => h.press(at(h, 0.29, 0.5)),
        31..=40 => h.move_to(at(h, 0.29 + 0.03 * (i % 60 - 30) as f32, 0.5)),
        41 => h.release(at(h, 0.59, 0.5)),
        _ => {}
    });
    harness.frames(30);
    run_osk(harness);
}

/// M2: the OSK appearing (A5) plus key taps — the `form` screen has a field take focus on the frame it
/// opens, so the OSK comes up by itself (the 180 ms show tween plus rendering the key panel). After
/// that `key_rect("q")` is tapped every 3 frames (press → release → the injection taking effect). With
/// feature `osk` off the scenario drops out too.
#[cfg(feature = "osk")]
fn run_osk(harness: &mut Harness) {
    measure("osk show (A5)", harness, 60, |h, i| {
        if i == 0 {
            h.shell.handle().launch(LaunchAction::open("form"));
        }
    });
    measure("osk key taps (A5, 60 frames)", harness, 60, |h, i| {
        let key = h.shell.osk().key_rect("q");
        match (i % 3, key) {
            (0, Some(rect)) => h.press(rect.center()),
            (1, Some(rect)) => h.release(rect.center()),
            _ => {}
        }
    });
    harness.shell.handle().back(); // close the OSK
    harness.frames(15);
    harness.shell.handle().back(); // home
    harness.frames(30);
}

#[cfg(not(feature = "osk"))]
fn run_osk(_harness: &mut Harness) {}

/// Register every screen the bench uses. It is split out of `main` partly for clippy's function-length
/// cap, but mostly because **separating the scenarios from the stage reads better**.
fn add_bench_screens(harness: &mut Harness) {
    let mut text = String::new();
    harness.shell.add(
        screen("form", move |ui: &mut egui::Ui, cx: &mut Cx| {
            let field = ui.add_sized([300.0, 48.0], egui::TextEdit::singleline(&mut text));
            if cx.event == Some(Lifecycle::Created) {
                field.request_focus();
            }
        })
        .title("form")
        .icon(icon::KEYBOARD)
        .desktop(),
    );
    for (id, icon) in ICONS {
        if id == "widgets" {
            continue;
        }
        harness.shell.add(
            screen(id, |ui: &mut egui::Ui, _cx: &mut Cx| {
                ui.heading("screen");
            })
            .title(id)
            .icon(icon)
            .desktop(),
        );
    }
    harness.shell.add(
        screen("widgets", |ui: &mut egui::Ui, _cx: &mut Cx| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for i in 0..200 {
                    ui.horizontal(|ui| {
                        ui.label(format!("row {i}"));
                        let _ = ui.button("button");
                    });
                }
            });
        })
        .title("200 widgets")
        .icon(icon::GAUGE)
        .desktop()
        .dock(),
    );
    let layout_state = std::rc::Rc::new(std::cell::Cell::new((false, 40.0_f32)));
    harness.shell.add(
        screen("layout", move |ui: &mut egui::Ui, cx: &mut Cx| {
            layout_screen(ui, cx, &layout_state);
        })
        .title("layout")
        .icon(icon::LIST),
    );
    // The eleven built-in settings screens. No desktop icons are attached — the 12-cell grid
    // (two pages) is the A4 scenario's premise, and adding to it here would move the page-swipe
    // numbers with it.
    #[cfg(feature = "settings")]
    fairing::settings::add_all(
        &mut harness.shell,
        &fairing::settings::SettingsConfig::default().without_home_icon(),
    );
    harness.shell.add(
        screen("child", |ui: &mut egui::Ui, cx: &mut Cx| {
            ui.heading("child");
            if ui.button("back").clicked() {
                cx.finish();
            }
        })
        .title("child"),
    );
}

fn main() -> fairing::Result<()> {
    // Really run the animations (`test_shell` forces reduce = true, so it is not used).
    let mut config = single_level_access();
    config.desktop.rows = 2; // 4 × 2 = 8 cells/page → 12 icons make two pages (the A4 scenario)
    let mut harness = Harness::new(config, mock::services())?;
    add_bench_screens(&mut harness);
    harness.frames(3);
    println!(
        "fairing bench - {} icons, 1024x600, 60 Hz virtual time, {} build",
        ICONS.len(),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    measure("idle desktop", &mut harness, 120, |_, _| {});
    // The icon zoom: open (240 ms) and home (200 ms) alternating every 30 frames → 4 round trips.
    measure(
        "icon zoom (A2 open/close x4)",
        &mut harness,
        240,
        |h, i| match i % 60 {
            0 => h.shell.handle().launch(LaunchAction::open("widgets")),
            30 => h.shell.handle().home(),
            _ => {}
        },
    );
    // push/pop: within a task, push (220 ms) / back (200 ms) every 20 frames → 5 pushes · 4 pops.
    measure("push/pop (A3 x5)", &mut harness, 200, |h, i| {
        if i == 0 {
            h.shell.handle().launch(LaunchAction::open("widgets"));
        } else if i % 40 == 20 {
            h.shell.handle().launch(LaunchAction::open("child"));
        } else if i % 40 == 0 {
            h.shell.handle().back();
        }
    });
    harness.shell.handle().back();
    harness.frames(20);
    measure("200-widget screen idle", &mut harness, 60, |_, _| {});
    // A touch-style drag scroll: press → 8 px up every frame → release.
    measure("200-widget drag scroll", &mut harness, 60, |h, i| match i {
        0 => h.press(egui::pos2(512.0, 500.0)),
        59 => h.release(egui::pos2(512.0, 28.0)),
        _ => h.move_to(egui::pos2(512.0, 500.0 - 8.0 * i as f32)),
    });
    measure("home (A2 close)", &mut harness, 20, |h, i| {
        if i == 0 {
            h.shell.handle().home();
        }
    });
    run_m2(&mut harness);
    run_layout(&mut harness);
    run_settings(&mut harness);
    measure("idle desktop (after)", &mut harness, 60, |_, _| {});
    Ok(())
}
