//! Rendered witnesses for the canonical control and component configuration paths.
#![cfg(feature = "mock")]

use fairing::testing::{single_level_access, Harness};
use fairing::theme::{ComponentSpec, ControlSpec};
use fairing::unit::{Dim, Span};
use fairing::widgets::{ListRow, Switch, TouchSlider};
use fairing::{screen, Cx, Shell};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum Widget {
    Switch,
    Slider,
    Row,
}

fn render(
    control: &ControlSpec,
    component: &ComponentSpec,
    widget: Widget,
) -> fairing::Result<(egui::Rect, Vec<egui::epaint::ClippedShape>)> {
    let (control, component) = (*control, *component);
    let bounds = Rc::new(Cell::new(egui::Rect::NOTHING));
    let out = Rc::clone(&bounds);
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config)
            .control_spec(control)
            .component_spec(component)
            .services(fairing::Services::null())
            .build(ctx)?;
        shell.add(screen(
            "widget",
            move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                let mut on = true;
                let mut value = 0.5;
                let response = match widget {
                    Widget::Switch => Switch::new(&mut on).show(ui, &mut cx.widgets()),
                    Widget::Slider => {
                        TouchSlider::new(&mut value, 0.0..=1.0).show(ui, &mut cx.widgets())
                    }
                    Widget::Row => ListRow::new("row")
                        .trailing_icon(fairing::icon::GAUGE)
                        .trailing("rendered-value")
                        .show(ui, &mut cx.widgets()),
                };
                out.set(response.rect);
            },
        ));
        shell.launch(fairing::LaunchAction::open("widget"));
        Ok(shell)
    })?
    .with_size(900.0, 640.0);
    h.frames(4);
    let shapes = h.frame_shapes();
    assert!(bounds.get().is_positive(), "the widget must be drawn");
    Ok((bounds.get(), shapes))
}

fn track_height(bounds: egui::Rect, shapes: &[egui::epaint::ClippedShape]) -> Option<f32> {
    shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Rect(rect)
                if bounds.contains(rect.rect.center())
                    && rect.rect.width() > rect.rect.height()
                    && rect.rect.height() < bounds.height()
                    && (rect.rect.center().y - bounds.center().y).abs() < 1.0 =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .max_by(|a, b| a.width().total_cmp(&b.width()))
        .map(|r| r.height())
}

#[test]
fn switch_mark_size_reaches_the_painted_track() -> fairing::Result<()> {
    for height in [18.0, 30.0] {
        let control = ControlSpec {
            mark_size: Span::fixed(Dim::du(height)).pinned(),
            ..ControlSpec::default()
        };
        let (bounds, shapes) = render(&control, &ComponentSpec::default(), Widget::Switch)?;
        let actual = track_height(bounds, &shapes);
        assert!(
            actual.is_some_and(|v| (v - height).abs() < 0.1),
            "switch track must follow the configured {height}, got {actual:?}"
        );
    }
    Ok(())
}

#[test]
fn slider_track_spec_reaches_painted_geometry() -> fairing::Result<()> {
    for thickness in [16.0, 28.0] {
        let control = ControlSpec {
            slider_track: Span::fixed(Dim::du(thickness)).pinned(),
            ..ControlSpec::default()
        };
        let (bounds, shapes) = render(&control, &ComponentSpec::default(), Widget::Slider)?;
        let actual = track_height(bounds, &shapes);
        assert!(
            actual.is_some_and(|v| (v - thickness).abs() < 0.1),
            "slider track must follow the configured {thickness}, got {actual:?}"
        );
    }
    Ok(())
}

#[test]
fn row_component_gap_moves_the_rendered_trailing_text() -> fairing::Result<()> {
    let mut positions = Vec::new();
    for gap in [8.0, 32.0] {
        let mut component = ComponentSpec::default();
        component.list_row[0] = Span::fixed(Dim::du(gap));
        let (bounds, shapes) = render(&ControlSpec::default(), &component, Widget::Row)?;
        let x = shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(text) if text.galley.text() == "rendered-value" => {
                Some(text.pos.x - bounds.left())
            }
            _ => None,
        });
        let Some(x) = x else {
            return Err(fairing::Error::Config("row text was not painted".into()));
        };
        positions.push(x);
    }
    assert!(
        positions
            .first()
            .zip(positions.get(1))
            .is_some_and(|(first, second)| (first - second - 24.0).abs() < 0.1),
        "the real trailing-text position must follow the component gap: {positions:?}"
    );
    Ok(())
}

#[test]
fn lock_ring_padding_reaches_the_painted_badge() -> fairing::Result<()> {
    for (pad, radius) in [(4.0, 10.0), (12.0, 14.0)] {
        let mut theme = fairing::theme::Theme::dark();
        theme.metrics.desktop_lock_size = 16.0;
        theme.metrics.desktop_lock_ring_pad = pad;
        let mut config = fairing::testing::access_config(&["viewer", "admin"], Some("top"));
        config.motion.reduce = true;
        let mut h = Harness::from_builder(move |ctx| {
            let mut shell = Shell::builder(config)
                .theme(theme)
                .services(fairing::Services::null())
                .build(ctx)?;
            shell.add(
                screen("locked", |_: &mut egui::Ui, _: &mut Cx<'_>| {})
                    .icon(fairing::icon::GAUGE)
                    .visibility(fairing::Visibility::Locked)
                    .desktop(),
            );
            Ok(shell)
        })?
        .with_size(900.0, 640.0);
        h.frames(3);
        let icon = h
            .shell
            .desktop()
            .icon_rect("locked")
            .ok_or_else(|| fairing::Error::Config("locked icon was not drawn".into()))?;
        let actual = h.frame_shapes().into_iter().find_map(|s| match s.shape {
            egui::Shape::Circle(circle) if circle.center.distance(icon.right_bottom()) < 0.1 => {
                Some(circle.radius)
            }
            _ => None,
        });
        assert!(
            actual.is_some_and(|v| (v - radius).abs() < 0.1),
            "lock badge must follow the configured ring padding {pad}, got {actual:?}"
        );
    }
    Ok(())
}

#[cfg(feature = "overlay")]
#[test]
fn shade_padding_moves_the_rendered_tile_row() -> fairing::Result<()> {
    let mut tops = Vec::new();
    for pad in [1.0, 2.0] {
        let mut theme = fairing::theme::Theme::dark();
        theme.metrics.corner_radius = 10.0;
        let mut spec = ComponentSpec::default();
        spec.shade.pad = pad;
        let mut config = single_level_access();
        config.motion.reduce = true;
        let mut h = Harness::from_builder(move |ctx| {
            Shell::builder(config)
                .theme(theme)
                .component_spec(spec)
                .services(fairing::Services::null())
                .build(ctx)
        })?
        .with_size(900.0, 640.0);
        h.frames(2);
        h.shell.launch(fairing::LaunchAction::OpenOverlay);
        h.run_for(0.5);
        let rect = h
            .shell
            .overlay()
            .tile_rect("tile.wifi")
            .ok_or_else(|| fairing::Error::Config("shade tile was not drawn".into()))?;
        tops.push(rect.top());
    }
    assert!(
        tops.first()
            .zip(tops.get(1))
            .is_some_and(|(first, second)| (second - first - 10.0).abs() < 0.1),
        "shade tile row must follow the configured panel padding: {tops:?}"
    );
    Ok(())
}
