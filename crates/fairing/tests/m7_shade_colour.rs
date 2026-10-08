//! **The shade's own colour** — `ColorRole::ShadeSurface`, set as `shade_surface` under
//! `[theme.palette]`. Unset, the shade is drawn in the screens' surface as before; set, the shade
//! alone changes and every other surface stays.
#![cfg(feature = "overlay")]

use fairing::testing::{single_level_access, Harness};
use fairing::theme::ColorRole;
use fairing::{LaunchAction, Shell, ShellConfig};

const SHADE: egui::Color32 = egui::Color32::from_rgb(0x12, 0x34, 0x56);

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// A shell with the shade pulled fully open.
fn open_shade(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .services(fairing::services::mock::services())
            .build(ctx)
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::OpenOverlay);
    for _ in 0..90 {
        if h.shell.overlay().is_open() {
            break;
        }
        h.frame();
    }
    h.frames(30);
    if h.shell.overlay().is_open() {
        Ok(h)
    } else {
        Err(fail("the shade did not open"))
    }
}

/// Every rect fill drawn this frame.
fn rect_fills(h: &mut Harness) -> Vec<egui::Color32> {
    fn walk(shape: egui::Shape, out: &mut Vec<egui::Color32>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.into_iter().for_each(|s| walk(s, out)),
            egui::Shape::Rect(r) => out.push(r.fill),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        walk(clipped.shape, &mut out);
    }
    out
}

fn with_shade_colour(reveal: &str) -> ShellConfig {
    let mut config = single_level_access();
    config.motion.reduce = true;
    reveal.clone_into(&mut config.overlay.reveal);
    config
        .theme
        .palette
        .insert("shade_surface".to_owned(), "#123456".to_owned());
    config
}

/// Unset, the shade follows the surface: a palette that does not name the role looks as before.
#[test]
fn an_unset_shade_colour_follows_the_surface() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = open_shade(config)?;
    let theme = h.shell.theme();
    assert_eq!(
        theme.color(ColorRole::ShadeSurface),
        theme.color(ColorRole::Surface)
    );
    let surface = theme.color(ColorRole::Surface);
    assert!(
        rect_fills(&mut h).contains(&surface),
        "no plate in the surface"
    );
    Ok(())
}

/// Set, the curtain is drawn in it and the screens' surface is left alone.
#[test]
fn a_curtain_shade_takes_its_own_colour() -> fairing::Result<()> {
    let mut h = open_shade(with_shade_colour("curtain"))?;
    assert_ne!(h.shell.theme().color(ColorRole::Surface), SHADE);
    assert!(
        rect_fills(&mut h).contains(&SHADE),
        "the curtain is not drawn in the shade colour"
    );
    Ok(())
}

/// The floating card takes it too.
#[test]
fn a_card_shade_takes_its_own_colour() -> fairing::Result<()> {
    let mut h = open_shade(with_shade_colour("card"))?;
    assert!(
        rect_fills(&mut h).contains(&SHADE),
        "the card is not drawn in the shade colour"
    );
    Ok(())
}

/// The light and dark palettes both carry it, so switching the theme keeps the shade's colour.
#[test]
fn the_shade_colour_survives_a_theme_switch() -> fairing::Result<()> {
    let mut h = open_shade(with_shade_colour("curtain"))?;
    let dark = h.shell.theme().dark;
    h.shell.set_theme_dark(!dark);
    h.frames(30);
    assert_eq!(h.shell.theme().color(ColorRole::ShadeSurface), SHADE);
    Ok(())
}
