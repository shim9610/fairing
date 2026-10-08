//! **The shade can stop at the tiles on the way down** — `[overlay] two_step`.
//!
//! It only ever had two positions, so reaching the shade to toggle Wi-Fi covered the screen with
//! an empty notification list. With the option on there is a stop where the tiles end: one pull
//! lands on it and a second carries on. The stop's height is read off the rects the panel drew,
//! because how tall the tile block is depends on how many tiles the gates left and how many
//! columns they fell into, both decided while the panel draws.
//!
//! The release rules are applied to **the gap the finger is in** rather than to the whole travel.
//! Against the whole travel a pull that stopped at the tiles reads as 40 % of the way open and
//! falls back shut, which is the bug this would otherwise have.

#![cfg(feature = "overlay")]

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::BigButton;
use fairing::{screen, Cx, Shell};

/// A shell whose shade is one-step or two.
fn shell(two_step: bool) -> fairing::Result<Harness> {
    Harness::from_builder(move |ctx| {
        let mut config = single_level_access();
        config.overlay.two_step = two_step;
        let mut shell = Shell::builder(config).build(ctx)?;
        shell.add(screen("s", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            fairing::layout::page(ui, cx, "s", |ui, cx| {
                fairing::layout::note(ui, cx, "body");
            });
        }));
        shell.launch(fairing::LaunchAction::open("s"));
        Ok(shell)
    })
    .map(|mut h| {
        h.frames(3);
        h
    })
}

/// Pull the shade from the top edge to `to_y`, let go, and settle. Returns where it came to rest.
fn pull_to(h: &mut Harness, to_y: f32) -> f32 {
    let x = h.screen_rect().width() * 0.5;
    h.press(egui::pos2(x, 2.0));
    h.frames(1);
    // In steps, so the engine sees a drag rather than a jump.
    let steps = 10;
    for i in 1..=steps {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / steps as f32;
        h.move_to(egui::pos2(x, 2.0 + (to_y - 2.0) * t));
        h.frames(1);
    }
    // **Hold still before letting go.** A release still carrying speed is a fling, and a fling is
    // meant to skip the stop — that is the rule, not a bug to work around. What these are about is
    // where a deliberate pull comes to rest.
    for _ in 0..6 {
        h.move_to(egui::pos2(x, to_y));
        h.frames(1);
    }
    h.release(egui::pos2(x, to_y));
    h.frames(40);
    h.shell.overlay().y()
}

/// **A two-step shade opens the tiles and stops.** The same pull on a one-step shade goes to the
/// bottom, which is the difference the option buys.
#[test]
fn the_first_pull_stops_at_the_tiles() -> fairing::Result<()> {
    let mut two = shell(true)?;
    let mut one = shell(false)?;
    // Open and close once, so the panel has drawn and the stop is known.
    let tall = two.screen_rect().height();
    let full = pull_to(&mut two, tall * 0.9);
    pull_to(&mut two, 2.0);

    let half = tall * 0.32;
    let stopped = pull_to(&mut two, half);
    let straight_through = pull_to(&mut one, half);

    assert!(
        stopped > 1.0 && stopped < full * 0.9,
        "a two-step shade's first pull has to come to rest short of the bottom: it stopped at \
         {stopped} against a full {full}"
    );
    assert!(
        straight_through > stopped * 1.2,
        "the same pull on a one-step shade goes further, or the option is doing nothing: \
         one-step {straight_through}, two-step {stopped}"
    );
    Ok(())
}

/// **A second pull carries on.** The stop is a stop, not a ceiling.
#[test]
fn a_second_pull_carries_on_to_the_notifications() -> fairing::Result<()> {
    let mut h = shell(true)?;
    let tall = h.screen_rect().height();
    let wide = h.screen_rect().width();
    let full = pull_to(&mut h, tall * 0.9);
    pull_to(&mut h, 2.0);

    let first = pull_to(&mut h, tall * 0.32);
    // From the stop, grab the panel and keep going.
    let x = wide * 0.5;
    h.press(egui::pos2(x, first - 4.0));
    h.frames(1);
    for i in 1..=10 {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / 10.0;
        h.move_to(egui::pos2(x, first - 4.0 + (tall * 0.95 - first) * t));
        h.frames(1);
    }
    h.release(egui::pos2(x, tall * 0.95));
    h.frames(40);
    let second = h.shell.overlay().y();
    assert!(
        second > first * 1.2,
        "a second pull from the stop has to reach the notifications: it went {first} then {second} \
         against a full {full}"
    );
    Ok(())
}

/// **A grab at the stop whose first move lands with the press still carries on**. From
/// the stop the hand takes the panel by its bottom edge and pulls on. On a slow frame the press
/// and the first move arrive together, and the moved point is already well below the panel, past
/// the band under it that still counts as the panel: read off that, the press was on the page,
/// and the rule that a press outside the resting panel closes it shut the shade the hand was
/// pulling open (it went from the stop to 0). Read off where the finger came down, it is a grab on
/// the panel, and the shade carries on.
#[test]
fn a_grab_at_the_stop_that_lands_with_its_first_move_carries_on() -> fairing::Result<()> {
    let mut h = shell(true)?;
    let tall = h.screen_rect().height();
    let wide = h.screen_rect().width();
    pull_to(&mut h, tall * 0.9);
    pull_to(&mut h, 2.0);

    let first = pull_to(&mut h, tall * 0.32);
    let x = wide * 0.5;
    h.press(egui::pos2(x, first - 4.0));
    h.move_to(egui::pos2(x, first + 120.0));
    h.frames(1);
    for i in 1..=10 {
        #[expect(clippy::cast_precision_loss, reason = "ten steps")]
        let t = i as f32 / 10.0;
        h.move_to(egui::pos2(
            x,
            first + 120.0 + (tall * 0.95 - first - 120.0) * t,
        ));
        h.frames(1);
    }
    h.release(egui::pos2(x, tall * 0.95));
    h.frames(40);
    let second = h.shell.overlay().y();
    assert!(
        h.shell.overlay().is_open() && second > first * 1.2,
        "a grab on the panel at the stop has to carry on, not shut it: {first} then {second}"
    );
    Ok(())
}

/// **The very first pull of the session stops too.**
///
/// The stop is read off the frame the panel drew, and the panel draws on every frame of the pull —
/// so by the time the finger lets go the block's height has been known for a dozen frames. If this
/// only worked from the second open, a device would go full-height once on boot and two-step ever
/// after, which is worse than either.
#[test]
fn even_the_first_pull_of_the_session_stops() -> fairing::Result<()> {
    let mut h = shell(true)?;
    let tall = h.screen_rect().height();
    let rest = pull_to(&mut h, tall * 0.32);
    assert!(
        rest > 1.0 && rest < tall * 0.5,
        "the first pull ever has to come to rest at the tiles like any other: it went to {rest}"
    );
    Ok(())
}

/// **The stop is under an undimmed screen.**
///
/// The scrim ramps with the pull, so on a shade that stops a quarter of the way down it has
/// already taken a quarter of the contrast off everything behind — which is most of the reason to
/// stop there gone. On a two-step shade it holds at nothing until the stop and takes its whole
/// sweep over what is left, so the dimming belongs to the notification list rather than to the
/// tiles.
#[test]
fn the_tiles_come_down_over_an_undimmed_screen() -> fairing::Result<()> {
    let mut two = shell(true)?;
    let mut one = shell(false)?;
    let tall = two.screen_rect().height();

    let at_stop = {
        pull_to(&mut two, tall * 0.32);
        two.shell.overlay().scrim_alpha()
    };
    let same_pull_one_step = {
        pull_to(&mut one, tall * 0.32);
        one.shell.overlay().scrim_alpha()
    };
    assert!(
        at_stop < 0.01,
        "at the stop the screen behind has to be left alone: the scrim was {at_stop}"
    );
    assert!(
        same_pull_one_step > 0.05,
        "the one-step shade is the control here and has to dim: it was {same_pull_one_step}"
    );

    // Past the stop it comes back — the notifications are modal like any other sheet.
    pull_to(&mut two, tall * 0.95);
    let wide_open = two.shell.overlay().scrim_alpha();
    assert!(
        wide_open > at_stop + 0.1,
        "past the stop the scrim has to arrive: {at_stop} at the stop, {wide_open} open"
    );
    Ok(())
}

/// **A row expanding under the stop moves the stop.** Resting on the tiles, a tap on a panel
/// tile opens its row — and the shade follows the block down to hold it, rather than leaving
/// the row cut off under the fold. A panel tile, because the built-in slider tiles
/// are not live without their services.
#[test]
fn a_row_expanding_at_the_stop_takes_the_shade_down_with_it() -> fairing::Result<()> {
    let mut h = shell(true)?;
    h.shell
        .add(fairing::overlay::tile_panel("tile.hopper", 168.0, |_ui, _cx| {}).label("Hoppers"));
    h.frames(2);
    let stop_y = pull_to(&mut h, 200.0);
    let tile = h
        .shell
        .overlay()
        .tile_rect("tile.hopper")
        .unwrap_or(egui::Rect::NOTHING);
    assert!(tile.is_positive(), "no hopper tile on the shade");
    assert!(
        h.shell.overlay().expanded_rect().is_none(),
        "a row is open before anything was tapped"
    );
    h.tap(tile.center());
    h.frames(60);
    let overlay = h.shell.overlay();
    let Some(row) = overlay.expanded_rect() else {
        return Err(fairing::Error::Runner(
            "tapping the hopper tile did not open its row".into(),
        ));
    };
    let y = overlay.y();
    // The stop moved down by about the row it took in.
    assert!(
        y > stop_y + row.height() * 0.5,
        "the shade rested at {stop_y}; with a {}-tall row open it is at {y}",
        row.height()
    );
    Ok(())
}

/// The page's button: where it was drawn, and how often it was pressed.
struct Page {
    button: egui::Rect,
    presses: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            button: egui::Rect::NOTHING,
            presses: 0,
        }
    }
}

/// **At the stop, doing anything else closes it**. The tiles come down over an
/// undimmed, live screen, so there is no scrim to take the tap that closes a full shade —
/// and a shade pulled down to toggle Wi-Fi stayed over the top of the screen through everything
/// the hand did next (user report). A press outside the panel now closes it, and still lands
/// where it was aimed: the button under it is pressed.
#[test]
fn a_press_on_the_page_closes_a_shade_resting_at_the_stop() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut config = single_level_access();
        config.overlay.two_step = true;
        let mut shell = Shell::builder(config).build(ctx)?;
        shell.add(screen("s", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            fairing::layout::page(ui, cx, "s", |ui, cx| {
                // Low on the page: well under where the tiles stop.
                ui.add_space(cx.pane.rect.height() * 0.6);
                cx.with_app::<Page, _>(|p, cx| {
                    let shown = BigButton::new("Go").show(ui, &mut cx.widgets());
                    p.button = shown.response.rect;
                    if shown.response.clicked() {
                        p.presses += 1;
                    }
                });
            });
        }));
        shell.launch(fairing::LaunchAction::open("s"));
        Ok(shell)
    })?
    .with_app(Page::default());
    h.frames(3);
    let tall = h.screen_rect().height();
    let stop = pull_to(&mut h, tall * 0.32);
    let full = h.shell.overlay().height();
    assert!(
        h.shell.overlay().is_open() && stop < full - 1.0,
        "the shade has to rest at the stop first: y {stop} of {full}"
    );
    let Some(button) = h.app_mut::<Page>().map(|p| p.button) else {
        return Err(fairing::Error::Config("the page state is there".to_owned()));
    };
    assert!(
        button.top() > stop + 1.0,
        "the button is on the page under the shade: {button:?}, shade down to {stop}"
    );
    h.tap(button.center());
    h.frames(40);
    assert!(
        h.shell.overlay().is_closed(),
        "a press on the page has to close a shade resting at the stop: y {}",
        h.shell.overlay().y()
    );
    let presses = h.app_mut::<Page>().map_or(0, |p| p.presses);
    assert_eq!(presses, 1, "and the press still reaches the button");
    Ok(())
}
