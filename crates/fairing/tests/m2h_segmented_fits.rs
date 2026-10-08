//! **A caller can ask whether the strip form is available before it draws** —
//! `SegmentedControl::fits`.
//!
//! The control gives the strip up for three reasons, and only one of them is the option count: a
//! cell under the touch target and a label that does not fit its cell are the other two. A caller
//! that has a different control for the crowded case — a console tab row that would rather be a
//! dropdown than a half-screen stack — therefore cannot decide from the count alone. `fits` is
//! that decision, and these say it is the same one `show` makes.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::SegmentedControl;
use fairing::{screen, Cx, Shell};

/// Four labels a 300-point track cannot hold side by side.
const LONG: [&str; 4] = [
    "Display brightness (auto)",
    "Power saving schedule",
    "Network status readout",
    "Storage health check",
];
/// Two that any track holds.
const SHORT: [&str; 2] = ["On", "Off"];

/// What the screen saw: what `fits` answered, and how tall what `show` drew was.
#[derive(Debug, Clone, Copy, Default)]
struct Seen {
    width: f32,
    long: bool,
    fits: bool,
    height: f32,
    room: f32,
}

/// One frame of a control of `n` labels in a `width`-point column.
fn seen(width: f32, long: bool) -> fairing::Result<Seen> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("r", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let Some(s) = cx.app_mut::<Seen>() else {
                return;
            };
            let (width, long) = (s.width, s.long);
            let labels: &[&str] = if long { &LONG } else { &SHORT };
            let room = ui.available_width();
            ui.scope(|ui| {
                ui.set_max_width(width);
                let control = SegmentedControl::new(labels, 0);
                let fits = control.fits(ui, &cx.widgets());
                let pick = control.show(ui, &mut cx.widgets());
                let height = pick.response.rect.height();
                if let Some(s) = cx.app_mut::<Seen>() {
                    s.fits = fits;
                    s.height = height;
                    s.room = room;
                }
            });
        }));
        shell.launch(fairing::LaunchAction::open("r"));
        Ok(shell)
    })?
    .with_size(1600.0, 900.0)
    .with_app(Seen {
        width,
        long,
        ..Seen::default()
    });
    h.frames(3);
    Ok(h.app_mut::<Seen>().copied().unwrap_or_default())
}

/// **The same decision, before and while drawing.** Two short options fit a wide column and the
/// control is one track; four long ones do not fit a narrow one and the control stacks — and
/// `fits` said so both times, without drawing anything.
#[test]
fn fits_answers_what_show_then_draws() -> fairing::Result<()> {
    let strip = seen(600.0, false)?;
    assert!(strip.fits, "two short options fit a 600-point column");

    let stacked = seen(300.0, true)?;
    assert!(
        !stacked.fits,
        "four long labels do not fit a 300-point column"
    );
    assert!(
        stacked.height > strip.height * 1.5,
        "the stacked form is taller than one track: {} vs {}",
        stacked.height,
        strip.height
    );
    Ok(())
}

/// **The count alone is not the answer.** The same four options fit a wide column and not a narrow
/// one, so a caller reading `labels.len()` against `control.segment_max` gets the crowded case
/// wrong exactly where it matters.
#[test]
fn the_same_options_fit_one_column_and_not_another() -> fairing::Result<()> {
    let wide = seen(1500.0, true)?;
    assert!(wide.fits, "wide enough for four: room {}", wide.room);
    assert!(!seen(300.0, true)?.fits, "not at a third of the width");
    Ok(())
}
