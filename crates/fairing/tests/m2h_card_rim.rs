//! **A card's rim is drawn on the card**, not on the place the card was given: with
//! `screen_inset` between the two, the rim stood clear of the fill on both sides and hugged it
//! top and bottom — a line and a surface that did not agree.

use fairing::testing::{single_level_access, Harness};
use fairing::{layout, screen, Cx, Shell};

#[test]
fn the_rim_hugs_the_fill_on_every_side() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            layout::page(ui, cx, "c", |ui, cx| {
                layout::group(ui, cx, |ui, _cx| {
                    ui.label("inside the card");
                });
            });
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?;
    h.frames(20);
    // The label, the smallest filled rect round it (the card), and the smallest stroked rect
    // round it (the rim).
    let mut label = egui::Rect::NOTHING;
    let mut fills = Vec::new();
    let mut strokes = Vec::new();
    for c in h.frame_shapes() {
        match c.shape {
            egui::Shape::Text(t) if t.galley.job.text == "inside the card" => {
                label = t.galley.rect.translate(t.pos.to_vec2());
            }
            egui::Shape::Rect(r) => {
                if r.fill.a() > 0 {
                    fills.push(r.rect);
                } else if r.stroke.width > 0.0 {
                    strokes.push(r.rect);
                }
            }
            _ => {}
        }
    }
    assert!(label.width() > 0.0, "the label is not on the frame");
    let smallest = |rects: &[egui::Rect]| {
        rects
            .iter()
            .filter(|r| r.contains_rect(label))
            .copied()
            .min_by(|a, b| a.area().total_cmp(&b.area()))
    };
    let (Some(card), Some(rim)) = (smallest(&fills), smallest(&strokes)) else {
        return Err(fairing::Error::Runner(format!(
            "no card ({:?}) or rim ({:?}) round the label",
            smallest(&fills),
            smallest(&strokes)
        )));
    };
    for (side, a, b) in [
        ("left", card.min.x, rim.min.x),
        ("right", card.max.x, rim.max.x),
        ("top", card.min.y, rim.min.y),
        ("bottom", card.max.y, rim.max.y),
    ] {
        assert!(
            (a - b).abs() < 1.0,
            "the rim's {side} edge is at {b} and the card's at {a}"
        );
    }
    Ok(())
}
