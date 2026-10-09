//! **A finger finds a row by its label, not by a number** — the regression the console tour had
//! after the rows shrank to one finger: a `Tap(90, 210)` written for the old row height landed
//! on the entry below, and nothing said so. The harness names the place instead
//! ([`Harness::text_rect`], [`Harness::tap_text`]), so a test written today is still right when
//! the finger, the type scale or the density changes.
#![cfg(all(feature = "settings", feature = "mock"))]

use fairing::settings::{add_all, SettingsConfig};
use fairing::testing::{single_level_access, Harness};
use fairing::unit::ScalePolicy;
use fairing::{Error, Shell};

/// The settings list on the Mock backends at `policy`'s finger, with the home screen open.
fn settings_at(policy: ScalePolicy) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .physical_mm(200.0, 120.0)
            .scale_policy(policy)
            .services(fairing::services::mock::services())
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        shell.launch(fairing::LaunchAction::open("settings.home"));
        Ok(shell)
    })?;
    h.frames(30);
    Ok(h)
}

/// **The same row is in a different place at a different finger, and the label still finds
/// it.** The bare-finger and the gloved rows differ by more than a quarter row, so the old
/// literal centre of one lands on another entry — while `tap_text` opens the right screen at
/// either.
#[test]
fn a_row_found_by_its_label_survives_a_change_of_finger() -> fairing::Result<()> {
    let mut bare = settings_at(ScalePolicy::default())?;
    let mut gloved = settings_at(ScalePolicy::gloved())?;
    let at_bare = bare.text_rect("Display")?;
    let at_gloved = gloved.text_rect("Display")?;
    let moved = (at_gloved.center().y - at_bare.center().y).abs();
    assert!(
        moved > at_bare.height(),
        "the row moved only {moved} px between fingers; its label is {} px tall",
        at_bare.height()
    );

    // The display screen is the one with the brightness row; the home screen opens on Wi-Fi.
    assert!(
        gloved.text_rect("Brightness").is_err(),
        "the display screen is already open"
    );

    // The bare finger's number pressed on the gloved layout is not the Display row any more.
    gloved.tap(at_bare.center());
    gloved.frames(4);
    assert!(
        gloved.text_rect("Brightness").is_err(),
        "a tap at the bare finger's number still opened Display on the gloved layout - the \
         rows did not move, so this test no longer shows the regression"
    );

    for h in [&mut bare, &mut gloved] {
        h.tap_text("Display")?;
        h.frames(4);
        h.text_rect("Brightness")
            .map_err(|e| Error::Config(format!("tap_text(\"Display\") did not open it: {e}")))?;
    }
    Ok(())
}

/// **A label that is not on the glass is an error that names what is**, never a tap on
/// nothing — the silent failure the tours had.
#[test]
fn a_missing_label_fails_with_what_is_on_the_glass() -> fairing::Result<()> {
    let mut h = settings_at(ScalePolicy::default())?;
    match h.tap_text("No such row") {
        Err(Error::Config(why)) => {
            assert!(why.contains("No such row"), "{why}");
            assert!(
                why.contains("Display"),
                "the message does not list the glass: {why}"
            );
        }
        Err(other) => return Err(other),
        Ok(()) => {
            return Err(Error::Config(
                "a tap on a label that is not there succeeded".into(),
            ))
        }
    }
    Ok(())
}

/// **A label drawn in more than one place is an error that names each** — the finger has one
/// place to go, and the test says which by a label drawn once.
#[test]
fn an_ambiguous_label_fails_with_both_places() -> fairing::Result<()> {
    let mut h = settings_at(ScalePolicy::default())?;
    // The wide settings screen has "Wi-Fi" in its list and as the open page's title.
    match h.text_rect("Wi-Fi") {
        Err(Error::Config(why)) => assert!(why.contains("texts read \"Wi-Fi\", at ["), "{why}"),
        Err(other) => return Err(other),
        Ok(rect) => {
            return Err(Error::Config(format!(
                "\"Wi-Fi\" is drawn once at {rect:?} - pick a label drawn more than once"
            )))
        }
    }
    Ok(())
}

/// **A text scrolled out of its area is not on the glass**: `texts()` leaves it out, so a finger
/// is never sent to a place it cannot reach.
#[test]
fn a_text_scrolled_away_is_not_on_the_glass() -> fairing::Result<()> {
    let mut h = settings_at(ScalePolicy::gloved())?;
    let on_glass: Vec<String> = h.texts().into_iter().map(|(t, _)| t).collect();
    let screen = h.screen_rect();
    for (_, rect) in h.texts() {
        assert!(
            screen.intersects(rect),
            "{rect:?} is off the screen; on the glass: {on_glass:?}"
        );
    }
    Ok(())
}
