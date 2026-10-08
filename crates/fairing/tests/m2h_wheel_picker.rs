//! **A drum always lands on a row, a flick carries further than a drag, and a joined drum rolls
//! round** — `WheelPicker`.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::WheelPicker;
use fairing::{screen, Cx, Shell};

const HOURS: [&str; 24] = [
    "00", "01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14", "15",
    "16", "17", "18", "19", "20", "21", "22", "23",
];

/// The screen's state: the chosen hour, where the drum is, and how often the value changed.
struct Bench {
    selected: usize,
    wrap: bool,
    drum: egui::Rect,
    changes: u32,
}

fn bench(selected: usize, wrap: bool) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("w", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let Some((mut selected, wrap)) = cx.app_mut::<Bench>().map(|b| (b.selected, b.wrap))
            else {
                return;
            };
            let response = WheelPicker::new("w", &HOURS, &mut selected)
                .wrap(wrap)
                .width(200.0)
                .show(ui, &mut cx.widgets());
            if let Some(b) = cx.app_mut::<Bench>() {
                b.selected = selected;
                b.drum = response.rect;
                if response.changed() {
                    b.changes += 1;
                }
            }
        }));
        shell.launch(fairing::LaunchAction::open("w"));
        Ok(shell)
    })?
    .with_app(Bench {
        selected,
        wrap,
        drum: egui::Rect::NOTHING,
        changes: 0,
    })
    .with_size(1024.0, 1000.0);
    h.frames(20);
    Ok(h)
}

fn state(h: &mut Harness) -> (usize, egui::Rect, u32) {
    h.app_mut::<Bench>()
        .map_or((0, egui::Rect::NOTHING, 0), |b| {
            (b.selected, b.drum, b.changes)
        })
}

/// **A drag of one row moves the value by one**, and letting go lands on that row and no
/// further: the drum does not drift.
#[test]
fn a_slow_drag_of_one_row_picks_the_next_value() -> fairing::Result<()> {
    let mut h = bench(5, false)?;
    let (_, drum, _) = state(&mut h);
    assert!(drum.height() > 0.0, "no drum");
    let row_h = drum.height() / 5.0;
    let from = drum.center();
    // Slowly, over many frames, so the release carries no fling.
    h.drag(from, egui::pos2(from.x, from.y - row_h), 40);
    h.run_for(1.5);
    let (selected, _, _) = state(&mut h);
    assert_eq!(
        selected, 6,
        "dragging up by one row should show the next hour"
    );
    Ok(())
}

/// **A flick carries further than the finger went**, and still stops on a row inside the drum.
#[test]
fn a_flick_carries_past_the_finger_and_settles_on_a_row() -> fairing::Result<()> {
    let mut h = bench(0, false)?;
    let (_, drum, _) = state(&mut h);
    let row_h = drum.height() / 5.0;
    let from = drum.center();
    // One row in three frames: fast.
    h.drag(from, egui::pos2(from.x, from.y - row_h), 3);
    h.run_for(2.0);
    let (selected, _, _) = state(&mut h);
    assert!(
        selected >= 2,
        "a flick of one row in three frames should carry past the next hour, not stop at {selected}"
    );
    assert!(selected <= 23, "the drum ran off its end: {selected}");
    Ok(())
}

/// **A tap on the row below the window turns the drum to it** — a finger that missed the
/// window meant that row.
#[test]
fn a_tap_on_a_row_turns_the_drum_to_it() -> fairing::Result<()> {
    let mut h = bench(5, false)?;
    let (_, drum, _) = state(&mut h);
    let row_h = drum.height() / 5.0;
    h.tap(egui::pos2(drum.center().x, drum.center().y + row_h));
    h.run_for(1.0);
    assert_eq!(
        state(&mut h).0,
        6,
        "tapping the row under the window did not turn to it"
    );
    Ok(())
}

/// **An open drum stops at its ends; a joined one rolls round.**
#[test]
fn the_ends_stop_an_open_drum_and_join_a_wrapped_one() -> fairing::Result<()> {
    let mut open = bench(23, false)?;
    let (_, drum, _) = state(&mut open);
    let row_h = drum.height() / 5.0;
    let from = drum.center();
    open.drag(from, egui::pos2(from.x, from.y - row_h * 2.0), 40);
    open.run_for(1.5);
    assert_eq!(
        state(&mut open).0,
        23,
        "an open drum went past its last row"
    );

    let mut joined = bench(23, true)?;
    let (_, drum, _) = state(&mut joined);
    let from = drum.center();
    joined.drag(from, egui::pos2(from.x, from.y - row_h), 40);
    joined.run_for(1.5);
    assert_eq!(
        state(&mut joined).0,
        0,
        "a joined drum did not roll from 23 to 00"
    );
    Ok(())
}

/// **The value follows the window as the drum turns**: `changed()` fires per row passed,
/// not once at the end.
#[test]
fn the_value_changes_as_each_row_passes_the_window() -> fairing::Result<()> {
    let mut h = bench(0, false)?;
    let (_, drum, _) = state(&mut h);
    let row_h = drum.height() / 5.0;
    let from = drum.center();
    h.drag(from, egui::pos2(from.x, from.y - row_h * 3.0), 60);
    h.run_for(1.5);
    let (selected, _, changes) = state(&mut h);
    assert_eq!(selected, 3);
    assert!(
        changes >= 3,
        "three rows passed the window but the value changed {changes} times"
    );
    Ok(())
}
