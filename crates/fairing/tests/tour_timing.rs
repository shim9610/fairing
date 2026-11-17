//! The production tour input driver must give the same drag/coast across renderer rates.
#![cfg(feature = "runner")]
#![allow(
    clippy::cast_precision_loss,
    reason = "small frame and row counts in a GUI regression"
)]

#[path = "../examples/common/mod.rs"]
mod common;

fn drag_and_coast(renderer_step: f64, tour_clock: bool) -> f32 {
    use fairing::egui;
    let ctx = egui::Context::default();
    if tour_clock {
        common::SyntheticInput::install_tour(&ctx);
    } else {
        // Independent nominal 60 Hz input, without the tour-clock implementation.
        common::SyntheticInput::install(&ctx);
    }
    let mut offset = 0.0;
    for frame in 0..120 {
        if frame == 3 {
            common::SyntheticInput::press(&ctx, egui::pos2(300.0, 400.0));
        } else if (4..=13).contains(&frame) {
            common::SyntheticInput::move_to(
                &ctx,
                egui::pos2(300.0, 400.0 - (frame - 3) as f32 * 20.0),
            );
        } else if frame == 14 {
            common::SyntheticInput::release(&ctx, egui::pos2(300.0, 200.0));
        }
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(100.0 + f64::from(frame) * renderer_step),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                ..Default::default()
            },
            |ui| {
                let scroll = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_source(egui::containers::scroll_area::ScrollSource::ALL)
                    .show(ui, |ui| {
                        for row in 0..150 {
                            ui.label(format!("Visible row {row}"));
                        }
                    });
                offset = scroll.state.clamped_offset().y;
                ctx.request_repaint();
            },
        );
        output.textures_delta.clear();
    }
    offset
}

#[test]
fn unrecorded_tour_scroll_does_not_depend_on_renderer_speed() {
    let reference = drag_and_coast(1.0 / 60.0, false);
    assert!(
        reference > 100.0,
        "the real ScrollArea must have moved: {reference}"
    );
    for renderer_step in [1.0 / 240.0, 1.0 / 60.0, 1.0 / 15.0] {
        let observed = drag_and_coast(renderer_step, true);
        assert!((observed-reference).abs() < 0.1,
            "the same tour drag/coast changed with renderer step {renderer_step}: {observed} versus {reference}");
    }
}
