//! The hidden entry points — the invisible door into a device's service menu.
//!
//! What it checks: the Android-style tap knock, the corner sequence knock, **whether the gate really does refuse**,
//! and **whether the audit event always fires**. A hidden entry point is concealment, not authentication, so there
//! must not be a single path that opens without the gate.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!` and no `unwrap`.

use fairing::access::{
    AccessEvent, Corner, HiddenEntry, KnockInput, KnockStep, KnockTrigger, TapKnock, Zone,
    ZoneKnock,
};
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{screen, Cx, LaunchAction, ShellEvent};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// The smallest screen, drawing one label.
fn stub(id: &'static str) -> fairing::screen::ScreenDecl {
    screen(id, move |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label(id);
    })
}

/// The declaration id currently focused.
fn focused(h: &Harness) -> Option<&str> {
    h.shell
        .workspace()
        .focused()
        .map(fairing::workspace::Instance::decl_id)
}

/// The `HiddenEntry` event ids piled up as of this frame.
fn opened(h: &mut Harness) -> Vec<String> {
    h.shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::HiddenEntry { id } => Some(id),
            _ => None,
        })
        .collect()
}

/// The Android style. Six times does nothing and **the seventh opens it.**
#[test]
fn seven_taps_open_the_hidden_screen() -> fairing::Result<()> {
    let knocks = Rc::new(Cell::new(0u32));
    let knocks_in = Rc::clone(&knocks);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("about", move |ui: &mut egui::Ui, cx: &mut Cx| {
            // The screen knocks at its own secret place. The shell knows nothing of where it was pressed.
            if knocks_in.get() > 0 {
                knocks_in.set(knocks_in.get() - 1);
                cx.knock("service");
            }
            ui.label("Model ACME-7");
        }));
        sh.add(stub("service_menu"));
        sh.add_hidden_entry(HiddenEntry::taps(
            "service",
            7,
            LaunchAction::open("service_menu"),
        ));
        sh.launch(LaunchAction::open("about"));
    })?;
    h.frames(2);
    assert_eq!(focused(&h), Some("about"));

    for i in 1..=6 {
        knocks.set(1);
        h.frames(2);
        assert!(opened(&mut h).is_empty(), "it must not open on number {i}");
        assert_eq!(focused(&h), Some("about"));
    }
    knocks.set(1);
    h.frames(2);
    assert_eq!(opened(&mut h), ["service"], "the seventh opens it");
    assert_eq!(focused(&h), Some("service_menu"));
    Ok(())
}

/// **The gate is the security.** Even with the knock complete, failing the gate opens no screen and raises only
/// `UnlockRequested` — and the audit event fires **anyway**.
#[test]
fn a_gated_entry_asks_to_unlock_instead_of_opening() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "service"], Some("top"));
    config
        .access
        .gates
        .insert("service".to_owned(), "service".to_owned());
    let mut h = test_shell(config, |sh| {
        sh.add(stub("service_menu"));
        sh.add_hidden_entry(
            HiddenEntry::taps("service", 2, LaunchAction::open("service_menu")).gate("service"),
        );
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();

    h.shell.knock("service");
    h.shell.knock("service");
    h.frames(2);

    let events: Vec<_> = h.shell.poll_events();
    let audited = events
        .iter()
        .any(|e| matches!(e, ShellEvent::HiddenEntry { id } if id == "service"));
    assert!(audited, "the audit event fires even when the gate refuses");
    let asked = events.iter().any(|e| {
        matches!(
            e,
            ShellEvent::Access(AccessEvent::UnlockRequested { gate, then })
                if gate.as_str() == "service" && then.is_some()
        )
    });
    assert!(asked, "it asks to be authenticated: {events:?}");
    assert_ne!(
        focused(&h),
        Some("service_menu"),
        "it must not open with the gate unpassed"
    );
    Ok(())
}

/// Touching the corners **in order** opens it. It needs no widget, so it works on a locked screen too.
#[test]
fn tapping_the_corners_in_order_opens_it() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::corners(
            "factory",
            [
                Corner::TopLeft,
                Corner::TopRight,
                Corner::BottomRight,
                Corner::BottomLeft,
            ],
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    let r = h.screen_rect();
    let inset = 8.0;
    for pos in [
        egui::pos2(r.min.x + inset, r.min.y + inset),
        egui::pos2(r.max.x - inset, r.min.y + inset),
        egui::pos2(r.max.x - inset, r.max.y - inset),
        egui::pos2(r.min.x + inset, r.max.y - inset),
    ] {
        h.tap(pos);
        h.frame();
    }
    assert_eq!(opened(&mut h), ["factory"]);
    assert_eq!(focused(&h), Some("factory"));
    Ok(())
}

/// **An edge swipe must not count as a knock.** Starting at a corner and dragging would advance the entry point
/// every time the shade is pulled.
#[test]
fn a_swipe_from_a_corner_is_not_a_knock() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::corners(
            "factory",
            [Corner::TopLeft],
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    let r = h.screen_rect();
    let from = egui::pos2(r.min.x + 8.0, r.min.y + 8.0);
    h.drag(from, from + egui::vec2(0.0, 200.0), 8);
    h.frames(2);
    assert!(opened(&mut h).is_empty(), "a swipe is not a knock");
    Ok(())
}

/// Pressing the middle of the screen does nothing.
#[test]
fn a_tap_in_the_middle_is_not_a_corner() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::corners(
            "factory",
            [Corner::TopLeft],
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    h.tap(h.screen_rect().center());
    h.frames(2);
    assert_eq!(opened(&mut h).len(), 0);
    Ok(())
}

/// A screen can read how many are left — Android's "3 to go" comes out of this.
#[test]
fn a_screen_can_read_how_many_knocks_are_left() -> fairing::Result<()> {
    let seen: Rc<Cell<Option<u8>>> = Rc::new(Cell::new(None));
    let seen_in = Rc::clone(&seen);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("about", move |ui: &mut egui::Ui, cx: &mut Cx| {
            seen_in.set(cx.knock_remaining("service"));
            ui.label("about");
        }));
        sh.add(stub("service_menu"));
        sh.add_hidden_entry(
            HiddenEntry::taps("service", 5, LaunchAction::open("service_menu")).hint_from(3),
        );
        sh.launch(LaunchAction::open("about"));
    })?;
    h.frames(2);
    assert_eq!(seen.get(), Some(5), "at the start they are all left");
    h.shell.knock("service");
    h.shell.knock("service");
    h.frames(2);
    assert_eq!(seen.get(), Some(3));
    assert_eq!(
        h.shell.hidden_entries().collect::<Vec<_>>(),
        ["service"],
        "the registration list can be read"
    );
    Ok(())
}

/// Registering and unregistering. Once taken off, knocking does not open it.
#[test]
fn removing_an_entry_closes_the_door() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("service_menu"));
        sh.add_hidden_entry(HiddenEntry::taps(
            "service",
            1,
            LaunchAction::open("service_menu"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    assert!(h.shell.remove_hidden_entry("service"));
    assert!(
        !h.shell.remove_hidden_entry("service"),
        "there is no second one"
    );
    h.shell.knock("service");
    h.frames(2);
    assert_eq!(opened(&mut h).len(), 0);
    assert_ne!(focused(&h), Some("service_menu"));
    Ok(())
}

/// Registering the same id again swaps it out (the progress is reset too).
#[test]
fn re_registering_replaces_and_resets() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("a"));
        sh.add(stub("b"));
        sh.add_hidden_entry(HiddenEntry::taps("x", 3, LaunchAction::open("a")));
    })?;
    h.frames(2);
    h.shell.knock("x");
    h.shell.knock("x");
    let _ = h.shell.poll_events().len();
    // Swap it out two knocks in → from the start, and the destination changes too.
    h.shell
        .add_hidden_entry(HiddenEntry::taps("x", 2, LaunchAction::open("b")));
    h.shell.knock("x");
    h.frames(2);
    assert!(opened(&mut h).is_empty(), "the progress was reset");
    h.shell.knock("x");
    h.frames(2);
    assert_eq!(opened(&mut h), ["x"]);
    assert_eq!(focused(&h), Some("b"), "it is the new destination");
    assert_eq!(
        h.shell.hidden_entries().count(),
        1,
        "it is not a duplicate registration"
    );
    Ok(())
}

/// Knocking an unknown id does not stop the shell (the same policy as — warn and ignore).
#[test]
fn knocking_an_unknown_entry_is_ignored() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    h.shell.knock("nope");
    h.frames(2);
    assert_eq!(opened(&mut h).len(), 0);
    Ok(())
}

/// Going past the interval starts over — it does not pile up all day.
#[test]
fn a_long_pause_between_taps_resets_the_count() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("service_menu"));
        sh.add_hidden_entry(HiddenEntry::new(
            "service",
            TapKnock::new(3).within(Duration::from_millis(300)),
            LaunchAction::open("service_menu"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    h.shell.knock("service");
    h.shell.knock("service");
    h.run_for(1.0); // push the shell's clock a second on to break it
    h.shell.knock("service");
    h.frames(2);
    assert!(
        opened(&mut h).is_empty(),
        "the one after the break is the first"
    );
    h.shell.knock("service");
    h.shell.knock("service");
    h.frames(2);
    assert_eq!(opened(&mut h), ["service"]);
    Ok(())
}

/// Where the gate is already passed it opens straight away (the same entry point, a different session level).
#[test]
fn a_gated_entry_opens_when_the_gate_passes() -> fairing::Result<()> {
    let mut config = access_config(&["service"], None);
    config
        .access
        .gates
        .insert("service".to_owned(), "service".to_owned());
    let mut h = test_shell(config, |sh| {
        sh.add(stub("service_menu"));
        sh.add_hidden_entry(
            HiddenEntry::taps("service", 1, LaunchAction::open("service_menu")).gate("service"),
        );
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    h.shell.knock("service");
    h.frames(2);
    assert_eq!(opened(&mut h), ["service"]);
    assert_eq!(
        focused(&h),
        Some("service_menu"),
        "with one level in the table everything passes"
    );
    Ok(())
}

// ── The trait: the integrator defines the trigger ──────────────────

/// A physical key combination on the device's front. **A trigger the crate does not provide**, written by the integrator.
#[derive(Debug, Default)]
struct KeyCombo {
    hit: usize,
}

const COMBO: [egui::Key; 3] = [egui::Key::F1, egui::Key::F2, egui::Key::F1];

impl KnockTrigger for KeyCombo {
    fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep {
        let mut step = KnockStep::Idle;
        for key in input.keys {
            if COMBO.get(self.hit) == Some(key) {
                self.hit += 1;
                step = KnockStep::Advanced;
                if self.hit == COMBO.len() {
                    self.hit = 0;
                    return KnockStep::Opened;
                }
            } else if self.hit != 0 {
                self.hit = 0;
                step = KnockStep::Reset;
            }
        }
        step
    }

    fn remaining(&self) -> Option<u8> {
        u8::try_from(COMBO.len() - self.hit).ok()
    }

    fn reset(&mut self) {
        self.hit = 0;
    }
}

/// A trigger the crate knows nothing of attaches through one trait.
#[test]
fn a_custom_trigger_opens_the_door() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::new(
            "factory",
            KeyCombo::default(),
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    for key in [egui::Key::F1, egui::Key::F2] {
        h.key(key);
        h.frame();
    }
    assert!(opened(&mut h).is_empty(), "two is not there yet");
    h.key(egui::Key::F1);
    h.frame();
    assert_eq!(opened(&mut h), ["factory"]);
    assert_eq!(focused(&h), Some("factory"));
    Ok(())
}

/// An integrator's trigger gives a hint too — how many are left is the trait's to settle.
#[test]
fn a_custom_trigger_reports_its_own_hint() -> fairing::Result<()> {
    let seen = Rc::new(Cell::new(u8::MAX));
    let seen_in = Rc::clone(&seen);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("about", move |ui: &mut egui::Ui, cx: &mut Cx| {
            if let Some(n) = cx.knock_remaining("factory") {
                seen_in.set(n);
            }
            ui.label("about");
        }));
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::new(
            "factory",
            KeyCombo::default(),
            LaunchAction::open("factory"),
        ));
        sh.launch(LaunchAction::open("about"));
    })?;
    h.frames(2);
    assert_eq!(seen.get(), 3, "nothing has been pressed yet");
    h.key(egui::Key::F1);
    h.frames(2);
    assert_eq!(seen.get(), 2);
    Ok(())
}

/// A wrong key takes it back to the start.
#[test]
fn a_wrong_key_resets_the_combo() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::new(
            "factory",
            KeyCombo::default(),
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    for key in [egui::Key::F1, egui::Key::F2, egui::Key::F3, egui::Key::F1] {
        h.key(key);
        h.frame();
    }
    assert!(
        opened(&mut h).is_empty(),
        "an F3 in the middle starts it over"
    );
    Ok(())
}

/// **Arbitrary coordinates** are a trigger too, not just the corners — being screen ratios, the place is the same at any resolution.
#[test]
fn an_arbitrary_zone_sequence_opens_it() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::new(
            "factory",
            // The left → the right of a band across the middle of the screen.
            ZoneKnock::new([
                Zone::new((0.0, 0.4), (0.2, 0.6)),
                Zone::new((0.8, 0.4), (1.0, 0.6)),
            ]),
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    let r = h.screen_rect();
    let at = |u: f32, v: f32| egui::pos2(r.min.x + r.width() * u, r.min.y + r.height() * v);
    for pos in [at(0.1, 0.5), at(0.9, 0.5)] {
        h.tap(pos);
        h.frame();
    }
    assert_eq!(opened(&mut h), ["factory"]);
    Ok(())
}

/// A trigger that cannot be counted gives no hint either — `knock_remaining` is `None`.
#[test]
fn an_uncountable_trigger_has_no_hint() -> fairing::Result<()> {
    /// Any single tap opens it. There is no notion of how many are left.
    #[derive(Debug)]
    struct AnyTap;

    impl KnockTrigger for AnyTap {
        fn feed(&mut self, input: &KnockInput<'_>) -> KnockStep {
            if input.taps.is_empty() {
                KnockStep::Idle
            } else {
                KnockStep::Opened
            }
        }

        fn reset(&mut self) {}
    }

    let seen = Rc::new(Cell::new(true));
    let seen_in = Rc::clone(&seen);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("about", move |ui: &mut egui::Ui, cx: &mut Cx| {
            seen_in.set(cx.knock_remaining("any").is_some());
            ui.label("about");
        }));
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::new(
            "any",
            AnyTap,
            LaunchAction::open("factory"),
        ));
        sh.launch(LaunchAction::open("about"));
    })?;
    h.frames(2);
    assert!(!seen.get(), "uncountable, it is `None`");
    Ok(())
}

/// Registering again takes the trigger back to the start — a trigger in progress does not leak into the new registration.
#[test]
fn re_registering_resets_the_trigger() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(stub("factory"));
        sh.add_hidden_entry(HiddenEntry::new(
            "factory",
            ZoneKnock::corners([Corner::TopLeft, Corner::TopRight]),
            LaunchAction::open("factory"),
        ));
    })?;
    h.frames(2);
    let _ = h.shell.poll_events().len();
    let r = h.screen_rect();
    h.tap(egui::pos2(r.min.x + 8.0, r.min.y + 8.0));
    h.frame();
    // Swap a trigger in progress out whole.
    h.shell.add_hidden_entry(HiddenEntry::new(
        "factory",
        ZoneKnock::corners([Corner::TopLeft, Corner::TopRight]),
        LaunchAction::open("factory"),
    ));
    h.tap(egui::pos2(r.max.x - 8.0, r.min.y + 8.0));
    h.frames(2);
    assert!(
        opened(&mut h).is_empty(),
        "a new trigger starts from the first corner"
    );
    Ok(())
}
