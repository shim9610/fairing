//! The `runner` feature: a minimal bootstrap on `eframe`.
//!
//! This module opens an `eframe` window and connects its UI callbacks to application
//! code. Fullscreen mode is configurable. Applications still supply device backends.
//!
//! `eframe` 0.37 has the shape
//! `eframe::App::ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`.
//! The panel receives a `Ui`, so [`run`]'s callback and
//! [`crate::Shell::frame`] take a root `Ui` too.
//!
//! The shell/state adapters call [`Shell::logic`] / [`Shell::logic_with`] from
//! `App::logic`, so commands, polling and access deadlines continue while no UI pass
//! runs. Their event handlers can run then too. [`run`] wraps only a UI closure;
//! custom hosts that own a shell must connect the logic callback themselves.

use crate::error::{Error, Result};
use crate::shell::{HostedShell, Shell};

/// The options for [`run`] and [`run_shell`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Options {
    /// Whether to open the window fullscreen. Usually `true` for a device build.
    pub fullscreen: bool,
    /// The window title, used by the window manager where titles are displayed.
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

/// Open a fullscreen window and call `app` for each UI pass. Returns when the window closes.
///
/// # Errors
/// [`Error::Runner`] if creating the window or initialising the graphics context fails.
#[allow(clippy::needless_pass_by_value)] // Builder-style consumption is the intended shape of this API.
pub fn run(options: Options, app: impl FnMut(&mut egui::Ui) + 'static) -> Result<()> {
    eframe::run_native(
        &options.title,
        native_options(&options),
        Box::new(move |_cc| {
            Ok(Box::new(ClosureApp {
                app,
                faulted: false,
            }))
        }),
    )
    .map_err(|err| Error::Runner(err.to_string()))
}

/// Open a window and run the [`Shell`] `build` produced on that context for each UI pass.
/// `Shell::new` needs an `egui::Context` (`Waker`), so the shell is built after
/// the window exists.
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_shell<H: HostedShell + 'static>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<H> + 'static,
) -> Result<()> {
    run_shell_with(options, build, |event| {
        log::debug!("shell event: {event:?}");
    })
}

/// [`run_shell`] with an application event handler.
///
/// Unlike `run_shell`, which logs events, this forwards them to `on_event`.
/// `on_event` is called in the UI or logic callback that emitted the event, including
/// when the window has no UI pass.
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
pub fn run_shell_with<H: HostedShell + 'static>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<H> + 'static,
    on_event: impl FnMut(&crate::ShellEvent) + 'static,
) -> Result<()> {
    run_shell_with_recovery(options, build, on_event, |_, _| {})
}

/// Run with an application-controlled recovery callback.
///
/// `on_recovery` runs on each logic tick that begins with a quarantined shell, including
/// when no UI pass occurs. It may call [`Shell::restart_with`] immediately, wait for
/// [`Shell::take_restart_request`], or implement any other application policy. The
/// library imposes no automatic/manual choice, retry count or delay. Normal dispatch
/// resumes on a subsequent callback after successful replacement. If this callback
/// unwinds, the current shell is quarantined and partial output is discarded.
///
/// Fault guidance is separately opt-in via [`crate::ShellBuilder::recovery_ui`].
/// External business state is the application's responsibility.
/// Use `ctx.request_repaint_after(delay)` when your policy needs a later logic tick.
///
/// ```no_run
/// # fn main() -> fairing::Result<()> {
/// use fairing::{recovery::RecoveryUi, runner, Shell, ShellConfig};
/// fn build(ctx: &fairing::egui::Context) -> fairing::Result<Shell> {
///     Shell::builder(ShellConfig::default()).recovery_ui(RecoveryUi {
///         message: "Follow the operator recovery procedure.".into(),
///         restart_label: Some("Request recovery".into()),
///         ..Default::default()
///     }).build(ctx)
/// }
/// runner::run_shell_with_recovery(runner::Options::default(), build, |_| {}, |ctx, shell| {
///     // Application policy: this example waits for a button; an automatic policy need not.
///     if shell.take_restart_request() {
///         if let Err(error) = shell.restart_with(|| build(ctx)) {
///             log::warn!("{error}");
///         }
///     }
/// })
/// # }
/// ```
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_shell_with_recovery<H: HostedShell + 'static>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<H> + 'static,
    on_event: impl FnMut(&crate::ShellEvent) + 'static,
    on_recovery: impl FnMut(&egui::Context, &mut H) + 'static,
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
                on_recovery: Box::new(on_recovery),
            }))
        }),
    )
    .map_err(|err| Error::Runner(err.to_string()))
}

/// **The shell plus the app's own state**, for a device whose screens share something.
///
/// [`run_shell`] is for an app that has no state of its own beyond the shell. Where it has —
/// a machine console, an order, a session — `build` hands back both, this owns both, and every
/// update lends the state to callbacks through the shell logic/frame methods.
/// The runner owns the state; the shell borrows it for each call.
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
pub fn run_app<S: std::any::Any, H: HostedShell + 'static>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<(H, S)> + 'static,
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
pub fn run_app_with<S: std::any::Any, H: HostedShell + 'static>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<(H, S)> + 'static,
    on_event: impl FnMut(&crate::ShellEvent) + 'static,
) -> Result<()> {
    run_app_with_recovery(options, build, on_event, |_, _, _| {})
}

/// Run with an application-controlled recovery callback.
///
/// `on_recovery` runs on each logic tick that begins with a quarantined shell, including
/// when no UI pass occurs. It may call [`Shell::restart_with`] immediately, wait for
/// [`Shell::take_restart_request`], or implement any other application policy. The
/// library imposes no automatic/manual choice, retry count or delay. Normal dispatch
/// resumes on a subsequent callback after successful replacement. If this callback
/// unwinds, the current shell is quarantined and partial output is discarded.
///
/// Fault guidance is separately opt-in via [`crate::ShellBuilder::recovery_ui`].
/// External business state is the application's responsibility.
/// Use `ctx.request_repaint_after(delay)` when your policy needs a later logic tick.
///
/// # Errors
/// [`Error::Runner`] for a window or graphics failure; `build`'s error unchanged.
#[allow(clippy::needless_pass_by_value)]
pub fn run_app_with_recovery<S: std::any::Any, H: HostedShell + 'static>(
    options: Options,
    build: impl FnOnce(&egui::Context) -> Result<(H, S)> + 'static,
    on_event: impl FnMut(&crate::ShellEvent) + 'static,
    on_recovery: impl FnMut(&egui::Context, &mut H, &mut S) + 'static,
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
                on_recovery: Box::new(on_recovery),
            }))
        }),
    )
    .map_err(|err| Error::Runner(err.to_string()))
}

type StateRecovery<S, H> = dyn FnMut(&egui::Context, &mut H, &mut S);
type ShellRecovery<H> = dyn FnMut(&egui::Context, &mut H);

/// A [`Shell`] plus the app's state as an `eframe::App` — the app owns both.
struct StateApp<S, H: HostedShell = Shell> {
    shell: H,
    state: S,
    on_recovery: Box<StateRecovery<S, H>>,
    on_event: Box<dyn FnMut(&crate::ShellEvent)>,
}

impl<S: std::any::Any, H: HostedShell> eframe::App for StateApp<S, H> {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.shell.is_faulted() {
            if !crate::runtime::contain(|| {
                (self.on_recovery)(ctx, &mut self.shell, &mut self.state);
            }) {
                self.shell.quarantine();
                crate::runtime::blank_faulted_frame(ctx);
            }
            return;
        }
        if !crate::runtime::contain(|| {
            self.shell.logic_with(ctx, &mut self.state);
            for event in self.shell.poll_events() {
                (self.on_event)(&event);
            }
        }) {
            self.shell.quarantine();
        }
        if self.shell.is_faulted() {
            crate::runtime::blank_faulted_frame(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.shell.is_faulted() {
            self.shell.frame_with(ui, &mut self.state);
            return;
        }
        if !crate::runtime::contain(|| {
            self.shell.frame_with(ui, &mut self.state);
            for event in self.shell.poll_events() {
                (self.on_event)(&event);
            }
        }) {
            self.shell.quarantine();
        }
        if self.shell.is_faulted() {
            crate::runtime::blank_faulted_frame(ui.ctx());
        }
    }
}

/// An adapter wrapping one `FnMut(&mut egui::Ui)` closure as an `eframe::App`.
struct ClosureApp<F> {
    app: F,
    faulted: bool,
}

impl<F> eframe::App for ClosureApp<F>
where
    F: FnMut(&mut egui::Ui) + 'static,
{
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.faulted {
            return;
        }
        if !crate::runtime::contain(|| (self.app)(ui)) {
            self.faulted = true;
        }
        if self.faulted {
            crate::runtime::blank_faulted_frame(ui.ctx());
        }
    }
}

/// A [`Shell`] as an `eframe::App`.
struct ShellApp<H: HostedShell = Shell> {
    shell: H,
    on_recovery: Box<ShellRecovery<H>>,
    on_event: Box<dyn FnMut(&crate::ShellEvent)>,
}

impl<H: HostedShell> eframe::App for ShellApp<H> {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.shell.is_faulted() {
            if !crate::runtime::contain(|| {
                (self.on_recovery)(ctx, &mut self.shell);
            }) {
                self.shell.quarantine();
                crate::runtime::blank_faulted_frame(ctx);
            }
            return;
        }
        if !crate::runtime::contain(|| {
            self.shell.logic(ctx);
            for event in self.shell.poll_events() {
                (self.on_event)(&event);
            }
        }) {
            self.shell.quarantine();
        }
        if self.shell.is_faulted() {
            crate::runtime::blank_faulted_frame(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.shell.is_faulted() {
            self.shell.frame(ui);
            return;
        }
        if !crate::runtime::contain(|| {
            self.shell.frame(ui);
            for event in self.shell.poll_events() {
                (self.on_event)(&event);
            }
        }) {
            self.shell.quarantine();
        }
        if self.shell.is_faulted() {
            crate::runtime::blank_faulted_frame(ui.ctx());
        }
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::{ShellApp, StateApp};
    use crate::{action, LaunchAction, Shell};
    use eframe::App;
    use std::cell::Cell;
    use std::rc::Rc;

    #[test]
    #[allow(
        clippy::panic,
        reason = "inject a host callback unwind into the real runner boundary"
    )]
    fn protected_runner_logic_contains_event_faults_and_preserves_recovery_mode(
    ) -> crate::Result<()> {
        let ctx = egui::Context::default();
        let build = |ctx: &egui::Context| {
            let config =
                crate::ShellConfig::default().configure(|c| c.access.session_tokens = true);
            Shell::builder(config).build_protected(ctx)
        };
        let shell = build(&ctx)?;
        shell.handle().logout();
        let mut app = ShellApp {
            shell,
            on_event: Box::new(|_| panic!("synthetic protected host event fault")),
            on_recovery: Box::new(|_, _| {}),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let output = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert_eq!(output.platform_output.commands.len(), 0);
        assert!(app.shell.is_faulted());
        let old = app.shell.handle();
        app.on_recovery = Box::new(move |ctx, shell| {
            assert!(shell.restart_with(|| build(ctx)).is_ok());
        });
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert!(!app.shell.is_faulted());
        assert!(old.try_send(crate::shell::Command::Home).is_err());
        assert!(app.shell.observe().session_token().is_none());
        Ok(())
    }

    #[test]
    fn application_can_restart_automatically_without_a_view_or_click() -> crate::Result<()> {
        let ctx = egui::Context::default();
        let mut shell = Shell::builder(crate::testing::single_level_access()).build(&ctx)?;
        shell.quarantine();
        let events = Rc::new(Cell::new(0));
        let seen = events.clone();
        let mut app = StateApp {
            shell,
            state: 41usize,
            on_event: Box::new(move |_| seen.set(seen.get() + 1)),
            on_recovery: Box::new(|ctx, shell, state| {
                *state += 1; // Only the application decides how to reconcile its state.
                assert!(shell
                    .restart_with(|| {
                        let fresh =
                            Shell::builder(crate::testing::single_level_access()).build(ctx)?;
                        fresh.handle().logout();
                        Ok(fresh)
                    })
                    .is_ok());
            }),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert!(!app.shell.is_faulted());
        assert_eq!(app.state, 42);
        assert_eq!(
            events.get(),
            0,
            "normal dispatch ran inside recovery callback"
        );
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert_eq!(app.state, 42, "recovery handler ran on a healthy shell");
        assert!(events.get() > 0, "new shell did not resume dispatch");
        Ok(())
    }

    #[test]
    #[allow(clippy::panic, reason = "synthetic developer recovery failure")]
    fn recovery_handler_unwind_blocks_even_a_successfully_replaced_shell() -> crate::Result<()> {
        let ctx = egui::Context::default();
        let mut shell = Shell::builder(crate::testing::single_level_access()).build(&ctx)?;
        shell.quarantine();
        let events = Rc::new(Cell::new(0));
        let seen = events.clone();
        let mut app = ShellApp {
            shell,
            on_event: Box::new(move |_| seen.set(seen.get() + 1)),
            on_recovery: Box::new(|ctx, shell| {
                assert!(shell
                    .restart_with(
                        || Shell::builder(crate::testing::single_level_access()).build(ctx)
                    )
                    .is_ok());
                shell.handle().logout();
                ctx.copy_text("private recovery output".into());
                panic!("synthetic recovery hook failure");
            }),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let output = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert_eq!(output.platform_output.commands.len(), 0);
        assert!(app.shell.is_faulted());
        assert_eq!(events.get(), 0);
        assert!(app
            .shell
            .handle()
            .try_send(crate::shell::Command::Home)
            .is_err());
        Ok(())
    }

    #[test]
    fn request_based_policy_stays_idle_until_the_optional_button_is_clicked() -> crate::Result<()> {
        let ctx = egui::Context::default();
        let view = crate::recovery::RecoveryUi {
            restart_label: Some("Resume through application policy".into()),
            ..Default::default()
        };
        let mut shell = Shell::builder(crate::testing::single_level_access())
            .recovery_ui(view.clone())
            .build(&ctx)?;
        let mut button = egui::Rect::NOTHING;
        ctx.run_ui(egui::RawInput::default(), |ui| {
            if let Some(response) = view.show(ui) {
                button = response.rect;
            }
        })
        .drop_without_applying_deltas();
        shell.quarantine();
        let mut app = ShellApp {
            shell,
            on_event: Box::new(|_| {}),
            on_recovery: Box::new(|ctx, shell| {
                if shell.take_restart_request() {
                    assert!(shell
                        .restart_with(
                            || Shell::builder(crate::testing::single_level_access()).build(ctx)
                        )
                        .is_ok());
                }
            }),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert!(app.shell.is_faulted());
        ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui, &mut frame))
            .drop_without_applying_deltas();
        for pressed in [true, false] {
            let input = egui::RawInput {
                events: vec![
                    egui::Event::PointerMoved(button.center()),
                    egui::Event::PointerButton {
                        pos: button.center(),
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                ..Default::default()
            };
            ctx.run_ui(input, |ui| app.ui(ui, &mut frame))
                .drop_without_applying_deltas();
        }
        assert!(app.shell.is_faulted(), "UI bypassed the developer handler");
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert!(!app.shell.is_faulted());
        Ok(())
    }

    #[test]
    #[allow(clippy::panic, reason = "synthetic event-handler fault")]
    fn event_failure_stops_delivery_and_does_not_retry_a_partially_applied_handler(
    ) -> crate::Result<()> {
        let ctx = egui::Context::default();
        let shell = Shell::builder(crate::testing::single_level_access()).build(&ctx)?;
        shell.handle().logout();
        shell.handle().logout();
        let calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&calls);
        let mut app = ShellApp {
            shell,
            on_recovery: Box::new(|_, _| {}),
            on_event: Box::new(move |_| {
                seen.set(seen.get() + 1);
                panic!("synthetic event failure");
            }),
        };
        let mut frame = eframe::Frame::_new_kittest();
        for _ in 0..3 {
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        }
        assert!(app.shell.is_faulted());
        assert_eq!(calls.get(), 1);
        Ok(())
    }

    #[test]
    #[allow(clippy::panic, reason = "synthetic raw UI failure")]
    fn raw_runner_discards_private_output_and_calls_a_failing_ui_only_once() {
        let ctx = egui::Context::default();
        let calls = Rc::new(Cell::new(0));
        let seen = Rc::clone(&calls);
        let mut app = super::ClosureApp {
            app: move |ui: &mut egui::Ui| {
                seen.set(seen.get() + 1);
                egui::Window::new("private window").show(ui.ctx(), |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| -> () {
                        ui.label("private test drawing");
                        ui.ctx().copy_text("private test clipboard".into());
                        panic!("synthetic nested raw UI failure");
                    });
                });
            },
            faulted: false,
        };
        let mut frame = eframe::Frame::_new_kittest();
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui, &mut frame));
            out.textures_delta.clear();
            assert_eq!(out.shapes.len(), 0);
            assert_eq!(out.platform_output.commands.len(), 0);
        }
        assert!(app.faulted);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn state_runner_delivers_logic_only_actions_and_events_once() -> crate::Result<()> {
        let ctx = egui::Context::default();
        let mut shell = Shell::builder(crate::testing::single_level_access()).build(&ctx)?;
        shell.add(action("work", |cx| {
            if let Some(count) = cx.app_mut::<usize>() {
                *count += 1;
            }
            cx.set_setting("app.done", crate::SettingValue::Bool(true));
        }));
        let delivered = Rc::new(Cell::new(0));
        let seen = Rc::clone(&delivered);
        let _ = shell.poll_events();
        shell.handle().launch(LaunchAction::run("work"));
        let mut app = StateApp {
            shell,
            on_recovery: Box::new(|_, _, _| {}),
            state: 0usize,
            on_event: Box::new(move |event| {
                if matches!(event, crate::ShellEvent::SettingChanged { key, .. } if key.0.as_ref() == "app.done")
                {
                    seen.set(seen.get() + 1);
                }
            }),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert_eq!(app.state, 1);
        assert_eq!(delivered.get(), 1);
        assert_eq!(app.shell.frame_no(), 0);
        let out = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.logic(ui.ctx(), &mut frame);
            app.ui(ui, &mut frame);
        });
        out.drop_without_applying_deltas();
        assert_eq!(app.state, 1);
        assert_eq!(delivered.get(), 1);
        Ok(())
    }

    #[test]
    fn shell_runner_delivers_logic_only_commands_without_drawing() -> crate::Result<()> {
        let ctx = egui::Context::default();
        let shell = Shell::builder(crate::testing::single_level_access()).build(&ctx)?;
        let events = Rc::new(Cell::new(0));
        let seen = Rc::clone(&events);
        shell.handle().logout();
        let mut app = ShellApp {
            shell,
            on_recovery: Box::new(|_, _| {}),
            on_event: Box::new(move |event| {
                if matches!(event, crate::ShellEvent::LogoutRequested) {
                    seen.set(seen.get() + 1);
                }
            }),
        };
        let mut frame = eframe::Frame::_new_kittest();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx, &mut frame));
        assert_eq!(events.get(), 1);
        assert_eq!(app.shell.frame_no(), 0);
        Ok(())
    }
}

impl Options {
    /// Customize an extensible configuration without an exhaustive literal.
    #[must_use]
    pub fn configure(mut self, customize: impl FnOnce(&mut Self)) -> Self {
        customize(&mut self);
        self
    }
}
