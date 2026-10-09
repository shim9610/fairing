//! The desktop demo. `cargo run -p fairing --features runner-x11 --example demo -- --size=1024x600`.
//!
//! The Mock backends plus a few screens. It checks by eye the icon → screen → back/home round trip,
//! the status bar (the clock, Mock Wi-Fi, the battery draining), the nav bar and an integrator
//! `status_item`. The Wi-Fi strength is changed by **a scenario thread** through a `MockControl`
//! channel, which wakes the UI with the `Waker` — `thread::spawn` and `sleep` are used in the
//! example files only (`xtask sync-check` blocks them in `crates/fairing/src`). The logs go through a
//! 20-line stderr logger (no `env_logger`).
//!
//! # Tour mode (`--tour <dir>`, or `FAIRING_TOUR_DIR`)
//!
//! It makes its screenshots by itself — one per file named in [`PLAN`]. It sets the state up through the shell API
//! (`Shell::launch` / `back` / `home`) and **a synthetic finger** (`common::SyntheticInput` — it puts
//! pointer events into the next pass's `RawInput`, as the headless harness does), sends an
//! `egui::ViewportCommand::Screenshot` at the appointed moments, and writes the `ColorImage` that
//! comes back through `egui::Event::Screenshot` out as a PNG. A mid-transition frame is captured by
//! reading the transition's progress `t` every frame and taking **the frame closest** to the target,
//! and a mid-gesture frame (the shade pull, the page swipe, back) is captured with the finger still
//! held (`motion.reduce` has to be off). After the last frame, a `ViewportCommand::Close`.
//!
//! The M1 frames 01–08 (the desktop, A2, A3, the parametric icons) plus the M2 frames:
//! `09-shade-mid` (a top pull at y ≈ H/2: the scrim, the tiles, the notification list) ·
//! `10-shade-open` · `10d-tile-long-press` (a tile pressed and held, going to its settings) ·
//! `10b-shade-slider` (a Slider tile expanded — the icon comes down from the tile to the left of the
//! row) · `10c-shade-swipe` (a notification row being pushed left — the space revealed marks it as
//! being thrown away) · `11-osk` (the `TextEdit` screen on the English qwerty, with the inset
//! applied) · `11a-osk-compose` (switched to dubeolsik, mid-composition — `ㅎ`+`ㅏ` with the active
//! underline) · `11b-osk-latin` (back on the qwerty, the syllable left in the field) ·
//! `12-toast` (two toasts plus a heads-up) · `13-page-swipe-mid` (a two-page desktop) ·
//! `14-back-gesture-mid` (the left-edge back gesture).
//!
//! With `--nav=gesture` the nav bar is the gesture style and the tour is
//! [`GESTURE_PLAN`] instead: `36-gesture-bar` (the home indicator) · `36a-lift` (the screen
//! following a finger up from the bottom edge) · `36b-lift-held` / `36c-overview` (a pause there:
//! the recent screens) · `36d-lift-home` (let go into home) · `36e-switch` / `36f-switched` (a
//! slide along the indicator to the task used before).
//!
//! The tour driver and the std-only PNG encoder are in `examples/common/` (shared with
//! `custom_chrome.rs`). What is left here is the script ([`PLAN`]) and the tour's shell
//! ([`build_tour`]).
//!
//! ```text
//! xvfb-run -a -s "-screen 0 1024x600x24" env LIBGL_ALWAYS_SOFTWARE=1 WINIT_UNIX_BACKEND=x11 \
//!   cargo run -p fairing --features runner-x11 --example demo -- --tour target/tour
//! ```

mod common;

use common::{Act, Expect, Side, Spot};
use fairing::icons::parametric::{self, BtIconState};
use fairing::icons::IconStyle;
use fairing::layout::{self, ExpandableRow};
use fairing::notify::Level;
use fairing::runner::{self, Options};
use fairing::services::mock::{
    MockBluetooth, MockClock, MockDisplay, MockPower, MockWifi, WifiMsg,
};
use fairing::services::{Services, WallTime, WifiState};
use fairing::widgets::{
    BadgeTone, BadgeValue, BigButton, ButtonKind, Checkbox, CountBadge, Dropdown, FieldLook,
    ListRow, Opener, SegmentedControl, TextField, Trigger, WheelPicker,
};
use fairing::{
    action, icon, screen, screen_with, status_item, ColorRole, Cx, IconRef, Lifecycle,
    Notification, NotificationId, Screen, ShellConfig, Slot,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// The default window size. The tour pins it to this size.
const SIZE: (f32, f32) = (1024.0, 600.0);

/// The session's subject as one human-readable line. `{:?}` spits out `Subject { id: None, level:
/// Level(0), … }` as it stands, which is no good on a screen.
///
/// **The level names come from [`Cx::levels`]** — writing `demo.toml`'s `[access] levels` out again
/// here would split the config from the code, and then the screen would show the wrong level.
fn subject_line(cx: &Cx<'_>) -> String {
    let s = &cx.session.subject;
    let level = cx
        .levels()
        .get(s.level)
        .map_or("unknown", |def| def.label.as_str());
    match &s.id {
        Some(id) => format!("{id} · {level}"),
        None => format!("anonymous - {level}"),
    }
}

/// How much of a display cell the icon takes.
const ICON_TILE: f32 = 0.42;
/// The icon name's text size (relative to the row height).
const ICON_LABEL: f32 = 0.22;

/// The pinned bottom bar's height (as a multiple of the row height). The least one button sits comfortably in.
const BAR_ROWS: f32 = 1.6;

/// **The progress showcase** — a stateful screen (`screen_with`) whose state is a handful of
/// simulated workers reporting at their own paces, and one switch that silences them all.
///
/// Every value is a function of the shell's clock, so nothing here spawns a thread: a worker is a
/// waveform. The switch stops the clock the waveforms read, which is exactly what a worker going
/// quiet looks like to the widgets — their values stop changing — so the bars' stall report can
/// be watched going off one by one, and clearing again when the clock resumes.
struct ProgressDemo {
    /// The workers are reporting. Off, every value holds and the bars go stale in turn.
    reporting: bool,
    /// The clock reading at which the workers went quiet.
    quiet_since: Option<f64>,
    /// How long the workers have been quiet in total, taken off the clock so a resumed worker
    /// carries on from where it stopped rather than jumping ahead.
    quiet_total: f64,
    /// Keep-alive messages sent, for the bar that stays alive while its value stands still.
    beats: u64,
}

impl ProgressDemo {
    fn new() -> Self {
        Self {
            reporting: true,
            quiet_since: None,
            quiet_total: 0.0,
            beats: 0,
        }
    }

    /// The workers' clock: the shell's, less the time they spent quiet, frozen while they are.
    fn clock(&self, now: f64) -> f64 {
        self.quiet_since.unwrap_or(now) - self.quiet_total
    }
}

/// A sawtooth 0 → 1 over `period` seconds: a job that runs and restarts.
#[expect(clippy::cast_possible_truncation, reason = "a fraction of a period")]
fn saw(t: f64, period: f64) -> f32 {
    (t / period).rem_euclid(1.0) as f32
}

/// A triangle 0 → 1 → 0 over `period` seconds: a level, not a job — it goes down again.
fn triangle(t: f64, period: f64) -> f32 {
    1.0 - (2.0 * saw(t, period) - 1.0).abs()
}

/// A worker that reports for six seconds of every ten and then falls silent at 70 %.
fn stalling(t: f64) -> f32 {
    #[expect(clippy::cast_possible_truncation, reason = "a fraction of a period")]
    let phase = t.rem_euclid(10.0) as f32;
    0.7 * (phase / 6.0).min(1.0)
}

fn percent(v: f32) -> String {
    format!("{:.0} %", v * 100.0)
}

/// A group whose free content is inset like a list row's text, so the captions line up with the
/// row titles above them and the bars stop short of the card's edge.
fn inset_group(ui: &mut egui::Ui, cx: &mut Cx<'_>, body: impl FnOnce(&mut egui::Ui, &mut Cx<'_>)) {
    layout::group_with(ui, cx, layout::Deco::new().pad_content(), body);
}

/// A caption over a bar that takes the whole width.
fn bar_row(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    caption: &str,
    bar: fairing::widgets::ProgressBar<'_>,
) {
    let m = &cx.theme.metrics;
    ui.label(
        egui::RichText::new(caption)
            .size(m.type_scale.small)
            .color(cx.theme.color(ColorRole::Muted)),
    );
    let _ = bar.show(ui, &mut cx.widgets());
    ui.add_space(cx.theme.control.gap);
}

impl Screen for ProgressDemo {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let now = ui.input(|i| i.time);
        let t = self.clock(now);
        if self.reporting {
            self.beats += 1;
        }
        // The values move with the clock, so the screen asks for the next frame itself.
        ui.ctx().request_repaint();
        let mut reporting = self.reporting;
        let beats = self.beats;

        layout::page(ui, cx, "progress", |ui, cx| {
            layout::title(ui, cx, "Progress");
            layout::note(
                ui,
                cx,
                "Simulated workers at their own paces. Every bar eases towards what it is told \
                 and holds its high-water mark unless a rewind is meant; the ones that report a \
                 stall are told what silence means.",
            );
            layout::group(ui, cx, |ui, cx| {
                let _ = layout::switch_row(
                    ui,
                    cx,
                    "Workers reporting",
                    Some(if reporting {
                        "Every value moves"
                    } else {
                        "Silent — the stall reports go off in turn"
                    }),
                    &mut reporting,
                    true,
                );
            });
            progress_paces(ui, cx, t);
            progress_arrival(ui, cx, t);
            progress_silence(ui, cx, t, beats);
            progress_looks(ui, cx, t);
            progress_rings(ui, cx, t);
            progress_meters(ui, cx, t);
        });

        if reporting != self.reporting {
            self.reporting = reporting;
            if reporting {
                self.quiet_total += now - self.quiet_since.take().unwrap_or(now);
            } else {
                self.quiet_since = Some(now);
            }
        }
    }
}

/// How long a worker may be quiet before its bar says so.
const STALE_AFTER: Duration = Duration::from_millis(1500);

/// Three jobs at three speeds.
fn progress_paces(ui: &mut egui::Ui, cx: &mut Cx<'_>, t: f64) {
    use fairing::widgets::ProgressBar;
    layout::section(ui, cx, "Three paces");
    inset_group(ui, cx, |ui, cx| {
        for (caption, period) in [
            ("Fast — a 4 s job", 4.0),
            ("Medium — 12 s", 12.0),
            ("Slow — 45 s", 45.0),
        ] {
            let v = saw(t, period);
            let text = percent(v);
            bar_row(
                ui,
                cx,
                caption,
                ProgressBar::determinate(v)
                    .allow_rewind(true)
                    .trailing(&text),
            );
        }
    });
}

/// The easing and the high-water mark: a bursty feed, and a level fed to two bars.
fn progress_arrival(ui: &mut egui::Ui, cx: &mut Cx<'_>, t: f64) {
    use fairing::widgets::ProgressBar;
    layout::section(ui, cx, "How the value arrives");
    inset_group(ui, cx, |ui, cx| {
        let bursty = saw(t.floor(), 12.0);
        let text = percent(bursty);
        bar_row(
            ui,
            cx,
            "Reported once a second — the bar eases between reports",
            ProgressBar::determinate(bursty)
                .allow_rewind(true)
                .trailing(&text),
        );
        let level = triangle(t, 16.0);
        let text = percent(level);
        bar_row(
            ui,
            cx,
            "A level that rises and falls — rewinds allowed",
            ProgressBar::determinate(level)
                .allow_rewind(true)
                .tone(ColorRole::Success)
                .trailing(&text),
        );
        bar_row(
            ui,
            cx,
            "The same feed with rewinds forbidden (the default) — it holds its high-water mark",
            ProgressBar::determinate(level)
                .tone(ColorRole::Success)
                .trailing(&text),
        );
    });
}

/// The stall report, with and without a heartbeat.
fn progress_silence(ui: &mut egui::Ui, cx: &mut Cx<'_>, t: f64, beats: u64) {
    use fairing::widgets::ProgressBar;
    layout::section(ui, cx, "Silence");
    inset_group(ui, cx, |ui, cx| {
        let v = stalling(t);
        let text = percent(v);
        bar_row(
            ui,
            cx,
            "Reports for 6 s of every 10, then nothing — stale after 1.5 s",
            ProgressBar::determinate(v)
                .allow_rewind(true)
                .stale_after(STALE_AFTER)
                .trailing(&text),
        );
        bar_row(
            ui,
            cx,
            "The same worker sending a heartbeat while it thinks — never stale",
            ProgressBar::determinate(v)
                .allow_rewind(true)
                .stale_after(STALE_AFTER)
                .heartbeat(beats)
                .trailing(&text),
        );
    });
}

/// The two looks, the indeterminate bar and the disabled one.
fn progress_looks(ui: &mut egui::Ui, cx: &mut Cx<'_>, t: f64) {
    use fairing::widgets::ProgressBar;
    layout::section(ui, cx, "Looks");
    inset_group(ui, cx, |ui, cx| {
        let v = saw(t, 12.0);
        let text = percent(v);
        bar_row(
            ui,
            cx,
            "Stop indicator — a gap at the head and a dot at the end",
            ProgressBar::determinate(v)
                .allow_rewind(true)
                .stop_indicator(true)
                .trailing(&text),
        );
        let cells = (saw(t, 14.0) * 7.0).floor();
        let text = format!("{cells:.0} / 7");
        bar_row(
            ui,
            cx,
            "Seven steps — one every two seconds",
            ProgressBar::determinate(cells / 7.0)
                .allow_rewind(true)
                .steps(7)
                .trailing(&text),
        );
        bar_row(
            ui,
            cx,
            "Indeterminate — the extent is not known",
            ProgressBar::indeterminate().trailing("…"),
        );
        bar_row(
            ui,
            cx,
            "Disabled",
            ProgressBar::determinate(0.6)
                .enabled(false)
                .trailing("60 %"),
        );
    });
}

/// The rings: both styles, a gauge, an indeterminate one and one that stalls.
fn progress_rings(ui: &mut egui::Ui, cx: &mut Cx<'_>, t: f64) {
    use fairing::widgets::{ProgressRing, RingStyle};
    layout::section(ui, cx, "Rings");
    inset_group(ui, cx, |ui, cx| {
        let gap = cx.theme.control.gap;
        // Five in one row, each as big as that allows (a pixel of slack so the fifth does not
        // wrap on a rounding) — and never smaller than a tile.
        let m = &cx.theme.metrics;
        let diameter = ((ui.available_width() - gap * 4.0) / 5.0 - 1.0)
            .floor()
            .clamp(m.tile_size, m.row_height * 2.4);
        let sweep = saw(t, 12.0);
        let flat = saw(t, 45.0);
        let gauge = triangle(t, 16.0);
        let quiet = stalling(t);
        let sweep_text = format!("{:.0}%", sweep * 100.0);
        let flat_text = format!("{:.0}%", flat * 100.0);
        let gauge_text = format!("{:.0}", gauge * 120.0);
        let quiet_text = format!("{:.0}%", quiet * 100.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let _ = ProgressRing::determinate(sweep)
                .allow_rewind(true)
                .value_text(&sweep_text)
                .label("Sweep")
                .diameter(diameter)
                .show(ui, &mut cx.widgets());
            let _ = ProgressRing::determinate(flat)
                .allow_rewind(true)
                .style(RingStyle::Flat)
                .value_text(&flat_text)
                .label("Flat")
                .diameter(diameter)
                .show(ui, &mut cx.widgets());
            let _ = ProgressRing::determinate(gauge)
                .allow_rewind(true)
                .gap_degrees(75.0)
                .tone(ColorRole::Success)
                .value_text(&gauge_text)
                .label("Gauge")
                .diameter(diameter)
                .show(ui, &mut cx.widgets());
            let _ = ProgressRing::indeterminate()
                .label("Working")
                .diameter(diameter)
                .show(ui, &mut cx.widgets());
            let _ = ProgressRing::determinate(quiet)
                .allow_rewind(true)
                .stale_after(STALE_AFTER)
                .value_text(&quiet_text)
                .label("Stalls")
                .diameter(diameter)
                .show(ui, &mut cx.widgets());
        });
    });
}

/// The meters — a measurement, not a progress: a temperature drifting across its band and past
/// its alarm, and a pressure held at a setpoint and reported once a second.
fn progress_meters(ui: &mut egui::Ui, cx: &mut Cx<'_>, t: f64) {
    use fairing::widgets::{LampState, Limit, Meter};
    layout::section(ui, cx, "Meters — a measurement, not a progress");
    inset_group(ui, cx, |ui, cx| {
        let temp = 30.0 * triangle(t, 40.0) + 45.0;
        let temp_text = format!("{temp:.1} °C");
        let temp_limits = [
            Limit::low(48.0, LampState::Fault).label("48"),
            Limit::high(67.0, LampState::Warn).label("67"),
            Limit::high(70.0, LampState::Fault).label("70"),
        ];
        meter_row(
            ui,
            cx,
            "Block temperature — drifting across its band and past the alarm; colour only for a verdict",
            Meter::new(temp, 40.0..=80.0)
                .normal(55.0..=65.0)
                .limits(&temp_limits)
                .stale_after(STALE_AFTER)
                .readout(&temp_text),
        );
        let pressure = 0.3 * triangle(t.floor(), 7.0) + 1.05;
        let pressure_text = format!("{pressure:.2} bar");
        let pressure_limits = [Limit::high(1.40, LampState::Fault).label("1.40")];
        meter_row(
            ui,
            cx,
            "Pressure — held at a setpoint, reported once a second; the deviation is a length",
            Meter::new(pressure, 0.8..=1.6)
                .normal(1.10..=1.30)
                .setpoint(1.20)
                .limits(&pressure_limits)
                .stale_after(STALE_AFTER)
                .readout(&pressure_text),
        );
    });
}

/// A caption over a meter that takes the whole width.
fn meter_row(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    caption: &str,
    meter: fairing::widgets::Meter<'_>,
) {
    let m = &cx.theme.metrics;
    ui.label(
        egui::RichText::new(caption)
            .size(m.type_scale.small)
            .color(cx.theme.color(ColorRole::Muted)),
    );
    let _ = meter.show(ui, &mut cx.widgets());
    ui.add_space(cx.theme.control.gap);
}

/// The Mock bundle: a running clock, a battery draining 3 % a minute (scheduling repaints through
/// `next_wake`), Wi-Fi, BT, and a display (brightness 70 — the `tile.brightness` slider · `keep_awake`).
fn services() -> (Services, fairing::services::mock::MockControl<WifiMsg>) {
    let mut power = MockPower::new(72, false);
    power.drain_per_min = 3;
    let wifi = MockWifi::new();
    let wifi_control = wifi.control();
    let services = Services::builder()
        .clock(MockClock::running(WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 540,
        }))
        .power(power)
        .wifi(wifi)
        .bluetooth(MockBluetooth::new())
        .display(MockDisplay::new(70))
        .build();
    (services, wifi_control)
}

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let size = common::arg_size(&args);
    let tour = common::arg_tour(&args);
    let options = Options {
        fullscreen: tour.is_none() && size.is_none(),
        title: "fairing demo".to_owned(),
        size: Some(size.unwrap_or(SIZE)),
    };
    // `--nav=gesture` tours the gesture navigation instead.
    let plan = if nav_arg().as_deref() == Some("gesture") {
        GESTURE_PLAN
    } else {
        PLAN
    };
    match tour {
        Some(dir) => common::run_tour(options, dir, plan, |ctx| Ok((build_tour(ctx)?, ()))),
        None => runner::run_shell(options, build),
    }
}

/// The demo's config. The headless regression test reads the same file (`tests/m1_headless.rs`).
const CONFIG: &str = include_str!("demo.toml");

/// The demo's default wallpaper — **the original artwork**.
///
/// The procedural Abyss (`--wallpaper=abyss`) is a code approximation of the same picture and is much
/// shallower in detail. The demo has to show how the brand actually looks, so the original is laid
/// down by default. It is baked into the executable, so no assets need loading onto the device.
///
/// **A different plate is prepared for each aspect ratio.** Stretching a landscape original onto a
/// portrait panel with `cover` crops the manta enormously and ruins the picture whole (measured).
/// Which plate to choose is **the integrator's judgement**, so it happens here rather than in the
/// crate — it is the problem guide 09 §2.1 handles as "how many plates do I need?".
/// **The light column is the palette's other half**. A texture follows nothing on its
/// own, so a shell switching to its light palette at dusk would keep the dark painting under a pale
/// UI. Only the landscape plate has a light twin so far; the rest pair with a flat background,
/// which is a pair like any other — a painting at night, a plain colour by day.
/// One row of [`WALLPAPERS`]: the aspect ratio's upper bound, the dark plate, and the light twin
/// where one exists.
fn default_wallpaper(ctx: &egui::Context) -> Option<fairing::desktop::Wallpaper> {
    if wallpaper_arg().is_some() {
        return None;
    }
    common::brand_wallpaper(ctx)
}

/// Build the shell once the window exists (the `Waker`), tying the config, the Mocks, the scenario thread and the declarations together.
fn build(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
    let mut config = ShellConfig::from_toml(CONFIG)?;
    apply_config_args(&mut config);
    let (services, wifi_control) = services();
    let mut builder = panel_scale(fairing::Shell::builder(config))
        .services(services)
        .fonts(common::korean_fonts())
        .image_loader(common::image_loader);
    if let Some(wallpaper) = default_wallpaper(ctx) {
        builder = builder.wallpaper(wallpaper);
    }
    let mut shell = builder.build(ctx)?;
    apply_legibility(&mut shell);

    // The scenario thread: it takes the Wi-Fi strength round 4 → 0 → 4 every 4 seconds. It sends
    // through the backend channel (`MockControl`) and wakes the UI with `Waker::wake` — no locks.
    let waker = shell.handle().waker().clone();
    std::thread::spawn(move || {
        let mut strength = 4u8;
        loop {
            std::thread::sleep(Duration::from_secs(4));
            strength = if strength == 0 { 4 } else { strength - 1 };
            let state = if strength == 0 {
                WifiState::Idle
            } else {
                WifiState::Connected {
                    ssid: "lab-ap".to_owned(),
                    strength,
                }
            };
            if !wifi_control.send(WifiMsg::State(state)) {
                break;
            }
            waker.wake();
        }
    });

    add_settings(&mut shell);
    add_screens(&mut shell);
    if status_labels() {
        apply_status_labels(&mut shell);
    }
    if let Some(placement) = dock_placement_arg() {
        log::info!("dock placement {placement:?}");
        shell.desktop_mut().set_dock_placement(placement);
    }
    add_hidden_entries(&mut shell);
    Ok(shell)
}

/// The label legibility correction (guide 09 §2.2).
///
/// On the original wallpaper the manta's **white belly passes through the middle of the icon grid** —
/// that part alone cannot keep §2.2's "dark and low-contrast" requirement. Rather than redrawing the
/// wallpaper, the integrator-side prescription is used.
///
/// **`veil` is the default here.** `shadow` brings the icons back too, but the "Widgets" label right
/// over the white belly was still weak (measured). White text on a white ground is not saved by one
/// layer of shadow. The veil flattens the picture but saves the whole grid for certain, and it suits
/// the deep-sea theme — `--legibility=shadow|none` lets you try the others.
///
/// It is turned off for flat and procedural wallpapers (`--wallpaper=background|abyss`), where it is
/// not needed — there is no reason to fog a well-authored background.
fn apply_legibility(shell: &mut fairing::Shell) {
    use fairing::desktop::LabelLegibility;
    let raster = wallpaper_arg().is_none_or(|name| name.starts_with("file:"));
    let mode = legibility_arg().unwrap_or(if raster {
        LabelLegibility::Veil
    } else {
        LabelLegibility::None
    });
    log::info!("label legibility {}", mode.as_str());
    shell.desktop_mut().set_legibility(mode);
}

/// `--legibility=none|shadow|veil` — the label legibility correction (guide 09 §2.2).
fn legibility_arg() -> Option<fairing::desktop::LabelLegibility> {
    let raw = std::env::args().find_map(|a| a.strip_prefix("--legibility=").map(str::to_owned))?;
    fairing::desktop::LabelLegibility::parse(&raw).or_else(|| {
        log::warn!("--legibility={raw} is not a known mode (none | shadow | veil)");
        None
    })
}

/// The built-in settings screens. **One line attaches all eleven.**
///
/// The demo's Mock backends offer Wi-Fi, Bluetooth, power, the clock and the display, so those five
/// screens are registered and network and accounts, having no backend, drop out by themselves.
///
/// The colours are untouched — the settings screens are all role colours, so `--preset=abyss` has
/// them follow the deep-sea palette as they are (guide 09 §8).
fn add_settings(shell: &mut fairing::Shell) {
    use fairing::settings::{add_all, SettingsConfig};
    // The desktop icons are turned off, since the demo lays out its own grid — only the screens are registered.
    add_all(shell, &SettingsConfig::default().without_home_icon());
}

/// Three hidden entry points. This is what a device's service menu looks like.
///
/// All three triggers are **the same trait**
/// ([`KnockTrigger`](fairing::access::KnockTrigger)) — the crate does not settle the method.
///
/// - **Taps** ([`TapKnock`](fairing::access::TapKnock)) — the model-name row on the Dashboard, seven
///   times. The Android way, and since the screen calls `cx.knock` from its own place, the shell does
///   not know where was pressed.
/// - **Coordinates** ([`ZoneKnock`](fairing::access::ZoneKnock)) — top left → top right → bottom right
///   → bottom left. It needs no widget and so works on a screen with all the chrome hidden. An
///   `admin` gate is put on it, so **even a completed knock goes through authentication**.
/// - **A key combination** (`KeyCombo`, implemented below) — F1 · F2 · F1. A trigger the crate does
///   not provide, written by the integrator in 40 lines. A device with physical keys on its front
///   looks like this.
fn add_hidden_entries(shell: &mut fairing::Shell) {
    use fairing::access::{Corner, HiddenEntry, TapKnock, ZoneKnock};
    use fairing::LaunchAction;
    shell.add(
        screen("service", |ui: &mut egui::Ui, cx: &mut Cx| {
            layout::action_bar(
                ui,
                cx,
                BAR_ROWS,
                |ui, cx| {
                    layout::title(ui, cx, "Service menu");
                    layout::status_card(
                        ui,
                        cx,
                        "Opened through a hidden entry point",
                        Some("A hidden entry point - customers never get this far"),
                        Some(ColorRole::Warning),
                    );
                    layout::section(ui, cx, "Session");
                    layout::group(ui, cx, |ui, cx| {
                        layout::info_row(ui, cx, "Subject", &subject_line(cx));
                    });
                    layout::note(
                        ui,
                        cx,
                        "Three ways in, all through the same trait: tap the model \
                         row on Dashboard seven times, tap the four corners in turn, \
                         or press F1 F2 F1. The crate does not pick the trigger for \
                         you.",
                    );
                },
                |ui, cx| {
                    if BigButton::new("Close")
                        .icon(icon::ARROW_LEFT)
                        .show(ui, &mut cx.widgets())
                        .clicked()
                    {
                        cx.finish();
                    }
                },
            );
        })
        .title("Service"),
    );
    shell.add_hidden_entry(
        HiddenEntry::new("service", TapKnock::new(7), LaunchAction::open("service")).hint_from(3),
    );
    shell.add_hidden_entry(
        HiddenEntry::new(
            "factory",
            ZoneKnock::corners([
                Corner::TopLeft,
                Corner::TopRight,
                Corner::BottomRight,
                Corner::BottomLeft,
            ]),
            LaunchAction::open("service"),
        )
        .gate("admin"),
    );
    shell.add_hidden_entry(
        HiddenEntry::new("keypad", KeyCombo::default(), LaunchAction::open("service"))
            .gate("admin"),
    );
}

/// A physical key combination on the device's front — **an example of an integrator writing a trigger
/// the crate does not provide.**
///
/// A [`KnockTrigger`](fairing::access::KnockTrigger) receives each frame's input every frame and holds
/// its state itself. It takes the time from `input.now` — calling `Instant::now()` would stop a test
/// pushing a time in.
#[derive(Debug, Default)]
struct KeyCombo {
    hit: usize,
}

impl KeyCombo {
    const WANT: [egui::Key; 3] = [egui::Key::F1, egui::Key::F2, egui::Key::F1];
}

impl fairing::access::KnockTrigger for KeyCombo {
    fn feed(&mut self, input: &fairing::access::KnockInput<'_>) -> fairing::access::KnockStep {
        use fairing::access::KnockStep;
        let mut step = KnockStep::Idle;
        for key in input.keys {
            if Self::WANT.get(self.hit) == Some(key) {
                self.hit += 1;
                step = KnockStep::Advanced;
                if self.hit == Self::WANT.len() {
                    self.hit = 0; // back to the beginning by itself, once open.
                    return KnockStep::Opened;
                }
            } else if self.hit != 0 {
                self.hit = 0;
                step = KnockStep::Reset;
            }
        }
        step
    }

    fn remaining(&self) -> Option<u8> {
        u8::try_from(Self::WANT.len() - self.hit).ok()
    }

    fn reset(&mut self) {
        self.hit = 0;
    }
}

/// The 12 desktop icons (11 screens plus 1 action, with 2 in the dock = 10 on the grid → two 4 × 2 pages) plus an integrator status item.
#[expect(
    clippy::too_many_lines,
    reason = "the demo is as long as the number of declarations"
)]
fn add_screens(shell: &mut fairing::Shell) {
    let opened = Rc::new(Cell::new(0u32));
    let opened_in = Rc::clone(&opened);
    shell.add(
        screen("dashboard", move |ui: &mut egui::Ui, cx: &mut Cx| {
            if cx.event == Some(fairing::Lifecycle::Created) {
                opened_in.set(opened_in.get() + 1);
            }
            layout::page(ui, cx, "dashboard", |ui, cx| {
                // The full header anatomy — a name, what the page is, and the page's own action on
                // the title's line. `layout::title` is this call with the last two left out.
                layout::header(ui, cx, "Dashboard", Some("Device overview"), |ui, cx| {
                    let _ = CountBadge::new(BadgeValue::Text("Demo"))
                        .tone(BadgeTone::Neutral)
                        .show(ui, &mut cx.widgets());
                });
                // On the render path, narrow accessors rather than cloning the snapshot (zero heap allocation).
                let (online, level) = (cx.services.wifi.enabled(), cx.services.wifi.strength());
                layout::status_card(
                    ui,
                    cx,
                    if online { "Connected" } else { "Offline" },
                    Some(&format!("Wi-Fi {level}/4 · fairing-lab")),
                    Some(if online {
                        ColorRole::Primary
                    } else {
                        ColorRole::Danger
                    }),
                );

                layout::section(ui, cx, "Device");
                // **A read-only row gets no chevron.** `layout::icon_row` is a pressable row and always
                // brings one — here a `ListRow` is used directly.
                layout::group(ui, cx, |ui, cx| {
                    let _ = ListRow::new("Wi-Fi")
                        .icon(icon::WIFI)
                        .icon_color(ColorRole::Primary)
                        .subtitle(if online { "fairing-lab" } else { "Off" })
                        .trailing(format!("{level}/4"))
                        .chevron(false)
                        .separator(false)
                        .show(ui, &mut cx.widgets());
                    if let Some(b) = cx.services.power.battery() {
                        let _ = ListRow::new("Battery")
                            .icon(icon::BATTERY)
                            .icon_color(ColorRole::Primary)
                            .subtitle(if b.charging {
                                "Charging"
                            } else {
                                "Discharging"
                            })
                            .trailing(format!("{} %", b.percent))
                            .chevron(false)
                            .separator(false)
                            .show(ui, &mut cx.widgets());
                    }
                });

                layout::section(ui, cx, "Shortcuts");
                layout::group(ui, cx, |ui, cx| {
                    if layout::nav_row(ui, cx, "Progress", Some("Stateful screen")).clicked() {
                        cx.open("progress");
                    }
                    if layout::nav_row(ui, cx, "Dropdowns", Some("Five triggers, four openers"))
                        .clicked()
                    {
                        cx.open("dropdowns");
                    }
                    if layout::nav_row(ui, cx, "Admin", Some("Gated")).clicked() {
                        cx.open("admin");
                    }
                });

                layout::section(ui, cx, "About");
                layout::group(ui, cx, |ui, cx| {
                    layout::info_row(
                        ui,
                        cx,
                        "Screen builds",
                        &format!("{} (resident, so the state survives)", opened_in.get()),
                    );
                    // The hidden entry point — Android's "the build number seven times" slot.
                    // The shell does not know where was pressed; the screen knocks from its own secret spot.
                    let model = ListRow::new("Model")
                        .trailing("FAIRING-DEMO / rev A")
                        .separator(false)
                        .show(ui, &mut cx.widgets());
                    if model.clicked() {
                        cx.knock("service");
                    }
                });
                if let Some(left) = cx.knock_remaining("service") {
                    // Like Android, it says so only near the end — before that it is quiet.
                    if (1..=3).contains(&left) {
                        layout::note(ui, cx, &format!("{left} more taps opens the service menu"));
                    }
                }
            });
        })
        .title("Dashboard")
        .description("Live readings at a glance: the backends, the clock and the panel.")
        .icon(icon::GAUGE)
        .desktop()
        .dock(),
    );
    shell.add(
        screen_with("progress", ProgressDemo::new)
            .title("Progress")
            .icon(icon::ACTIVITY)
            .desktop()
            .dock(),
    );
    shell.add(
        screen("admin", |ui: &mut egui::Ui, cx: &mut Cx| {
            layout::page(ui, cx, "admin", |ui, cx| {
                layout::title(ui, cx, "Admin");
                layout::status_card(
                    ui,
                    cx,
                    "Unlocked",
                    Some("maintainer-level gate"),
                    Some(ColorRole::Success),
                );
                layout::note(
                    ui,
                    cx,
                    "You came in through the shell's own prompt: the reference \
                     `PinTable` checked the PIN or the pattern from demo.toml. The unlock is \
                     temporary - in two minutes the session drops back to viewer \
                     and this screen closes by itself. The padlock in the status \
                     bar drops it now.",
                );
                layout::section(ui, cx, "Session");
                layout::group(ui, cx, |ui, cx| {
                    layout::info_row(ui, cx, "Subject", &subject_line(cx));
                });
            });
        })
        .title("Admin")
        .description("The session: who is at the panel and for how long the unlock holds.")
        .icon(icon::SHIELD)
        .desktop(),
    );
    shell.add(
        screen("fullscreen", |ui: &mut egui::Ui, cx: &mut Cx| {
            // With the chrome hidden whole, **the screen** provides the way back — the pinned bottom
            // bar is that place. A button tacked onto the end of a list has to be scrolled to, and on a
            // device that is another way of saying "there is no way back".
            layout::action_bar(
                ui,
                cx,
                BAR_ROWS,
                |ui, cx| {
                    layout::title(ui, cx, "Fullscreen");
                    layout::note(
                        ui,
                        cx,
                        "One `.fullscreen()` took the status bar and the nav bar \
                         away. For process views and video playback, where the screen \
                         is the whole point.",
                    );
                    layout::status_card(
                        ui,
                        cx,
                        "Chrome hidden",
                        Some("No status bar, no nav bar"),
                        None,
                    );
                },
                |ui, cx| {
                    if BigButton::new("Close")
                        .icon(icon::ARROW_LEFT)
                        .show(ui, &mut cx.widgets())
                        .clicked()
                    {
                        cx.finish();
                    }
                },
            );
        })
        .title("Fullscreen")
        .icon(icon::DISPLAY)
        .desktop()
        .fullscreen(),
    );
    // The integrator icon extension: an arbitrary drawing-callback icon (it really walks the `register_icon_painter` path).
    let ring = shell.register_icon_painter(Box::new(|painter, rect, style, color| {
        let r = rect.width() / 2.0 - 2.0;
        painter.circle_stroke(
            rect.center(),
            r,
            egui::Stroke::new(style.stroke_px(), color),
        );
        painter.circle_filled(rect.center(), r * 0.35, color);
    }));
    shell.add(
        screen("painter", move |ui: &mut egui::Ui, cx: &mut Cx| {
            layout::page(ui, cx, "painter", |ui, cx| {
                layout::title(ui, cx, "Custom painter");
                layout::note(
                    ui,
                    cx,
                    "The desktop icon for this screen is neither an SVG nor a glyph \
                     - it is a draw callback registered with `register_icon_painter`.",
                );
                // `Outlined` — the box holds a drawing and nothing else, so the drawing should be
                // the only thing in it with weight. A filled card would put a grey slab behind
                // three grey rings.
                layout::section_card_with(
                    ui,
                    cx,
                    "One callback, many sizes",
                    layout::Deco::new().container(layout::Container::Outlined),
                    |ui, cx| {
                        let m = cx.theme.metrics;
                        let (rect, _) = ui.allocate_exact_size(
                            egui::vec2(ui.available_width(), m.row_height * 2.4),
                            egui::Sense::hover(),
                        );
                        let mut x = rect.left() + m.screen_inset * 2.0;
                        for side in [m.icon_size * 1.6, m.icon_size, m.icon_size * 0.6] {
                            let at = egui::Rect::from_center_size(
                                egui::pos2(x + side / 2.0, rect.center().y),
                                egui::Vec2::splat(side),
                            );
                            cx.icons.paint(
                                ui.painter(),
                                at,
                                &IconRef::Custom(ring),
                                &IconStyle::sized(side)
                                    .color(fairing::icons::IconColor::Role(ColorRole::Primary)),
                                cx.theme,
                            );
                            x += side + m.screen_inset * 2.0;
                        }
                    },
                );
                layout::note(
                    ui,
                    cx,
                    "`IconStyle::stroke_px` derives the stroke width from the size, \
                     so the lines stay apart even when the icon is drawn small.",
                );
            });
        })
        .title("Painter")
        .icon(IconRef::Custom(ring))
        .desktop(),
    );
    shell.add(
        action("scan-wifi", |cx| {
            let _ = cx.services.wifi.scan();
        })
        .title("Scan")
        .icon(icon::WIFI)
        .desktop(),
    );
    add_icon_gallery(shell);
    add_m2_screens(shell);
    shell.add(status_item("temp", Slot::Right, |ui, _cx| {
        ui.label("36.5°");
    }));
}

/// The three M2 display screens: `form` (two `TextEdit`s — the OSK; the first field takes focus when it
/// opens), `widgets` (a `BigButton` long press · a `Switch` · a `TouchSlider` · a `ListRow`) and
/// `notify` (raising notifications and toasts).
fn add_m2_screens(shell: &mut fairing::Shell) {
    add_form_screen(shell);
    add_widgets_screen(shell);
    add_dropdowns_screen(shell);
    add_notify_screen(shell);
    add_m2_icons_screen(shell);
}

/// A display of the 27 built-in icons added in M2 — they are also the second page's icons.
fn add_m2_icons_screen(shell: &mut fairing::Shell) {
    shell.add(
        screen("icons2", |ui: &mut egui::Ui, cx: &mut Cx| {
            layout::page(ui, cx, "icons2", |ui, cx| {
                layout::title(ui, cx, "Icons");
                layout::note(
                    ui,
                    cx,
                    "The base set is 78 icons; the 27 below were added in M2. Every one of \
                     them is a single `IconRef::Builtin(\"name\")` - the colour comes \
                     from a palette role and the stroke width is derived from the \
                     size.",
                );
                layout::section(ui, cx, "Added in M2");
                // **The icon display is laid out by `Grid` too.** Writing a line break into `egui::Grid`
                // as `i % 7 == 6` sent seven cells off the edge of a narrow panel — the column count has
                // to be settled by the width.
                let row = cx.theme.metrics.row_height;
                layout::Grid::new(2.0, 2.0)
                    .deco(layout::Deco::new().visual_inset(3.0))
                    .show(ui, cx, &M2_ICONS, |ui, cx, cell| {
                        let v = cell.visual;
                        let side = (v.height() * ICON_TILE).min(v.width() * ICON_TILE);
                        cx.icons.paint(
                            ui.painter(),
                            egui::Rect::from_center_size(
                                egui::pos2(v.center().x, v.top() + v.height() * 0.38),
                                egui::Vec2::splat(side),
                            ),
                            &IconRef::Builtin(cell.item),
                            &IconStyle::sized(side),
                            cx.theme,
                        );
                        // The names vary in length (`hard-drive` against `sd-card`), so they are shrunk
                        // to the cell's width — at a fixed size they intrude on the neighbour.
                        layout::fit_text(
                            ui.painter(),
                            egui::pos2(v.center().x, v.bottom() - v.height() * 0.18),
                            egui::Align2::CENTER_CENTER,
                            cell.item,
                            v.width() * 0.92,
                            egui::FontId::proportional(row * ICON_LABEL),
                            cx.theme.color(ColorRole::Muted),
                        );
                    });
            });
        })
        .title("Icons+")
        .icon(icon::GRID)
        .desktop(),
    );
}

/// The icons added in M2 (`assets/icons/MAPPING.md`).
const M2_ICONS: [&str; 27] = [
    "arrow-up",
    "arrow-down",
    "arrow-left",
    "arrow-right",
    "split",
    "fullscreen",
    "minimize",
    "ethernet",
    "usb",
    "sd-card",
    "airplane",
    "brightness",
    "calendar",
    "users",
    "download",
    "upload",
    "save",
    "file",
    "grid",
    "list",
    "printer",
    "terminal",
    "fan",
    "plug",
    "image",
    "memory",
    "hard-drive",
];

/// `form`: two `TextEdit`s. On the frame it opens (`Created` / `Resumed`) the first field takes focus
/// and the OSK comes up by itself — the reproducibility of the tour's `11-osk.png`.
fn add_form_screen(shell: &mut fairing::Shell) {
    // A screen closure is `FnMut`, so its own state is just a `mut` capture — no cell needed for
    // something only this screen touches.
    let mut form = (String::new(), String::new());
    shell.add(
        screen("form", move |ui: &mut egui::Ui, cx: &mut Cx| {
            ui.heading("Form (OSK)");
            ui.label(format!(
                "inset_bottom = {:.0} px (content is not pushed)",
                cx.pane.inset_bottom
            ));
            let f = &mut form;
            // **No size is written as a number.** Pinned at `360 × 48`, it stayed that way however the
            // panel changed, and only the field grew while the text clung to the top. `TextField` takes
            // its height from `touch_target` and its corners, hint colour and focus ring from the theme.
            let a = TextField::new(&mut f.0)
                .id_salt("name")
                .hint("device name")
                .show(ui, &mut cx.widgets());
            TextField::new(&mut f.1)
                .id_salt("note")
                .hint("note")
                .multiline(true)
                .show(ui, &mut cx.widgets());
            // The tour's reproducibility: on the frame it opens the first field takes focus, so the OSK comes up by itself.
            if matches!(cx.event, Some(Lifecycle::Created | Lifecycle::Resumed)) {
                a.request_focus();
            }
        })
        .title("Form")
        .icon(icon::KEYBOARD)
        .desktop(),
    );
}

/// `widgets`: the A7 widget display — the long-press ring, the press scale, the switch knob, the slider thumb, the list rows.
/// The Widgets screen's Choice card: the rest of the control vocabulary.
///
/// `Checkbox` and `SegmentedControl` shipped with the vocabulary and nothing in the crate or this
/// demo drew one, so a change to either reached no screenshot and no eye. A pick-one list is a
/// radio group, which `layout::choice_rows` now is - see the Locale settings screen.
fn choice_card(ui: &mut egui::Ui, cx: &mut Cx<'_>, checked: &mut bool, segment: &mut usize) {
    layout::section(ui, cx, "Choice");
    layout::group(ui, cx, |ui, cx| {
        let inset = cx.theme.metrics.content_inset;
        ui.horizontal(|ui| {
            ui.add_space(inset);
            let _ = Checkbox::new(checked).show(ui, &mut cx.widgets());
            // No gap of its own: the box sits inside a whole `touch_target` slot, so the slot's
            // own margin is already the space before the label. `control.gap` on top of it left
            // the label visibly adrift.
            ui.label("Repeat on failure");
        });
        ui.add_space(inset * 0.5);
        ui.horizontal(|ui| {
            ui.add_space(inset);
            let width = (ui.available_width() - inset).max(inset);
            ui.allocate_ui_with_layout(
                egui::vec2(width, ui.available_height()),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if let Some(i) = SegmentedControl::new(&["Auto", "On", "Off"], *segment)
                        .show(ui, &mut cx.widgets())
                        .picked
                    {
                        *segment = i;
                    }
                },
            );
        });
        ui.add_space(inset * 0.5);
    });
}

/// The Widgets screen's Lists card: three settings rows.
fn lists_card(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    layout::section(ui, cx, "Lists");
    layout::group(ui, cx, |ui, cx| {
        let _ = ListRow::new("Ethernet")
            .subtitle("192.168.0.20")
            .icon(icon::ETHERNET)
            .icon_color(ColorRole::Primary)
            .trailing("up")
            .chevron(true)
            .separator(false)
            .show(ui, &mut cx.widgets());
        let _ = ListRow::new("Printer")
            .subtitle("Idle")
            .icon(icon::PRINTER)
            .icon_color(ColorRole::Primary)
            .chevron(true)
            .separator(false)
            .show(ui, &mut cx.widgets());
        let _ = ListRow::new("Storage")
            .subtitle("Unavailable")
            .icon(icon::HARD_DRIVE)
            .enabled(false)
            .separator(false)
            .show(ui, &mut cx.widgets());
    });
}

/// The Widgets screen's Expandable card: which rows are open, the header switch, the slider
/// inside a body, the accordion's open slot, and whether Advanced has been revealed.
#[derive(Clone, Copy)]
struct Expand {
    /// Display and Night light: open or not.
    open: [bool; 2],
    night_on: bool,
    strength: f32,
    which: Option<usize>,
    revealed: bool,
}

/// The Expandable card and the accordion under it.
fn expandable_card(ui: &mut egui::Ui, cx: &mut Cx<'_>, e: &mut Expand) {
    layout::section(ui, cx, "Expandable");
    layout::group(ui, cx, |ui, cx| {
        let [display, night] = &mut e.open;
        let _ = ExpandableRow::new("display", "Display")
            .icon(icon::DISPLAY)
            .subtitle("Resolution, scale")
            .summary("1920 × 1080")
            .show(ui, cx, display, |ui, cx| {
                layout::info_row(ui, cx, "Scale", "150 %");
                layout::info_row(ui, cx, "Orientation", "Landscape");
            });
        let _ = ExpandableRow::new("night", "Night light")
            .icon(icon::MOON)
            .subtitle("Warmer colours after dark")
            .switch(&mut e.night_on)
            .show(ui, cx, night, |ui, cx| {
                let _ = layout::slider_row(
                    ui,
                    cx,
                    "Strength",
                    &mut e.strength,
                    0.0..=100.0,
                    " %",
                    true,
                );
                let _ = layout::nav_row(ui, cx, "Schedule", Some("Sunset to sunrise"));
            });
        let _ = layout::advanced_rows(
            ui,
            cx,
            "advanced",
            "Advanced",
            &["Proxy", "MAC address"],
            &mut e.revealed,
            |ui, cx| {
                layout::info_row(ui, cx, "Proxy", "None");
                layout::info_row(ui, cx, "MAC address", "Randomised");
            },
        );
    });
    layout::note(
        ui,
        cx,
        "The whole row opens it and the chevron only says so; the switch on Night light \
         keeps its own patch and does not open the row. Advanced goes away once tapped.",
    );
    layout::section(ui, cx, "One at a time");
    layout::group(ui, cx, |ui, cx| {
        for (i, (name, inside)) in [("Network", "Wi-Fi, Proxy"), ("Sound", "Volume, Alerts")]
            .into_iter()
            .enumerate()
        {
            layout::accordion(&mut e.which, i, |open| {
                let _ =
                    ExpandableRow::new(name, name)
                        .summary(inside)
                        .show(ui, cx, open, |ui, cx| {
                            layout::info_row(ui, cx, "Inside", inside);
                        });
            });
        }
    });
}

fn add_widgets_screen(shell: &mut fairing::Shell) {
    let widgets = Rc::new(Cell::new((false, 40.0_f32, 0u32, true, 1usize)));
    let expand = Rc::new(Cell::new(Expand {
        open: [false; 2],
        night_on: true,
        strength: 60.0,
        which: None,
        revealed: false,
    }));
    shell.add(
        screen("widgets", move |ui: &mut egui::Ui, cx: &mut Cx| {
            let (mut on, mut value, mut done, mut checked, mut segment) = widgets.get();
            let mut e = expand.get();
            layout::page(ui, cx, "widgets", |ui, cx| {
                layout::title(ui, cx, "Widgets");

                layout::section(ui, cx, "Buttons");
                // **The buttons are inside the card.** They were outside it for a while — because
                // `ButtonKind::Normal` is `SurfaceVariant` and vanished whole on a card of the same
                // colour. Now that `Normal` carries an `Outline` border, they are back in place.
                // Free content in a card: `pad_content` keeps the buttons clear of the card's
                // corner, and the row wraps within that inset on a narrow panel.
                layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
                    ui.horizontal_wrapped(|ui| {
                        let hold = BigButton::new("Power off")
                            .icon(icon::POWER)
                            .kind(ButtonKind::Danger)
                            .long_press(Duration::from_secs(1))
                            .show(ui, &mut cx.widgets());
                        if hold.completed {
                            done += 1;
                        }
                        let _ = BigButton::new("OK")
                            .icon(icon::CHECK)
                            .kind(ButtonKind::Primary)
                            .show(ui, &mut cx.widgets());
                        let _ = BigButton::new("Cancel").show(ui, &mut cx.widgets());
                    });
                });
                layout::note(
                    ui,
                    cx,
                    &format!(
                        "The red button is a one-second long press - let go while \
                         the ring is filling and it cancels. An irreversible action \
                         never gets a single tap. {done} completed so far."
                    ),
                );

                layout::section(ui, cx, "Values");
                layout::group(ui, cx, |ui, cx| {
                    let _ = layout::switch_row(
                        ui,
                        cx,
                        "Auto brightness",
                        Some(if on { "On" } else { "Off" }),
                        &mut on,
                        true,
                    );
                    let _ = layout::slider_row(
                        ui,
                        cx,
                        "Brightness",
                        &mut value,
                        0.0..=100.0,
                        " %",
                        true,
                    );
                });
                layout::note(
                    ui,
                    cx,
                    "Tapping anywhere on the row toggles it - if only the switch \
                     were the touch target, a gloved hand would keep missing. The \
                     slider thumb grows 1.3x while you hold it.",
                );

                // **The rest of the control set, on a screen.** `Checkbox` and `SegmentedControl`
                // shipped with the vocabulary and nothing in the crate or this demo drew one, so a
                // change to either reached no screenshot and no eye. A pick-one list is a radio
                // group, and `layout::choice_rows` is that - see the Locale settings screen.
                choice_card(ui, cx, &mut checked, &mut segment);

                lists_card(ui, cx);

                expandable_card(ui, cx, &mut e);
            });
            widgets.set((on, value, done, checked, segment));
            expand.set(e);
        })
        .title("Widgets")
        .icon(icon::WRENCH)
        .desktop(),
    );
}

/// What the Dropdowns screen chooses among. Each set is what its trigger is for: a sort order
/// with its shortcuts on a button; three lock-in settings as form fields; a unit inline; a filter
/// chip; a mode tile — and then the four openers, each over the kind of list it is for.
const SORT_BY: [&str; 4] = ["Name", "Date", "Size", "Type"];
const SORT_KEYS: [&str; 4] = ["Ctrl+1", "Ctrl+2", "Ctrl+3", "Ctrl+4"];
const LOCK_MODES: [&str; 4] = ["PDH", "Side of fringe", "Dither", "Ramp"];
const INPUTS: [&str; 3] = ["PD In 1", "PD In 2", "Fast In"];
const RANGES: [&str; 3] = ["±1 V", "±5 V", "±10 V"];
const UNITS: [&str; 3] = ["mV", "V", "kV"];
const SHOW: [&str; 3] = ["All", "Errors", "Warnings"];
const MODES: [&str; 3] = ["Standby", "Run", "Service"];
const EDIT: [&str; 4] = ["Cut", "Copy", "Paste", "Select all"];
const EDIT_KEYS: [&str; 4] = ["Ctrl+X", "Ctrl+C", "Ctrl+V", "Ctrl+A"];
const UNIT_GRID: [&str; 9] = ["mV", "V", "kV", "mA", "A", "Hz", "kHz", "MHz", "°C"];
const SOURCES: [&str; 5] = ["Laser 1", "Laser 2", "Reference", "External", "Simulated"];
const DEVICES: [&str; 20] = [
    "Amplifier",
    "Attenuator",
    "Beam splitter",
    "Chopper",
    "Collimator",
    "Detector",
    "Etalon",
    "Fiber coupler",
    "Filter wheel",
    "Grating",
    "Isolator",
    "Lens",
    "Mirror",
    "Modulator",
    "Photodiode",
    "Polarizer",
    "Prism",
    "Shutter",
    "Waveplate",
    "Wavemeter",
];

/// The drums: a clock's hour and its minute in fives, both joined at the ends, and a quantity
/// that is not.
const HOURS: [&str; 24] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "15",
    "16", "17", "18", "19", "20", "21", "22", "23",
];
const MINUTES: [&str; 12] = [
    "00", "05", "10", "15", "20", "25", "30", "35", "40", "45", "50", "55",
];
const QUANTITY: [&str; 20] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17",
    "18", "19", "20",
];

/// The Dropdowns screen's choices, one per control — the screen's own state, captured by its
/// closure.
#[derive(Default)]
struct Picks {
    hour: usize,
    minute: usize,
    quantity: usize,
    sort: usize,
    lock: usize,
    input: usize,
    range: usize,
    unit: usize,
    show: usize,
    mode: usize,
    edit: usize,
    unit_grid: usize,
    source: usize,
    device: usize,
}

/// `dropdowns`: the five closed forms of `Dropdown` and its four ways of opening.
fn add_dropdowns_screen(shell: &mut fairing::Shell) {
    let mut picks = Picks {
        input: 1,
        mode: 1,
        hour: 9,
        minute: 6,
        quantity: 2,
        ..Picks::default()
    };
    shell.add(
        screen("dropdowns", move |ui: &mut egui::Ui, cx: &mut Cx| {
            let p = &mut picks;
            layout::page(ui, cx, "dropdowns", |ui, cx| {
                layout::title(ui, cx, "Dropdowns");
                layout::section(ui, cx, "Triggers");
                layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
                    dropdown_triggers(ui, cx, p);
                });
                layout::note(
                    ui,
                    cx,
                    "Five closed forms, one contract: a tap opens, a row picks, a tap outside \
                     closes without landing on what is under it. The sort button carries its \
                     shortcuts at the right of its rows — shown, not bound.",
                );
                layout::section(ui, cx, "Openers");
                layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
                    dropdown_openers(ui, cx, p);
                });
                layout::note(
                    ui,
                    cx,
                    "The count picks the form: up to four is a segmented control, five to \
                     twelve a list, short symbols a gloved hand picks go in a grid, a phone's \
                     habit is the sheet, and past twelve you search: the trigger itself becomes \
                     the field, and the page keeps it clear of the keyboard.",
                );
                layout::section(ui, cx, "Ordered values");
                layout::group_with(ui, cx, layout::Deco::new().pad_content(), |ui, cx| {
                    wheels(ui, cx, p);
                });
                layout::note(
                    ui,
                    cx,
                    "A value with an order is a drum, not a list: flick it and it carries, let \
                     go and it lands on a row, and the hour rolls from 23 into 00. A tap on a \
                     row under the window turns to it.",
                );
            });
        })
        .title("Dropdowns")
        .icon(icon::LIST)
        .desktop(),
    );
}

/// The Triggers card: a button with shortcuts, the three field looks side by side, an inline
/// value beside a chip, and a tile.
fn dropdown_triggers(ui: &mut egui::Ui, cx: &mut Cx<'_>, p: &mut Picks) {
    let gap = cx.theme.control.gap;
    let _ = Dropdown::new("dd.sort", &SORT_BY, &mut p.sort)
        .label("Sort by")
        .hints(&SORT_KEYS)
        .show(ui, &mut cx.widgets());
    ui.add_space(gap);
    ui.columns(3, |cols| {
        if let [a, b, c] = cols {
            let _ = Dropdown::new("dd.lock", &LOCK_MODES, &mut p.lock)
                .label("Lock mode")
                .trigger(Trigger::Field(FieldLook::Outlined))
                .show(a, &mut cx.widgets());
            let _ = Dropdown::new("dd.input", &INPUTS, &mut p.input)
                .label("Input")
                .trigger(Trigger::Field(FieldLook::Filled))
                .show(b, &mut cx.widgets());
            let _ = Dropdown::new("dd.range", &RANGES, &mut p.range)
                .label("Range")
                .trigger(Trigger::Field(FieldLook::Underlined))
                .show(c, &mut cx.widgets());
        }
    });
    ui.add_space(gap);
    ui.horizontal(|ui| {
        let _ = Dropdown::new("dd.unit", &UNITS, &mut p.unit)
            .label("Units")
            .trigger(Trigger::Inline)
            .show(ui, &mut cx.widgets());
        ui.add_space(gap);
        let _ = Dropdown::new("dd.show", &SHOW, &mut p.show)
            .label("Show")
            .trigger(Trigger::Chip)
            .show(ui, &mut cx.widgets());
    });
    ui.add_space(gap);
    let _ = Dropdown::new("dd.mode", &MODES, &mut p.mode)
        .label("Mode")
        .trigger(Trigger::Tile)
        .show(ui, &mut cx.widgets());
}

/// The Ordered values card: an hour and a minute drum that roll round, and a quantity that stops
/// at its ends.
fn wheels(ui: &mut egui::Ui, cx: &mut Cx<'_>, p: &mut Picks) {
    ui.columns(3, |cols| {
        if let [a, b, c] = cols {
            let _ = WheelPicker::new("dd.hour", &HOURS, &mut p.hour)
                .wrap(true)
                .show(a, &mut cx.widgets());
            let _ = WheelPicker::new("dd.minute", &MINUTES, &mut p.minute)
                .wrap(true)
                .show(b, &mut cx.widgets());
            let _ = WheelPicker::new("dd.quantity", &QUANTITY, &mut p.quantity)
                .show(c, &mut cx.widgets());
        }
    });
}

/// The Openers card: a list with shortcuts, a grid of units, a sheet of sources and a search
/// over twenty devices.
fn dropdown_openers(ui: &mut egui::Ui, cx: &mut Cx<'_>, p: &mut Picks) {
    let gap = cx.theme.control.gap;
    ui.columns(2, |cols| {
        if let [a, b] = cols {
            let _ = Dropdown::new("dd.edit", &EDIT, &mut p.edit)
                .label("Edit")
                .hints(&EDIT_KEYS)
                .show(a, &mut cx.widgets());
            let _ = Dropdown::new("dd.grid", &UNIT_GRID, &mut p.unit_grid)
                .label("Unit")
                .opener(Opener::Grid)
                .show(b, &mut cx.widgets());
        }
    });
    ui.add_space(gap);
    ui.columns(2, |cols| {
        if let [a, b] = cols {
            let _ = Dropdown::new("dd.source", &SOURCES, &mut p.source)
                .label("Source")
                .opener(Opener::Sheet)
                .show(a, &mut cx.widgets());
            let _ = Dropdown::new("dd.device", &DEVICES, &mut p.device)
                .label("Device")
                .opener(Opener::Search)
                .show(b, &mut cx.widgets());
        }
    });
}

/// `notify`: buttons raise notifications (heads-ups) and toasts and toggle the shade — all through the `cx.shell` handle.
fn add_notify_screen(shell: &mut fairing::Shell) {
    let sent = Rc::new(Cell::new(0u32));
    shell.add(
        screen("notify", move |ui: &mut egui::Ui, cx: &mut Cx| {
            layout::page(ui, cx, "notify", |ui, cx| {
                layout::title(ui, cx, "Notifications");
                layout::status_card(
                    ui,
                    cx,
                    &format!("{} sent", sent.get()),
                    Some("Pull the shade down to see what piled up"),
                    Some(ColorRole::Primary),
                );

                layout::section(ui, cx, "Send");
                layout::group(ui, cx, |ui, cx| {
                    if layout::icon_row(
                        ui,
                        cx,
                        icon::BELL,
                        "Notification",
                        Some("Pops up as heads-up, then piles into the shade"),
                        None,
                    )
                    .clicked()
                    {
                        let n = sent.get() + 1;
                        sent.set(n);
                        cx.shell.notify(
                            Notification::new(
                                NotificationId(u64::from(n)),
                                format!("Job {n} finished"),
                            )
                            .body("3 files exported")
                            .source("notify")
                            .icon(icon::DOWNLOAD),
                        );
                    }
                    if layout::icon_row(
                        ui,
                        cx,
                        icon::INFO,
                        "Toast",
                        Some("Shows for a moment and goes - never piles up"),
                        None,
                    )
                    .clicked()
                    {
                        cx.shell.toast("Saved");
                    }
                    if layout::icon_row(
                        ui,
                        cx,
                        icon::ARROW_DOWN,
                        "Open the shade",
                        Some("Same as pulling from the top edge"),
                        None,
                    )
                    .clicked()
                    {
                        cx.shell.toggle_overlay();
                    }
                });
                layout::note(
                    ui,
                    cx,
                    "A notification and a toast are different things. A notification \
                     is a fact you may need to look up later, so it stays in the \
                     shade; a toast is a receipt for what you just did.",
                );
            });
        })
        .title("Notify")
        .icon(icon::BELL)
        .desktop(),
    );
}

/// The parametric icon display screen. The tour's `07-parametric.png` is this screen.
fn add_icon_gallery(shell: &mut fairing::Shell) {
    shell.add(
        screen("icons", |ui: &mut egui::Ui, cx: &mut Cx| {
            gallery_ui(ui, cx);
        })
        .title("Icons")
        .icon(icon::SIGNAL)
        .desktop(),
    );
}

/// One display cell.
#[derive(Debug, Clone, Copy)]
enum Param {
    /// `wifi(level, off)`.
    Wifi(u8, bool),
    /// `battery(percent, charging)`.
    Battery(u8, bool),
    /// `volume(level, muted)`.
    Volume(u8, bool),
    /// `bluetooth(state)`.
    Bluetooth(BtIconState),
    /// `signal(bars)`.
    Signal(u8),
    /// `progress_ring(t)`.
    Ring(f32),
}

impl Param {
    fn draw(self, painter: &egui::Painter, rect: egui::Rect, style: &parametric::ParamStyle) {
        match self {
            Self::Wifi(level, off) => parametric::wifi(painter, rect, level, off, style),
            Self::Battery(percent, charging) => {
                parametric::battery(painter, rect, percent, charging, style);
            }
            Self::Volume(level, muted) => parametric::volume(painter, rect, level, muted, style),
            Self::Bluetooth(state) => parametric::bluetooth(painter, rect, state, style),
            Self::Signal(bars) => parametric::signal(painter, rect, bars, style),
            Self::Ring(t) => parametric::progress_ring(painter, rect, t, style),
        }
    }
}

/// One row: the name on the left plus the cells.
type Row = (&'static str, &'static [(&'static str, Param)]);

/// All six parametric icons.
const GALLERY: &[Row] = &[
    (
        "wifi(level, off)",
        &[
            ("0", Param::Wifi(0, false)),
            ("1", Param::Wifi(1, false)),
            ("2", Param::Wifi(2, false)),
            ("3", Param::Wifi(3, false)),
            ("4", Param::Wifi(4, false)),
            ("off", Param::Wifi(4, true)),
        ],
    ),
    (
        "battery(%, charging)",
        &[
            ("100", Param::Battery(100, false)),
            ("50", Param::Battery(50, false)),
            ("15", Param::Battery(15, false)),
            ("80 +", Param::Battery(80, true)),
        ],
    ),
    (
        "volume(level, muted)",
        &[
            ("0", Param::Volume(0, false)),
            ("1", Param::Volume(1, false)),
            ("2", Param::Volume(2, false)),
            ("3", Param::Volume(3, false)),
            ("muted", Param::Volume(2, true)),
        ],
    ),
    (
        "bluetooth(state)",
        &[
            ("off", Param::Bluetooth(BtIconState::Off)),
            ("on", Param::Bluetooth(BtIconState::On)),
            ("connected", Param::Bluetooth(BtIconState::Connected)),
        ],
    ),
    (
        "signal(bars)",
        &[
            ("0", Param::Signal(0)),
            ("1", Param::Signal(1)),
            ("2", Param::Signal(2)),
            ("3", Param::Signal(3)),
            ("4", Param::Signal(4)),
        ],
    ),
    (
        "progress_ring(t)",
        &[
            ("0.00", Param::Ring(0.0)),
            ("0.25", Param::Ring(0.25)),
            ("0.50", Param::Ring(0.5)),
            ("0.75", Param::Ring(0.75)),
            ("1.00", Param::Ring(1.0)),
        ],
    ),
];

/// The name column's width (px).
const GALLERY_LABEL_W: f32 = 168.0;

/// The most cells in a row (`GALLERY`'s maximum).
const GALLERY_COLS: f32 = 6.0;

/// Draw the display. It divides the area left into 6 rows and draws the name plus the cells (the icon and the argument caption) itself.
fn gallery_ui(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    ui.heading("Parametric icons");
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
    if rect.width() < GALLERY_LABEL_W || rect.height() <= 0.0 {
        return;
    }
    let painter = ui.painter().clone();
    let fg = cx.theme.color(ColorRole::OnSurface);
    let muted = cx.theme.color(ColorRole::Muted);
    let row_h = rect.height() / 6.0;
    let cell_w = (rect.width() - GALLERY_LABEL_W) / GALLERY_COLS;
    let side = (row_h - 20.0).min(cell_w - 10.0).max(8.0);
    let style = IconStyle::sized(side).param_style(cx.theme);
    let name_font = egui::FontId::monospace(13.0);
    let caption_font = egui::FontId::proportional(11.0);
    for (index, (name, cells)) in (0u8..).zip(GALLERY.iter()) {
        let top = f32::from(index).mul_add(row_h, rect.top());
        painter.text(
            egui::pos2(rect.left(), top + row_h / 2.0),
            egui::Align2::LEFT_CENTER,
            *name,
            name_font.clone(),
            fg,
        );
        for (column, (caption, param)) in (0u8..).zip(cells.iter()) {
            let cx0 =
                f32::from(column).mul_add(cell_w, rect.left() + GALLERY_LABEL_W) + cell_w / 2.0;
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(cx0, top + side / 2.0 + 4.0),
                egui::vec2(side, side),
            );
            param.draw(&painter, icon_rect, &style);
            painter.text(
                egui::pos2(cx0, icon_rect.bottom() + 2.0),
                egui::Align2::CENTER_TOP,
                *caption,
                caption_font.clone(),
                muted,
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tour mode
// ─────────────────────────────────────────────────────────────────────────────

/// The gesture navigation's tour (`--nav=gesture`): the home indicator, a lift on its
/// way up, a lift held into the overview, a lift let go into home, and a quick switch between two
/// tasks along the indicator.
const GESTURE_PLAN: &[Act] = &[
    Act::Settle,
    Act::Wait(15),
    Act::Open("dashboard"),
    Act::Settle,
    Act::Wait(2),
    Act::Home,
    Act::Settle,
    Act::Open("widgets"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("36-gesture-bar.png"),
    // The demo keeps the recent screens behind `nav.recents` (maintainer): a pause asks for the
    // PIN first, the screen going back down, and the cards come up once it is in.
    // (A pause is 150 ms of stillness; the wait is on the clock, not in frames.)
    Act::Press(Spot::Edge(Side::Bottom, 0.5)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.91),
        frames: 8,
    },
    Act::WaitMs(400),
    Act::Release,
    Act::Settle,
    Act::Pin("2468"),
    Act::Settle,
    Act::Back,
    Act::Settle,
    Act::Wait(2),
    // Up from the bottom edge: the screen follows the finger, shrinking as it rises, the point
    // pressed staying under it.
    Act::Press(Spot::Edge(Side::Bottom, 0.5)),
    Act::DragTo {
        to: Spot::Page(0.55, 0.91),
        frames: 12,
    },
    Act::Wait(1),
    Act::Shot("36a-lift.png"),
    // Held there: the overview comes up, the screen carried on into its card.
    Act::WaitMs(400),
    Act::Shot("36b-lift-held.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("36c-overview.png"),
    Act::Back,
    Act::Settle,
    Act::Wait(2),
    // Flung up and let go: home, the screen carrying on from where the finger left it into its
    // icon.
    Act::Press(Spot::Edge(Side::Bottom, 0.5)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.58),
        frames: 5,
    },
    Act::Release,
    Act::Wait(5),
    Act::Shot("36d-lift-home.png"),
    Act::Settle,
    Act::Wait(2),
    // Back into the gallery, then along the indicator to the right: the dashboard, used before
    // it, slides in from the left.
    Act::Open("widgets"),
    Act::Settle,
    Act::Wait(2),
    Act::Press(Spot::Edge(Side::Bottom, 0.29)),
    Act::DragTo {
        to: Spot::Edge(Side::Bottom, 0.7),
        frames: 10,
    },
    Act::Wait(1),
    Act::Shot("36e-switch.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("36f-switched.png"),
];

/// The tour's script. The file names are fixed.
const PLAN: &[Act] = &[
    Act::Settle,
    Act::Wait(15), // time for the font atlas and the first repaint to settle
    // A warm-up — not captured. The egui `Area` of a Pane layer slot (`("fairing.screen", depth)`)
    // runs **its first visible frame as a sizing pass**, so that frame's `Ui` drawing is invisible
    // whole (`egui-0.36.1/src/containers/area.rs`: `sizing_pass = state.is_none()` →
    // `ui_builder.invisible()`). Capturing a transition's first frame would come out with that layer
    // empty, so slots 0 and 1 are each raised once beforehand. The glyph atlas warms up with them and
    // the frames get faster.
    Act::Open("painter"),
    Act::Settle,
    Act::Open("progress"),
    Act::Settle,
    Act::Back,
    Act::Settle,
    Act::Back,
    Act::Settle,
    Act::Wait(6),
    Act::Shot("01-desktop.png"),
    // The README's animation is this stretch (`--record`): an icon zooms open into its screen, a
    // second screen pushes over it and pops back off, and home closes the screen into its icon.
    // The waits after the shots only hold each picture long enough to read.
    Act::Record("demo-desktop"),
    Act::Wait(30),
    Act::Open("dashboard"),
    Act::HomeMid(0.5, "02-icon-zoom-mid.png"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("03-screen-open.png"),
    Act::Wait(30),
    Act::Open("progress"),
    Act::StackMid(0.5, "04-push-mid.png"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("05-pushed.png"),
    Act::Wait(30),
    Act::Back,
    Act::StackMid(0.5, "06-back-mid.png"),
    Act::Settle,
    Act::Wait(20),
    Act::Home,
    Act::Settle,
    Act::Wait(30),
    Act::RecordEnd,
    Act::Open("icons"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("07-parametric.png"),
    Act::Home,
    Act::HomeMid(0.5, "08-home-mid.png"),
    Act::Settle,
    Act::Wait(4),
    // ── M2 ──
    // Two notifications are put in first — they show in the shade's list (a heads-up is absorbed the moment the pull begins).
    Act::Notify(Level::Success, "Backup done", "12 files, 3.2 MB"),
    Act::Notify(Level::Error, "Sensor 3 offline", "check wiring"),
    Act::Wait(30),
    // 09 · 10: the top edge pull, to the middle of the content — about half the curtain's full
    // drop. The render mapping being a curtain (A1), the mid frame shows the panel's **head**
    // (the tile row plus the front of the notification list) with the handle and the scrim at
    // the curtain's end, and the footer still outside it; then on to four fifths, past the snap.
    Act::Press(Spot::Edge(Side::Top, 0.01)),
    Act::DragTo {
        to: Spot::Page(0.01, 0.44),
        frames: 12,
    },
    Act::Wait(2),
    Act::Shot("09-shade-mid.png"),
    Act::DragTo {
        to: Spot::Page(0.01, 0.8),
        frames: 8,
    },
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Expect(Expect::ShadeOpen),
    Act::Expect(Expect::Text("Sensor 3 offline")),
    Act::Shot("10-shade-open.png"),
    // 10b: press a Slider tile to open its expansion row. **This state was missing from the tour**, so
    // nobody saw the track sitting there alone — the icon comes down from the tile and says whose value it is.
    Act::Tap(Spot::Text("Brightness")),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("10b-shade-slider.png"),
    // 10c: a notification row being pushed left — the space revealed shows it is **being thrown away**.
    // The finger lands on the row, found by its title and pressed well into its body: a push
    // from the title's own place has no room to go left before the glass ends.
    Act::Press(Spot::Near("Sensor 3 offline", 350.0, 0.0)),
    Act::DragTo {
        to: Spot::Across(0.21),
        frames: 10,
    },
    Act::Wait(2),
    Act::Shot("10c-shade-swipe.png"),
    // Pushed past the confirmation line (a third of the width) — releasing here really removes it, and
    // the script goes on through the row below rising to fill the gap.
    Act::DragTo {
        to: Spot::Across(0.0),
        frames: 6,
    },
    Act::Release,
    // The place removed closes by **collapsing** — the row below rises to fill the gap. The
    // list's own motion is not the shell's `is_animating`, so the check waits for it.
    Act::Settle,
    Act::Until(Expect::NoText("Sensor 3 offline"), 120),
    Act::Expect(Expect::Text("Backup done")),
    // 10d: press and hold the Wi-Fi tile to go to the Wi-Fi settings (the shade goes up). With nothing
    // attached it only raises a `TileLongPressed` and goes nowhere.
    Act::Press(Spot::Text("Wi-Fi")),
    Act::WaitMs(700),
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::ShadeClosed),
    Act::Shot("10d-tile-long-press.png"),
    Act::Home,
    Act::Settle,
    // 11: the OSK — `form` has the first field take focus on the frame it opens (the A5 180 ms show).
    Act::Open("form"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("11-osk.png"),
    // Over to two-set Hangul, which the demo does not open on (`demo.toml [osk] layout = "qwerty"`),
    // for a mid-composition step — as far as `ㅎ` + `ㅏ`. A composing syllable goes into the field as
    // real characters too (`osk::inject::inject_compose` — that is where the reason for not using
    // `Preedit` is).
    Act::Osk("hangul"),
    Act::Wait(6),
    Act::Key("ㅎ"),
    Act::Wait(4),
    Act::Key("ㅏ"),
    Act::Wait(4),
    Act::Shot("11a-osk-compose.png"),
    Act::Key("ㄴ"),
    Act::Wait(4),
    // Back on the English qwerty, as the `한/영` key crosses: the syllable stays in the field.
    Act::Osk("qwerty"),
    Act::Wait(6),
    Act::Shot("11b-osk-latin.png"),
    Act::Back, // close the OSK (the priority)
    Act::Settle,
    Act::Back, // a root pop → home
    Act::Settle,
    Act::Wait(4),
    // 12: two toasts plus a heads-up (A6). Captured after the 160/220 ms entrances finish.
    Act::Toast("Saved"),
    Act::Toast("Wi-Fi reconnected"),
    Act::Notify(Level::Success, "Job finished", "3 files exported"),
    Act::Wait(20),
    Act::Shot("12-toast.png"),
    // The toasts' 3 s and the heads-up's 4.4 s pass, on the clock.
    Act::Settle,
    Act::WaitMs(4600),
    Act::Expect(Expect::NoText("Job finished")),
    // 13: mid page swipe (A4) — a horizontal drag over the grid. `pos = dx / W`, and each page's
    // cells are centred within it, so both pages are in view only once it is dragged half the
    // width: from two thirds across to a fifth.
    Act::Press(Spot::Page(0.68, 0.5)),
    Act::DragTo {
        to: Spot::Page(0.18, 0.5),
        frames: 12,
    },
    Act::Wait(2),
    Act::Shot("13-page-swipe-mid.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    // 14: mid left-edge back (the A3 gesture) — dragging progress out from over dashboard.
    Act::Open("dashboard"),
    Act::Settle,
    Act::Open("progress"),
    Act::Settle,
    Act::Wait(4),
    Act::Press(Spot::Edge(Side::Left, 0.5)),
    Act::DragTo {
        to: Spot::Page(0.35, 0.5),
        frames: 12,
    },
    Act::Wait(2),
    Act::Shot("14-back-gesture-mid.png"),
    Act::Release,
    Act::Settle,
    Act::Home,
    Act::Settle,
    Act::Wait(2),
    // ── The built-in settings screens ──
    Act::Open("settings.home"),
    Act::Settle,
    Act::Wait(4),
    Act::Expect(Expect::Screen("settings.home")),
    Act::Shot("15-settings-home.png"),
    Act::Open("settings.wifi"),
    Act::Settle,
    Act::Wait(4),
    Act::Expect(Expect::Screen("settings.wifi")),
    Act::Expect(Expect::Text("Scan for networks")),
    Act::Shot("16-settings-wifi.png"),
    Act::Back,
    Act::Settle,
    Act::Open("settings.display"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("17-settings-display.png"),
    Act::Back,
    Act::Settle,
    Act::Open("settings.about"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("18-settings-about.png"),
    // The list is longer than the screen — push it up to see whether the open-source notices really come out.
    Act::Press(Spot::Page(0.5, 0.77)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.14),
        frames: 10,
    },
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("19-settings-scrolled.png"),
    Act::Home,
    Act::Settle,
    Act::Open("settings.bluetooth"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("20-settings-bluetooth.png"),
    Act::Home,
    Act::Settle,
    Act::Open("settings.sound"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("21-settings-sound.png"),
    Act::Home,
    Act::Settle,
    Act::Open("settings.datetime"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("22-settings-datetime.png"),
    Act::Home,
    Act::Settle,
    Act::Open("settings.locale"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("23-settings-locale.png"),
    Act::Home,
    Act::Settle,
    Act::Open("settings.power"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("24-settings-power.png"),
    Act::Home,
    Act::Settle,
    // ── The widget, notification and custom screens (the showcase) ──
    // These were missing until now. How the widgets and layouts the crate offers actually stand is what
    // **these four** show, and with them absent from the tour nobody could see it.
    Act::Open("widgets"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("25-widgets.png"),
    // The Choice card - checkbox, segmented - sits below the fold on a 1024 x 600 panel, and a
    // control nobody photographs is a control nobody checks.
    Act::Press(Spot::Page(0.5, 0.85)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.14),
        frames: 10,
    },
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("25b-widgets-choice.png"),
    // The Expandable card and the accordion sit at the end of the page: two dead-stop scrolls
    // reach it, and the page's end is where it always lands.
    Act::Press(Spot::Page(0.5, 0.87)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.02),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    Act::Press(Spot::Page(0.5, 0.87)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.02),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    // At a one-finger row the page's end starts below the Display row: a short pull back down
    // brings the whole expandable card into view, held still so it does not fling.
    Act::Press(Spot::Page(0.5, 0.21)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.68),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::Text("Night light")),
    Act::Shot("25c-widgets-expandable.png"),
    // The Display row, open: its body unrolls and Night light moves down.
    Act::Expect(Expect::Text("1920 × 1080")),
    Act::Tap(Spot::Text("Display")),
    Act::Settle,
    Act::Wait(8),
    // Open, the summary goes and the body's own rows come.
    Act::Expect(Expect::Text("Orientation")),
    Act::Expect(Expect::NoText("1920 × 1080")),
    Act::Shot("25d-widgets-expanded.png"),
    // The end of the page: the Advanced row and the accordion.
    Act::Press(Spot::Page(0.5, 0.87)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.02),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    Act::Press(Spot::Page(0.5, 0.87)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.02),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Shot("25e-widgets-accordion.png"),
    Act::Home,
    Act::Settle,
    Act::Open("notify"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("26-notify.png"),
    Act::Home,
    Act::Settle,
    Act::Open("painter"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("27-painter.png"),
    Act::Home,
    Act::Settle,
    Act::Open("fullscreen"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("28-fullscreen.png"),
    Act::Home,
    Act::Settle,
    Act::Open("service"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("29-service.png"),
    Act::Home,
    Act::Settle,
    Act::Open("icons2"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("30-icons-builtin.png"),
    Act::Home,
    Act::Settle,
    Act::Open("dropdowns"),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("31-dropdowns.png"),
    // The openers sit a screen down. The scroll is held still before the release: a flung page
    // keeps moving after the shot. The openers are then pressed by their captions, so where the
    // card came to rest does not matter.
    Act::Press(Spot::Page(0.5, 0.87)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.02),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::Text("Edit")),
    Act::Expect(Expect::Text("Device")),
    Act::Shot("31b-dropdowns-openers.png"),
    // Each opener is pressed by its caption and checked by an entry only its list shows; a tap on
    // the page beside it closes it again, and the entry is checked to be gone.
    Act::Tap(Spot::Text("Edit")),
    Act::Settle,
    Act::Wait(8),
    Act::Expect(Expect::Text("Ctrl+X")),
    Act::Shot("31d-dropdown-list-hints.png"),
    Act::Tap(Spot::Page(0.88, 0.77)),
    Act::Settle,
    Act::Expect(Expect::NoText("Ctrl+X")),
    Act::Tap(Spot::Text("Unit")),
    Act::Settle,
    Act::Wait(8),
    Act::Expect(Expect::Text("kHz")),
    Act::Shot("31e-dropdown-grid.png"),
    Act::Tap(Spot::Page(0.2, 0.77)),
    Act::Settle,
    Act::Expect(Expect::NoText("kHz")),
    Act::Tap(Spot::Text("Source")),
    Act::Settle,
    Act::Wait(8),
    Act::Expect(Expect::Text("Laser 2")),
    Act::Shot("31f-dropdown-sheet.png"),
    Act::Tap(Spot::Page(0.2, 0.03)),
    Act::Settle,
    Act::Expect(Expect::NoText("Laser 2")),
    // The search: opened, the list is there to be read; a second tap, on the field the head has
    // become, gives it the keyboard — the qwerty, to type a Latin query.
    Act::Tap(Spot::Text("Device")),
    Act::Settle,
    Act::Expect(Expect::Text("Amplifier")),
    // The second tap goes on the field's value half, to the right of its caption: the caption
    // names the field, the value is what takes the focus.
    Act::Tap(Spot::Near("Device", 140.0, 0.0)),
    Act::Settle,
    Act::Wait(8),
    Act::Expect(Expect::OskUp),
    Act::Shot("31g-dropdown-search.png"),
    Act::Wait(4),
    Act::Key("m"),
    Act::Key("o"),
    Act::Settle,
    Act::Wait(8),
    // "mo" leaves Modulator in the list and takes Attenuator out. (Amplifier is still on the
    // glass as the field's own value.)
    Act::Expect(Expect::Text("Modulator")),
    Act::Expect(Expect::NoText("Attenuator")),
    Act::Shot("31h-dropdown-search-typed.png"),
    Act::Tap(Spot::Page(0.1, 0.19)),
    Act::Settle,
    // The keyboard is solid now and takes a while to slide away: a press on it would not
    // scroll the page.
    Act::Until(Expect::OskDown, 120),
    Act::Settle,
    Act::Press(Spot::Page(0.5, 0.87)),
    Act::DragTo {
        to: Spot::Page(0.5, 0.02),
        frames: 10,
    },
    Act::Wait(15),
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::Text("Ordered values")),
    Act::Shot("31c-dropdowns-drums.png"),
    Act::Home,
    Act::Settle,
    Act::Wait(2),
    // ── The shell's own prompt and lock screen (A9) ──
    // `admin` is the one icon left locked: opening it brings the keypad up over the desktop.
    Act::Open("admin"),
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::Text("Enter PIN")),
    Act::Shot("32-unlock-prompt.png"),
    // A wrong PIN: the table's own words in the title's place, and the card shaking (caught
    // mid-swing).
    Act::Pin("1357"),
    Act::Wait(3),
    Act::Expect(Expect::Text("Wrong PIN")),
    Act::Shot("32a-wrong-pin.png"),
    Act::Settle,
    Act::Wait(10),
    // The pattern tab: the same level by a path through the dots.
    Act::PromptTab(1),
    Act::Settle,
    Act::Wait(6),
    Act::Shot("32c-pattern.png"),
    // A wrong pattern stays on the dots in red while the card shakes.
    Act::Pattern(&[1, 4, 7, 8]),
    Act::Wait(3),
    Act::Shot("32d-wrong-pattern.png"),
    Act::Settle,
    Act::Wait(10),
    // The demo's pattern, a Z — caught with the finger still down on the last dot. On the release
    // the prompt goes, `admin` opens through its gate, and the padlock comes up in the status bar.
    Act::PatternHold(&[1, 2, 3, 5, 7, 8, 9]),
    Act::Wait(8),
    Act::Shot("32e-pattern-drawn.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(6),
    Act::Expect(Expect::Screen("admin")),
    Act::Expect(Expect::Text("Unlocked")),
    Act::Shot("32b-unlocked.png"),
    // The lock tile's `LaunchAction::Lock`, which the shell carries out in `prompt` mode.
    Act::Lock,
    Act::Settle,
    Act::Wait(6),
    Act::Shot("33-lock-screen.png"),
    Act::Pin("2468"),
    Act::Settle,
    Act::Home,
    Act::Settle,
    Act::Wait(2),
    // ── Two panes (A8) ──
    // The dashboard, then the widget gallery opened beside it: the dashboard shrinks to the first
    // half while the gallery slides in to the second (caught on the way).
    Act::Open("dashboard"),
    Act::Settle,
    Act::Wait(2),
    Act::OpenBeside("widgets"),
    Act::Wait(5),
    Act::Shot("34a-split-entering.png"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("34-split.png"),
    // The divider under a finger: both panes lay themselves out again as it moves.
    Act::GrabDivider,
    Act::DragTo {
        to: Spot::Across(0.37),
        frames: 10,
    },
    Act::Wait(2),
    Act::Shot("34b-divider-drag.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    // Pushed to the end, it squeezes the gallery under its minimum (dimmed) — and let go there, the
    // gallery's pane closes and the dashboard fills the content again.
    Act::GrabDivider,
    Act::DragTo {
        to: Spot::Across(1.1),
        frames: 12,
    },
    Act::Wait(2),
    Act::Shot("34c-divider-squeeze.png"),
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("34d-unsplit.png"),
    // ── The overview (A10) ──
    // Recents over the dashboard: it shrinks into its card while the others fade in beside it.
    Act::Recents,
    Act::Wait(7),
    Act::Shot("35a-overview-entering.png"),
    Act::Settle,
    Act::Wait(4),
    Act::Shot("35-overview.png"),
    // A drag across the cards scrolls them.
    Act::Press(Spot::Page(0.68, 0.5)),
    Act::DragTo {
        to: Spot::Page(0.25, 0.5),
        frames: 14,
    },
    Act::Release,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("35b-overview-scrolled.png"),
    Act::Back,
    Act::Settle,
    Act::Wait(2),
    // The split control opens the same cards as a picker for the other side.
    Act::SplitControl,
    Act::Settle,
    Act::Wait(4),
    Act::Shot("35c-overview-picker.png"),
    Act::Back,
    Act::Settle,
    Act::Home,
    Act::Settle,
    Act::Wait(2),
    // ── An icon's info popover ──
    // A finger held on an icon: what it is, before anyone opens it — the 500 ms long press,
    // waited on the clock.
    Act::PressIcon("dashboard"),
    Act::WaitMs(800),
    Act::Release,
    Act::Settle,
    Act::Wait(2),
    Act::Shot("37-icon-info.png"),
    // A press anywhere puts it away, and opens nothing.
    Act::Tap(Spot::Page(0.5, 0.5)),
    Act::Settle,
    // The PIN from the unlock above still holds: back to viewer, and a locked icon says which
    // level it needs.
    Act::Logout,
    Act::Settle,
    Act::PressIcon("admin"),
    Act::WaitMs(800),
    Act::Release,
    Act::Settle,
    Act::Wait(2),
    Act::Shot("37a-icon-info-locked.png"),
    Act::Tap(Spot::Page(0.5, 0.5)),
    Act::Settle,
    Act::Wait(2),
];

/// The tour's shell — the same config and declarations as the demo, but with no scenario thread (for the screenshots' reproducibility).
fn build_tour(ctx: &egui::Context) -> fairing::Result<fairing::Shell> {
    let mut config = ShellConfig::from_toml(CONFIG)?;
    apply_config_args(&mut config);
    if config.motion.reduce {
        log::warn!("motion.reduce = true - no mid-transition shots will come out");
    }
    let (services, _control) = services();
    let mut builder = panel_scale(fairing::Shell::builder(config))
        .services(services)
        .fonts(common::korean_fonts())
        .image_loader(common::image_loader);
    if let Some(wallpaper) = default_wallpaper(ctx) {
        builder = builder.wallpaper(wallpaper);
    }
    let mut shell = builder.build(ctx)?;
    apply_legibility(&mut shell);
    add_settings(&mut shell);
    add_screens(&mut shell);
    // **The hidden entry points go on the tour shell too.** Without them the `service` screen is never
    // declared and `Act::Open("service")` quietly captures the desktop — which is what happened. The
    // three triggers do not collide with the tour's synthetic input (a tour of the corners · F1 F2 F1 ·
    // seven times on the same row).
    add_hidden_entries(&mut shell);
    if status_labels() {
        apply_status_labels(&mut shell);
    }
    if let Some(placement) = dock_placement_arg() {
        log::info!("dock placement {placement:?}");
        shell.desktop_mut().set_dock_placement(placement);
    }
    Ok(shell)
}

/// Lay `--panel-mm` · `--finger-mm` · `--legacy` onto the builder.
///
/// Given a physical size, the shell settles `pixels_per_point` and the metrics at that density.
/// Without one it is the unknown-density fallback, and the startup log says so.
fn panel_scale(builder: fairing::ShellBuilder) -> fairing::ShellBuilder {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut b = builder;
    if let Some((w, h)) = common::arg_panel_mm(&args) {
        log::info!("panel size {w} x {h} mm");
        b = b.physical_mm(w, h);
    }
    // The crate's default, a bare finger, is right for the shop terminal the demo imitates.
    // `--finger-mm=13` shows the gloved density.
    if let Some(finger) = common::arg_finger_mm(&args) {
        log::info!("finger {finger} mm");
        b = b.scale_policy(fairing::unit::ScalePolicy::default().with_finger_mm(finger));
    }
    if common::arg_legacy(&args) {
        log::info!("drawing with pre-M2b metrics (legacy_du)");
        b = b.metrics_spec(fairing::theme::MetricsSpec::legacy_du());
    } else if status_labels() {
        // Standing a caption above and below the icon takes a thicker bar — a value the integrator settles.
        use fairing::unit::{Dim, Span};
        b = b.metrics_spec(fairing::theme::MetricsSpec {
            status_bar_height: Span::fixed(Dim::mm(13.0)).min(Dim::du(56.0)),
            ..fairing::theme::MetricsSpec::default()
        });
    }
    b
}

/// `--wallpaper=NAME` — the wallpaper (`abyss` · a role name · `#RRGGBB` · `file:<path>`).
///
/// `file:` is read only with feature `raster` on:
///
/// ```text
/// cargo run --example demo --features runner,mock,raster -- ///     --wallpaper=file:$PWD/assets/brand/abyss-landscape.webp
/// ```
fn wallpaper_arg() -> Option<String> {
    std::env::args().find_map(|a| a.strip_prefix("--wallpaper=").map(str::to_owned))
}

/// `--wallpaper-fit=cover|contain|stretch` — how a raster wallpaper is fitted.
fn wallpaper_fit_arg() -> Option<String> {
    std::env::args().find_map(|a| a.strip_prefix("--wallpaper-fit=").map(str::to_owned))
}

/// `--rail=none|left|right` — the icon rail (the kiosk layout).
fn rail_arg() -> Option<String> {
    std::env::args().find_map(|a| a.strip_prefix("--rail=").map(str::to_owned))
}

/// `--abyss-tier=flat|lite|full` — the Abyss quality tier.
fn abyss_tier_arg() -> Option<fairing::config::AbyssTier> {
    use fairing::config::AbyssTier;
    let raw = std::env::args().find_map(|a| a.strip_prefix("--abyss-tier=").map(str::to_owned))?;
    match raw.as_str() {
        "flat" => Some(AbyssTier::Flat),
        "lite" => Some(AbyssTier::Lite),
        "full" => Some(AbyssTier::Full),
        other => {
            log::warn!("--abyss-tier={other} is not a known tier (flat | lite | full)");
            None
        }
    }
}

/// Lay the demo's flags onto the config — once, before the shell is built.
fn apply_config_args(config: &mut ShellConfig) {
    if let Some(preset) = preset_arg() {
        log::info!("palette preset {}", preset.as_str());
        preset.as_str().clone_into(&mut config.theme.preset);
    }
    if let Some(theme) = theme_arg() {
        log::info!("theme {theme}");
        config.shell.theme = theme;
    }
    if let Some(name) = wallpaper_arg() {
        log::info!("wallpaper {name}");
        config.desktop.wallpaper = name;
    }
    if let Some(fit) = wallpaper_fit_arg() {
        log::info!("wallpaper fit {fit}");
        config.desktop.wallpaper_fit = fit;
    }
    if let Some(rail) = rail_arg() {
        log::info!("icon rail {rail}");
        config.desktop.rail = rail;
    }
    if let Some(tier) = abyss_tier_arg() {
        log::info!("abyss tier {tier:?}");
        config.desktop.abyss.tier = tier;
    }
    if let Some(nav) = nav_arg() {
        log::info!("nav bar {nav}");
        config.nav_bar.style = nav;
    }
}

/// `--nav=buttons|gesture` — the nav bar's style. Without it, the config's (buttons).
fn nav_arg() -> Option<String> {
    let raw = std::env::args().find_map(|a| a.strip_prefix("--nav=").map(str::to_owned))?;
    if raw != "buttons" && raw != "gesture" {
        log::warn!("--nav={raw} is not a known style (buttons | gesture)");
        return None;
    }
    Some(raw)
}

/// `--theme=dark|light` — which of the preset's two palettes to start on. Without it, the
/// config's default (dark). Both palettes ship with every preset, so a device that lives under a
/// window and a device in a dim workshop take the same build.
fn theme_arg() -> Option<String> {
    let raw = std::env::args().find_map(|a| a.strip_prefix("--theme=").map(str::to_owned))?;
    if raw != "dark" && raw != "light" {
        log::warn!("--theme={raw} is not a known theme (dark | light)");
        return None;
    }
    Some(raw)
}

/// `--preset=base|abyss|linen` — the palette preset. Without it, the config's default (base).
fn preset_arg() -> Option<fairing::theme::Preset> {
    let raw = std::env::args().find_map(|a| a.strip_prefix("--preset=").map(str::to_owned))?;
    let out = fairing::theme::Preset::parse(&raw);
    if out.is_none() {
        log::warn!("--preset={raw} is not a known preset (base | abyss | linen)");
    }
    out
}

/// `--dock=left|right|top|bottom|band` — where the dock goes.
fn dock_placement_arg() -> Option<fairing::desktop::DockPlacement> {
    use fairing::desktop::{Axis, DockPlacement};
    use fairing::gesture::Edge;
    let raw = std::env::args().find_map(|a| a.strip_prefix("--dock=").map(str::to_owned))?;
    Some(match raw.as_str() {
        "top" => DockPlacement::Edge(Edge::Top),
        "left" => DockPlacement::Edge(Edge::Left),
        "right" => DockPlacement::Edge(Edge::Right),
        // A row across the desktop — put two thirds of the way down so it does not overlap the icon grid.
        "band" => DockPlacement::Band {
            axis: Axis::Horizontal,
            at: 0.66,
        },
        _ => DockPlacement::Edge(Edge::Bottom),
    })
}

/// `--status-labels` — put captions on the status bar items to show the real-instrumentation layout.
fn status_labels() -> bool {
    std::env::args().any(|a| a == "--status-labels")
}

/// Give the status bar items captions, text sizes and minimum widths.
fn apply_status_labels(shell: &mut fairing::Shell) {
    use fairing::chrome::LabelPos;
    let bar = shell.status_bar_mut();
    for (id, label) in [
        ("status.wifi", "Network"),
        ("status.bluetooth", "Bluetooth"),
        ("status.battery", "Power"),
        ("status.notifications", "Notification"),
    ] {
        if let Some(spec) = bar.spec_mut(id) {
            spec.label = Some(label.to_owned());
            spec.label_pos = LabelPos::Below;
            spec.pad_x = Some(6.0);
        }
    }
    // The clock takes a larger size rather than a caption. A minimum width keeps its place from shifting as the value wobbles.
    if let Some(spec) = bar.spec_mut("status.clock") {
        spec.text_size = Some(22.0);
        spec.min_width = Some(84.0);
    }
}
