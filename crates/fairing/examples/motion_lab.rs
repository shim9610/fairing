//! The motion tuning tool: it replays the shade, the pages, push/pop and the
//! icon zoom on a loop while the `[motion]` tokens are adjusted live on sliders. Run it on the board,
//! and save the values you like into `[motion]` — "feel" cannot be settled in a document; it is
//! settled on the real thing.
//!
//! - **The frame graph, top right**: the last [`SAMPLES`] frames' intervals (ms) plus the p50 and
//!   p95. The frame cap of 33 ms is drawn as a gridline. A bar over the line means that motion is
//!   eating the budget.
//! - **The replay loop**: home → the pages there and back → the icon zoom → the shade opening and
//!   closing → push/pop, round and round without end. It exists to repeat the same transition
//!   hands-off and compare the numbers.
//! - **Export**: it prints the current values as `[motion]` TOML to the log (stderr). Paste it
//!   straight into a config file. (The workspace lint blocks `print_stdout`, so it goes to the
//!   logger.)
//!
//! It runs the shell itself with [`fairing::runner::run`] rather than
//! [`fairing::runner::run_shell`] — the replay loop and the frame graph need a `&mut Shell` every
//! frame (the same frame as `examples/common`'s tour).
//!
//! `cargo run -p fairing --features runner-x11 --example motion_lab -- --size=1024x600`

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // Slider reals ↔ integer ms.

mod common;

use fairing::services::mock;
use fairing::theme::{ColorRole, MotionTokens};
use fairing::widgets::{BigButton, Switch, TextField};
use fairing::{icon, screen, Cx, LaunchAction, Shell, ShellConfig};
use std::fmt::Write as _;
use std::time::Duration;

/// How many samples the frame graph holds (3 s at 60 Hz).
const SAMPLES: usize = 180;

/// The frame budget (the 33 ms cap). The graph's gridline, and its vertical maximum.
const BUDGET_MS: f32 = 33.0;

/// The graph box's size (px).
const GRAPH_SIZE: egui::Vec2 = egui::vec2(240.0, 72.0);

/// How many frames one replay step stays for (≈ 0.75 s at 60 Hz).
const STEP_FRAMES: u32 = 45;

// ── The knobs ─────────────────────────────────────────────────────────────────

/// The bundle of values the sliders edit. It covers every section of `[motion]` (`crossfade_ms` ·
/// `theme_fade_ms` · `clear_top_ms` included — the M2 integration gave them config keys).
#[derive(Debug, Clone, Copy)]
struct Knobs {
    // Shared
    spring_k: f32,
    spring_c: f32,
    snap_ratio: f32,
    fling: f32,
    slop: f32,
    tap_ms: f32,
    long_press_ms: f32,
    reduce: bool,
    // A2 home ↔ task · A3 push/pop
    push_ms: f32,
    pop_ms: f32,
    parallax: f32,
    dim: f32,
    home_open_ms: f32,
    home_close_ms: f32,
    desktop_scale: f32,
    clear_top_ms: f32,
    // A1 the shade
    shade_k: f32,
    shade_c: f32,
    shade_snap: f32,
    shade_rubber: f32,
    shade_rubber_max: f32,
    // A4 the pages
    page_k: f32,
    page_c: f32,
    page_fling: f32,
    page_rubber: f32,
    page_rubber_max: f32,
    // A5 OSK
    osk_show_ms: f32,
    osk_hide_ms: f32,
    osk_debounce_ms: f32,
    // A6 toasts and heads-ups
    toast_in_ms: f32,
    toast_out_ms: f32,
    toast_shift_ms: f32,
    heads_up_in_ms: f32,
    heads_up_out_ms: f32,
    heads_up_hold_ms: f32,
    // A7 widgets
    press_ms: f32,
    press_release_ms: f32,
    press_scale: f32,
    switch_ms: f32,
    crossfade_ms: f32,
    theme_fade_ms: f32,
}

impl Knobs {
    fn from_config(cfg: &ShellConfig) -> Self {
        let m = &cfg.motion;
        let tokens = MotionTokens::from_config(m);
        Self {
            spring_k: m.spring.k,
            spring_c: m.spring.c,
            snap_ratio: m.snap_ratio,
            fling: m.fling_px_s,
            slop: m.slop_px,
            tap_ms: m.tap_ms as f32,
            long_press_ms: m.long_press_ms as f32,
            reduce: m.reduce,
            push_ms: m.push.ms as f32,
            pop_ms: m.pop.ms as f32,
            parallax: m.push.parallax,
            dim: m.push.dim,
            home_open_ms: m.home.open_ms as f32,
            home_close_ms: m.home.close_ms as f32,
            desktop_scale: m.home.desktop_scale,
            clear_top_ms: tokens.clear_top.duration.as_millis() as f32,
            shade_k: m.shade.spring.k,
            shade_c: m.shade.spring.c,
            shade_snap: m.shade.snap_ratio,
            shade_rubber: m.shade.rubber,
            shade_rubber_max: m.shade.rubber_max_px,
            page_k: m.page.spring.k,
            page_c: m.page.spring.c,
            page_fling: m.page.fling_px_s,
            page_rubber: m.page.rubber,
            page_rubber_max: m.page.rubber_max,
            osk_show_ms: m.osk.show_ms as f32,
            osk_hide_ms: m.osk.hide_ms as f32,
            osk_debounce_ms: m.osk.hide_debounce_ms as f32,
            toast_in_ms: m.toast.in_ms as f32,
            toast_out_ms: m.toast.out_ms as f32,
            toast_shift_ms: m.toast.shift_ms as f32,
            heads_up_in_ms: m.toast.heads_up_in_ms as f32,
            heads_up_out_ms: m.toast.heads_up_out_ms as f32,
            heads_up_hold_ms: m.toast.heads_up_hold_ms as f32,
            press_ms: m.press.ms as f32,
            press_release_ms: m.press.release_ms as f32,
            press_scale: m.press.scale,
            switch_ms: m.switch.ms as f32,
            crossfade_ms: tokens.crossfade.duration.as_millis() as f32,
            theme_fade_ms: tokens.theme_fade.duration.as_millis() as f32,
        }
    }

    /// Build a [`MotionTokens`] from the current values (`base` is where the rest of the settings come from).
    fn tokens(&self, base: &ShellConfig) -> MotionTokens {
        let mut cfg = base.motion.clone();
        cfg.reduce = self.reduce;
        cfg.spring.k = self.spring_k;
        cfg.spring.c = self.spring_c;
        cfg.snap_ratio = self.snap_ratio;
        cfg.fling_px_s = self.fling;
        cfg.slop_px = self.slop;
        cfg.tap_ms = ms(self.tap_ms);
        cfg.long_press_ms = ms(self.long_press_ms);
        cfg.push.ms = ms(self.push_ms);
        cfg.push.parallax = self.parallax;
        cfg.push.dim = self.dim;
        cfg.pop.ms = ms(self.pop_ms);
        cfg.home.open_ms = ms(self.home_open_ms);
        cfg.home.close_ms = ms(self.home_close_ms);
        cfg.home.desktop_scale = self.desktop_scale;
        cfg.shade.spring.k = self.shade_k;
        cfg.shade.spring.c = self.shade_c;
        cfg.shade.snap_ratio = self.shade_snap;
        cfg.shade.rubber = self.shade_rubber;
        cfg.shade.rubber_max_px = self.shade_rubber_max;
        cfg.page.spring.k = self.page_k;
        cfg.page.spring.c = self.page_c;
        cfg.page.fling_px_s = self.page_fling;
        cfg.page.rubber = self.page_rubber;
        cfg.page.rubber_max = self.page_rubber_max;
        cfg.osk.show_ms = ms(self.osk_show_ms);
        cfg.osk.hide_ms = ms(self.osk_hide_ms);
        cfg.osk.hide_debounce_ms = ms(self.osk_debounce_ms);
        cfg.toast.in_ms = ms(self.toast_in_ms);
        cfg.toast.out_ms = ms(self.toast_out_ms);
        cfg.toast.shift_ms = ms(self.toast_shift_ms);
        cfg.toast.heads_up_in_ms = ms(self.heads_up_in_ms);
        cfg.toast.heads_up_out_ms = ms(self.heads_up_out_ms);
        cfg.toast.heads_up_hold_ms = ms(self.heads_up_hold_ms);
        cfg.press.ms = ms(self.press_ms);
        cfg.press.release_ms = ms(self.press_release_ms);
        cfg.press.scale = self.press_scale;
        cfg.switch.ms = ms(self.switch_ms);
        cfg.crossfade_ms = ms(self.crossfade_ms);
        cfg.theme_fade_ms = ms(self.theme_fade_ms);
        cfg.clear_top_ms = ms(self.clear_top_ms);
        MotionTokens::from_config(&cfg)
    }

    /// The `[motion]` TOML. It can be pasted straight into a config file.
    fn motion_toml(&self) -> String {
        let mut out = String::with_capacity(1024);
        let _ = writeln!(out, "[motion]");
        let _ = writeln!(out, "reduce = {}", self.reduce);
        let _ = writeln!(out, "snap_ratio = {:.3}", self.snap_ratio);
        let _ = writeln!(out, "fling_px_s = {:.1}", self.fling);
        let _ = writeln!(out, "slop_px = {:.1}", self.slop);
        let _ = writeln!(out, "tap_ms = {}", ms(self.tap_ms));
        let _ = writeln!(out, "long_press_ms = {}", ms(self.long_press_ms));
        let _ = writeln!(out, "spring = {}", spring(self.spring_k, self.spring_c));
        let _ = writeln!(
            out,
            "\n[motion.push]\nms = {}\nparallax = {:.3}\ndim = {:.3}",
            ms(self.push_ms),
            self.parallax,
            self.dim
        );
        let _ = writeln!(out, "\n[motion.pop]\nms = {}", ms(self.pop_ms));
        let _ = writeln!(
            out,
            "\n[motion.home]\nopen_ms = {}\nclose_ms = {}\ndesktop_scale = {:.3}",
            ms(self.home_open_ms),
            ms(self.home_close_ms),
            self.desktop_scale
        );
        let _ = writeln!(
            out,
            "\n[motion.shade]\nspring = {}\nsnap_ratio = {:.3}\nrubber = {:.3}\nrubber_max_px = {:.1}",
            spring(self.shade_k, self.shade_c),
            self.shade_snap,
            self.shade_rubber,
            self.shade_rubber_max
        );
        let _ = writeln!(
            out,
            "\n[motion.page]\nspring = {}\nfling_px_s = {:.1}\nrubber = {:.3}\nrubber_max = {:.3}",
            spring(self.page_k, self.page_c),
            self.page_fling,
            self.page_rubber,
            self.page_rubber_max
        );
        let _ = writeln!(
            out,
            "\n[motion.osk]\nshow_ms = {}\nhide_ms = {}\nhide_debounce_ms = {}",
            ms(self.osk_show_ms),
            ms(self.osk_hide_ms),
            ms(self.osk_debounce_ms)
        );
        let _ = writeln!(
            out,
            "\n[motion.toast]\nin_ms = {}\nout_ms = {}\nshift_ms = {}\n\
             heads_up_in_ms = {}\nheads_up_out_ms = {}\nheads_up_hold_ms = {}",
            ms(self.toast_in_ms),
            ms(self.toast_out_ms),
            ms(self.toast_shift_ms),
            ms(self.heads_up_in_ms),
            ms(self.heads_up_out_ms),
            ms(self.heads_up_hold_ms)
        );
        let _ = writeln!(
            out,
            "\n[motion.press]\nms = {}\nrelease_ms = {}\nscale = {:.3}",
            ms(self.press_ms),
            ms(self.press_release_ms),
            self.press_scale
        );
        let _ = writeln!(out, "\n[motion.switch]\nms = {}", ms(self.switch_ms));
        // Being a top-level `[motion]` key, the section header is written again at the end so it does not fall under an earlier section.
        let _ = writeln!(
            out,
            "\n[motion]\ncrossfade_ms = {}\ntheme_fade_ms = {}\nclear_top_ms = {}",
            ms(self.crossfade_ms),
            ms(self.theme_fade_ms),
            ms(self.clear_top_ms)
        );
        out
    }
}

fn ms(value: f32) -> u64 {
    value.round().clamp(0.0, 60_000.0) as u64
}

fn spring(k: f32, c: f32) -> String {
    format!("{{ k = {k:.1}, c = {c:.1} }}")
}

// ── The state the screen and the driver share ─────────────────────────────────

/// The values the `lab` screen (the sliders) and the frame driver (the replay and the graph) share.
#[derive(Debug)]
struct Shared {
    knobs: Knobs,
    /// A slider moved — the driver calls [`Shell::set_motion`] next frame.
    dirty: bool,
    /// The replay loop is running.
    playing: bool,
    /// Where to draw the graph this frame ([`lab_ui`] reserves it in the layout).
    graph_rect: Option<egui::Rect>,
    /// Please export the TOML this frame.
    export: bool,
}

// ── The frame driver ──────────────────────────────────────────────────────────

/// It runs the shell directly and lays the replay loop and the frame graph over it.
struct Lab {
    shell: Option<Shell>,
    /// **The app owns it**. The driver reads and writes it directly, and the lab screen
    /// gets it for the length of a frame through `Shell::frame_with` — no cell between them.
    shared: Shared,
    base: ShellConfig,
    /// The replay step (0 = home, …).
    step: usize,
    /// The frames left in this step.
    wait: u32,
    /// The recent frame intervals (ms), a ring buffer — zero heap allocation per frame.
    times: [f32; SAMPLES],
    head: usize,
    filled: usize,
    /// `--screen <id>` — opened once, on the frame the shell comes up.
    open: Option<String>,
}

/// One turn of the replay loop. The names are written above the graph.
const PLAY: [&str; 8] = [
    "home (A2 close)",
    "page → 1 (A4)",
    "page → 0 (A4)",
    "open lab (A2 open)",
    "shade open (A1)",
    "shade close (A1)",
    "push child (A3)",
    "pop (A3)",
];

impl Lab {
    fn new(shared: Shared, base: ShellConfig) -> Self {
        Self {
            shell: None,
            shared,
            base,
            step: 0,
            wait: 0,
            times: [0.0; SAMPLES],
            head: 0,
            filled: 0,
            open: None,
        }
    }

    /// `--screen <id>` — open this screen right after the shell comes up. Paired with `--shot` to
    /// capture a screen other than home (the form screen with a `TextEdit` on it, say).
    fn screen(mut self, id: Option<String>) -> Self {
        self.open = id;
        self
    }

    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.shell.is_none() {
            match build(&ctx) {
                Ok(shell) => {
                    if let Some(id) = self.open.take() {
                        shell.handle().launch(LaunchAction::open(id));
                    }
                    self.shell = Some(shell);
                }
                Err(err) => {
                    ui.label(format!("cannot build the shell: {err}"));
                    return;
                }
            }
        }
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        // The app owns the knobs and lends them for the frame.
        shell.frame_with(ui, &mut self.shared);
        for event in shell.poll_events() {
            log::debug!("shell event: {event:?}");
        }

        self.record(&ctx);
        let (dirty, playing, export) = {
            let flags = (self.shared.dirty, self.shared.playing, self.shared.export);
            self.shared.dirty = false;
            self.shared.export = false;
            flags
        };
        if dirty {
            let tokens = self.shared.knobs.tokens(&self.base);
            if let Some(shell) = self.shell.as_mut() {
                shell.set_motion(tokens);
            }
        }
        if export {
            log::info!("\n{}", self.shared.knobs.motion_toml());
        }
        if playing {
            // **It has to be stoppable from anywhere.** While the loop goes round home, the shade and
            // a child screen, the lab screen's stop button is off-screen. The graph is always up, so
            // press that, or hit Esc. It is read from the raw input — the graph is a `layer_painter`
            // and so has no widget, and the shell's Areas sit over it, so egui's hit test does not
            // catch it.
            if self.stop_requested(&ctx) {
                self.shared.playing = false;
                self.wait = 0;
            } else {
                self.advance();
                ctx.request_repaint();
            }
        } else {
            self.wait = 0;
        }
        self.graph(ui, playing);
    }

    /// Whether this means stop the replay — a tap on the graph, or Esc.
    fn stop_requested(&self, ctx: &egui::Context) -> bool {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return true;
        }
        let Some(rect) = self.graph_rect() else {
            return false;
        };
        ctx.input(|i| {
            i.pointer.primary_pressed()
                && i.pointer.interact_pos().is_some_and(|p| rect.contains(p))
        })
    }

    /// Where the graph goes — what [`lab_ui`] reserved on the title row. There is none on a frame the
    /// lab screen is not in view (the desktop, a child screen).
    fn graph_rect(&self) -> Option<egui::Rect> {
        self.shared.graph_rect
    }

    /// Put this frame's interval into the ring buffer.
    fn record(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.unstable_dt);
        if let Some(slot) = self.times.get_mut(self.head) {
            *slot = dt * 1000.0;
        }
        self.head = (self.head + 1) % SAMPLES;
        self.filled = (self.filled + 1).min(SAMPLES);
    }

    /// A percentile (0 with no samples). The sort is over a fixed array — no heap allocation.
    fn percentile(&self, p: f32) -> f32 {
        let mut buf = self.times;
        let Some(used) = buf.get_mut(..self.filled) else {
            return 0.0;
        };
        if used.is_empty() {
            return 0.0;
        }
        used.sort_by(f32::total_cmp);
        let index = ((used.len() as f32 - 1.0) * p).round() as usize;
        used.get(index).copied().unwrap_or(0.0)
    }

    /// One step of the replay loop.
    fn advance(&mut self) {
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        let tokens = shell.theme().motion;
        match self.step {
            0 => shell.home(),
            1 => shell.desktop_mut().swipe_to(1, &tokens),
            2 => shell.desktop_mut().swipe_to(0, &tokens),
            3 => shell.launch(LaunchAction::open("lab")),
            4 | 5 => shell.handle().toggle_overlay(),
            6 => shell.launch(LaunchAction::open("child")),
            _ => shell.back(),
        }
        self.step = (self.step + 1) % PLAY.len();
        self.wait = STEP_FRAMES;
    }

    /// The frame graph, top right (the bars are the frame intervals, the horizontal line the 33 ms budget).
    fn graph(&self, ui: &egui::Ui, playing: bool) {
        let Some(shell) = self.shell.as_ref() else {
            return;
        };
        let theme = shell.theme();
        // **It does not cover the status bar** — see [`Self::graph_rect`].
        let Some(rect) = self.graph_rect() else {
            return;
        };
        let painter = ui.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("motion_lab.graph"),
        ));
        painter.rect_filled(
            rect,
            6.0,
            theme.color(ColorRole::Surface).gamma_multiply(0.9),
        );
        painter.rect_stroke(
            rect,
            6.0,
            egui::Stroke::new(1.0, theme.color(ColorRole::Outline)),
            egui::StrokeKind::Inside,
        );
        let plot = rect.shrink2(egui::vec2(6.0, 18.0));
        let step = plot.width() / SAMPLES as f32;
        for i in 0..self.filled {
            // The oldest to the left.
            let slot = (self.head + SAMPLES - self.filled + i) % SAMPLES;
            let value = self.times.get(slot).copied().unwrap_or(0.0);
            let h = (value / BUDGET_MS).clamp(0.0, 1.0) * plot.height();
            let x = plot.min.x + step * i as f32;
            let color = if value > BUDGET_MS {
                theme.color(ColorRole::Danger)
            } else {
                theme.color(ColorRole::Primary)
            };
            painter.line_segment(
                [
                    egui::pos2(x, plot.max.y),
                    egui::pos2(x, plot.max.y - h.max(1.0)),
                ],
                egui::Stroke::new(step.max(1.0), color),
            );
        }
        // The 16.7 ms (60 Hz) gridline.
        let y = plot.max.y - (16.7 / BUDGET_MS) * plot.height();
        painter.line_segment(
            [egui::pos2(plot.min.x, y), egui::pos2(plot.max.x, y)],
            egui::Stroke::new(1.0, theme.color(ColorRole::Muted)),
        );
        painter.text(
            rect.left_top() + egui::vec2(6.0, 2.0),
            egui::Align2::LEFT_TOP,
            format!(
                "p50 {:.1} · p95 {:.1} ms{}",
                self.percentile(0.5),
                self.percentile(0.95),
                if playing { " · ▶" } else { "" }
            ),
            egui::FontId::monospace(11.0),
            theme.color(ColorRole::OnSurface),
        );
        if playing {
            painter.text(
                rect.left_bottom() + egui::vec2(6.0, -2.0),
                egui::Align2::LEFT_BOTTOM,
                PLAY.get(self.step).copied().unwrap_or(""),
                egui::FontId::monospace(11.0),
                theme.color(ColorRole::Muted),
            );
            // **Press here to stop.** While the loop goes round home, the shade and a child screen, the
            // lab screen's stop button is off-screen — it had to be pressed on the right beat. The
            // graph is always up.
            painter.text(
                rect.right_bottom() + egui::vec2(-6.0, -2.0),
                egui::Align2::RIGHT_BOTTOM,
                "tap / Esc to stop",
                egui::FontId::monospace(11.0),
                theme.color(ColorRole::Primary),
            );
        }
    }
}

// ── Building the shell ────────────────────────────────────────────────────────

fn main() -> fairing::Result<()> {
    common::init_logger();
    let args: Vec<String> = std::env::args().collect();
    let size = common::arg_size(&args);
    let base = ShellConfig::default();
    let seed = Shared {
        knobs: Knobs::from_config(&base),
        dirty: false,
        playing: false,
        export: false,
        graph_rect: None,
    };
    let mut lab = Lab::new(seed, base).screen(common::arg_screen(&args));
    // `--shot <dir>` — take one frame and close. This example holds the shell itself and does
    // something different every frame, so it does not go through `common::run_tour` — and so it sat
    // there **with nobody able to see it**.
    let mut shot =
        common::arg_shot(&args).map(|dir| common::OneShot::new(dir, "01-motion-lab.png", 30));
    fairing::runner::run(
        fairing::runner::Options {
            fullscreen: size.is_none(),
            title: "fairing motion lab".to_owned(),
            size,
        },
        move |ui| {
            lab.frame(ui);
            if let Some(shot) = shot.as_mut() {
                shot.tick(ui.ctx());
            }
        },
    )
}

/// Fill in two pages' worth of icons so the page swipe (A4) can be seen. One `TextEdit` on each
/// screen — for checking the OSK (A5).
fn add_fillers(shell: &mut Shell) {
    for (id, ic) in [
        ("a", icon::CAMERA),
        ("b", icon::CHART),
        ("c", icon::CPU),
        ("d", icon::FOLDER),
        ("e", icon::WRENCH),
        ("f", icon::SHIELD),
        ("g", icon::ACTIVITY),
        ("h", icon::THERMOMETER),
        ("i", icon::DISPLAY),
        ("j", icon::KEYBOARD),
        ("k", icon::LANGUAGE),
        ("l", icon::CLOCK),
        ("m", icon::BELL),
    ] {
        // **The buffer lives outside the closure.** Inside, it would be rebuilt every frame and the
        // character just typed thrown away on that frame — which looks like input not working at all.
        let mut text = String::new();
        shell.add(
            screen(id, move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                ui.heading("filler");
                fairing::layout::page(ui, cx, id, |ui, cx| {
                    TextField::new(&mut text)
                        .hint("type here")
                        .show(ui, &mut cx.widgets());
                });
            })
            .title(id)
            .icon(ic)
            .desktop(),
        );
    }
}

fn build(ctx: &egui::Context) -> fairing::Result<Shell> {
    let config = ShellConfig::default();
    let mut shell = Shell::new(config, mock::services(), ctx)?;
    shell.add(
        screen("lab", move |ui: &mut egui::Ui, cx: &mut Cx| {
            cx.with_app::<Shared, _>(|state, cx| lab_ui(ui, cx, state));
        })
        .title("motion lab")
        .icon(icon::GAUGE)
        .desktop()
        .dock(),
    );
    shell.add(
        screen("child", |ui: &mut egui::Ui, cx: &mut Cx| {
            ui.heading("child");
            if BigButton::new("back")
                .show(ui, &mut cx.widgets())
                .response
                .clicked()
            {
                cx.finish();
            }
        })
        .title("child"),
    );
    add_fillers(&mut shell);
    shell.handle().launch(LaunchAction::open("lab"));
    Ok(shell)
}

/// One slider row. `true` if the value changed.
fn knob(
    ui: &mut egui::Ui,
    cx: &mut Cx<'_>,
    label: &str,
    value: &mut f32,
    range: (f32, f32),
) -> bool {
    // It uses **the crate's `slider_row`** — the title, the track and the value on one line, folding
    // to two by itself when the track gets short. It used to stack a `ui.label` and a
    // `TouchSlider` separately, which made one knob two rows tall and turned scanning thirty of them
    // into pure scrolling.
    fairing::layout::slider_row(ui, cx, label, value, range.0..=range.1, "", true)
}

fn lab_ui(ui: &mut egui::Ui, cx: &mut Cx<'_>, state: &mut Shared) {
    // **The graph's place is reserved on the title row.** Left as a floating layer it overlaps
    // whatever is there — the status bar at first, and the `home` button once the buttons moved up.
    // Taking the place from the layout means it cannot overlap, and the "press here to stop" hit is
    // this Rect too.
    ui.horizontal(|ui| {
        ui.heading("motion lab");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (rect, _) = ui.allocate_exact_size(GRAPH_SIZE, egui::Sense::hover());
            state.graph_rect = Some(rect);
        });
    });
    // **The buttons go on top, with no space reserved.**
    //
    // As a bar pinned at the bottom, the bar's height would have to be settled in advance — and how
    // many rows eight of them fold into depends on the width and the label lengths. In practice both
    // `80.0` and `2.6 rows` were wrong and the last button was cut off. On top, they use only as much
    // as they fold into and the knob list takes the rest — there is no number to get right. Always
    // being in view during a replay is a bonus (stop used to be off-screen).
    lab_buttons(ui, cx, state);
    ui.separator();
    lab_knobs(ui, cx, state);
}
/// Every token knob. The body of this tool.
#[allow(clippy::too_many_lines)] // Laying every token out on one screen is the point of this tool.
fn lab_knobs(ui: &mut egui::Ui, cx: &mut Cx<'_>, guard: &mut Shared) {
    let shared: &mut Shared = guard;
    let mut changed_here = false;
    {
        // **One** scroll using all the height left. This used to be put inside an `action_bar` as
        // well, which made two layers of vertical scroll — and then the outer one eats a slider drag
        // and the value does not move.
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let k = &mut shared.knobs;
                egui::CollapsingHeader::new("common")
                    .default_open(true)
                    .show(ui, |ui| {
                        for (label, value, range) in [
                            ("spring k", &mut k.spring_k, (50.0, 1200.0)),
                            ("spring c", &mut k.spring_c, (0.0, 120.0)),
                            ("snap ratio", &mut k.snap_ratio, (0.05, 0.9)),
                            ("fling px/s", &mut k.fling, (100.0, 2000.0)),
                            ("slop px", &mut k.slop, (2.0, 40.0)),
                            ("tap ms", &mut k.tap_ms, (100.0, 800.0)),
                            ("long press ms", &mut k.long_press_ms, (200.0, 1500.0)),
                        ] {
                            changed_here |= knob(ui, cx, label, value, range);
                        }
                        ui.horizontal(|ui| {
                            ui.label("reduce");
                            changed_here |= Switch::new(&mut k.reduce)
                                .show(ui, &mut cx.widgets())
                                .changed();
                        });
                    });
                ui.collapsing("A2 home · A3 push/pop · clear-top", |ui| {
                    for (label, value, range) in [
                        ("push ms", &mut k.push_ms, (60.0, 600.0)),
                        ("pop ms", &mut k.pop_ms, (60.0, 600.0)),
                        ("parallax", &mut k.parallax, (0.0, 0.6)),
                        ("dim", &mut k.dim, (0.0, 0.6)),
                        ("home open ms", &mut k.home_open_ms, (60.0, 600.0)),
                        ("home close ms", &mut k.home_close_ms, (60.0, 600.0)),
                        ("desktop scale", &mut k.desktop_scale, (0.7, 1.0)),
                        ("clear-top ms", &mut k.clear_top_ms, (0.0, 600.0)),
                    ] {
                        changed_here |= knob(ui, cx, label, value, range);
                    }
                });
                ui.collapsing("A1 shade", |ui| {
                    for (label, value, range) in [
                        ("shade k", &mut k.shade_k, (50.0, 1200.0)),
                        ("shade c", &mut k.shade_c, (0.0, 120.0)),
                        ("shade snap", &mut k.shade_snap, (0.05, 0.9)),
                        ("shade rubber", &mut k.shade_rubber, (0.0, 1.0)),
                        ("shade rubber max px", &mut k.shade_rubber_max, (0.0, 160.0)),
                    ] {
                        changed_here |= knob(ui, cx, label, value, range);
                    }
                });
                ui.collapsing("A4 page", |ui| {
                    for (label, value, range) in [
                        ("page k", &mut k.page_k, (50.0, 1200.0)),
                        ("page c", &mut k.page_c, (0.0, 120.0)),
                        ("page fling px/s", &mut k.page_fling, (100.0, 2000.0)),
                        ("page rubber", &mut k.page_rubber, (0.0, 1.0)),
                        ("page rubber max", &mut k.page_rubber_max, (0.0, 0.5)),
                    ] {
                        changed_here |= knob(ui, cx, label, value, range);
                    }
                });
                ui.collapsing("A5 OSK", |ui| {
                    for (label, value, range) in [
                        ("osk show ms", &mut k.osk_show_ms, (0.0, 600.0)),
                        ("osk hide ms", &mut k.osk_hide_ms, (0.0, 600.0)),
                        ("osk debounce ms", &mut k.osk_debounce_ms, (0.0, 500.0)),
                    ] {
                        changed_here |= knob(ui, cx, label, value, range);
                    }
                });
                ui.collapsing("A6 toast · heads-up", |ui| {
                    for (label, value, range) in [
                        ("toast in ms", &mut k.toast_in_ms, (0.0, 600.0)),
                        ("toast out ms", &mut k.toast_out_ms, (0.0, 600.0)),
                        ("toast shift ms", &mut k.toast_shift_ms, (0.0, 600.0)),
                        ("heads-up in ms", &mut k.heads_up_in_ms, (0.0, 600.0)),
                        ("heads-up out ms", &mut k.heads_up_out_ms, (0.0, 600.0)),
                        (
                            "heads-up hold ms",
                            &mut k.heads_up_hold_ms,
                            (500.0, 10000.0),
                        ),
                    ] {
                        changed_here |= knob(ui, cx, label, value, range);
                    }
                });
                ui.collapsing("A7 widgets", |ui| {
                    for (label, value, range) in [
                        ("press ms", &mut k.press_ms, (0.0, 400.0)),
                        ("press release ms", &mut k.press_release_ms, (0.0, 400.0)),
                        ("press scale", &mut k.press_scale, (0.85, 1.0)),
                        ("switch ms", &mut k.switch_ms, (0.0, 400.0)),
                        ("crossfade ms", &mut k.crossfade_ms, (0.0, 600.0)),
                        ("theme fade ms", &mut k.theme_fade_ms, (0.0, 800.0)),
                    ] {
                        changed_here |= knob(ui, cx, label, value, range);
                    }
                    // A long press is **a widget behaviour**, so this is its place — on the button row
                    // above it would make eight, which do not fit on one line.
                    if BigButton::new("hold 2 s")
                        .long_press(Duration::from_secs(2))
                        .show(ui, &mut cx.widgets())
                        .completed
                    {
                        cx.shell.toast("long press!");
                    }
                });
            });
    }
    if changed_here {
        shared.dirty = true;
    }
}

/// The pinned button row.
fn lab_buttons(ui: &mut egui::Ui, cx: &mut Cx<'_>, guard: &mut Shared) {
    let shared: &mut Shared = guard;
    ui.horizontal_wrapped(|ui| {
        // Keep the labels short — eight have to fit on **one line** for the knob list to have room.
        let label = if shared.playing { "stop" } else { "play" };
        if BigButton::new(label)
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            shared.playing = !shared.playing;
        }
        if BigButton::new("export")
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            shared.export = true;
        }
        if BigButton::new("shade")
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            cx.shell.toggle_overlay();
        }
        if BigButton::new("child")
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            cx.open("child");
        }
        if BigButton::new("toast")
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            cx.shell.toast("hello from motion lab");
        }
        if BigButton::new("notify")
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            cx.shell.notify(
                fairing::Notification::new(fairing::NotificationId::of("lab"), "Motion lab")
                    .body("heads-up banner check"),
            );
        }
        if BigButton::new("home")
            .show(ui, &mut cx.widgets())
            .response
            .clicked()
        {
            cx.shell.home();
        }
    });
}
