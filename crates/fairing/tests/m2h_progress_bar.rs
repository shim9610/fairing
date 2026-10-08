//! **A bar is trusted only while it behaves like a clock, and says so when it stops** —
//! `ProgressBar`'s eased value, high-water mark, stop indicator, steps and stall.
//!
//! Everything is read off the frame: the fill is the accent-coloured rect at the bar's
//! thickness, the track is the `SurfaceVariant` one, the stop dot is the accent circle.

use fairing::testing::{single_level_access, Harness};
use fairing::widgets::ProgressBar;
use fairing::{screen, ColorRole, Cx, Shell};
use std::time::Duration;

/// The bar's thickness in the bench, so its rects can be told from every other one.
const THICK: f32 = 12.0;

/// What the test hands the screen, and what the screen reports back — the crate's app-state
/// slot, so nothing needs a lock.
struct Bench {
    value: f32,
    beat: u64,
    stop: bool,
    steps: u8,
    allow_rewind: bool,
    stale_after: Option<Duration>,
    /// Written by the screen: the accent and the track colour.
    accent: egui::Color32,
    track: egui::Color32,
}

impl Bench {
    fn at(value: f32) -> Self {
        Self {
            value,
            beat: 1,
            stop: false,
            steps: 0,
            allow_rewind: false,
            stale_after: None,
            accent: egui::Color32::TRANSPARENT,
            track: egui::Color32::TRANSPARENT,
        }
    }
}

/// The rects at the bar's thickness, with their fill: the track pieces and the fill pieces.
fn bar_rects(h: &mut Harness) -> Vec<(egui::Rect, egui::Color32)> {
    let mut out = Vec::new();
    for c in h.frame_shapes() {
        let egui::Shape::Rect(r) = c.shape else {
            continue;
        };
        if (r.rect.height() - THICK).abs() < 0.75 && r.fill.a() > 0 {
            out.push((r.rect, r.fill));
        }
    }
    out
}

/// The accent circles on the frame — the stop dot, when there is one.
fn accent_circles(h: &mut Harness, accent: egui::Color32) -> Vec<egui::Pos2> {
    let mut out = Vec::new();
    for c in h.frame_shapes() {
        if let egui::Shape::Circle(circle) = c.shape {
            if circle.fill == accent {
                out.push(circle.center);
            }
        }
    }
    out
}

/// The widest rect in `colour`, or an empty rect.
fn widest(rects: &[(egui::Rect, egui::Color32)], colour: egui::Color32) -> egui::Rect {
    rects
        .iter()
        .filter(|(_, c)| *c == colour)
        .map(|(r, _)| *r)
        .fold(egui::Rect::NOTHING, |best, r| {
            if r.width() > best.width() {
                r
            } else {
                best
            }
        })
}

/// The accent and the track colour, as the screen saw them.
fn colours(h: &mut Harness) -> (egui::Color32, egui::Color32) {
    h.app_mut::<Bench>().map_or(
        (egui::Color32::TRANSPARENT, egui::Color32::TRANSPARENT),
        |b| (b.accent, b.track),
    )
}

/// How far along the track the fill reaches, 0 to 1, on the classic look.
fn shown_fraction(h: &mut Harness) -> f32 {
    let (accent, track) = colours(h);
    let rects = bar_rects(h);
    let fill = widest(&rects, accent);
    let track = widest(&rects, track);
    if track.width() <= 0.0 {
        return -1.0;
    }
    if fill.width() <= 0.0 {
        return 0.0;
    }
    fill.width() / track.width()
}

/// A harness showing one bar the test can drive between frames.
fn bench(bench: Bench) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("r", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let accent = cx.theme.color(ColorRole::Primary);
            let track = cx.theme.color(ColorRole::SurfaceVariant);
            let Some(b) = cx.app_mut::<Bench>() else {
                return;
            };
            b.accent = accent;
            b.track = track;
            let mut bar = ProgressBar::determinate(b.value)
                .thickness(THICK)
                .stop_indicator(b.stop)
                .steps(b.steps)
                .allow_rewind(b.allow_rewind);
            if let Some(after) = b.stale_after {
                bar = bar.stale_after(after).heartbeat(b.beat);
            }
            let _ = bar.show(ui, &mut cx.widgets());
        }));
        shell.launch(fairing::LaunchAction::open("r"));
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

#[test]
fn the_bar_eases_and_holds_its_high_water_mark_unless_a_rewind_is_meant() -> fairing::Result<()> {
    let mut held = bench(Bench::at(0.6))?;
    let before = shown_fraction(&mut held);
    assert!(
        (0.55..=0.65).contains(&before),
        "handed 0.6, the bar shows {before}"
    );
    set(&mut held, |b| b.value = 0.2);
    held.frames(30);
    let after = shown_fraction(&mut held);
    assert!(
        after >= before - 0.02,
        "handed 0.2 after 0.6, a bar that forbids rewinds went from {before} to {after}"
    );

    let mut free = bench(Bench {
        allow_rewind: true,
        ..Bench::at(0.6)
    })?;
    set(&mut free, |b| b.value = 0.2);
    free.frames(1);
    let mid = shown_fraction(&mut free);
    assert!(
        mid > 0.25 && mid < 0.6,
        "one frame after 0.6 -> 0.2 the bar should be part way, not at {mid}"
    );
    free.frames(30);
    let settled = shown_fraction(&mut free);
    assert!(
        (0.15..=0.25).contains(&settled),
        "with rewinds allowed the bar has to follow the value down: {settled}"
    );
    Ok(())
}

/// **The stop indicator keeps a gap at the head and a dot at the end, until the head arrives.**
#[test]
fn the_stop_indicator_keeps_a_gap_and_a_dot_until_the_head_arrives() -> fairing::Result<()> {
    let mut h = bench(Bench {
        stop: true,
        ..Bench::at(0.4)
    })?;
    let (accent, track_colour) = colours(&mut h);
    let rects = bar_rects(&mut h);
    let fill = widest(&rects, accent);
    let track = widest(&rects, track_colour);
    assert!(
        fill.width() > 0.0 && track.width() > 0.0,
        "no bar on the frame"
    );
    assert!(
        track.min.x >= fill.max.x + THICK - 0.5,
        "the track starts at {} and the head ends at {}: no gap of one thickness",
        track.min.x,
        fill.max.x
    );
    let dots = accent_circles(&mut h, accent);
    assert_eq!(dots.len(), 1, "one stop dot at 40 %, found {}", dots.len());
    let dot = dots.first().copied().unwrap_or(egui::pos2(-1.0, -1.0));
    assert!(
        dot.x > track.max.x - THICK && dot.x <= track.max.x,
        "the dot is at x={} and the track ends at {}",
        dot.x,
        track.max.x
    );

    set(&mut h, |b| b.value = 1.0);
    h.frames(60);
    assert!(
        accent_circles(&mut h, accent).is_empty(),
        "a full bar still shows the stop dot"
    );
    let rects = bar_rects(&mut h);
    assert!(
        widest(&rects, track_colour).width() <= 0.0,
        "a full bar still shows a piece of track"
    );
    Ok(())
}

/// **Steps cut the track into countable cells** — 3/7 is three full cells of seven, all alike.
#[test]
fn steps_cut_the_track_into_countable_cells() -> fairing::Result<()> {
    let mut h = bench(Bench {
        steps: 7,
        ..Bench::at(3.0 / 7.0)
    })?;
    h.frames(30);
    let (accent, track_colour) = colours(&mut h);
    let rects = bar_rects(&mut h);
    let cells: Vec<egui::Rect> = rects
        .iter()
        .filter(|(_, c)| *c == track_colour)
        .map(|(r, _)| *r)
        .collect();
    let filled: Vec<egui::Rect> = rects
        .iter()
        .filter(|(_, c)| *c == accent)
        .map(|(r, _)| *r)
        .collect();
    assert_eq!(cells.len(), 7, "seven cells, found {}", cells.len());
    assert_eq!(
        filled.len(),
        3,
        "three filled cells at 3/7, found {}",
        filled.len()
    );
    let cell_w = cells.first().map_or(0.0, egui::Rect::width);
    for r in &filled {
        assert!(
            (r.width() - cell_w).abs() < 1.5,
            "a filled cell is {} wide and a cell is {cell_w}",
            r.width()
        );
    }
    Ok(())
}

/// **Silence past the limit is reported, and the next beat clears it.** The value never moves
/// here; only the heartbeat says whether the worker is alive.
#[test]
fn a_bar_reports_a_stall_and_the_next_beat_clears_it() -> fairing::Result<()> {
    let mut h = bench(Bench {
        stale_after: Some(Duration::from_millis(500)),
        ..Bench::at(0.6)
    })?;
    let (accent, track) = colours(&mut h);
    let fills = |h: &mut Harness| -> Vec<egui::Color32> {
        bar_rects(h)
            .iter()
            .filter(|(_, c)| *c != track)
            .map(|(_, c)| *c)
            .collect()
    };
    // 30 frames in: half a second has not passed, the fill is the plain accent.
    assert_eq!(fills(&mut h), vec![accent], "stalled before the limit");
    h.frames(20);
    // 50 frames = 0.83 s of silence: the fill is breathing, so it is not the plain accent.
    let stale = fills(&mut h);
    assert!(
        !stale.is_empty() && stale.iter().all(|c| *c != accent),
        "0.83 s past a 0.5 s limit and the fill is still the plain accent: {stale:?}"
    );
    set(&mut h, |b| b.beat += 1);
    h.frames(2);
    assert_eq!(
        fills(&mut h),
        vec![accent],
        "a new beat did not clear the stall"
    );
    Ok(())
}
