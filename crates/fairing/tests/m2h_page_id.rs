//! **A page's id is its scroll position's identity** — `layout::page`.
//!
//! Nothing else tells two pages apart: three tabs drawn through one call with one id share one
//! offset, so a tab scrolled halfway opens the next tab halfway (the console example's three
//! boards did; an integrator who built on it saw it). The id takes anything that hashes, so
//! `("board", tab)` is a page each.

use fairing::testing::{single_level_access, Harness};
use fairing::{layout, screen, Cx, Shell};

/// Which tab is up, and where its first row was drawn last frame.
#[derive(Default)]
struct Tabs {
    tab: usize,
    first_row_top: f32,
}

/// A screen of two tabs, each a long page: under one id, or one id each.
fn harness(one_id: bool) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("t", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            cx.with_app::<Tabs, _>(|t, cx| {
                let tab = if one_id { 0 } else { t.tab };
                layout::page(ui, cx, ("tab", tab), |ui, _cx| {
                    for i in 0..80 {
                        let row = ui.label(format!("row {i}"));
                        if i == 0 {
                            t.first_row_top = row.rect.top();
                        }
                    }
                });
            });
        }));
        shell.launch(fairing::LaunchAction::open("t"));
        Ok(shell)
    })?
    .with_app(Tabs::default());
    h.set_size(800.0, 480.0);
    h.run_for(1.0);
    Ok(h)
}

/// Scroll the first tab down, switch to the second, and say where the second's first row is.
fn second_tab_top_after_scrolling_the_first(h: &mut Harness) -> (f32, f32) {
    let at_rest = h.app_mut::<Tabs>().map_or(0.0, |t| t.first_row_top);
    let middle = h.screen_rect().center();
    h.wheel(middle, egui::vec2(0.0, -600.0));
    // egui smooths a wheel over several frames: all of it has to land on the first tab.
    h.run_for(0.5);
    let scrolled = h.app_mut::<Tabs>().map_or(0.0, |t| t.first_row_top);
    assert!(
        scrolled < at_rest - 100.0,
        "the first tab has to have scrolled: its first row went {at_rest} -> {scrolled}"
    );
    if let Some(t) = h.app_mut::<Tabs>() {
        t.tab = 1;
    }
    h.run_for(0.5);
    let second = h.app_mut::<Tabs>().map_or(0.0, |t| t.first_row_top);
    (at_rest, second)
}

#[test]
fn each_page_id_keeps_its_own_scroll_position() -> fairing::Result<()> {
    let mut h = harness(false)?;
    let (at_rest, second) = second_tab_top_after_scrolling_the_first(&mut h);
    assert!(
        (second - at_rest).abs() < 1.0,
        "the second tab has its own id and opens at the top: its first row is at {second}, rest \
         is {at_rest}"
    );
    Ok(())
}

#[test]
fn pages_under_one_id_share_one_scroll_position() -> fairing::Result<()> {
    let mut h = harness(true)?;
    let (at_rest, second) = second_tab_top_after_scrolling_the_first(&mut h);
    assert!(
        second < at_rest - 100.0,
        "under one id the second tab opens where the first was left: its first row is at \
         {second}, rest is {at_rest}"
    );
    Ok(())
}
