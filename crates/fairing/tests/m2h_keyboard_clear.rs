//! **A page keeps the field being typed into clear of the keyboard** — `layout::page`.
//!
//! The keyboard lies over the bottom of the pane and pushes nothing. A page already padded its
//! foot by the keyboard's height so its rows *could* be scrolled clear, but nothing scrolled
//! them: a field in the lower half of the screen was typed into blind. Now the page scrolls the
//! focused widget up while the keyboard rises, and then leaves the page to the operator.

#![cfg(feature = "osk")]

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::TextField;
use fairing::{layout, screen, Cx, Shell};

/// The screen's state: where the field was drawn, and what was typed.
struct Page {
    field: egui::Rect,
    typed: String,
}

/// The keyboard's rect, where it is up.
fn keyboard(h: &Harness) -> Option<egui::Rect> {
    if !h.shell.osk().is_visible() {
        return None;
    }
    h.ctx.memory(|m| m.area_rect(egui::Id::new("fairing.osk")))
}

fn field(h: &mut Harness) -> egui::Rect {
    h.app_mut::<Page>().map_or(egui::Rect::NOTHING, |p| p.field)
}

/// A page with a text field in the lower half of a short screen, and rows past it to scroll to.
fn harness() -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("p", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "p", |ui, cx| {
                ui.add_space(260.0);
                cx.with_app::<Page, _>(|p, cx| {
                    p.field = TextField::new(&mut p.typed)
                        .id_salt("typed")
                        .show(ui, &mut cx.widgets())
                        .rect;
                });
                for i in 0..12 {
                    layout::note(ui, cx, &format!("row {i}"));
                }
            });
        }));
        shell.launch(fairing::LaunchAction::open("p"));
        Ok(shell)
    })?
    .with_app(Page {
        field: egui::Rect::NOTHING,
        typed: String::new(),
    });
    h.set_size(1024.0, 600.0);
    h.run_for(1.0);
    Ok(h)
}

#[test]
fn the_page_scrolls_a_covered_field_clear_of_the_keyboard() -> fairing::Result<()> {
    let mut h = harness()?;
    let before = field(&mut h);
    assert!(before.height() > 0.0, "no field was drawn");
    h.tap(before.center());
    h.run_for(1.5);
    let Some(keys) = keyboard(&h) else {
        return Err(fairing::Error::Config(
            "the keyboard did not come up for the field".to_owned(),
        ));
    };
    assert!(
        before.max.y > keys.min.y,
        "the field has to start out where the keyboard would cover it: field {before:?}, keys \
         {keys:?}"
    );
    let after = field(&mut h);
    assert!(
        after.max.y <= keys.min.y + 0.5,
        "the page did not scroll the field clear of the keyboard: field {after:?}, keys {keys:?}"
    );
    let row = h.shell.theme().metrics.row_height;
    assert!(
        keys.min.y - after.max.y <= row * 1.5,
        "and no further than it had to - the page is the operator's: field {after:?}, keys \
         {keys:?}"
    );
    Ok(())
}

#[test]
fn a_page_scrolled_by_hand_while_typing_stays_where_it_was_put() -> fairing::Result<()> {
    let mut h = harness()?;
    let before = field(&mut h);
    h.tap(before.center());
    h.run_for(1.5);
    let clear = field(&mut h);
    // A drag down the empty space above the field scrolls the content down again, back under
    // the keys.
    let from = egui::pos2(500.0, clear.min.y - 30.0);
    h.drag(from, from + egui::vec2(0.0, 120.0), 8);
    h.run_for(1.0);
    let moved = field(&mut h);
    assert!(
        moved.min.y > clear.min.y + 40.0,
        "the drag has to have scrolled the page: {clear:?} -> {moved:?}"
    );
    h.run_for(1.0);
    let later = field(&mut h);
    assert!(
        (later.min.y - moved.min.y).abs() < 1.0,
        "and the page must not snap back while the keyboard stays up: {moved:?} -> {later:?}"
    );
    Ok(())
}
