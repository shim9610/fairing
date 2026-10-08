//! **A tab bar keeps everything inside its own cell**. Labels were laid out with no wrap
//! and centred on the cell, so a long one ran over its neighbours; a glyph wider than a narrow cell
//! did the same.

use fairing::layout::{tab_bar, Tab};
use fairing::testing::{single_level_access, Harness};
use fairing::{icon, screen, Cx, LaunchAction, Services};
use std::cell::Cell;
use std::rc::Rc;

/// The rect the bar took, recorded by the screen that draws it.
type BarRect = Rc<Cell<egui::Rect>>;

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("missing: {what}")))
}

/// A screen with one tab bar of `labels`, on a panel `width` du wide.
fn tab_screen(labels: &'static [&'static str], width: f32) -> fairing::Result<(Harness, BarRect)> {
    let mut h = Harness::new(single_level_access(), Services::null())?.with_size(width, 600.0);
    let bar = Rc::new(Cell::new(egui::Rect::NOTHING));
    let seen = Rc::clone(&bar);
    let mut selected = 0;
    h.shell
        .add(screen("tabs", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let tabs: Vec<Tab<'_>> = labels
                .iter()
                .map(|label| Tab {
                    label,
                    icon: icon::HOME,
                })
                .collect();
            let top = ui.cursor().min;
            let width = ui.available_width();
            let _ = tab_bar(ui, cx, "t.tabs", &tabs, &mut selected);
            seen.set(egui::Rect::from_min_size(
                top,
                egui::vec2(width, cx.theme.metrics.nav_bar_height),
            ));
        }));
    h.shell.launch(LaunchAction::open("tabs"));
    h.run_for(0.5);
    Ok((h, bar))
}

/// Every text drawn on the next frame, with the rect it covers.
fn texts(h: &mut Harness) -> Vec<(String, egui::Rect, bool)> {
    fn walk(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect, bool)>) {
        match shape {
            egui::Shape::Text(text) => {
                let rect = egui::Rect::from_min_size(text.pos, text.galley.size());
                let cut = text
                    .galley
                    .rows
                    .iter()
                    .any(|row| row.glyphs.iter().any(|glyph| glyph.chr == '…'));
                out.push((text.galley.text().to_owned(), rect, cut));
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// `n` as an `f32` — the counts here are a handful of tabs.
fn f(n: usize) -> f32 {
    f32::from(u16::try_from(n).unwrap_or(u16::MAX))
}

/// The `index`-th of `count` equal cells across `bar`.
fn cell(bar: egui::Rect, index: usize, count: usize) -> egui::Rect {
    let w = bar.width() / f(count);
    egui::Rect::from_min_size(
        egui::pos2(w.mul_add(f(index), bar.min.x), bar.min.y),
        egui::vec2(w, bar.height()),
    )
}

const MIXED: &[&str] = &[
    "Home",
    "A tab label far too long for its cell",
    "Orders",
    "Another label that will not fit either",
];

/// A long label is cut with an ellipsis inside its cell, a gap short of each side so it does not
/// meet its neighbour's; a short one is drawn whole.
#[test]
fn a_long_label_is_cut_inside_its_cell() -> fairing::Result<()> {
    let (mut h, bar) = tab_screen(MIXED, 480.0)?;
    let bar = bar.get();
    let gap = h.shell.theme().control.gap;
    let drawn = texts(&mut h);
    for (index, label) in MIXED.iter().enumerate() {
        let (_, rect, cut) = need(
            drawn.iter().find(|(text, ..)| text == label),
            &format!("the label {label:?}"),
        )?;
        let cell = cell(bar, index, MIXED.len()).shrink2(egui::vec2(gap * 0.5, 0.0));
        assert!(
            rect.min.x >= cell.min.x - 0.5 && rect.max.x <= cell.max.x + 0.5,
            "{label:?} at {rect:?} runs out of its cell, less half a gap each side: {cell:?}"
        );
        let long = label.len() > 10;
        assert_eq!(*cut, long, "{label:?}: cut = {cut}");
    }
    Ok(())
}

/// Sixteen tabs on a narrow panel: cells narrower than the glyph. Everything the bar draws inside
/// a cell — glyph, label, the selection mark — stays inside it.
#[test]
fn a_cell_narrower_than_the_glyph_keeps_the_glyph_inside() -> fairing::Result<()> {
    const MANY: &[&str] = &[
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p",
    ];
    let (mut h, bar) = tab_screen(MANY, 360.0)?;
    let bar = bar.get();
    let count = MANY.len();
    let mut checked = 0;
    for clipped in h.frame_shapes() {
        let rect = clipped.shape.visual_bounding_rect();
        // The bar's own background and top rule span it all; the screen behind it is elsewhere.
        if !rect.is_positive() || !bar.contains_rect(rect) || rect.width() > bar.width() * 0.5 {
            continue;
        }
        let x = rect.center().x;
        let Some(home) = (0..count)
            .map(|index| cell(bar, index, count))
            .find(|home| home.x_range().contains(x))
        else {
            continue;
        };
        assert!(
            rect.min.x >= home.min.x - 0.5 && rect.max.x <= home.max.x + 0.5,
            "a shape at {rect:?} runs out of its cell {home:?}"
        );
        checked += 1;
    }
    assert!(checked >= count, "only {checked} shapes were checked");
    Ok(())
}
