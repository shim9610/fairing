//! **The integrator's three routes into the built-in screens** (guide 09 §8).
//!
//! Each of these is a thing the module docs promise. They are here because two of them did not
//! compile: the screen bodies were private, and a declaration's id was fixed at construction with
//! only a getter - so "clone a built-in as a screen of my own" had no route but to write the body
//! again against source the crate does not expose.

#![cfg(feature = "settings")]

use fairing::screen;
use fairing::settings::screens::{self, SettingsEntry};
use fairing::testing::{single_level_access, Harness};
use fairing::{Cx, IconRef, LaunchAction};

/// Every string the frame drew.
fn drawn(h: &mut Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for s in h.frame_shapes() {
        walk(&s.shape, &mut out);
    }
    out
}

/// **The settings list is a `Vec` the integrator owns.** Rows out, rows of their own in, in
/// whatever order - and `home_with` takes it whole.
#[test]
fn the_settings_menu_is_a_list_you_edit() {
    let mut entries = screens::entries();
    let before = entries.len();
    entries.retain(|e| e.id != "settings.bluetooth");
    entries.insert(
        0,
        SettingsEntry::new("app.heater", IconRef::Builtin("thermometer"), "Heater"),
    );
    assert_eq!(entries.len(), before);
    assert_eq!(entries.first().map(|e| e.id.as_str()), Some("app.heater"));
    let decl = screens::home_with(entries);
    assert_eq!(decl.id(), "settings.home");
}

/// **A built-in declaration can be registered under an id of your own**, carrying its body, title,
/// icon, gate and chrome with it.
#[test]
fn a_builtin_screen_can_be_cloned_under_a_new_id() {
    let original = screens::display();
    assert_eq!(original.id(), "settings.display");

    let mine = screens::display().with_id("app.display").title("Panel");
    assert_eq!(mine.id(), "app.display");
    assert_eq!(mine.label(), "Panel");
}

/// **A built-in body is a starting point**, not a wall: it drops into a screen of your own, with
/// rows of your own around it.
#[test]
fn a_builtin_body_can_be_reused_inside_your_own_screen() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = Harness::new(config, fairing::services::Services::null())?.with_size(800.0, 600.0);
    h.shell.add(
        screen("app.display", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            fairing::layout::page(ui, cx, "app.display", |ui, cx| {
                fairing::layout::section(ui, cx, "Ours");
                let _ = fairing::layout::info_row(ui, cx, "Panel", "7 inch");
                screens::display_body(ui, cx);
            });
        })
        .title("Panel"),
    );
    h.shell.handle().launch(LaunchAction::open("app.display"));
    h.frames(3);
    // It drew: the built-in body puts a "Brightness" row in, and ours puts "Panel" above it.
    let texts = drawn(&mut h);
    assert!(texts.iter().any(|t| t == "Panel"), "our row is missing");
    assert!(
        texts.iter().any(|t| t.contains("Brightness")),
        "the built-in body drew nothing: {texts:?}"
    );
    Ok(())
}

/// **The settings layout, declared for a screen of your own.** Not the settings module, not its
/// menu - the shape: a scrolling column of a title, sections and cards of rows.
///
/// Written against the pieces alone this is two nested closures with the screen id in both, and a
/// `page` that is easy to leave out - which costs nothing until that one screen is the only one
/// cut off at the bottom. `page_screen` takes the body and `section_card` takes the pair that 25
/// of the crate's 32 `section` calls already are.
#[test]
fn the_settings_layout_declares_for_any_screen() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    // Tall enough to hold the whole page: `egui::Label` culls itself outside the viewport, so a
    // heading scrolled past the bottom edge would be missing for a reason that is not this test's.
    let mut h = Harness::new(config, fairing::services::Services::null())?.with_size(800.0, 1100.0);

    let mut purge = false;
    h.shell.add(
        fairing::layout::page_screen("app.heater", move |ui, cx| {
            fairing::layout::title(ui, cx, "Heater");
            fairing::layout::section_card(ui, cx, "Hopper", |ui, cx| {
                let _ = fairing::layout::info_row(ui, cx, "Resin A", "72 %");
                let _ = fairing::layout::info_row(ui, cx, "Resin B", "40 %");
            });
            fairing::layout::section_card(ui, cx, "Maintenance", |ui, cx| {
                let _ =
                    fairing::layout::switch_row(ui, cx, "Purge on stop", None, &mut purge, true);
            });
            fairing::layout::note(ui, cx, "Purging takes about a minute.");
        })
        .title("Heater")
        .icon(IconRef::Builtin("thermometer"))
        .desktop(),
    );
    h.shell.handle().launch(LaunchAction::open("app.heater"));
    h.frames(3);

    let texts = drawn(&mut h);
    for want in [
        "Heater",
        "Hopper",
        "Resin A",
        "72 %",
        "Maintenance",
        "Purge on stop",
        "Purging takes about a minute.",
    ] {
        assert!(
            texts.iter().any(|t| t == want),
            "{want:?} is missing from {texts:?}"
        );
    }
    Ok(())
}
