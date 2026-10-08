//! The `runner` feature: a minimal bootstrap on `eframe`.
//!
//! For an integrator who does not own their own event loop and backends, this module opens a
//! fullscreen kiosk window with `eframe` and calls a callback every frame.
//!
//! `eframe` 0.36.1 has the shape
//! `eframe::App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)` (confirmed against
//! the registry source; not `update()`). The panel receives a `Ui`, so [`run`]'s callback and
//! [`crate::Shell::frame`] take a root `Ui` too.

use crate::error::{Error, Result};
use crate::shell::Shell;

/// The options for [`run`] and [`run_shell`].
#[derive(Debug, Clone)]
pub struct Options {
    /// Whether to open the window fullscreen. Usually `true` for a device build.
    pub fullscreen: bool,
    /// The window title (shown by a window manager during desktop development). It barely matters for a fullscreen device deployment.
    pub title: String,
    /// The window size (windowed mode, `--size`). Without it, the runner's default.
    pub size: Option<(f32, f32)>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            fullscreen: true,
            title: "fairing".to_owned(),
            size: None,
        }
    }
}

fn native_options(options: &Options) -> eframe::NativeOptions {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(options.title.clone())
        .with_fullscreen(options.fullscreen)
        .with_decorations(false);
    if let Some((w, h)) = options.size {
        viewport = viewport.with_inner_size([w, h]);
    }
    eframe::NativeOptions {
        viewport,
        ..Default::default()
    }
}

/// Open a fullscreen window and call `app` every frame. Returns when the window closes.
///
/// # Errors
/// [`Error::Runner`] if creating the window or initialising the graphics context fails.
#[allow(clippy::needless_pass_by_value)] // Builder-style consumption is the intended shape of this API.
pub fn run(options: Options, app: impl FnMut(&mut egui::Ui) + 'static) -> Result<()> {
    eframe::run_native(
        &options.title,
        native_options(&options),
        Box::new(move |_cc| Ok(Box::new(ClosureApp { app }))),
    )
    .map_err(|err| Error::Runner(err.to_string()))
}

/// Open a window and run the [`Shell`] `build` produced on that context, every frame.
/// `Shell::new` needs an `egui::Context` (`Waker`), so the shell is built after
/// the window exists.
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_shell(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<Shell> + 'static,
) -> Result<()> {
    run_shell_with(options, build, |event| {
        log::debug!("shell event: {event:?}");
    })
}

/// [`run_shell`], but it **hands you the [`ShellEvent`](crate::ShellEvent)s**.
///
/// `run_shell` only logs the events. With the runner, that left no way at all to receive
/// `Access(UnlockRequested)` (the moment an authentication prompt has to go up),
/// `SettingChanged` or `ScreenClosed` — and for that one thing you had to drop to
/// `runner::run` and write the frame loop yourself. Every device with a lock screen is in that
/// position.
///
/// `on_event` is called **within the frame** the shell emitted the event in.
///
/// ```no_run
/// # fn main() -> fairing::Result<()> {
/// use fairing::{runner, ShellEvent};
///
/// runner::run_shell_with(
///     runner::Options::default(),
///     |ctx| fairing::Shell::builder(fairing::ShellConfig::default()).build(ctx),
///     |event| {
///         if let ShellEvent::Access(ev) = event {
///             log::info!("authentication needed: {ev:?}");
///         }
///     },
/// )
/// # }
/// ```
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_shell_with(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<Shell> + 'static,
    on_event: impl FnMut(&crate::ShellEvent) + 'static,
) -> Result<()> {
    eframe::run_native(
        &options.title,
        native_options(&options),
        Box::new(move |cc| {
            let shell = build(&cc.egui_ctx)
                .map_err(|err| -> Box<dyn std::error::Error + Send + Sync> { Box::new(err) })?;
            Ok(Box::new(ShellApp {
                shell,
                on_event: Box::new(on_event),
            }))
        }),
    )
    .map_err(|err| Error::Runner(err.to_string()))
}

/// **The shell plus the app's own state**, for a device whose screens share something.
///
/// [`run_shell`] is for an app that has no state of its own beyond the shell. Where it has —
/// a machine console, an order, a session — `build` hands back both, this owns both, and every
/// frame lends the state to the screens through
/// [`Shell::frame_with`](crate::Shell::frame_with). No screen captures it and the shell does not
/// store it.
///
/// ```no_run
/// # fn main() -> fairing::Result<()> {
/// # #[derive(Default)] struct Console;
/// use fairing::{runner, Services, Shell, ShellConfig};
///
/// runner::run_app(runner::Options::default(), |ctx| {
///     let shell = Shell::new(ShellConfig::default(), Services::null(), ctx)?;
///     Ok((shell, Console::default()))
/// })
/// # }
/// ```
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_app<S: std::any::Any>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<(Shell, S)> + 'static,
) -> Result<()> {
    run_app_with(options, build, |event| {
        log::debug!("shell event: {event:?}");
    })
}

/// [`run_app`], but it hands you the [`ShellEvent`](crate::ShellEvent)s — [`run_shell_with`]'s
/// counterpart.
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_app_with<S: std::any::Any>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<(Shell, S)> + 'static,
    on_event: impl FnMut(&crate::ShellEvent) + 'static,
) -> Result<()> {
    eframe::run_native(
        &options.title,
        native_options(&options),
        Box::new(move |cc| {
            let (shell, state) = build(&cc.egui_ctx)
                .map_err(|err| -> Box<dyn std::error::Error + Send + Sync> { Box::new(err) })?;
            Ok(Box::new(StateApp {
                shell,
                state,
                on_event: Box::new(on_event),
            }))
        }),
    )
    .map_err(|err| Error::Runner(err.to_string()))
}

/// A [`Shell`] plus the app's state as an `eframe::App` — the app owns both.
struct StateApp<S> {
    shell: Shell,
    state: S,
    on_event: Box<dyn FnMut(&crate::ShellEvent)>,
}

impl<S: std::any::Any> eframe::App for StateApp<S> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.frame_with(ui, &mut self.state);
        for event in self.shell.poll_events() {
            (self.on_event)(&event);
        }
    }
}

/// An adapter wrapping one `FnMut(&mut egui::Ui)` closure as an `eframe::App`.
struct ClosureApp<F> {
    app: F,
}

impl<F> eframe::App for ClosureApp<F>
where
    F: FnMut(&mut egui::Ui) + 'static,
{
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        (self.app)(ui);
    }
}

/// A [`Shell`] as an `eframe::App`.
struct ShellApp {
    shell: Shell,
    on_event: Box<dyn FnMut(&crate::ShellEvent)>,
}

impl eframe::App for ShellApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.frame(ui);
        for event in self.shell.poll_events() {
            (self.on_event)(&event);
        }
    }
}
