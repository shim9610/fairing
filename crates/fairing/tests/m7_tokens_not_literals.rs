//! **A type size is the scale's and a time is the motion's** — two places that had a number of
//! their own: the status card sized its words as a share of the row (a gloved row made them half
//! as big again), and the long-press ring's cancel ran on a literal 80 ms that `motion.reduce`
//! could not turn off.
#![cfg(feature = "mock")]

use fairing::layout;
use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::unit::ScalePolicy;
use fairing::widgets::BigButton;
use fairing::{screen, Cx, Shell};
use std::time::Duration;

/// The status card drawn at `policy`'s finger, settled.
fn status_card_at(policy: ScalePolicy) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .physical_mm(200.0, 120.0)
            .scale_policy(policy)
            .build(ctx)?;
        shell.add(screen("probe", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::status_card(ui, cx, "Connected", Some("Lab-5G, strong"), None);
        }));
        shell.launch(fairing::LaunchAction::open("probe"));
        Ok(shell)
    })?;
    h.frames(30);
    Ok(h)
}

/// The font size the text reading `label` was drawn at this frame.
fn size_of(h: &mut Harness, label: &str) -> Option<f32> {
    h.frame_shapes().into_iter().find_map(|c| match c.shape {
        egui::Shape::Text(t) if t.galley.job.text == label => {
            t.galley.job.sections.first().map(|s| s.format.font_id.size)
        }
        _ => None,
    })
}

/// **The status card's words are the type scale's**, at a bare finger and at a gloved one: the
/// title is `body` and the detail `small`, not 0.34 and 0.23 of a row that grows with the hand.
#[test]
fn the_status_card_is_set_in_the_type_scale() -> fairing::Result<()> {
    for policy in [ScalePolicy::default(), ScalePolicy::gloved()] {
        let mut h = status_card_at(policy)?;
        let scale = h.shell.theme().metrics.type_scale;
        let title = size_of(&mut h, "Connected")
            .ok_or_else(|| fairing::Error::Config("the title was not drawn".into()))?;
        let detail = size_of(&mut h, "Lab-5G, strong")
            .ok_or_else(|| fairing::Error::Config("the detail was not drawn".into()))?;
        assert!(
            (title - scale.body).abs() < 0.01,
            "the title is {title}, the scale's body is {}",
            scale.body
        );
        assert!(
            (detail - scale.small).abs() < 0.01,
            "the detail is {detail}, the scale's small is {}",
            scale.small
        );
    }
    Ok(())
}

/// **A long press let go early stops at once under `motion.reduce`.** The ring's cancel ran on a
/// literal 80 ms tween, so the shell went on animating after the release with reduce on.
#[test]
fn a_cancelled_long_press_is_instant_under_reduce() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |shell| {
        shell.add(screen("probe", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = BigButton::new("Hold")
                .long_press(Duration::from_secs(2))
                .show(ui, &mut cx.widgets());
        }));
        shell.launch(fairing::LaunchAction::open("probe"));
    })?;
    h.frames(4);
    let at = h.text_rect("Hold")?.center();
    h.hold(at, 6);
    h.release(at);
    h.frame();
    h.frame();
    assert!(
        !h.shell.is_animating(),
        "the ring is still winding back after the release with motion.reduce on"
    );
    Ok(())
}
