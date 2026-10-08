//! **A widget given a slot smaller than its own floor overflows it**.
//!
//! Every control in the crate floors its own size. Hand one a `max_rect` under that floor and it
//! does not shrink to fit: it takes the floor and draws outside the rect it was given. So a caller
//! reserving room for a control has to reserve *the control's* number, and the failures are all the
//! same shape - a slot worked out from some other token that used to agree with it.
//!
//! `FeatureCard` reserved `metrics.touch_target` for its action disc while `IconButton` sizes
//! itself from `control.icon_button`; the first is anchored to the hand and the second to the eye,
//! so lowering `touch_target` for a mouse-driven console left the disc hanging out of
//! the card's bottom edge. These hold the reservations to the widgets that fill them.

use fairing::layout::{self, Container, Deco};
use fairing::testing::{single_level_access, Harness};
use fairing::theme::MetricsSpec;
use fairing::unit::{Dim, Span};
use fairing::widgets::FeatureCard;
use fairing::{icon, screen, Cx, Shell};

/// A spec whose `touch_target` is well under what an `IconButton` needs - a console driven by a
/// mouse and a stylus rather than a gloved fingertip.
fn small_target() -> MetricsSpec {
    MetricsSpec {
        touch_target: Span::fixed(Dim::finger(0.5)),
        ..MetricsSpec::default()
    }
}

fn shapes_of(
    build: impl Fn(&mut egui::Ui, &mut Cx<'_>) + Send + Sync + 'static,
) -> fairing::Result<Vec<egui::Shape>> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .metrics_spec(small_target())
            .build(ctx)?;
        shell.add(screen("s", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            build(ui, cx);
        }));
        shell.launch(fairing::LaunchAction::open("s"));
        Ok(shell)
    })?;
    h.frames(3);
    Ok(h.frame_shapes().into_iter().map(|c| c.shape).collect())
}

#[test]
fn a_feature_cards_action_disc_stays_inside_the_card() -> fairing::Result<()> {
    let shapes = shapes_of(|ui, cx| {
        layout::page(ui, cx, "p", |ui, cx| {
            let _ = FeatureCard::new("Quick Test")
                .body("Use default settings.")
                .action(icon::ARROW_RIGHT, "start")
                .show(ui, &mut cx.widgets());
        });
    })?;
    // The disc is the only filled circle on the frame; the card is the filled rect around it.
    let disc = shapes
        .iter()
        .find_map(|s| match s {
            egui::Shape::Circle(c) if c.fill.a() > 0 => Some(c.visual_bounding_rect()),
            _ => None,
        })
        .unwrap_or(egui::Rect::NOTHING);
    let card = shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r) if r.fill.a() > 0 && r.rect.contains(disc.center()) => {
                Some(r.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.height().total_cmp(&b.height()))
        .unwrap_or(egui::Rect::NOTHING);
    assert!(
        disc.is_finite() && card.is_finite(),
        "expected a filled disc inside a filled card, got disc {disc:?} in card {card:?}"
    );
    assert!(
        disc.bottom() <= card.bottom() + 0.5 && disc.top() >= card.top() - 0.5,
        "the action disc runs out of the card that reserved room for it: {disc:?} in {card:?} - \
         the reservation has to be `IconButton::measure`, not another token that happens to agree"
    );
    Ok(())
}

/// The leftmost x any row paints at, which is where its icon starts.
fn left_edge(shapes: &[egui::Shape]) -> f32 {
    shapes
        .iter()
        .map(egui::Shape::visual_bounding_rect)
        .filter(|r| r.is_finite() && r.width() > 0.0 && r.width() < 200.0)
        .map(|r| r.left())
        .fold(f32::INFINITY, f32::min)
}

#[test]
fn a_divided_action_bar_lines_its_row_up_with_the_list_above() -> fairing::Result<()> {
    // One `list_item` in the body and one in the foot. A row insets itself, so a bar that insets it
    // again puts the foot a `screen_inset` to the right of everything above it.
    let with_bar = shapes_of(|ui, cx| {
        layout::action_bar_with(
            ui,
            cx,
            1.4,
            Deco::new().container(Container::Divided),
            |ui, cx| {
                let _ = layout::list_item(ui, cx, icon::HOME, "Home", false);
            },
            |ui, cx| {
                let _ = layout::list_item(ui, cx, icon::POWER, "Power Off", false);
            },
        );
    })?;
    let body_only = shapes_of(|ui, cx| {
        let _ = layout::list_item(ui, cx, icon::HOME, "Home", false);
    })?;
    let bar = left_edge(&with_bar);
    let body = left_edge(&body_only);
    assert!(
        (bar - body).abs() < 1.0,
        "a `Divided` bar's row starts at {bar} where a plain row starts at {body} - the two are \
         stacked in one column and have to share a left edge"
    );
    Ok(())
}
