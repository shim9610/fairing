//! The icon rail (the kiosk layout).
//!
//! What it checks: whether the rail really does **narrow the content** (a rail a screen covers is not a rail),
//! whether it stands aside on a narrow panel, whether home is left empty, and whether it is off by default.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx};

/// A shell with three icons on it. `rail` is the `[desktop] rail` value.
fn rail_shell(rail: &str, w: f32, h: f32) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    rail.clone_into(&mut config.desktop.rail);
    let mut h = Harness::new(config, fairing::services::Services::null())?.with_size(w, h);
    for id in ["menu", "orders", "report"] {
        h.shell.add(
            screen(id, |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("body");
            })
            .title(id)
            .icon(fairing::icons::IconRef::Builtin("grid"))
            .desktop(),
        );
    }
    h.frames(4);
    Ok(h)
}

/// **It is off by default.** The rail is an opt-in layout.
#[test]
fn the_default_is_no_rail() -> fairing::Result<()> {
    let h = rail_shell("none", 1024.0, 600.0)?;
    assert!(h.shell.layout().rail.is_none());
    Ok(())
}

/// The rail really does **narrow the content** — a rail a screen covers is not a rail.
#[test]
fn the_rail_narrows_the_content() -> fairing::Result<()> {
    let plain = rail_shell("none", 1024.0, 600.0)?;
    let railed = rail_shell("left", 1024.0, 600.0)?;
    let (a, b) = (plain.shell.layout().content, railed.shell.layout().content);
    assert!(
        b.width() < a.width(),
        "the content did not narrow: {a:?} → {b:?}"
    );
    let rail = railed.shell.layout().rail;
    let Some(rail) = rail else {
        return Err(fairing::Error::Config("there is no rail".to_owned()));
    };
    // The rail and the content do not overlap; they meet.
    assert!((rail.max.x - b.min.x).abs() < 0.5, "{rail:?} vs {b:?}");
    Ok(())
}

/// Placed on the right it sticks to the other side.
#[test]
fn the_rail_can_sit_on_the_right() -> fairing::Result<()> {
    let h = rail_shell("right", 1024.0, 600.0)?;
    let (rail, content) = (h.shell.layout().rail, h.shell.layout().content);
    let Some(rail) = rail else {
        return Err(fairing::Error::Config("there is no rail".to_owned()));
    };
    assert!(rail.min.x > content.min.x, "{rail:?} vs {content:?}");
    assert!((content.max.x - rail.min.x).abs() < 0.5);
    Ok(())
}

/// The rail is **below the status bar and the nav bar** — crossing a bar has the bar drawn over the rail and the icons cut off.
#[test]
fn the_rail_stays_below_the_bars() -> fairing::Result<()> {
    let h = rail_shell("left", 1024.0, 600.0)?;
    let layout = h.shell.layout();
    let Some(rail) = layout.rail else {
        return Err(fairing::Error::Config("there is no rail".to_owned()));
    };
    if let Some(status) = layout.status {
        assert!(
            rail.top() >= status.bottom() - 0.5,
            "{rail:?} vs {status:?}"
        );
    }
    if let Some(nav) = layout.nav {
        assert!(rail.bottom() <= nav.top() + 0.5, "{rail:?} vs {nav:?}");
    }
    Ok(())
}

/// **On a narrow panel it stands aside.** Narrower than two icon cells, the rail plus a screen
/// leaves neither usable.
#[test]
fn a_narrow_panel_drops_the_rail() -> fairing::Result<()> {
    let h = rail_shell("left", 160.0, 320.0)?;
    assert!(
        h.shell.layout().rail.is_none(),
        "it is narrow and the rail stayed"
    );
    Ok(())
}

/// The rail never takes more than half the content — the screen side is the main thing.
#[test]
fn the_rail_never_takes_more_than_half() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    "left".clone_into(&mut config.desktop.rail);
    config.desktop.rail_width = 0.5;
    let mut h = Harness::new(config, fairing::services::Services::null())?.with_size(1024.0, 600.0);
    h.frames(3);
    let layout = h.shell.layout();
    let Some(rail) = layout.rail else {
        return Err(fairing::Error::Config("there is no rail".to_owned()));
    };
    assert!(
        rail.width() <= layout.content.width() + 0.5,
        "the rail is wider than the content: {rail:?} vs {:?}",
        layout.content
    );
    Ok(())
}

/// **Home is not left empty.** There is no such thing as a kiosk with only a menu and nothing beside it — the first item opens by itself.
#[test]
fn the_rail_opens_its_first_entry() -> fairing::Result<()> {
    let h = rail_shell("left", 1024.0, 600.0)?;
    assert!(!h.shell.workspace().is_home(), "it stayed at home");
    assert_eq!(
        h.shell
            .workspace()
            .focused()
            .map(fairing::workspace::Instance::decl_id),
        Some("menu")
    );
    Ok(())
}

/// **Home is not empty.** Pressing the home button does not leave the space beside the rail blank either.
///
/// Which screen it goes back to is the workspace's rule as it is — it resumes the first item's task, so having gone
/// deeper inside that task it comes back to that place (the same as an Android launcher). What the rail guarantees
/// goes as far as "an empty home is never seen".
#[test]
fn going_home_never_leaves_an_empty_pane() -> fairing::Result<()> {
    let mut h = rail_shell("left", 1024.0, 600.0)?;
    h.shell.launch(fairing::LaunchAction::open("report"));
    h.frames(3);
    h.shell.home();
    h.frames(12);
    assert!(
        !h.shell.workspace().is_home(),
        "going home left the space beside the rail empty"
    );
    Ok(())
}

/// An unknown value warns and turns off — a typo does not stop the shell.
#[test]
fn an_unknown_rail_value_falls_back_to_none() -> fairing::Result<()> {
    let h = rail_shell("middle", 1024.0, 600.0)?;
    assert!(h.shell.layout().rail.is_none());
    Ok(())
}
