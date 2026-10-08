//! The M2 headless integration test — the shell's own part.
//!
//! The cases: the `LaunchAction::OpenOverlay` gate, a status bar tap → the shade toggling plus
//! `nav_item("osk")`, a `tile()` declaration added and removed → `overlay().tile_rect` plus `DeclRemoved`,
//! idling at 0 fps (3 frames after a toast and a peek expire), `reduce` settling at once plus the drag
//! following, the back priority (the overlay → the OSK → `on_back` → pop),
//! `Command::{Notify, Toast, ToggleOverlay, SetMotion}`, the emergency progress ring, the gesture back's
//! progress when it is grabbed again and the release spring's settle scaling, `Toggle(wifi.enabled)` judging
//! against the backend, and the `status.notifications` default slot. Building with the features off is a command
//! (`--no-default-features --features mock --all-targets`).
//!
//! The rules: `fn … -> fairing::Result<()>`, and positions come from `tile_rect` / `item_rect` / `key_rect` / a widget `Rect`.
// Turning the `overlay` feature off drops this target from the build too (checked by `--no-default-features --all-targets`).
#![cfg(feature = "overlay")]

use fairing::chrome::{nav_item, NavItem, NavStyle};
use fairing::notify::HeadsUpPhase;
use fairing::overlay::OverlayState;
use fairing::settings::{SettingKey, SettingValue};
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::workspace::{StackTransition, Task};
use fairing::{
    screen, screen_with, tile, AccessEvent, BackAction, ChromePolicy, Cx, LaunchAction,
    Notification, NotificationId, Screen, Services, ShellEvent, TileKind,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The screen width (the reference board's).
const W: f32 = 1024.0;

/// The active task's stack depth (0 at home).
fn depth(h: &Harness) -> usize {
    h.shell.workspace().active_task().map_or(0, Task::len)
}

/// `Option` → `Result` (the tests do not use `unwrap`).
fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("missing: {what}")))
}

/// It runs frames until `done` goes true (at most `max`). The number of frames run.
fn settle(h: &mut Harness, max: usize, done: impl Fn(&Harness) -> bool) -> fairing::Result<usize> {
    for n in 0..=max {
        if done(h) {
            return Ok(n);
        }
        h.frame();
    }
    Err(fairing::Error::Config(format!(
        "the condition did not go true inside {max} frames"
    )))
}

/// Held down, it moves `step` at a time for `frames` frames from `from` (never letting go). The last position.
fn pull(h: &mut Harness, from: egui::Pos2, step: egui::Vec2, frames: usize) -> egui::Pos2 {
    h.press(from);
    h.frame();
    let mut pos = from;
    for _ in 0..frames {
        pos += step;
        h.move_to(pos);
        h.frame();
    }
    pos
}

/// Held down, `frames` frames in place (emptying the velocity window). It does not let go.
fn still(h: &mut Harness, pos: egui::Pos2, frames: usize) {
    for _ in 0..frames {
        h.move_to(pos);
        h.frame();
    }
}

/// A `reduce = false` shell (with the Null backends), plus two warm-up frames.
fn animated() -> fairing::Result<Harness> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    Ok(h)
}

/// It opens the shade by command and lets it settle.
fn open_shade(h: &mut Harness) -> fairing::Result<()> {
    h.shell.launch(LaunchAction::OpenOverlay);
    settle(h, 90, |h| h.shell.overlay().is_open())?;
    Ok(())
}

// ------------------------------------------------------- the overlay gate

/// `OpenOverlay` goes through the `overlay.open` gate. At level 2 with top it does not open and gives
/// `UnlockRequested`; at level 1 it opens. `back()` closes the shade first.
#[test]
fn open_overlay_respects_the_overlay_open_gate() -> fairing::Result<()> {
    let mut h = test_shell(access_config(&["viewer", "admin"], Some("top")), |_| {})?;
    h.frames(2);
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(2);
    assert!(h.shell.overlay().is_closed());
    let events = h.shell.poll_events();
    assert!(events.iter().any(|e| matches!(
        e,
        ShellEvent::Access(AccessEvent::UnlockRequested { gate, then })
            if gate.as_str() == "overlay.open" && *then == Some(LaunchAction::OpenOverlay)
    )));
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(2);
    assert!(
        h.shell.overlay().is_open(),
        "{:?}",
        h.shell.overlay().state()
    );
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OverlayToggled(true))));
    h.shell.back();
    h.frames(2);
    assert!(h.shell.overlay().is_closed(), "back closes the shade first");
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OverlayToggled(false))));
    Ok(())
}

// ----------------------------------------------------- the status bar tap

/// With `tap_opens_shade` a status bar tap opens the shade (the open goes through the `launch(OpenOverlay)`
/// path). An open shade covers the status bar, so the close is checked by command (`ToggleOverlay`), and tapping
/// again opens it once more. The tap is inside the top edge zone but does not cross the slop, so it is not a gesture.
#[test]
fn status_bar_tap_toggles_shade() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(2);
    let status = need(h.shell.layout().status, "Layout.status")?;
    // Tap a place with no item on it (the middle) — this is a tap on the bar itself, not an `ItemTapped`.
    h.tap(status.center());
    h.frames(2);
    assert!(h.shell.overlay().is_open(), "a tap → the shade opens");
    h.shell.handle().toggle_overlay();
    h.frames(3);
    assert!(
        h.shell.overlay().is_closed(),
        "a command toggle → it closes"
    );
    h.tap(status.center());
    h.frames(2);
    assert!(h.shell.overlay().is_open(), "another tap → it opens");
    // With `tap_opens_shade = false` a bar tap does nothing.
    let mut cfg = single_level_access();
    cfg.status_bar.tap_opens_shade = false;
    let mut h = test_shell(cfg, |_| {})?;
    h.frames(2);
    let status = need(h.shell.layout().status, "Layout.status")?;
    h.tap(status.center());
    h.frames(2);
    assert!(h.shell.overlay().is_closed());
    Ok(())
}

/// An integrator's `nav_item("osk")` declaration plus a nav item `Custom("osk")` tap → `Osk::toggle` →
/// `OskToggled(true)`, and tapping again → `OskToggled(false)`. Checked on an `OskMode::Manual` screen.
#[cfg(feature = "osk")]
#[test]
fn custom_osk_nav_item_toggles_the_osk() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |shell| {
        shell.add(nav_item("osk", |ui, _cx| {
            // A selectable label takes the click (egui's `selectable_labels`), so it is turned off — a tap the
            // closure does not consume comes to the shell as `NavAction::Custom("osk")`.
            ui.add(egui::Label::new("kbd").selectable(false));
        }));
        shell.add(
            screen("manual", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("manual");
            })
            .chrome(ChromePolicy {
                osk: fairing::screen::OskMode::Manual,
                ..ChromePolicy::default()
            }),
        );
    })?;
    h.shell.nav_bar_mut().style = NavStyle::Buttons {
        items: vec![
            NavItem::Back,
            NavItem::Home,
            NavItem::Custom("osk".to_owned()),
        ],
    };
    h.shell.launch(LaunchAction::open("manual"));
    h.frames(3);
    let item = need(
        h.shell
            .nav_bar()
            .item_rect(&NavItem::Custom("osk".to_owned())),
        "nav Custom(osk)",
    )?;
    let _ = h.shell.poll_events();
    h.tap(item.center());
    assert!(h.shell.osk().is_shown(), "a nav item tap opens it");
    assert!(need(h.shell.layout().osk, "Layout.osk")?.is_positive());
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OskToggled(true))));
    h.tap(item.center());
    assert!(!h.shell.osk().is_shown(), "one more tap closes it");
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OskToggled(false))));
    Ok(())
}

// ----------------------------------------------------- a tile declaration

/// `add(tile("heater", Toggle("app.heater")))` → a `tile_rect("heater")` on the shade, and a tap →
/// `SettingChanged { app.heater, true }`; `remove("heater")` → no Rect plus `DeclRemoved`.
#[test]
fn tile_decl_add_remove_and_events() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |shell| {
        shell.add(
            tile("heater", TileKind::Toggle(SettingKey::from("app.heater")))
                .icon(fairing::icon::THERMOMETER)
                .label("Heater"),
        );
    })?;
    h.frames(2);
    open_shade(&mut h)?;
    h.frames(2);
    let cell = need(h.shell.overlay().tile_rect("heater"), "tile_rect(heater)")?;
    assert!(
        h.shell
            .overlay()
            .tiles()
            .iter()
            .any(|(t, _, allowed)| t.id == "heater" && *allowed),
        "a declaration not in the settings list goes on the end"
    );
    let _ = h.shell.poll_events();
    h.tap(cell.center());
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::SettingChanged { key, value: SettingValue::Bool(true) }
                if key.0.as_ref() == "app.heater"
        )),
        "{events:?}"
    );
    assert!(h.shell.remove("heater"));
    h.frames(2);
    assert!(h.shell.overlay().tile_rect("heater").is_none());
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::DeclRemoved(id) if id == "heater")));
    assert!(
        !h.shell.remove("heater"),
        "an id that is not there is false"
    );
    Ok(())
}

// ---------------------------------------------------------- idle at 0 fps

/// Idle at 0 fps: 3 frames after a toast expires, and after a hidden screen's peek expires,
/// `repaint_requested == false` (the two-frame delay rule from the harness's comment).
#[test]
fn idle_after_toast_and_peek() -> fairing::Result<()> {
    let mut cfg = single_level_access();
    cfg.notify.toast_ms = 200;
    let mut h = Harness::new(cfg, Services::null())?;
    h.shell.add(
        screen("fs", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("fs");
        })
        .fullscreen(),
    );
    h.frames(2);
    h.shell.toast("hello");
    h.frames(2);
    assert_eq!(h.shell.toasts().visible().len(), 1);
    assert!(h.shell.is_animating(), "the entry tween");
    // 160 entering plus 200 holding plus 200 ms leaving.
    h.run_for(0.7);
    assert!(h.shell.toasts().visible().is_empty());
    h.frames(3);
    assert!(!h.repaint_requested, "idle once the toast has expired");
    assert!(h.shell.toasts().next_deadline().is_none());

    // The peek: a short pull and release on a hidden screen (32 px ≥ half the status bar's height, progress < the snap).
    h.shell.launch(LaunchAction::open("fs"));
    settle(&mut h, 90, |h| !h.shell.is_animating())?;
    assert!(
        h.shell.layout().status.is_none(),
        "the status bar is hidden"
    );
    let pos = pull(&mut h, egui::pos2(10.0, 8.0), egui::vec2(0.0, 8.0), 4);
    h.release(pos);
    h.frame();
    settle(&mut h, 90, |h| h.shell.overlay().is_closed())?;
    h.frame();
    assert!(h.shell.overlay().frame().peek.is_some(), "the peek row");
    assert!(h.shell.status_bar().item_rect("status.clock").is_some());
    assert!(
        h.shell.overlay().frame().peek.is_some() && !h.shell.is_animating(),
        "a peek is a scheduled wake, not an animation"
    );
    h.run_for(2.1);
    assert!(
        h.shell.overlay().frame().peek.is_none(),
        "it goes 2 s later"
    );
    h.frames(3);
    assert!(!h.repaint_requested, "idle once the peek has expired");
    Ok(())
}

// ----------------------------------------------------------------- reduce

/// `reduce`: the shade drag follows the finger as it is (frame 6 `y == 146`) and goes straight to
/// `Open`/`Closed` on the releasing frame; a page release lands on a whole number at once too.
#[test]
fn reduce_settles_shade_and_page_immediately_but_drag_still_follows() -> fairing::Result<()> {
    // The shade (the A1 board: 1024×568 → H = 480).
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    let mut h = Harness::new(cfg, Services::null())?.with_size(1024.0, 568.0);
    h.frames(2);
    let from = egui::pos2(10.0, 8.0);
    let to = egui::pos2(10.0, 300.0);
    h.press(from);
    h.frame();
    for i in 1u8..=6 {
        h.move_to(from + (to - from) * (f32::from(i) / 12.0));
        h.frame();
    }
    assert!(matches!(
        h.shell.overlay().state(),
        OverlayState::Dragging { .. }
    ));
    assert!(
        (h.shell.overlay().y() - 146.0).abs() < 0.5,
        "the drag is 1:1 even with reduce: y = {}",
        h.shell.overlay().y()
    );
    for i in 7u8..=12 {
        h.move_to(from + (to - from) * (f32::from(i) / 12.0));
        h.frame();
    }
    h.release(to);
    h.frame();
    assert!(
        h.shell.overlay().is_open(),
        "Open straight away on the releasing frame: {:?}",
        h.shell.overlay().state()
    );
    assert!((h.shell.overlay().y() - h.shell.overlay().height()).abs() < 1e-3);
    h.frames(3);
    assert!(!h.repaint_requested, "idle once settled");
    // Pulling up a little and letting go closes it at once. The place pressed comes out of the shade's **real height** —
    // a fixed coordinate would press outside the curtain the moment the bar's thickness changed and the drag would never start.
    let inside = h.shell.overlay().height() - 40.0;
    let pos = pull(&mut h, egui::pos2(10.0, inside), egui::vec2(0.0, -60.0), 5);
    h.release(pos);
    h.frame();
    assert!(
        h.shell.overlay().is_closed(),
        "{:?}",
        h.shell.overlay().state()
    );

    // The pages (N = 3): a whole-number position on the releasing frame.
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    cfg.desktop.columns = 1;
    cfg.desktop.rows = 1;
    let mut h = Harness::new(cfg, Services::null())?;
    for id in ["p0", "p1", "p2"] {
        h.shell.add(
            screen(id, |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("page");
            })
            .title(id)
            .icon(fairing::icon::FOLDER)
            .desktop(),
        );
    }
    h.frames(3);
    assert_eq!(h.shell.desktop().pages().len(), 3);
    let start = egui::pos2(W / 2.0, 200.0);
    h.press(start);
    h.frame();
    let mut pos = start;
    for _ in 0..20 {
        pos += egui::vec2(-15.0, 0.0);
        h.move_to(pos);
        h.frame();
    }
    assert!(h.shell.desktop().swipe().is_dragging());
    assert!(
        (h.shell.desktop().page_pos() - 300.0 / W).abs() < 1e-2,
        "the drag follows: {}",
        h.shell.desktop().page_pos()
    );
    h.release(pos);
    h.frame();
    assert!(
        !h.shell.desktop().swipe().is_animating(),
        "reduce: it settles at once"
    );
    assert!((h.shell.desktop().page_pos() - 1.0).abs() < 1e-6);
    assert_eq!(h.shell.desktop().page(), 1);
    Ok(())
}

/// `reduce` (the OSK): a `TextEdit` tap → `inset_bottom == osk_h` on the showing frame.
#[cfg(feature = "osk")]
#[test]
fn reduce_shows_the_osk_at_full_height_immediately() -> fairing::Result<()> {
    let rect = Rc::new(Cell::new(egui::Rect::NOTHING));
    let seen = Rc::clone(&rect);
    let mut h = test_shell(single_level_access(), move |shell| {
        let text = Rc::new(RefCell::new(String::new()));
        shell.add(screen("form", move |ui: &mut egui::Ui, _: &mut Cx| {
            let mut s = text.borrow_mut();
            let r = ui.add_sized([300.0, 48.0], egui::TextEdit::singleline(&mut *s));
            seen.set(r.rect);
        }));
    })?;
    h.shell.launch(LaunchAction::open("form"));
    h.frames(3);
    let field = rect.get();
    assert!(field.is_positive(), "TextEdit Rect");
    h.press(field.center());
    h.frame();
    h.release(field.center());
    h.frame();
    let mut shown_frame = None;
    for i in 0..4 {
        if h.shell.osk().is_shown() {
            shown_frame = Some(i);
            break;
        }
        h.frame();
    }
    assert!(shown_frame.is_some(), "the OSK shows");
    let osk_h = h.shell.osk().height();
    assert!(osk_h > 0.0);
    assert!(
        (h.shell.osk().inset_bottom() - osk_h).abs() < 0.5,
        "inset = osk_h on the showing frame: {} vs {osk_h}",
        h.shell.osk().inset_bottom()
    );
    assert!(!h.shell.osk().is_animating());
    Ok(())
}

// ------------------------------------------------------- the back priority

/// A screen that eats `on_back` once (Consumed) and passes it on after that.
struct Consumer {
    left: u32,
    text: String,
}

impl Screen for Consumer {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.add_sized(
            [300.0, 48.0],
            egui::TextEdit::singleline(&mut self.text).id_salt("consumer"),
        );
    }

    fn on_back(&mut self, _cx: &mut Cx<'_>) -> BackAction {
        if self.left > 0 {
            self.left -= 1;
            BackAction::Consumed
        } else {
            BackAction::Pop
        }
    }
}

/// Back goes (the prompt, M3) → close the overlay → close the OSK → `on_back` → pop → ignore it at home,
/// in that order. Each step eats exactly one `back()`.
#[cfg(feature = "osk")]
#[test]
fn back_priority_is_overlay_then_osk_then_on_back_then_pop() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |shell| {
        shell.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("a");
        }));
        shell.add(screen_with("b", || Consumer {
            left: 1,
            text: String::new(),
        }));
    })?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    assert_eq!(depth(&h), 2);
    // The OSK: tap the TextEdit to take the focus → it shows.
    let content = h.shell.layout().content;
    h.tap(content.min + egui::vec2(60.0, 40.0));
    settle(&mut h, 10, |h| h.shell.osk().is_shown())?;
    // Open the shade too.
    open_shade(&mut h)?;
    h.frames(2);
    assert!(h.shell.overlay().is_open() && h.shell.osk().is_shown());
    let _ = h.shell.poll_events();

    h.shell.back();
    h.frames(2);
    assert!(h.shell.overlay().is_closed(), "1) the overlay");
    assert!(h.shell.osk().is_shown(), "the OSK is still there");
    assert_eq!(depth(&h), 2);

    h.shell.back();
    h.frames(2);
    assert!(!h.shell.osk().is_shown(), "2) OSK");
    assert!(
        h.shell.osk().is_dismissed(),
        "it does not come back up with the focus still there"
    );
    assert_eq!(depth(&h), 2);

    h.shell.back();
    h.frames(2);
    assert_eq!(depth(&h), 2, "3) on_back ate it");
    assert!(!h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { .. })));

    h.shell.back();
    h.frames(2);
    assert_eq!(depth(&h), 1, "4) pop");
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "b")));

    h.shell.back();
    h.frames(2);
    assert!(h.shell.workspace().is_home(), "a root pop = home");
    h.shell.back();
    h.frames(2);
    assert!(h.shell.workspace().is_home(), "ignored at home");
    Ok(())
}

// ------------------------------------------------- the handle commands

/// `Command::{Notify, Toast, ToggleOverlay, SetMotion}` land on the next frame — the same path from any thread
/// (mpsc, two stages).
#[test]
fn handle_commands_notify_toast_toggle_overlay_and_set_motion() -> fairing::Result<()> {
    let mut h = animated()?;
    let handle = h.shell.handle();
    std::thread::scope(|s| {
        s.spawn(move || {
            handle.notify(Notification::new(NotificationId::of("job"), "job").body("run"));
            handle.toast("hi");
            handle.toggle_overlay();
        });
    });
    h.frames(2);
    assert_eq!(h.shell.notifications().len(), 1);
    assert_eq!(h.shell.toasts().visible().len(), 1);
    assert!(
        !h.shell.overlay().is_closed(),
        "{:?}",
        h.shell.overlay().state()
    );
    // Once the shade leaves `Closed` a heads-up is absorbed and the unread count is 0.
    assert!(h.shell.heads_up().visible().is_none());
    assert_eq!(h.shell.notifications().unread(), 0);
    settle(&mut h, 90, |h| h.shell.overlay().is_open())?;
    h.shell.handle().toggle_overlay();
    h.frames(2);
    assert!(matches!(
        h.shell.overlay().state(),
        OverlayState::Settling { opening: false }
    ));
    // `SetMotion(reduce)`: from the next frame a new transition is instant.
    let mut tokens = h.shell.theme().motion;
    tokens.reduce = true;
    tokens.push = fairing::motion::Tween::instant();
    h.shell.handle().set_motion(tokens);
    h.frames(2);
    assert!(h.shell.theme().motion.reduce);
    settle(&mut h, 90, |h| h.shell.overlay().is_closed())?;
    h.shell.handle().toggle_overlay();
    h.frames(2);
    assert!(
        h.shell.overlay().is_open(),
        "with reduce tokens the open is instant too: {:?}",
        h.shell.overlay().state()
    );
    Ok(())
}

/// A heads-up shows **only while the shade is closed** — a notification arriving with it open goes straight to the list.
#[test]
fn heads_up_only_while_the_shade_is_closed() -> fairing::Result<()> {
    let mut h = animated()?;
    h.shell
        .notify(Notification::new(NotificationId::of("a"), "a"));
    h.frame();
    assert_eq!(h.shell.heads_up().phase(), Some(HeadsUpPhase::Entering));
    open_shade(&mut h)?;
    assert!(
        h.shell.heads_up().visible().is_none(),
        "open, it is absorbed"
    );
    h.shell
        .notify(Notification::new(NotificationId::of("b"), "b"));
    h.frames(2);
    assert!(
        h.shell.heads_up().visible().is_none(),
        "no banner while it is open"
    );
    assert_eq!(h.shell.notifications().len(), 2);
    Ok(())
}

// ----------------------------------------------------- the emergency progress ring

/// A7: pressing the top corner of an `edge_guard` screen fills the progress ring linearly (0.5 at 1 s),
/// and lifting the finger plays it back over 80 ms — the shell keeps repainting throughout.
#[test]
fn emergency_ring_fills_linearly_and_rewinds_after_release() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.shell.add(
        screen("guard", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("guard");
        })
        .chrome(ChromePolicy {
            edge_guard: true,
            ..ChromePolicy::default()
        }),
    );
    h.frames(2);
    h.shell.launch(LaunchAction::open("guard"));
    settle(&mut h, 90, |h| !h.shell.is_animating())?;
    let corner = egui::pos2(1010.0, 10.0);
    // 60 frames = 1 s (half of 2 s).
    h.hold(corner, 60);
    let p = h.shell.policy_driver().emergency_progress();
    assert!((p - 0.5).abs() < 0.05, "half at 1 s: {p}");
    assert!(h.shell.policy_driver().emergency_at().is_some());
    assert!(h.shell.is_animating(), "the ring is moving");
    h.release(corner);
    h.frame();
    h.frame();
    let p = h.shell.policy_driver().emergency_progress();
    assert!(p > 0.0 && p < 0.5, "playing back: {p}");
    assert!(h.shell.is_animating(), "playing back is an animation too");
    assert!(
        h.repaint_requested,
        "it keeps going even where the ring is the only reason to repaint"
    );
    h.run_for(0.2);
    assert!(
        h.shell.policy_driver().emergency_progress().abs() < 1e-6,
        "0 after 80 ms"
    );
    assert!(!h.shell.is_animating());
    h.frames(3);
    assert!(!h.repaint_requested, "idle once the ring is off");
    assert!(
        !h.shell
            .poll_events()
            .iter()
            .any(|e| matches!(e, ShellEvent::Emergency)),
        "short of 2 s it is not an emergency"
    );
    Ok(())
}

// ------------------------------------------------- the gesture back (A3)

/// Two screens a → b, on a `reduce = false` shell that takes the left-edge back.
fn two_screens() -> fairing::Result<Harness> {
    let mut h = animated()?;
    for id in ["a", "b"] {
        h.shell.add(screen(id, |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("s");
        }));
    }
    h.shell.launch(LaunchAction::open("a"));
    settle(&mut h, 90, |h| !h.shell.is_animating())?;
    h.shell.launch(LaunchAction::open("b"));
    settle(&mut h, 90, |h| !h.shell.is_animating())?;
    assert_eq!(depth(&h), 2);
    Ok(h)
}

/// A3's "letting go gives a Spring": after a cancelling release the return spring really does run for several
/// frames — the result of clamping the settle test to `1 / W` because `p` is a normalised value (`Animated::release_scaled`).
#[test]
fn gesture_back_release_spring_takes_real_frames() -> fairing::Result<()> {
    let mut h = two_screens()?;
    let start = egui::pos2(5.0, 300.0);
    let end = pull(&mut h, start, egui::vec2(10.0, 0.0), 20);
    assert!(matches!(
        h.shell.workspace().stack_transition(),
        StackTransition::DraggingBack {
            confirmed: None,
            ..
        }
    ));
    let p = h.shell.workspace().stack_transition().t();
    assert!((p - 200.0 / W).abs() < 0.02, "1:1: {p}");
    still(&mut h, end, 12);
    h.release(end);
    h.frame();
    assert_eq!(
        h.shell.workspace().stack_transition().back_confirmed(),
        Some(false),
        "0.2 W · v ≈ 0 → cancelled"
    );
    let mut frames = 0;
    while h.shell.is_animating() {
        h.frame();
        frames += 1;
        assert!(frames < 120, "the return does not end");
    }
    assert!(
        frames >= 8,
        "the return spring really does run: {frames} frames"
    );
    assert_eq!(depth(&h), 2);
    Ok(())
}

/// A3's "grabbed again mid-cancel it carries on from the current p": the release after a re-grab is judged on
/// **where it was grabbed plus the new dx / W** — the new dx alone falls short (0.2 W) but the sum crosses the snap (0.33) and confirms.
#[test]
fn gesture_back_regrab_confirms_with_the_combined_progress() -> fairing::Result<()> {
    let mut h = two_screens()?;
    let start = egui::pos2(5.0, 300.0);
    // Drag to 0.3 W, stop and let go → the cancelling return starts.
    let end = pull(&mut h, start, egui::vec2(15.0, 0.0), 20);
    still(&mut h, end, 12);
    h.release(end);
    h.frame();
    assert_eq!(
        h.shell.workspace().stack_transition().back_confirmed(),
        Some(false)
    );
    h.frames(2);
    let grab_at = h.shell.workspace().stack_transition().t();
    assert!(grab_at > 0.15 && grab_at < 0.3, "mid-return: {grab_at}");
    // Grab it again and drag only 0.2 W more (short of 0.33 on its own).
    let end = pull(&mut h, start, egui::vec2(10.0, 0.0), 20);
    // The return advances a little over the two frames that cross the slop, so where it was grabbed is slightly below the value read.
    let grab = h.shell.workspace().gesture_back_grab();
    assert!(
        grab > 0.05 && grab <= grab_at + 1e-3,
        "it carries on from where it was grabbed: {grab} (the value read was {grab_at})"
    );
    let p = h.shell.workspace().stack_transition().t();
    assert!(p > 0.33, "the summed progress: {p}");
    // Drawn at the caught `p` plus the new dx / W (200 px of 1024), the grab counted once.
    let expected = (grab + 200.0 / 1024.0).clamp(0.0, 1.0);
    assert!(
        (p - expected).abs() < 0.03,
        "drawn at {p}, while the release is judged on {expected}"
    );
    still(&mut h, end, 12);
    let _ = h.shell.poll_events();
    h.release(end);
    h.frame();
    assert_eq!(
        h.shell.workspace().stack_transition().back_confirmed(),
        Some(true),
        "the sum crosses the snap and confirms"
    );
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "b")));
    settle(&mut h, 120, |h| !h.shell.is_animating())?;
    assert_eq!(depth(&h), 1);
    Ok(())
}

// --------------------------------------------------------- the settings toggle

/// `LaunchAction::Toggle(wifi.enabled)` writes the opposite of **the backend's current value** even with an empty
/// memory view — it does not turn an already-on Mock Wi-Fi on again on a fresh shell. An integrator's key goes by
/// the memory view (`None → true`).
#[test]
fn toggle_uses_the_backend_state_for_builtin_keys() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.shell.services_mut().wifi = Box::new(fairing::services::mock::MockWifi::new());
    h.frames(2);
    assert!(h.shell.services().wifi.enabled(), "the Mock starts on");
    let _ = h.shell.poll_events();
    h.shell
        .launch(LaunchAction::Toggle(SettingKey::from("wifi.enabled")));
    let events = h.shell.poll_events();
    assert!(
        events.iter().any(|e| matches!(
            e,
            ShellEvent::SettingChanged { key, value: SettingValue::Bool(false) }
                if key.0.as_ref() == "wifi.enabled"
        )),
        "{events:?}"
    );
    assert!(!h.shell.services().wifi.enabled(), "it went off");
    h.shell
        .launch(LaunchAction::Toggle(SettingKey::from("wifi.enabled")));
    assert!(h.shell.services().wifi.enabled(), "it comes back on");
    // An integrator's key: by the memory view.
    h.shell
        .launch(LaunchAction::Toggle(SettingKey::from("app.heater")));
    let events = h.shell.poll_events();
    assert!(events.iter().any(|e| matches!(
        e,
        ShellEvent::SettingChanged { key, value: SettingValue::Bool(true) }
            if key.0.as_ref() == "app.heater"
    )));
    Ok(())
}

// --------------------------------------------------------- the default slots

/// The head of the default `[status_bar] right` is `status.notifications` — the badge is drawn without the demos
/// and the tests putting it in by hand. The order on the right is guide 03's example as it is.
#[test]
fn status_notifications_is_in_the_default_right_slot() -> fairing::Result<()> {
    // The Null backends hide the Wi-Fi, BT and battery items (they report no capability), so it is built with Mocks.
    let mut cfg = single_level_access();
    cfg.motion.reduce = true;
    let mut h = Harness::new(cfg, fairing::services::mock::services())?;
    h.frames(2);
    let bell = need(
        h.shell.status_bar().item_rect("status.notifications"),
        "status.notifications",
    )?;
    let bt = need(h.shell.status_bar().item_rect("status.bluetooth"), "bt")?;
    assert!(
        bell.max.x <= bt.min.x + 0.5,
        "the bell is to the left of Bluetooth: {bell:?} {bt:?}"
    );
    h.shell
        .notify(Notification::new(NotificationId::of("n"), "n"));
    h.frames(2);
    assert_eq!(h.shell.status_bar().unread(), 1);
    // A bell tap → the shade (the built-in `tap_action = OpenOverlay`).
    h.tap(bell.center());
    h.frames(2);
    assert!(!h.shell.overlay().is_closed());
    Ok(())
}
