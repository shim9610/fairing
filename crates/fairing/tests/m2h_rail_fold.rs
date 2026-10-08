//! **A rail folds in one motion, as far as it is told, from wherever it is told** — `layout::Rail`.
//!
//! Folded to icons, the rail used to keep the bar beside the live icon: a line with nothing beside
//! it. Now the bar melts into a disc behind the icon as the arm narrows. Two options came with it:
//! `Fold::Away` folds the arm off the screen entirely, and `FoldGesture::Anywhere` reads the fold
//! from a flick across the page as well as a drag across the arm — now the default, with
//! `FoldGesture::Arm` the way back.

use fairing::layout::{self, Fold, FoldGesture, Rail};
use fairing::testing::{single_level_access, Harness};
use fairing::widgets::{BigButton, TouchSlider};
use fairing::{icon, screen, Cx, Shell};

/// A panel wide enough for a rail.
const WIDE: egui::Vec2 = egui::vec2(1200.0, 700.0);

/// The rail's entries.
const ENTRIES: [&str; 3] = ["Home", "Run", "Results"];

/// A harness whose one screen is `rail`, with "Home" selected.
fn harness(rail: Rail) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = rail.show(
                ui,
                cx,
                |ui, cx| {
                    for (i, name) in ENTRIES.iter().enumerate() {
                        let _ = layout::list_item(ui, cx, icon::HOME, name, i == 0);
                    }
                },
                |ui, cx| layout::note(ui, cx, "page"),
            );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?;
    h.set_size(WIDE.x, WIDE.y);
    // Past the screen's own opening transition, so the rows are measured where they rest.
    h.run_for(1.0);
    Ok(h)
}

/// The filled rects on the last frame.
fn filled(h: &mut Harness) -> Vec<egui::epaint::RectShape> {
    h.frame_shapes()
        .into_iter()
        .filter_map(|c| match c.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some(r),
            _ => None,
        })
        .collect()
}

/// The arm: the leftmost tall, narrow column - the pane's backing and the page are tall too, but
/// they span more than half the screen.
fn arm(shapes: &[egui::epaint::RectShape]) -> Option<egui::Rect> {
    shapes
        .iter()
        .map(|r| r.rect)
        .filter(|r| r.height() > WIDE.y * 0.5 && r.width() > 1.0 && r.width() < WIDE.x * 0.5)
        .min_by(|a, b| a.left().total_cmp(&b.left()))
}

/// The marks: the accent-coloured rects no taller than a row — the pill, the bar, the disc.
fn marks(h: &mut Harness) -> Vec<egui::epaint::RectShape> {
    let row = h
        .shell
        .theme()
        .metrics
        .row_height
        .max(h.shell.theme().metrics.touch_target);
    filled(h)
        .into_iter()
        .filter(|r| {
            r.rect.height() <= row * 1.5 && r.rect.width() > 1.0 && r.rect.width() < WIDE.x * 0.5
        })
        .collect()
}

/// A leftward flick across the page, fast enough to be one.
fn flick_page_left(h: &mut Harness) {
    h.drag(
        egui::pos2(WIDE.x * 0.7, WIDE.y * 0.5),
        egui::pos2(WIDE.x * 0.4, WIDE.y * 0.5),
        3,
    );
    h.run_for(1.0);
}

#[test]
fn a_flick_across_the_page_folds_the_rail_only_where_it_is_read_there() -> fairing::Result<()> {
    let mut anywhere = harness(
        Rail::new()
            .collapsible(true)
            .fold_gesture(FoldGesture::Anywhere),
    )?;
    let mut arm_only = harness(Rail::new().collapsible(true).fold_gesture(FoldGesture::Arm))?;
    let open = arm(&filled(&mut anywhere)).map_or(0.0, |r| r.width());
    flick_page_left(&mut anywhere);
    flick_page_left(&mut arm_only);
    let folded = arm(&filled(&mut anywhere)).map_or(0.0, |r| r.width());
    let kept = arm(&filled(&mut arm_only)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "a flick across the page has to fold a rail that reads its gesture anywhere: {open} -> {folded}"
    );
    assert!(
        (kept - open).abs() < 1.0,
        "and leave one that reads it on the arm alone where it was: {open} -> {kept}"
    );
    Ok(())
}

#[test]
fn a_rail_can_fold_away_entirely() -> fairing::Result<()> {
    let mut h = harness(Rail::new().collapsible(true).fold_to(Fold::Away))?;
    let Some(open_arm) = arm(&filled(&mut h)) else {
        return Err(fairing::Error::Config("open, there is an arm".to_owned()));
    };
    Rail::new().set_folded(&h.ctx, true);
    h.run_for(1.0);
    let shapes = filled(&mut h);
    assert!(
        arm(&shapes).is_none(),
        "folded away, no arm is left: {:?}",
        arm(&shapes)
    );
    // The page panel starts where the arm used to: nothing of the chrome shows beside it.
    let page_left = shapes
        .iter()
        .filter(|r| r.rect.height() > WIDE.y * 0.5 && r.rect.width() > WIDE.x * 0.5)
        .map(|r| r.rect.left())
        .fold(f32::NEG_INFINITY, f32::max);
    // The band is painted out to the glass; the page keeps the screen's own inset from it.
    let inset = h.shell.theme().metrics.screen_inset;
    assert!(
        page_left >= open_arm.left() - 0.5 && page_left <= open_arm.left() + inset + 0.5,
        "the page takes the whole screen once the arm is gone; it starts at {page_left}, the band \
         began at {} and the screen inset is {inset}",
        open_arm.left()
    );
    // And the grip at the left edge brings it back.
    let y = open_arm.center().y;
    h.press(egui::pos2(open_arm.left() + 6.0, y));
    h.frames(1);
    h.move_to(egui::pos2(open_arm.left() + 120.0, y));
    h.frames(2);
    h.release(egui::pos2(open_arm.left() + 120.0, y));
    h.run_for(1.0);
    assert!(
        arm(&filled(&mut h)).is_some(),
        "a drag from the left edge opens a rail that folded away"
    );
    Ok(())
}

#[test]
fn the_bar_melts_into_a_disc_behind_the_live_icon() -> fairing::Result<()> {
    let mut h = harness(Rail::new().collapsible(true))?;
    let bar_w = h.shell.theme().control.accent_bar;
    // Open: the pill (wide) and the bar (one accent bar wide), as `m2h_select_mark` pins.
    let open = marks(&mut h);
    let bar = open
        .iter()
        .find(|r| (r.rect.width() - bar_w).abs() < 0.5)
        .cloned();
    assert!(bar.is_some(), "open, the bar is there: {open:?}");
    let pill_alpha = open
        .iter()
        .filter(|r| r.rect.width() > bar_w * 4.0)
        .map(|r| r.fill.a())
        .max()
        .unwrap_or(0);

    // Folding: a frame partway through has the melting mark - wider than the bar, narrower than a
    // disc, and no longer solid.
    Rail::new().set_folded(&h.ctx, true);
    h.frames(4);
    let mid = marks(&mut h);
    let melting = mid
        .iter()
        .find(|r| r.rect.width() > bar_w + 0.5 && r.fill.a() < 255)
        .cloned();
    assert!(
        melting.is_some(),
        "partway through the fold the bar is already wider and softer: {mid:?}"
    );

    // Folded: one mark, a disc - as wide as it is tall, rounded all the way, sitting on the arm's
    // centre line - a step darker than the open pill was, and no bar anywhere.
    h.run_for(1.0);
    let shapes = filled(&mut h);
    let arm = arm(&shapes).unwrap_or(egui::Rect::NOTHING);
    let folded = marks(&mut h);
    let [disc] = folded.as_slice() else {
        return Err(fairing::Error::Config(format!(
            "folded, exactly one mark remains; found {folded:?}"
        )));
    };
    assert!(
        (disc.rect.width() - disc.rect.height()).abs() < 1.0,
        "the mark is a disc: {:?}",
        disc.rect
    );
    assert!(
        (disc.rect.center().x - arm.center().x).abs() < 1.0,
        "centred on the arm ({}) not in the gutter: {}",
        arm.center().x,
        disc.rect.center().x
    );
    assert!(
        f32::from(disc.corner_radius.nw) >= disc.rect.width() * 0.5 - 1.0,
        "rounded all the way: radius {} for width {}",
        disc.corner_radius.nw,
        disc.rect.width()
    );
    assert!(
        disc.fill.a() > pill_alpha && disc.fill.a() < 255,
        "a step darker than the pill ({pill_alpha}) and still a tint: {}",
        disc.fill.a()
    );
    Ok(())
}

#[test]
fn opening_puts_the_bar_back_exactly_where_it_was() -> fairing::Result<()> {
    let mut h = harness(Rail::new().collapsible(true))?;
    let before = marks(&mut h);
    Rail::new().set_folded(&h.ctx, true);
    h.run_for(1.0);
    Rail::new().set_folded(&h.ctx, false);
    h.run_for(1.0);
    let after = marks(&mut h);
    assert_eq!(
        before.len(),
        after.len(),
        "the same marks: {before:?} vs {after:?}"
    );
    for (a, b) in before.iter().zip(&after) {
        assert!(
            (a.rect.min - b.rect.min).length() < 0.5 && (a.rect.max - b.rect.max).length() < 0.5,
            "the same places: {:?} vs {:?}",
            a.rect,
            b.rect
        );
        assert_eq!(a.fill, b.fill, "the same ink");
    }
    Ok(())
}

/// A drag across the page that stops before it lets go — a mouse, or a careful finger.
fn drag_page_left_and_stop(h: &mut Harness) {
    let y = WIDE.y * 0.5;
    h.press(egui::pos2(WIDE.x * 0.75, y));
    h.frames(1);
    for i in 1..=20 {
        #[expect(clippy::cast_precision_loss, reason = "twenty steps")]
        let t = i as f32 / 20.0;
        h.move_to(egui::pos2(WIDE.x * (0.75 - 0.35 * t), y));
        h.frames(1);
    }
    // Still for a while: whatever speed the drag had is gone by the time it is released.
    h.frames(15);
    h.release(egui::pos2(WIDE.x * 0.4, y));
    h.run_for(1.0);
}

#[test]
fn a_slow_drag_across_the_page_folds_the_rail_too() -> fairing::Result<()> {
    let mut h = harness(
        Rail::new()
            .collapsible(true)
            .fold_gesture(FoldGesture::Anywhere),
    )?;
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    drag_page_left_and_stop(&mut h);
    let folded = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "a hand that stops before it lets go has no speed to read, and the distance has to do: \
         {open} -> {folded}"
    );
    Ok(())
}

/// The page's slider: its value, and where it was drawn.
struct Page {
    value: f32,
    slider: egui::Rect,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            value: 50.0,
            slider: egui::Rect::NOTHING,
        }
    }
}

#[test]
fn a_drag_a_slider_owns_never_folds_the_rail() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = Rail::new()
                .collapsible(true)
                .fold_gesture(FoldGesture::Anywhere)
                .show(
                    ui,
                    cx,
                    |ui, cx| {
                        let _ = layout::list_item(ui, cx, icon::HOME, "Home", true);
                    },
                    |ui, cx| {
                        cx.with_app::<Page, _>(|p, cx| {
                            p.slider = TouchSlider::new(&mut p.value, 0.0..=100.0)
                                .show(ui, &mut cx.widgets())
                                .rect;
                        });
                    },
                );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?
    .with_app(Page::default());
    h.set_size(WIDE.x, WIDE.y);
    h.run_for(1.0);
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    let Some((before, slider)) = h.app_mut::<Page>().map(|p| (p.value, p.slider)) else {
        return Err(fairing::Error::Config("the page state is there".to_owned()));
    };
    // A fast, long drag along the slider - both of the page rules would fire on it.
    let y = slider.center().y;
    h.drag(
        egui::pos2(slider.center().x + slider.width() * 0.2, y),
        egui::pos2(slider.center().x - slider.width() * 0.2, y),
        3,
    );
    h.run_for(1.0);
    let after = h.app_mut::<Page>().map_or(before, |p| p.value);
    assert!(
        (after - before).abs() > 1.0,
        "the drag has to have been the slider's - its value went {before} -> {after}"
    );
    let kept = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        (kept - open).abs() < 1.0,
        "and a drag a control owns leaves the rail alone: {open} -> {kept}"
    );
    Ok(())
}

/// **A short drag on the page folds the rail as it crosses the threshold**, mouse or finger: the
/// rule the arm has always had, everywhere. The first build waited for the release and asked for
/// speed or a long distance, and a mouse that stops before it lets go never met either.
#[test]
fn a_short_drag_on_the_page_folds_the_rail_at_once() -> fairing::Result<()> {
    let mut h = harness(
        Rail::new()
            .collapsible(true)
            .fold_gesture(FoldGesture::Anywhere),
    )?;
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    let y = WIDE.y * 0.5;
    h.press(egui::pos2(WIDE.x * 0.7, y));
    h.frames(1);
    // Forty du, slowly, and held: no release yet.
    for i in 1..=8 {
        #[expect(clippy::cast_precision_loss, reason = "eight steps")]
        let t = i as f32 / 8.0;
        h.move_to(egui::pos2(WIDE.x * 0.7 - 40.0 * t, y));
        h.frames(1);
    }
    h.run_for(1.0);
    let folded = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "forty du across the page, finger still down, has to have folded the rail: {open} -> \
         {folded}"
    );
    h.release(egui::pos2(WIDE.x * 0.7 - 40.0, y));
    h.run_for(0.5);
    Ok(())
}

/// **A folded icon's disc is never cut by the arm's inset.** The disc is centred in the chrome
/// band, which runs from the glass; a rail's rows sit in a scroll area whose clip starts a screen
/// inset in from it, and at the crate's default glyph size the disc reaches into that inset. The
/// rail is shaped like the console's - an action bar over a page - which is where it showed.
#[test]
fn a_folded_disc_is_whole_in_a_rail_of_scrolling_rows() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = Rail::new().collapsible(true).show(
                ui,
                cx,
                |ui, cx| {
                    layout::action_bar_with(
                        ui,
                        cx,
                        1.4,
                        layout::Deco::new().container(layout::Container::Divided),
                        |ui, cx| {
                            layout::page(ui, cx, "rail", |ui, cx| {
                                for (i, name) in ENTRIES.iter().enumerate() {
                                    let _ = layout::list_item(ui, cx, icon::HOME, name, i == 1);
                                }
                            });
                        },
                        |ui, cx| {
                            let _ = layout::list_item(ui, cx, icon::HOME, "Power off", false);
                        },
                    );
                },
                |ui, cx| layout::note(ui, cx, "page"),
            );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?;
    h.set_size(WIDE.x, WIDE.y);
    h.run_for(1.0);
    Rail::new().set_folded(&h.ctx, true);
    h.run_for(1.0);
    let shapes = filled(&mut h);
    let band = arm(&shapes).unwrap_or(egui::Rect::NOTHING);
    let row = h
        .shell
        .theme()
        .metrics
        .row_height
        .max(h.shell.theme().metrics.touch_target);
    // The disc: the square-ish mark no taller than a row, with the clip it was drawn under.
    let disc = h.frame_shapes().into_iter().find_map(|c| match c.shape {
        egui::Shape::Rect(r)
            if r.fill.a() > 0
                && r.rect.height() <= row * 1.5
                && r.rect.width() > 1.0
                && (r.rect.width() - r.rect.height()).abs() < 1.0 =>
        {
            Some((r.rect, c.clip_rect))
        }
        _ => None,
    });
    let Some((disc, clip)) = disc else {
        return Err(fairing::Error::Config("no disc was drawn".to_owned()));
    };
    assert!(
        band.contains_rect(disc),
        "the disc {disc:?} lies in the band {band:?}"
    );
    assert!(
        clip.contains_rect(disc),
        "the disc {disc:?} is cut by the clip {clip:?} it was drawn under"
    );
    assert!(
        (disc.center().x - band.center().x).abs() < 1.0,
        "and it is centred in the band: {} vs {}",
        disc.center().x,
        band.center().x
    );
    Ok(())
}

/// The page's button: where it was drawn, and how often it was pressed.
struct Button {
    rect: egui::Rect,
    clicks: u32,
}

impl Default for Button {
    fn default() -> Self {
        Self {
            rect: egui::Rect::NOTHING,
            clicks: 0,
        }
    }
}

/// **A swipe that begins on a button folds the rail.** A button senses drags too — its
/// long-press ring has to know the finger moved — and the first build took any dragged widget no
/// taller than two rows for a control that owned its drag, so a swipe from a button never folded
/// the rail. Now only a control that follows the finger says so (`drag::claim`), and a
/// button does not.
#[test]
fn a_swipe_that_begins_on_a_button_folds_the_rail() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = Rail::new()
                .collapsible(true)
                .fold_gesture(FoldGesture::Anywhere)
                .show(
                    ui,
                    cx,
                    |ui, cx| {
                        let _ = layout::list_item(ui, cx, icon::HOME, "Home", true);
                    },
                    |ui, cx| {
                        cx.with_app::<Button, _>(|b, cx| {
                            let shown = BigButton::new("Start run").show(ui, &mut cx.widgets());
                            b.rect = shown.response.rect;
                            if shown.response.clicked() {
                                b.clicks += 1;
                            }
                        });
                    },
                );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?
    .with_app(Button::default());
    h.set_size(WIDE.x, WIDE.y);
    h.run_for(1.0);
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    let Some(button) = h.app_mut::<Button>().map(|b| b.rect) else {
        return Err(fairing::Error::Config("the page state is there".to_owned()));
    };
    assert!(button.is_positive(), "the button was drawn: {button:?}");
    // From the middle of the button, sixty du to the left, slowly, and held.
    let y = button.center().y;
    h.press(button.center());
    h.frames(1);
    for i in 1..=8 {
        #[expect(clippy::cast_precision_loss, reason = "eight steps")]
        let t = i as f32 / 8.0;
        h.move_to(egui::pos2(button.center().x - 60.0 * t, y));
        h.frames(1);
    }
    h.run_for(1.0);
    let folded = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "a swipe that began on a button has to fold the rail: {open} -> {folded}"
    );
    h.release(egui::pos2(button.center().x - 60.0, y));
    h.run_for(0.5);
    let clicks = h.app_mut::<Button>().map_or(0, |b| b.clicks);
    assert_eq!(clicks, 0, "and a swipe is not a press of the button");
    Ok(())
}

/// One step of a finger, the way egui-winit reports a touch: the touch event, then the pointer it
/// emulates from it — a press on `Start`, a move on `Move`, and a release followed by a
/// `PointerGone` on `End`.
fn finger(h: &mut Harness, phase: egui::TouchPhase, pos: egui::Pos2) {
    h.push_event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(0),
        id: egui::TouchId(0),
        phase,
        pos,
        force: None,
    });
    match phase {
        egui::TouchPhase::Start => h.press(pos),
        egui::TouchPhase::Move => h.move_to(pos),
        egui::TouchPhase::End => h.release(pos),
        egui::TouchPhase::Cancel => h.push_event(egui::Event::PointerGone),
    }
}

/// A finger swipe across the page: down at `from`, `dx` to one side over six frames, and up.
fn finger_swipe(h: &mut Harness, from: egui::Pos2, dx: f32) {
    finger(h, egui::TouchPhase::Start, from);
    h.frames(1);
    for step in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0] {
        finger(
            h,
            egui::TouchPhase::Move,
            egui::pos2(from.x + dx * step / 6.0, from.y),
        );
        h.frames(1);
    }
    finger(h, egui::TouchPhase::End, egui::pos2(from.x + dx, from.y));
    h.run_for(1.0);
}

/// **A finger does what a mouse does.** The console this was reported from is a touch kiosk, and
/// egui-winit reports a touch as the touch event plus the pointer it emulates from it. The rail
/// reads that pointer, so a finger across the page folds the rail and a finger back opens it.
#[test]
fn a_finger_swipe_across_the_page_folds_the_rail_and_back_opens_it() -> fairing::Result<()> {
    let mut h = harness(
        Rail::new()
            .collapsible(true)
            .fold_gesture(FoldGesture::Anywhere),
    )?;
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    let y = WIDE.y * 0.5;
    finger_swipe(&mut h, egui::pos2(WIDE.x * 0.7, y), -90.0);
    let folded = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "a finger swipe across the page has to fold the rail: {open} -> {folded}"
    );
    finger_swipe(&mut h, egui::pos2(WIDE.x * 0.5, y), 90.0);
    let back = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        (back - open).abs() < 1.0,
        "and a finger swipe the other way has to open it again: {open} -> {back}"
    );
    Ok(())
}

/// **A flick that lands whole in one frame still counts.** On a slow panel a quick finger is
/// pressed, moved and lifted between two repaints, so the whole swipe arrives in one frame's
/// events; egui's `press_origin` is set and cleared inside that one pass and is never seen. The
/// rail reads the press off the frame's events as well.
#[test]
fn a_flick_that_lands_in_one_frame_still_folds_the_rail() -> fairing::Result<()> {
    let mut h = harness(
        Rail::new()
            .collapsible(true)
            .fold_gesture(FoldGesture::Anywhere),
    )?;
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    let y = WIDE.y * 0.5;
    let from = egui::pos2(WIDE.x * 0.7, y);
    finger(&mut h, egui::TouchPhase::Start, from);
    for step in [30.0, 60.0, 90.0] {
        finger(&mut h, egui::TouchPhase::Move, egui::pos2(from.x - step, y));
    }
    finger(&mut h, egui::TouchPhase::End, egui::pos2(from.x - 120.0, y));
    h.frames(1);
    h.run_for(1.0);
    let folded = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "a flick pressed, moved and lifted inside one frame has to fold the rail: {open} -> \
         {folded}"
    );
    Ok(())
}

/// The page's own pad: where it was drawn, and how far it has been dragged sideways.
struct Pad {
    region: egui::Rect,
    dragged_x: f32,
}

impl Default for Pad {
    fn default() -> Self {
        Self {
            region: egui::Rect::NOTHING,
            dragged_x: 0.0,
        }
    }
}

/// **A region that keeps its drags is left alone by the page swipe**. An integrator's
/// own object — a canvas, a chart, a pad drawn straight onto the page — reads the pointer itself
/// and has no response to claim with, so it says every frame that every drag beginning in its
/// rect is its own (`drag::keep`). A drag from inside it leaves the rail where it is and still
/// reaches the pad in full — keeping takes nothing from it; one from the page beside it folds
/// the rail as any other.
#[test]
fn a_region_that_keeps_its_drags_is_left_alone_by_the_page_swipe() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let _ = Rail::new().collapsible(true).show(
                ui,
                cx,
                |ui, cx| {
                    let _ = layout::list_item(ui, cx, icon::HOME, "Home", true);
                },
                |ui, cx| {
                    cx.with_app::<Pad, _>(|p, cx| {
                        // A pad across the top of the page, a third of it tall.
                        let region = egui::Rect::from_min_size(
                            ui.max_rect().min,
                            egui::vec2(ui.available_width(), ui.max_rect().height() / 3.0),
                        );
                        fairing::drag::keep(ui.ctx(), region);
                        // The pad reads its own drag, the way a pan or a scrub would.
                        let pad = ui.interact(region, ui.id().with("pad"), egui::Sense::drag());
                        p.dragged_x += pad.drag_delta().x;
                        p.region = region;
                        layout::note(ui, cx, "page");
                    });
                },
            );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?
    .with_app(Pad::default());
    h.set_size(WIDE.x, WIDE.y);
    h.run_for(1.0);
    let open = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    let Some(region) = h.app_mut::<Pad>().map(|p| p.region) else {
        return Err(fairing::Error::Config("the page state is there".to_owned()));
    };
    assert!(region.is_positive(), "the pad was drawn: {region:?}");
    // From inside the pad: the pad's.
    let inside = region.center();
    h.drag(inside, egui::pos2(inside.x - 150.0, inside.y), 6);
    h.run_for(1.0);
    let kept = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        (kept - open).abs() < 1.0,
        "a drag that began in a kept region leaves the rail alone: {open} -> {kept}"
    );
    let dragged_x = h.app_mut::<Pad>().map_or(0.0, |p| p.dragged_x);
    assert!(
        dragged_x < -100.0,
        "and the pad still got the whole drag — keeping takes nothing from it: {dragged_x}"
    );
    // From the page below it: the rail's.
    let below = egui::pos2(inside.x, region.bottom() + 100.0);
    h.drag(below, egui::pos2(below.x - 150.0, below.y), 6);
    h.run_for(1.0);
    let folded = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        folded < open * 0.5,
        "and one from the page beside it folds the rail as any other: {open} -> {folded}"
    );
    Ok(())
}

/// Whether the app has the rail put away this frame.
#[derive(Default)]
struct Hide {
    hidden: bool,
}

/// **A rail the app hides is gone until the app shows it again**. A home screen that
/// is a wall of tiles has no use for the rail; `Rail::hidden(true)` slides the arm off and the
/// page takes the whole width, the status bar staying where it is. No grip and no swipe brings
/// it back — the condition is the app's — and `hidden(false)` slides it back in.
#[test]
fn a_rail_the_app_hides_is_gone_until_the_app_shows_it() -> fairing::Result<()> {
    let mut h = Harness::from_builder(|ctx| {
        let mut shell = Shell::builder(single_level_access()).build(ctx)?;
        shell.add(screen("c", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let hidden = cx.with_app::<Hide, _>(|s, _| s.hidden).unwrap_or(false);
            let _ = Rail::new().collapsible(true).hidden(hidden).show(
                ui,
                cx,
                |ui, cx| {
                    for (i, name) in ENTRIES.iter().enumerate() {
                        let _ = layout::list_item(ui, cx, icon::HOME, name, i == 0);
                    }
                },
                |ui, cx| layout::note(ui, cx, "page"),
            );
        }));
        shell.launch(fairing::LaunchAction::open("c"));
        Ok(shell)
    })?
    .with_app(Hide::default());
    h.set_size(WIDE.x, WIDE.y);
    h.run_for(1.0);
    let Some(open_arm) = arm(&filled(&mut h)) else {
        return Err(fairing::Error::Config("shown, there is an arm".to_owned()));
    };
    let status_h = h.shell.theme().metrics.row_height;

    if let Some(s) = h.app_mut::<Hide>() {
        s.hidden = true;
    }
    h.run_for(1.0);
    let shapes = filled(&mut h);
    assert!(
        arm(&shapes).is_none(),
        "hidden, no arm is left: {:?}",
        arm(&shapes)
    );
    // The page starts where the arm used to; the status bar row above it is still painted.
    let page_left = shapes
        .iter()
        .filter(|r| r.rect.height() > WIDE.y * 0.5 && r.rect.width() > WIDE.x * 0.5)
        .map(|r| r.rect.left())
        .fold(f32::NEG_INFINITY, f32::max);
    let inset = h.shell.theme().metrics.screen_inset;
    assert!(
        page_left <= open_arm.left() + inset + 0.5,
        "the page takes the whole width once the rail is hidden: it starts at {page_left}, the \
         band began at {}",
        open_arm.left()
    );
    let status_bar = shapes
        .iter()
        .any(|r| r.rect.top() < status_h && r.rect.width() > WIDE.x * 0.5);
    assert!(
        status_bar,
        "the status bar is the shell's and stays: {shapes:?}"
    );

    // Neither the grip at the left edge nor a swipe across the page brings a hidden rail back.
    let y = open_arm.center().y;
    h.drag(
        egui::pos2(open_arm.left() + 6.0, y),
        egui::pos2(open_arm.left() + 160.0, y),
        4,
    );
    h.run_for(1.0);
    assert!(
        arm(&filled(&mut h)).is_none(),
        "a drag from the left edge cannot bring back a rail the app hid"
    );
    h.drag(egui::pos2(WIDE.x * 0.4, y), egui::pos2(WIDE.x * 0.7, y), 4);
    h.run_for(1.0);
    assert!(
        arm(&filled(&mut h)).is_none(),
        "nor can a swipe across the page"
    );

    if let Some(s) = h.app_mut::<Hide>() {
        s.hidden = false;
    }
    h.run_for(1.0);
    let back = arm(&filled(&mut h)).map_or(0.0, |r| r.width());
    assert!(
        (back - open_arm.width()).abs() < 1.0,
        "shown again, the rail is back as it was: {} -> {back}",
        open_arm.width()
    );
    Ok(())
}
