//! Regression tests for the access-control fixes before 0.1.0: the network form behind its gate,
//! the attempt limit, a tile's gate, the lock screen's floor and the hidden entries' hints.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

#![cfg(all(feature = "settings", feature = "mock", feature = "overlay"))]

use fairing::access::{AccessEvent, Corner, HiddenEntry, Subject, TapKnock, Zone};
use fairing::overlay::{tile, TileKind};
use fairing::settings::{add_all, SettingKey, SettingsConfig};
use fairing::testing::{access_config, access_config_mode, test_shell, Harness};
use fairing::{screen, Cx, LaunchAction, Level, Shell, ShellEvent};

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

fn text_rect(h: &mut Harness, wanted: &str) -> Option<egui::Rect> {
    fn walk(shape: &egui::Shape, wanted: &str, found: &mut Option<egui::Rect>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                // The first drawn: the top of a list (the prompt has each label once).
                if found.is_none() {
                    *found = Some(text.galley.rect.translate(text.pos.to_vec2()));
                }
            }
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    walk(s, wanted, found);
                }
            }
            _ => {}
        }
    }
    let mut found = None;
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, wanted, &mut found);
    }
    found
}

fn drawn(h: &mut Harness, wanted: &str) -> bool {
    text_rect(h, wanted).is_some()
}

fn tap_text(h: &mut Harness, wanted: &str) -> fairing::Result<()> {
    let rect = text_rect(h, wanted).ok_or_else(|| fail(format!("`{wanted}` is not drawn")))?;
    h.tap(rect.center());
    h.frames(3);
    Ok(())
}

fn type_pin(h: &mut Harness, pin: &str) {
    for digit in pin.chars() {
        let key = match digit {
            '0' => egui::Key::Num0,
            '1' => egui::Key::Num1,
            '2' => egui::Key::Num2,
            '3' => egui::Key::Num3,
            '4' => egui::Key::Num4,
            '5' => egui::Key::Num5,
            '6' => egui::Key::Num6,
            '7' => egui::Key::Num7,
            '8' => egui::Key::Num8,
            _ => egui::Key::Num9,
        };
        h.key(key);
    }
    h.frames(2);
}

fn access_events(h: &mut Harness) -> Vec<AccessEvent> {
    h.shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::Access(a) => Some(a),
            _ => None,
        })
        .collect()
}

fn level(h: &Harness) -> Level {
    h.shell.access().session().subject.level
}

fn at(level: u16) -> Subject {
    Subject {
        level: Level(level),
        ..Subject::default()
    }
}

// ------------------------------------------------------------------ settings.network.edit

/// Three levels in `routing` mode, the mock backends, the built-in settings screens; the network
/// screen at operator and its editing at maintainer.
fn network_shell() -> fairing::Result<Harness> {
    let mut config = access_config_mode(
        &["viewer", "operator", "maintainer"],
        Some("bottom"),
        "routing",
    );
    for (gate, lvl) in [
        ("settings.network", "operator"),
        ("settings.network.edit", "maintainer"),
    ] {
        config.access.gates.insert(gate.to_owned(), lvl.to_owned());
    }
    config.motion.reduce = true;
    let h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config)
            .services(fairing::services::mock::services())
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        Ok(shell)
    })?
    .with_size(1024.0, 1400.0);
    Ok(h)
}

fn settle(h: &mut Harness) {
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
}

fn eth0_dhcp(h: &Harness) -> fairing::Result<bool> {
    h.shell
        .services()
        .network
        .interfaces()
        .into_iter()
        .find(|i| i.name == "eth0")
        .map(|i| i.dhcp)
        .ok_or_else(|| fail("no eth0"))
}

/// An open IPv4 form is gone, not applied, once the session drops below `settings.network.edit`.
#[test]
fn the_network_form_does_not_apply_once_the_session_drops_below_its_gate() -> fairing::Result<()> {
    let mut h = network_shell()?;
    h.frames(2);
    h.shell.handle().set_subject(at(2));
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.network"));
    settle(&mut h);
    tap_text(&mut h, "Set the IPv4 address")?;
    assert!(drawn(&mut h, "Apply"), "the maintainer has the form");
    // The session drops to operator (a timeout, a temporary unlock running out, set_subject).
    h.shell.handle().set_subject(at(1));
    settle(&mut h);
    assert_eq!(level(&h), Level(1));
    assert!(
        h.shell.workspace().find("settings.network").is_some(),
        "operator still passes settings.network"
    );
    // Whatever is on the screen, an operator must not reach NetworkBackend::configure.
    if drawn(&mut h, "Manual") {
        tap_text(&mut h, "Manual")?;
    }
    if drawn(&mut h, "Apply") {
        tap_text(&mut h, "Apply")?;
    }
    assert!(
        eth0_dhcp(&h)?,
        "an operator (below settings.network.edit) changed eth0 to a manual address"
    );
    Ok(())
}

/// A form a maintainer left open is not handed to the next operator who opens the screen.
#[test]
fn a_network_form_left_open_does_not_reappear_for_a_lower_session() -> fairing::Result<()> {
    let mut h = network_shell()?;
    h.frames(2);
    h.shell.handle().set_subject(at(2));
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.network"));
    settle(&mut h);
    tap_text(&mut h, "Set the IPv4 address")?;
    assert!(drawn(&mut h, "Apply"));
    // Back home and the session down to viewer: the network screen closes (viewer is below it).
    h.shell.home();
    settle(&mut h);
    h.shell.handle().set_subject(at(0));
    settle(&mut h);
    assert!(h.shell.workspace().find("settings.network").is_none());
    // Later, an operator.
    h.shell.handle().set_subject(at(1));
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.network"));
    settle(&mut h);
    assert!(
        !drawn(&mut h, "Apply"),
        "an operator opening settings.network got the maintainer's IPv4 form"
    );
    Ok(())
}

// ------------------------------------------------------------------------ a tile's gate

/// A locked tile with a shared gate asks for that gate, not for its id.
#[test]
fn a_locked_tile_asks_for_its_gate_not_its_id() -> fairing::Result<()> {
    let mut config = access_config_mode(&["viewer", "maintainer"], Some("bottom"), "routing");
    config
        .access
        .gates
        .insert("heating".to_owned(), "maintainer".to_owned());
    let mut h = test_shell(config, |sh| {
        sh.add(
            tile("heater", TileKind::Toggle(SettingKey::from("app.heater")))
                .label("Heater")
                .gate("heating"),
        );
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
    let cell = h
        .shell
        .overlay()
        .tile_rect("heater")
        .ok_or_else(|| fail("no heater tile"))?;
    let _ = h.shell.poll_events();
    h.tap(cell.center());
    h.frames(2);
    let gates: Vec<String> = access_events(&mut h)
        .into_iter()
        .filter_map(|e| match e {
            AccessEvent::UnlockRequested { gate, .. } => Some(gate.as_str().to_owned()),
            _ => None,
        })
        .collect();
    assert_eq!(
        gates,
        vec!["heating".to_owned()],
        "the tile's gate is `heating`"
    );
    Ok(())
}

// ------------------------------------------------------------------- the attempt limit

/// A right PIN the shell refuses as not enough counts against the attempt limit instead of resetting it.
#[test]
fn a_known_lower_pin_counts_against_the_attempt_limit() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.access.unlock_mode = "switch".to_owned();
    config.access.pin_table.pins = [("operator", "1234"), ("maintainer", "9876")]
        .iter()
        .map(|(l, p)| ((*l).to_owned(), (*p).to_owned()))
        .collect();
    config.access.pin_table.attempt_limit = Some(3);
    let mut h = test_shell(config, |sh| {
        sh.add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("calibration");
        }));
    })?;
    h.frames(2);
    h.shell.handle().set_subject(at(1));
    h.frames(2);
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(h.shell.unlock_prompt_visible());
    let _ = h.shell.poll_events();
    // Wrong maintainer guesses, the operator's own PIN after each: the limit of three is reached
    // by the second guess (guess, refused, guess) and nothing in between starts the count again.
    for wrong in ["0000", "0001"] {
        type_pin(&mut h, wrong);
        type_pin(&mut h, "1234");
    }
    let events = access_events(&mut h);
    let denied = events
        .iter()
        .filter(|e| matches!(e, AccessEvent::Denied { .. }))
        .count();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AccessEvent::Locked { .. })),
        "{denied} refusals in a row (two wrong maintainer PINs, each followed by the operator's) and no lockout: {events:?}"
    );
    Ok(())
}

// -------------------------------------------------------------- a lower grant on the lock screen

/// A PIN below the starting level on the lock screen neither demotes the session nor closes its screens.
#[test]
fn a_lower_pin_on_the_lock_screen_does_not_demote_below_the_start() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.access.initial = Some("operator".to_owned());
    config.access.unlock_mode = "switch".to_owned();
    config
        .access
        .gates
        .insert("ops".to_owned(), "operator".to_owned());
    config.access.pin_table.pins = [("viewer", "1111"), ("maintainer", "9876")]
        .iter()
        .map(|(l, p)| ((*l).to_owned(), (*p).to_owned()))
        .collect();
    let mut h = test_shell(config, |sh| {
        sh.add(screen("ops", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("line");
        }));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("ops"));
    h.frames(3);
    assert!(h.shell.workspace().find("ops").is_some());
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    type_pin(&mut h, "1111");
    assert!(
        level(&h) >= Level(1) && h.shell.workspace().find("ops").is_some(),
        "the viewer PIN on the lock screen took the session to {:?} (start is operator) and ops open = {}",
        level(&h),
        h.shell.workspace().find("ops").is_some()
    );
    Ok(())
}

// ------------------------------------------------------------------------ hidden entries

/// `Zone::corner` resolves to a square sized from the screen's short side.
#[test]
fn a_corner_zone_is_a_square_of_the_short_side() {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 480.0));
    let r = Zone::corner(Corner::TopLeft, 0.12).resolve(screen);
    let side = 480.0 * 0.12;
    assert!(
        (r.width() - side).abs() < 0.01 && (r.height() - side).abs() < 0.01,
        "corner zone is {} x {}, not a {side} square",
        r.width(),
        r.height()
    );
}

/// An entry with no `hint_from` gives `cx.knock_remaining` nothing to show.
#[test]
fn an_entry_without_hint_from_stays_quiet() -> fairing::Result<()> {
    let seen = std::rc::Rc::new(std::cell::Cell::new(None));
    let saw = std::rc::Rc::clone(&seen);
    let mut h = test_shell(fairing::testing::single_level_access(), move |sh| {
        sh.add_hidden_entry(HiddenEntry::new(
            "service",
            TapKnock::new(7),
            LaunchAction::open("nowhere"),
        ));
        sh.add(screen("about", move |_: &mut egui::Ui, cx: &mut Cx<'_>| {
            saw.set(Some(cx.knock_remaining("service")));
        }));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("about"));
    h.frames(3);
    assert_eq!(
        seen.get(),
        Some(None),
        "an entry with no hint_from tells the screen how many knocks are left"
    );
    Ok(())
}
