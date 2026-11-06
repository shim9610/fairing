//! Concrete witness for an unmediated user painter, without changing product code.
use fairing::{egui, Gate, Shell, ShellConfig};
use std::{cell::Cell, rc::Rc};

const SECRET: &str = "MEDIATION_PROBE_PRIVATE_RESULT_71493";
struct State {
    secret: String,
    writes: usize,
}

fn contains(shape: &egui::Shape, needle: &str) -> bool {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().any(|s| contains(s, needle)),
        egui::Shape::Text(text) => text.galley.job.text.contains(needle),
        _ => false,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let expected = std::env::args().any(|a| a == "--expect-counterexample");
    let ctx = egui::Context::default();
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    config.motion.reduce = true;
    let calls = Rc::new(Cell::new(0usize));
    let denied = Rc::new(Cell::new(true));
    let (observed, permission) = (Rc::clone(&calls), Rc::clone(&denied));
    let mut shell = Shell::builder(config)
        .status_bar_painter(move |ui, bar| {
            observed.set(observed.get() + 1);
            permission.set(permission.get() && !bar.cx.allows("probe.private-result"));
            if let Some(state) = bar.cx.app_mut::<State>() {
                state.writes += 1;
                ui.label(&state.secret);
            }
        })
        .build_protected(&ctx)?;
    let mut state = State {
        secret: SECRET.into(),
        writes: 0,
    };
    let mut exposed = false;
    for frame in 0..32 {
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                time: Some(f64::from(frame) / 60.0),
                ..Default::default()
            },
            |ui| shell.frame_with(ui, &mut state),
        );
        exposed |= output.shapes.iter().any(|s| contains(&s.shape, SECRET));
        output.drop_without_applying_deltas();
    }
    let unauthenticated = shell.shell().access().session_token().is_none();
    let required_denied = !shell
        .shell()
        .access()
        .allows(&Gate::from("probe.private-result"));
    let confirmed = unauthenticated
        && required_denied
        && denied.get()
        && calls.get() > 0
        && state.writes > 0
        && exposed;
    println!("{{\"status\":\"{}\",\"callback_calls\":{},\"unauthenticated\":{},\"required_permission_denied\":{},\"app_state_writes\":{},\"secret_in_output_shapes\":{}}}",if confirmed{"counterexample-confirmed"}else{"not-reproduced"},calls.get(),unauthenticated,required_denied,state.writes,exposed);
    if !confirmed {
        return Err(
            "Mediation witness did not reproduce; inspect source/model correspondence".into(),
        );
    }
    if !expected {
        eprintln!("UNMEDIATED_CALLBACK_CONFIRMED");
        std::process::exit(2);
    }
    Ok(())
}
