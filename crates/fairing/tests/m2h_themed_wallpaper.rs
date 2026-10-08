//! **A picture wallpaper follows the palette too** — `Wallpaper::Themed`.
//!
//! `Solid` follows a theme change because a role does, and `ThemedPainter` follows because it is
//! handed the theme. A texture follows nothing: it is one image, so a shell that switches to its
//! light palette at dusk keeps the dark painting under a pale UI, and the labels that were drawn to
//! read against that painting stop reading.
//!
//! What is checked here is the switch itself — that the two sides of a pair really do reach the
//! screen as different drawings — and that a shell whose wallpaper is not a pair is untouched.

#![cfg(feature = "mock")]

use fairing::desktop::Wallpaper;
use fairing::testing::{single_level_access, Harness};
use fairing::theme::ColorRole;

/// The wallpaper's colour: the **tallest** full-width filled rect of the frame.
///
/// Not the first — the status bar is painted before the desktop — and not the widest, since the
/// bars are full width too. The wallpaper is the only one that covers the whole content band
/// between them. Nested shapes are walked, because a `Ui`'s output arrives inside a `Shape::Vec`
/// and the wallpaper is in there.
fn ground(h: &mut Harness) -> Option<egui::Color32> {
    fn walk(sh: &egui::Shape, full: f32, out: &mut Option<(f32, egui::Color32)>) {
        match sh {
            egui::Shape::Rect(r)
                if r.fill.a() > 0
                    && (r.rect.width() - full).abs() < 1.0
                    && out.is_none_or(|(hgt, _)| r.rect.height() > hgt) =>
            {
                *out = Some((r.rect.height(), r.fill));
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, full, out)),
            _ => {}
        }
    }
    let full = h.screen_rect().width();
    let mut best = None;
    for c in h.frame_shapes() {
        walk(&c.shape, full, &mut best);
    }
    best.map(|(_, fill)| fill)
}

/// A shell showing its desktop — the wallpaper is the desktop's ground, so without an icon on it
/// there is nothing to paint and the frame is just the status bar.
fn shell_with(wallpaper: Wallpaper) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = Harness::new(config, fairing::services::Services::null())?;
    h.shell.add(
        fairing::screen("one", |ui: &mut egui::Ui, _: &mut fairing::Cx<'_>| {
            ui.label("one");
        })
        .desktop(),
    );
    h.shell.desktop_mut().set_wallpaper(wallpaper);
    h.frames(2);
    Ok(h)
}

/// Two flat colours stand in for two paintings: what matters is that the *side* changes, and a
/// colour is something a test can read back without a texture upload.
#[test]
fn a_pair_draws_its_dark_side_dark_and_its_light_side_light() -> fairing::Result<()> {
    let (night, day) = (
        egui::Color32::from_rgb(0x04, 0x12, 0x1f),
        egui::Color32::from_rgb(0xfa, 0xf8, 0xf4),
    );
    let mut h = shell_with(Wallpaper::themed(
        Wallpaper::Fixed(night),
        Wallpaper::Fixed(day),
    ))?;

    h.shell.set_theme_dark(true);
    h.frames(2);
    assert_eq!(
        ground(&mut h),
        Some(night),
        "the dark palette has to get the dark side of the pair"
    );

    h.shell.set_theme_dark(false);
    h.frames(2);
    assert_eq!(
        ground(&mut h),
        Some(day),
        "and switching the palette has to switch the picture — this is the whole point of the \
         variant, and the defect it closes is a dark painting left under a pale UI"
    );
    Ok(())
}

/// A wallpaper that is not a pair keeps drawing what it always drew, in either palette.
#[test]
fn anything_that_is_not_a_pair_is_untouched() -> fairing::Result<()> {
    let fixed = egui::Color32::from_rgb(0x7f, 0x3a, 0x11);
    let mut h = shell_with(Wallpaper::Fixed(fixed))?;
    for dark in [true, false] {
        h.shell.set_theme_dark(dark);
        h.frames(2);
        assert_eq!(
            ground(&mut h),
            Some(fixed),
            "dark={dark}: a plain wallpaper must not start following the theme"
        );
    }
    Ok(())
}

/// The role wallpaper already followed the palette, and still has to — `Themed` is for pictures,
/// not a replacement for what roles do.
#[test]
fn a_role_wallpaper_still_follows_on_its_own() -> fairing::Result<()> {
    let mut h = shell_with(Wallpaper::Solid(ColorRole::Background))?;
    h.shell.set_theme_dark(true);
    h.frames(2);
    let night = ground(&mut h);
    h.shell.set_theme_dark(false);
    h.frames(2);
    assert_ne!(
        night,
        ground(&mut h),
        "a role wallpaper resolves through the palette, so the two themes cannot paint the same"
    );
    Ok(())
}
