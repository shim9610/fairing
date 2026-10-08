//! **A meter is read by where its pointer sits and what colour it is** — `Meter`.
//!
//! The pointer is the three-cornered path above the track; its fill is the verdict's ink. The
//! verdict is judged on the reported value the frame it arrives, while the pointer glides.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::{LampState, Limit, Meter};
use fairing::{screen, ColorRole, Cx, Shell};
use std::time::Duration;

/// The track's thickness in the bench: the pointer rises 0.6 of it.
const THICK: f32 = 16.0;

/// What the test hands the screen and what the screen reports back, in the app-state slot.
struct Bench {
    value: f32,
    beat: u64,
    stale_after: Option<Duration>,
    /// Written by the screen.
    state: LampState,
    stale: bool,
    on_surface: egui::Color32,
    warning: egui::Color32,
    danger: egui::Color32,
}

impl Bench {
    fn at(value: f32) -> Self {
        Self {
            value,
            beat: 1,
            stale_after: None,
            state: LampState::Unknown,
            stale: false,
            on_surface: egui::Color32::TRANSPARENT,
            warning: egui::Color32::TRANSPARENT,
            danger: egui::Color32::TRANSPARENT,
        }
    }
}

fn bench(bench: Bench) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("m", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let inks = (
                cx.theme.color(ColorRole::OnSurface),
                cx.theme.color(ColorRole::Warning),
                cx.theme.color(ColorRole::Danger),
            );
            let Some((value, beat, stale_after)) = cx
                .app_mut::<Bench>()
                .map(|b| (b.value, b.beat, b.stale_after))
            else {
                return;
            };
            let limits = [
                Limit::high(67.0, LampState::Warn),
                Limit::high(70.0, LampState::Fault),
            ];
            let mut meter = Meter::new(value, 40.0..=80.0)
                .normal(55.0..=65.0)
                .thickness(THICK)
                .limits(&limits)
                .readout("x");
            if let Some(after) = stale_after {
                meter = meter.stale_after(after).heartbeat(beat);
            }
            let reading = meter.show(ui, &mut cx.widgets());
            if let Some(b) = cx.app_mut::<Bench>() {
                (b.on_surface, b.warning, b.danger) = inks;
                b.state = reading.state;
                b.stale = reading.stale;
            }
        }));
        shell.launch(fairing::LaunchAction::open("m"));
        Ok(shell)
    })?
    .with_app(bench);
    h.frames(30);
    Ok(h)
}

fn set(h: &mut Harness, f: impl FnOnce(&mut Bench)) {
    if let Some(b) = h.app_mut::<Bench>() {
        f(b);
    }
}

fn state(h: &mut Harness) -> (LampState, bool) {
    h.app_mut::<Bench>()
        .map_or((LampState::Unknown, false), |b| (b.state, b.stale))
}

/// The pointer: the three-cornered path as tall as the pointer's rise. Its centre x and fill.
fn pointer(h: &mut Harness) -> Option<(f32, egui::Color32)> {
    let rise = THICK * 0.6;
    for c in h.frame_shapes() {
        let egui::Shape::Path(path) = c.shape else {
            continue;
        };
        if path.points.len() != 3 {
            continue;
        }
        let bounds = egui::Rect::from_points(&path.points);
        if (bounds.height() - rise).abs() < 0.5 {
            return Some((bounds.center().x, path.fill));
        }
    }
    None
}

/// **The pointer glides to the value and never leaves the track.**
#[test]
fn the_pointer_glides_to_the_value() -> fairing::Result<()> {
    let mut h = bench(Bench::at(45.0))?;
    let (x_low, _) = pointer(&mut h).unwrap_or((-1.0, egui::Color32::TRANSPARENT));
    assert!(x_low > 0.0, "no pointer on the frame");
    set(&mut h, |b| b.value = 75.0);
    h.frames(1);
    let (x_mid, _) = pointer(&mut h).unwrap_or((-1.0, egui::Color32::TRANSPARENT));
    h.frames(30);
    let (x_high, _) = pointer(&mut h).unwrap_or((-1.0, egui::Color32::TRANSPARENT));
    assert!(
        x_high > x_low + 50.0,
        "45 -> 75 moved the pointer from {x_low} to {x_high}"
    );
    assert!(
        x_mid > x_low && x_mid < x_high,
        "one frame after the change the pointer should be part way: {x_low} < {x_mid} < {x_high}"
    );
    Ok(())
}

/// **The verdict colours the pointer the frame the value arrives**, before the pointer gets
/// there — and the state comes back to the caller in a lamp's words.
#[test]
fn the_verdict_is_judged_on_the_reported_value_at_once() -> fairing::Result<()> {
    let mut h = bench(Bench::at(60.0))?;
    let (on_surface, warning, danger) = h.app_mut::<Bench>().map_or(
        (
            egui::Color32::TRANSPARENT,
            egui::Color32::TRANSPARENT,
            egui::Color32::TRANSPARENT,
        ),
        |b| (b.on_surface, b.warning, b.danger),
    );
    assert_eq!(state(&mut h).0, LampState::Ok);
    assert_eq!(
        pointer(&mut h).map(|p| p.1),
        Some(on_surface),
        "in the band: no colour"
    );

    set(&mut h, |b| b.value = 66.0);
    h.frames(30);
    assert_eq!(
        state(&mut h).0,
        LampState::Ok,
        "above the band but under the limit that guards it: a deviation, no verdict"
    );

    set(&mut h, |b| b.value = 68.0);
    h.frames(1);
    assert_eq!(state(&mut h).0, LampState::Warn);
    assert_eq!(
        pointer(&mut h).map(|p| p.1),
        Some(warning),
        "the warning ink should be on the pointer the very next frame"
    );

    set(&mut h, |b| b.value = 72.0);
    h.frames(30);
    assert_eq!(state(&mut h).0, LampState::Fault);
    assert_eq!(pointer(&mut h).map(|p| p.1), Some(danger));
    Ok(())
}

/// **A stale reading greys and stands still; the next beat restores it.**
#[test]
fn a_stale_reading_greys_and_the_next_beat_restores_it() -> fairing::Result<()> {
    let mut h = bench(Bench {
        stale_after: Some(Duration::from_millis(500)),
        ..Bench::at(60.0)
    })?;
    let on_surface = h
        .app_mut::<Bench>()
        .map_or(egui::Color32::TRANSPARENT, |b| b.on_surface);
    assert_eq!(pointer(&mut h).map(|p| p.1), Some(on_surface));
    assert!(!state(&mut h).1);
    h.frames(20);
    let (_, stale) = state(&mut h);
    assert!(
        stale,
        "0.83 s past a 0.5 s limit and the reading is not stale"
    );
    let fill = pointer(&mut h).map_or(egui::Color32::TRANSPARENT, |p| p.1);
    assert!(
        fill != on_surface && fill.a() < 255,
        "a stale pointer should be greyed, got {fill:?}"
    );
    set(&mut h, |b| b.beat += 1);
    h.frames(2);
    assert!(!state(&mut h).1, "a new beat did not clear the stale flag");
    assert_eq!(pointer(&mut h).map(|p| p.1), Some(on_surface));
    Ok(())
}
