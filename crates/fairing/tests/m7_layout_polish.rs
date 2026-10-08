//! **The page furniture at a one-finger row** — what was tightened once the rows shrank to one
//! touch target: the built-in settings screens in cards, a title that reserves a control's height
//! only when it carries a control, a section heading as tall as its text, an explanation that
//! wraps at a readable measure, and the two titles of the wide settings screen on one line.
#![cfg(all(feature = "settings", feature = "mock"))]

use fairing::layout;
use fairing::settings::{add_all, SettingsConfig};
use fairing::testing::{single_level_access, Harness};
use fairing::theme::ColorRole;
use fairing::widgets::BigButton;
use fairing::{screen, Cx, Shell};
use std::cell::Cell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// One screen drawn by `body`, opened and settled.
fn drawing(body: impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static) -> fairing::Result<Harness> {
    let mut body = body;
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen(
            "probe",
            move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                body(ui, cx);
            },
        ));
        shell.launch(fairing::LaunchAction::open("probe"));
        Ok(shell)
    })?;
    h.frames(4);
    Ok(h)
}

/// The settings screens on the Mock backends, with `id` open.
fn settings(id: &'static str) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .services(fairing::services::mock::services())
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        shell.launch(fairing::LaunchAction::open(id));
        Ok(shell)
    })?;
    h.frames(30);
    Ok(h)
}

/// How far the cursor moved over `f`, recorded each frame.
fn height_of(seen: &Rc<Cell<f32>>, ui: &mut egui::Ui, f: impl FnOnce(&mut egui::Ui)) {
    let before = ui.cursor().min.y;
    f(ui);
    seen.set(ui.cursor().min.y - before);
}

/// **A bare title is as tall as its words; a title with an action is a control tall.** It used to
/// reserve a control's height either way, which left a row-sized gap under every plain title.
#[test]
fn a_title_reserves_a_controls_height_only_for_an_action() -> fairing::Result<()> {
    let bare = Rc::new(Cell::new(0.0));
    let acted = Rc::new(Cell::new(0.0));
    let touch = Rc::new(Cell::new(0.0));
    let (b, a, t) = (Rc::clone(&bare), Rc::clone(&acted), Rc::clone(&touch));
    drawing(move |ui, cx| {
        t.set(cx.theme.metrics.touch_target);
        height_of(&b, ui, |ui| layout::title(ui, cx, "Plain"));
        height_of(&a, ui, |ui| {
            layout::header(ui, cx, "With a button", None, |ui, cx| {
                let _ = BigButton::new("Go").show(ui, &mut cx.widgets());
            });
        });
    })?;
    let (bare, acted, touch) = (bare.get(), acted.get(), touch.get());
    assert!(bare > 0.0, "the title was not drawn");
    assert!(
        acted - bare > touch * 0.2,
        "a bare title ({bare}) took nearly as much room as one with a button ({acted})"
    );
    Ok(())
}

/// **A section heading is its own text tall**, not a touch target: laid out in a horizontal row it
/// took egui's `interact_size` and floated as far from its card as from the one above.
#[test]
fn a_section_heading_is_its_text_tall() -> fairing::Result<()> {
    let seen = Rc::new(Cell::new(0.0));
    let touch = Rc::new(Cell::new(0.0));
    let (s, t) = (Rc::clone(&seen), Rc::clone(&touch));
    drawing(move |ui, cx| {
        t.set(cx.theme.metrics.touch_target);
        height_of(&s, ui, |ui| layout::section(ui, cx, "Networks"));
    })?;
    let (seen, touch) = (seen.get(), touch.get());
    assert!(seen > 0.0, "the heading was not drawn");
    assert!(
        seen < touch * 0.75,
        "the heading took {seen} du, a touch target is {touch}"
    );
    Ok(())
}

/// **An explanation wraps at about seventy characters**, however wide the pane.
#[test]
fn a_note_wraps_at_a_readable_measure() -> fairing::Result<()> {
    let width = Rc::new(Cell::new(0.0));
    let cap = Rc::new(Cell::new(0.0));
    let (w, c) = (Rc::clone(&width), Rc::clone(&cap));
    drawing(move |ui, cx| {
        let m = &cx.theme.metrics;
        c.set(m.type_scale.small * 36.0 + m.content_inset * 2.0 + 1.0);
        let r = ui.scope(|ui| {
            layout::note(ui, cx, &"A long explanation that keeps going. ".repeat(12));
        });
        w.set(r.response.rect.width());
    })?;
    let (width, cap) = (width.get(), cap.get());
    assert!(width > 0.0, "the note was not drawn");
    assert!(width <= cap, "the note ran {width} du wide, past {cap}");
    Ok(())
}

/// **The built-in settings screens group their rows in cards** — filled, rounded, in
/// `SurfaceVariant` — as the demo's own screens and the phones beside them do.
#[test]
fn the_settings_screens_are_cards() -> fairing::Result<()> {
    let mut h = settings("settings.display")?;
    let card = h.shell.theme().color(ColorRole::SurfaceVariant);
    let cards = h
        .frame_shapes()
        .into_iter()
        .filter(|c| match &c.shape {
            egui::Shape::Rect(r) => r.fill == card && r.corner_radius.nw > 0,
            _ => false,
        })
        .count();
    assert!(cards >= 2, "{cards} filled cards on the display screen");
    Ok(())
}

/// The heading-sized text drawn this frame, with where it was drawn.
fn headings(h: &mut Harness) -> Vec<(String, f32)> {
    let size = h.shell.theme().metrics.type_scale.heading;
    h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Text(t) => {
                let big = t
                    .galley
                    .job
                    .sections
                    .first()
                    .is_some_and(|s| (s.format.font_id.size - size).abs() < 0.01);
                big.then(|| (t.galley.text().to_owned(), t.pos.y))
            }
            _ => None,
        })
        .collect()
}

/// **The wide settings screen's two titles share a line.** The list's title sat under its page's
/// top gap and the right column's did not, so "Wi-Fi" stood higher than "Settings" beside it.
#[test]
fn the_two_settings_titles_share_a_line() -> fairing::Result<()> {
    let mut h = settings("settings.home")?;
    let found = headings(&mut h);
    let y_of = |text: &str| {
        found
            .iter()
            .find(|(t, _)| t == text)
            .map(|(_, y)| *y)
            .ok_or_else(|| fail(format!("no {text:?} heading in {found:?}")))
    };
    let (list, right) = (y_of("Settings")?, y_of("Wi-Fi")?);
    assert!(
        (list - right).abs() < 0.5,
        "the list's title is at {list}, the open screen's at {right}"
    );
    Ok(())
}
