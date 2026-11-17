//! A real-window smoke test of the protected example, using its actual authenticator.
use super::{build, common};
use common::SyntheticInput;
use fairing::{egui, runner, ProtectedShell};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

enum Step {
    Text(&'static str),
    Read(&'static str),
    Tap(&'static str),
    TapOldButton,
    LoggedOut,
    Count(usize),
    Shot(&'static str),
}

const PLAN: &[Step] = &[
    Step::Text("Development only: type DEMO and press Enter"),
    Step::Shot("01-login.png"),
    Step::Read("WRONG"),
    Step::Text("Type the public demonstration marker DEMO"),
    Step::Read("DEMO"),
    Step::Text("Operations: 0"),
    Step::Tap("Operate"),
    Step::Count(1),
    Step::Text("Operations: 1"),
    Step::Shot("02-authorized.png"),
    Step::Tap("Log out and request a new session"),
    Step::LoggedOut,
    Step::TapOldButton,
    Step::LoggedOut,
    Step::Count(1),
    Step::Shot("03-logged-out.png"),
    Step::Read("DEMO"),
    Step::Text("Operations: 1"),
    Step::Tap("Operate"),
    Step::Count(2),
    Step::Text("Operations: 2"),
    Step::Shot("04-reauthenticated.png"),
];

#[derive(Default)]
struct Tour {
    frame: usize,
    step: usize,
    release: Option<egui::Pos2>,
    old_button: Option<egui::Pos2>,
    pending: usize,
    written: usize,
}

impl Tour {
    fn tick(
        &mut self,
        ctx: &egui::Context,
        shell: &ProtectedShell,
        count: usize,
        dir: &std::path::Path,
    ) -> Result<bool, String> {
        self.frame += 1;
        if self.frame > 1_200 {
            return Err("protected tour timed out, including screenshot delivery".into());
        }
        for (path, image) in common::frame_shots(ctx) {
            common::write_png(&path, &image).map_err(|err| err.to_string())?;
            self.pending -= 1;
            self.written += 1;
        }
        if let Some(pos) = self.release.take() {
            SyntheticInput::release(ctx, pos);
        }
        let Some(step) = PLAN.get(self.step) else {
            return Ok(self.pending == 0);
        };
        // Half a second on the shared virtual clock lets the real prompt/screen settle.
        if !self.frame.is_multiple_of(30) {
            return Ok(false);
        }
        let texts = common::drawn_texts(ctx);
        match step {
            Step::Text(label) => {
                if !texts.iter().any(|(text, _)| text == label) {
                    return Err(format!(
                        "step {}: missing {label:?}; visible: {texts:?}",
                        self.step
                    ));
                }
            }
            Step::Read(value) => {
                SyntheticInput::push(ctx, egui::Event::Text((*value).into()));
                for pressed in [true, false] {
                    SyntheticInput::push(
                        ctx,
                        egui::Event::Key {
                            key: egui::Key::Enter,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        },
                    );
                }
            }
            Step::Tap(label) => {
                let pos = texts
                    .iter()
                    .find(|(text, _)| text == label)
                    .map(|(_, rect)| rect.center())
                    .ok_or_else(|| format!("step {}: button {label:?} is missing", self.step))?;
                if *label == "Operate" {
                    self.old_button = Some(pos);
                }
                SyntheticInput::press(ctx, pos);
                self.release = Some(pos);
            }
            Step::TapOldButton => {
                let pos = self.old_button.ok_or("no previous Operate button")?;
                SyntheticInput::press(ctx, pos);
                self.release = Some(pos);
            }
            Step::LoggedOut => {
                if shell.observe().session_token().is_some()
                    || texts.iter().any(|(text, _)| {
                        text == "Protected application" || text.starts_with("Operations:")
                    })
                {
                    return Err("logged-out session still exposes protected content".into());
                }
            }
            Step::Count(expected) => {
                if count != *expected {
                    return Err(format!("operation count: expected {expected}, got {count}"));
                }
            }
            Step::Shot(name) => {
                common::ask_for_frame(ctx, dir.join(name));
                self.pending += 1;
            }
        }
        self.step += 1;
        Ok(false)
    }
}

pub(super) fn run(options: runner::Options, dir: PathBuf) -> fairing::Result<()> {
    std::fs::create_dir_all(&dir).map_err(|err| fairing::Error::Runner(err.to_string()))?;
    let failure = Rc::new(RefCell::new(None));
    let reported = Rc::clone(&failure);
    let mut app = None;
    let mut tour = Tour::default();
    let mut finished = false;
    runner::run(options, move |ui| {
        let ctx = ui.ctx().clone();
        if finished {
            return;
        }
        let result = (|| {
            if app.is_none() {
                ctx.set_pixels_per_point(1.0);
                SyntheticInput::install_tour(&ctx);
                app = Some(build(&ctx).map_err(|err| err.to_string())?);
            }
            let (shell, state) = app.as_mut().ok_or("protected app not built")?;
            shell.frame_with(ui, state);
            let _ = shell.poll_events();
            tour.tick(&ctx, shell, *state, &dir)
        })();
        match result {
            Ok(false) => ctx.request_repaint(),
            result => {
                if let Err(err) = result {
                    log::error!("protected tour: {err}");
                    *reported.borrow_mut() = Some(err);
                } else {
                    log::info!("tour finished - {} PNGs", tour.written);
                }
                finished = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    })?;
    let error = failure.borrow_mut().take();
    error.map_or(Ok(()), |err| Err(fairing::Error::Runner(err)))
}
