//! **Nothing outgrows its room**: a button asked for more width than there is takes
//! the width there is.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::BigButton;
use fairing::{layout, screen, Cx, Shell};

/// What the screen measured this frame.
#[derive(Default)]
struct Bench {
    /// The button's rect, and the room it was given.
    button: Option<(egui::Rect, f32)>,
}

fn bench() -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("b", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let mut button = None;
            layout::page(ui, cx, "bench", |ui, cx| {
                // A column narrower than the button's wish.
                ui.scope(|ui| {
                    ui.set_max_width(200.0);
                    let room = ui.available_width();
                    let rect = BigButton::new("Pay ₩0")
                        .min_size(egui::vec2(400.0, 48.0))
                        .show(ui, &mut cx.widgets())
                        .response
                        .rect;
                    button = Some((rect, room));
                });
            });
            if let Some(b) = cx.app_mut::<Bench>() {
                b.button = button;
            }
        }));
        shell.launch(fairing::LaunchAction::open("b"));
        Ok(shell)
    })?
    .with_app(Bench::default());
    h.frames(10);
    Ok(h)
}

/// **A button asked for more than its room takes the room**, whole, and no more.
#[test]
fn a_button_wider_than_its_room_takes_the_room() -> fairing::Result<()> {
    let mut h = bench()?;
    let (rect, room) = h
        .app_mut::<Bench>()
        .and_then(|b| b.button)
        .unwrap_or((egui::Rect::NOTHING, 0.0));
    assert!(
        room > 0.0 && room < 300.0,
        "the bench wants a narrow room, it has {room}"
    );
    assert!(
        rect.width() <= room + 0.5,
        "asked for 400 in a room of {room}, the button took {}",
        rect.width()
    );
    assert!(
        rect.width() >= room - 0.5,
        "the button did not take the room it was given: {} of {room}",
        rect.width()
    );
    Ok(())
}
