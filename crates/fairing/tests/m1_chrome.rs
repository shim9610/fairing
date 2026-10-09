//! The M1 integration test for the desktop plus the chrome: the integrator status item, hidden and
//! locked icons and the config page override, along with the layout, gate-filter and tap tests.
//!
//! The rules: an item's position comes from `shell.desktop().icon_rect(id)` · `shell.nav_bar().item_rect(&NavItem)` ·
//! `shell.status_bar().item_rect(id)`, never from a copy of the layout formula. The tests are written as
//! `fn … -> fairing::Result<()>` (the `panic!`/`unwrap` lint).

use fairing::access::AccessEvent;
use fairing::chrome::{NavItem, StatusItem, StatusItemSpec};
use fairing::config::{IconOverride, PageConfig};
use fairing::desktop::Badge;
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{
    action, icon, screen, status_item, Cx, Gate, LaunchAction, ShellConfig, ShellEvent, Slot,
    Visibility,
};
use std::cell::Cell;
use std::rc::Rc;

/// It turns a Rect that was not found into an error rather than a `panic!` (the `panic!` lint).
fn missing(what: &str) -> fairing::Error {
    fairing::Error::Config(format!("could not find {what}"))
}

/// The smallest screen declaration, drawing a label alone.
fn stub(id: &'static str) -> fairing::screen::ScreenDecl {
    screen(id, move |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label(id);
    })
}

/// A one-glyph status bar spec (the gate defaults to the id).
fn text_spec(id: &str, text: &str, priority: i8) -> StatusItemSpec {
    StatusItemSpec {
        id: id.to_owned(),
        item: StatusItem::Text(text.to_owned()),
        gate: Some(Gate::from(id)),
        priority,
        ..StatusItemSpec::default()
    }
}

// ------------------------------------------------------- the desktop layout

/// With no place given, it fills row-first in declaration order.
#[test]
fn desktop_places_declarations_in_order() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        for id in ["a", "b", "c"] {
            sh.add(stub(id).icon(icon::GAUGE).desktop());
        }
    })?;
    h.frames(2);
    let page = h
        .shell
        .desktop()
        .pages()
        .first()
        .ok_or_else(|| missing("page 0"))?;
    assert_eq!(page.slot_at(0, 0).map(|s| s.id.as_str()), Some("a"));
    assert_eq!(page.slot_at(1, 0).map(|s| s.id.as_str()), Some("b"));
    assert_eq!(page.slot_at(2, 0).map(|s| s.id.as_str()), Some("c"));
    assert!(page.slot_at(3, 0).is_none(), "an empty cell stays empty");
    Ok(())
}

/// `[[desktop.pages]]`'s `col`/`row` pins the place.
#[test]
fn config_page_override_places_icon() -> fairing::Result<()> {
    let cfg = ShellConfig::from_toml(
        "[desktop]\ncolumns = 4\nrows = 3\n\n[[desktop.pages]]\nicons = [{ id = \"b\", col = 2, row = 1 }]\n",
    )?;
    let mut h = test_shell(cfg, |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
        sh.add(stub("b").icon(icon::BELL).desktop());
    })?;
    h.frames(2);
    let page = h
        .shell
        .desktop()
        .pages()
        .first()
        .ok_or_else(|| missing("page 0"))?;
    assert_eq!(page.slot_at(2, 1).map(|s| s.id.as_str()), Some("b"));
    assert_eq!(
        page.slot_at(0, 0).map(|s| s.id.as_str()),
        Some("a"),
        "a declaration with no place given goes to the first cell left"
    );
    Ok(())
}

/// The config overrides the label, the icon and `locked` too. An id that is not there is ignored (with a warning).
#[test]
fn config_override_relabels_and_hides_locked_icon() -> fairing::Result<()> {
    let mut cfg = access_config(&["viewer", "admin"], Some("top"));
    cfg.desktop.pages = vec![PageConfig {
        icons: vec![
            IconOverride {
                id: "secret".to_owned(),
                label: Some("Secret".to_owned()),
                icon: Some("lock".to_owned()),
                locked: Some("hide".to_owned()),
                ..IconOverride::default()
            },
            IconOverride {
                id: "nope".to_owned(),
                ..IconOverride::default()
            },
        ],
    }];
    let mut h = test_shell(cfg, |sh| {
        sh.add(
            stub("secret")
                .title("The original label")
                .icon(icon::GAUGE)
                .desktop(),
        );
    })?;
    h.frames(2);
    let page = h
        .shell
        .desktop()
        .pages()
        .first()
        .ok_or_else(|| missing("page 0"))?;
    let slot = page
        .slot_at(0, 0)
        .ok_or_else(|| missing("the secret slot"))?;
    assert_eq!(slot.id, "secret");
    assert_eq!(slot.label, "Secret", "the config overrides the label");
    assert_eq!(slot.visibility, Visibility::Hidden, "locked = hide");
    assert!(
        h.shell.desktop().icon_rect("secret").is_none(),
        "Hidden does not draw where the gate is not passed"
    );
    Ok(())
}

/// `Hidden` does not draw, and a `Locked` tap does not open but asks to be unlocked.
#[test]
fn hidden_icon_not_rendered_and_locked_tap_requests_unlock() -> fairing::Result<()> {
    let mut h = test_shell(access_config(&["viewer", "admin"], Some("top")), |sh| {
        sh.add(
            stub("hidden")
                .icon(icon::GAUGE)
                .desktop()
                .visibility(Visibility::Hidden),
        );
        sh.add(
            stub("locked")
                .icon(icon::BELL)
                .desktop()
                .visibility(Visibility::Locked),
        );
    })?;
    h.frames(2);
    assert!(h.shell.desktop().icon_rect("hidden").is_none());
    let rect = h
        .shell
        .desktop()
        .icon_rect("locked")
        .ok_or_else(|| missing("the locked icon"))?;
    h.tap(rect.center());
    assert!(
        h.shell.workspace().find("locked").is_none(),
        "a locked icon does not open"
    );
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::Access(AccessEvent::UnlockRequested { gate, .. }) if gate.as_str() == "locked"
        )),
        "TapLocked → UnlockRequested"
    );
    Ok(())
}

/// Where an icon points at an action declaration, a tap runs that action.
#[test]
fn desktop_action_icon_tap_runs_action() -> fairing::Result<()> {
    let runs = Rc::new(Cell::new(0_u32));
    let counter = Rc::clone(&runs);
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            action("beep", move |_: &mut Cx| {
                counter.set(counter.get() + 1);
            })
            .title("beep")
            .icon(icon::BELL)
            .desktop(),
        );
    })?;
    h.frames(2);
    let rect = h
        .shell
        .desktop()
        .icon_rect("beep")
        .ok_or_else(|| missing("the beep icon"))?;
    h.tap(rect.center());
    assert_eq!(runs.get(), 1, "one icon tap = one action");
    assert!(
        h.shell.workspace().is_home(),
        "an action does not open a screen"
    );
    Ok(())
}

/// A declaration given both `.desktop()` and `.dock()` is put **in the dock alone**: the same id in
/// two cells makes `icon_rect(id)` (the A2 starting point) ambiguous. It differs from the "both" a
/// `.dock()  // in the dock too` comment would suggest, so it is pinned down here.
#[test]
fn desktop_and_dock_together_place_only_in_the_dock() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("both").icon(icon::GAUGE).desktop().dock());
        sh.add(stub("grid").icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    let dock: Vec<&str> = h
        .shell
        .desktop()
        .dock()
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    let grid: Vec<&str> = h
        .shell
        .desktop()
        .icons()
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(dock, ["both"]);
    assert_eq!(
        grid,
        ["grid"],
        "an id that went to the dock is not put on the grid again"
    );
    assert!(h.shell.desktop().icon_rect("both").is_some());
    Ok(())
}

/// The dock is not the bottom band alone. A dock cell and a grid cell **must not overlap** at any
/// of the five places — the four edges and **a line crossing the desktop** — because overlapping has the labels
/// treading on each other. The cell Rects come from [`SlotCx`] (the real cell including the label, so a stronger
/// check than the icon Rect).
#[test]
fn every_dock_placement_keeps_the_grid_clear() -> fairing::Result<()> {
    use fairing::desktop::{Axis, DockPlacement, SlotCx};
    use fairing::gesture::Edge;
    use std::cell::RefCell;

    /// The (is-dock, cell) list drawn at one place.
    fn cells(placement: DockPlacement) -> fairing::Result<Vec<(bool, egui::Rect)>> {
        let seen: Rc<RefCell<Vec<(bool, egui::Rect)>>> = Rc::new(RefCell::new(Vec::new()));
        let seen_in = Rc::clone(&seen);
        let mut config = single_level_access();
        config.motion.reduce = true;
        let mut h = Harness::from_builder(move |ctx| {
            let mut shell = fairing::Shell::builder(config)
                .services(fairing::Services::null())
                .slot_painter(move |_ui: &mut egui::Ui, slot: SlotCx<'_>| {
                    seen_in.borrow_mut().push((slot.in_dock, slot.cell));
                })
                .build(ctx)?;
            for id in ["d1", "d2"] {
                shell.add(stub(id).icon(icon::GAUGE).dock());
            }
            for id in ["g1", "g2", "g3", "g4"] {
                shell.add(stub(id).icon(icon::GAUGE).desktop());
            }
            shell.desktop_mut().set_dock_placement(placement);
            Ok(shell)
        })?;
        // The first frame settles the layout and the second draws with it.
        h.frames(2);
        seen.borrow_mut().clear();
        h.frames(1);
        let out = seen.borrow().clone();
        Ok(out)
    }

    for placement in [
        DockPlacement::Edge(Edge::Bottom),
        DockPlacement::Edge(Edge::Top),
        DockPlacement::Edge(Edge::Left),
        DockPlacement::Edge(Edge::Right),
        DockPlacement::Band {
            axis: Axis::Horizontal,
            at: 0.66,
        },
        DockPlacement::Band {
            axis: Axis::Vertical,
            at: 0.3,
        },
    ] {
        let drawn = cells(placement)?;
        let dock: Vec<egui::Rect> = drawn.iter().filter(|(d, _)| *d).map(|(_, r)| *r).collect();
        let grid: Vec<egui::Rect> = drawn.iter().filter(|(d, _)| !*d).map(|(_, r)| *r).collect();
        assert_eq!(dock.len(), 2, "{placement:?}: it draws two dock cells");
        assert_eq!(grid.len(), 4, "{placement:?}: it draws four grid cells");
        for d in &dock {
            for g in &grid {
                assert!(
                    !d.intersects(*g),
                    "{placement:?}: the dock cell {d:?} overlaps the grid cell {g:?}"
                );
            }
        }
    }
    Ok(())
}

/// The dock takes the config list's order first and the `.dock()` declarations go on the end.
/// **There is no cap on the count** — the five-cell cap was a phone convention and M2b removed it.
#[test]
fn dock_follows_config_order_then_declarations() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.desktop.dock = ["f", "e", "d", "c", "b"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let mut h = test_shell(cfg, |sh| {
        sh.add(stub("a").icon(icon::GAUGE).dock());
        for id in ["b", "c", "d", "e", "f"] {
            sh.add(stub(id).icon(icon::GAUGE));
        }
    })?;
    h.frames(2);
    let ids: Vec<&str> = h
        .shell
        .desktop()
        .dock()
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["f", "e", "d", "c", "b", "a"],
        "the config order comes first and the `.dock()` declarations go on the end (no cap)"
    );
    assert!(
        h.shell.desktop().icon_rect("f").is_some(),
        "a dock icon records a Rect too"
    );
    assert!(
        h.shell.desktop().icons().is_empty(),
        "an id that went to the dock is not put on the grid again"
    );
    assert!(
        h.shell.desktop().icon_rect("a").is_some(),
        "with no cap, the `.dock()` declaration a gets a place in the dock too"
    );
    Ok(())
}

/// The badge: `set_badge` is `false` for an id that is not there and `true` for one that is, and it survives a rebuild.
#[test]
fn badge_survives_rebuild_and_unknown_id_is_false() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    assert!(!h.shell.set_badge("zzz", Some(&Badge::Count(1))));
    assert!(h.shell.set_badge("a", Some(&Badge::Count(3))));
    h.frames(1);
    // Putting a new declaration in makes the registry dirty → a rebuild. The badge has to survive.
    h.shell.add(stub("b").icon(icon::BELL).desktop());
    h.frames(2);
    let page = h
        .shell
        .desktop()
        .pages()
        .first()
        .ok_or_else(|| missing("page 0"))?;
    let slot = page.slot_at(0, 0).ok_or_else(|| missing("the a slot"))?;
    assert_eq!(slot.badge, Some(Badge::Count(3)));
    assert!(h.shell.set_badge("a", None));
    h.frames(1);
    let page = h
        .shell
        .desktop()
        .pages()
        .first()
        .ok_or_else(|| missing("page 0"))?;
    assert!(page
        .slot_at(0, 0)
        .ok_or_else(|| missing("the a slot"))?
        .badge
        .is_none());
    Ok(())
}

/// Overflowing one page goes to the next, and tapping an indicator changes the page
/// (the sideways swipe is M2 · A4).
#[test]
fn page_indicator_tap_switches_page() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.desktop.columns = 2;
    cfg.desktop.rows = 1;
    let mut h = test_shell(cfg, |sh| {
        for id in ["a", "b", "c", "d"] {
            sh.add(stub(id).icon(icon::GAUGE).desktop());
        }
    })?;
    h.frames(2);
    assert_eq!(h.shell.desktop().pages().len(), 2);
    assert_eq!(h.shell.desktop().page(), 0);
    assert!(h.shell.desktop().icon_rect("a").is_some());
    assert!(
        h.shell.desktop().icon_rect("c").is_none(),
        "an icon on another page is not drawn"
    );
    let dot = h
        .shell
        .desktop()
        .page_indicator_rect(1)
        .ok_or_else(|| missing("the page 1 indicator"))?;
    h.tap(dot.center());
    assert_eq!(h.shell.desktop().page(), 1);
    h.frames(1);
    assert!(h.shell.desktop().icon_rect("c").is_some());
    assert!(h.shell.desktop().icon_rect("a").is_none());
    Ok(())
}

/// A7: the visual response starts on the first pressed frame (a scale animation → a repaint request), and
/// letting go goes back and idles again. Looked at with `reduce = false` so the animations are on.
#[test]
fn desktop_press_feedback_starts_on_first_frame_and_settles() -> fairing::Result<()> {
    // A locked icon is pressed so it does not mix with the A2 (the open transition) — a tap opens no screen.
    let mut h = Harness::new(
        access_config(&["viewer", "admin"], Some("top")),
        fairing::Services::null(),
    )?;
    h.shell.add(
        stub("locked")
            .icon(icon::GAUGE)
            .desktop()
            .visibility(Visibility::Locked),
    );
    // Warm up until the glyph raster and the Area sizing pass are done, then check for idle.
    h.run_for(0.3);
    assert!(!h.repaint_requested, "idle, it does not ask for a repaint");
    let rect = h
        .shell
        .desktop()
        .icon_rect("locked")
        .ok_or_else(|| missing("the locked icon"))?;
    h.press(rect.center());
    h.frame();
    assert!(
        h.shell.desktop().is_pressed(),
        "the press feedback starts on the very frame it was pressed (≤ 16 ms). egui's `Response` is one pass \
         late, so the raw pointer state is looked at alongside it"
    );
    h.release(rect.center());
    h.run_for(0.4);
    assert!(
        !h.shell.desktop().is_pressed(),
        "letting go comes back to 1"
    );
    assert!(
        !h.repaint_requested,
        "back to 0 fps once the release animation is over"
    );
    assert!(h.shell.desktop().icon_rect("locked").is_some());
    Ok(())
}

// ------------------------------------------------------- the status bar

/// An integrator's item is called and leaves a Rect behind. A built-in item comes out with `remove`.
#[test]
fn status_bar_custom_item_and_builtin_remove() -> fairing::Result<()> {
    let calls = Rc::new(Cell::new(0_u32));
    let counter = Rc::clone(&calls);
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(status_item(
            "temp",
            Slot::Right,
            move |ui: &mut egui::Ui, _: &mut Cx| {
                counter.set(counter.get() + 1);
                ui.label("42C");
            },
        ));
    })?;
    h.frames(3);
    assert!(calls.get() >= 3, "it draws every frame");
    assert!(h.shell.status_bar().item_rect("temp").is_some());
    assert!(h.shell.status_bar().item_rect("status.clock").is_some());
    assert!(
        h.shell.remove("status.clock"),
        "the built-in item is removed"
    );
    h.frames(1);
    assert!(!h.shell.remove("status.clock"), "there is no second one");
    assert!(h.shell.status_bar().item_rect("status.clock").is_none());
    assert!(
        h.shell.status_bar().item_rect("temp").is_some(),
        "the other items are as they were"
    );
    Ok(())
}

/// The left, centre and right slot order. The left starts from the left, the right from the right edge, and the centre is centred.
#[test]
fn status_bar_slots_are_laid_out_left_center_right() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    {
        let bar = h.shell.status_bar_mut();
        bar.left = vec![text_spec("t.left", "LEFT", 0)];
        bar.center = vec![text_spec("t.center", "CENTER", 0)];
        bar.right = vec![text_spec("t.right", "RIGHT", 0)];
    }
    h.frames(2);
    let bar = h.shell.status_bar();
    let left = bar.item_rect("t.left").ok_or_else(|| missing("t.left"))?;
    let center = bar
        .item_rect("t.center")
        .ok_or_else(|| missing("t.center"))?;
    let right = bar.item_rect("t.right").ok_or_else(|| missing("t.right"))?;
    assert!(left.min.x < center.min.x, "left < centre");
    assert!(center.max.x < right.min.x, "centre < right");
    let screen = h.screen_rect();
    assert!(
        (left.min.x - screen.min.x) < (screen.max.x - right.max.x) + 1.0,
        "the left sticks to the left margin"
    );
    let edge_pad = h.shell.theme().metrics.status_edge_pad;
    assert!(
        (screen.max.x - right.max.x) <= edge_pad + 1.0,
        "the right sticks to the right edge"
    );
    let center_offset = (center.center().x - screen.center().x).abs();
    assert!(
        center_offset < 4.0,
        "the centre is centred ({center_offset})"
    );
    Ok(())
}

/// Where the width is short it folds the lower-`priority` items first.
#[test]
fn status_bar_collapses_low_priority_first() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    {
        let bar = h.shell.status_bar_mut();
        bar.left.clear();
        bar.center.clear();
        bar.right = vec![
            text_spec("t.low", "LOWLOWLOWLOW", -1),
            text_spec("t.mid", "MIDMIDMIDMID", 0),
            text_spec("t.high", "HIGHHIGHHIGH", 1),
        ];
    }
    // At a generous width all three show.
    h.frames(2);
    for id in ["t.low", "t.mid", "t.high"] {
        assert!(
            h.shell.status_bar().item_rect(id).is_some(),
            "{id} shows on a wide screen"
        );
    }
    // On a narrow screen the lowest priority folds first.
    let mut narrow =
        Harness::new(single_level_access(), fairing::Services::null())?.with_size(160.0, 600.0);
    {
        let bar = narrow.shell.status_bar_mut();
        bar.left.clear();
        bar.center.clear();
        bar.right = vec![
            text_spec("t.low", "LOWLOWLOWLOW", -1),
            text_spec("t.mid", "MIDMIDMIDMID", 0),
            text_spec("t.high", "HIGHHIGHHIGH", 1),
        ];
    }
    narrow.frames(2);
    let bar = narrow.shell.status_bar();
    assert!(
        bar.item_rect("t.high").is_some(),
        "the highest priority stays"
    );
    assert!(bar.item_rect("t.low").is_none(), "the lowest folds first");
    Ok(())
}

/// An item that does not pass the gate is not rendered.
#[test]
fn status_bar_gate_filters_items() -> fairing::Result<()> {
    let mut cfg = access_config(&["viewer", "admin"], Some("top"));
    // Only the `open` gate is assigned to the lowest level (viewer). The rest are default_gate = top.
    cfg.access
        .gates
        .insert("open".to_owned(), "viewer".to_owned());
    let mut h = test_shell(cfg, |sh| {
        sh.add(status_item(
            "open",
            Slot::Left,
            |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("open");
            },
        ));
        sh.add(status_item(
            "secret",
            Slot::Left,
            |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("secret");
            },
        ));
    })?;
    h.frames(2);
    assert!(
        h.shell.status_bar().item_rect("open").is_some(),
        "a gate assigned to the viewer level passes"
    );
    assert!(
        h.shell.status_bar().item_rect("secret").is_none(),
        "an unassigned gate is refused, default_gate being top"
    );
    Ok(())
}

/// An item tap → `ItemTapped(id)` → the shell runs the `tap_action`.
#[test]
fn status_bar_item_tap_runs_tap_action() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("wifi-settings"));
    })?;
    {
        let bar = h.shell.status_bar_mut();
        bar.left = vec![text_spec("t.net", "NET", 0)];
        bar.center.clear();
        bar.right.clear();
        if let Some(spec) = bar.left.first_mut() {
            spec.tap_action = Some(LaunchAction::open("wifi-settings"));
        }
    }
    h.frames(2);
    let rect = h
        .shell
        .status_bar()
        .item_rect("t.net")
        .ok_or_else(|| missing("t.net"))?;
    assert!(h.shell.status_bar().tap_action("t.net").is_some());
    h.tap(rect.center());
    assert!(
        h.shell.workspace().find("wifi-settings").is_some(),
        "an item tap runs the tap_action"
    );
    Ok(())
}

// ------------------------------------------------------------ the nav bar

/// Back → pop, home → the desktop. The touch target is 48 px or more.
#[test]
fn nav_back_pops_and_home_returns_to_desktop() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
        sh.add(stub("b"));
    })?;
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("b"));
    h.frames(2);
    assert!(h.shell.workspace().find("b").is_some());

    let back = h
        .shell
        .nav_bar()
        .item_rect(&NavItem::Back)
        .ok_or_else(|| missing("the back button"))?;
    assert!(
        back.width() >= 48.0 && back.height() >= 48.0,
        "the minimum touch target, 48 px ({back:?})"
    );
    h.tap(back.center());
    assert!(h.shell.workspace().find("b").is_none(), "back = pop");
    assert!(!h.shell.workspace().is_home());

    let home = h
        .shell
        .nav_bar()
        .item_rect(&NavItem::Home)
        .ok_or_else(|| missing("the home button"))?;
    h.tap(home.center());
    assert!(h.shell.workspace().is_home());
    assert!(
        h.shell
            .poll_events()
            .iter()
            .any(|e| matches!(e, ShellEvent::WentHome)),
        "the WentHome event"
    );
    Ok(())
}

/// An item not in the config is not drawn, and a `recents` tap **reports** rather than
/// changing the workspace itself.
#[test]
fn nav_items_follow_config_and_recents_reports() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.nav_bar.items = vec!["back".to_owned(), "home".to_owned()];
    let mut h = test_shell(cfg, |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    assert!(h.shell.nav_bar().item_rect(&NavItem::Back).is_some());
    assert!(h.shell.nav_bar().item_rect(&NavItem::Home).is_some());
    assert!(
        h.shell.nav_bar().item_rect(&NavItem::Recents).is_none(),
        "an item not in the config is not drawn"
    );

    let mut cfg = single_level_access();
    cfg.nav_bar.items = vec!["recents".to_owned()];
    let mut h = test_shell(cfg, |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    let _ = h.shell.poll_events();
    let recents = h
        .shell
        .nav_bar()
        .item_rect(&NavItem::Recents)
        .ok_or_else(|| missing("the recents button"))?;
    h.tap(recents.center());
    h.frames(2);
    assert!(
        h.shell
            .poll_events()
            .contains(&ShellEvent::OverviewRequested),
        "the tap leaves as an event for the integrator"
    );
    assert!(
        h.shell.workspace().find("a").is_some() && !h.shell.workspace().is_home(),
        "and the shell itself moves nothing — there is no overview in the crate"
    );
    Ok(())
}

/// **The shell does not decide that a control cannot work.**
///
/// `Recents` and `Split` were drawn grey and their taps thrown away, because the crate's own
/// overview and split panes are M5. But the crate having nowhere to go does not mean the **device**
/// has nowhere to go — the icon is the shell's to draw and what it does is the integrator's to say.
/// So both are live and both report. `Back` stays the one item the shell dims, because "is there a
/// stack behind you" is a fact the shell owns.
#[test]
fn recents_and_split_are_live_and_report() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.nav_bar.items = ["back", "home", "recents", "split"]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
    let mut h = test_shell(cfg, |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    let nav = h.shell.nav_bar();
    assert!(
        !nav.item_enabled(&NavItem::Back),
        "back is disabled at home"
    );
    assert!(nav.item_enabled(&NavItem::Home));
    assert!(
        nav.item_enabled(&NavItem::Recents),
        "no longer greyed out — the device may have an overview"
    );
    assert!(nav.item_enabled(&NavItem::Split));

    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(3);
    assert!(
        h.shell.nav_bar().item_enabled(&NavItem::Back),
        "back is enabled where there is a stack"
    );
    let _ = h.shell.poll_events();

    let split = h
        .shell
        .nav_bar()
        .item_rect(&NavItem::Split)
        .ok_or_else(|| missing("the split button"))?;
    h.tap(split.center());
    h.frames(2);
    assert!(
        h.shell.poll_events().contains(&ShellEvent::SplitRequested),
        "the split tap reports too"
    );
    assert!(
        h.shell.workspace().find("a").is_some() && !h.shell.workspace().is_home(),
        "and the shell moves nothing of its own accord"
    );
    Ok(())
}

/// **Lock, log out and the overview are reported, never swallowed**.
///
/// `Shell::launch` had a catch-all arm that wrote these three to the log and dropped them, so the
/// shade footer's lock button, the `tile.lock` quick tile and `ShellHandle::logout` were all
/// controls that did nothing and gave the integrator no way to make them do anything. The arms are
/// written out now, so a new `LaunchAction` cannot be swallowed the same way.
#[test]
fn lock_logout_and_overview_leave_as_events() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("a").icon(icon::GAUGE).desktop());
    })?;
    h.frames(2);
    let _ = h.shell.poll_events();

    for (action, want) in [
        (LaunchAction::Lock, ShellEvent::LockRequested),
        (LaunchAction::Logout, ShellEvent::LogoutRequested),
        (LaunchAction::OpenOverview, ShellEvent::OverviewRequested),
    ] {
        h.shell.launch(action.clone());
        let events = h.shell.poll_events();
        assert!(
            events.contains(&want),
            "{action:?} has to reach the integrator, got {events:?}"
        );
    }

    // `ShellHandle::logout` takes the same road — it wrote "logout lands in M3" and stopped.
    h.shell.handle().logout();
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events.contains(&ShellEvent::LogoutRequested),
        "the handle reports as well, got {events:?}"
    );
    Ok(())
}

/// Every line of text drawn on the next frame.
fn drawn_lines(h: &mut Harness) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    walk(shape, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, &mut out);
    }
    out
}

/// A shell whose only status bar item is the clock, on a stopped clock at a known moment.
#[cfg(feature = "mock")]
fn clock_shell(at: fairing::time::WallTime) -> fairing::Result<Harness> {
    clock_shell_with(at, "hm")
}

/// The same, with a chosen `[status_bar] clock_format`.
#[cfg(feature = "mock")]
fn clock_shell_with(at: fairing::time::WallTime, format: &str) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.status_bar.right = vec!["status.clock".to_owned()];
    config.status_bar.left = Vec::new();
    config.status_bar.center = Vec::new();
    format.clone_into(&mut config.status_bar.clock_format);
    Harness::from_builder(move |ctx| {
        let services = fairing::services::Services::builder()
            .clock(fairing::services::mock::MockClock::fixed(at))
            .build();
        fairing::Shell::builder(config)
            .services(services)
            .build(ctx)
    })
}

/// **The 24-hour setting reaches the status bar clock**.
///
/// `settings.datetime` has always had the switch, and it wrote `ui.clock_12h` — under a bare string
/// literal, with **nothing anywhere reading it**. `[status_bar] clock_format` fixed the hour
/// convention at build time, so the switch appeared to do nothing at all.
#[cfg(feature = "mock")]
#[test]
fn the_24_hour_setting_reaches_the_status_bar_clock() -> fairing::Result<()> {
    use fairing::settings::{keys, SettingValue};

    // 2024-01-01T00:30:00Z at +09:00 → 09:30 local.
    let at = fairing::time::WallTime {
        utc_secs: 1_704_069_000,
        offset_min: 540,
    };
    let mut h = clock_shell(at)?;
    h.frames(2);
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "09:30"),
        "the configured shape is 24-hour to begin with: {lines:?}"
    );

    h.shell
        .set_setting(keys::UI_CLOCK_12H.into(), SettingValue::Bool(true));
    h.frames(2);
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "9:30 AM"),
        "the switch has to reach the bar on the same clock minute: {lines:?}"
    );
    assert!(
        !lines.iter().any(|line| line == "09:30"),
        "and the 24-hour spelling is gone: {lines:?}"
    );

    h.shell
        .set_setting(keys::UI_CLOCK_12H.into(), SettingValue::Bool(false));
    h.frames(2);
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "09:30"),
        "and back again: {lines:?}"
    );
    Ok(())
}

/// **The time source can be connected while the shell runs**.
///
/// `Services::builder().clock(..)` sets it before the shell exists; `Shell::set_clock` is the same
/// thing afterwards, which is what a device that learns its time from a PLC or an NTP sync needs.
#[cfg(feature = "mock")]
#[test]
fn the_clock_backend_can_be_replaced_at_run_time() -> fairing::Result<()> {
    let mut h = clock_shell(fairing::time::WallTime {
        utc_secs: 1_704_069_000,
        offset_min: 540,
    })?;
    h.frames(2);
    assert!(drawn_lines(&mut h).iter().any(|line| line == "09:30"));

    // The same instant, told to the shell as UTC+00:00 by a clock handed over now.
    h.shell.set_clock(fairing::services::mock::MockClock::fixed(
        fairing::time::WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 0,
        },
    ));
    h.frames(2);
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "00:30"),
        "the bar reads the clock that is installed now: {lines:?}"
    );
    Ok(())
}

/// **A 12-hour `clock_format` is not overruled by a setting nobody wrote**.
///
/// The `ui.clock_12h` setting decides the hour convention, and reading it as "absent means
/// 24-hour" turned `clock_format = "hm12"` — an explicit choice by whoever built the device — back
/// into `09:30` on the first frame. The setting is seeded from the configured shape at startup, so
/// the config is the starting point and the switch on `settings.datetime` is what changes it.
#[cfg(feature = "mock")]
#[test]
fn a_twelve_hour_clock_format_survives_startup() -> fairing::Result<()> {
    use fairing::settings::{keys, SettingValue};

    let at = fairing::time::WallTime {
        utc_secs: 1_704_069_000,
        offset_min: 540,
    };
    for (format, want) in [
        ("hm12", "9:30 AM"),
        ("hms12", "9:30:00 AM"),
        ("date_hm12", "01-01 9:30 AM"),
        ("hm", "09:30"),
        ("hms", "09:30:00"),
        ("date_hm", "01-01 09:30"),
    ] {
        let mut h = clock_shell_with(at, format)?;
        h.frames(2);
        let lines = drawn_lines(&mut h);
        assert!(
            lines.iter().any(|line| line == want),
            "clock_format = {format:?} has to draw {want:?}, got {lines:?}"
        );
    }

    // And the switch still wins once it is thrown, in both directions.
    let mut h = clock_shell_with(at, "hm12")?;
    h.frames(2);
    assert_eq!(
        h.shell.settings().get(&keys::UI_CLOCK_12H.into()),
        Some(&SettingValue::Bool(true)),
        "the setting is seeded from the configured shape"
    );
    h.shell
        .set_setting(keys::UI_CLOCK_12H.into(), SettingValue::Bool(false));
    h.frames(2);
    let lines = drawn_lines(&mut h);
    assert!(
        lines.iter().any(|line| line == "09:30"),
        "turning the switch off reaches a 12-hour configured bar too: {lines:?}"
    );
    assert_eq!(
        h.shell.status_bar().drawn_clock(),
        Some(fairing::time::ClockFormat::Hm),
        "`drawn_clock` says what was drawn, not what was configured — the shell picks its idle \
         repaint interval from it"
    );
    assert_eq!(
        h.shell.status_bar().configured_clock(),
        Some(fairing::time::ClockFormat::Hm12),
        "and `configured_clock` is still the config's own shape"
    );
    Ok(())
}
