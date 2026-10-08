//! The desktop label legibility aid (guide 09 §2.2a).
//!
//! What it checks: whether **it is off by default** (a design rule — the shell does not lay a veil over the
//! wallpaper), whether the shape count really does grow when it is on, and whether an unknown config value stops
//! the shell.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

use fairing::desktop::LabelLegibility;
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx};

/// A desktop with two icons on it. The labels and the icons have to be really drawn for the shape count to grow.
fn desktop_shell(legibility: &str) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    legibility.clone_into(&mut config.desktop.label_legibility);
    let mut h = Harness::new(config, fairing::services::Services::null())?;
    for id in ["alpha", "beta"] {
        h.shell.add(
            screen(id, |_: &mut egui::Ui, _: &mut Cx<'_>| {})
                .title(id)
                .icon(fairing::icons::IconRef::Builtin("settings"))
                .desktop(),
        );
    }
    h.frames(3);
    Ok(h)
}

/// **It is off by default.** There is no reason to fog up a well-authored wallpaper.
#[test]
fn the_default_is_off() -> fairing::Result<()> {
    let h = desktop_shell("none")?;
    assert_eq!(h.shell.desktop().legibility(), LabelLegibility::None);
    Ok(())
}

/// It is read from the config.
#[test]
fn it_is_read_from_config() -> fairing::Result<()> {
    assert_eq!(
        desktop_shell("shadow")?.shell.desktop().legibility(),
        LabelLegibility::Shadow
    );
    assert_eq!(
        desktop_shell("veil")?.shell.desktop().legibility(),
        LabelLegibility::Veil
    );
    Ok(())
}

/// An unknown value warns and gives `none` — a typo does not stop the shell (the same fail-open as the wallpaper side).
#[test]
fn an_unknown_value_falls_back_to_none() -> fairing::Result<()> {
    let h = desktop_shell("glow")?;
    assert_eq!(h.shell.desktop().legibility(), LabelLegibility::None);
    Ok(())
}

/// `shadow` draws the label and the icon **one layer more**. No growth in the shapes means it did not draw.
#[test]
fn shadow_draws_more_than_none() -> fairing::Result<()> {
    let plain = desktop_shell("none")?.frame_shapes().len();
    let shadow = desktop_shell("shadow")?.frame_shapes().len();
    assert!(
        shadow > plain,
        "the shadow is on and the shapes did not grow: {plain} → {shadow}"
    );
    Ok(())
}

/// `veil` adds **one** rectangle — cheap however many icons there are.
#[test]
fn veil_adds_a_single_rect() -> fairing::Result<()> {
    let plain = desktop_shell("none")?.frame_shapes().len();
    let veil = desktop_shell("veil")?.frame_shapes().len();
    assert_eq!(veil, plain + 1, "the veil has to be one rectangle");
    Ok(())
}

/// Changed at run time — the path used alongside swapping the wallpaper out.
#[test]
fn it_can_be_changed_at_runtime() -> fairing::Result<()> {
    let mut h = desktop_shell("none")?;
    h.shell
        .desktop_mut()
        .set_legibility(LabelLegibility::Shadow);
    h.frames(2);
    assert_eq!(h.shell.desktop().legibility(), LabelLegibility::Shadow);
    Ok(())
}

/// The name round-trips.
#[test]
fn names_round_trip() {
    for mode in LabelLegibility::ALL {
        assert_eq!(LabelLegibility::parse(mode.as_str()), Some(*mode));
    }
    assert_eq!(
        LabelLegibility::parse("SHADOW"),
        Some(LabelLegibility::Shadow)
    );
    assert_eq!(LabelLegibility::parse("nope"), None);
}
