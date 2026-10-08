//! **A progress ring is trusted only while it behaves like a clock** — `ProgressRing`.
//!
//! Two things the ring promises about the value it *shows*, as opposed to the value it is handed:
//! it eases towards each reported value rather than jumping, and it does not go backwards unless
//! the caller says a rewind is meant. Both are read off the frame: the filled band is a mesh, and
//! how far round it reaches is the value on screen.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::ProgressRing;
use fairing::{screen, ColorRole, Cx, Shell};

/// What the test hands the screen, and what the screen reports back — the crate's app-state
/// slot, so no lock is needed to share it.
struct Bench {
    /// The value the ring is handed each frame.
    value: f32,
    /// The theme's accent, written by the screen so the test can find the fill mesh.
    accent: egui::Color32,
}

/// How far round the filled band reaches, as a fraction of a turn from twelve o'clock, read off
/// the fill mesh's vertices in the accent colour.
fn shown_fraction(h: &mut Harness, accent: egui::Color32) -> f32 {
    let mut best = 0.0_f32;
    for c in h.frame_shapes() {
        let egui::Shape::Mesh(mesh) = c.shape else {
            continue;
        };
        let centre = mesh.calc_bounds().center();
        // The head cap is a disc of the full accent; its centre is the furthest point round.
        // Angle from twelve o'clock, clockwise, as a fraction of a turn.
        for v in mesh.vertices.iter().filter(|v| v.color == accent) {
            let d = v.pos - centre;
            let angle = d.y.atan2(d.x) + std::f32::consts::FRAC_PI_2;
            let frac = angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
            best = best.max(frac);
        }
    }
    best
}

/// A harness showing one ring whose value the test can change between frames.
fn ring(allow_rewind: bool) -> fairing::Result<(Harness, egui::Color32)> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("r", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let accent = cx.theme.color(ColorRole::Primary);
            let value = cx.app_mut::<Bench>().map_or(0.0, |bench| {
                bench.accent = accent;
                bench.value
            });
            let _ = ProgressRing::determinate(value)
                .diameter(160.0)
                .allow_rewind(allow_rewind)
                .show(ui, &mut cx.widgets());
        }));
        shell.launch(fairing::LaunchAction::open("r"));
        Ok(shell)
    })?
    .with_app(Bench {
        value: 0.6,
        accent: egui::Color32::TRANSPARENT,
    });
    h.frames(30);
    let accent = h
        .app_mut::<Bench>()
        .map_or(egui::Color32::TRANSPARENT, |bench| bench.accent);
    Ok((h, accent))
}

/// Hand the ring a new value for the frames that follow.
fn set_value(h: &mut Harness, value: f32) {
    if let Some(bench) = h.app_mut::<Bench>() {
        bench.value = value;
    }
}

#[test]
fn the_ring_holds_its_high_water_mark_unless_a_rewind_is_meant() -> fairing::Result<()> {
    let (mut held, accent) = ring(false)?;
    let before = shown_fraction(&mut held, accent);
    set_value(&mut held, 0.2);
    held.frames(30);
    let after = shown_fraction(&mut held, accent);
    assert!(
        (0.5..=0.7).contains(&before),
        "the ring was handed 0.6 and shows {before}"
    );
    assert!(
        after >= before - 0.02,
        "handed 0.2 after 0.6, a ring that forbids rewinds went from {before} to {after}"
    );

    let (mut free, accent) = ring(true)?;
    let before = shown_fraction(&mut free, accent);
    set_value(&mut free, 0.2);
    free.frames(30);
    let after = shown_fraction(&mut free, accent);
    assert!(
        after < before - 0.2,
        "with rewinds allowed the ring has to follow the value down: {before} -> {after}"
    );
    Ok(())
}

/// **The shown value eases rather than jumps.** One frame after a change it is somewhere between
/// the old value and the new one.
#[test]
fn a_change_is_eased_over_frames_not_snapped() -> fairing::Result<()> {
    let (mut h, accent) = ring(true)?;
    let start = shown_fraction(&mut h, accent);
    set_value(&mut h, 0.0);
    h.frames(1);
    let mid = shown_fraction(&mut h, accent);
    assert!(
        mid > 0.1 && mid < start,
        "one frame after 0.6 -> 0.0 the ring should be part way, not at {mid} (from {start})"
    );
    Ok(())
}

/// **The centre text stays inside the hole**, on a ring too small for it at its natural size:
/// each line is shrunk to the chord where it sits, so the word under the number cannot run
/// into the band. Flat style, so the largest mesh on the frame is the ring's own outline. (A ring
/// smaller still drops the word rather than smear it — below 5 px a line is left out.)
#[test]
fn the_centre_text_stays_inside_the_hole_of_a_small_ring() -> fairing::Result<()> {
    use fairing::widgets::RingStyle;
    const DIAMETER: f32 = 96.0;
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("r", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = ProgressRing::determinate(1.0)
                .diameter(DIAMETER)
                .style(RingStyle::Flat)
                .value_text("100%")
                .label("Working")
                .show(ui, &mut cx.widgets());
        }));
        shell.launch(fairing::LaunchAction::open("r"));
        Ok(shell)
    })?;
    h.frames(20);
    // The ring: the largest mesh on the frame. Its hole ends one band width in from its edge.
    let mut ring = egui::Rect::NOTHING;
    let mut texts: Vec<egui::Rect> = Vec::new();
    for c in h.frame_shapes() {
        match c.shape {
            egui::Shape::Mesh(mesh) => {
                let b = mesh.calc_bounds();
                if b.width() > ring.width() {
                    ring = b;
                }
            }
            egui::Shape::Text(t) => texts.push(t.galley.rect.translate(t.pos.to_vec2())),
            _ => {}
        }
    }
    // The mesh is the ring plus its one-pixel anti-aliasing skirt on each side.
    assert!(
        (ring.width() - DIAMETER).abs() < 3.0,
        "the largest mesh is not the ring: {ring:?}"
    );
    let centre = ring.center();
    let outer_r = DIAMETER * 0.5;
    let inner_r = outer_r - DIAMETER * 0.12;
    let inside: Vec<egui::Rect> = texts
        .into_iter()
        .filter(|r| r.center().distance(centre) < outer_r)
        .collect();
    assert_eq!(
        inside.len(),
        2,
        "a number and a word in the ring, found {}",
        inside.len()
    );
    for r in inside {
        for corner in [
            r.left_top(),
            r.right_top(),
            r.left_bottom(),
            r.right_bottom(),
        ] {
            let d = corner.distance(centre);
            assert!(
                d <= inner_r + 1.0,
                "a text corner at {d:.1} from the centre, the hole ends at {inner_r:.1}: {r:?}"
            );
        }
    }
    Ok(())
}
