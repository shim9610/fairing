//! **A shell built from inside a frame must not draw in that frame**.
//!
//! `ShellBuilder::build` installs the fonts, but `Context::set_fonts` does not apply in the pass it
//! is called from — egui swaps the definitions in at the start of the *next* pass. Until then this
//! crate's named families are not bound, and epaint does not fall back on an unbound family:
//!
//! ```text
//! FontFamily::Name("fairing-strong") is not bound to any fonts
//! ```
//!
//! So the first `Theme::strong` draw takes the process down. It is not a rare shape either —
//! `eframe::App::update` gives you the context inside a pass, which is where an integrator would
//! naturally build. It cost the kiosk example every screenshot: its first screen is a grid of
//! `MediaCard`s and a card titles itself in the strong face, so the tour died before shot one.

#![cfg(feature = "mock")]

use fairing::testing::single_level_access;
use fairing::{screen, Cx, Shell};
use std::cell::Cell;
use std::rc::Rc;

/// Run one pass, building the shell **inside** it, and draw a label in the strong face.
///
/// Returns how many times the screen's body ran, so the caller can tell "did not draw" from
/// "drew nothing".
fn pass_building_inside(ctx: &egui::Context, drawn: &Rc<Cell<u32>>) -> fairing::Result<()> {
    let d = Rc::clone(drawn);
    let mut out = Ok(());
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        // Built from **inside** the pass, which is what `eframe::App::update` hands an integrator.
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

/// The crash, reproduced: build inside a pass, then draw the strong face in that same pass.
///
/// Before the guard in `Shell::frame_inner` this panicked inside epaint. It has to come back
/// without drawing instead — and then draw normally once the fonts are live.
#[test]
fn a_shell_built_inside_a_pass_waits_for_its_fonts() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let drawn = Rc::new(Cell::new(0));

    pass_building_inside(&ctx, &drawn)?;
    assert_eq!(
        drawn.get(),
        0,
        "the screen must not be drawn in the pass the fonts were installed in — that draw is the \
         panic this test exists for"
    );

    // The definitions land at the start of the next pass, so a shell built in *that* one is fine.
    pass_building_inside(&ctx, &drawn)?;
    assert_eq!(
        drawn.get(),
        1,
        "once the families are live the shell has to draw as usual, or the guard is a hang rather \
         than a fix"
    );
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
