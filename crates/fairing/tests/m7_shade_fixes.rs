//! Regression tests for the shade: its height following the layout, dismissals that outlive it,
//! and what its panel draws (fixes before 0.1.0).
#![cfg(feature = "overlay")]

use fairing::overlay::{tile, OverlayState, TileKind};
use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::{LaunchAction, Notification, NotificationId, Services};

fn missing(what: &str) -> fairing::Error {
    fairing::Error::Config(format!("could not find {what}"))
}

/// Every string drawn this frame.
fn drawn_text(h: &mut Harness) -> String {
    fn walk(shape: &egui::Shape, out: &mut String) {
        match shape {
            egui::Shape::Text(text) => {
                out.push_str(text.galley.text());
                out.push('\n');
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = String::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

fn settle_until(
    h: &mut Harness,
    max: usize,
    done: impl Fn(&Harness) -> bool,
) -> fairing::Result<()> {
    for _ in 0..=max {
        if done(h) {
            return Ok(());
        }
        h.frame();
    }
    Err(fairing::Error::Config(format!(
        "did not settle: {:?}",
        h.shell.overlay().state()
    )))
}

fn open_shade(h: &mut Harness) -> fairing::Result<()> {
    h.shell.launch(LaunchAction::OpenOverlay);
    settle_until(h, 120, |h| h.shell.overlay().is_open())
}

/// Press the × on the only notification, then close the shade at once; the dismissal still
/// lands.
fn dismiss_then_close(mut h: Harness) -> fairing::Result<()> {
    h.frames(2);
    h.shell
        .notify(Notification::new(NotificationId::of("gone"), "Dismiss me"));
    open_shade(&mut h)?;
    h.frames(2);
    let hit = h
        .shell
        .overlay()
        .close_hit_rect()
        .ok_or_else(|| missing("close hit"))?;
    h.press(hit.center());
    h.frame();
    h.release(hit.center());
    h.frame();
    h.shell.handle().toggle_overlay();
    h.frames(180);
    assert!(h.shell.overlay().is_closed(), "precondition: closed");
    assert_eq!(
        h.shell.notifications().len(),
        0,
        "the × was pressed, but the notification is still there after the shade closed"
    );
    Ok(())
}

/// A curtain shade closed while a dismissal flies out still removes the notification.
#[test]
fn dismissal_survives_closing_the_shade_mid_animation() -> fairing::Result<()> {
    dismiss_then_close(Harness::new(single_level_access(), Services::null())?)
}

/// A card shade, whose way out is a short tween, closed right after the × keeps the dismissal.
#[test]
fn dismissal_survives_closing_a_card_shade_mid_animation() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.overlay.reveal = "card".to_owned();
    dismiss_then_close(Harness::new(cfg, Services::null())?)
}

/// A shade still settling open when the screen gets shorter lands open at the new height.
#[test]
fn shade_settling_open_follows_a_height_change() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    assert!(
        matches!(
            h.shell.overlay().state(),
            OverlayState::Settling { opening: true }
        ),
        "precondition: settling open, got {:?}",
        h.shell.overlay().state()
    );
    let before = h.shell.overlay().height();
    h.set_size(1024.0, 400.0);
    settle_until(&mut h, 200, |h| h.shell.overlay().is_open())?;
    h.frames(5);
    let (y, hh) = (h.shell.overlay().y(), h.shell.overlay().height());
    assert!(
        hh < before - 1.0,
        "precondition: H shrank ({before} -> {hh})"
    );
    assert!(
        (y - hh).abs() < 1.0,
        "Open with y = {y} but H = {hh} (was {before})"
    );
    Ok(())
}

/// A fully open two-step shade stays fully open when the screen grows.
#[test]
fn two_step_shade_fully_open_follows_a_taller_screen() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.overlay.two_step = true;
    let mut h = Harness::new(cfg, Services::null())?.with_size(1024.0, 400.0);
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(5);
    let (y0, h0) = (h.shell.overlay().y(), h.shell.overlay().height());
    assert!(
        (y0 - h0).abs() < 1.0,
        "precondition: fully open ({y0} of {h0})"
    );
    h.set_size(1024.0, 600.0);
    h.frames(30);
    let (y, hh) = (h.shell.overlay().y(), h.shell.overlay().height());
    assert!(hh > h0 + 1.0, "precondition: H grew ({h0} -> {hh})");
    assert!(
        h.shell.overlay().is_open() && (y - hh).abs() < 1.0,
        "fully open shade left at y = {y} of H = {hh} (state {:?})",
        h.shell.overlay().state()
    );
    Ok(())
}

/// A Gauges tile with no rows does not open an expansion row when tapped.
#[test]
fn empty_gauges_tile_does_not_open() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |shell| {
        shell.add(
            tile(
                "tile.empty",
                TileKind::Gauges {
                    rows: vec![],
                    label_width: 0.0,
                },
            )
            .label("Empty"),
        );
    })?;
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    let r = h
        .shell
        .overlay()
        .tile_rect("tile.empty")
        .ok_or_else(|| missing("tile.empty"))?;
    h.tap(r.center());
    h.frames(2);
    assert_eq!(
        h.shell.overlay().expanded_tile(),
        None,
        "an empty Gauges tile opened its row (expanded_rect = {:?})",
        h.shell.overlay().expanded_rect()
    );
    Ok(())
}

/// "Clear all" is not drawn when every listed notification is persistent.
#[test]
fn clear_all_not_drawn_when_only_persistent() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    h.shell
        .notify(Notification::new(NotificationId::of("p"), "Pinned").persistent());
    open_shade(&mut h)?;
    h.frames(2);
    let text = drawn_text(&mut h);
    assert!(
        text.contains("Pinned"),
        "precondition: the list is drawn:\n{text}"
    );
    assert!(
        !text.contains("Clear all"),
        "\"Clear all\" is drawn with nothing clearable:\n{text}"
    );
    h.shell
        .notify(Notification::new(NotificationId::of("q"), "Passing"));
    h.frames(2);
    let text = drawn_text(&mut h);
    assert!(
        text.contains("Clear all"),
        "with something clearable it is back:\n{text}"
    );
    Ok(())
}
