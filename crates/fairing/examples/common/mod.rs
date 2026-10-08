//! The pieces the examples (`demo.rs` · `kiosk.rs` · `custom_chrome.rs`) share — the stderr logger,
//! argument parsing, installing a Hangul font, the screenshot tour driver (a synthetic finger
//! included) and a PNG encoder written with std alone.
//!
//! Each example gives only its own **script** (a list of [`Act`]s) and its shell-building function,
//! and takes the rest from here. This module is compiled whole into each example through `mod
//! common;`, so a different part of it is used by each — which is why `dead_code` is lifted for the
//! whole file (this is not library code).
#![allow(dead_code)]

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput};
use fairing::notify::Level;
use fairing::runner::{self, Options};
use fairing::workspace::StackTransition;
use fairing::{LaunchAction, Notification, NotificationId};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("[{}] {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

static LOGGER: StderrLogger = StderrLogger;

/// Install the logger once (no `env_logger`: the examples add no dependency).
pub fn init_logger() {
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
}

/// Find a Hangul font installed on the device and load it.
///
/// The crate goes as far as finding the path ([`fairing::fonts::korean_font`]) — which font to load
/// under which name is the app's, and rightly so. But **those ten lines of loading came out identical
/// in every example** (`demo.rs` and `kiosk.rs`), so they were brought down here. It is not a hole in
/// the crate's API but a share common to the examples.
///
/// Where none is found, or it cannot be read, the set is empty, egui's default fonts are used as they
/// are and Hangul comes out as □ — **it warns and carries on.** The old `kiosk.rs` version swallowed
/// that failure whole, so an unreadable font left nothing but □ for no stated reason.
///
/// It takes no arguments and reads `std::env::args()` directly — all three callers are in the middle
/// of a shell builder, and there is no reason to carry `args` that far.
/// The Korean face the examples carry — Noto Sans KR, cut to Hangul, Latin and the symbols a
/// panel uses (`assets/fonts/README.md`), **built into the example binaries** so an example
/// cannot draw tofu wherever it is run from: not the source tree, not a machine with a font of
/// its own. 2.8 MB a weight is the price of a demo that cannot break.
const BUNDLED_KO: &[u8] = include_bytes!("../../../../assets/fonts/NotoSansKR-Regular.ttf");
/// Its bold, for [`fairing::fonts::FontFamilies::Strong`].
const BUNDLED_KO_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/NotoSansKR-Bold.ttf");

pub fn korean_fonts() -> fairing::FontSet {
    let mut fonts = fairing::FontSet::new();
    // `--no-font` — the diagnostic switch that separates out what the font fallback does to the rendering.
    if std::env::args().any(|a| a == "--no-font") {
        log::info!("--no-font: using the egui default fonts only");
        return fonts;
    }
    // The system's Korean font, or the one built in: a machine with no Korean font
    // of its own used to draw Hangul — and `₩`, which the Latin faces lack — as tofu, and that
    // machine is the usual one an example is first run on.
    let system =
        fairing::fonts::korean_font().and_then(|path| {
            match fairing::FontSource::from_path("ko", &path) {
                Ok(source) => {
                    log::info!("Korean font: {}", path.display());
                    Some(source)
                }
                Err(err) => {
                    log::warn!("cannot read the Korean font: {err}");
                    None
                }
            }
        });
    if let Some(source) = system {
        fonts.push(source);
    } else {
        log::info!("Korean font: built in (Noto Sans KR subset)");
        fonts.push(fairing::FontSource::from_static("ko", BUNDLED_KO));
    }
    // The bold face for `FontFamilies::Strong`. egui's own fonts are one weight (`Ubuntu-Light`), so
    // without this the screen and row titles fall back to the regular face and the whole shell reads
    // flat — see `Theme::strong`. `FontPriority::First` because a bold added as a fallback would only
    // be reached by characters the regular face lacks, which is the opposite of what is wanted.
    let system_bold = fairing::fonts::strong_font().and_then(|path| {
        match fairing::FontSource::from_path("strong", &path) {
            Ok(source) => {
                log::info!("bold font: {}", path.display());
                Some(source)
            }
            Err(err) => {
                log::warn!("cannot read the bold font: {err}");
                None
            }
        }
    });
    if let Some(source) = system_bold {
        fonts.push(
            source
                .families(fairing::fonts::FontFamilies::Strong)
                .priority(fairing::fonts::FontPriority::First),
        );
        // Hangul in the strong family: a system bold is usually a Latin face (DejaVu,
        // Liberation), and a Korean title or a price in won set in it fell through to tofu.
        // The built-in Korean bold goes in **behind** it, so the Latin bold keeps its glyphs
        // and Hangul finds a bold of its own.
        fonts.push(
            fairing::FontSource::from_static("strong-ko", BUNDLED_KO_BOLD)
                .families(fairing::fonts::FontFamilies::Strong),
        );
    } else {
        log::info!("bold font: built in (Noto Sans KR Bold subset)");
        fonts.push(
            fairing::FontSource::from_static("strong", BUNDLED_KO_BOLD)
                .families(fairing::fonts::FontFamilies::Strong)
                .priority(fairing::fonts::FontPriority::First),
        );
    }
    fonts
}

/// What the examples' own screenshot commands carry. The shell asks for screenshots too — a card's
/// frosted backdrop takes one as it opens — and the images come back as the same
/// `Event::Screenshot`, so the examples keep to the ones marked as theirs.
#[derive(Debug, Clone, Copy)]
struct OurShot;

/// Ask eframe for a screenshot marked as the examples' own.
fn ask_for_shot(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
        OurShot,
    )));
}

/// The images that rode this frame's input back for [`ask_for_shot`]; the shell's are left alone.
fn our_shots(ctx: &egui::Context) -> Vec<Arc<egui::ColorImage>> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Screenshot {
                    image, user_data, ..
                } if user_data
                    .data
                    .as_ref()
                    .is_some_and(|data| data.is::<OurShot>()) =>
                {
                    Some(Arc::clone(image))
                }
                _ => None,
            })
            .collect()
    })
}

/// What a recorded frame's screenshot carries ([`Act::Record`]): the file it is written to.
#[derive(Debug, Clone)]
struct FrameShot(PathBuf);

/// Ask for a screenshot of this frame, bound for `path`.
fn ask_for_frame(ctx: &egui::Context, path: PathBuf) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
        FrameShot(path),
    )));
}

/// The recorded frames that rode this frame's input back, with the file each is bound for.
fn frame_shots(ctx: &egui::Context) -> Vec<(PathBuf, Arc<egui::ColorImage>)> {
    ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Screenshot {
                    image, user_data, ..
                } => user_data
                    .data
                    .as_ref()
                    .and_then(|data| data.downcast_ref::<FrameShot>())
                    .map(|FrameShot(path)| (path.clone(), Arc::clone(image))),
                _ => None,
            })
            .collect()
    })
}

/// Take **one frame** with no tour — for an example with a frame loop of its own.
///
/// [`run_tour`] asks for a "closure that builds the shell", and an example that holds the shell itself
/// and does something different every frame — recording frame times, the replay loop, the graph, as
/// `motion_lab` does — does not fit into it. So those examples sat there **with nobody able to see
/// them**.
///
/// eframe's screenshot is N → N+2 (see `Tour`'s docs). Here there is only the one, so it is requested
/// on frame `after` and written when the event arrives, then the window closes.
pub struct OneShot {
    dir: PathBuf,
    name: &'static str,
    after: u32,
    frame: u32,
    asked: bool,
}

impl OneShot {
    /// Wait `after` frames and take one into `dir/name`.
    #[must_use]
    pub fn new(dir: PathBuf, name: &'static str, after: u32) -> Self {
        Self {
            dir,
            name,
            after,
            frame: 0,
            asked: false,
        }
    }

    /// Called at the end of every frame. Once it has taken its shot, it closes the window.
    pub fn tick(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        ctx.request_repaint();
        if !self.asked {
            if self.frame >= self.after {
                ask_for_shot(ctx);
                self.asked = true;
            }
            return;
        }
        let Some(image) = our_shots(ctx).into_iter().next() else {
            return;
        };
        if let Err(err) = std::fs::create_dir_all(&self.dir) {
            log::error!("cannot create {}: {err}", self.dir.display());
        }
        let path = self.dir.join(self.name);
        match write_png(&path, &image) {
            Ok(bytes) => log::info!(
                "{}: {}x{}, {bytes} bytes",
                self.name,
                image.width(),
                image.height()
            ),
            Err(err) => log::error!("cannot write {}: {err}", path.display()),
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// `--screen <id>` / `--screen=<id>` — the screen to leave open at startup. Used with `--shot` it can
/// capture a screen other than home.
pub fn arg_screen(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if let Some(id) = arg.strip_prefix("--screen=") {
            return Some(id.to_owned());
        }
        if arg == "--screen" {
            return it.next().cloned();
        }
    }
    None
}

/// `--shot <dir>` — take one frame only ([`OneShot`]).
pub fn arg_shot(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if let Some(dir) = arg.strip_prefix("--shot=") {
            return Some(PathBuf::from(dir));
        }
        if arg == "--shot" {
            return it.next().map(PathBuf::from);
        }
    }
    None
}

/// `--size=1024x600`.
pub fn arg_size(args: &[String]) -> Option<(f32, f32)> {
    let raw = args.iter().find_map(|a| a.strip_prefix("--size="))?;
    let (w, h) = raw.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// `--panel-mm=154x86` — the panel's physical size (mm). Given one, the shell settles the scale at that density.
pub fn arg_panel_mm(args: &[String]) -> Option<(f32, f32)> {
    let raw = args.iter().find_map(|a| a.strip_prefix("--panel-mm="))?;
    let (w, h) = raw.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// `--finger-mm=13` — the finger's diameter (mm). Not given, the policy default (bare, 9 mm).
pub fn arg_finger_mm(args: &[String]) -> Option<f32> {
    args.iter()
        .find_map(|a| a.strip_prefix("--finger-mm="))?
        .parse()
        .ok()
}

/// `--legacy` — draw at the pre-M2b metrics (`MetricsSpec::legacy_du`). For comparison.
pub fn arg_legacy(args: &[String]) -> bool {
    args.iter().any(|a| a == "--legacy")
}

/// `--tour <dir>`, or `FAIRING_TOUR_DIR`.
pub fn arg_tour(args: &[String]) -> Option<PathBuf> {
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if let Some(dir) = arg.strip_prefix("--tour=") {
            return Some(PathBuf::from(dir));
        }
        if arg == "--tour" {
            return it.next().map(PathBuf::from);
        }
    }
    std::env::var_os("FAIRING_TOUR_DIR").map(PathBuf::from)
}

/// One step of a tour. An example writes its script out of this list.
#[derive(Debug, Clone, Copy)]
pub enum Act {
    /// Until the animations and transitions finish (capped at [`SETTLE_MAX`] frames).
    Settle,
    /// Wait N frames.
    Wait(u32),
    /// Capture this frame.
    Shot(&'static str),
    /// Keep the frame where the A2 (home ↔ task) transition's **raw `t`** is closest to the target.
    HomeMid(f32, &'static str),
    /// Keep the frame where the A3 (push/pop) transition's **eased `s`** is closest to the target
    /// (reckoned on A3's `x_in = (1 − s)·W` — at `s = 0.5` the incoming layer is half the screen).
    StackMid(f32, &'static str),
    /// Open a screen (an A2 from home, an A3 push over a screen).
    Open(&'static str),
    /// Open a screen in the other pane — a split from one pane (A8), a push on the other pane
    /// from two.
    OpenBeside(&'static str),
    /// Press on the split's divider handle, wherever it is — an [`Act::MoveTo`] after this drags it.
    /// With no split up it only logs and moves on.
    GrabDivider,
    /// The recents control — `LaunchAction::OpenOverview`.
    Recents,
    /// The split control — `LaunchAction::ToggleSplit`.
    SplitControl,
    /// Back (an A3 pop, or an A2 close at the root).
    Back,
    /// Home (an A2 close).
    Home,
    /// A synthetic finger press at `(x, y)` — it rides the next pass's `RawInput` ([`SyntheticInput`]).
    Press(f32, f32),
    /// Move in a straight line to `(x, y)` over `frames` frames while held (without releasing). A
    /// **mid-transition frame** of a shade pull, a page swipe or the back gesture puts an [`Act::Shot`]
    /// after this.
    MoveTo {
        /// The target x.
        x: f32,
        /// The target y.
        y: f32,
        /// How many frames it takes (at 60 Hz, `step ≈ distance / frames` px per frame → the release velocity).
        frames: u32,
    },
    /// Release (a `PointerButton` release plus `PointerGone`).
    Release,
    /// A tap at `(x, y)`: press, then release on the next frame.
    Tap(f32, f32),
    /// A toast (`Shell::toast`).
    Toast(&'static str),
    /// A notification, `(level, title, body)` (`Shell::notify`; the id is the title's FNV). With the
    /// shade closed it is a heads-up. The level picks the severity shape as well as the colour.
    Notify(Level, &'static str, &'static str),
    /// Replace the OSK's layout (`"qwerty"` · `"numpad"` · `"hangul"`). An unknown name only logs.
    Osk(&'static str),
    /// Knock one OSK key **by its label** (`"ㅎ"` · `" "` · `"⌫"`). Where it is comes from
    /// `shell.osk().key_rect(label)` — no coordinates are written into the script. Where the key is
    /// not on the current face it only logs and moves on.
    Key(&'static str),
    /// **Start recording** into `<tour dir>/<name>/`: from here every other frame is written as
    /// `0000.png`, `0001.png`, … on `--record`'s fixed 60 Hz clock ([`VirtualClock`]), so the
    /// frames stand 1/30 s apart however slowly the software rasteriser draws them. Without
    /// `--record` it does nothing, and the stills-only tour stays fast. A [`Act::Shot`] inside a
    /// recording is fine: both screenshots go out in the same frame.
    Record(&'static str),
    /// Stop recording.
    RecordEnd,
    /// Tap a PIN in on the shell's keypad, digit by digit — a press on one frame, the release on
    /// the next. Where each key is comes from `shell.prompt_digit_rect(..)`, so a shuffled keypad
    /// is tapped right too. With no keypad up it only logs and moves on.
    Pin(&'static str),
    /// Tap the prompt's tab for its method `n` (from 0) — PIN, pattern, … in the order the
    /// authenticator offers them. With no tabs up it only logs and moves on.
    PromptTab(usize),
    /// Draw a pattern on the shell's pattern pad: press on the first dot, slide through the rest
    /// a few frames a stroke, release on the last. The dots count from 1, row by row, as
    /// `[access.pattern_table]` writes them; where each is comes from `shell.prompt_dot_center`.
    Pattern(&'static [u8]),
    /// [`Act::Pattern`] without the release: the finger stays on the last dot, for a shot of the
    /// path being drawn. An [`Act::Release`] lifts it and submits.
    PatternHold(&'static [u8]),
    /// `LaunchAction::Lock` — in `prompt` mode, the lock screen.
    Lock,
    /// `LaunchAction::Logout` — the session back to the subject it started as.
    Logout,
    /// A finger down on the desktop icon `id`, wherever it is drawn (`shell.desktop().icon_rect`),
    /// held until an [`Act::Release`] — a long press, with an [`Act::Wait`] between. With no such
    /// icon on screen it only logs and moves on.
    PressIcon(&'static str),
}

// ─────────────────────────────────────────────────────────────────────────────
// Synthetic input (the eframe path)
// ─────────────────────────────────────────────────────────────────────────────

/// The frames [`Act::Pattern`] takes from one dot to the next.
const PATTERN_STROKE: usize = 3;

/// The egui plugin that attaches synthetic pointer events to **the next pass's `RawInput`**.
///
/// `ctx.input_mut(|i| i.events.push(..))` will not do — egui 0.36.1 replaces `InputState::events`
/// wholesale with `RawInput`'s on each pass (`begin_pass`), and refreshes the `pointer` state (the
/// press, the position, the velocity) only from `RawInput` events at that moment. So they have to go
/// in the same place the headless harness (`fairing::testing::Harness`) puts them, `RawInput.events`,
/// for the gesture engine (`ctx.input().pointer`) to see them. The queue lives in `ctx.data`, so the
/// plugin is an empty type (no lock types; the same frame as `crate::osk::inject`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SyntheticInput;

/// **`--record`'s clock.** Each frame is exactly [`RECORD_STEP`] after the one before, however long
/// it took to draw, so every animation advances by the same step on every frame and a recording of
/// it comes out smooth. The shell's time is the sum of egui's `stable_dt`, which follows
/// `RawInput.time` while repaints are requested, so setting that time is all it takes. The clock
/// starts from the real time it takes over at, so time never runs backwards.
#[derive(Debug, Default, Clone, Copy)]
struct VirtualClock {
    on: bool,
    time: Option<f64>,
}

/// One frame of [`VirtualClock`] — the 60 Hz the tour scripts are written for (a `MoveTo` over
/// `frames` frames releases at `distance / frames × 60` px/s).
const RECORD_STEP: f64 = 1.0 / 60.0;
/// The same step, as `RawInput::predicted_dt` takes it.
const RECORD_DT: f32 = 1.0 / 60.0;

impl SyntheticInput {
    fn queue_id() -> egui::Id {
        egui::Id::new("fairing.examples.synthetic_input")
    }

    fn clock_id() -> egui::Id {
        egui::Id::new("fairing.examples.virtual_clock")
    }

    /// Put the frames on [`VirtualClock`] from the next one on.
    fn start_clock(ctx: &egui::Context) {
        ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<VirtualClock>(Self::clock_id())
                .on = true;
        });
    }

    /// Register the plugin once (the same type is not taken twice, but a flag is checked first so as
    /// not to make an `Arc`).
    pub fn install(ctx: &egui::Context) {
        let installed = egui::Id::new("fairing.examples.synthetic_input.installed");
        if ctx.data(|d| d.get_temp::<bool>(installed)) == Some(true) {
            return;
        }
        ctx.add_plugin(Self);
        ctx.data_mut(|d| d.insert_temp(installed, true));
    }

    /// Put one event into the next pass.
    pub fn push(ctx: &egui::Context, event: Event) {
        ctx.data_mut(|d| {
            let queue: &mut Vec<Event> = d.get_temp_mut_or_default(Self::queue_id());
            queue.push(event);
        });
        ctx.request_repaint();
    }

    /// A press (`PointerMoved` plus `PointerButton { pressed: true }`).
    pub fn press(ctx: &egui::Context, pos: Pos2) {
        Self::push(ctx, Event::PointerMoved(pos));
        Self::push(
            ctx,
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::default(),
            },
        );
    }

    /// A move.
    pub fn move_to(ctx: &egui::Context, pos: Pos2) {
        Self::push(ctx, Event::PointerMoved(pos));
    }

    /// A release (`PointerButton { pressed: false }` plus `PointerGone`).
    pub fn release(ctx: &egui::Context, pos: Pos2) {
        Self::push(
            ctx,
            Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::default(),
            },
        );
        Self::push(ctx, Event::PointerGone);
    }
}

impl egui::Plugin for SyntheticInput {
    fn debug_name(&self) -> &'static str {
        "fairing.examples.synthetic_input"
    }

    fn input_hook(&mut self, ctx: &egui::Context, input: &mut RawInput) {
        // **Pin the window as focused.** The tour runs on Xvfb with no window manager, where winit
        // never once sends `Focused(true)`. egui draws the caret and the IME composition underline only
        // while `input.focused` is true (`viewport_has_focus` in `text_edit/builder.rs`), so without
        // turning this on the caret and the composition marking drop out of the text-field pictures
        // entirely — what is visible on a real device would be missing from the documentation images
        // alone.
        input.focused = true;
        ctx.data_mut(|d| {
            let clock: &mut VirtualClock = d.get_temp_mut_or_default(Self::clock_id());
            if clock.on {
                let time = clock
                    .time
                    .map_or_else(|| input.time.unwrap_or(0.0), |t| t + RECORD_STEP);
                clock.time = Some(time);
                input.time = Some(time);
                input.predicted_dt = RECORD_DT;
            }
            let queue: &mut Vec<Event> = d.get_temp_mut_or_default(Self::queue_id());
            if !queue.is_empty() {
                input.events.append(queue);
            }
        });
    }
}

/// How many frames `Settle` gives up after (a transition is 220 ms ≈ 14 frames, so it is generous).
const SETTLE_MAX: u32 = 240;

/// How many frames a mid-transition shot waits for the transition to start before giving up.
const MID_MAX: u32 = 240;

/// The progress window a mid-transition shot is taken in (the target ± this). On a software rasteriser
/// the progress jumps by more than 0.2 in a frame, so the target cannot be hit exactly — several are
/// taken around it and only the closest kept.
const MID_WINDOW: f32 = 0.3;

/// A mid-transition shot in progress.
struct Mid {
    /// The target progress.
    at: f32,
    /// The file name.
    name: &'static str,
    /// The progress of the image kept so far (none means not one has been kept yet).
    best: Option<f32>,
}

/// One step's result.
enum Flow {
    /// On to the next step within the same frame.
    Next,
    /// Wait for the next frame.
    Wait,
}

/// The app that runs the tour. The shell is built on the first frame the window exists (`Shell::new`
/// asks for a `Context`).
///
/// # The one-frame delay
/// `eframe` 0.36.1's glow backend gathers the viewport commands at the end of a frame
/// (`handle_viewport_output`, `glow_integration.rs` line 864) and reads a screenshot with
/// `read_screen_rgba` (line 806) **after drawing the next frame**. So the picture of a command sent on
/// frame N is frame N+1's screen, and the `egui::Event::Screenshot` rides frame N+2's input.
/// [`Tour::last_t`] holds the previous frame's progress (= the frame captured), so which `t` was
/// captured is known exactly.
struct Tour<S> {
    dir: PathBuf,
    /// The script (given by the example).
    plan: &'static [Act],
    /// The shell-building function (given by the example). Called **once**, on the first frame the
    /// window exists — it is an `FnOnce` because some examples stand a different shell up per form.
    build: Option<BuildApp<S>>,
    shell: Option<fairing::Shell>,
    /// The app's own state, once `build` has made it.
    state: Option<S>,
    step: usize,
    frames: u32,
    /// An ordinary shot: the file name whose screenshot command has been sent and whose image is awaited.
    pending: Option<&'static str>,
    /// The mid-transition shot in progress.
    mid: Option<Mid>,
    /// How many screenshots have not come back yet.
    inflight: u32,
    /// The previous frame's transition progress (= the value for the frame the image now returning was captured on).
    last_t: Option<f32>,
    /// Whether the mid-transition shot has seen the transition even once.
    saw: bool,
    /// Where the synthetic finger is now (after a press). [`Act::MoveTo`]'s starting point.
    finger: Pos2,
    /// The starting point of the [`Act::MoveTo`] in progress.
    move_from: Option<Pos2>,
    written: u32,
    failed: bool,
    done: bool,
    /// `--record`'s state, `None` without it.
    film: Option<Film>,
}

/// `--record`: the frames run on [`VirtualClock`] and [`Act::Record`] writes them.
#[derive(Default)]
struct Film {
    /// The recording in progress.
    recording: Option<Recording>,
    /// Frames asked for and not back yet — the window stays open for them.
    inflight: u32,
    /// Frames written, in all.
    written: u32,
}

/// An [`Act::Record`] in progress.
struct Recording {
    dir: PathBuf,
    /// Frames since it started; every other one is kept, for 30 frames a second.
    tick: u32,
    /// The number the next kept frame is written under.
    next: u32,
}

/// The function that builds the shell for a tour (given by the example).
pub type BuildShell = Box<dyn FnOnce(&egui::Context) -> fairing::Result<fairing::Shell>>;

/// The same, for an example whose app owns state of its own. The tour holds it and lends
/// it to the screens each frame through `Shell::frame_with`, exactly as `runner::run_app` does.
pub type BuildApp<S> = Box<dyn FnOnce(&egui::Context) -> fairing::Result<(fairing::Shell, S)>>;

/// Run the script, leaving PNGs behind, and close the window at the end.
///
/// # Errors
/// A [`fairing::Error`] where the output directory cannot be made or the runner fails.
pub fn run_tour<S: std::any::Any>(
    options: Options,
    dir: PathBuf,
    plan: &'static [Act],
    build: impl FnOnce(&egui::Context) -> fairing::Result<(fairing::Shell, S)> + 'static,
) -> fairing::Result<()> {
    std::fs::create_dir_all(&dir).map_err(|err| {
        fairing::Error::Runner(format!(
            "cannot create the tour directory {}: {err}",
            dir.display()
        ))
    })?;
    log::info!("tour started - output {}", dir.display());
    let film = std::env::args()
        .any(|arg| arg == "--record")
        .then(Film::default);
    if film.is_some() {
        log::info!("--record: a fixed 60 Hz clock, and frames for every Act::Record");
    }
    let mut tour = Tour {
        dir,
        plan,
        build: Some(Box::new(build)),
        shell: None,
        step: 0,
        frames: 0,
        pending: None,
        mid: None,
        inflight: 0,
        last_t: None,
        saw: false,
        finger: Pos2::ZERO,
        move_from: None,
        written: 0,
        failed: false,
        done: false,
        state: None,
        film,
    };
    runner::run(options, move |ui| tour.frame(ui))
}

impl<S: std::any::Any> Tour<S> {
    /// Every frame: draw the shell, write any screenshot that has arrived, and advance the script.
    fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.shell.is_none() && !self.start(&ctx) {
            return;
        }
        if let (Some(shell), Some(state)) = (self.shell.as_mut(), self.state.as_mut()) {
            shell.frame_with(ui, state);
            for event in shell.poll_events() {
                log::debug!("shell event: {event:?}");
            }
        }
        self.collect(&ctx);
        self.develop(&ctx);
        self.advance(&ctx);
        self.film(&ctx);
        // Remember this frame's progress — the image coming back next frame is this very screen.
        self.last_t = self.progress();
        // The script runs frame by frame, so the idle-0-fps policy is overridden for now.
        ctx.request_repaint();
    }

    /// The first frame: pin the DPI to 1.0 and build the shell. On a failure it closes the window.
    fn start(&mut self, ctx: &egui::Context) -> bool {
        ctx.set_pixels_per_point(1.0);
        SyntheticInput::install(ctx);
        if self.film.is_some() {
            SyntheticInput::start_clock(ctx);
        }
        let Some(build) = self.build.take() else {
            return false;
        };
        match build(ctx) {
            Ok((shell, state)) => {
                self.shell = Some(shell);
                self.state = Some(state);
                true
            }
            Err(err) => {
                log::error!("cannot build the tour shell: {err}");
                self.failed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                false
            }
        }
    }

    /// The progress of the transition running right now. It hands back A2's as the **raw `t`** and A3's
    /// as the **eased `s`** — exactly the two argument conventions of `workspace::transition` (`a2_open`
    /// takes a raw `t` and eases it inside, while `a3_push` / `a3_pop` take an already eased value).
    /// With neither, `None`.
    fn progress(&self) -> Option<f32> {
        let shell = self.shell.as_ref()?;
        let home = shell.workspace().home_transition();
        if home.is_active() {
            return Some(home.t());
        }
        let motion = shell.theme().motion;
        match shell.workspace().stack_transition() {
            StackTransition::Idle => None,
            StackTransition::Pushing { t } => Some(motion.push.easing.apply(t.value())),
            StackTransition::Popping { t, .. } => Some(motion.pop.easing.apply(t.value())),
            // The back gesture is s = p (no easing, A3).
            StackTransition::DraggingBack { p, .. } => Some(p.value()),
        }
    }

    /// Write the `Event::Screenshot` that rode this frame's input out as a PNG.
    fn collect(&mut self, ctx: &egui::Context) {
        for image in our_shots(ctx) {
            self.inflight = self.inflight.saturating_sub(1);
            let shot_t = self.last_t;
            if self.mid.is_some() {
                self.keep_best_mid(&image, shot_t);
                continue;
            }
            let Some(name) = self.pending.take() else {
                log::warn!("a screenshot arrived that nobody asked for - dropping it");
                continue;
            };
            self.save(name, &image, None, true);
        }
    }

    /// [`Act::Record`]: ask for this frame, every other frame, while a recording runs.
    fn film(&mut self, ctx: &egui::Context) {
        let Some(film) = self.film.as_mut() else {
            return;
        };
        let Some(rec) = film.recording.as_mut() else {
            return;
        };
        rec.tick += 1;
        if rec.tick % 2 == 1 {
            ask_for_frame(ctx, rec.dir.join(format!("{:04}.png", rec.next)));
            rec.next += 1;
            film.inflight += 1;
        }
    }

    /// Write the recorded frames that came back this frame.
    fn develop(&mut self, ctx: &egui::Context) {
        let Some(film) = self.film.as_mut() else {
            return;
        };
        for (path, image) in frame_shots(ctx) {
            film.inflight = film.inflight.saturating_sub(1);
            match write_png(&path, &image) {
                Ok(_) => film.written += 1,
                Err(err) => {
                    self.failed = true;
                    log::error!("cannot write {}: {err}", path.display());
                }
            }
        }
    }

    /// [`Act::Record`] (`Some(name)`) and [`Act::RecordEnd`] (`None`). Without `--record` both are
    /// nothing at all.
    fn record_step(&mut self, name: Option<&'static str>) -> Flow {
        if let Some(film) = self.film.as_mut() {
            if let Some(rec) = film.recording.take() {
                log::info!("{} frames for {}", rec.next, rec.dir.display());
            }
            if let Some(name) = name {
                let dir = self.dir.join(name);
                match std::fs::create_dir_all(&dir) {
                    Ok(()) => {
                        log::info!("recording {name}");
                        film.recording = Some(Recording {
                            dir,
                            tick: 0,
                            next: 0,
                        });
                    }
                    Err(err) => {
                        self.failed = true;
                        log::error!("cannot create {}: {err}", dir.display());
                    }
                }
            }
        }
        self.next_step();
        Flow::Next
    }

    /// A mid-transition shot: overwrite where the image is closer to the target.
    fn keep_best_mid(&mut self, image: &egui::ColorImage, shot_t: Option<f32>) {
        let Some(mid) = self.mid.as_ref() else {
            return;
        };
        let Some(t) = shot_t else {
            // This was captured outside the transition (it had already finished) — thrown away.
            return;
        };
        let better = mid
            .best
            .is_none_or(|best| (t - mid.at).abs() < (best - mid.at).abs());
        if !better {
            return;
        }
        let (name, at, first) = (mid.name, mid.at, mid.best.is_none());
        self.save(name, image, Some((t, at)), first);
        if let Some(mid) = self.mid.as_mut() {
            mid.best = Some(t);
        }
    }

    fn save(&mut self, name: &str, image: &egui::ColorImage, mid: Option<(f32, f32)>, first: bool) {
        let path = self.dir.join(name);
        match write_png(&path, image) {
            Ok(bytes) => {
                let [w, h] = image.size;
                if first {
                    self.written += 1;
                }
                match mid {
                    Some((t, at)) => {
                        log::info!(
                            "{name}: {w}x{h}, {bytes} bytes, transition t = {t:.3} (target {at:.2})"
                        );
                    }
                    None => log::info!("{name}: {w}x{h}, {bytes} bytes"),
                }
            }
            Err(err) => {
                self.failed = true;
                log::error!("cannot write {name}: {err}");
            }
        }
    }

    /// Advance the script. It stops while awaiting an ordinary shot's image.
    fn advance(&mut self, ctx: &egui::Context) {
        if self.pending.is_some() {
            return;
        }
        loop {
            let Some(act) = self.plan.get(self.step).copied() else {
                // A recording left running ends with the script; its last frames are still on
                // their way, so the window waits for them.
                if let Some(film) = self.film.as_mut() {
                    film.recording = None;
                    if film.inflight > 0 {
                        return;
                    }
                }
                if !self.done {
                    self.done = true;
                    let frames = self.film.as_ref().map_or_else(String::new, |film| {
                        format!(", {} recorded frames", film.written)
                    });
                    if self.failed {
                        log::error!(
                            "tour finished - {} PNGs{frames}, some shots failed",
                            self.written
                        );
                    } else {
                        log::info!("tour finished - {} PNGs{frames}", self.written);
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            };
            match self.act(act, ctx) {
                Flow::Next => {}
                Flow::Wait => return,
            }
        }
    }

    fn next_step(&mut self) {
        self.step += 1;
        self.frames = 0;
        self.saw = false;
        self.mid = None;
        self.move_from = None;
    }

    fn request_shot(&mut self, ctx: &egui::Context) {
        ask_for_shot(ctx);
        self.inflight += 1;
    }

    /// One step.
    fn act(&mut self, act: Act, ctx: &egui::Context) -> Flow {
        self.frames += 1;
        let Some(shell) = self.shell.as_ref() else {
            return Flow::Wait;
        };
        match act {
            Act::Settle => {
                let settled = !shell.is_animating() && self.frames >= 2;
                if !settled && self.frames < SETTLE_MAX {
                    return Flow::Wait;
                }
                if !settled {
                    log::warn!("Settle did not finish within {SETTLE_MAX} frames - moving on");
                }
                self.next_step();
                Flow::Next
            }
            Act::Wait(n) => {
                if self.frames < n {
                    return Flow::Wait;
                }
                self.next_step();
                Flow::Next
            }
            Act::Shot(name) => {
                self.request_shot(ctx);
                self.pending = Some(name);
                self.next_step();
                Flow::Wait
            }
            Act::HomeMid(at, name) | Act::StackMid(at, name) => self.mid_step(ctx, at, name),
            Act::Open(id) => self.with_shell(|shell| shell.launch(LaunchAction::open(id))),
            Act::OpenBeside(id) => self.with_shell(|shell| {
                shell.launch(LaunchAction::Open {
                    id: id.to_owned(),
                    in_other_pane: true,
                });
            }),
            Act::Recents => self.with_shell(|shell| shell.launch(LaunchAction::OpenOverview)),
            Act::SplitControl => self.with_shell(|shell| shell.launch(LaunchAction::ToggleSplit)),
            Act::GrabDivider => self.grab_divider_step(ctx),
            Act::Back => self.with_shell(fairing::Shell::back),
            Act::Home => self.with_shell(fairing::Shell::home),
            Act::Press(x, y) => {
                self.finger = egui::pos2(x, y);
                SyntheticInput::press(ctx, self.finger);
                self.next_step();
                Flow::Wait
            }
            Act::MoveTo { x, y, frames } => self.move_step(ctx, egui::pos2(x, y), frames),
            Act::Release => {
                SyntheticInput::release(ctx, self.finger);
                self.next_step();
                Flow::Wait
            }
            Act::Tap(x, y) => {
                // The press is this frame (→ the next pass) and the release the frame after.
                let pos = egui::pos2(x, y);
                if self.frames == 1 {
                    self.finger = pos;
                    SyntheticInput::press(ctx, pos);
                    return Flow::Wait;
                }
                SyntheticInput::release(ctx, pos);
                self.next_step();
                Flow::Wait
            }
            Act::Toast(text) => self.with_shell(|shell| shell.toast(text)),
            Act::Notify(level, title, body) => self.with_shell(|shell| {
                shell.notify(
                    Notification::new(NotificationId::of(title), title)
                        .body(body)
                        .level(level),
                );
            }),
            Act::Osk(name) => {
                self.with_shell(
                    |shell| match fairing::osk::OskLayout::parse(name, true, false) {
                        Some(layout) => shell.osk_mut().set_layout(layout),
                        None => log::warn!("Act::Osk(\"{name}\") - unknown layout"),
                    },
                )
            }
            Act::Key(label) => self.key_step(ctx, label),
            Act::Record(name) => self.record_step(Some(name)),
            Act::RecordEnd => self.record_step(None),
            Act::Pin(digits) => self.pin_step(ctx, digits),
            Act::PromptTab(index) => self.tab_step(ctx, index),
            Act::Pattern(dots) => self.pattern_step(ctx, dots, true),
            Act::PatternHold(dots) => self.pattern_step(ctx, dots, false),
            Act::Lock => self.with_shell(|shell| shell.launch(LaunchAction::Lock)),
            Act::Logout => self.with_shell(|shell| shell.launch(LaunchAction::Logout)),
            Act::PressIcon(id) => self.press_icon_step(ctx, id),
        }
    }

    /// [`Act::PressIcon`]: a finger down on the icon, where the desktop drew it.
    fn press_icon_step(&mut self, ctx: &egui::Context, id: &str) -> Flow {
        let Some(icon) = self.shell.as_ref().and_then(|s| s.desktop().icon_rect(id)) else {
            log::warn!("Act::PressIcon(\"{id}\") - no such icon on screen");
            self.next_step();
            return Flow::Next;
        };
        self.finger = icon.center();
        SyntheticInput::press(ctx, self.finger);
        self.next_step();
        Flow::Wait
    }

    /// [`Act::GrabDivider`]: a finger down on the split's divider.
    fn grab_divider_step(&mut self, ctx: &egui::Context) -> Flow {
        let Some(handle) = self
            .shell
            .as_ref()
            .and_then(|s| s.workspace().divider_rect())
        else {
            log::warn!("Act::GrabDivider - no divider on screen");
            self.next_step();
            return Flow::Next;
        };
        self.finger = handle.center();
        SyntheticInput::press(ctx, self.finger);
        self.next_step();
        Flow::Wait
    }

    /// [`Act::PromptTab`]: the press on this frame, the release on the next.
    fn tab_step(&mut self, ctx: &egui::Context, index: usize) -> Flow {
        if self.frames == 1 {
            let Some(rect) = self.shell.as_ref().and_then(|s| s.prompt_tab_rect(index)) else {
                log::warn!("Act::PromptTab({index}) - no such tab on screen");
                self.next_step();
                return Flow::Next;
            };
            self.finger = rect.center();
            SyntheticInput::press(ctx, self.finger);
            return Flow::Wait;
        }
        SyntheticInput::release(ctx, self.finger);
        self.next_step();
        Flow::Wait
    }

    /// [`Act::Pattern`]: the press on the first dot, [`PATTERN_STROKE`] frames of moves to each dot
    /// after it, and the release where `release` asks for one.
    #[allow(clippy::cast_precision_loss)] // A frame count is a small integer.
    fn pattern_step(&mut self, ctx: &egui::Context, dots: &'static [u8], release: bool) -> Flow {
        let points: Option<Vec<Pos2>> = self.shell.as_ref().and_then(|shell| {
            dots.iter()
                .map(|dot| shell.prompt_dot_center(dot.saturating_sub(1)))
                .collect()
        });
        let Some(points) = points.filter(|p| !p.is_empty()) else {
            log::warn!("Act::Pattern({dots:?}) - no pattern pad on screen");
            self.next_step();
            return Flow::Next;
        };
        if self.frames == 1 {
            self.finger = points.first().copied().unwrap_or(self.finger);
            SyntheticInput::press(ctx, self.finger);
            return Flow::Wait;
        }
        let k = usize::try_from(self.frames - 2).unwrap_or(usize::MAX);
        let stroke = k / PATTERN_STROKE;
        if let (Some(from), Some(to)) = (points.get(stroke), points.get(stroke + 1)) {
            let t = (k % PATTERN_STROKE + 1) as f32 / PATTERN_STROKE as f32;
            self.finger = *from + (*to - *from) * t;
            SyntheticInput::move_to(ctx, self.finger);
            return Flow::Wait;
        }
        if release {
            SyntheticInput::release(ctx, self.finger);
        }
        self.next_step();
        Flow::Wait
    }

    /// [`Act::Pin`]: two frames a digit, the press and then the release.
    fn pin_step(&mut self, ctx: &egui::Context, digits: &'static str) -> Flow {
        let n = usize::try_from(self.frames.saturating_sub(1)).unwrap_or(usize::MAX);
        let (index, release) = (n / 2, n % 2 == 1);
        let Some(digit) = digits.as_bytes().get(index).map(|b| b.wrapping_sub(b'0')) else {
            self.next_step();
            return Flow::Next;
        };
        let Some(rect) = self.shell.as_ref().and_then(|s| s.prompt_digit_rect(digit)) else {
            log::warn!("Act::Pin(\"{digits}\") - no key for {digit} on screen");
            self.next_step();
            return Flow::Next;
        };
        if release {
            SyntheticInput::release(ctx, rect.center());
        } else {
            SyntheticInput::press(ctx, rect.center());
        }
        Flow::Wait
    }

    /// [`Act::Key`]: find the OSK key by label, press it, and release it on the next frame. Where the
    /// label cannot be found it only logs and moves on, so the script does not stop.
    fn key_step(&mut self, ctx: &egui::Context, label: &'static str) -> Flow {
        let Some(rect) = self.shell.as_ref().and_then(|s| s.osk().key_rect(label)) else {
            log::warn!("Act::Key(\"{label}\") - no such key on the current face");
            self.next_step();
            return Flow::Next;
        };
        if self.frames == 1 {
            self.finger = rect.center();
            SyntheticInput::press(ctx, self.finger);
            return Flow::Wait;
        }
        SyntheticInput::release(ctx, self.finger);
        self.next_step();
        Flow::Wait
    }

    /// [`Act::MoveTo`]: move to the next point along the line each frame. On the frame it reaches the
    /// last point, on to the next step (that event applies on the next pass).
    #[allow(clippy::cast_precision_loss)] // A frame count is a small integer.
    fn move_step(&mut self, ctx: &egui::Context, to: Pos2, frames: u32) -> Flow {
        let from = *self.move_from.get_or_insert(self.finger);
        let frames = frames.max(1);
        let k = self.frames.min(frames);
        let t = k as f32 / frames as f32;
        self.finger = from + (to - from) * t;
        SyntheticInput::move_to(ctx, self.finger);
        if k >= frames {
            self.next_step();
        }
        Flow::Wait
    }

    fn with_shell(&mut self, action: impl FnOnce(&mut fairing::Shell)) -> Flow {
        if let Some(shell) = self.shell.as_mut() {
            action(shell);
        }
        self.next_step();
        Flow::Next
    }

    /// A mid-transition shot: capture every frame around the target ([`MID_WINDOW`]) and keep only the
    /// closest. Because of the one-frame delay, "the t right now" cannot settle which screen is captured.
    fn mid_step(&mut self, ctx: &egui::Context, at: f32, name: &'static str) -> Flow {
        if self.mid.is_none() {
            self.mid = Some(Mid {
                at,
                name,
                best: None,
            });
        }
        if let Some(t) = self.progress() {
            self.saw = true;
            // What gets captured is **the next** frame, so the decision looks one step ahead. Where the
            // step is not known yet (a transition's first frame), it captures anyway.
            let predicted = self.last_t.map_or(at, |prev| (t - prev).max(0.0) + t);
            if (predicted - at).abs() <= MID_WINDOW {
                self.request_shot(ctx);
            }
            return Flow::Wait;
        }
        if !self.saw && self.frames < MID_MAX {
            return Flow::Wait;
        }
        if self.inflight > 0 {
            return Flow::Wait; // an image is still on its way
        }
        if self.mid.as_ref().is_some_and(|mid| mid.best.is_none()) {
            log::warn!("{name}: missed the transition - shooting the current frame");
            self.request_shot(ctx);
            self.pending = Some(name);
            self.next_step();
            return Flow::Wait;
        }
        self.next_step();
        Flow::Next
    }
}
// ─────────────────────────────────────────────────────────────────────────────
// The PNG encoder (std alone — forbids a crate outside the allowlist)
// ─────────────────────────────────────────────────────────────────────────────

/// The PNG signature.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// The maximum length of one zlib `stored` (uncompressed) block.
const STORED_MAX: usize = 0xFFFF;

/// The most steps `adler32` can safely accumulate within a u32 (zlib's `NMAX`).
const ADLER_NMAX: usize = 5552;

/// `adler32`'s modulus.
const ADLER_BASE: u32 = 65521;

/// The PNG `CRC-32` accumulator (polynomial `0xEDB88320`). It runs bit by bit, with no table.
struct Crc32(u32);

impl Crc32 {
    fn new() -> Self {
        Self(0xFFFF_FFFF)
    }

    fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = (self.0 & 1).wrapping_neg();
                self.0 = (self.0 >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
    }

    fn finish(self) -> [u8; 4] {
        (!self.0).to_be_bytes()
    }
}

/// zlib's `adler32` (4 bytes, big-endian).
fn adler32(bytes: &[u8]) -> [u8; 4] {
    let mut a = 1u32;
    let mut b = 0u32;
    // Breaking the steps at NMAX keeps it from overflowing within a u32 (zlib's own reasoning).
    for chunk in bytes.chunks(ADLER_NMAX) {
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= ADLER_BASE;
        b %= ADLER_BASE;
    }
    ((b << 16) | a).to_be_bytes()
}

/// An uncompressed zlib stream: the header `0x78 0x01` plus `stored` deflate blocks plus the `adler32`.
fn zlib_stored(data: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&[0x78, 0x01]);
    let mut chunks = data.chunks(STORED_MAX).peekable();
    if chunks.peek().is_none() {
        // Even empty input needs one block (BFINAL = 1, LEN = 0).
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    while let Some(chunk) = chunks.next() {
        let len = u16::try_from(chunk.len()).unwrap_or(u16::MAX);
        out.push(u8::from(chunks.peek().is_none())); // BFINAL, BTYPE = 00
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&adler32(data));
}

/// One lump of length plus kind plus data plus `CRC-32`.
fn png_chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let mut crc = Crc32::new();
    crc.update(&kind);
    crc.update(data);
    out.extend_from_slice(&crc.finish());
}

/// A `ColorImage` → 8-bit truecolour (RGB) PNG bytes. The alpha is dropped (the window is opaque).
fn encode_png(image: &egui::ColorImage) -> Option<Vec<u8>> {
    let [w, h] = image.size;
    if w == 0 || h == 0 || image.pixels.len() < w.saturating_mul(h) {
        return None;
    }
    let mut raw = Vec::with_capacity(h.saturating_mul(w.saturating_mul(3).saturating_add(1)));
    for row in image.pixels.chunks_exact(w) {
        raw.push(0); // filter 0 = None
        for px in row {
            raw.extend_from_slice(&[px.r(), px.g(), px.b()]);
        }
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&u32::try_from(w).ok()?.to_be_bytes());
    ihdr.extend_from_slice(&u32::try_from(h).ok()?.to_be_bytes());
    // Bit depth 8, colour type 2 (truecolour), compression 0 (deflate), filter 0, no interlacing.
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut idat = Vec::with_capacity(raw.len() + raw.len() / STORED_MAX * 5 + 16);
    zlib_stored(&raw, &mut idat);
    let mut out = Vec::with_capacity(idat.len() + 128);
    out.extend_from_slice(&PNG_MAGIC);
    png_chunk(&mut out, *b"IHDR", &ihdr);
    png_chunk(&mut out, *b"IDAT", &idat);
    png_chunk(&mut out, *b"IEND", &[]);
    Some(out)
}

/// Save as a PNG and hand back the byte count.
pub fn write_png(path: &Path, image: &egui::ColorImage) -> std::io::Result<usize> {
    let Some(bytes) = encode_png(image) else {
        return Err(std::io::Error::other("empty image"));
    };
    std::fs::write(path, &bytes)?;
    Ok(bytes.len())
}

/// The decoder an integrator wires into `ShellBuilder::image_loader` (guide 09 §1).
///
/// **The crate does not decode images.** This function sitting among the examples is the evidence —
/// `image` is a dev-dependency of `fairing` and never enters an integrator's tree, and an integrator
/// writes these ten lines into their own code with the decoder they chose.
///
/// Wired up, one config line changes the wallpaper:
///
/// ```toml
/// [desktop]
/// wallpaper = "file:/opt/acme/brand/store-bg.webp"
/// wallpaper_fit = "cover"
/// ```
///
/// # Errors
///
/// [`fairing::Error::Io`] where the file cannot be read, [`fairing::Error::Image`] where it cannot be
/// decoded.
pub fn image_loader(ctx: &egui::Context, path: &Path) -> fairing::Result<egui::TextureHandle> {
    let bytes = std::fs::read(path).map_err(|e| fairing::Error::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    let rgba = image::load_from_memory(&bytes)
        .map_err(|e| fairing::Error::Image(e.to_string()))?
        .to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    // Where the original is much larger than the screen, `LINEAR` (with no mipmaps) shimmers — guide 09 §1.3.
    if size[0] > 4096 || size[1] > 4096 {
        log::warn!(
            "{}: {} x {} may exceed the GPU texture limit",
            path.display(),
            size[0],
            size[1]
        );
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Ok(ctx.load_texture(path.to_string_lossy(), image, egui::TextureOptions::LINEAR))
}

/// Bytes baked into the executable, as a texture (guide 09 §1.3).
///
/// The in-memory version of [`image_loader`]. For when it is carried as an `include_bytes!` rather
/// than a file path — with no need to load assets onto the device separately, a brand's default
/// wallpaper is usually this one.
///
/// # Errors
///
/// [`fairing::Error::Image`] where the decode fails.
pub fn decode_texture(
    ctx: &egui::Context,
    bytes: &[u8],
    name: &str,
) -> fairing::Result<egui::TextureHandle> {
    let rgba = image::load_from_memory(bytes)
        .map_err(|e| fairing::Error::Image(e.to_string()))?
        .to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Ok(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
}

/// **The crate's own wallpaper**, picked for the panel's aspect — shared by `demo` and `console`.
///
/// It lived in `demo` and the console example needed the same five plates and the same light/dark
/// pairing, which is the point at which a copy stops being cheaper than a move.
type WallpaperPlate = (f32, &'static [u8], Option<&'static [u8]>);

const WALLPAPERS: &[WallpaperPlate] = &[
    (
        0.70,
        include_bytes!("../../../../assets/brand/abyss-portrait.webp"),
        None,
    ),
    (
        1.15,
        include_bytes!("../../../../assets/brand/abyss-square.webp"),
        None,
    ),
    (
        1.55,
        include_bytes!("../../../../assets/brand/abyss-4x3.webp"),
        None,
    ),
    (
        2.10,
        include_bytes!("../../../../assets/brand/abyss-landscape.webp"),
        Some(include_bytes!(
            "../../../../assets/brand/abyss-light-landscape.webp"
        )),
    ),
    (
        f32::INFINITY,
        include_bytes!("../../../../assets/brand/abyss-ultrawide.webp"),
        None,
    ),
];

/// The wallpaper for a screen's aspect ratio. The table is in ascending order of the aspect ratio's **upper bound**.
fn wallpaper_for(aspect: f32) -> (&'static [u8], Option<&'static [u8]>) {
    // The last row is `INFINITY`, so `find` always matches, but in case the table is ever edited it
    // falls back to the portrait plate — better than choosing none.
    let fallback = WALLPAPERS
        .first()
        .map_or((&[][..], None), |(_, dark, light)| (*dark, *light));
    WALLPAPERS
        .iter()
        .find(|(max, _, _)| aspect <= *max)
        .map_or(fallback, |(_, dark, light)| (*dark, *light))
}

/// Upload the default wallpaper as a texture. The shell comes up even on a failure — a demo failing
/// to start over one wallpaper would be awkward (the same rule as guide 09 §1.2).
///
/// Given `--wallpaper=`, this is skipped — not because the config is stronger than the code, but
/// because that argument exists **to try a different wallpaper**.
pub fn brand_wallpaper(ctx: &egui::Context) -> Option<fairing::desktop::Wallpaper> {
    // A tour given `--size=WxH` goes by that size; a run without it, by the window's. Where there is
    // no window yet (before the first frame) it assumes landscape — the first repaint sets it right.
    let screen = ctx.input(|i| i.raw.screen_rect).unwrap_or(egui::Rect::ZERO);
    let aspect = if screen.height() > 1.0 {
        screen.width() / screen.height()
    } else {
        16.0 / 9.0
    };
    let (dark_bytes, light_bytes) = wallpaper_for(aspect);
    let fit = fairing::desktop::Fit::Cover;
    let dark = match decode_texture(ctx, dark_bytes, "fairing.brand.wallpaper") {
        Ok(texture) => fairing::desktop::Wallpaper::owned(texture, fit),
        Err(e) => {
            log::error!(
                "cannot read the built-in wallpaper: {e} - falling back to the configured one"
            );
            return None;
        }
    };
    // **The light half.** Without it the shell would keep this dark painting under its light
    // palette. A plate with no light twin pairs with the flat background instead, which
    // is honest: better a plain page than a night picture at noon.
    let light = light_bytes
        .and_then(
            |bytes| match decode_texture(ctx, bytes, "fairing.brand.wallpaper.light") {
                Ok(texture) => Some(fairing::desktop::Wallpaper::owned(texture, fit)),
                Err(e) => {
                    log::warn!("cannot read the light wallpaper: {e} - a flat page instead");
                    None
                }
            },
        )
        .unwrap_or(fairing::desktop::Wallpaper::Solid(
            fairing::ColorRole::Background,
        ));
    Some(fairing::desktop::Wallpaper::themed(dark, light))
}
