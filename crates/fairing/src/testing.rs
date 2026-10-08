//! The headless test harness. It runs frames through
//! `egui::Context::run_ui` with no window and no GPU, and takes no dev-dependencies.
//! An integrator can test their own screens the same way.

// UI geometry: small integer counts and pixel values crossing to f32. The loss is meaningless in this
// range, so the cast lints are lifted for the whole file (the workspace's pedantic lints stay).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use crate::config::{AccessConfig, ShellConfig};
use crate::error::Result;
use crate::services::Services;
use crate::shell::Shell;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

/// A headless shell plus synthetic input.
pub struct Harness {
    /// The egui context.
    pub ctx: egui::Context,
    /// The shell.
    pub shell: Shell,
    size: Vec2,
    time: f64,
    pending: Vec<Event>,
    /// How many frames have run.
    pub frames: u64,
    /// Whether an immediate repaint was requested (`ctx.requested_repaint_last_pass`).
    ///
    /// **It shows up one pass late.** egui 0.36.1 refreshes `prev_pass_paint_delay` in
    /// `begin_pass`, so the value read after frame N is what frame N−1 requested. And one
    /// immediate request stays `true` for **two frames** by egui's rule ("each request results in
    /// two repaints", `outstanding = 1`) — so even on the frame E where a transition ends and
    /// `is_animating()` is `false`, this stays `true` through E+1. So "0 fps at rest" is asserted
    /// **three frames after** the last animated frame, and "reacts on the first frame" is
    /// asserted from shell state like `shell.desktop().is_pressed()` rather than from this
    /// (A7).
    pub repaint_requested: bool,
    /// The app's own state, lent to the screens each frame. `()` unless
    /// [`Harness::with_app`] put something there.
    app: Box<dyn std::any::Any>,
    /// How many `ViewportCommand::Screenshot`s the shell has sent so far — the harness has no
    /// runner to take them; [`Harness::answer_screenshot`] plays its part.
    screenshots: usize,
}

/// The default screen size (the reference board).
pub const DEFAULT_SIZE: Vec2 = Vec2::new(1024.0, 600.0);

/// The frame interval (60 Hz).
pub const FRAME_DT: f64 = 1.0 / 60.0;

/// [`FRAME_DT`] as an `f32`.
pub const FRAME_DT_F32: f32 = 1.0 / 60.0;

impl Harness {
    /// A new harness.
    ///
    /// # Errors
    /// A configuration error from `Shell::new`.
    pub fn new(config: ShellConfig, services: Services) -> Result<Self> {
        Self::from_builder(|ctx| Shell::new(config, services, ctx))
    }

    /// Build the shell the way an integrator would and wrap it — for setups [`Harness::new`]
    /// cannot express, such as the theme injection and painter hooks of
    /// [`crate::Shell::builder`]. `build` receives the headless context the
    /// harness made.
    ///
    /// # Errors
    /// Whatever `build` returned (usually [`crate::Error::Config`]).
    pub fn from_builder(build: impl FnOnce(&egui::Context) -> Result<Shell>) -> Result<Self> {
        let ctx = egui::Context::default();
        let shell = build(&ctx)?;
        Ok(Self {
            ctx,
            shell,
            size: DEFAULT_SIZE,
            time: 0.0,
            pending: Vec::new(),
            frames: 0,
            repaint_requested: false,
            app: Box::new(()),
            screenshots: 0,
        })
    }

    /// The screen size.
    #[must_use]
    pub fn with_size(mut self, width: f32, height: f32) -> Self {
        self.size = Vec2::new(width, height);
        self
    }

    /// **Lend the screens some app state** for every frame this harness runs — the test's
    /// stand-in for the app that owns the shell.
    #[must_use]
    pub fn with_app<S: std::any::Any>(mut self, state: S) -> Self {
        self.app = Box::new(state);
        self
    }

    /// Read that state back — what the screens did to it.
    pub fn app_mut<S: std::any::Any>(&mut self) -> Option<&mut S> {
        self.app.downcast_mut()
    }

    /// Change the screen size **while it is running** — to simulate a rotation or a window
    /// resize.
    ///
    /// [`Harness::with_size`] is a builder that fixes it at construction, so it cannot produce a
    /// "changed while running" case. Verifying something that only happens on **a change**, like
    /// `Lifecycle::Resized`, needs this.
    pub fn set_size(&mut self, width: f32, height: f32) {
        self.size = Vec2::new(width, height);
    }

    /// The screen rect.
    #[must_use]
    pub fn screen_rect(&self) -> Rect {
        Rect::from_min_size(Pos2::ZERO, self.size)
    }

    /// The shell's monotonic time (as of the last frame, `Shell::now`). It advances by exactly
    /// [`FRAME_DT`] per frame — use it as the reference when asserting on
    /// `evict_after` or a backend's `next_wake`.
    #[must_use]
    pub fn now(&self) -> std::time::Instant {
        self.shell.now()
    }

    /// The next frame's `RawInput.time` (seconds). The frames run so far × [`FRAME_DT`].
    #[must_use]
    pub fn time(&self) -> f64 {
        self.time
    }

    /// One frame (advancing time by 1/60 s).
    pub fn frame(&mut self) {
        self.run_frame().drop_without_applying_deltas();
    }

    /// Run one frame and return **the shapes drawn in it**, merged in layer order, so you can
    /// assert on "what actually got drawn this frame" — egui's `Area` sizing pass throws its
    /// output away entirely, and this is how slot warm-up regressions are caught.
    #[must_use]
    pub fn frame_shapes(&mut self) -> Vec<egui::epaint::ClippedShape> {
        let mut output = self.run_frame();
        let shapes = std::mem::take(&mut output.shapes);
        output.drop_without_applying_deltas();
        shapes
    }

    fn run_frame(&mut self) -> egui::FullOutput {
        let input = RawInput {
            screen_rect: Some(self.screen_rect()),
            time: Some(self.time),
            predicted_dt: FRAME_DT_F32,
            events: std::mem::take(&mut self.pending),
            ..Default::default()
        };
        let Self {
            ctx, shell, app, ..
        } = self;
        let output = ctx.run_ui(input, |ui| shell.frame_with(ui, &mut **app));
        self.screenshots += output
            .viewport_output
            .values()
            .flat_map(|v| v.commands.iter())
            .filter(|c| matches!(c, egui::ViewportCommand::Screenshot(_)))
            .count();
        self.repaint_requested = self.ctx.requested_repaint_last_pass();
        self.time += FRAME_DT;
        self.frames += 1;
        output
    }

    /// Several frames.
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame();
        }
    }

    /// Let `seconds` pass **without drawing** — a panel asleep between repaints, as a reactive
    /// one is whenever nothing moves. The next frame sees the whole gap in `RawInput::time`, the
    /// way a real wake does; the shell's deadlines (a temporary unlock, the session timeout, the
    /// idle lock) are measured against it.
    pub fn sleep(&mut self, seconds: f64) {
        self.time += seconds.max(0.0);
    }

    /// Run frames until the given time has passed.
    pub fn run_for(&mut self, seconds: f64) {
        let n = (seconds / FRAME_DT).ceil().max(0.0) as usize;
        self.frames(n);
    }

    /// Queue an event for the next frame.
    pub fn push_event(&mut self, event: Event) {
        self.pending.push(event);
    }

    /// Move the pointer.
    pub fn move_to(&mut self, pos: Pos2) {
        self.pending.push(Event::PointerMoved(pos));
    }

    /// How many screenshots the shell has asked the runner for so far
    /// (`ViewportCommand::Screenshot` — a card's frosted backdrop).
    #[must_use]
    pub fn screenshots_requested(&self) -> usize {
        self.screenshots
    }

    /// **Answer a screenshot** the way a runner would: `image` arrives on the next frame as
    /// `Event::Screenshot`. The harness has no framebuffer to read, so a test hands one over.
    pub fn answer_screenshot(&mut self, image: egui::ColorImage) {
        self.pending.push(Event::Screenshot {
            viewport_id: egui::ViewportId::ROOT,
            user_data: egui::UserData::default(),
            image: std::sync::Arc::new(image),
        });
    }

    /// Press (on the next frame).
    pub fn press(&mut self, pos: Pos2) {
        self.pending.push(Event::PointerMoved(pos));
        self.pending.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        });
    }

    /// Release (on the next frame).
    pub fn release(&mut self, pos: Pos2) {
        self.pending.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        });
        self.pending.push(Event::PointerGone);
    }

    /// A tap: the press frame, the release frame, and one more frame for it to take effect. egui
    /// raises `Response::clicked()` on the pass that handles the release event and the shell acts
    /// in stage 14 of that same frame, so the third frame is where the consequences land (the
    /// registry going dirty → a rebuild, lifecycle propagation on a `reduce` transition) —
    /// integration tests 7 and 8 pin this frame count.
    pub fn tap(&mut self, pos: Pos2) {
        self.press(pos);
        self.frame();
        self.release(pos);
        self.frame();
        self.frame();
    }

    /// Press and release one key (for testing hardware keypad triggers and shortcuts).
    ///
    /// It is sent with `repeat: false` — an auto-repeat is not a person pressing once.
    pub fn key(&mut self, key: egui::Key) {
        for pressed in [true, false] {
            self.pending.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        self.frame();
    }

    /// Hold a press for `frames` frames (long presses, the emergency gesture). It does not release.
    pub fn hold(&mut self, pos: Pos2, frames: usize) {
        self.press(pos);
        self.frame();
        for _ in 0..frames {
            self.move_to(pos);
            self.frame();
        }
    }

    /// A fling: from `from`, move `step` per frame for `frames` frames, then release. egui's
    /// `pointer.velocity()` is a regression over the last 100 ms window, so at 60 Hz `step`
    /// px/frame lands as ≈ `step × 60` px/s (A1's "released at −1000" is
    /// `step.y = −16.7`).
    pub fn fling(&mut self, from: Pos2, step: Vec2, frames: usize) {
        self.press(from);
        self.frame();
        let mut pos = from;
        for _ in 0..frames.max(1) {
            pos += step;
            self.move_to(pos);
            self.frame();
        }
        self.release(pos);
        self.frame();
    }

    /// Wheel or trackpad scrolling (`Event::MouseWheel`, with `delta` in pixels — scrolling up is
    /// `delta.y > 0`). The pointer is moved to `pos` first, so the `ScrollArea` there receives it.
    pub fn wheel(&mut self, pos: Pos2, delta: Vec2) {
        self.pending.push(Event::PointerMoved(pos));
        self.pending.push(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            modifiers: Modifiers::default(),
            phase: egui::TouchPhase::Move,
        });
    }

    /// A text input event (the same path as OSK injection, `Event::Text`).
    pub fn type_text(&mut self, text: &str) {
        self.pending.push(Event::Text(text.to_owned()));
    }

    /// A drag: press, move over `steps` frames, release.
    pub fn drag(&mut self, from: Pos2, to: Pos2, steps: usize) {
        self.press(from);
        self.frame();
        let steps = steps.max(1);
        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            self.move_to(from + (to - from) * t);
            self.frame();
        }
        self.release(to);
        self.frame();
    }
}

/// A config with one level (no authentication).
#[must_use]
pub fn single_level_access() -> ShellConfig {
    access_config(&["only"], None)
}

/// A config with a level table and a `default_gate` (`mode = "prompt"`).
#[must_use]
pub fn access_config(levels: &[&str], default_gate: Option<&str>) -> ShellConfig {
    access_config_mode(levels, default_gate, "prompt")
}

/// A level table, a `default_gate` and a mode.
#[must_use]
pub fn access_config_mode(levels: &[&str], default_gate: Option<&str>, mode: &str) -> ShellConfig {
    ShellConfig {
        access: AccessConfig {
            mode: mode.to_owned(),
            levels: levels.iter().map(|s| (*s).to_owned()).collect(),
            default_gate: default_gate.map(str::to_owned),
            ..AccessConfig::default()
        },
        ..ShellConfig::default()
    }
}

/// Build a shell on the `Null` backends (a fixed clock) and add declarations with `setup`
/// (`test_shell`).
///
/// **It forces `motion.reduce = true`** — transitions finish immediately, which is what makes
/// the "two frames and it has landed" contract hold. To test the animation itself, pass a config
/// to [`Harness::new`] directly.
///
/// # Errors
/// A configuration error.
pub fn test_shell(mut config: ShellConfig, setup: impl FnOnce(&mut Shell)) -> Result<Harness> {
    config.motion.reduce = true;
    let services = Services::builder()
        .clock(crate::services::null::NullClock)
        .build();
    let mut harness = Harness::new(config, services)?;
    setup(&mut harness.shell);
    Ok(harness)
}

/// `run_frames(&mut shell, n)`.
pub fn run_frames(harness: &mut Harness, n: usize) {
    harness.frames(n);
}
