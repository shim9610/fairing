//! **The rail's selection mark survives a size change** — `layout::list_item`.
//!
//! The mark is two shapes: a translucent pill behind the row, and an accent bar down its leading
//! edge. The bar used to be inset from the pill's top and bottom by the *whole* corner radius, on
//! the reasoning that a full-height bar's square ends stick out past the corners. That reasoning is
//! right and the arithmetic was wrong: `control_radius` is anchored to the text and the row height
//! to the finger, so the two are free to move apart, and as the radius approaches half the row the
//! bar's height goes to zero. It does not fail — it fades out, which is the kind of thing that
//! ships. These tests hold the bar to a visible share of the pill at row heights from a dense
//! desktop list to a gloved panel's.

use fairing::testing::{single_level_access, Harness};
use fairing::theme::MetricsSpec;
use fairing::unit::{Dim, Span};
use fairing::{icon, layout, screen, Cx, Shell};

/// The selected row's shapes, at a spec whose `row_height` is pinned to `du` and whose
/// `control_radius` is left on its text-anchored default.
///
/// Since a length carries its anchor in its type, that mix has to be written `pinned()`, so it can
/// no longer be fallen into — but it is still a thing somebody may deliberately do, and these are
/// the row heights it produces.
fn marks(row_du: f32) -> fairing::Result<Vec<egui::epaint::RectShape>> {
    let spec = MetricsSpec {
        row_height: Span::fixed(Dim::du(row_du)).pinned(),
        ..MetricsSpec::default()
    };
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .metrics_spec(spec)
            .build(ctx)?;
        shell.add(screen("rail", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "rail", |ui, cx| {
                layout::list_item(ui, cx, icon::HOME, "Home", true);
                layout::list_item(ui, cx, icon::ACTIVITY, "Run", false);
            });
        }));
        shell.launch(fairing::LaunchAction::open("rail"));
        Ok(shell)
    })?;
    h.frames(3);
    Ok(h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r),
            _ => None,
        })
        .collect())
}

/// The pill and the bar, found by shape rather than by colour.
///
/// The bar is the **narrowest** filled rect on the frame — nothing else the rail draws is a few du
/// wide — and the pill is the shortest filled rect that begins just to its right on the same rows.
/// Since the bar moved out to the row's gutter it sits *beside* the pill rather than inside it, so
/// "the smallest rect containing the bar" finds the pane's own backing and not the mark's neighbour.
fn pill_and_bar(row_du: f32) -> fairing::Result<(egui::Rect, egui::Rect)> {
    let shapes = marks(row_du)?;
    let bar = shapes
        .iter()
        .map(|r| r.rect)
        .filter(|r| r.width() > 0.0)
        .min_by(|a, b| a.width().total_cmp(&b.width()))
        .unwrap_or(egui::Rect::NOTHING);
    let pill = shapes
        .iter()
        .map(|r| r.rect)
        .filter(|r| {
            r.left() >= bar.right() - 0.5
                && r.left() < bar.right() + bar.width() * 4.0
                && r.width() > bar.width() * 4.0
                && (r.center().y - bar.center().y).abs() < r.height()
        })
        .min_by(|a, b| a.height().total_cmp(&b.height()))
        .unwrap_or(egui::Rect::NOTHING);
    Ok((pill, bar))
}

#[test]
fn the_accent_bar_keeps_its_height_as_the_row_shrinks() -> fairing::Result<()> {
    for row_du in [40.0_f32, 46.0, 56.0, 72.0, 96.0] {
        let (pill, bar) = pill_and_bar(row_du)?;
        let share = bar.height() / pill.height().max(1.0);
        assert!(
            share > 0.45,
            "at row_height = {row_du} du the selection bar is {share:.0}% of the pill \
             ({:.1} of {:.1} du) — the mark the rail is read by fades out as the row and the \
             corner radius move apart",
            bar.height(),
            pill.height(),
        );
    }
    Ok(())
}

/// **The bar is beside the pill, and no taller than it.**
///
/// It used to be drawn inside, where it had to dodge the corner arc — arithmetic that shrank the
/// mark to nothing on a short row and, even when it did not, left it a stub floating in the fill
/// rather than an edge marker. Outside there is no arc to dodge. What still has to hold
/// is that it is a mark *for* the pill: level with it, and not overrunning it.
#[test]
fn the_accent_bar_sits_in_the_gutter_beside_the_pill() -> fairing::Result<()> {
    for row_du in [40.0_f32, 46.0, 56.0, 72.0, 96.0] {
        let (pill, bar) = pill_and_bar(row_du)?;
        assert!(
            bar.right() <= pill.left() + 0.5,
            "at row_height = {row_du} du the bar is on the pill rather than beside it: \
             {bar:?} against {pill:?}"
        );
        assert!(
            bar.top() >= pill.top() - 0.5 && bar.bottom() <= pill.bottom() + 0.5,
            "at row_height = {row_du} du the bar is taller than the row it marks: {bar:?} \
             against {pill:?}"
        );
    }
    Ok(())
}

/// The leading icon's painted rect, at a `row_height` pinned in `du` while `icon_size` keeps its
/// finger-anchored default — the same mix, seen from the other side.
fn icon_box(row_du: f32) -> fairing::Result<(f32, f32)> {
    let spec = MetricsSpec {
        row_height: Span::fixed(Dim::du(row_du)).pinned(),
        ..MetricsSpec::default()
    };
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .metrics_spec(spec)
            .build(ctx)?;
        shell.add(screen("rail", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "rail", |ui, cx| {
                layout::list_item(ui, cx, icon::HOME, "Home", false);
            });
        }));
        shell.launch(fairing::LaunchAction::open("rail"));
        Ok(shell)
    })?;
    h.frames(3);
    // An icon is a mesh or a path, not a rect: take the tallest non-rect shape's bounds.
    let tallest = h
        .frame_shapes()
        .into_iter()
        .filter(|c| !matches!(c.shape, egui::Shape::Rect(_) | egui::Shape::Text(_)))
        .map(|c| c.shape.visual_bounding_rect())
        .filter(|r| r.is_finite() && r.height() > 0.0)
        .map(|r| r.height())
        .fold(0.0_f32, f32::max);
    Ok((tallest, row_du))
}

#[test]
fn the_leading_icon_fits_the_row_it_sits_in() -> fairing::Result<()> {
    for row_du in [40.0_f32, 46.0, 56.0, 72.0, 96.0] {
        let (icon_h, _) = icon_box(row_du)?;
        assert!(
            icon_h <= row_du,
            "at row_height = {row_du} du the leading icon paints {icon_h:.1} du tall — \
             `icon_size` is anchored to the finger and the row to whatever the integrator pinned, \
             so the glyph grows straight through the row that holds it"
        );
    }
    Ok(())
}
