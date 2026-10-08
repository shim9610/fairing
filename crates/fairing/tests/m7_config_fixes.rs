//! Regression tests for the configuration and sizing fixes made before 0.1.0: float keys that are
//! not numbers, the sub-springs' ranges, the `frac_*` reference at ppp 2, the type scale's anchor,
//! palette hex, the automatic grid, `dock_band`, the hero's dots and a resize mid page spring.

#![cfg(feature = "mock")]

use fairing::testing::{single_level_access, Harness};
use fairing::theme::MetricsSpec;
use fairing::unit::{Dim, ScalePolicy, Span};
use fairing::{layout, screen, ColorRole, Cx, Services, Shell, ShellConfig};

fn null_services() -> Services {
    Services::builder()
        .clock(fairing::services::null::NullClock)
        .build()
}

fn run_until_idle(h: &mut Harness, max_frames: usize) -> bool {
    for _ in 0..max_frames {
        h.frame();
        if !h.shell.is_animating() {
            return true;
        }
    }
    false
}

// ── [motion.page.spring] / [motion.shade.spring] ───────────────────────────────────────────

/// The shade's and the page's springs are refused out of range like the base spring (k > 0, c >= 0).
#[test]
fn sub_springs_are_validated_like_the_base_spring() {
    let cases = [
        "[motion.page.spring]\nk = 0\n",
        "[motion.page.spring]\nk = -100\n",
        "[motion.page.spring]\nc = -1\n",
        "[motion.shade.spring]\nk = 0\n",
        "[motion.shade.spring]\nk = -100\n",
        "[motion.shade.spring]\nc = -1\n",
    ];
    let accepted: Vec<&str> = cases
        .iter()
        .copied()
        .filter(|toml| ShellConfig::from_toml(toml).is_ok())
        .collect();
    assert!(
        accepted.is_empty(),
        "out-of-range sub-springs passed validation: {accepted:#?}"
    );
}

/// A page spring of k = 0, which would never settle, does not get past the shell's builder.
#[test]
fn zero_page_spring_is_refused_at_build() {
    let mut cfg = single_level_access();
    cfg.motion.page.spring.k = 0.0;
    assert!(
        Harness::new(cfg, null_services()).is_err(),
        "a page spring that never settles was accepted"
    );
}

// ── NaN / inf ───────────────────────────────────────────────────────────────────────────────

/// `nan` and `inf`, valid TOML floats, are refused for every float key, ranged or not.
#[test]
fn nan_and_inf_are_refused_for_float_keys() {
    let cases = [
        "[motion.spring]\nk = nan\n",
        "[motion.spring]\nk = inf\n",
        "[motion.spring]\nc = nan\n",
        "[motion]\nfling_px_s = nan\n",
        "[motion]\nslop_px = nan\n",
        "[motion.shade]\nrubber = nan\n",
        "[motion.page]\nfling_px_s = nan\n",
        "[motion.page]\nrubber = nan\n",
        "[osk]\nmin_key_px = nan\n",
        "[osk]\nmin_key_px = inf\n",
        "[motion.shade.spring]\nk = inf\n",
        "[motion.page.spring]\nc = nan\n",
        "[motion.shade]\nrubber_max_px = nan\n",
        "[desktop]\nrail_width = nan\n",
        "[desktop.abyss]\nveil_top = inf\n",
        "[gesture]\nemergency_corner_px = nan\n",
        "[overlay]\ncard_relief = nan\n",
    ];
    let accepted: Vec<&str> = cases
        .iter()
        .copied()
        .filter(|toml| ShellConfig::from_toml(toml).is_ok())
        .collect();
    assert!(
        accepted.is_empty(),
        "non-finite values passed validation: {accepted:#?}"
    );
}

// ── Scale::root_du ──────────────────────────────────────────────────────────────────────────

/// At ppp = 2, `Scale::root_du` is the root rect in du, not divided by the ppp a second time.
#[test]
fn root_du_is_the_root_rect_in_du() -> fairing::Result<()> {
    // 1024 x 600 px at 2 / MM_PER_DU px per mm → ppp = 2.
    let px_per_mm = 2.0 / fairing::unit::MM_PER_DU;
    let (w_mm, h_mm) = (1024.0 / px_per_mm, 600.0 / px_per_mm);
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(single_level_access())
            .scale_policy(ScalePolicy::default().with_allow_env(false))
            .physical_mm(w_mm, h_mm)
            .build(ctx)
    })?;
    h.frames(4);
    let scale = *h.shell.scale();
    assert!(
        (scale.pixels_per_point - 2.0).abs() < 1e-2,
        "setup: ppp = {}",
        scale.pixels_per_point
    );
    // The root in du as egui sees it this frame.
    let l = h.shell.layout();
    let root_w = l.content.width();
    assert!(
        (root_w - 512.0).abs() < 1.0,
        "setup: the root is 512 du wide at ppp 2, got {root_w}"
    );
    assert!(
        (scale.root_du.x - root_w).abs() < 1.0,
        "root_du.x = {} but the root rect is {root_w} du wide",
        scale.root_du.x
    );
    Ok(())
}

/// At ppp = 2, a `frac_w(0.25)` token is a quarter of the real 512 du root.
#[test]
fn frac_tokens_resolve_against_the_real_root() -> fairing::Result<()> {
    let px_per_mm = 2.0 / fairing::unit::MM_PER_DU;
    let (w_mm, h_mm) = (1024.0 / px_per_mm, 600.0 / px_per_mm);
    let spec = MetricsSpec {
        icon_cell: Span::fixed(Dim::frac_w(0.25)).pinned(),
        ..MetricsSpec::default()
    };
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(single_level_access())
            .scale_policy(ScalePolicy::default().with_allow_env(false))
            .physical_mm(w_mm, h_mm)
            .metrics_spec(spec)
            .build(ctx)
    })?;
    h.frames(4);
    let cell = h.shell.theme().metrics.icon_cell;
    assert!(
        (cell - 128.0).abs() < 1.0,
        "frac_w(0.25) of a 512 du root should be 128 du, got {cell}"
    );
    Ok(())
}

// ── The type scale's anchor ─────────────────────────────────────────────────────────────────

/// The type scale follows the viewing distance, not the finger (guide 04 §1.2): a gloved policy
/// grows the touch target and leaves the text alone, a longer viewing distance grows the text.
#[test]
fn type_scale_tracks_the_viewing_distance_not_the_finger() -> fairing::Result<()> {
    let body = |policy: ScalePolicy| -> fairing::Result<(f32, f32)> {
        let mut h = Harness::from_builder(move |ctx| {
            Shell::builder(single_level_access())
                .scale_policy(policy.with_allow_env(false))
                .physical_mm(152.4, 89.3)
                .build(ctx)
        })?;
        h.frames(3);
        let m = h.shell.theme().metrics;
        Ok((m.type_scale.body, m.touch_target))
    };
    let (bare_body, bare_touch) = body(ScalePolicy::bare())?;
    let (gloved_body, gloved_touch) = body(ScalePolicy::gloved())?;
    assert!(
        gloved_touch > bare_touch * 1.2,
        "the finger moves the touch target ({bare_touch} vs {gloved_touch})"
    );
    assert!(
        (gloved_body - bare_body).abs() < 0.5,
        "the finger moved the text: bare body = {bare_body}, gloved body = {gloved_body}"
    );
    let far = ScalePolicy {
        viewing_distance_mm: 1000.0,
        ..ScalePolicy::gloved()
    };
    let (far_body, _) = body(far)?;
    assert!(
        far_body > gloved_body * 1.5,
        "twice the viewing distance, and the body text is {far_body} against {gloved_body}"
    );
    Ok(())
}

// ── Palette hex ─────────────────────────────────────────────────────────────────────────────

/// A palette colour with a `+` sign in it is not `#RRGGBB` / `#RRGGBBAA` and is refused.
#[test]
fn palette_hex_with_a_sign_is_refused() {
    for value in ["#+12345", "#+1234567"] {
        let toml = format!("[theme.palette]\nprimary = \"{value}\"\n");
        let built = ShellConfig::from_toml(&toml).and_then(|c| c.build_theme());
        assert!(
            built.is_err(),
            "`{value}` is not #RRGGBB / #RRGGBBAA and was accepted"
        );
    }
}

// ── The automatic grid ──────────────────────────────────────────────────────────────────────

/// The automatic grid settles on one page when one page without the indicator holds every icon,
/// even though the desktop started out with two.
#[test]
fn auto_grid_does_not_keep_a_needless_second_page() -> fairing::Result<()> {
    // First measure the cell, the bars and the indicator on this panel.
    let probe = {
        let mut cfg = single_level_access();
        cfg.desktop.columns = 0;
        cfg.desktop.rows = 0;
        let mut h = Harness::new(cfg, null_services())?;
        h.frames(3);
        let m = h.shell.theme().metrics;
        let l = h.shell.layout();
        (
            m.icon_cell,
            m.page_indicator_height,
            600.0 - l.content.height(),
        )
    };
    let (cell, indicator, chrome) = probe;
    // Content height = 2 cells + half the indicator: two rows without it, one row with it.
    let content_h = 2.0f32.mul_add(cell, indicator * 0.5);
    let height = content_h + chrome;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let columns = (1024.0 / cell).floor() as usize;
    let n = columns * 2;
    assert!(n > 12, "setup: need more than the 4x3 seed holds (n = {n})");

    let mut cfg = single_level_access();
    cfg.desktop.columns = 0;
    cfg.desktop.rows = 0;
    let mut h = Harness::new(cfg, null_services())?.with_size(1024.0, height);
    for i in 0..n {
        h.shell.add(
            screen(format!("s{i}"), |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("x");
            })
            .title("x")
            .icon(fairing::icon::FOLDER)
            .desktop(),
        );
    }
    h.frames(6);
    let content = h.shell.layout().content.height();
    assert!(
        (content - content_h).abs() < 1.0,
        "setup: content {content} vs planned {content_h}"
    );
    let pages = h.shell.desktop().pages().len();
    assert_eq!(
        pages, 1,
        "{n} icons fit one page of {columns} x 2 on a {content}-du-tall content area, \
         but the auto grid settled on {pages} pages"
    );
    Ok(())
}

// ── dock_band ───────────────────────────────────────────────────────────────────────────────

/// `[desktop] dock_band = nan` is refused at load.
#[test]
fn dock_band_nan_is_refused() {
    let cfg = ShellConfig::from_toml(
        "[access]\nlevels = [\"only\"]\n[desktop]\ndock = [\"d\"]\ndock_band = nan\n",
    );
    assert!(cfg.is_err(), "dock_band = nan was accepted");
}

// ── The hero's dots ─────────────────────────────────────────────────────────────────────────

/// (circles drawn, circles lit in `Primary`) for a hero showing page `current` of `count`.
fn hero_dots(current: usize, count: usize) -> fairing::Result<(usize, usize)> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("hero", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::hero(ui, cx, 16.0 / 9.0, (current, count), |_, _, _| ());
        }));
        shell.launch(fairing::LaunchAction::open("hero"));
        Ok(shell)
    })?;
    // Past the screen's fade-in, so `Primary` is drawn at full alpha.
    h.frames(90);
    let primary = h.shell.theme().color(ColorRole::Primary);
    let shapes = h.frame_shapes();
    let (mut dots, mut lit) = (0, 0);
    for clipped in &shapes {
        let mut stack = vec![&clipped.shape];
        while let Some(shape) = stack.pop() {
            match shape {
                egui::Shape::Vec(v) => stack.extend(v.iter()),
                egui::Shape::Circle(c) => {
                    dots += 1;
                    if c.fill == primary {
                        lit += 1;
                    }
                }
                _ => {}
            }
        }
    }
    Ok((dots, lit))
}

/// Past twelve pages the hero still lights the live page's dot, out of at most twelve.
#[test]
fn hero_lights_a_dot_on_a_late_page() -> fairing::Result<()> {
    let (_, lit_early) = hero_dots(2, 20)?;
    assert_eq!(lit_early, 1, "page 3 of 20 lights one dot");
    for current in [14, 19] {
        let (dots, lit) = hero_dots(current, 20)?;
        assert_eq!(
            lit,
            1,
            "page {} of 20: {dots} circles drawn, {lit} lit in Primary",
            current + 1
        );
    }
    Ok(())
}

// ── A resize mid page spring ────────────────────────────────────────────────────────────────

/// A resize while a page spring runs still lands the desktop on a page, not between two.
#[test]
fn resize_mid_page_spring_still_lands_on_a_page() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.desktop.columns = 1;
    cfg.desktop.rows = 1;
    let mut h = Harness::new(cfg, null_services())?;
    for id in ["p0", "p1", "p2"] {
        h.shell.add(
            screen(id, |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("page");
            })
            .title(id)
            .icon(fairing::icon::FOLDER)
            .desktop(),
        );
    }
    h.frames(3);
    let dot = h
        .shell
        .desktop()
        .page_indicator_rect(1)
        .ok_or_else(|| fairing::Error::Config("no indicator".into()))?;
    h.tap(dot.center());
    // A few frames into the spring, the panel rotates.
    h.frames(4);
    let mid = h.shell.desktop().page_pos();
    assert!(mid > 0.05 && mid < 0.95, "setup: mid-spring, pos = {mid}");
    h.set_size(600.0, 1024.0);
    let idle = run_until_idle(&mut h, 600);
    let pos = h.shell.desktop().page_pos();
    #[allow(clippy::cast_precision_loss)]
    let page = h.shell.desktop().page() as f32;
    assert!(idle, "setup: it comes to rest");
    assert!(
        (pos - page).abs() < 1e-3,
        "at rest the desktop sits at pos {pos} while page() = {page}"
    );
    Ok(())
}
