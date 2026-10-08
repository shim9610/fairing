//! **The three container kinds draw three different things** — `layout::Container`.
//!
//! The vocabulary only carries meaning if the three are distinguishable, and the point of naming
//! them was that a screen picks one per purpose. Before this every container was a filled card and
//! the built-in settings screens were a stack of them — the "big grey boxes repeating, so nothing
//! tells you which element matters" the design review named. Nothing pinned the fix, so a `Deco`
//! default drifting back to a fill would have silently restored it.
//!
//! # How these tests read the frame
//!
//! By **difference**, not by classifying shapes absolutely. The status bar, the dock and the page's
//! own background are painted the same way whichever kind the screen used, so the container is
//! exactly what one kind paints and another does not. `Divided` is the natural baseline: it draws
//! no box, so everything above it is the box under test.

use fairing::layout::{self, Container, Deco};
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, Shell};

/// One screen drawing a single group of the given kind, run until it is settled.
fn shell_drawing(kind: Container) -> fairing::Result<Harness> {
    Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("box", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "box", |ui, cx| {
                layout::group_with(ui, cx, Deco::new().container(kind), |ui, cx| {
                    layout::note(ui, cx, "body");
                });
            });
        }));
        shell.launch(fairing::LaunchAction::open("box"));
        Ok(shell)
    })
    .map(|mut h| {
        h.frames(3);
        h
    })
}

/// A painted rect, **rounded to whole points** so two frames of the same drawing compare equal.
///
/// The coordinates stay `f32` rather than being cast: they are already whole after `round`, so
/// equality is exact, and a cast would only add a lint to silence.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Box {
    min: egui::Pos2,
    max: egui::Pos2,
    filled: bool,
    stroked: bool,
}

impl Box {
    fn width(self) -> f32 {
        self.max.x - self.min.x
    }
}

fn boxes(kind: Container) -> fairing::Result<Vec<Box>> {
    let mut h = shell_drawing(kind)?;
    Ok(h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) => Some(Box {
                min: r.rect.min.round(),
                max: r.rect.max.round(),
                filled: r.fill.a() > 0,
                stroked: r.stroke.width > 0.0 && r.stroke.color.a() > 0,
            }),
            _ => None,
        })
        .collect())
}

/// What `a` paints **visibly** and `b` does not — the container, when `b` is `Divided`.
///
/// egui emits rects that draw nothing (no fill, no stroke) as it lays a frame out, and because
/// `Divided` is full-bleed those land at different coordinates than `Filled`'s do. They are not
/// what anyone sees, so they are not what these tests are about.
fn only_in(a: &[Box], b: &[Box]) -> Vec<Box> {
    a.iter()
        .filter(|x| x.filled || x.stroked)
        .filter(|x| !b.contains(x))
        .copied()
        .collect()
}

#[test]
fn filled_has_a_body_and_outlined_has_only_an_edge() -> fairing::Result<()> {
    let base = boxes(Container::Divided)?;
    let filled = only_in(&boxes(Container::Filled)?, &base);
    let outlined = only_in(&boxes(Container::Outlined)?, &base);

    assert!(
        filled.iter().any(|b| b.filled),
        "a `Filled` container has to have a body: {filled:?}"
    );
    assert!(
        !outlined.iter().any(|b| b.filled),
        "an `Outlined` container must not be filled — its whole job is to leave the weight to what \
         is inside it: {outlined:?}"
    );
    assert!(
        outlined.iter().any(|b| b.stroked),
        "an `Outlined` container is identified by its edge, so it has to draw one: {outlined:?}"
    );
    Ok(())
}

/// **`Divided` draws no box at all** — it adds nothing the other kinds do not also paint.
///
/// This is also what pins "nothing unfilled is raised". On a dark palette height is a rim drawn
/// inside the silhouette, and a rim around a container with no fill is just a box. Measured
/// when the container kinds were first rendered: with the rim left on, every divider-only group
/// in the settings screens came out boxed, which is exactly what an extra rect here would mean.
#[test]
fn divided_adds_no_box_and_is_never_raised() -> fairing::Result<()> {
    let extra = only_in(&boxes(Container::Divided)?, &boxes(Container::Filled)?);
    assert!(
        extra.is_empty(),
        "a `Divided` group painted {} rect(s) of its own — it is meant to draw no box, and a \
         stroked one means the elevation rim is back: {extra:?}",
        extra.len()
    );
    Ok(())
}

/// **The rule is full-bleed** — it reaches past where a card's body would stop.
///
/// A rule that stops short of the screen's edge reads as the bottom of a card that forgot to draw
/// the rest of itself, so this compares it against the filled card rather than only asserting it
/// exists.
#[test]
fn the_divider_runs_wider_than_a_card() -> fairing::Result<()> {
    let card = only_in(&boxes(Container::Filled)?, &boxes(Container::Divided)?)
        .into_iter()
        .filter(|b| b.filled)
        .map(Box::width)
        // No card body at all is a failure of this test's premise, and the sentinel says so
        // through the same assert rather than through a second kind of error.
        .max_by(f32::total_cmp)
        .unwrap_or(f32::INFINITY);

    let mut h = shell_drawing(Container::Divided)?;
    let rule = h
        .frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::LineSegment { points: [a, b], .. }
                if (a.y - b.y).abs() < 0.5 && (a.x - b.x).abs() > 1.0 =>
            {
                Some((a.x - b.x).abs())
            }
            _ => None,
        })
        .max_by(f32::total_cmp)
        .unwrap_or(0.0);

    assert!(
        rule > card,
        "the rule has to run wider than a card's body, or it reads as a card's missing bottom: \
         rule {rule:.1} vs card {card:.1} (a rule of 0.0 means none was drawn at all)"
    );
    Ok(())
}
