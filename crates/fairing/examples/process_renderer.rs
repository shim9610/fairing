//! Linux UI worker. Start through fairing-process's `work_engine` example, not directly.
use fairing::process::{Error, Frame, RenderToken, Worker};
use std::sync::Arc;

struct Display {
    worker: Worker,
    token: Option<RenderToken>,
    ticks: u64,
    pending: Option<Frame>,
}
impl Display {
    fn flush(&mut self) -> Result<bool, Error> {
        if let Some(frame) = self.pending.take() {
            match self.worker.try_commit(frame) {
                Ok(()) => {}
                Err(error) if error.kind() == Error::Full => {
                    self.pending = Some(error.into_frame());
                    return Ok(false);
                }
                Err(error) => return Err(error.kind()),
            }
        }
        Ok(true)
    }
    fn update_model(&mut self, ctx: &fairing::egui::Context) -> Result<(), Error> {
        if !self.flush()? {
            ctx.request_repaint();
            return Ok(());
        }
        if let Some(model) = self.worker.try_snapshot() {
            let bytes: [u8; 8] = model.bytes().try_into().map_err(|_| Error::Protocol)?;
            self.ticks = u64::from_le_bytes(bytes);
            self.token = Some(model.token());
            // Model-applied receipt. This does not claim the compositor displayed it.
            self.pending = Some(self.worker.frame(self.token)?);
            let _ = self.flush()?;
            ctx.request_repaint();
        }
        Ok(())
    }
}
impl eframe::App for Display {
    fn logic(&mut self, ctx: &fairing::egui::Context, _frame: &mut eframe::Frame) {
        if self.worker.is_closed() || self.update_model(ctx).is_err() {
            ctx.send_viewport_cmd(fairing::egui::ViewportCommand::Close);
        }
    }
    fn ui(&mut self, ui: &mut fairing::egui::Ui, _frame: &mut eframe::Frame) {
        ui.heading("Independent display");
        ui.label(format!("Work completed: {} ticks", self.ticks));
        if ui
            .add_enabled(
                self.token.is_some() && self.pending.is_none(),
                fairing::egui::Button::new("Acknowledge"),
            )
            .clicked()
        {
            if let Ok(mut frame) = self.worker.frame(self.token) {
                let mut action = [0u8; 9];
                if let Some(opcode) = action.first_mut() {
                    *opcode = 1;
                }
                if let Some(value) = action.get_mut(1..) {
                    value.copy_from_slice(&self.ticks.to_le_bytes());
                }
                if frame.try_action(&action).is_ok() {
                    self.pending = Some(frame);
                }
            }
        }
        if self.flush().is_err() {
            ui.ctx()
                .send_viewport_cmd(fairing::egui::ViewportCommand::Close);
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Admission/resource-limit bootstrap precedes the window, GL and font initialization.
    let worker = Worker::connect()?;
    eframe::run_native(
        "Independent display",
        eframe::NativeOptions {
            viewport: fairing::egui::ViewportBuilder::default().with_inner_size([480.0, 240.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let context = cc.egui_ctx.clone();
            worker.set_waker(Arc::new(move || context.request_repaint()))?;
            Ok(Box::new(Display {
                worker,
                token: None,
                ticks: 0,
                pending: None,
            }))
        }),
    )
    .map_err(|error| error.to_string().into())
}
