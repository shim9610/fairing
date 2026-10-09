//! **A card on a rail's page stands off the page, and a page on its way is still moving** — two
//! things the console showed up: a `Filled` card on the `SurfaceVariant` page inside the rail's
//! elbow was the page's own colour and vanished, and a page transit timed off egui's clock was
//! invisible to `Shell::is_animating`, so a picture taken "once everything had settled" caught the
//! page half faded in and its tiles reading as disabled.
#![cfg(feature = "mock")]

use fairing::layout;
use fairing::testing::{single_level_access, Harness};
use fairing::theme::ColorRole;
use fairing::{screen, Cx, Shell};

/// One screen drawn by `body`, with `state` lent to it each frame, opened and settled.
fn drawing<S: std::any::Any>(
    state: S,
    body: impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static,
) -> fairing::Result<Harness> {
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
    })?
    .with_app(state);
    // Past the open transition, which draws the arriving screen as a picture of itself.
    h.frames(30);
    Ok(h)
}

/// The fills of the rounded-on-every-corner rectangles drawn this frame — the cards.
fn card_fills(h: &mut Harness) -> Vec<egui::Color32> {
    h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r)
                if r.corner_radius.nw > 0
                    && r.corner_radius.ne > 0
                    && r.corner_radius.sw > 0
                    && r.corner_radius.se > 0 =>
            {
                Some(r.fill)
            }
            _ => None,
        })
        .collect()
}

fn one_group(ui: &mut egui::Ui, cx: &mut Cx<'_>) {
    layout::group(ui, cx, |ui, cx| {
        layout::info_row(ui, cx, "Pressure", "1.2 bar");
    });
}

/// **On a plain screen a `Filled` card is `SurfaceVariant`** — the step in from the `Surface`
/// the screen is drawn on.
#[test]
fn a_card_on_a_screen_is_surface_variant() -> fairing::Result<()> {
    let mut h = drawing((), one_group)?;
    let (surface, variant) = (
        h.shell.theme().color(ColorRole::Surface),
        h.shell.theme().color(ColorRole::SurfaceVariant),
    );
    let fills = card_fills(&mut h);
    assert!(
        fills.contains(&variant),
        "no SurfaceVariant card in {fills:?}"
    );
    assert!(
        !fills.contains(&surface),
        "a Surface card on a Surface screen: {fills:?}"
    );
    Ok(())
}

/// **On a rail's page the same card is `Surface`.** The page panel is `SurfaceVariant`, and a
/// card of the panel's own colour has no edge at all — the console's groups had none.
#[test]
fn a_card_on_a_rail_page_is_surface() -> fairing::Result<()> {
    let mut h = drawing((), |ui, cx| {
        let _ = layout::Rail::new().show(
            ui,
            cx,
            |ui, cx| {
                let _ = layout::list_item(ui, cx, fairing::icon::GAUGE, "Overview", true);
            },
            one_group,
        );
    })?;
    let (surface, variant) = (
        h.shell.theme().color(ColorRole::Surface),
        h.shell.theme().color(ColorRole::SurfaceVariant),
    );
    assert_ne!(
        surface, variant,
        "the palette has one surface colour; the test cannot tell"
    );
    let fills = card_fills(&mut h);
    assert!(
        fills.contains(&surface),
        "no Surface card on the rail's page; the rounded fills are {fills:?}"
    );
    Ok(())
}

/// The page a transit shows, keyed from the app state.
struct Shown(usize);

/// **A page on its way counts as animating.** `layout::transit` times itself off egui's clock
/// and used to ask only for a repaint; `Shell::is_animating` — what a wait-for-rest watches —
/// said the shell was still, and a picture taken then had the page half faded in.
#[test]
fn a_page_transit_keeps_the_shell_animating() -> fairing::Result<()> {
    let mut h = drawing(Shown(0), |ui, cx| {
        let key = cx.app::<Shown>().map_or(0, |s| s.0);
        layout::transit(ui, cx, key, |ui, _cx, k| {
            ui.label(format!("page {k}"));
        });
    })?;
    h.frames(10);
    assert!(!h.shell.is_animating(), "still at rest before the change");
    if let Some(s) = h.app_mut::<Shown>() {
        s.0 = 1;
    }
    h.frame();
    assert!(
        h.shell.is_animating(),
        "the page is on its way and the shell does not say so"
    );
    h.run_for(2.0);
    assert!(
        !h.shell.is_animating(),
        "the transit is long over and the shell still says moving"
    );
    Ok(())
}
