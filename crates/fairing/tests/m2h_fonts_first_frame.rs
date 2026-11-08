//! A shell built inside a pass draws only once its named font families are live.
//! egui 0.37 applies fonts immediately before text layout; after text layout the change
//! is deferred; the tests check first-pass drawing and drawing on the following pass.

#![cfg(feature = "mock")]

use fairing::testing::single_level_access;
use fairing::{screen, Cx, Shell};
use std::cell::Cell;
use std::rc::Rc;

/// Run one pass, building the shell **inside** it, and draw a label in the strong face.
///
/// Returns how many times the screen's body ran, so the caller can tell "did not draw" from
/// "drew nothing".
fn pass_building_inside(
    ctx: &egui::Context,
    drawn: &Rc<Cell<u32>>,
    text_laid_out: bool,
) -> fairing::Result<()> {
    let d = Rc::clone(drawn);
    let mut out = Ok(());
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        if text_laid_out {
            ui.label("text laid out before font installation");
        }
        // Build inside the pass, before or after the first text layout.
        let mut shell = match Shell::builder(single_level_access()).build(ui.ctx()) {
            Ok(shell) => shell,
            Err(err) => {
                out = Err(err);
                return;
            }
        };
        let d = Rc::clone(&d);
        shell.add(screen("s", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            d.set(d.get() + 1);
            ui.label(
                egui::RichText::new("strong")
                    .font(cx.theme.strong(cx.theme.metrics.type_scale.body)),
            );
        }));
        shell.launch(fairing::LaunchAction::open("s"));
        shell.frame(ui);
    });
    // The texture deltas a pass produces have to be taken or explicitly dropped; nothing here
    // paints to a screen, so they are dropped.
    output.drop_without_applying_deltas();
    out
}

/// Immediate and deferred font installation reach drawing on the expected pass.
#[test]
fn a_shell_built_inside_a_pass_obeys_font_readiness() -> fairing::Result<()> {
    for text_laid_out in [false, true] {
        let ctx = egui::Context::default();
        let drawn = Rc::new(Cell::new(0));
        pass_building_inside(&ctx, &drawn, text_laid_out)?;
        let first = u32::from(!text_laid_out);
        assert_eq!(
            drawn.get(),
            first,
            "only live fonts may draw in the first pass"
        );
        pass_building_inside(&ctx, &drawn, text_laid_out)?;
        assert_eq!(
            drawn.get(),
            first + 1,
            "the next pass draws without remaining stuck"
        );
    }
    Ok(())
}

/// The guard's own question, asked directly: a fresh context has none of our families.
#[test]
fn a_fresh_context_does_not_have_our_families() {
    let ctx = egui::Context::default();
    let mut live = true;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        live = fairing::fonts::families_are_live(ui.ctx());
    })
    .drop_without_applying_deltas();
    assert!(
        !live,
        "egui's own defaults cannot contain `fairing-strong`, so this has to be false — if it is \
         ever true the guard stops guarding anything"
    );
}
