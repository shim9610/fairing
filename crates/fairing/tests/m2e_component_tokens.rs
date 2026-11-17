//! Component-token regressions: geometry, style, metadata and validation.
//!
//! Public builder configurations are checked against response geometry, painted shapes,
//! rendered text and egui style. Value-model tests check validation separately.
//!
//! A knob that does not turn is worse than no knob.

#![cfg(feature = "mock")]

use fairing::testing::{single_level_access, Harness};
use fairing::theme::ComponentSpec;
use fairing::unit::{Dim, Span};
use fairing::widgets::{BigButton, ButtonKind};
use fairing::{screen, Cx, Shell};
use std::cell::Cell;
use std::rc::Rc;

/// The same, but able to override a [`MetricsSpec`] token too.
///
/// The control vocabulary moved the shared lengths — a row's content inset, the strokes, the marks
/// — out of `ComponentSpec`, where each belonged to one widget, and into `Metrics` and the control
/// tokens, where one value serves every control. A test that only reaches `ComponentSpec` can no
/// longer prove the path for those.
fn measure_with(
    spec: Option<&ComponentSpec>,
    metrics: Option<&fairing::theme::MetricsSpec>,
    draw: impl Fn(&mut egui::Ui, &mut Cx<'_>, &Cell<f32>) + 'static,
) -> Option<f32> {
    let spec = spec.copied();
    let metrics = metrics.cloned();
    let seen = Rc::new(Cell::new(f32::NAN));
    let sink = Rc::clone(&seen);
    let built = Harness::from_builder(move |ctx| {
        let mut builder =
            Shell::builder(single_level_access()).services(fairing::services::Services::null());
        if let Some(spec) = spec {
            builder = builder.component_spec(spec);
        }
        if let Some(metrics) = metrics.clone() {
            builder = builder.metrics_spec(metrics);
        }
        let mut shell = builder.build(ctx)?;
        shell.add(
            screen("w", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                draw(ui, cx, &sink);
            })
            .title("w"),
        );
        shell.launch(fairing::LaunchAction::open("w"));
        Ok(shell)
    });
    let mut h = built.ok()?.with_size(900.0, 640.0);
    h.frames(3);
    let v = seen.get();
    v.is_finite().then_some(v)
}

/// A one-`du` spec.
fn du(v: f32) -> Span {
    Span::fixed(Dim::du(v))
}

/// Changing `MetricsSpec::content_inset` changes button side padding.
/// The test pins both values so the expected width delta is independent of defaults.
#[test]
fn the_button_padding_reaches_the_widget() -> fairing::Result<()> {
    // With a short label the `min_size` floor bites and the width does not change however much the padding grows —
    // a length where the content decides the width is used.
    fn draw(ui: &mut egui::Ui, cx: &mut Cx<'_>, sink: &Cell<f32>) {
        let r = BigButton::new("Confirm and continue")
            .kind(ButtonKind::Primary)
            .show(ui, &mut cx.widgets());
        sink.set(r.response.rect.width());
    }
    // **Both ends are pinned, rather than one end and an assumption about the default.** The default
    // `content_inset` resolves with the size policy rather than necessarily using its 16 du
    // floor, so an expected delta written against 16 measured the policy
    // rather than the path. Two explicit `du` spans make the arithmetic depend on nothing.
    let narrow = fairing::theme::MetricsSpec {
        content_inset: du(16.0).pinned(),
        ..fairing::theme::MetricsSpec::default()
    };
    let wide = fairing::theme::MetricsSpec {
        content_inset: du(48.0).pinned(),
        ..fairing::theme::MetricsSpec::default()
    };

    let (Some(stock), Some(padded)) = (
        measure_with(None, Some(&narrow), draw),
        measure_with(None, Some(&wide), draw),
    ) else {
        return Err(fairing::Error::Config(
            "the button was not drawn".to_owned(),
        ));
    };
    // It has to widen by (48 − 16) × 2 = 64 du across.
    assert!(
        (padded - stock - 64.0).abs() < 1.0,
        "the button's padding did not reach: {stock} → {padded}"
    );
    Ok(())
}

/// A broken spec is caught as **a config error** — it does not quietly draw a track of thickness 0.
#[test]
fn a_broken_spec_is_rejected() {
    assert!(ComponentSpec::default().validate().is_ok());
    let bad = ComponentSpec {
        slider_label_gap: du(f32::NAN),
        ..ComponentSpec::default()
    };
    assert!(bad.validate().is_err());
}

/// **The indicator's hit band is a ratio token**. Today's default is `touch_target × 0.5` because the
/// dots sit right below the grid and taking a whole touch target would steal the taps of the last icon row — a
/// device with room in its grid has to be able to raise it to 1.0.
#[test]
fn the_page_indicator_hit_band_is_a_token() -> fairing::Result<()> {
    use fairing::theme::Theme;

    /// Three icons on a one-cell grid make three pages. It measures the first dot's tap Rect height.
    fn band(ratio: f32) -> Option<f32> {
        let mut cfg = single_level_access();
        cfg.desktop.columns = 1;
        cfg.desktop.rows = 1;
        let built = Harness::from_builder(move |ctx| {
            // Both sides inject a theme — injecting on one side alone changes whether `MetricsSpec` is resolved,
            // which changes `touch_target` itself, and then it is no longer a ratio being measured.
            let mut theme = Theme::dark();
            theme.metrics.page_indicator_hit_ratio = ratio;
            let mut shell = Shell::builder(cfg)
                .services(fairing::services::Services::null())
                .theme(theme)
                .build(ctx)?;
            for id in ["p0", "p1", "p2"] {
                shell.add(
                    screen(id, |_: &mut egui::Ui, _: &mut Cx<'_>| {})
                        .title(id)
                        .icon(fairing::icon::FOLDER)
                        .desktop(),
                );
            }
            Ok(shell)
        });
        let mut h = built.ok()?.with_size(640.0, 480.0);
        h.frames(3);
        h.shell.desktop().page_indicator_rect(0).map(|r| r.height())
    }

    let (Some(half), Some(full)) = (band(0.5), band(1.0)) else {
        return Err(fairing::Error::Config(
            "the indicator was not drawn (only one page?)".to_owned(),
        ));
    };
    // Against a `touch_target` of 48: 0.5 equals the indicator band's height (24) and 1.0 is 48.
    assert!(
        (full - half - 24.0).abs() < 1.0,
        "the hit ratio did not reach: {half} → {full}"
    );
    Ok(())
}

/// Check shade-multiple defaults, spec resolution and invalid-value rejection.
/// This does not open a shade or inspect the panel's geometry or paint.
#[test]
fn shade_ratios_resolve_and_reject_invalid_values() -> fairing::Result<()> {
    use fairing::theme::{ComponentSpec, ShadeMetrics};

    let base = ComponentSpec::default();
    // The default multiples are the values `PanelStyle` was using as they are.
    let d = ShadeMetrics::default();
    assert!((d.pad - 1.34).abs() < f32::EPSILON);
    assert!((d.card_pad - 1.17).abs() < f32::EPSILON);
    assert!((d.tile_puck - 0.8).abs() < f32::EPSILON);
    assert!((d.close - 0.72).abs() < f32::EPSILON);

    // The four finger-axis ones are **drawn sizes**, so they have to be below 1.0 to mean anything — the hit Rect
    // is a whole `touch_target` and only the drawing goes inside it.
    for (name, v) in [
        ("tile_puck", d.tile_puck),
        ("note_icon", d.note_icon),
        ("close", d.close),
        ("footer_button", d.footer_button),
    ] {
        assert!(v > 0.0 && v <= 1.0, "{name} goes past the hit Rect: {v}");
    }

    // Swapping the multiples has the spec carry them over as they are.
    let wide = ComponentSpec {
        shade: ShadeMetrics {
            pad: 2.5,
            ..ShadeMetrics::default()
        },
        ..base
    };
    assert!((wide.resolve(&fairing::unit::Scale::identity()).shade.pad - 2.5).abs() < f32::EPSILON);

    // A broken multiple is a config error.
    let bad = ComponentSpec {
        shade: ShadeMetrics {
            close: -1.0,
            ..ShadeMetrics::default()
        },
        ..base
    };
    let Err(err) = bad.validate() else {
        return Err(fairing::Error::Config(
            "a negative multiple was let through".to_owned(),
        ));
    };
    assert!(format!("{err}").contains("shade.close"), "{err}");
    Ok(())
}

/// **A text size changed at run time reaches egui**.
///
/// `Metrics::type_scale` was promoted to a token so the override ladder could reach the text size,
/// but the ladder had no run-time rung: `Theme::apply` ran once from the builder and again only
/// from inside the palette crossfade, and `Shell` handed the theme out as `&Theme`. So a size set
/// after start-up never reached `egui::Style::text_styles` — the one place it has to land.
///
/// On a device the text size is the physical question "is this readable from 70 cm", which is
/// answered on the machine, not at compile time.
#[test]
fn a_text_size_set_at_run_time_reaches_egui() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        Shell::builder(single_level_access())
            .services(fairing::services::Services::null())
            .build(ctx)
    })?;
    h.frames(2);

    // `Theme::apply` writes the style **for the current egui theme**, so it is read back the same way.
    let body_of = |h: &Harness| {
        let theme = if h.shell.theme().dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        h.ctx
            .style_of(theme)
            .text_styles
            .get(&egui::TextStyle::Body)
            .map(|f| f.size)
    };
    let before = body_of(&h);
    let built = h.shell.theme().metrics.type_scale.body;
    assert!(
        before.is_some_and(|got| (got - built).abs() < f32::EPSILON),
        "the builder's text size did not reach egui to begin with: {before:?} vs {built}"
    );

    // **The spec, not the resolved metrics** — with a spec in play the shell rebuilds
    // `theme.metrics` from it every frame, so a size written onto the theme is thrown away.
    let want = h.shell.theme().metrics.type_scale.body + 7.0;
    let mut spec = h.shell.metrics_spec();
    spec.type_scale[1] = fairing::unit::Span::fixed(fairing::unit::Dim::du(want)).pinned();
    h.shell.set_metrics_spec(spec);
    h.frames(2);
    assert!(
        (h.shell.theme().metrics.type_scale.body - want).abs() < f32::EPSILON,
        "the spec did not reach the resolved metrics: {}",
        h.shell.theme().metrics.type_scale.body
    );
    assert!(
        body_of(&h).is_some_and(|got| (got - want).abs() < f32::EPSILON),
        "the size changed at run time never reached egui's style: {:?} (was {before:?})",
        body_of(&h)
    );
    Ok(())
}
