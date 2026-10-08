//! **The keyboard's rows have a physical cap**. The height was only a share of the
//! screen (`[osk] height_ratio`, 0.38) with a floor per row, so on a tall kiosk panel each row of
//! keys grew to the size of a palm. Now no row is taller than `metrics.osk_max_key`, and the floor
//! (`[osk] min_key_px`) still wins where the two cross.
#![cfg(feature = "osk")]

use fairing::testing::{single_level_access, Harness};
use fairing::theme::MetricsSpec;
use fairing::unit::{Dim, ScalePolicy, Span};
use fairing::{screen, LaunchAction, Shell, ShellConfig, Theme};

/// A shell of `width` × `height` whose one screen focuses a text field, so the keyboard comes up.
fn keyboard_up(
    width: f32,
    height: f32,
    build: impl FnOnce(&egui::Context) -> fairing::Result<Shell>,
) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(build)?.with_size(width, height);
    let mut text = String::new();
    let mut focused = false;
    h.shell.add(screen(
        "form",
        move |ui: &mut egui::Ui, _cx: &mut fairing::Cx<'_>| {
            let field = ui.add(egui::TextEdit::singleline(&mut text));
            if !focused {
                field.request_focus();
                focused = true;
            }
        },
    ));
    h.shell.launch(LaunchAction::open("form"));
    h.run_for(1.0);
    assert!(h.shell.osk().is_shown(), "the keyboard did not come up");
    Ok(h)
}

/// The rows on the default qwerty's letter face: three of letters and the bottom row.
const ROWS: f32 = 4.0;

/// A portrait kiosk, 1080 × 2560 px on 380 × 900 mm: the share alone would give a keyboard of
/// 973 px. The rows stop at the cap instead.
#[test]
fn a_tall_panel_stops_the_rows_at_the_cap() -> fairing::Result<()> {
    let h = keyboard_up(1080.0, 2560.0, |ctx| {
        Shell::builder(single_level_access())
            .physical_mm(380.0, 900.0)
            .build(ctx)
    })?;
    let cap = h.shell.theme().metrics.osk_max_key;
    let height = h.shell.osk().height();
    assert!(
        height < 0.38 * 2560.0 - 1.0,
        "the share was not cut: {height}"
    );
    assert!(
        (height - ROWS * cap).abs() < 0.01,
        "the keyboard is {height}, not {ROWS} rows of {cap} du"
    );
    Ok(())
}

/// The cap is one and a half fingers: on a panel dense enough that its du floor does not hold it
/// up, it follows `ScalePolicy::finger_mm`, and a tall keyboard stops there. 5 px/mm keeps
/// `pixels_per_point` at 1, so the test does not pay for a font atlas at a higher scale.
#[test]
fn the_cap_is_one_and_a_half_fingers() -> fairing::Result<()> {
    let h = keyboard_up(600.0, 1600.0, |ctx| {
        Shell::builder(single_level_access())
            .physical_mm(120.0, 320.0)
            .build(ctx)
    })?;
    let scale = h.shell.scale();
    let finger = scale.finger_mm * scale.du_per_mm;
    let cap = h.shell.theme().metrics.osk_max_key;
    assert!(
        (cap - 1.5 * finger).abs() < 0.5,
        "the cap is {cap} du, one and a half fingers is {}",
        1.5 * finger
    );
    assert!(cap > 72.5, "the du floor holds the cap up: {cap}");
    let height = h.shell.osk().height();
    assert!(
        (height - ROWS * cap).abs() < 0.01,
        "the keyboard is {height}, not {ROWS} rows of {cap} du"
    );
    Ok(())
}

/// The cap never falls under 72 du: with a bare finger on the same panel, one and a half fingers
/// is less than that, and the rows may still be 72 du tall.
#[test]
fn the_cap_keeps_its_du_floor() -> fairing::Result<()> {
    let h = keyboard_up(1000.0, 600.0, |ctx| {
        Shell::builder(single_level_access())
            .scale_policy(ScalePolicy::bare())
            .physical_mm(200.0, 120.0)
            .build(ctx)
    })?;
    let scale = h.shell.scale();
    let fingers = 1.5 * scale.finger_mm * scale.du_per_mm;
    assert!(fingers < 72.0, "not the floor's case: {fingers} du");
    let cap = h.shell.theme().metrics.osk_max_key;
    assert!(
        (cap - 72.0).abs() < 0.01,
        "the cap is {cap} du, under its 72 du floor"
    );
    Ok(())
}

/// The floor wins where the two cross: rows pinned at 60 du at most, but `min_key_px = 120`.
#[test]
fn the_floor_wins_over_the_cap() -> fairing::Result<()> {
    let h = keyboard_up(1024.0, 600.0, |ctx| {
        let mut config: ShellConfig = single_level_access();
        config.osk.min_key_px = 120.0;
        let spec = MetricsSpec {
            osk_max_key: Some(Span::fixed(Dim::du(60.0)).pinned()),
            ..MetricsSpec::default()
        };
        Shell::builder(config).metrics_spec(spec).build(ctx)
    })?;
    let height = h.shell.osk().height();
    assert!(
        (height - ROWS * 120.0).abs() < 0.01,
        "the floor did not win: {height}, not {ROWS} rows of 120 du"
    );
    Ok(())
}

/// **The cap can be lifted**: `None` in the spec, and the share alone sets the height again — on
/// the same tall panel that stops at the cap above.
#[test]
fn the_cap_can_be_lifted() -> fairing::Result<()> {
    let h = keyboard_up(1080.0, 2560.0, |ctx| {
        let spec = MetricsSpec {
            osk_max_key: None,
            ..MetricsSpec::default()
        };
        Shell::builder(single_level_access())
            .physical_mm(380.0, 900.0)
            .metrics_spec(spec)
            .build(ctx)
    })?;
    assert!(h.shell.theme().metrics.osk_max_key.is_infinite());
    let share = 0.38 * h.screen_rect().height();
    let height = h.shell.osk().height();
    assert!(
        (height - share).abs() < 0.5,
        "lifted, the keyboard is {height}, not the share {share}"
    );
    Ok(())
}

/// **A theme lifts it with `f32::INFINITY`** — and it stays lifted when a run-time spec is
/// seeded from that theme's metrics (`Shell::metrics_spec_mut`), where a pinned infinite length
/// would have folded to 0 and squashed the keyboard to its floor.
#[test]
fn a_theme_lifts_the_cap_with_infinity() -> fairing::Result<()> {
    let mut h = keyboard_up(1080.0, 2560.0, |ctx| {
        let mut theme = Theme::default();
        theme.metrics.osk_max_key = f32::INFINITY;
        Shell::builder(single_level_access())
            .physical_mm(380.0, 900.0)
            .theme(theme)
            .build(ctx)
    })?;
    let _ = h.shell.metrics_spec_mut();
    h.frames(2);
    assert!(h.shell.theme().metrics.osk_max_key.is_infinite());
    let share = 0.38 * h.screen_rect().height();
    let height = h.shell.osk().height();
    assert!(
        (height - share).abs() < 0.5,
        "lifted, the keyboard is {height}, not the share {share}"
    );
    Ok(())
}

/// On the reference 1024 × 600 panel the cap is far above the share, and nothing changes.
#[test]
fn the_reference_panel_keeps_its_share() -> fairing::Result<()> {
    let h = keyboard_up(1024.0, 600.0, |ctx| {
        Shell::builder(single_level_access()).build(ctx)
    })?;
    let height = h.shell.osk().height();
    assert!(
        (height - 0.38 * 600.0).abs() < 0.01,
        "the reference keyboard changed: {height}"
    );
    Ok(())
}
