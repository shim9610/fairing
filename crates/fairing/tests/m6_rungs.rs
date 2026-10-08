//! M6 — the override rungs the chrome was missing: placing the
//! nav bar's items yourself; placing and drawing the toasts, the heads-up banner and the keyboard's
//! keys yourself.
//!
//! What it checks: a layout gets the built-in places and the shell draws, presses and times what
//! it places; a place emptied leaves its item out; the layout sees the ids and which items are
//! live; a painter draws the card and the shell still takes its tap; a painter can ask for
//! height; a nav bar painter wins over a nav bar layout; the gesture bar has nothing to place; a
//! key moved types where it went, and a key painter is told which key is held and which is locked;
//! a shade tile moved is pressed where it went, the rows below the tiles follow the lowest one, and
//! the tiles round an expanded one stay where the layout put them; a shade tile painter draws every
//! tile and is told what is lit, held and locked while the shell keeps the taps, and a shade panel
//! painter draws the ground — curtain or card, and the panel a split shade crosses away from — with
//! the list fading into the colour it says.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::chrome::NavItem;
use fairing::notify::{HeadsUpCx, HeadsUpLayoutCx, ToastCx, ToastLayoutCx};
use fairing::testing::{single_level_access, Harness};
use fairing::{
    screen, Cx, LaunchAction, NavLayoutCx, Notification, NotificationId, Shell, ShellConfig,
    ShellEvent,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fail(format!("{what} is missing")))
}

/// The base config: one level, motion reduced, the three buttons.
fn config() -> ShellConfig {
    let mut config = single_level_access();
    config.motion.reduce = true;
    config
}

/// A shell with one screen to open, built with `build` (a layout or a painter).
fn shell_with(
    config: ShellConfig,
    build: impl FnOnce(fairing::ShellBuilder) -> fairing::ShellBuilder,
) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| build(Shell::builder(config)).build(ctx))?;
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

/// Back hard against the bar's left edge; everything else where the shell put it.
fn back_to_the_left(bar: &NavLayoutCx<'_>, cells: &mut [egui::Rect]) {
    if let Some(cell) = bar.index_of("back").and_then(|i| cells.get_mut(i)) {
        *cell = cell.translate(egui::vec2(bar.rect.left() - cell.left(), 0.0));
    }
}

/// **The shell draws and presses where the layout puts an item** — back moved to the left edge is
/// reported there, and a tap there goes back.
#[test]
fn a_layout_moves_an_item_and_its_tap() -> fairing::Result<()> {
    let plain = shell_with(config(), |b| b)?;
    let before = need(plain.shell.nav_bar().item_rect(&NavItem::Back), "back")?;
    let mut h = shell_with(config(), |b| b.nav_bar_layout(back_to_the_left))?;
    let bar = need(h.shell.layout().nav, "the nav bar")?;
    let back = need(h.shell.nav_bar().item_rect(&NavItem::Back), "back")?;
    assert!(
        (back.left() - bar.left()).abs() < 0.5,
        "{back:?} in {bar:?}"
    );
    assert!((back.width() - before.width()).abs() < 0.5, "only moved");
    assert_eq!(
        h.shell.nav_bar().item_rect(&NavItem::Home),
        plain.shell.nav_bar().item_rect(&NavItem::Home),
        "home stays where the shell put it"
    );
    // Open a screen, then go back from the moved button.
    let icon = need(h.shell.desktop().icon_rect("pumps"), "the pumps icon")?;
    h.tap(icon.center());
    h.frames(2);
    assert!(!h.shell.workspace().is_home());
    let _ = h.shell.poll_events();
    h.tap(back.center());
    h.frames(2);
    assert!(
        h.shell.workspace().is_home(),
        "the moved back button goes back"
    );
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(e, ShellEvent::WentHome)),
        "{events:?}"
    );
    Ok(())
}

/// **The cells come laid out the built-in way**, so a layout only changes what it means to.
#[test]
fn the_cells_come_laid_out_the_built_in_way() -> fairing::Result<()> {
    let plain = shell_with(config(), |b| b)?;
    let seen: Rc<RefCell<Vec<egui::Rect>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    let _h = shell_with(config(), move |b| {
        b.nav_bar_layout(move |_: &NavLayoutCx<'_>, cells: &mut [egui::Rect]| {
            record.replace(cells.to_vec());
        })
    })?;
    let seen = seen.borrow();
    assert_eq!(seen.len(), 3, "one cell an item: {seen:?}");
    for (cell, item) in seen
        .iter()
        .zip([NavItem::Back, NavItem::Home, NavItem::Recents])
    {
        let drawn = need(plain.shell.nav_bar().item_rect(&item), item.id())?;
        assert!(
            (cell.min - drawn.min).length() < 0.5 && (cell.max - drawn.max).length() < 0.5,
            "{item:?}: {cell:?} vs {drawn:?}"
        );
    }
    Ok(())
}

/// **A cell emptied leaves its item out** — not drawn, not pressed, no rect to report.
#[test]
fn an_emptied_cell_leaves_its_item_out() -> fairing::Result<()> {
    let plain = shell_with(config(), |b| b)?;
    let recents = need(
        plain.shell.nav_bar().item_rect(&NavItem::Recents),
        "recents",
    )?;
    let mut h = shell_with(config(), |b| {
        b.nav_bar_layout(|bar: &NavLayoutCx<'_>, cells: &mut [egui::Rect]| {
            if let Some(cell) = bar.index_of("recents").and_then(|i| cells.get_mut(i)) {
                *cell = egui::Rect::NOTHING;
            }
        })
    })?;
    assert_eq!(h.shell.nav_bar().item_rect(&NavItem::Recents), None);
    let _ = h.shell.poll_events();
    h.tap(recents.center());
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::OverviewRequested)),
        "the left-out button still took a tap: {events:?}"
    );
    Ok(())
}

/// **The layout sees the ids, and which items are live** — back at home is not, a `nav_item` past
/// its gate is.
#[test]
fn the_layout_sees_the_ids_and_what_is_live() -> fairing::Result<()> {
    let mut config = config();
    config.nav_bar.items = vec!["back".to_owned(), "home".to_owned(), "kbd".to_owned()];
    let seen: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    let mut h = shell_with(config, move |b| {
        b.nav_bar_layout(move |bar: &NavLayoutCx<'_>, _: &mut [egui::Rect]| {
            let mut out = Vec::new();
            for i in 0..bar.len() {
                out.push((bar.id(i).unwrap_or("?").to_owned(), bar.is_live(i)));
            }
            record.replace(out);
        })
    })?;
    h.shell.add(fairing::nav_item(
        "kbd",
        |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("⌨");
        },
    ));
    h.frames(2);
    assert_eq!(
        *seen.borrow(),
        vec![
            ("back".to_owned(), false),
            ("home".to_owned(), true),
            ("kbd".to_owned(), true)
        ]
    );
    Ok(())
}

/// **A painter wins over a layout**: it draws the whole bar, so the layout is
/// never called.
#[test]
fn a_painter_wins_over_a_layout() -> fairing::Result<()> {
    let laid = Rc::new(Cell::new(0_u32));
    let painted = Rc::new(Cell::new(0_u32));
    let (l, p) = (Rc::clone(&laid), Rc::clone(&painted));
    let _h = shell_with(config(), move |b| {
        b.nav_bar_layout(move |_: &NavLayoutCx<'_>, _: &mut [egui::Rect]| {
            l.set(l.get() + 1);
        })
        .nav_bar_painter(move |_: &mut egui::Ui, _: &mut fairing::BarCx<'_>| {
            p.set(p.get() + 1);
        })
    })?;
    assert!(painted.get() > 0, "the painter draws");
    assert_eq!(laid.get(), 0, "the layout is never called");
    Ok(())
}

/// The gesture bar has no items: there is nothing to place, and the layout is not called.
#[test]
fn the_gesture_bar_has_nothing_to_place() -> fairing::Result<()> {
    let mut config = config();
    "gesture".clone_into(&mut config.nav_bar.style);
    let laid = Rc::new(Cell::new(0_u32));
    let l = Rc::clone(&laid);
    let _h = shell_with(config, move |b| {
        b.nav_bar_layout(move |_: &NavLayoutCx<'_>, _: &mut [egui::Rect]| {
            l.set(l.get() + 1);
        })
    })?;
    assert_eq!(laid.get(), 0);
    Ok(())
}

/// A cell reaching past the bar is cut by it: what is reported and pressed is the part inside.
#[test]
fn a_cell_past_the_bar_is_cut_by_it() -> fairing::Result<()> {
    let h = shell_with(config(), |b| {
        b.nav_bar_layout(|bar: &NavLayoutCx<'_>, cells: &mut [egui::Rect]| {
            if let Some(cell) = bar.index_of("home").and_then(|i| cells.get_mut(i)) {
                *cell = cell.expand2(egui::vec2(0.0, 40.0));
            }
        })
    })?;
    let bar = need(h.shell.layout().nav, "the nav bar")?;
    let home = need(h.shell.nav_bar().item_rect(&NavItem::Home), "home")?;
    assert!(bar.contains_rect(home), "{home:?} in {bar:?}");
    Ok(())
}

/// Every text drawn this frame.
fn texts(h: &mut Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// The top right corner, 16 in from both edges, the stack going down.
fn toasts_top_right(area: &ToastLayoutCx<'_>, rects: &mut [egui::Rect]) {
    let mut top = area.area.top() + 16.0;
    for rect in rects.iter_mut() {
        *rect = egui::Rect::from_min_size(
            egui::pos2(area.area.right() - 16.0 - rect.width(), top),
            rect.size(),
        );
        top += rect.height() + 8.0;
    }
}

/// **A toast layout moves the stack** — the toast is drawn and reported where the layout put it.
#[test]
fn a_toast_layout_moves_the_stack() -> fairing::Result<()> {
    let mut h = shell_with(config(), |b| b.toast_layout(toasts_top_right))?;
    h.shell.toast("Saved");
    h.frames(3);
    let area = h.shell.layout().content;
    let rect = need(
        h.shell.toasts().visible().first().map(|t| t.rect),
        "the toast",
    )?;
    assert!(
        (rect.top() - (area.top() + 16.0)).abs() < 0.5,
        "{rect:?} in {area:?}"
    );
    assert!(
        (rect.right() - (area.right() - 16.0)).abs() < 0.5,
        "{rect:?} in {area:?}"
    );
    Ok(())
}

/// **The toast layout gets the stack placed the built-in way**, at rest.
#[test]
fn the_toast_layout_gets_the_built_in_stack() -> fairing::Result<()> {
    let mut plain = shell_with(config(), |b| b)?;
    plain.shell.toast("Saved");
    plain.frames(3);
    let built_in = need(
        plain.shell.toasts().visible().first().map(|t| t.rect),
        "the toast",
    )?;
    let seen: Rc<RefCell<Vec<egui::Rect>>> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&seen);
    let mut h = shell_with(config(), move |b| {
        b.toast_layout(move |_: &ToastLayoutCx<'_>, rects: &mut [egui::Rect]| {
            record.replace(rects.to_vec());
        })
    })?;
    h.shell.toast("Saved");
    h.frames(3);
    let seen = seen.borrow();
    let first = need(seen.first().copied(), "a rect")?;
    assert!(
        (first.min - built_in.min).length() < 0.5 && (first.max - built_in.max).length() < 0.5,
        "{first:?} vs {built_in:?}"
    );
    Ok(())
}

/// **A rect emptied keeps its toast off the screen** — still timed, so it goes when it would have.
#[test]
fn an_emptied_toast_rect_keeps_it_off_the_screen() -> fairing::Result<()> {
    let mut h = shell_with(config(), |b| {
        b.toast_layout(|_: &ToastLayoutCx<'_>, rects: &mut [egui::Rect]| {
            rects.fill(egui::Rect::NOTHING);
        })
    })?;
    h.shell
        .toast(fairing::Toast::new("Saved").duration(std::time::Duration::from_millis(500)));
    h.frames(3);
    assert_eq!(h.shell.toasts().visible().len(), 1, "it is on its way");
    assert!(!texts(&mut h).iter().any(|t| t == "Saved"), "and not drawn");
    h.run_for(1.0);
    h.frames(2);
    assert!(h.shell.toasts().visible().is_empty(), "it went on time");
    Ok(())
}

/// A toast painter: the text with a mark, a card it asks to be 120 tall.
fn marked_toast(ui: &mut egui::Ui, toast: &mut ToastCx<'_>) {
    toast.set_height(120.0);
    ui.painter()
        .rect_filled(toast.rect, 0.0, egui::Color32::DARK_BLUE);
    ui.painter().text(
        toast.rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("» {}", toast.toast.text),
        egui::FontId::proportional(16.0),
        egui::Color32::WHITE,
    );
}

/// **A toast painter draws the card**, the size it asks for — and the shell still takes the tap
/// that puts it away.
#[test]
fn a_toast_painter_draws_the_card_and_the_shell_takes_its_tap() -> fairing::Result<()> {
    let mut h = shell_with(config(), |b| b.toast_painter(marked_toast))?;
    h.shell.toast("Saved");
    h.frames(3);
    let drawn = texts(&mut h);
    assert!(drawn.iter().any(|t| t == "» Saved"), "{drawn:?}");
    assert!(
        !drawn.iter().any(|t| t == "Saved"),
        "the built-in card is not drawn"
    );
    let rect = need(
        h.shell.toasts().visible().first().map(|t| t.rect),
        "the toast",
    )?;
    assert!(
        (rect.height() - 120.0).abs() < 0.5,
        "the height it asked for: {rect:?}"
    );
    h.tap(rect.center());
    h.frames(30);
    assert!(h.shell.toasts().visible().is_empty(), "the tap put it away");
    Ok(())
}

/// A banner that asks to come down at the bottom left.
fn banner_bottom_left(cx: &HeadsUpLayoutCx<'_>, rect: &mut egui::Rect) {
    *rect = egui::Rect::from_min_size(
        egui::pos2(
            cx.screen.left() + 16.0,
            cx.screen.bottom() - 16.0 - rect.height(),
        ),
        egui::vec2(300.0, 40.0),
    );
}

fn post(h: &mut Harness) {
    h.shell.notify(
        Notification::new(NotificationId::of("pump"), "Pump stopped")
            .body("Pressure fell below 0.4 bar.")
            .action(LaunchAction::open("pumps")),
    );
}

/// **A heads-up layout places the banner** — its width included; its height stays its own.
#[test]
fn a_heads_up_layout_places_the_banner() -> fairing::Result<()> {
    let mut h = shell_with(config(), |b| b.heads_up_layout(banner_bottom_left))?;
    post(&mut h);
    h.frames(3);
    let screen = h.screen_rect();
    let rect = need(h.shell.heads_up().rect(), "the banner")?;
    assert!((rect.left() - 16.0).abs() < 0.5, "{rect:?}");
    assert!(
        (rect.width() - 300.0).abs() < 0.5,
        "the width it was given: {rect:?}"
    );
    assert!(
        rect.height() >= h.shell.theme().metrics.heads_up_height - 0.5,
        "its own height, not the 40 asked: {rect:?}"
    );
    assert!(rect.bottom() <= screen.bottom(), "{rect:?}");
    Ok(())
}

/// **A rect left no width keeps the banner off the screen** — still timed, so it goes when it
/// would have.
#[test]
fn an_emptied_banner_rect_keeps_it_off_the_screen() -> fairing::Result<()> {
    let mut h = shell_with(config(), |b| {
        b.heads_up_layout(|_: &HeadsUpLayoutCx<'_>, rect: &mut egui::Rect| {
            *rect = egui::Rect::NOTHING;
        })
    })?;
    post(&mut h);
    h.frames(3);
    assert_eq!(
        h.shell.heads_up().visible(),
        Some(NotificationId::of("pump"))
    );
    assert_eq!(h.shell.heads_up().rect(), None);
    assert!(
        !texts(&mut h).iter().any(|t| t == "Pump stopped"),
        "not drawn"
    );
    h.run_for(4.5);
    h.frames(2);
    assert_eq!(h.shell.heads_up().visible(), None, "it went on time");
    Ok(())
}

/// A banner painter: the title with a mark.
fn marked_banner(ui: &mut egui::Ui, banner: &mut HeadsUpCx<'_>) {
    banner.set_height(150.0);
    ui.painter()
        .rect_filled(banner.rect, 0.0, egui::Color32::DARK_RED);
    ui.painter().text(
        banner.rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("! {}", banner.title),
        egui::FontId::proportional(16.0),
        egui::Color32::WHITE,
    );
}

/// **A heads-up painter draws the banner**, as tall as it asks — and its tap still opens the
/// notification.
#[test]
fn a_heads_up_painter_draws_the_banner_and_its_tap_opens_it() -> fairing::Result<()> {
    let mut h = shell_with(config(), |b| b.heads_up_painter(marked_banner))?;
    post(&mut h);
    h.frames(3);
    let drawn = texts(&mut h);
    assert!(drawn.iter().any(|t| t == "! Pump stopped"), "{drawn:?}");
    assert!(
        !drawn.iter().any(|t| t == "Pump stopped"),
        "the built-in banner is not drawn"
    );
    let rect = need(h.shell.heads_up().rect(), "the banner")?;
    assert!(
        (rect.height() - 150.0).abs() < 0.5,
        "the height it asked for: {rect:?}"
    );
    let _ = h.shell.poll_events();
    h.tap(rect.center());
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "pumps")),
        "the tap opened the notification: {events:?}"
    );
    Ok(())
}

/// The keyboard's rungs (feature `osk`).
#[cfg(feature = "osk")]
mod keyboard {
    use super::{config, need, texts};
    use fairing::osk::{KeyAction, OskKeyCx, OskKeyLayoutCx};
    use fairing::testing::Harness;
    use fairing::{screen, Cx, LaunchAction, Shell};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    /// A screen with one field and the keyboard up over it; what is typed lands in the string.
    fn typing(
        build: impl FnOnce(fairing::ShellBuilder) -> fairing::ShellBuilder,
    ) -> fairing::Result<(Harness, Rc<RefCell<String>>)> {
        let config = config();
        let mut h = Harness::from_builder(move |ctx| build(Shell::builder(config)).build(ctx))?;
        let text = Rc::new(RefCell::new(String::new()));
        let field = Rc::new(Cell::new(egui::Rect::NOTHING));
        let (t, f) = (Rc::clone(&text), Rc::clone(&field));
        h.shell
            .add(screen("form", move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                let edit = ui.add_sized(
                    [300.0, 48.0],
                    egui::TextEdit::singleline(&mut *t.borrow_mut()),
                );
                f.set(edit.rect);
            }));
        h.shell.handle().launch(LaunchAction::open("form"));
        h.frames(3);
        h.tap(field.get().center());
        for _ in 0..12 {
            if h.shell.osk().is_visible() {
                break;
            }
            h.frame();
        }
        if !h.shell.osk().is_visible() {
            return Err(super::fail("the keyboard did not come up"));
        }
        Ok((h, text))
    }

    /// `q` and `p` trade places.
    fn swap_q_and_p(keys: &OskKeyLayoutCx<'_>, rects: &mut [egui::Rect]) {
        if let (Some(q), Some(p)) = (keys.index_of("q"), keys.index_of("p")) {
            rects.swap(q, p);
        }
    }

    /// **A key goes, and types, where the layout puts it** — `q` and `p` traded places: `q` is
    /// reported in `p`'s place, and a tap there types `q`.
    #[test]
    fn a_key_layout_moves_a_key_and_its_tap() -> fairing::Result<()> {
        let (plain, _) = typing(|b| b)?;
        let p_place = need(plain.shell.osk().key_rect("p"), "p")?;
        let q_place = need(plain.shell.osk().key_rect("q"), "q")?;
        let (mut h, text) = typing(|b| b.osk_key_layout(swap_q_and_p))?;
        assert_eq!(h.shell.osk().key_rect("q"), Some(p_place));
        assert_eq!(h.shell.osk().key_rect("p"), Some(q_place));
        h.tap(p_place.center());
        h.tap(q_place.center());
        assert_eq!(*text.borrow(), "qp", "each key types where it went");
        Ok(())
    }

    /// **The rects come placed the built-in way**, one per key in reading order, so a layout only
    /// changes what it means to — and it is told which face is up and which keys it has.
    #[test]
    fn the_key_layout_gets_the_built_in_places() -> fairing::Result<()> {
        type Seen = Vec<(usize, String, egui::Rect)>;
        let (plain, _) = typing(|b| b)?;
        let seen: Rc<RefCell<Seen>> = Rc::new(RefCell::new(Vec::new()));
        let record = Rc::clone(&seen);
        let (mut h, _) = typing(move |b| {
            b.osk_key_layout(move |keys: &OskKeyLayoutCx<'_>, rects: &mut [egui::Rect]| {
                let mut out = Vec::new();
                for (i, rect) in rects.iter().enumerate() {
                    if let Some((_, _, key)) = keys.key(i) {
                        out.push((keys.face_index, key.label.to_string(), *rect));
                    }
                }
                assert_eq!(keys.len(), rects.len(), "one rect a key");
                record.replace(out);
            })
        })?;
        {
            let seen = seen.borrow();
            assert!(seen.len() > 30, "the whole lower-case face: {}", seen.len());
            for (face, label, rect) in seen.iter() {
                assert_eq!(*face, 0, "the lower-case face is up");
                let built_in = need(plain.shell.osk().key_rect(label), label)?;
                assert!(
                    (rect.min - built_in.min).length() < 0.01
                        && (rect.max - built_in.max).length() < 0.01,
                    "{label}: {rect:?} vs {built_in:?}"
                );
            }
        }
        // ⇧ brings up the upper-case face, and the layout places that one.
        let shift = need(h.shell.osk().key_rect("⇧"), "⇧")?;
        h.tap(shift.center());
        let seen = seen.borrow();
        assert!(
            seen.iter().all(|(face, _, _)| *face == 1),
            "the face on show"
        );
        assert!(seen.iter().any(|(_, label, _)| label == "Q"), "its keys");
        Ok(())
    }

    /// **A place emptied leaves its key out** — no rect to report, and nothing typed where it was.
    #[test]
    fn an_emptied_place_leaves_its_key_out() -> fairing::Result<()> {
        let (plain, _) = typing(|b| b)?;
        let q_place = need(plain.shell.osk().key_rect("q"), "q")?;
        let (mut h, text) = typing(|b| {
            b.osk_key_layout(|keys: &OskKeyLayoutCx<'_>, rects: &mut [egui::Rect]| {
                if let Some(rect) = keys.index_of("q").and_then(|i| rects.get_mut(i)) {
                    *rect = egui::Rect::NOTHING;
                }
            })
        })?;
        assert_eq!(h.shell.osk().key_rect("q"), None);
        h.tap(q_place.center());
        assert_eq!(*text.borrow(), "", "the left-out key typed");
        let w = need(h.shell.osk().key_rect("w"), "w")?;
        h.tap(w.center());
        assert_eq!(*text.borrow(), "w", "the others still type");
        Ok(())
    }

    /// What a key painter was told about one key on one frame.
    #[derive(Debug, Clone, PartialEq)]
    struct Told {
        label: String,
        rect: egui::Rect,
        pressed: bool,
        locked: bool,
        letter: bool,
    }

    /// A key painter that marks each label and remembers what it was told.
    fn marking(seen: &Rc<RefCell<Vec<Told>>>) -> impl FnMut(&egui::Painter, &mut OskKeyCx<'_>) {
        let seen = Rc::clone(seen);
        move |painter: &egui::Painter, key: &mut OskKeyCx<'_>| {
            painter.rect_filled(key.rect, 0.0, egui::Color32::DARK_GREEN);
            painter.text(
                key.rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("<{}>", key.label),
                egui::FontId::proportional(14.0),
                egui::Color32::WHITE,
            );
            seen.borrow_mut().push(Told {
                label: key.label.to_owned(),
                rect: key.rect,
                pressed: key.pressed,
                locked: key.locked,
                letter: matches!(key.action, KeyAction::Text(_)),
            });
        }
    }

    /// **A key painter draws every key in the shell's place**, the built-in labels gone — and the
    /// keys still type.
    #[test]
    fn a_key_painter_draws_the_keys_and_they_still_type() -> fairing::Result<()> {
        let (plain, _) = typing(|b| b)?;
        let q_place = need(plain.shell.osk().key_rect("q"), "q")?;
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = marking(&seen);
        let (mut h, text) = typing(move |b| b.osk_key_painter(painter))?;
        seen.borrow_mut().clear();
        let drawn = texts(&mut h);
        assert!(drawn.iter().any(|t| t == "<q>"), "{drawn:?}");
        assert!(
            !drawn.iter().any(|t| t == "q"),
            "the built-in key is not drawn"
        );
        {
            let seen = seen.borrow();
            let q = need(seen.iter().find(|k| k.label == "q"), "q painted")?;
            assert_eq!(q.rect, q_place, "in the shell's place");
            assert!(q.letter, "q types a letter");
            let back = need(seen.iter().find(|k| k.label == "⌫"), "⌫ painted")?;
            assert!(!back.letter, "⌫ does not");
        }
        h.tap(q_place.center());
        assert_eq!(*text.borrow(), "q");
        Ok(())
    }

    /// **The painter is told which key a finger is on**, given it shrunk by the press, and which ⇧
    /// is locked.
    #[test]
    fn a_key_painter_is_told_what_is_held_and_what_is_locked() -> fairing::Result<()> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = marking(&seen);
        let (mut h, _) = typing(move |b| b.osk_key_painter(painter))?;
        let q_place = need(h.shell.osk().key_rect("q"), "q")?;
        h.press(q_place.center());
        h.frames(8);
        seen.borrow_mut().clear();
        h.frame();
        {
            let seen = seen.borrow();
            let q = need(seen.iter().find(|k| k.label == "q"), "q painted")?;
            assert!(q.pressed, "q is held");
            assert!(
                q.rect.width() < q_place.width() - 0.5,
                "shrunk: {:?}",
                q.rect
            );
            assert!(
                seen.iter().filter(|k| k.label != "q").all(|k| !k.pressed),
                "only q"
            );
        }
        h.release(q_place.center());
        h.frames(3);
        // ⇧ twice, quickly: caps lock.
        let shift = need(h.shell.osk().key_rect("⇧"), "⇧")?;
        h.tap(shift.center());
        h.tap(shift.center());
        assert!(h.shell.osk().shift_locked());
        seen.borrow_mut().clear();
        h.frame();
        let seen = seen.borrow();
        let shift = need(seen.iter().find(|k| k.label == "⇧"), "⇧ painted")?;
        assert!(shift.locked, "the locked ⇧");
        assert!(
            seen.iter().filter(|k| k.label != "⇧").all(|k| !k.locked),
            "only ⇧"
        );
        Ok(())
    }
}

/// The shade's rung (feature `overlay`).
#[cfg(feature = "overlay")]
mod shade {
    use super::{config, need};
    use fairing::overlay::{
        OverlayPanel, OverlayReveal, ShadePanelCx, ShadeTileCx, ShadeTileLayoutCx,
    };
    use fairing::settings::SettingValue;
    use fairing::testing::{access_config, Harness};
    use fairing::{ColorRole, LaunchAction, Shell, ShellConfig, ShellEvent};
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A shell on the mock services (Wi-Fi on, a brightness to slide), the shade open.
    fn shade_with(
        config: ShellConfig,
        build: impl FnOnce(fairing::ShellBuilder) -> fairing::ShellBuilder,
    ) -> fairing::Result<Harness> {
        let mut h = Harness::from_builder(move |ctx| {
            build(Shell::builder(config).services(fairing::services::mock::services())).build(ctx)
        })?;
        h.frames(2);
        h.shell.launch(LaunchAction::OpenOverlay);
        for _ in 0..90 {
            if h.shell.overlay().is_open() {
                break;
            }
            h.frame();
        }
        h.frames(2);
        if h.shell.overlay().is_open() {
            Ok(h)
        } else {
            Err(super::fail("the shade did not open"))
        }
    }

    fn tile(h: &Harness, id: &str) -> fairing::Result<egui::Rect> {
        need(h.shell.overlay().tile_rect(id), id)
    }

    /// The same rect, to a hair (the built-in rows add up their places; a layout's are given).
    fn same(a: egui::Rect, b: egui::Rect) -> bool {
        (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01
    }

    /// Wi-Fi and the theme tile trade places.
    fn swap_wifi_and_theme(tiles: &ShadeTileLayoutCx<'_>, rects: &mut [egui::Rect]) {
        if let (Some(a), Some(b)) = (tiles.index_of("tile.wifi"), tiles.index_of("tile.theme")) {
            rects.swap(a, b);
        }
    }

    /// **A tile goes, and is pressed, where the layout puts it** — Wi-Fi in the theme tile's place
    /// turns Wi-Fi off from there.
    #[test]
    fn a_tile_layout_moves_a_tile_and_its_tap() -> fairing::Result<()> {
        let plain = shade_with(config(), |b| b)?;
        let theme_place = tile(&plain, "tile.theme")?;
        let wifi_place = tile(&plain, "tile.wifi")?;
        let mut h = shade_with(config(), |b| b.shade_tile_layout(swap_wifi_and_theme))?;
        let (wifi, theme) = (tile(&h, "tile.wifi")?, tile(&h, "tile.theme")?);
        assert!(same(wifi, theme_place), "{wifi:?} vs {theme_place:?}");
        assert!(same(theme, wifi_place), "{theme:?} vs {wifi_place:?}");
        let _ = h.shell.poll_events();
        h.tap(theme_place.center());
        let events = h.shell.poll_events();
        assert!(
            events.iter().any(|e| matches!(
                e,
                ShellEvent::SettingChanged { key, value: SettingValue::Bool(false) }
                    if key.0.as_ref() == "wifi.enabled"
            )),
            "{events:?}"
        );
        assert!(!h.shell.services().wifi.enabled());
        Ok(())
    }

    /// **The rects come placed the built-in way**, one per tile in order, inside `area` — and the
    /// layout is told the columns, which tile is out and which the session may use.
    #[test]
    fn the_tile_layout_gets_the_built_in_places() -> fairing::Result<()> {
        type Seen = Vec<(String, egui::Rect, bool)>;
        type Told = Option<(usize, Option<usize>, egui::Rect)>;
        let plain = shade_with(config(), |b| b)?;
        let seen: Rc<RefCell<Seen>> = Rc::new(RefCell::new(Vec::new()));
        let told: Rc<RefCell<Told>> = Rc::new(RefCell::new(None));
        let (record, tell) = (Rc::clone(&seen), Rc::clone(&told));
        let _h = shade_with(config(), move |b| {
            b.shade_tile_layout(
                move |tiles: &ShadeTileLayoutCx<'_>, rects: &mut [egui::Rect]| {
                    let mut out = Vec::new();
                    for (i, rect) in rects.iter().enumerate() {
                        let id = tiles.id(i).unwrap_or("?").to_owned();
                        out.push((id, *rect, tiles.is_allowed(i)));
                    }
                    assert_eq!(tiles.len(), rects.len(), "one rect a tile");
                    record.replace(out);
                    tell.replace(Some((tiles.columns, tiles.expanded, tiles.area)));
                },
            )
        })?;
        let seen = seen.borrow();
        assert_eq!(seen.len(), 6, "{seen:?}");
        let (columns, expanded, area) = need(*told.borrow(), "the layout's context")?;
        assert_eq!((columns, expanded), (6, None));
        for (id, rect, allowed) in seen.iter() {
            let built_in = tile(&plain, id)?;
            assert!(same(*rect, built_in), "{id}: {rect:?} vs {built_in:?}");
            assert!(area.contains_rect(*rect), "{id}: {rect:?} in {area:?}");
            assert!(*allowed, "{id}: one level, every tile allowed");
        }
        Ok(())
    }

    /// The layout is told which tiles the session may not use.
    #[test]
    fn the_tile_layout_sees_the_locked_tiles() -> fairing::Result<()> {
        let mut cfg = access_config(&["viewer", "admin"], Some("top"));
        cfg.motion.reduce = true;
        cfg.access
            .gates
            .insert("overlay.open".to_owned(), "viewer".to_owned());
        let seen: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
        let record = Rc::clone(&seen);
        let _h = shade_with(cfg, move |b| {
            b.shade_tile_layout(move |tiles: &ShadeTileLayoutCx<'_>, _: &mut [egui::Rect]| {
                let out = (0..tiles.len())
                    .map(|i| (tiles.id(i).unwrap_or("?").to_owned(), tiles.is_allowed(i)))
                    .collect();
                record.replace(out);
            })
        })?;
        let seen = seen.borrow();
        let wifi = need(seen.iter().find(|(id, _)| id == "tile.wifi"), "tile.wifi")?;
        assert!(!wifi.1, "tile.wifi is past the session: {seen:?}");
        Ok(())
    }

    /// **A place emptied leaves its tile out** — no rect, and a tap where it was does nothing.
    #[test]
    fn an_emptied_place_leaves_its_tile_out() -> fairing::Result<()> {
        let plain = shade_with(config(), |b| b)?;
        let wifi_place = tile(&plain, "tile.wifi")?;
        let mut h = shade_with(config(), |b| {
            b.shade_tile_layout(|tiles: &ShadeTileLayoutCx<'_>, rects: &mut [egui::Rect]| {
                if let Some(rect) = tiles.index_of("tile.wifi").and_then(|i| rects.get_mut(i)) {
                    *rect = egui::Rect::NOTHING;
                }
            })
        })?;
        assert_eq!(h.shell.overlay().tile_rect("tile.wifi"), None);
        assert!(
            h.shell.overlay().tile_rect("tile.bluetooth").is_some(),
            "the others stay"
        );
        h.tap(wifi_place.center());
        h.frames(2);
        assert!(
            h.shell.services().wifi.enabled(),
            "the left-out tile took the tap"
        );
        Ok(())
    }

    /// Wi-Fi a row lower than the built-in rows go.
    fn wifi_lower(tiles: &ShadeTileLayoutCx<'_>, rects: &mut [egui::Rect]) {
        if let Some(rect) = tiles.index_of("tile.wifi").and_then(|i| rects.get_mut(i)) {
            *rect = rect.translate(egui::vec2(0.0, rect.height() + 12.0));
        }
    }

    /// **The rows below the tiles follow the lowest one** — the notification list starts under the
    /// tile the layout put lower, and so does the row a tile opens.
    #[test]
    fn the_rows_below_follow_the_lowest_tile() -> fairing::Result<()> {
        let plain = shade_with(config(), |b| b)?;
        let plain_list = need(plain.shell.overlay().list_rect(), "the list")?;
        let mut h = shade_with(config(), |b| b.shade_tile_layout(wifi_lower))?;
        let wifi = tile(&h, "tile.wifi")?;
        let list = need(h.shell.overlay().list_rect(), "the list")?;
        assert!(
            list.top() > wifi.bottom(),
            "the list under the lowest tile: {list:?} {wifi:?}"
        );
        assert!(
            (list.top() - plain_list.top() - (wifi.height() + 12.0)).abs() < 0.5,
            "a row lower: {list:?} vs {plain_list:?}"
        );
        // The brightness row opens under the lowest tile too.
        let brightness = tile(&h, "tile.brightness")?;
        h.tap(brightness.center());
        h.frames(40);
        let row = need(h.shell.overlay().expanded_rect(), "the brightness row")?;
        assert!(row.top() > wifi.bottom(), "{row:?} under {wifi:?}");
        Ok(())
    }

    /// Pull the shade from the top edge to `to_y`, hold still, let go and settle; where it rests.
    fn pull_to(h: &mut Harness, to_y: f32) -> f32 {
        let x = h.screen_rect().width() * 0.5;
        h.press(egui::pos2(x, 2.0));
        h.frame();
        for i in 1..=10_u8 {
            let t = f32::from(i) / 10.0;
            h.move_to(egui::pos2(x, 2.0 + (to_y - 2.0) * t));
            h.frame();
        }
        for _ in 0..6 {
            h.move_to(egui::pos2(x, to_y));
            h.frame();
        }
        h.release(egui::pos2(x, to_y));
        h.frames(40);
        h.shell.overlay().y()
    }

    /// Where the first pull of a two-step shade stops, with `build`'s hooks.
    fn the_stop(
        build: impl FnOnce(fairing::ShellBuilder) -> fairing::ShellBuilder,
    ) -> fairing::Result<f32> {
        let mut config = config();
        config.overlay.two_step = true;
        let mut h = Harness::from_builder(move |ctx| build(Shell::builder(config)).build(ctx))?;
        h.frames(3);
        let tall = h.screen_rect().height();
        // Open and close once, so the panel has drawn and the stop is known.
        pull_to(&mut h, tall * 0.9);
        pull_to(&mut h, 2.0);
        Ok(pull_to(&mut h, tall * 0.42))
    }

    /// **A two-step shade stops under the lowest tile** the layout placed.
    #[test]
    fn the_two_step_stop_follows_the_lowest_tile() -> fairing::Result<()> {
        let plain = the_stop(|b| b)?;
        let lower = the_stop(|b| b.shade_tile_layout(wifi_lower))?;
        let probe = shade_with(config(), |b| b)?;
        let row = tile(&probe, "tile.wifi")?.height() + 12.0;
        assert!(plain > 1.0, "the plain shade stops: {plain}");
        assert!(
            (lower - plain - row).abs() < 1.0,
            "a row further down: {plain} → {lower} (a row is {row})"
        );
        Ok(())
    }

    /// **The tiles round an expanded one stay where the layout put them** — they do not close up
    /// as the built-in rows do — and the layout is told which tile is out.
    #[test]
    fn the_tiles_round_an_expanded_one_stay_put() -> fairing::Result<()> {
        let told = Rc::new(RefCell::new(None));
        let tell = Rc::clone(&told);
        let mut h = shade_with(config(), move |b| {
            b.shade_tile_layout(move |tiles: &ShadeTileLayoutCx<'_>, _: &mut [egui::Rect]| {
                tell.replace(tiles.expanded);
            })
        })?;
        let wifi = tile(&h, "tile.wifi")?;
        let brightness = tile(&h, "tile.brightness")?;
        h.tap(brightness.center());
        h.frames(40);
        assert_eq!(h.shell.overlay().expanded_tile(), Some("tile.brightness"));
        let wifi_now = tile(&h, "tile.wifi")?;
        assert!(
            same(wifi_now, wifi),
            "Wi-Fi stayed where it was: {wifi_now:?}"
        );
        let out = tile(&h, "tile.brightness")?;
        assert!(
            out.center().y > brightness.bottom(),
            "it went down to its row: {out:?}"
        );
        assert_eq!(
            *told.borrow(),
            Some(2),
            "the layout is told brightness is out"
        );
        // Pressed again, it comes back to its place.
        h.tap(out.center());
        h.frames(40);
        let back = tile(&h, "tile.brightness")?;
        assert!(
            (back.center() - brightness.center()).length() < 1.0,
            "{back:?}"
        );
        Ok(())
    }

    // ------------------------------------------------------------ rung 5: the painters

    /// The marker a painter draws with, and the ground it says — colours nothing else uses.
    const MARK: egui::Color32 = egui::Color32::from_rgb(3, 2, 1);
    const GROUND: egui::Color32 = egui::Color32::from_rgb(1, 2, 3);

    /// What a tile painter was told, the last time it drew each tile.
    #[derive(Debug, Clone)]
    #[allow(clippy::struct_excessive_bools)] // A copy of the painter's four flags.
    struct ToldTile {
        id: String,
        rect: egui::Rect,
        label: String,
        lit: bool,
        live: bool,
        allowed: bool,
        pressed: bool,
    }

    /// A tile painter that fills each tile with the marker and writes down what it was told.
    fn recording_tiles(
        seen: &Rc<RefCell<Vec<ToldTile>>>,
    ) -> impl FnMut(&egui::Painter, &mut ShadeTileCx<'_>) {
        let seen = Rc::clone(seen);
        move |painter: &egui::Painter, tile: &mut ShadeTileCx<'_>| {
            painter.rect_filled(tile.rect, 0.0, MARK);
            let mut seen = seen.borrow_mut();
            seen.retain(|told| told.id != tile.id);
            seen.push(ToldTile {
                id: tile.id.to_owned(),
                rect: tile.rect,
                label: tile.label.to_owned(),
                lit: tile.lit,
                live: tile.live,
                allowed: tile.allowed,
                pressed: tile.pressed,
            });
        }
    }

    fn told(seen: &Rc<RefCell<Vec<ToldTile>>>, id: &str) -> fairing::Result<ToldTile> {
        need(seen.borrow().iter().find(|told| told.id == id).cloned(), id)
    }

    /// Every shape drawn on the next frame, groups opened up.
    fn flat_shapes(h: &mut Harness) -> Vec<egui::Shape> {
        fn walk(shape: egui::Shape, out: &mut Vec<egui::Shape>) {
            if let egui::Shape::Vec(shapes) = shape {
                for shape in shapes {
                    walk(shape, out);
                }
            } else {
                out.push(shape);
            }
        }
        let mut out = Vec::new();
        for clipped in h.frame_shapes() {
            walk(clipped.shape, &mut out);
        }
        out
    }

    fn wifi_turned_off(events: &[ShellEvent]) -> bool {
        events.iter().any(|e| {
            matches!(
                e,
                ShellEvent::SettingChanged { key, value: SettingValue::Bool(false) }
                    if key.0.as_ref() == "wifi.enabled"
            )
        })
    }

    /// **A tile painter draws every tile, and the shell still takes their taps** — each tile told
    /// where it is and what it is called, no built-in puck drawn, and Wi-Fi turning off when its
    /// painted tile is tapped.
    #[test]
    fn a_tile_painter_draws_the_tiles_and_the_shell_takes_their_taps() -> fairing::Result<()> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_tiles(&seen);
        let mut h = shade_with(config(), move |b| b.shade_tile_painter(painter))?;
        h.frames(1);
        assert_eq!(seen.borrow().len(), 6, "{:?}", seen.borrow());
        for told in seen.borrow().iter() {
            let place = tile(&h, &told.id)?;
            assert!(
                same(told.rect, place),
                "{}: {:?} vs {place:?}",
                told.id,
                told.rect
            );
            assert!(!told.label.is_empty(), "{} has no label", told.id);
        }
        let wifi = told(&seen, "tile.wifi")?;
        assert!(wifi.lit && wifi.live && wifi.allowed, "{wifi:?}");
        let place = tile(&h, "tile.wifi")?;
        let drawn = flat_shapes(&mut h);
        assert!(
            drawn.iter().any(
                |s| matches!(s, egui::Shape::Rect(r) if r.fill == MARK && same(r.rect, place))
            ),
            "the painter's Wi-Fi tile is not drawn"
        );
        assert!(
            !drawn
                .iter()
                .any(|s| matches!(s, egui::Shape::Circle(c) if place.contains(c.center))),
            "a built-in puck is still drawn under the painter's tile"
        );
        let _ = h.shell.poll_events();
        h.tap(place.center());
        let events = h.shell.poll_events();
        assert!(wifi_turned_off(&events), "{events:?}");
        Ok(())
    }

    /// **A tile painter is told which tile is held** — and which the session may not use.
    #[test]
    fn a_tile_painter_is_told_what_is_held_and_what_is_locked() -> fairing::Result<()> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_tiles(&seen);
        let mut h = shade_with(config(), move |b| b.shade_tile_painter(painter))?;
        let place = tile(&h, "tile.bluetooth")?;
        h.press(place.center());
        h.frames(2);
        assert!(told(&seen, "tile.bluetooth")?.pressed, "held, not told");
        assert!(!told(&seen, "tile.wifi")?.pressed, "the wrong tile is held");
        h.release(place.center());
        h.frames(2);
        assert!(
            !told(&seen, "tile.bluetooth")?.pressed,
            "let go, still held"
        );

        let mut cfg = access_config(&["viewer", "admin"], Some("top"));
        cfg.motion.reduce = true;
        cfg.access
            .gates
            .insert("overlay.open".to_owned(), "viewer".to_owned());
        let locked = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_tiles(&locked);
        let _h = shade_with(cfg, move |b| b.shade_tile_painter(painter))?;
        assert!(
            !told(&locked, "tile.wifi")?.allowed,
            "Wi-Fi is past the session"
        );
        Ok(())
    }

    /// **A tile going out to its row is painted all the way** — the painter draws brightness while
    /// it shrinks toward the slider's row, its `fade` going down from 1. Motion is on here: reduced,
    /// the tile is at its row on the next frame.
    #[test]
    fn a_tile_going_out_to_its_row_is_painted_all_the_way() -> fairing::Result<()> {
        let fades = Rc::new(RefCell::new(Vec::new()));
        let seen = Rc::clone(&fades);
        let mut h = shade_with(fairing::testing::single_level_access(), move |b| {
            b.shade_tile_painter(move |_: &egui::Painter, tile: &mut ShadeTileCx<'_>| {
                if tile.id == "tile.brightness" {
                    seen.borrow_mut().push(tile.fade);
                }
            })
        })?;
        let brightness = tile(&h, "tile.brightness")?;
        fades.borrow_mut().clear();
        h.tap(brightness.center());
        h.frames(40);
        let fades = fades.borrow();
        assert!(
            fades.iter().any(|&fade| fade > 0.05 && fade < 0.95),
            "brightness is not painted on its way out: {fades:?}"
        );
        Ok(())
    }

    /// What a panel painter was told: its rect, which panel, how it arrives, its alpha, and
    /// whether it is the one being crossed away from.
    type ToldPanel = (egui::Rect, OverlayPanel, OverlayReveal, f32, bool);

    /// A panel painter that fills the panel with the marker, says the ground is [`GROUND`], and
    /// writes down what it was told.
    fn recording_panel(
        seen: &Rc<RefCell<Vec<ToldPanel>>>,
    ) -> impl FnMut(&egui::Painter, &mut ShadePanelCx<'_>) {
        let seen = Rc::clone(seen);
        move |painter: &egui::Painter, panel: &mut ShadePanelCx<'_>| {
            painter.rect_filled(panel.rect, 0.0, MARK);
            panel.ground = GROUND;
            seen.borrow_mut().push((
                panel.rect,
                panel.panel,
                panel.reveal,
                panel.alpha,
                panel.leaving,
            ));
        }
    }

    /// **A panel painter draws the ground, and the content stays** — told the panel's rect, that it
    /// is the one shade hung as a curtain and fully there; the built-in plate not drawn; the tiles
    /// still drawn over it and still pressed.
    #[test]
    fn a_panel_painter_draws_the_ground_and_the_content_stays() -> fairing::Result<()> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_panel(&seen);
        let mut h = shade_with(config(), move |b| b.shade_panel_painter(painter))?;
        seen.borrow_mut().clear();
        h.frames(1);
        let panel = need(h.shell.overlay().frame().panel, "the panel")?;
        let (rect, which, reveal, alpha, leaving) =
            need(seen.borrow().last().copied(), "the painter's call")?;
        assert!(same(rect, panel), "{rect:?} vs {panel:?}");
        assert_eq!(
            (which, reveal, leaving),
            (OverlayPanel::Shade, OverlayReveal::Curtain, false)
        );
        assert!((alpha - 1.0).abs() < 1e-3, "alpha {alpha}");
        let surface = h.shell.theme().color(ColorRole::Surface);
        let drawn = flat_shapes(&mut h);
        assert!(
            drawn.iter().any(
                |s| matches!(s, egui::Shape::Rect(r) if r.fill == MARK && same(r.rect, panel))
            ),
            "the painter's ground is not drawn"
        );
        assert!(
            !drawn.iter().any(
                |s| matches!(s, egui::Shape::Rect(r) if r.fill == surface && same(r.rect, panel))
            ),
            "the built-in plate is still drawn"
        );
        let wifi = tile(&h, "tile.wifi")?;
        let _ = h.shell.poll_events();
        h.tap(wifi.center());
        let events = h.shell.poll_events();
        assert!(wifi_turned_off(&events), "{events:?}");
        Ok(())
    }

    /// **A card is told it is a card, and how far it has arrived** — less than all of it on the
    /// way in, all of it at rest.
    #[test]
    fn a_card_panel_painter_is_told_the_card_and_its_arrival() -> fairing::Result<()> {
        let mut cfg = fairing::testing::single_level_access();
        "card".clone_into(&mut cfg.overlay.reveal);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_panel(&seen);
        let mut h = shade_with(cfg, move |b| b.shade_panel_painter(painter))?;
        // Open means the pull is done; the card goes on resolving for a few frames after it.
        h.frames(60);
        let seen = seen.borrow();
        assert!(
            seen.iter().all(|told| told.2 == OverlayReveal::Card),
            "{seen:?}"
        );
        assert!(
            seen.iter().any(|told| told.3 < 0.99),
            "no frame of the arrival: {seen:?}"
        );
        let last = need(seen.last().copied(), "the painter's call")?;
        assert!((last.3 - 1.0).abs() < 1e-3, "at rest: {last:?}");
        Ok(())
    }

    /// **A card with a panel painter takes no screenshot** — the painter draws the ground and is
    /// handed no frosted backdrop, so the shell does not ask the runner for one to frost.
    #[test]
    fn a_card_panel_painter_takes_no_screenshot() -> fairing::Result<()> {
        let mut cfg = fairing::testing::single_level_access();
        "card".clone_into(&mut cfg.overlay.reveal);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_panel(&seen);
        let mut h = shade_with(cfg, move |b| b.shade_panel_painter(painter))?;
        h.frames(10);
        assert!(!seen.borrow().is_empty(), "the painter was not called");
        assert_eq!(
            h.screenshots_requested(),
            0,
            "a screenshot was taken for a frost nobody draws"
        );
        Ok(())
    }

    /// **A curtain's painter is told what shows** — partway down, `rect` is the part revealed, not
    /// the panel at its full height.
    #[test]
    fn a_curtain_painter_is_told_what_shows() -> fairing::Result<()> {
        let told = Rc::new(RefCell::new(None));
        let tell = Rc::clone(&told);
        let mut h = Harness::from_builder(move |ctx| {
            Shell::builder(config())
                .shade_panel_painter(move |_: &egui::Painter, panel: &mut ShadePanelCx<'_>| {
                    tell.replace(Some((panel.rect, panel.content)));
                })
                .build(ctx)
        })?;
        h.frames(3);
        let x = h.screen_rect().width() * 0.5;
        h.press(egui::pos2(x, 2.0));
        h.frame();
        for i in 1..=6_u8 {
            h.move_to(egui::pos2(x, 2.0 + 30.0 * f32::from(i)));
            h.frame();
        }
        let (rect, content) = need(*told.borrow(), "the painter's call")?;
        assert!(
            rect.height() + 1.0 < content.height(),
            "partway down, all of the panel is told as showing: {rect:?} of {content:?}"
        );
        h.release(egui::pos2(x, 182.0));
        h.frames(2);
        Ok(())
    }

    /// **The list fades into the ground the painter says** — where it overflows, its end is the
    /// painter's colour, not the theme's surface.
    #[test]
    fn the_list_fades_into_the_painters_ground() -> fairing::Result<()> {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_panel(&seen);
        let mut h = shade_with(config(), move |b| b.shade_panel_painter(painter))?;
        for i in 0..30 {
            let title = format!("Notice {i}");
            h.shell.notify(fairing::Notification::new(
                fairing::NotificationId::of(&title),
                title.as_str(),
            ));
        }
        h.frames(5);
        let drawn = flat_shapes(&mut h);
        let faded = drawn.iter().any(
            |s| matches!(s, egui::Shape::Mesh(m) if m.vertices.iter().any(|v| v.color == GROUND)),
        );
        assert!(
            faded,
            "the list's end does not fade into the painter's ground"
        );
        Ok(())
    }

    /// **A locked tile's padlock sits on the painter's ground** — the disc behind it is cut from
    /// the colour the panel painter says, not the theme's surface.
    #[test]
    fn a_locked_tiles_padlock_sits_on_the_painters_ground() -> fairing::Result<()> {
        let mut cfg = access_config(&["viewer", "admin"], Some("top"));
        cfg.motion.reduce = true;
        cfg.access
            .gates
            .insert("overlay.open".to_owned(), "viewer".to_owned());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_panel(&seen);
        let mut h = shade_with(cfg, move |b| b.shade_panel_painter(painter))?;
        let wifi = tile(&h, "tile.wifi")?;
        let drawn = flat_shapes(&mut h);
        assert!(
            drawn.iter().any(|s| matches!(
                s,
                egui::Shape::Circle(c) if c.fill == GROUND && wifi.contains(c.center)
            )),
            "the padlock's disc is not the painter's ground"
        );
        Ok(())
    }

    /// **A split shade crossing paints both panels** — the one crossed to, and the one crossed
    /// away from, told it is leaving.
    #[test]
    fn a_split_crossing_paints_the_leaving_panel_too() -> fairing::Result<()> {
        let mut cfg = fairing::testing::single_level_access();
        "split".clone_into(&mut cfg.overlay.layout);
        let seen = Rc::new(RefCell::new(Vec::new()));
        let painter = recording_panel(&seen);
        let mut h = Harness::from_builder(move |ctx| {
            Shell::builder(cfg).shade_panel_painter(painter).build(ctx)
        })?
        .with_size(1280.0, 800.0);
        h.frames(40);
        let status = need(h.shell.layout().status, "the status bar")?;
        let at = |share: f32| egui::pos2(status.min.x + status.width() * share, status.center().y);
        h.tap(at(0.35));
        h.frames(40);
        assert_eq!(h.shell.overlay().panel(), OverlayPanel::Notifications);
        seen.borrow_mut().clear();
        h.tap(at(0.65));
        h.frames(3);
        let seen = seen.borrow();
        assert!(
            seen.iter()
                .any(|t| t.4 && t.1 == OverlayPanel::Notifications && t.3 < 1.0),
            "the panel crossed away from is not painted leaving: {seen:?}"
        );
        assert!(
            seen.iter()
                .any(|t| !t.4 && t.1 == OverlayPanel::ControlCenter),
            "the panel crossed to is not painted: {seen:?}"
        );
        Ok(())
    }
}
