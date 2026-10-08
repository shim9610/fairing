//! **Placing the status bar's items yourself** — rung 4 of the override ladder.
//! A layout gets the items as the bar would place them, collapsed ones included, and the
//! bar still measures, draws and presses them where it says.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.
#![cfg(feature = "mock")]

use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, LaunchAction, Shell, ShellConfig, Slot, StatusLayoutCx};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("{what} is missing")))
}

/// One level, motion reduced: the default bar, the clock on the left and four items on the right.
fn config() -> ShellConfig {
    let mut config = single_level_access();
    config.motion.reduce = true;
    config
}

/// A shell `width` wide on the mock services — so Bluetooth, Wi-Fi and the battery are reported and
/// drawn — with one screen to open, built with `build`.
fn shell_with(
    width: f32,
    build: impl FnOnce(fairing::ShellBuilder) -> fairing::ShellBuilder,
) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| {
        build(Shell::builder(config()).services(fairing::services::mock::services())).build(ctx)
    })?
    .with_size(width, 600.0);
    h.shell.add(
        screen("pumps", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("pumps");
        })
        .icon(fairing::icon::GAUGE)
        .desktop(),
    );
    h.frames(3);
    Ok(h)
}

fn same(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min - b.min).length() < 0.5 && (a.max - b.max).length() < 0.5
}

/// The clock to the middle of the bar.
fn clock_to_the_middle(bar: &StatusLayoutCx<'_>, rects: &mut [egui::Rect]) {
    if let Some(clock) = bar.index_of("status.clock").and_then(|i| rects.get_mut(i)) {
        *clock = clock.translate(egui::vec2(bar.rect.center().x - clock.center().x, 0.0));
    }
}

/// **The bar draws and presses an item where the layout puts it** — the clock moved to the middle
/// is reported there, and a tap there does what the clock's tap does.
#[test]
fn a_status_layout_moves_an_item_and_its_tap() -> fairing::Result<()> {
    let plain = shell_with(1024.0, |b| b)?;
    let before = need(
        plain.shell.status_bar().item_rect("status.clock"),
        "the clock",
    )?;
    let mut h = shell_with(1024.0, |b| b.status_bar_layout(clock_to_the_middle))?;
    let bar = need(h.shell.layout().status, "the status bar")?;
    let clock = need(h.shell.status_bar().item_rect("status.clock"), "the clock")?;
    assert!(
        (clock.center().x - bar.center().x).abs() < 0.5,
        "{clock:?} in {bar:?}"
    );
    assert!((clock.width() - before.width()).abs() < 0.5, "only moved");
    assert_eq!(
        h.shell.status_bar().item_rect("status.battery"),
        plain.shell.status_bar().item_rect("status.battery"),
        "the battery stays where the bar put it"
    );
    if let Some(spec) = h.shell.status_bar_mut().spec_mut("status.clock") {
        spec.tap_action = Some(LaunchAction::open("pumps"));
    }
    assert!(h.shell.workspace().is_home());
    h.tap(clock.center());
    h.frames(2);
    assert!(
        !h.shell.workspace().is_home(),
        "the moved clock did not take the tap"
    );
    Ok(())
}

/// What a layout was told of one item: its id, its slot and the rect it came with.
type Told = (String, Option<Slot>, egui::Rect);

/// **The rects come laid out the built-in way**, one per item in left, centre, right order, so
/// a layout only changes what it means to.
#[test]
fn the_status_rects_come_laid_out_the_built_in_way() -> fairing::Result<()> {
    let plain = shell_with(1024.0, |b| b)?;
    let seen: Rc<RefCell<Vec<Told>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    let _h = shell_with(1024.0, move |b| {
        b.status_bar_layout(move |bar: &StatusLayoutCx<'_>, rects: &mut [egui::Rect]| {
            let mut out = Vec::new();
            for (i, rect) in rects.iter().enumerate() {
                out.push((bar.id(i).unwrap_or("?").to_owned(), bar.slot(i), *rect));
            }
            record.replace(out);
        })
    })?;
    let seen = seen.borrow();
    let ids: Vec<&str> = seen.iter().map(|(id, ..)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "status.clock",
            "status.notifications",
            "status.bluetooth",
            "status.wifi",
            "status.battery"
        ]
    );
    assert_eq!(
        seen.first().and_then(|(_, slot, _)| *slot),
        Some(Slot::Left)
    );
    assert_eq!(
        seen.last().and_then(|(_, slot, _)| *slot),
        Some(Slot::Right)
    );
    for (id, _, rect) in seen.iter() {
        let drawn = need(plain.shell.status_bar().item_rect(id), id)?;
        assert!(same(*rect, drawn), "{id}: {rect:?} vs {drawn:?}");
    }
    Ok(())
}

/// **A rect emptied leaves its item out** — not drawn, not pressed, no rect to report.
#[test]
fn an_emptied_status_rect_leaves_its_item_out() -> fairing::Result<()> {
    let plain = shell_with(1024.0, |b| b)?;
    let clock = need(
        plain.shell.status_bar().item_rect("status.clock"),
        "the clock",
    )?;
    let mut h = shell_with(1024.0, |b| {
        b.status_bar_layout(|bar: &StatusLayoutCx<'_>, rects: &mut [egui::Rect]| {
            if let Some(rect) = bar.index_of("status.clock").and_then(|i| rects.get_mut(i)) {
                *rect = egui::Rect::NOTHING;
            }
        })
    })?;
    assert_eq!(h.shell.status_bar().item_rect("status.clock"), None);
    if let Some(spec) = h.shell.status_bar_mut().spec_mut("status.clock") {
        spec.tap_action = Some(LaunchAction::open("pumps"));
    }
    h.tap(clock.center());
    h.frames(2);
    assert!(
        h.shell.workspace().is_home(),
        "the left-out clock still took a tap"
    );
    Ok(())
}

/// **A collapsed item comes empty, and a layout can bring it back.** On a bar too narrow for all
/// of them the lowest priority goes; the layout is told which, and a rect given to it draws it.
#[test]
fn a_collapsed_item_comes_empty_and_a_layout_can_bring_it_back() -> fairing::Result<()> {
    let plain = shell_with(150.0, |b| b)?;
    let gone: Vec<&str> = [
        "status.clock",
        "status.notifications",
        "status.bluetooth",
        "status.wifi",
        "status.battery",
    ]
    .into_iter()
    .filter(|id| plain.shell.status_bar().item_rect(id).is_none())
    .collect();
    let first = need(gone.first().copied(), "a collapsed item on a 150 px bar")?;
    let told: Rc<RefCell<Vec<(String, bool, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let back = first.to_owned();
    let h = shell_with(150.0, move |b| {
        b.status_bar_layout(move |bar: &StatusLayoutCx<'_>, rects: &mut [egui::Rect]| {
            let mut out = Vec::new();
            for (i, rect) in rects.iter().enumerate() {
                out.push((
                    bar.id(i).unwrap_or("?").to_owned(),
                    bar.is_collapsed(i),
                    rect.is_positive(),
                ));
            }
            record.replace(out);
            if let Some(rect) = bar.index_of(&back).and_then(|i| rects.get_mut(i)) {
                *rect =
                    egui::Rect::from_min_size(bar.inner.min, egui::vec2(24.0, bar.inner.height()));
            }
        })
    })?;
    let told = told.borrow();
    let entry = need(told.iter().find(|(id, ..)| id == first), first)?;
    assert!(
        entry.1 && !entry.2,
        "{first} is not told collapsed: {told:?}"
    );
    assert!(
        told.iter()
            .all(|(_, collapsed, filled)| collapsed != filled),
        "a collapsed item came with a rect, or a shown one without: {told:?}"
    );
    assert!(
        h.shell.status_bar().item_rect(first).is_some(),
        "{first} given a rect is not drawn"
    );
    Ok(())
}

/// **A painter wins over a layout**: it draws the whole bar, so the layout is
/// never called.
#[test]
fn a_status_bar_painter_wins_over_a_status_layout() -> fairing::Result<()> {
    let laid = Rc::new(Cell::new(0_u32));
    let painted = Rc::new(Cell::new(0_u32));
    let (l, p) = (Rc::clone(&laid), Rc::clone(&painted));
    let _h = shell_with(1024.0, move |b| {
        b.status_bar_layout(move |_: &StatusLayoutCx<'_>, _: &mut [egui::Rect]| {
            l.set(l.get() + 1);
        })
        .status_bar_painter(move |_: &mut egui::Ui, _: &mut fairing::BarCx<'_>| {
            p.set(p.get() + 1);
        })
    })?;
    assert!(painted.get() > 0, "the painter draws");
    assert_eq!(laid.get(), 0, "the layout is never called");
    Ok(())
}

/// A rect reaching past the bar is cut by it: what is reported and pressed is the part inside.
#[test]
fn a_status_rect_past_the_bar_is_cut_by_it() -> fairing::Result<()> {
    let h = shell_with(1024.0, |b| {
        b.status_bar_layout(|bar: &StatusLayoutCx<'_>, rects: &mut [egui::Rect]| {
            if let Some(rect) = bar.index_of("status.wifi").and_then(|i| rects.get_mut(i)) {
                *rect = rect.expand2(egui::vec2(0.0, 40.0));
            }
        })
    })?;
    let bar = need(h.shell.layout().status, "the status bar")?;
    let wifi = need(h.shell.status_bar().item_rect("status.wifi"), "Wi-Fi")?;
    assert!(bar.contains_rect(wifi), "{wifi:?} in {bar:?}");
    Ok(())
}

/// **A collapsed `status_item` is not run** beyond the one out-of-sight pass that measures it.
/// Its closure draws it, so a closure called each frame for an item with no room would draw into
/// nothing — and do whatever else it does each frame.
#[test]
fn a_collapsed_status_item_is_not_run() -> fairing::Result<()> {
    let runs = Rc::new(Cell::new(0_u32));
    let count = Rc::clone(&runs);
    let mut h = shell_with(150.0, |b| b)?;
    h.shell.add(
        fairing::status_item(
            "app.temp",
            Slot::Right,
            move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                count.set(count.get() + 1);
                ui.label("36.5 °C");
            },
        )
        .priority(-10),
    );
    h.frames(3);
    assert_eq!(
        h.shell.status_bar().item_rect("app.temp"),
        None,
        "it has room"
    );
    assert_eq!(runs.get(), 1, "measured once, out of sight");
    h.frames(10);
    assert_eq!(runs.get(), 1, "a collapsed item's closure ran again");
    Ok(())
}
