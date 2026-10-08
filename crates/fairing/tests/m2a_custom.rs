//! The M2a integration test — the compile-time customisation hooks.
//!
//! What it checks: [`fairing::Shell::builder`]'s theme injection, the `nav_item` declaration, the status bar and
//! nav bar painters, the desktop cell painter and the `[theme.palette]` override. The contract is that all of it
//! has to be possible **without an integrator touching the crate**, so the test uses the public API alone.
//!
//! The rules are the other M1 tests': positions come from `icon_rect` / `item_rect` rather than a copy of the
//! layout formula, and it is written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

use fairing::chrome::NavItem;
use fairing::desktop::SlotCx;
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::theme::Palette;
use fairing::{icon, nav_item, screen, ColorRole, Cx, Error, Services, Shell, ShellConfig, Theme};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// It turns a value that was not found into an error rather than a `panic!` (the lint).
fn missing(what: &str) -> Error {
    Error::Config(format!("could not find {what}"))
}

/// The marker colour that counts whether a painter drew (a value used nowhere else).
const MARK: egui::Color32 = egui::Color32::from_rgb(3, 2, 1);

/// How many marker shapes were drawn this frame.
fn marks(shapes: &[egui::epaint::ClippedShape]) -> usize {
    shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) => rect.fill == MARK,
            _ => false,
        })
        .count()
}

/// The smallest screen declaration, drawing a label alone (it has an icon → it goes on the desktop).
fn icon_screen(id: &'static str) -> fairing::screen::ScreenDecl {
    screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
        ui.label(id);
    })
    .title(id)
    .icon(icon::GAUGE)
    .desktop()
}

// ─────────────────────────────────────────────────────────────────────────────
// The nav_item declaration (the other declarations have the same shape)
// ─────────────────────────────────────────────────────────────────────────────

/// A `nav_item(id, ..)` closure is called in its own item's place, and that place is looked up with `item_rect`.
#[test]
fn nav_item_closure_draws_in_its_cell_and_records_the_rect() -> fairing::Result<()> {
    let calls = Rc::new(Cell::new(0u32));
    let seen = Rc::new(Cell::new(egui::Rect::NOTHING));
    let mut config = single_level_access();
    config.nav_bar.items = vec!["back".to_owned(), "home".to_owned(), "kbd".to_owned()];
    let (calls_in, seen_in) = (Rc::clone(&calls), Rc::clone(&seen));
    let mut h = test_shell(config, move |shell| {
        shell.add(nav_item(
            "kbd",
            move |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
                calls_in.set(calls_in.get() + 1);
                seen_in.set(ui.max_rect());
                ui.painter().rect_filled(ui.max_rect(), 0.0, MARK);
            },
        ));
    })?;
    h.frames(2);
    assert!(
        calls.get() >= 2,
        "it is called every frame: {}",
        calls.get()
    );
    let rect = h
        .shell
        .nav_bar()
        .item_rect(&NavItem::Custom("kbd".to_owned()))
        .ok_or_else(|| missing("the nav_item `kbd`'s Rect"))?;
    assert_eq!(
        seen.get(),
        rect,
        "the Ui the closure got is the item's place"
    );
    // It is on the same row as the built-in items and does not overlap them.
    let home = h
        .shell
        .nav_bar()
        .item_rect(&NavItem::Home)
        .ok_or_else(|| missing("the home item's Rect"))?;
    assert!((rect.min.y - home.min.y).abs() < f32::EPSILON);
    assert!(
        rect.min.x >= home.max.x - f32::EPSILON,
        "a place to the right of home"
    );
    assert!(marks(&h.frame_shapes()) > 0, "the closure really does draw");
    Ok(())
}

/// A `nav_item` that does not pass the gate is not drawn and gets no `item_rect` either.
#[test]
fn nav_item_is_skipped_when_its_gate_fails() -> fairing::Result<()> {
    let calls = Rc::new(Cell::new(0u32));
    // `default_gate = "top"` — an unassigned gate (`kbd`) needs maintainer.
    let mut config = access_config(&["viewer", "maintainer"], Some("top"));
    config.nav_bar.items = vec!["home".to_owned(), "kbd".to_owned()];
    let calls_in = Rc::clone(&calls);
    let mut h = test_shell(config, move |shell| {
        shell.add(nav_item(
            "kbd",
            move |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
                calls_in.set(calls_in.get() + 1);
                ui.label("K");
            },
        ));
    })?;
    h.frames(2);
    assert_eq!(
        calls.get(),
        0,
        "an item that does not pass the gate is not drawn"
    );
    assert_eq!(
        h.shell
            .nav_bar()
            .item_rect(&NavItem::Custom("kbd".to_owned())),
        None
    );
    assert!(
        h.shell.nav_bar().item_rect(&NavItem::Home).is_some(),
        "the other items are as they were"
    );
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// The bar painters
// ─────────────────────────────────────────────────────────────────────────────

/// With a status bar painter, **the built-in render is not called** and only the item list comes over.
#[test]
fn status_bar_painter_replaces_the_builtin_render() -> fairing::Result<()> {
    // The baseline: with no painter the built-in render records the clock's Rect.
    let mut plain = test_shell(single_level_access(), |_| {})?;
    plain.frames(2);
    assert!(
        plain.shell.status_bar().item_rect("status.clock").is_some(),
        "the built-in render records an item Rect"
    );

    let items: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let rect = Rc::new(Cell::new(egui::Rect::NOTHING));
    let (items_in, rect_in) = (Rc::clone(&items), Rc::clone(&rect));
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .services(Services::null())
            .status_bar_painter(move |ui: &mut egui::Ui, bar: &mut fairing::BarCx<'_>| {
                rect_in.set(bar.rect);
                let mut list = items_in.borrow_mut();
                list.clear();
                list.extend(bar.items().map(|item| (item.id.to_owned(), item.live())));
                ui.painter().rect_filled(bar.rect, 0.0, MARK);
            })
            .build(ctx)
    })?;
    h.frames(2);
    assert!(
        h.shell.status_bar().item_rect("status.clock").is_none(),
        "with a painter the built-in items are not drawn"
    );
    let drawn = rect.get();
    let laid = h
        .shell
        .layout()
        .status
        .ok_or_else(|| fairing::Error::Config("there is no layout.status".to_owned()))?;
    assert!(
        drawn.min.distance(laid.min) < 0.01 && drawn.max.distance(laid.max) < 0.01,
        "the shell's layout still settles the Rect: drawn {drawn:?} vs laid out {laid:?}"
    );
    // The default config's slot list as it is, in left → centre → right order (the gate table has one level, so all pass).
    // M2: the head of the right is `status.notifications`.
    let ids: Vec<String> = items.borrow().iter().map(|(id, _)| id.clone()).collect();
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
    assert!(items.borrow().iter().all(|(_, live)| *live));
    assert!(marks(&h.frame_shapes()) > 0, "the painter really does draw");
    Ok(())
}

/// The nav bar painter is the same — the shell keeps only the Rects and the enabled judgements (the rule as it is).
#[test]
fn nav_bar_painter_replaces_the_builtin_render() -> fairing::Result<()> {
    let mut plain = test_shell(single_level_access(), |_| {})?;
    plain.frames(2);
    assert!(plain.shell.nav_bar().item_rect(&NavItem::Home).is_some());

    let items: Rc<RefCell<Vec<(String, bool)>>> = Rc::new(RefCell::new(Vec::new()));
    let back_enabled = Rc::new(Cell::new(true));
    let (items_in, back_in) = (Rc::clone(&items), Rc::clone(&back_enabled));
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .services(Services::null())
            .nav_bar_painter(move |ui: &mut egui::Ui, bar: &mut fairing::BarCx<'_>| {
                back_in.set(bar.back_enabled);
                let mut list = items_in.borrow_mut();
                list.clear();
                list.extend(bar.items().map(|item| (item.id.to_owned(), item.live())));
                ui.painter().rect_filled(bar.rect, 0.0, MARK);
            })
            .build(ctx)
    })?;
    h.frames(2);
    assert_eq!(
        h.shell.nav_bar().item_rect(&NavItem::Home),
        None,
        "with a painter there are no built-in item Rects"
    );
    assert!(!back_enabled.get(), "back is disabled at home");
    assert_eq!(
        *items.borrow(),
        vec![
            ("back".to_owned(), false),
            ("home".to_owned(), true),
            // `recents` is no longer greyed out. The crate has no overview of its own, but
            // the device may, so the item is live and a press leaves as
            // `ShellEvent::OverviewRequested`. `back` is the one item the shell still judges,
            // because "is there a stack behind you" is a fact the shell owns.
            ("recents".to_owned(), true),
        ],
        "the painter is handed the shell's own liveness judgements, whatever they are"
    );
    assert!(marks(&h.frame_shapes()) > 0);
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// The desktop cell painter
// ─────────────────────────────────────────────────────────────────────────────

/// What the cell painter is given: the cell and icon Rects, whether the gate passed, and the press scale. The
/// hit test and recording the A2 starting Rect stay the shell's.
#[test]
fn slot_painter_gets_the_cell_gate_and_press_state() -> fairing::Result<()> {
    /// One cell as the painter saw it.
    #[derive(Clone, Copy)]
    struct Seen {
        cell: egui::Rect,
        icon: egui::Rect,
        scale: f32,
        pressed: bool,
        allowed: bool,
        in_dock: bool,
    }

    let seen: Rc<RefCell<Vec<(String, Seen)>>> = Rc::new(RefCell::new(Vec::new()));
    let seen_in = Rc::clone(&seen);
    // `default_gate = "top"`, so `locked` needs maintainer and only `open` is assigned to viewer.
    let mut config = access_config(&["viewer", "maintainer"], Some("top"));
    config.motion.reduce = true;
    config
        .access
        .gates
        .insert("open".to_owned(), "viewer".to_owned());
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config)
            .services(Services::null())
            .slot_painter(move |ui: &mut egui::Ui, slot: SlotCx<'_>| {
                seen_in.borrow_mut().push((
                    slot.slot.id.clone(),
                    Seen {
                        cell: slot.cell,
                        icon: slot.icon,
                        scale: slot.scale,
                        pressed: slot.pressed,
                        allowed: slot.allowed,
                        in_dock: slot.in_dock,
                    },
                ));
                ui.painter().rect_filled(slot.cell, 0.0, MARK);
            })
            .build(ctx)?;
        shell.add(icon_screen("open"));
        shell.add(icon_screen("locked"));
        Ok(shell)
    })?;
    h.frames(2);
    assert!(marks(&h.frame_shapes()) >= 2, "the painter draws per cell");

    let last = |id: &str| -> Option<Seen> {
        seen.borrow()
            .iter()
            .rev()
            .find(|(slot, _)| slot == id)
            .map(|(_, seen)| *seen)
    };
    let open = last("open").ok_or_else(|| missing("the `open` cell"))?;
    let locked = last("locked").ok_or_else(|| missing("the `locked` cell"))?;
    assert!(open.allowed, "a gate assigned to viewer passes");
    assert!(
        !locked.allowed,
        "an unassigned gate takes default_gate = top"
    );
    assert!(!open.in_dock, "it is a grid cell");
    assert!(
        open.cell.contains_rect(open.icon),
        "the icon is inside the cell"
    );
    assert!(
        (open.scale - 1.0).abs() < f32::EPSILON,
        "unpressed it is 1.0"
    );
    // The shell records the A2 starting Rect even with a painter.
    assert_eq!(
        h.shell.desktop().icon_rect("open"),
        Some(open.icon),
        "icon_rect is the icon Rect before the press, as it was"
    );

    // Judging the press is the shell's too — the painter gets only the result.
    h.press(open.icon.center());
    h.frame();
    let pressed = last("open").ok_or_else(|| missing("the pressed `open` cell"))?;
    assert!(
        pressed.pressed,
        "it lands on the first pressed frame (10 A7)"
    );
    assert!(pressed.scale < 1.0, "the press scale: {}", pressed.scale);
    assert!(
        pressed.cell.contains_rect(pressed.icon),
        "pressed, it does not go outside the cell"
    );
    h.release(open.icon.center());
    h.frame();
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// The theme
// ─────────────────────────────────────────────────────────────────────────────

/// `[theme.palette]` lies over dark/light, and a wrong role name or colour is an error at start-up.
#[test]
fn theme_palette_override_applies_and_bad_input_is_a_config_error() -> fairing::Result<()> {
    let config = ShellConfig::from_toml(
        r##"
[shell]
theme = "light"

[theme.palette]
primary = "#ff0000"
background = "#101112"
"##,
    )?;
    let theme = config.build_theme()?;
    assert!(!theme.dark, "[shell] theme is light, as it was");
    assert_eq!(
        theme.color(ColorRole::Primary),
        egui::Color32::from_rgb(0xff, 0x00, 0x00)
    );
    assert_eq!(
        theme.color(ColorRole::Background),
        egui::Color32::from_rgb(0x10, 0x11, 0x12)
    );
    assert_eq!(
        theme.color(ColorRole::Surface),
        Palette::light().surface,
        "a role not overridden is as it was"
    );

    // A typo in the role name.
    let bad_role = ShellConfig::from_toml("[theme.palette]\nprimarry = \"#ff0000\"\n")?;
    assert!(matches!(bad_role.build_theme(), Err(Error::Config(_))));
    // The colour format (using a role name as a value is refused too — it is a colour, not an alias).
    for value in ["red", "#ff00", "primary"] {
        let bad = ShellConfig::from_toml(&format!("[theme.palette]\nprimary = \"{value}\"\n"))?;
        assert!(
            matches!(bad.build_theme(), Err(Error::Config(_))),
            "{value} is not #RRGGBB"
        );
    }
    // Building the shell fails with the same error (it is not quietly ignored).
    let ctx = egui::Context::default();
    assert!(matches!(
        Shell::new(bad_role, Services::null(), &ctx),
        Err(Error::Config(_))
    ));
    Ok(())
}

/// The bars' heights and the status icons' size on a plain shell, after a few frames.
fn bar_sizes(config: ShellConfig) -> fairing::Result<(f32, f32, f32)> {
    let mut h = Harness::from_builder(move |ctx| Shell::builder(config).build(ctx))?;
    h.frames(3);
    let status = h
        .shell
        .layout()
        .status
        .ok_or_else(|| missing("the status bar Rect"))?;
    let nav = h
        .shell
        .layout()
        .nav
        .ok_or_else(|| missing("the nav bar Rect"))?;
    Ok((
        status.height(),
        nav.height(),
        h.shell.status_bar().icon.size,
    ))
}

/// **What the TOML pins, the bars are**. The crate's physical metrics used to overwrite
/// `[status_bar] height`, `[status_bar] icon_size` and `[nav_bar] height` every frame, so the three
/// keys did nothing at all.
#[test]
fn the_toml_bar_sizes_take_effect() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.status_bar.height = Some(48.0);
    config.status_bar.icon_size = Some(26.0);
    config.nav_bar.height = Some(80.0);
    let (status, nav, icon) = bar_sizes(config)?;
    assert!((status - 48.0).abs() < 0.01, "the status bar is {status}");
    assert!((nav - 80.0).abs() < 0.01, "the nav bar is {nav}");
    assert!((icon - 26.0).abs() < 0.01, "the status icons are {icon}");
    Ok(())
}

/// Unset, the sizes stay the crate's physical ones — the same as before the keys worked.
#[test]
fn unset_bar_sizes_stay_physical() -> fairing::Result<()> {
    let (status, nav, icon) = bar_sizes(single_level_access())?;
    let (pinned_status, pinned_nav, pinned_icon) = {
        let mut config = single_level_access();
        config.status_bar.height = Some(32.0);
        config.status_bar.icon_size = Some(18.0);
        config.nav_bar.height = Some(56.0);
        bar_sizes(config)?
    };
    // At the assumed density 7 mm is 44 du, a finger plus 8 du is 90, and 3.6 mm is 23 — each
    // above its du floor, so unset and "pinned to the floor" come out different.
    assert!(status > pinned_status + 1.0, "{status} vs {pinned_status}");
    assert!(nav > pinned_nav + 1.0, "{nav} vs {pinned_nav}");
    assert!(icon > pinned_icon + 1.0, "{icon} vs {pinned_icon}");
    Ok(())
}

/// An integrator's metrics spec is the higher rung: it wins over the TOML.
#[test]
fn a_metrics_spec_wins_over_the_toml_bar_sizes() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.status_bar.height = Some(48.0);
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .metrics_spec(fairing::theme::MetricsSpec::legacy_du())
            .build(ctx)
    })?;
    h.frames(3);
    let status = h
        .shell
        .layout()
        .status
        .ok_or_else(|| missing("the status bar Rect"))?;
    assert!(
        (status.height() - 32.0).abs() < 0.01,
        "the spec's 32 du, not the TOML's 48: {}",
        status.height()
    );
    Ok(())
}

/// An injected theme is a higher rung too: its metrics win over what the TOML pins.
#[test]
fn an_injected_theme_wins_over_the_toml_bar_sizes() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.status_bar.height = Some(48.0);
    let mut injected = Theme::dark();
    injected.metrics.status_bar_height = 44.0;
    let mut h =
        Harness::from_builder(move |ctx| Shell::builder(config).theme(injected).build(ctx))?;
    h.frames(3);
    let status = h
        .shell
        .layout()
        .status
        .ok_or_else(|| missing("the status bar Rect"))?;
    assert!(
        (status.height() - 44.0).abs() < 0.01,
        "the theme's 44 du, not the TOML's 48: {}",
        status.height()
    );
    Ok(())
}

/// An unknown preset is refused with every preset named — read from `Preset::ALL`, so a new one
/// cannot be left out of the message the way linen was. Both readers say so: the
/// builder's and `ShellConfig::build_theme`'s.
#[test]
fn an_unknown_preset_names_every_preset() {
    let mut config = single_level_access();
    config.theme.preset = "nope".to_owned();
    let names = |err: Error| {
        let text = err.to_string();
        ["base", "abyss", "linen"]
            .iter()
            .all(|name| text.contains(name))
            .then_some(())
            .ok_or(text)
    };
    let built = config.build_theme().err().map(names);
    assert_eq!(built, Some(Ok(())), "build_theme");
    let ctx = egui::Context::default();
    let shell = Shell::builder(config).build(&ctx).err().map(names);
    assert_eq!(shell, Some(Ok(())), "Shell::builder");
}

/// Injecting a whole theme has `[shell] theme` and `[theme.palette]` ignored.
/// The bar heights follow the injected theme's metric tokens.
#[test]
fn injected_theme_wins_over_the_config_theme() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.shell.theme = "light".to_owned();
    config
        .theme
        .palette
        .insert("primary".to_owned(), "#ff0000".to_owned());
    config.motion.reduce = true;
    let mut injected = Theme::dark();
    injected.metrics.status_bar_height = 44.0;
    injected.metrics.nav_bar_height = 72.0;
    let want = injected.clone();

    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .theme(injected)
            .services(Services::null())
            .build(ctx)
    })?;
    h.frames(2);
    assert!(h.shell.theme().dark, "the injected dark theme wins");
    assert_eq!(
        h.shell.theme().color(ColorRole::Primary),
        want.color(ColorRole::Primary),
        "the [theme.palette] override is ignored"
    );
    // The theme's tokens are the single source for the bar heights — the layout uses those values.
    let status = h
        .shell
        .layout()
        .status
        .ok_or_else(|| missing("the status bar Rect"))?;
    let nav = h
        .shell
        .layout()
        .nav
        .ok_or_else(|| missing("the nav bar Rect"))?;
    assert!((status.height() - 44.0).abs() < f32::EPSILON);
    assert!((nav.height() - 72.0).abs() < f32::EPSILON);
    Ok(())
}

/// A `[theme.palette]` override lies over **both dark and light**, so it survives a theme
/// toggle. It used to be flattened to the neutral palette by one toggle.
#[test]
fn theme_toggle_keeps_preset_and_overrides() -> fairing::Result<()> {
    use fairing::theme::Preset;
    let mut config = single_level_access();
    config.motion.reduce = true; // it ends the crossfade at once.
    config.theme.preset = "abyss".to_owned();
    config
        .theme
        .palette
        .insert("primary".to_owned(), "#ff7a00".to_owned());
    let orange = egui::Color32::from_rgb(0xff, 0x7a, 0x00);

    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config).services(Services::null()).build(ctx)
    })?;
    h.frames(2);
    assert_eq!(h.shell.theme().color(ColorRole::Primary), orange);
    // The preset really is Abyss — checked through a role that is not overridden.
    assert_eq!(
        h.shell.theme().color(ColorRole::Background),
        Palette::preset(Preset::Abyss, true).background,
        "[theme] preset = abyss settles the palette"
    );

    h.shell.set_theme_dark(false);
    h.frames(3);
    assert!(!h.shell.theme().dark, "it went over to light");
    assert_eq!(
        h.shell.theme().color(ColorRole::Primary),
        orange,
        "the override lies over the light side too and survives"
    );
    assert_eq!(
        h.shell.theme().color(ColorRole::Background),
        Palette::preset(Preset::Abyss, false).background,
        "the preset is as it was on the light side too"
    );
    Ok(())
}

/// A palette put in through the guide's proper path (injecting a `Theme`) is not erased
/// by a theme toggle — both sides of the pair being the same, only the `dark` flag and egui's `Visuals` change.
#[test]
fn injected_theme_survives_theme_toggle() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    let purple = egui::Color32::from_rgb(0x8b, 0x5c, 0xf6);
    // An injected theme brings **its own** motion tokens — `[motion] reduce` reaches only a theme made from the
    // config. To end the crossfade at once it has to be reduced here too.
    let injected = Theme {
        palette: Palette {
            primary: purple,
            ..Palette::dark()
        },
        motion: fairing::theme::MotionTokens::from_config(&fairing::config::MotionConfig {
            reduce: true,
            ..fairing::config::MotionConfig::default()
        }),
        ..Theme::dark()
    };

    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .theme(injected)
            .services(Services::null())
            .build(ctx)
    })?;
    h.frames(2);
    assert_eq!(h.shell.theme().color(ColorRole::Primary), purple);

    h.shell.set_theme_dark(false);
    h.frames(3);
    assert!(!h.shell.theme().dark, "the flag does change");
    assert_eq!(
        h.shell.theme().color(ColorRole::Primary),
        purple,
        "an injected palette must not vanish on one toggle"
    );

    // The old behaviour (the neutral palette on a toggle) comes back in one line if it is wanted.
    h.shell.set_palettes(Palette::dark(), Palette::light());
    h.shell.set_theme_dark(true);
    h.frames(3);
    assert_eq!(
        h.shell.theme().color(ColorRole::Primary),
        Palette::dark().primary,
        "changing the pair with set_palettes has that pair win from the next toggle"
    );
    Ok(())
}

/// `ShellBuilder::palettes` beats `[theme] preset` and `[theme.palette]`.
#[test]
fn builder_palettes_win_over_the_config() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    config.theme.preset = "abyss".to_owned();
    let mine_dark = Palette {
        background: egui::Color32::from_rgb(0x10, 0x00, 0x00),
        ..Palette::dark()
    };
    let mine_light = Palette {
        background: egui::Color32::from_rgb(0x00, 0x10, 0x00),
        ..Palette::light()
    };

    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .palettes(mine_dark, mine_light)
            .services(Services::null())
            .build(ctx)
    })?;
    h.frames(2);
    let (dark, light) = h.shell.palettes();
    assert_eq!(*dark, mine_dark);
    assert_eq!(*light, mine_light);
    h.shell.set_theme_dark(false);
    h.frames(3);
    assert_eq!(
        h.shell.theme().color(ColorRole::Background),
        mine_light.background,
        "the pair's light side comes"
    );
    Ok(())
}

/// With `[nav_bar] enabled = false` there is no bar and the content is that much wider
/// (the setup `examples/custom_chrome.rs` uses).
#[test]
fn nav_bar_can_be_switched_off_in_config() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.nav_bar.enabled = false;
    let mut h = test_shell(config, |_| {})?;
    h.frames(2);
    assert_eq!(h.shell.layout().nav, None);
    assert!(
        (h.shell.layout().content.max.y - h.screen_rect().max.y).abs() < f32::EPSILON,
        "the content comes all the way to the bottom of the screen"
    );
    Ok(())
}
