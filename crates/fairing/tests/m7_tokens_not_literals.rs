//! **A type size is the scale's and a time is the motion's** — two places that had a number of
//! their own: the status card sized its words as a share of the row (a gloved row made them half
//! as big again), and the long-press ring's cancel ran on a literal 80 ms that `motion.reduce`
//! could not turn off.
#![cfg(all(feature = "mock", feature = "overlay"))]

use fairing::layout;
use fairing::testing::{single_level_access, test_shell, Harness};
use fairing::unit::ScalePolicy;
use fairing::widgets::BigButton;
use fairing::{screen, Cx, Shell};
use std::time::Duration;

/// The shade open at `policy`'s finger, with two tiles on it.
fn shade_at(policy: ScalePolicy) -> fairing::Result<Harness> {
    use fairing::settings::SettingKey;
    use fairing::{tile, TileKind};
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .physical_mm(200.0, 120.0)
            .scale_policy(policy)
            .build(ctx)?;
        shell.add(tile("a", TileKind::Toggle(SettingKey::from("app.a"))).label("A"));
        shell.add(tile("b", TileKind::Toggle(SettingKey::from("app.b"))).label("B"));
        shell.launch(fairing::LaunchAction::OpenOverlay);
        Ok(shell)
    })?;
    h.frames(60);
    Ok(h)
}

/// **The gap between two quick tiles is the finger's `screen_inset`**, so a gloved panel's row
/// breathes like its rows do; it was a fixed 12 du at any finger.
#[test]
fn the_quick_tile_gap_follows_the_finger() -> fairing::Result<()> {
    let mut gaps = (0.0, 0.0);
    for (gloved, policy) in [
        (false, ScalePolicy::default()),
        (true, ScalePolicy::gloved()),
    ] {
        let h = shade_at(policy)?;
        let (a, b) = (
            h.shell.overlay().tile_rect("a"),
            h.shell.overlay().tile_rect("b"),
        );
        let (Some(a), Some(b)) = (a, b) else {
            return Err(fairing::Error::Config("the tiles were not laid out".into()));
        };
        let gap = b.min.x - a.max.x;
        let inset = h.shell.theme().metrics.screen_inset;
        assert!(
            (gap - inset).abs() < 0.5,
            "the gap is {gap}, the finger's screen_inset is {inset}"
        );
        if gloved {
            gaps.1 = gap;
        } else {
            gaps.0 = gap;
        }
    }
    assert!(
        gaps.1 > gaps.0 + 1.0,
        "the gloved gap ({}) is no wider than the bare one ({})",
        gaps.1,
        gaps.0
    );
    Ok(())
}

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

/// **The gap between status items is `components.status_bar.item_gap`** — a token with a default
/// (10 du) an integrator can set through `ShellBuilder::component_spec`, where it was a constant.
#[test]
fn the_status_bar_gap_is_a_component_token() -> fairing::Result<()> {
    use fairing::theme::ComponentSpec;
    use fairing::unit::{Dim, Span};
    let wide = 31.0;
    let mut h = Harness::from_builder(move |ctx| {
        let spec = ComponentSpec {
            status_bar: [Span::fixed(Dim::du(wide)), Span::fixed(Dim::du(8.0))],
            ..ComponentSpec::default()
        };
        Shell::builder(single_level_access())
            .services(fairing::services::mock::services())
            .component_spec(spec)
            .build(ctx)
    })?;
    h.frames(4);
    let sb = h.shell.status_bar();
    let mut rects: Vec<egui::Rect> = ["status.notifications", "status.bluetooth", "status.wifi"]
        .into_iter()
        .filter_map(|id| sb.item_rect(id))
        .collect();
    assert!(
        rects.len() >= 2,
        "the right cluster was not laid out: {rects:?}"
    );
    rects.sort_by(|a, b| a.min.x.total_cmp(&b.min.x));
    for pair in rects.windows(2) {
        let (a, b) = (pair.first(), pair.get(1));
        let (Some(a), Some(b)) = (a, b) else {
            continue;
        };
        let gap = b.min.x - a.max.x;
        assert!(
            (gap - wide).abs() < 0.5,
            "the gap is {gap}, the spec said {wide}"
        );
    }
    Ok(())
}
