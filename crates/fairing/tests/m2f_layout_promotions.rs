//! The three that **came up** into the crate from the kiosk harness (harness findings 2 · 4 · 6).
//!
//! What this file holds to is not the values but **why they came up**. All three were places that had an integrator
//! writing the same thing again, so a regression has the examples quietly reviving the old code.
//!
//! - [`Grid::max_columns`](fairing::layout::Grid::max_columns) — without it, the five lines that work the minimum
//!   cell back out of the width were copied into six places.
//! - [`fit_size`](fairing::layout::fit_size) — shrinking per cell gives sibling tiles text at different sizes from
//!   each other. It did not show in Korean and did show once switched to English.
//! - The `ButtonKind::Normal` border — a button disappeared on a card of the same colour, and **two examples,
//!   knowing nothing of each other**, each wrote their own way round it.
//! - [`Grid::fill_height`](fairing::layout::Grid::fill_height) — the grid did not use the vertical space left, so
//!   the bottom half of a panel stood on end was empty outright. Four places measured
//!   `height left / row count / row_height` themselves and sat it once more with `add_space`.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

#![cfg(feature = "mock")]

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::{BigButton, ButtonKind};
use fairing::{layout, screen, Cx, LaunchAction};
use std::cell::RefCell;
use std::rc::Rc;

fn harness(w: f32, h: f32) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    Ok(Harness::new(config, fairing::services::Services::null())?.with_size(w, h))
}

/// Harness finding 2: **the column cap.** It stops at the cap however much width is left.
///
/// It is compared against having no cap — at the same width and the same minimum cell, adding the cap alone has to
/// give fewer columns and wider tiles. "The cap went in and the column count is the same" is a knob that does not turn.
#[test]
fn max_columns_caps_the_grid_and_widens_the_tiles() -> fairing::Result<()> {
    fn measure(cap: Option<usize>) -> fairing::Result<(usize, f32)> {
        let mut h = harness(1400.0, 700.0)?;
        let seen: Rc<RefCell<Vec<egui::Rect>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        h.shell.add(
            screen("g", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                sink.borrow_mut().clear();
                let mut grid = layout::Grid::new(2.0, 2.0);
                if let Some(n) = cap {
                    grid = grid.max_columns(n);
                }
                grid.show(ui, cx, &[0_u8, 1, 2, 3, 4, 5], |_, _, cell| {
                    sink.borrow_mut().push(cell.rect);
                });
            })
            .title("g"),
        );
        h.shell.handle().launch(LaunchAction::open("g"));
        h.frames(3);
        let rects = seen.borrow().clone();
        let Some(first) = rects.first() else {
            return Err(fairing::Error::Config("the grid was not drawn".to_owned()));
        };
        // How many cells on the first row = how many share a y.
        let cols = rects
            .iter()
            .filter(|r| (r.top() - first.top()).abs() < 0.5)
            .count();
        Ok((cols, first.width()))
    }

    let (wide_cols, wide_w) = measure(None)?;
    assert!(
        wide_cols >= 4,
        "{wide_cols} columns at width 1400 with no cap"
    );

    let (capped_cols, capped_w) = measure(Some(3))?;
    assert_eq!(
        capped_cols, 3,
        "the cap is 3 and there are {capped_cols} columns"
    );
    assert!(
        capped_w > wide_w,
        "fewer columns has to give wider tiles: {capped_w} <= {wide_w}"
    );

    // Where the width allows fewer than the cap, the cap does not step in — it does not force 3 columns onto a
    // narrow panel (that would take the tiles below the touch target).
    let mut narrow = harness(360.0, 700.0)?;
    let seen: Rc<RefCell<Vec<egui::Rect>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    narrow.shell.add(
        screen("g", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            sink.borrow_mut().clear();
            layout::Grid::new(2.0, 2.0).max_columns(3).show(
                ui,
                cx,
                &[0_u8, 1, 2, 3, 4, 5],
                |_, _, cell| {
                    sink.borrow_mut().push(cell.rect);
                },
            );
        })
        .title("g"),
    );
    narrow.shell.handle().launch(LaunchAction::open("g"));
    narrow.frames(3);
    let rects = seen.borrow().clone();
    let Some(first) = rects.first() else {
        return Err(fairing::Error::Config("the grid was not drawn".to_owned()));
    };
    let cols = rects
        .iter()
        .filter(|r| (r.top() - first.top()).abs() < 0.5)
        .count();
    assert!(
        cols <= 3,
        "{cols} columns on a narrow panel — the cap behaved like a floor"
    );
    assert!(
        first.width() >= narrow.shell.theme().metrics.touch_target,
        "the tile width {} is below the touch target",
        first.width()
    );
    Ok(())
}

/// Harness finding 4: **sibling cells use the same text size.**
///
/// This is the defect `--lang=en` brought out — the Korean `결제하기` (4 glyphs) fitted whole while only the English
/// `Cash · Call Staff` (17 glyphs) shrank, so three tiles standing side by side had text at three different sizes.
#[test]
fn fit_size_picks_one_size_for_every_sibling() -> fairing::Result<()> {
    let mut h = harness(900.0, 600.0)?;
    let got: Rc<RefCell<Vec<f32>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&got);
    h.shell.add(
        screen("t", move |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            let labels = [
                "Short",
                "A bit longer",
                "This is a very much longer label indeed",
            ];
            let base = egui::FontId::proportional(40.0);
            let max_w = 200.0;
            let mut out = sink.borrow_mut();
            out.clear();
            // The size fitted as a set: all three have to be equal.
            let uniform = layout::fit_size(ui.painter(), &labels, max_w, base.clone());
            out.push(uniform.size);
            // The size shrunk per cell: it differs per label.
            for label in labels {
                let solo = layout::fit_size(
                    ui.painter(),
                    std::slice::from_ref(&label),
                    max_w,
                    base.clone(),
                );
                out.push(solo.size);
            }
        })
        .title("t"),
    );
    h.shell.handle().launch(LaunchAction::open("t"));
    h.frames(3);
    let out = got.borrow().clone();
    let Some((uniform, solos)) = out.split_first() else {
        return Err(fairing::Error::Config("nothing was measured".to_owned()));
    };
    let Some(shortest) = solos.first() else {
        return Err(fairing::Error::Config("nothing was measured".to_owned()));
    };
    let Some(longest) = solos.last() else {
        return Err(fairing::Error::Config("nothing was measured".to_owned()));
    };
    // Shrinking per cell leaves the short label alone and shrinks only the long one — that was the problem.
    assert!(
        shortest > longest,
        "shrunk separately and the sizes are equal: {shortest} vs {longest}"
    );
    // Fitted as a set it goes by **the longest**.
    assert!(
        (uniform - longest).abs() < 0.01,
        "the set size {uniform} differs from the longest label's {longest}"
    );
    Ok(())
}

/// Harness finding 6: **a `Normal` button shows even on a card of the same colour.**
///
/// The background used to be `SurfaceVariant` while a [`layout::group`] card was `SurfaceVariant` too, so the button
/// disappeared outright. It carries an edge — looked at through whether the shapes drawn hold **a stroke where the
/// button is**. `Primary` has a strong enough ground not to carry one.
///
/// **The edge is `Muted`, not `Outline`.** It was `Outline` until the control vocabulary, and this
/// assertion passed the whole time while the border it asserted was not visible: measured against
/// the `SurfaceVariant` card it sits on, `Outline` is 1.18 / 1.50 / 1.15 / 1.31 across the four
/// shipped palettes — under the 1.5 where an edge starts to exist at all. `Muted` is 5.08 / 5.95 /
/// 4.93 / 4.03. A test that checks a stroke is *drawn* and not that it can be *seen* is exactly how
/// a fix stays broken, so the role is named here rather than left to whatever the button happens
/// to use.
#[test]
fn a_normal_button_draws_an_outline_but_a_primary_one_does_not() -> fairing::Result<()> {
    fn stroked(kind: ButtonKind) -> fairing::Result<bool> {
        let mut h = harness(600.0, 400.0)?;
        h.shell.add(
            screen("b", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                layout::group(ui, cx, |ui, cx| {
                    let _ = BigButton::new("Button")
                        .kind(kind)
                        .show(ui, &mut cx.widgets());
                });
            })
            .title("b"),
        );
        h.shell.handle().launch(LaunchAction::open("b"));
        h.frames(3);
        // `ControlEdge`, which is what a control's boundary is now: the same job `Muted` was
        // doing here, moved off the 4.5 text floor onto WCAG 1.4.11's own 3.0. A neutral button's
        // face is a tint of the ink rather than an opaque role, so this edge is the whole of what
        // says where the button ends.
        let edge = h.shell.theme().color(fairing::ColorRole::ControlEdge);
        Ok(h.frame_shapes().iter().any(|s| match &s.shape {
            egui::Shape::Rect(r) => r.stroke.width > 0.0 && r.stroke.color == edge,
            _ => false,
        }))
    }

    assert!(
        stroked(ButtonKind::Normal)?,
        "the Normal button has no ControlEdge border — nothing says where it ends"
    );
    assert!(
        !stroked(ButtonKind::Primary)?,
        "the Primary button was given a border too — its ground is strong and it goes muddy"
    );
    Ok(())
}

/// Harness finding 3: **the cells take the vertical space left.** But they stop at a ratio of the width.
///
/// Three things at once — does filling make the cells bigger, does the cap bite, and once the cap bites is the
/// height left **shared above and below**. Without the third the grid sticks to the ceiling and the examples go
/// back to putting `add_space` in by hand.
#[test]
fn fill_height_grows_the_cell_but_stops_at_the_aspect_cap() -> fairing::Result<()> {
    // 2 cells × 1 row = a layout with the whole vertical left over. At a height of 800 the floor is one row.
    fn measure(aspect: Option<f32>) -> fairing::Result<(egui::Rect, f32)> {
        let mut h = harness(700.0, 800.0)?;
        let seen: Rc<RefCell<Vec<egui::Rect>>> = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&seen);
        h.shell.add(
            screen("g", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                sink.borrow_mut().clear();
                let mut grid = layout::Grid::new(2.0, 1.0).max_columns(2);
                if let Some(a) = aspect {
                    grid = grid.fill_height(a);
                }
                grid.show(ui, cx, &[0_u8, 1], |_, _, cell| {
                    sink.borrow_mut().push(cell.rect);
                });
            })
            .title("g"),
        );
        h.shell.handle().launch(LaunchAction::open("g"));
        h.frames(3);
        let rects = seen.borrow().clone();
        let Some(first) = rects.first().copied() else {
            return Err(fairing::Error::Config("the grid was not drawn".to_owned()));
        };
        Ok((first, h.shell.theme().metrics.row_height))
    }

    let (plain, row) = measure(None)?;
    // Without filling it is the 1 row given to `new`, as it was.
    assert!(
        (plain.height() - row).abs() < 1.0,
        "it was not filled and the cell height is {} (1 row = {row})",
        plain.height()
    );

    // A generous cap — it takes the vertical space left.
    let (filled, _) = measure(Some(4.0))?;
    assert!(
        filled.height() > plain.height() * 2.0,
        "it was filled and the cell is only {} (unfilled it is {})",
        filled.height(),
        plain.height()
    );

    // A tight cap — it stops at 0.8 of the width. It has to be above the floor (1 row) for the cap to really bite.
    let (capped, _) = measure(Some(0.8))?;
    assert!(
        capped.height() < filled.height(),
        "the cap is 0.8 and the height did not shrink: {} vs {}",
        capped.height(),
        filled.height()
    );
    assert!(
        (capped.height() - capped.width() * 0.8).abs() < 2.0,
        "the cap has to be 0.8 of the width and it is {} (width {})",
        capped.height(),
        capped.width()
    );

    // Held by the cap, the height left is shared **above and below** — a top of 0 means it stuck to the ceiling.
    assert!(
        capped.top() > filled.top() + 1.0,
        "the cap bit and the grid is stuck to the top (top {} vs {} uncapped)",
        capped.top(),
        filled.top()
    );
    Ok(())
}

/// Harness finding 1: **the vertical reach band.** `split_width`'s vertical counterpart.
///
/// Four things — does it split only on a panel stood on end, does it back off at a height that cannot give two
/// bands, does it clamp the ratio, and does it measure the height it gives back from **the space left**.
///
/// The rule that the judgement goes by the Pane (the same as `split_width`) is not measured here — inside a screen
/// the Pane and the space left differ only by the inset, and a situation where the two give different answers
/// cannot be made headless. That rule is the function's docs' to carry.
#[test]
fn split_height_bands_a_standing_panel_but_not_a_lying_one() -> fairing::Result<()> {
    /// `(the judgement, the height left at that moment)`.
    type Asked = Option<(Option<f32>, f32)>;

    fn ask(w: f32, h: f32, top: f32) -> fairing::Result<(Option<f32>, f32)> {
        let mut harness_ = harness(w, h)?;
        let got: Rc<RefCell<Asked>> = Rc::new(RefCell::new(None));
        let sink = Rc::clone(&got);
        harness_.shell.add(
            screen("s", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                let avail = ui.available_rect_before_wrap().height();
                *sink.borrow_mut() = Some((layout::split_height(ui, cx, top), avail));
            })
            .title("s"),
        );
        harness_.shell.handle().launch(LaunchAction::open("s"));
        harness_.frames(3);
        let out = *got.borrow();
        out.ok_or_else(|| fairing::Error::Config("the screen did not run".to_owned()))
    }

    // A panel stood on end — it splits. The top band is that share of the height left.
    let (band, avail) = ask(500.0, 1600.0, 0.34)?;
    let Some(hero) = band else {
        return Err(fairing::Error::Config(
            "a panel on end and it did not split".to_owned(),
        ));
    };
    assert!(
        (hero - avail * 0.34).abs() < 1.0,
        "the top band is {hero} (0.34 of the height left {avail} = {})",
        avail * 0.34
    );

    // A panel laid down — it does not split. Cutting a wide screen top from bottom flattens both halves.
    assert!(
        ask(1600.0, 500.0, 0.34)?.0.is_none(),
        "it split a panel laid down"
    );

    // A panel tall in shape but low — it does not split. Neither band could hold anything.
    assert!(
        ask(200.0, 320.0, 0.34)?.0.is_none(),
        "it split at a height that cannot give two bands"
    );

    // The ratio is clamped — given 0.9 it still does not push the control band below reach.
    let (wide, avail) = ask(500.0, 1600.0, 0.9)?;
    let Some(hero) = wide else {
        return Err(fairing::Error::Config("it did not split".to_owned()));
    };
    assert!(
        hero < avail * 0.7,
        "it took 0.9 as it was: {hero} (the height left is {avail})"
    );
    Ok(())
}
