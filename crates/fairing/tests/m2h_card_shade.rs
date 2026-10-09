//! **The shade as a card** — `[overlay] reveal = "card"`.
//!
//! A curtain hangs from the top edge and is drawn down over the page: nothing in it moves but
//! the edge, and the eye reads "a cloth lifted off something that was already there". A card is
//! already its final size at its final place, inset from the sides with four round corners, and
//! what the pull drives is how far it has *arrived*: it fades in, comes down a short way and its
//! edge sharpens from a soft blob into a corner. It is gone in a fraction of the time it took to
//! come, and on a wide screen it rests on the side it was pulled from.
//!
//! These read the plate the overlay draws under its content — the one rect shape on the panel's
//! own Rect with no feather — rather than matching colours.

#![cfg(feature = "overlay")]

use fairing::overlay::OverlayReveal;
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, Shell};

/// A shell whose shade is a curtain or a card, one-step or two, on a screen of `size`.
fn shell(reveal: &str, two_step: bool, size: (f32, f32)) -> fairing::Result<Harness> {
    shell_with(reveal, two_step, size, |_| {})
}

/// [`shell`], with the config tuned further.
fn shell_with(
    reveal: &str,
    two_step: bool,
    size: (f32, f32),
    tune: impl FnOnce(&mut fairing::ShellConfig) + 'static,
) -> fairing::Result<Harness> {
    let reveal = reveal.to_owned();
    Harness::from_builder(move |ctx| {
        let mut config = single_level_access();
        config.overlay.reveal = reveal;
        config.overlay.two_step = two_step;
        tune(&mut config);
        let mut shell = Shell::builder(config).build(ctx)?;
        shell.add(screen("s", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            fairing::layout::page(ui, cx, "s", |ui, cx| {
                fairing::layout::note(ui, cx, "body");
            });
        }));
        shell.launch(fairing::LaunchAction::open("s"));
        Ok(shell)
    })
    .map(|h| {
        let mut h = h.with_size(size.0, size.1);
        h.frames(3);
        h
    })
}

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("{what} is missing")))
}

/// Pull the shade from the top edge at `x` to `to_y`, hold still, let go, and settle. Returns
/// where it came to rest.
fn pull_to(h: &mut Harness, x: f32, to_y: f32) -> f32 {
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    let steps = 10;
    for i in 1..=steps {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / steps as f32;
        h.move_to(egui::pos2(x, 2.0 + (to_y - 2.0) * t));
        h.frames(1);
    }
    // Held still before letting go — a release carrying speed is a fling, which is a different rule.
    for _ in 0..6 {
        h.move_to(egui::pos2(x, to_y));
        h.frames(1);
    }
    h.release(egui::pos2(x, to_y));
    h.frames(40);
    h.shell.overlay().y()
}

/// Two Rects within a pixel of each other.
fn same(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min.x - b.min.x).abs() < 1.0
        && (a.max.x - b.max.x).abs() < 1.0
        && (a.min.y - b.min.y).abs() < 1.0
        && (a.max.y - b.max.y).abs() < 1.0
}

/// One frame's plate and the panel Rect it was drawn on: the rect shape on the panel's own Rect
/// with no feather and no texture, plus whether a feathered (soft) copy and a frosted backdrop
/// (a texture-filled copy) were drawn under it.
struct Drawn {
    panel: egui::Rect,
    plate: egui::epaint::RectShape,
    soft: bool,
    frost: bool,
}

fn drawn(h: &mut Harness) -> Option<Drawn> {
    let shapes = h.frame_shapes();
    let panel = h.shell.overlay().frame().panel?;
    let rects = shapes.into_iter().filter_map(|c| match c.shape {
        egui::Shape::Rect(r) if r.fill.a() > 0 && same(r.rect, panel) => Some(r),
        _ => None,
    });
    let mut plate = None;
    let mut soft = false;
    let mut frost = false;
    for r in rects {
        if r.blur_width > 0.0 {
            soft = true;
        } else if r.brush.is_some() {
            frost = true;
        } else if plate.is_none() {
            plate = Some(r);
        }
    }
    Some(Drawn {
        panel,
        plate: plate?,
        soft,
        frost,
    })
}

/// What one frame draws of a card's relief and rim: the brightest white along the
/// panel's top edge in a mesh over the panel (`None` with no such mesh), whether that mesh is also
/// black along the bottom edge, and whether a stroked, unfilled rect lies on the panel's Rect —
/// the floating rim.
struct Raised {
    light: Option<u8>,
    shade: bool,
    rim: bool,
}

fn raised(h: &mut Harness) -> fairing::Result<Raised> {
    let shapes = h.frame_shapes();
    let panel = need(h.shell.overlay().frame().panel, "the panel")?;
    let mut out = Raised {
        light: None,
        shade: false,
        rim: false,
    };
    for c in shapes {
        match c.shape {
            egui::Shape::Mesh(m)
                if m.texture_id == egui::TextureId::default()
                    && !m.vertices.is_empty()
                    && m.vertices.iter().all(|v| panel.expand(1.0).contains(v.pos)) =>
            {
                let light = m
                    .vertices
                    .iter()
                    .filter(|v| v.pos.y - panel.min.y < 4.0 && v.color.r() == v.color.a())
                    .map(|v| v.color.a())
                    .max()
                    .filter(|&a| a > 0);
                if light.is_some() {
                    out.light = light;
                    out.shade = m.vertices.iter().any(|v| {
                        panel.max.y - v.pos.y < 4.0 && v.color.a() > 0 && v.color.r() == 0
                    });
                }
            }
            egui::Shape::Rect(r)
                if r.stroke.width > 0.0 && r.fill.a() == 0 && same(r.rect, panel) =>
            {
                out.rim = true;
            }
            _ => {}
        }
    }
    Ok(out)
}

/// A point on the page well away from the card: the other side, half a touch target in from
/// the glass's edge, at the content's middle height — off the bars and the edge zones.
fn beside(h: &Harness, panel: egui::Rect) -> egui::Pos2 {
    let screen = h.screen_rect();
    let in_from_edge = h.shell.theme().metrics.touch_target * 0.5;
    let x = if panel.center().x < screen.center().x {
        screen.max.x - in_from_edge
    } else {
        screen.min.x + in_from_edge
    };
    egui::pos2(x, h.shell.layout().content.center().y)
}

/// A tap well away from any card.
fn tap_beside(h: &mut Harness, panel: egui::Rect) {
    let at = beside(h, panel);
    h.tap(at);
    h.frames(40);
}

/// **At rest a card is inset from both sides, under the status bar, with four round corners** —
/// the width share and the corner measured off One UI 8 (0.445 of the width; a radius of 9 % of
/// the card's width, four `corner_radius` here). On a screen wide enough that the share holds the
/// tile row with room to spare, so the share is what decides the width.
#[test]
fn a_card_rests_inset_with_four_round_corners() -> fairing::Result<()> {
    let mut h = shell("card", false, (1600.0, 900.0))?;
    assert_eq!(h.shell.overlay().reveal(), OverlayReveal::Card);
    let screen = h.screen_rect();
    pull_to(&mut h, screen.width() * 0.5, screen.height() * 0.8);
    assert!(h.shell.overlay().is_open(), "the pull has to open it");
    let d = need(drawn(&mut h), "the plate")?;
    let panel = d.panel;
    assert!(
        panel.min.x > screen.min.x + 4.0 && panel.max.x < screen.max.x - 4.0,
        "a card is inset from both sides: {panel:?} on {screen:?}"
    );
    assert!(
        (panel.width() - screen.width() * 0.46).abs() < 2.0,
        "the width is the configured share of the screen: {} of {}",
        panel.width(),
        screen.width()
    );
    let metrics = h.shell.theme().metrics;
    assert!(
        panel.min.y > metrics.status_bar_height + 1.0,
        "it hangs under the status bar with a gap: top {} against a bar of {}",
        panel.min.y,
        metrics.status_bar_height
    );
    let cr = d.plate.corner_radius;
    assert!(
        cr.nw > 0 && cr.nw == cr.ne && cr.nw == cr.sw && cr.nw == cr.se,
        "all four corners are round and the same: {cr:?}"
    );
    assert!(
        (f32::from(cr.nw) - metrics.corner_radius * 4.0).abs() < 1.0,
        "the corner is four corner_radius ({}): {}",
        metrics.corner_radius * 4.0,
        cr.nw
    );
    assert_eq!(d.plate.fill.a(), 255, "at rest the plate is solid");
    assert!(!d.soft, "at rest there is no soft edge under it");
    Ok(())
}

/// **It arrives from above, faint and soft, already its full width.** Mid-pull the plate is
/// translucent with a feathered copy under it, and it sits above where it will rest.
#[test]
fn a_card_arrives_from_above_fading_in_with_a_soft_edge() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    let x = screen.width() * 0.5;
    // Learn where it rests, then put it away.
    pull_to(&mut h, x, screen.height() * 0.8);
    let rest = need(drawn(&mut h), "the plate at rest")?.panel;
    tap_beside(&mut h, rest);
    assert!(
        h.shell.overlay().is_closed(),
        "a tap beside the card closes it"
    );
    // A short pull, held: to a sixth of the card's height, short of the snap at a third.
    let short = rest.height() / 6.0;
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    for i in 1..=6 {
        #[expect(clippy::cast_precision_loss, reason = "six steps")]
        let dy = short * i as f32 / 6.0;
        h.move_to(egui::pos2(x, 2.0 + dy));
        h.frames(1);
    }
    let early = need(drawn(&mut h), "the plate mid-pull")?;
    assert!(
        early.plate.fill.a() > 0 && early.plate.fill.a() < 200,
        "mid-pull the plate is translucent: alpha {}",
        early.plate.fill.a()
    );
    assert!(early.soft, "mid-pull a soft, feathered copy is under it");
    assert!(
        early.panel.min.y < rest.min.y - 2.0,
        "it is still above its rest: top {} against {}",
        early.panel.min.y,
        rest.min.y
    );
    assert!(
        (early.panel.min.x - rest.min.x).abs() < 1.0
            && (early.panel.width() - rest.width()).abs() < 1.0,
        "it is already its full width at its place: {:?} against {rest:?}",
        early.panel
    );
    // Held still before the release, so what is let go is a distance and not a speed.
    for _ in 0..4 {
        h.move_to(egui::pos2(x, 2.0 + short));
        h.frames(1);
    }
    h.release(egui::pos2(x, 2.0 + short));
    h.frames(40);
    assert!(
        h.shell.overlay().is_closed(),
        "a short pull let go falls back shut"
    );
    Ok(())
}

/// **A flick still shows the arrival.** Tied 1:1 to the finger, a hand that covers the whole stop
/// in two frames left nothing to see — the first real capture had the card fully there one frame
/// after it was faintly there. The card trails the pull by a tenth of a second on the way in, so
/// just after the flick it is still faint and above its rest, and held, it lands exactly.
#[test]
fn a_flick_still_shows_the_card_arriving() -> fairing::Result<()> {
    let mut h = shell("card", true, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    let x = screen.width() * 0.5;
    // Learn where it rests at the stop, then put it away with a press on the page.
    pull_to(&mut h, x, screen.height() * 0.3);
    let rest = need(drawn(&mut h), "the plate at the stop")?.panel;
    tap_beside(&mut h, rest);
    assert!(
        h.shell.overlay().is_closed(),
        "a press on the page closes it"
    );
    // Two big steps, well past the stop, and held.
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    h.move_to(egui::pos2(x, screen.height() * 0.2));
    h.frames(1);
    h.move_to(egui::pos2(x, screen.height() * 0.4));
    h.frames(1);
    let early = need(drawn(&mut h), "the plate just after the flick")?;
    assert!(
        early.plate.fill.a() < 230,
        "just after a flick the card is still arriving: alpha {}",
        early.plate.fill.a()
    );
    assert!(
        early.panel.min.y < rest.min.y - 1.0,
        "and still above its rest: top {} against {}",
        early.panel.min.y,
        rest.min.y
    );
    h.frames(40);
    let late = need(drawn(&mut h), "the plate held")?;
    assert_eq!(late.plate.fill.a(), 255, "held, it lands solid");
    assert!(!late.soft, "and sharp");
    assert!(
        (late.panel.min.y - rest.min.y).abs() < 1.0,
        "at its rest: top {} against {}",
        late.panel.min.y,
        rest.min.y
    );
    h.release(egui::pos2(x, screen.height() * 0.4));
    h.frames(40);
    Ok(())
}

/// **On a wide screen the card rests on the side it was pulled from** — here a screen where the
/// tile row's need, rather than the share, sets the width; it is still a card beside the page.
#[test]
fn a_wide_screen_puts_the_card_on_the_side_pulled_from() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    pull_to(&mut h, screen.width() * 0.15, screen.height() * 0.8);
    let left = need(h.shell.overlay().frame().panel, "the left card")?;
    assert!(
        left.center().x < screen.center().x && left.min.x < screen.width() * 0.1,
        "pulled from the left it rests on the left: {left:?}"
    );
    tap_beside(&mut h, left);
    assert!(h.shell.overlay().is_closed());
    pull_to(&mut h, screen.width() * 0.85, screen.height() * 0.8);
    let right = need(h.shell.overlay().frame().panel, "the right card")?;
    assert!(
        right.center().x > screen.center().x && right.max.x > screen.width() * 0.9,
        "pulled from the right it rests on the right: {right:?}"
    );
    assert!(
        (left.width() - right.width()).abs() < 1.0,
        "the same card either side"
    );
    Ok(())
}

/// **A screen too narrow for the share gives the card the whole width** — a phone's shade, with
/// the same inset either side, whatever side it was pulled from.
#[test]
fn a_narrow_screen_gives_the_card_the_whole_width() -> fairing::Result<()> {
    let mut h = shell("card", false, (480.0, 800.0))?;
    let screen = h.screen_rect();
    pull_to(&mut h, screen.width() * 0.15, screen.height() * 0.8);
    let panel = need(h.shell.overlay().frame().panel, "the card")?;
    let (left, right) = (panel.min.x - screen.min.x, screen.max.x - panel.max.x);
    assert!(
        left > 2.0 && (left - right).abs() < 1.0,
        "the same inset either side: {left} and {right} on {panel:?}"
    );
    let pad = h.shell.theme().metrics.corner_radius * 1.34;
    assert!(
        (left - pad).abs() < 1.0,
        "the inset is the shade's edge padding ({pad}): {left}"
    );
    Ok(())
}

/// **It goes out in a fraction of the time it came.** The open is the release spring; the
/// close is a short tween.
#[test]
fn a_card_goes_out_in_a_fraction_of_the_time_it_came() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    let x = screen.width() * 0.5;
    let to_y = screen.height() * 0.6;
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    for i in 1..=10 {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / 10.0;
        h.move_to(egui::pos2(x, 2.0 + (to_y - 2.0) * t));
        h.frames(1);
    }
    for _ in 0..6 {
        h.move_to(egui::pos2(x, to_y));
        h.frames(1);
    }
    h.release(egui::pos2(x, to_y));
    let mut opening = 0;
    while !h.shell.overlay().is_open() && opening < 120 {
        h.frame();
        opening += 1;
    }
    assert!(
        (4..120).contains(&opening),
        "the open settles on the spring: {opening} frames"
    );
    let panel = need(h.shell.overlay().frame().panel, "the card")?;
    let at = beside(&h, panel);
    h.tap(at);
    let mut closing = 0;
    while !h.shell.overlay().is_closed() && closing < 120 {
        h.frame();
        closing += 1;
    }
    assert!(
        closing * 2 < opening,
        "the close is a fraction of the open: {closing} frames out against {opening} in"
    );
    Ok(())
}

/// **A two-step card keeps its stop, and a press on the page at the stop still closes it**
/// (holds for a card). The card is as tall as the tiles at the stop and grows past it.
#[test]
fn a_two_step_card_stops_at_the_tiles_and_a_press_on_the_page_closes_it() -> fairing::Result<()> {
    let mut h = shell("card", true, (1024.0, 600.0))?;
    let screen = h.screen_rect();
    let x = screen.width() * 0.5;
    let stop = pull_to(&mut h, x, screen.height() * 0.35);
    let full = h.shell.overlay().height();
    assert!(
        h.shell.overlay().is_open() && stop > 1.0 && stop < full - 1.0,
        "a two-step card rests at the tiles first: y {stop} of {full}"
    );
    let at_stop = need(h.shell.overlay().frame().panel, "the card at the stop")?;
    assert!(
        (at_stop.height() - stop).abs() < 1.0,
        "at the stop the card is as tall as the stop: {} against {stop}",
        at_stop.height()
    );
    // A press on the page, away from the card.
    let at = beside(&h, at_stop);
    h.tap(at);
    h.frames(40);
    assert!(
        h.shell.overlay().is_closed(),
        "a press on the page closes a card resting at the stop: y {}",
        h.shell.overlay().y()
    );
    // Past the stop it grows: pull on and the card is taller than the stop.
    pull_to(&mut h, x, screen.height() * 0.35);
    // Grab the card itself, low on it and below the tiles: it sits to one side of the screen,
    // and how far across depends on how wide the card resolves.
    let card = need(h.shell.overlay().frame().panel, "the card at the stop")?;
    let (gx, y) = (card.center().x, card.max.y - 4.0);
    h.press(egui::pos2(gx, y));
    h.frames(1);
    for i in 1..=10 {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / 10.0;
        h.move_to(egui::pos2(gx, y + (screen.height() * 0.95 - y) * t));
        h.frames(1);
    }
    h.release(egui::pos2(gx, screen.height() * 0.95));
    h.frames(40);
    let grown = need(h.shell.overlay().frame().panel, "the grown card")?;
    assert!(
        grown.height() > at_stop.height() * 1.2,
        "past the stop the card grows: {} from {}",
        grown.height(),
        at_stop.height()
    );
    Ok(())
}

/// **A card stands off the page; a curtain does not**. Over its plate a card draws one
/// mesh, white along its top edge and black along its bottom. On the dark palette it also draws
/// the floating rim — the theme's boundary for a floating thing, which the shadow alone left out —
/// and on a light one no rim, its boundary being the shadow. `card_relief = 0` lays it flat and
/// keeps the rim, which is the elevation's and not the relief's. A curtain draws neither.
#[test]
fn a_card_stands_off_the_page_and_a_curtain_does_not() -> fairing::Result<()> {
    let open = |h: &mut Harness| {
        let screen = h.screen_rect();
        pull_to(h, screen.width() * 0.5, screen.height() * 0.8);
    };
    let mut h = shell("card", false, (1280.0, 800.0))?;
    open(&mut h);
    let r = raised(&mut h)?;
    assert!(
        r.light.is_some() && r.shade,
        "a card is lit along the top and shaded along the bottom"
    );
    assert!(r.rim, "and on the dark palette it has the floating rim");

    let mut h = shell_with("card", false, (1280.0, 800.0), |c| {
        "light".clone_into(&mut c.shell.theme);
    })?;
    open(&mut h);
    let r = raised(&mut h)?;
    assert!(r.light.is_some() && r.shade, "a light palette's card too");
    assert!(!r.rim, "with no rim: its boundary is the shadow");

    let mut h = shell_with("card", false, (1280.0, 800.0), |c| {
        c.overlay.card_relief = 0.0;
    })?;
    open(&mut h);
    let r = raised(&mut h)?;
    assert!(r.light.is_none(), "card_relief = 0 lays the card flat");
    assert!(r.rim, "and keeps the rim");

    let mut h = shell("curtain", false, (1280.0, 800.0))?;
    open(&mut h);
    let r = raised(&mut h)?;
    assert!(
        r.light.is_none() && !r.rim,
        "a curtain is a sheet, not a slab"
    );
    Ok(())
}

/// **The relief comes in with the card**: nothing while it is still a blob, then
/// brighter frame by frame as its content resolves, and at rest as bright as it gets — never
/// dimmer on the way in.
#[test]
fn the_relief_comes_in_with_the_card() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    let x = screen.width() * 0.5;
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    let mut seen = Vec::new();
    for i in 1..=24 {
        #[expect(clippy::cast_precision_loss, reason = "24 steps")]
        let t = i as f32 / 24.0;
        h.move_to(egui::pos2(x, 2.0 + screen.height() * 0.8 * t));
        seen.push(raised(&mut h)?.light.unwrap_or(0));
    }
    h.release(egui::pos2(x, screen.height() * 0.8));
    h.frames(40);
    let rest = need(raised(&mut h)?.light, "the light at rest")?;
    assert_eq!(
        seen.first(),
        Some(&0),
        "none while the card is a blob: {seen:?}"
    );
    assert!(
        seen.windows(2)
            .all(|w| matches!(w, [before, after] if after >= before)),
        "never dimmer on the way in: {seen:?}"
    );
    assert!(
        seen.iter().any(|&a| a > 0 && a < rest),
        "partway in on the way: {seen:?} (at rest {rest})"
    );
    assert!(
        seen.iter().all(|&a| a <= rest),
        "and at rest as bright as it gets: {seen:?} (at rest {rest})"
    );
    Ok(())
}

/// **The curtain stays the default**, and stays a curtain: the full width, hung from the top,
/// only its bottom corners round.
#[test]
fn the_curtain_stays_the_default() -> fairing::Result<()> {
    let mut h = shell("curtain", false, (1280.0, 800.0))?;
    assert_eq!(h.shell.overlay().reveal(), OverlayReveal::Curtain);
    let screen = h.screen_rect();
    pull_to(&mut h, screen.width() * 0.5, screen.height() * 0.8);
    let d = need(drawn(&mut h), "the plate")?;
    assert!(
        (d.panel.min.x - screen.min.x).abs() < 1.0 && (d.panel.max.x - screen.max.x).abs() < 1.0,
        "a curtain spans the width: {:?}",
        d.panel
    );
    let cr = d.plate.corner_radius;
    assert!(
        cr.nw == 0 && cr.ne == 0 && cr.sw > 0,
        "a curtain rounds its bottom corners only: {cr:?}"
    );
    assert!(!d.soft, "a curtain has no soft edge");
    Ok(())
}

/// **A card goes out the way it came, backwards** — the plate fades as it lifts back up the way
/// it came down and its edge goes soft again; it keeps its height the whole way (a card leaving
/// does not shrink), and it is gone in a handful of frames.
#[test]
fn a_card_goes_out_the_way_it_came() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    pull_to(&mut h, screen.width() * 0.5, screen.height() * 0.8);
    let rest = need(drawn(&mut h), "the plate at rest")?;
    assert_eq!(rest.plate.fill.a(), 255);
    // A tap beside it, pressed and let go by hand so every frame of the way out is seen.
    let beside = beside(&h, rest.panel);
    h.press(beside);
    h.frame();
    h.release(beside);
    let mut out = Vec::new();
    for _ in 0..30 {
        // A plate faded to nothing is not found, and that is not the end: run until it is shut.
        if let Some(d) = drawn(&mut h) {
            out.push(d);
        }
        if h.shell.overlay().is_closed() {
            break;
        }
    }
    assert!(h.shell.overlay().is_closed(), "the tap beside it shut it");
    let leaving: Vec<&Drawn> = out.iter().filter(|d| d.plate.fill.a() < 255).collect();
    assert!(
        !leaving.is_empty() && leaving.len() <= 8,
        "it is seen going, and gone in a handful of frames: {} frames",
        leaving.len()
    );
    for (a, b) in out.iter().zip(out.iter().skip(1)) {
        assert!(
            b.plate.fill.a() <= a.plate.fill.a(),
            "it only fades on the way out: {} then {}",
            a.plate.fill.a(),
            b.plate.fill.a()
        );
        assert!(
            b.panel.min.y <= a.panel.min.y + 0.01,
            "it only lifts on the way out: top {} then {}",
            a.panel.min.y,
            b.panel.min.y
        );
    }
    for d in &out {
        assert!(
            (d.panel.height() - rest.panel.height()).abs() < 1.0,
            "it keeps its height on the way out: {} against {}",
            d.panel.height(),
            rest.panel.height()
        );
    }
    let last = need(leaving.last().copied(), "the last frame it was seen")?;
    assert!(
        last.panel.min.y < rest.panel.min.y - 1.0,
        "it went back up the way it came: last top {} against its rest {}",
        last.panel.min.y,
        rest.panel.min.y
    );
    assert!(
        leaving.iter().any(|d| d.soft),
        "its edge went soft again on the way out"
    );
    Ok(())
}

/// A screenshot-sized picture to answer with: bold vertical stripes, so a frost shows.
fn stripes(h: &Harness) -> egui::ColorImage {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a test screen of a thousand points"
    )]
    let (w, ht) = (
        h.screen_rect().width() as usize,
        h.screen_rect().height() as usize,
    );
    let mut pixels = Vec::with_capacity(w * ht);
    for _ in 0..ht {
        for x in 0..w {
            pixels.push(if (x / 40) % 2 == 0 {
                egui::Color32::from_rgb(220, 60, 60)
            } else {
                egui::Color32::from_rgb(40, 60, 200)
            });
        }
    }
    egui::ColorImage::new([w, ht], pixels)
}

/// Pull a card open from the middle of the top edge, answering the screenshot it asks for as it
/// first shows with [`stripes`] the way a runner would, a frame later. How many it had asked for
/// by the time it was answered comes back.
fn open_frosted(h: &mut Harness) -> usize {
    let screen = h.screen_rect();
    let before = h.screenshots_requested();
    let x = screen.width() * 0.5;
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    for i in 1..=4 {
        #[expect(clippy::cast_precision_loss, reason = "four steps")]
        let y = 2.0 + 40.0 * i as f32;
        h.move_to(egui::pos2(x, y));
        h.frames(1);
    }
    let asked = h.screenshots_requested() - before;
    let shot = stripes(h);
    h.answer_screenshot(shot);
    h.move_to(egui::pos2(x, screen.height() * 0.8));
    h.frames(6);
    h.release(egui::pos2(x, screen.height() * 0.8));
    h.frames(40);
    asked
}

/// **A card frosts what it lies on**. As it first shows it asks the runner for one
/// screenshot; when the answer comes it lies on a frosted copy cut to its own shape, and its
/// plate lets three tenths of it through (`card_glass` 0.7).
#[test]
fn a_card_frosts_what_it_lies_on() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let before = h.screenshots_requested();
    assert_eq!(
        open_frosted(&mut h),
        1,
        "the card asked for one screenshot as it first showed"
    );
    assert!(h.shell.overlay().is_open());
    let d = need(drawn(&mut h), "the plate at rest")?;
    assert!(
        d.frost,
        "the frosted copy lies under the card, cut to its shape"
    );
    let alpha = d.plate.fill.a();
    assert!(
        (175..=183).contains(&alpha),
        "on its frost the plate is glass at 0.7: alpha {alpha}"
    );
    assert_eq!(
        h.screenshots_requested(),
        before + 1,
        "and asked only the once"
    );
    Ok(())
}

/// **A frost that no longer matches is put down**. The dark mode tile is on the
/// controls card: switched there, the page behind turns dark (or light) while the frost still
/// shows it as it was, and three tenths of that under the switched plate washes the card out. So
/// the card goes solid, and does not ask again — it is over the page by then, and a second
/// screenshot would frost the card into its own backdrop. The next open frosts afresh.
#[test]
fn a_theme_switched_under_the_card_puts_its_frost_down() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let before = h.screenshots_requested();
    assert_eq!(open_frosted(&mut h), 1);
    assert!(
        need(drawn(&mut h), "the plate on its frost")?.frost,
        "glass to begin with"
    );
    let dark = h.shell.theme().dark;
    h.shell.set_theme_dark(!dark);
    h.frames(2);
    let d = need(drawn(&mut h), "the plate after the switch")?;
    assert!(
        !d.frost,
        "the frost of the page as it was is not left under the switched card"
    );
    assert_eq!(d.plate.fill.a(), 255, "the plate is solid again");
    assert_eq!(
        h.screenshots_requested(),
        before + 1,
        "and it does not ask again with the card over the page"
    );
    tap_beside(&mut h, d.panel);
    assert!(!h.shell.overlay().is_open(), "a tap beside it shut it");
    assert_eq!(open_frosted(&mut h), 1, "the next open asks afresh");
    assert!(
        need(drawn(&mut h), "the plate on its new frost")?.frost,
        "and lies on glass again"
    );
    Ok(())
}

/// **No answer, no glass**: without a screenshot back the card stays solid, and with
/// `card_glass = 1` it does not ask at all.
#[test]
fn without_a_frost_the_card_stays_solid() -> fairing::Result<()> {
    let mut h = shell("card", false, (1280.0, 800.0))?;
    let screen = h.screen_rect();
    pull_to(&mut h, screen.width() * 0.5, screen.height() * 0.8);
    let d = need(drawn(&mut h), "the plate")?;
    assert!(!d.frost, "nothing came back, so nothing is under it");
    assert_eq!(d.plate.fill.a(), 255, "and the plate is solid");

    let mut h = shell_with("card", false, (1280.0, 800.0), |c| {
        c.overlay.card_glass = 1.0;
    })?;
    let before = h.screenshots_requested();
    pull_to(&mut h, screen.width() * 0.5, screen.height() * 0.8);
    assert_eq!(
        h.screenshots_requested(),
        before,
        "with the glass off, a card asks for nothing"
    );
    Ok(())
}
