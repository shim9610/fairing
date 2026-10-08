//! **The four selection marks draw four different things** — `layout::SelectMark`.
//!
//! The mark is what a rail is read by, and which one reads best is a property of the panel: a
//! console wants the pill and the bar, a dense desktop list wants the bar alone because a fill
//! behind one row of many reads as a stripe, and a rail that is already an accent panel wants
//! neither. The vocabulary only means anything if the four are distinguishable, so these count the
//! shapes each one adds over the same row drawn unselected.

use fairing::layout::{self, SelectMark};
use fairing::testing::{single_level_access, Harness};
use fairing::{icon, screen, Cx, Shell};

/// The filled rects one row paints, selected with `mark` (or not selected at all for `None`).
fn marks(mark: Option<SelectMark>) -> fairing::Result<Vec<egui::Rect>> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("rail", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "rail", |ui, cx| match mark {
                Some(mark) => {
                    let _ = layout::list_item_with(ui, cx, mark, icon::HOME, "Home", true);
                }
                None => {
                    let _ = layout::list_item(ui, cx, icon::HOME, "Home", false);
                }
            });
        }));
        shell.launch(fairing::LaunchAction::open("rail"));
        Ok(shell)
    })?;
    h.frames(3);
    Ok(h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r.rect),
            _ => None,
        })
        .collect())
}

/// What `mark` paints that an unselected row does not: the mark itself.
fn added(mark: SelectMark) -> fairing::Result<Vec<egui::Rect>> {
    let base = marks(None)?;
    Ok(marks(Some(mark))?
        .into_iter()
        .filter(|r| !base.iter().any(|b| b.min == r.min && b.max == r.max))
        .collect())
}

#[test]
fn each_mark_draws_what_its_name_says() -> fairing::Result<()> {
    let pill_and_bar = added(SelectMark::PillAndBar)?;
    let pill = added(SelectMark::Pill)?;
    let bar = added(SelectMark::Bar)?;
    let tint = added(SelectMark::Tint)?;

    assert!(
        tint.is_empty(),
        "`Tint` marks the row with colour on its icon and label and nothing else, so it must add \
         no shape at all: {tint:?}"
    );
    assert_eq!(
        pill.len(),
        1,
        "`Pill` is one shape - the fill behind the row: {pill:?}"
    );
    assert_eq!(
        bar.len(),
        1,
        "`Bar` is one shape - the mark down the leading edge: {bar:?}"
    );
    assert_eq!(
        pill_and_bar.len(),
        2,
        "`PillAndBar` is both of them: {pill_and_bar:?}"
    );

    let nothing = egui::Rect::NOTHING;
    let (pill_r, bar_r) = (
        pill.first().copied().unwrap_or(nothing),
        bar.first().copied().unwrap_or(nothing),
    );
    assert!(
        pill_r.width() > bar_r.width() * 4.0,
        "the pill spans the row and the bar is a few du of it, so these cannot be the same shape \
         under two names: pill {pill_r:?}, bar {bar_r:?}"
    );
    Ok(())
}

/// **The bar is at the same x whether or not a pill is drawn.**
///
/// The leading gutter is reserved by every mark, so a rail whose selection moves between a `Bar`
/// row and a `PillAndBar` row does not slide its column sideways as it goes. Sizing the
/// pill off whether it happens to have a bar beside it is the version that does.
#[test]
fn the_gutter_is_reserved_whether_or_not_a_bar_fills_it() -> fairing::Result<()> {
    let lone = added(SelectMark::Bar)?;
    let both = added(SelectMark::PillAndBar)?;
    let lone_bar = lone
        .iter()
        .map(egui::Rect::left)
        .fold(f32::INFINITY, f32::min);
    let paired_bar = both
        .iter()
        .map(egui::Rect::left)
        .fold(f32::INFINITY, f32::min);
    assert!(
        (lone_bar - paired_bar).abs() < 0.5,
        "a bar drawn alone starts at {lone_bar} and one drawn with a pill at {paired_bar} — the \
         gutter has to be the same either way or the column shifts as the selection moves"
    );
    let paired_pill = both
        .iter()
        .map(egui::Rect::left)
        .fold(f32::NEG_INFINITY, f32::max);
    assert!(
        paired_pill > paired_bar,
        "the pill begins after the gutter, not on it: pill at {paired_pill}, bar at {paired_bar}"
    );
    Ok(())
}
