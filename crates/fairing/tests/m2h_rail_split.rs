//! **The chrome is one L, not two regions with a rule between them** — `layout::rail_split`.
//!
//! Every other screen anatomy in the crate divides with a line, because with an arbitrary palette
//! `Surface` and `Background` can be close enough that colour alone separates nothing. That stops
//! being true when the rail is chrome rather than a region of the page: the top bar already fills
//! `Surface`, so an arm filling the same role continues it, and a line between them would cut the
//! shape in half at the point it is meant to turn the corner. What divides the page from the
//! chrome is the page's own panel, one step in.
//!
//! # Reading a role off the frame
//!
//! The roles cannot be read from `shell.theme()` and compared with what was painted. A `Pane`
//! fades everything drawn inside it so a screen can sit over a wallpaper, so `Surface` as the
//! status bar paints it — outside the pane — and `Surface` as it lands on the frame — inside one —
//! are different `Color32`s for the same role. So the screen paints a two-du **swatch** of each
//! role it is being held to, which goes through exactly the fade the arm does. "The arm is
//! `Surface`" is then something these can check rather than assert about.

use fairing::layout;
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, ColorRole, Cx, Shell};

/// A panel wide enough for `rail_width` to say yes.
const WIDE: egui::Vec2 = egui::vec2(1200.0, 700.0);
/// One too upright for it.
const UPRIGHT: egui::Vec2 = egui::vec2(600.0, 900.0);
/// The swatches' side.
const SWATCH: f32 = 2.0;

/// The filled rects on one `rail_split` frame, and `[Surface, SurfaceVariant]` as painted there.
fn frame(size: egui::Vec2) -> fairing::Result<(Vec<egui::epaint::RectShape>, [egui::Color32; 2])> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let at = ui.max_rect();
            for (i, role) in [ColorRole::Surface, ColorRole::SurfaceVariant]
                .into_iter()
                .enumerate()
            {
                #[expect(clippy::cast_precision_loss, reason = "two swatches: `i` is 0 or 1")]
                let dy = i as f32 * SWATCH;
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(at.right() - SWATCH, at.top() + dy),
                        egui::Vec2::splat(SWATCH),
                    ),
                    0.0,
                    cx.theme.color(role),
                );
            }
            let _ = layout::rail_split(
                ui,
                cx,
                |ui, cx| layout::note(ui, cx, "rail"),
                |ui, cx| layout::note(ui, cx, "page"),
            );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?;
    h.set_size(size.x, size.y);
    h.frames(3);
    let shapes: Vec<egui::epaint::RectShape> = h
        .frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r),
            _ => None,
        })
        .collect();
    // The swatches are the only squares that small, and they come out in the order painted.
    let swatch: Vec<egui::Color32> = shapes
        .iter()
        .filter(|r| r.rect.width() <= SWATCH + 0.5 && r.rect.height() <= SWATCH + 0.5)
        .map(|r| r.fill)
        .collect();
    let roles = [
        swatch
            .first()
            .copied()
            .unwrap_or(egui::Color32::TRANSPARENT),
        swatch.get(1).copied().unwrap_or(egui::Color32::TRANSPARENT),
    ];
    Ok((shapes, roles))
}

/// The arm and the page: the two columns that **touch**, which is what is structurally unique
/// about them. Picking "the leftmost tall rect" would find the pane's own backing instead.
fn columns(
    shapes: &[egui::epaint::RectShape],
) -> ((egui::Color32, egui::Rect), (egui::Color32, egui::Rect)) {
    let tallest = shapes
        .iter()
        .map(|r| r.rect.height())
        .fold(0.0_f32, f32::max);
    let tall: Vec<_> = shapes
        .iter()
        .filter(|r| r.rect.height() > tallest * 0.5 && r.rect.width() > SWATCH)
        .collect();
    let nothing = (egui::Color32::TRANSPARENT, egui::Rect::NOTHING);
    tall.iter()
        .flat_map(|a| tall.iter().map(move |b| (a, b)))
        .find(|(a, b)| {
            a.rect.left() < b.rect.left() && (a.rect.right() - b.rect.left()).abs() < 1.5
        })
        .map_or((nothing, nothing), |(a, b)| {
            ((a.fill, a.rect), (b.fill, b.rect))
        })
}

#[test]
fn the_arm_takes_the_bars_colour_and_the_page_steps_off_it() -> fairing::Result<()> {
    let (shapes, [surface, variant]) = frame(WIDE)?;
    let (arm, page) = columns(&shapes);
    assert_eq!(
        arm.0, surface,
        "the arm has to be the status bar's own role, or the two do not read as one L"
    );
    assert_eq!(
        page.0, variant,
        "the page's default panel is `SurfaceVariant`, a step in from the chrome"
    );
    assert_ne!(
        page.0, surface,
        "with the dividing line gone, the page stepping off the chrome is the only thing left \
         separating them"
    );
    Ok(())
}

/// **Nothing is drawn on the seam.** The arms meet edge to edge; a stroke there would be the rule
/// this layout exists to remove.
#[test]
fn no_line_is_drawn_where_the_arm_meets_the_page() -> fairing::Result<()> {
    let (shapes, _) = frame(WIDE)?;
    let (arm, page) = columns(&shapes);
    let seam = arm.1.right();
    assert!(
        seam.is_finite() && (seam - page.1.left()).abs() < 1.5,
        "the arm and the page have to touch: {:?} then {:?}",
        arm.1,
        page.1
    );
    let on_seam = shapes
        .iter()
        .filter(|r| r.rect.width() < 3.0 && r.rect.height() > arm.1.height() * 0.5)
        .filter(|r| (r.rect.center().x - seam).abs() < 2.0)
        .count();
    assert_eq!(
        on_seam, 0,
        "a hairline down the seam is exactly the rule `rail_split` replaces with a colour step"
    );
    Ok(())
}

#[test]
fn an_upright_panel_folds_the_rail_away_instead_of_cramping_both() -> fairing::Result<()> {
    let (shapes, [surface, _]) = frame(UPRIGHT)?;
    let tallest = shapes
        .iter()
        .map(|r| r.rect.height())
        .fold(0.0_f32, f32::max);
    let arms = shapes
        .iter()
        .filter(|r| {
            r.fill == surface && r.rect.height() > tallest * 0.5 && r.rect.width() < UPRIGHT.x * 0.5
        })
        .count();
    assert_eq!(
        arms, 0,
        "on a panel too upright to divide there is no arm at all — the page takes the screen, the \
         same fold `split_width` makes"
    );
    Ok(())
}

/// The arm's width on a frame where the pointer has already swiped across it.
fn folded_arm_width(collapsible: bool) -> fairing::Result<f32> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = layout::Rail::new().collapsible(collapsible).show(
                ui,
                cx,
                |ui, cx| layout::note(ui, cx, "rail"),
                |ui, cx| layout::note(ui, cx, "page"),
            );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?;
    h.set_size(WIDE.x, WIDE.y);
    h.frames(3);
    let before = columns(&filled(&mut h)).0 .1.width();
    // A press inside the arm, dragged left well past the threshold, then held: the gesture is read
    // off the pointer, so the press has to still be down when the frame is drawn.
    h.press(egui::pos2(before * 0.5, WIDE.y * 0.5));
    h.frames(1);
    h.move_to(egui::pos2(4.0, WIDE.y * 0.5));
    h.frames(30);
    Ok(columns(&filled(&mut h)).0 .1.width())
}

fn filled(h: &mut Harness) -> Vec<egui::epaint::RectShape> {
    h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r),
            _ => None,
        })
        .collect()
}

/// **A swipe folds a collapsible rail, and does nothing to one that is not.**
///
/// The second half is the point of the flag: folding is a choice about the product, and a rail
/// that folds when nobody asked for it is a support call rather than a preference.
#[test]
fn a_swipe_folds_the_arm_only_where_the_rail_says_it_may() -> fairing::Result<()> {
    let folds = folded_arm_width(true)?;
    let fixed = folded_arm_width(false)?;
    assert!(
        folds < fixed * 0.75,
        "a swipe across a `collapsible` rail has to fold it: it went to {folds} where a fixed one \
         stayed at {fixed}"
    );
    Ok(())
}
