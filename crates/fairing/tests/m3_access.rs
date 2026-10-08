//! M3 — the shell's own prompt and lock screen (A9).
//!
//! What a device gets in `mode = "prompt"`: a failed gate opens the prompt over the screen, the
//! authenticator decides, a grant runs what was asked for through the gate again; the session
//! comes back down by itself (a temporary unlock running out, the timeout, the idle lock) and the
//! lock screen stands between an idle panel and the next person.
//!
//! The PINs come from `[access.pin_table]` — the reference `PinTable` — except where a test needs
//! a method the table does not offer (a password, a badge reader); those bring an authenticator of
//! their own, as an integrator would.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::access::{
    AccessEvent, AuthMethod, AuthOutcome, Authenticator, ChangeReason, Credential, Subject,
    UnlockMode,
};
use fairing::testing::{access_config, access_config_mode, test_shell, Harness};
use fairing::{screen, screen_with, Cx, LaunchAction, Level, Lifecycle, ShellConfig, ShellEvent};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// Three levels, everything at the top unless said otherwise, and a PIN for the two upper ones.
fn pins() -> ShellConfig {
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.access.pin_table.pins = [("operator", "1234"), ("maintainer", "9876")]
        .iter()
        .map(|(level, pin)| ((*level).to_owned(), (*pin).to_owned()))
        .collect();
    config
}

/// A shell with an `admin` screen behind the `admin` gate (the top level, by `default_gate`).
fn shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = test_shell(config, |sh| {
        sh.add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("calibration");
        }));
    })?;
    h.frames(2);
    Ok(h)
}

/// The topmost drawn text that reads exactly `wanted` — the prompt is above everything it would
/// otherwise be confused with, and shapes come out in layer order.
fn text_rect(h: &mut Harness, wanted: &str) -> Option<egui::Rect> {
    fn walk(shape: &egui::Shape, wanted: &str, found: &mut Option<egui::Rect>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                *found = Some(text.galley.rect.translate(text.pos.to_vec2()));
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

fn tap_text(h: &mut Harness, wanted: &str) -> fairing::Result<()> {
    let rect = text_rect(h, wanted).ok_or_else(|| fail(format!("`{wanted}` is not drawn")))?;
    h.tap(rect.center());
    Ok(())
}

/// Tap a PIN in on the keypad, key by key, wherever the keys are.
fn tap_pin(h: &mut Harness, pin: &str) -> fairing::Result<()> {
    for digit in pin.chars() {
        tap_text(h, &digit.to_string())?;
    }
    h.frames(2);
    Ok(())
}

/// Type a PIN on a hardware keypad.
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

/// A locked screen asks instead of opening — the contract M1 held back.
#[test]
fn locked_screen_prompts_instead_of_opening() -> fairing::Result<()> {
    let mut h = shell(pins())?;
    h.shell.handle().launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_none());
    assert!(matches!(
        access_events(&mut h).as_slice(),
        [AccessEvent::UnlockRequested { .. }]
    ));
    Ok(())
}

/// The whole road: the keypad, the grant, the screen opening through the gate again.
#[test]
fn the_right_pin_on_the_keypad_opens_what_was_asked_for() -> fairing::Result<()> {
    let mut h = shell(pins())?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    let _ = h.shell.poll_events();
    tap_pin(&mut h, "9876")?;
    assert!(!h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_some());
    assert_eq!(level(&h), Level(2));
    let events = h.shell.poll_events();
    let access: Vec<&AccessEvent> = events
        .iter()
        .filter_map(|e| match e {
            ShellEvent::Access(a) => Some(a),
            _ => None,
        })
        .collect();
    assert!(
        matches!(
            access.as_slice(),
            [
                AccessEvent::Unlocked {
                    level: Level(2),
                    mode: UnlockMode::Temporary,
                    subject_id: None,
                    ..
                },
                AccessEvent::SessionChanged {
                    from: Level(0),
                    to: Level(2),
                    reason: ChangeReason::Unlock
                },
            ]
        ),
        "{access:?}"
    );
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "admin")));
    // No event ever carries what was typed.
    assert!(!format!("{events:?}").contains("9876"));
    Ok(())
}

/// A wrong PIN is refused in the authenticator's own words, and the prompt stays.
#[test]
fn a_wrong_pin_is_refused_in_the_authenticators_words() -> fairing::Result<()> {
    let mut h = shell(pins())?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    let _ = h.shell.poll_events();
    type_pin(&mut h, "0000");
    assert!(h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_none());
    assert!(text_rect(&mut h, "Wrong PIN").is_some());
    assert!(matches!(
        access_events(&mut h).as_slice(),
        [AccessEvent::Denied { gate }] if gate.as_str() == "admin"
    ));
    // Still listening.
    type_pin(&mut h, "9876");
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// Three levels and a pattern for the top one — `[access.pattern_table]`, no PINs.
fn patterns() -> ShellConfig {
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config
        .access
        .pattern_table
        .patterns
        .insert("maintainer".to_owned(), "1-2-3-5-7-8-9".to_owned());
    config
}

/// Draw a pattern on the prompt's dots the way a finger would: down on the first, through the
/// rest a frame a dot, up on the last. The dots count from 1, as `[access.pattern_table]` writes
/// them.
fn draw_pattern(h: &mut Harness, dots: &[u8]) -> fairing::Result<()> {
    let mut points = Vec::with_capacity(dots.len());
    for dot in dots {
        let at = h
            .shell
            .prompt_dot_center(dot.saturating_sub(1))
            .ok_or_else(|| fail(format!("dot {dot} is not drawn")))?;
        points.push(at);
    }
    let (Some(first), Some(last)) = (points.first().copied(), points.last().copied()) else {
        return Err(fail("a pattern needs a dot"));
    };
    h.press(first);
    h.frame();
    for at in points.iter().skip(1) {
        h.move_to(*at);
        h.frame();
    }
    h.release(last);
    h.frame();
    h.frame();
    Ok(())
}

/// A path through the dots is a way in like a PIN — the grant, the screen opening
/// through its gate again.
#[test]
fn a_pattern_on_the_dots_opens_what_was_asked_for() -> fairing::Result<()> {
    let mut h = shell(patterns())?;
    h.shell.launch(LaunchAction::open("admin"));
    h.run_for(0.3);
    let _ = h.shell.poll_events();
    assert!(text_rect(&mut h, "Draw pattern").is_some());
    // A stroke from 3 to 7 crosses 5, so the Z is drawn through it, as the table writes it.
    draw_pattern(&mut h, &[1, 2, 3, 5, 7, 8, 9])?;
    assert!(!h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_some());
    assert_eq!(level(&h), Level(2));
    Ok(())
}

/// A path under `min_points` is answered on the spot and spends nothing; a wrong one is refused
/// in the table's words and counts — so with an attempt limit of two the right one still gets in.
#[test]
fn a_short_pattern_costs_nothing_and_a_wrong_one_is_refused() -> fairing::Result<()> {
    let mut config = patterns();
    config.access.pattern_table.attempt_limit = Some(2);
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.run_for(0.3);
    let _ = h.shell.poll_events();
    draw_pattern(&mut h, &[1, 2, 3])?;
    assert!(text_rect(&mut h, "Connect at least 4 dots").is_some());
    assert!(
        access_events(&mut h).is_empty(),
        "nothing reached the authenticator"
    );
    draw_pattern(&mut h, &[1, 4, 7, 8])?;
    assert!(text_rect(&mut h, "Wrong pattern").is_some());
    assert!(matches!(
        access_events(&mut h).as_slice(),
        [AccessEvent::Denied { .. }]
    ));
    h.run_for(0.4);
    draw_pattern(&mut h, &[1, 2, 3, 5, 7, 8, 9])?;
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// With a PIN table and a pattern table the prompt has a tab for each, and either gets in.
#[test]
fn both_tables_give_the_prompt_a_tab_each() -> fairing::Result<()> {
    let mut config = pins();
    config.access.pattern_table = patterns().access.pattern_table;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.run_for(0.3);
    assert!(text_rect(&mut h, "Enter PIN").is_some());
    tap_text(&mut h, "Pattern")?;
    h.run_for(0.2);
    assert!(text_rect(&mut h, "Draw pattern").is_some());
    draw_pattern(&mut h, &[1, 2, 3, 5, 7, 8, 9])?;
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// `max_len`: with PINs of different lengths the keypad has no length to stop at, and stops at
/// `max_len` instead — a seventh digit on a cap of six goes nowhere.
#[test]
fn max_len_stops_the_keypad_taking_digits() -> fairing::Result<()> {
    let mut config = pins();
    config.access.pin_table.pins = [("operator", "1234"), ("maintainer", "987654")]
        .iter()
        .map(|(level, pin)| ((*level).to_owned(), (*pin).to_owned()))
        .collect();
    config.access.pin_table.max_len = Some(6);
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "9876543");
    h.key(egui::Key::Enter);
    h.frames(2);
    assert!(
        h.shell.workspace().find("admin").is_some(),
        "the seventh digit was never taken"
    );
    Ok(())
}

/// An operator PIN on a maintainer gate is a real grant — the session goes up to operator — and
/// the gate, checked again, asks again.
#[test]
fn a_pin_for_a_lower_level_unlocks_that_level_and_asks_again() -> fairing::Result<()> {
    let mut h = shell(pins())?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "1234");
    assert_eq!(level(&h), Level(1));
    assert!(h.shell.unlock_prompt_visible(), "the launch asked again");
    assert!(h.shell.workspace().find("admin").is_none());
    Ok(())
}

/// `attempt_limit` greys the keypad for `lock_secs`; even the right PIN waits it out.
#[test]
fn the_attempt_limit_locks_the_keypad_until_it_runs_out() -> fairing::Result<()> {
    let mut config = pins();
    config.access.pin_table.attempt_limit = Some(2);
    config.access.pin_table.lock_secs = Some(1);
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    let _ = h.shell.poll_events();
    type_pin(&mut h, "0000");
    type_pin(&mut h, "1111");
    let events = access_events(&mut h);
    assert!(
        matches!(
            events.as_slice(),
            [AccessEvent::Denied { .. }, AccessEvent::Locked { .. }]
        ),
        "{events:?}"
    );
    type_pin(&mut h, "9876");
    assert!(
        h.shell.workspace().find("admin").is_none(),
        "the keys are deaf"
    );
    h.run_for(1.1);
    type_pin(&mut h, "9876");
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// `unlock_mode = "temporary"`: the unlock runs out by itself, and the screen it opened closes
/// with it (a downgrade ends what no longer passes).
#[test]
fn a_temporary_unlock_runs_out_and_takes_its_screen_with_it() -> fairing::Result<()> {
    let mut config = pins();
    config.access.temporary_secs = 1;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "9876");
    assert!(h.shell.workspace().find("admin").is_some());
    let _ = h.shell.poll_events();
    h.run_for(1.2);
    assert_eq!(level(&h), Level(0));
    assert!(h.shell.workspace().find("admin").is_none());
    assert!(access_events(&mut h).iter().any(|e| matches!(
        e,
        AccessEvent::SessionChanged {
            reason: ChangeReason::ElevationExpired,
            ..
        }
    )));
    Ok(())
}

/// The deadlines run on the wall clock, not on drawn frames. A reactive panel draws nothing while
/// nothing moves — it sleeps until the next deadline or touch — and a temporary unlock still has
/// to end when its time is up, even if not one frame was drawn in between.
#[test]
fn a_temporary_unlock_runs_out_on_a_panel_that_sleeps() -> fairing::Result<()> {
    let mut config = pins();
    config.access.temporary_secs = 300;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "9876");
    assert_eq!(level(&h), Level(2));
    h.frames(30);
    // Five minutes and a second of an idle panel: no frames, then one wake.
    h.sleep(301.0);
    h.frames(2);
    assert_eq!(level(&h), Level(0), "the unlock outlived its five minutes");
    assert!(h.shell.workspace().find("admin").is_none());
    Ok(())
}

/// The idle lock counts the time the panel slept, too.
#[test]
fn the_idle_lock_counts_the_time_the_panel_slept() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 60;
    let mut h = shell(config)?;
    h.frames(10);
    h.sleep(61.0);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    Ok(())
}

/// `unlock_mode = "switch"` lasts — until `session_timeout_secs` pass with nobody touching the
/// panel. A touch puts the timeout back.
#[test]
fn a_switch_unlock_lasts_until_the_session_times_out() -> fairing::Result<()> {
    let mut config = pins();
    config.access.unlock_mode = "switch".to_owned();
    config.access.session_timeout_secs = 1;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "9876");
    h.run_for(0.6);
    h.move_to(egui::pos2(500.0, 300.0));
    h.run_for(0.6);
    assert_eq!(level(&h), Level(2), "the touch at 0.6 s put it back");
    let _ = h.shell.poll_events();
    h.run_for(0.6);
    assert_eq!(level(&h), Level(0));
    assert!(h.shell.workspace().find("admin").is_none());
    assert!(access_events(&mut h).iter().any(|e| matches!(
        e,
        AccessEvent::SessionChanged {
            reason: ChangeReason::Timeout,
            ..
        }
    )));
    Ok(())
}

/// `idle_lock_secs`: the lock screen comes up over an idle panel and a PIN takes it away.
#[test]
fn the_idle_lock_brings_up_the_lock_screen_and_a_pin_leaves_it() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 1;
    let mut h = shell(config)?;
    h.run_for(1.2);
    assert!(h.shell.lock_screen_visible());
    let events = h.shell.poll_events();
    assert!(events.contains(&ShellEvent::LockRequested));
    assert!(events.contains(&ShellEvent::Access(AccessEvent::LockScreenToggled(true))));
    type_pin(&mut h, "1234");
    assert!(!h.shell.lock_screen_visible());
    assert_eq!(level(&h), Level(1));
    let events = access_events(&mut h);
    assert!(events.iter().any(|e| matches!(
        e,
        AccessEvent::Unlocked { gate, .. } if gate.as_str() == "session.lock"
    )));
    assert!(events.contains(&AccessEvent::LockScreenToggled(false)));
    Ok(())
}

/// A locked panel is not locked again: another quiet stretch on the lock screen does not tell the
/// device to lock once more.
#[test]
fn a_lock_screen_left_alone_is_not_locked_twice() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 1;
    let mut h = shell(config)?;
    h.run_for(1.2);
    assert!(h.shell.lock_screen_visible());
    let _ = h.shell.poll_events();
    h.move_to(egui::pos2(500.0, 300.0));
    h.run_for(1.2);
    assert!(h.shell.lock_screen_visible());
    assert!(!h.shell.poll_events().contains(&ShellEvent::LockRequested));
    Ok(())
}

/// Continue is there only where `[access.lock_screen] allow_continue` says so, and leaves as the
/// starting subject.
#[test]
fn continue_leaves_the_lock_screen_only_where_allowed() -> fairing::Result<()> {
    let mut closed = pins();
    closed.access.idle_lock_secs = 1;
    let mut h = shell(closed.clone())?;
    h.run_for(1.2);
    assert!(text_rect(&mut h, "Continue").is_none());

    let mut open = closed;
    open.access.lock_screen.allow_continue = true;
    let mut h = shell(open)?;
    h.run_for(1.2);
    let _ = h.shell.poll_events();
    tap_text(&mut h, "Continue")?;
    h.frames(2);
    assert!(!h.shell.lock_screen_visible());
    assert_eq!(level(&h), Level(0));
    assert!(access_events(&mut h).contains(&AccessEvent::LockScreenToggled(false)));
    Ok(())
}

/// Back is the prompt's first: it cancels an unlock prompt, and does nothing at all on the
/// lock screen — the way out of that is the prompt.
#[test]
fn back_cancels_an_unlock_prompt_but_not_the_lock_screen() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 2;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    h.shell.back();
    h.frames(1);
    assert!(!h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_none());
    h.run_for(2.2);
    assert!(h.shell.lock_screen_visible());
    h.shell.back();
    h.frames(1);
    assert!(h.shell.lock_screen_visible());
    Ok(())
}

/// In `prompt` mode the shell carries out the lock and the logout it reports (over):
/// the lock screen, and back to the starting subject.
#[test]
fn lock_and_logout_are_carried_out_in_prompt_mode() -> fairing::Result<()> {
    let mut config = pins();
    config.access.unlock_mode = "switch".to_owned();
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "9876");
    let _ = h.shell.poll_events();

    h.shell.launch(LaunchAction::Logout);
    h.frames(1);
    assert_eq!(level(&h), Level(0));
    let events = h.shell.poll_events();
    assert!(events.contains(&ShellEvent::LogoutRequested));
    assert!(events.iter().any(|e| matches!(
        e,
        ShellEvent::Access(AccessEvent::SessionChanged {
            reason: ChangeReason::Logout,
            ..
        })
    )));

    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    let events = h.shell.poll_events();
    assert!(events.contains(&ShellEvent::LockRequested));
    assert!(events.contains(&ShellEvent::Access(AccessEvent::LockScreenToggled(true))));
    Ok(())
}

/// `routing` never draws the prompt, PIN table or not — authentication is the integrator's.
#[test]
fn routing_never_prompts_even_with_pins() -> fairing::Result<()> {
    let mut config = pins();
    config.access.mode = "routing".to_owned();
    config.access.idle_lock_secs = 1;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(!h.shell.unlock_prompt_visible());
    h.run_for(1.2);
    assert!(!h.shell.lock_screen_visible());
    // The idle lock is still reported, for the integrator's own lock screen.
    assert!(h.shell.poll_events().contains(&ShellEvent::LockRequested));
    Ok(())
}

/// A shuffled keypad is a different layout, and the digits still say what they are.
#[test]
fn a_shuffled_keypad_still_takes_the_right_digits() -> fairing::Result<()> {
    let mut config = pins();
    config.access.pin_table.shuffle = true;
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    tap_pin(&mut h, "9876")?;
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// A screen of the integrator's that writes its lifecycle down.
struct Recorder {
    log: Rc<RefCell<Vec<Lifecycle>>>,
}

impl fairing::Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("r");
    }

    fn on_lifecycle(&mut self, event: Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push(event);
    }
}

/// The prompt covers the focused screen, so it is `Paused` under it and `Resumed` after.
#[test]
fn the_prompt_pauses_the_screen_under_it() -> fairing::Result<()> {
    let mut config = pins();
    config
        .access
        .gates
        .insert("r".to_owned(), "viewer".to_owned());
    let log = Rc::new(RefCell::new(Vec::new()));
    let l = Rc::clone(&log);
    let mut h = shell(config)?;
    h.shell
        .add(screen_with("r", move || Recorder { log: Rc::clone(&l) }));
    h.frames(1);
    h.shell.launch(LaunchAction::open("r"));
    h.frames(3);
    log.borrow_mut().clear();
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(3);
    assert_eq!(*log.borrow(), vec![Lifecycle::Paused]);
    h.shell.back();
    h.frames(3);
    assert_eq!(*log.borrow(), vec![Lifecycle::Paused, Lifecycle::Resumed]);
    Ok(())
}

/// `status.lock` is an open padlock while the session is above its start; a tap drops back.
#[test]
fn the_status_padlock_shows_while_unlocked_and_a_tap_drops_back() -> fairing::Result<()> {
    let mut config = pins();
    config.access.unlock_mode = "switch".to_owned();
    config.status_bar.right = vec!["status.lock".to_owned()];
    config
        .access
        .gates
        .insert("status.lock".to_owned(), "bottom".to_owned());
    let mut h = shell(config)?;
    assert!(h.shell.status_bar().item_rect("status.lock").is_none());
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "9876");
    let padlock = h
        .shell
        .status_bar()
        .item_rect("status.lock")
        .ok_or_else(|| fail("no padlock while unlocked"))?;
    h.tap(padlock.center());
    h.frames(2);
    assert_eq!(level(&h), Level(0));
    assert!(h.shell.status_bar().item_rect("status.lock").is_none());
    assert!(
        h.shell.workspace().find("admin").is_none(),
        "closed on the way down"
    );
    Ok(())
}

/// What a password authenticator saw.
#[derive(Default)]
struct Seen {
    user: Option<String>,
    secret: String,
}

/// A password authenticator: user `ada`, secret `hunter2`, maintainer.
struct Accounts(Rc<RefCell<Seen>>);

impl Authenticator for Accounts {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Password { needs_user: true }]
    }

    fn submit(&mut self, credential: Credential, _now: Instant) -> AuthOutcome {
        let Credential::Password { user, secret } = credential else {
            return AuthOutcome::Denied {
                message: "no".to_owned(),
            };
        };
        let ok = user.as_deref() == Some("ada") && secret == "hunter2";
        *self.0.borrow_mut() = Seen { user, secret };
        if ok {
            AuthOutcome::Granted(Subject {
                id: Some("ada".to_owned()),
                level: Level(2),
                ..Subject::default()
            })
        } else {
            AuthOutcome::Denied {
                message: "No such account".to_owned(),
            }
        }
    }
}

fn custom(authenticator: impl Authenticator + 'static) -> fairing::Result<Harness> {
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.motion.reduce = true;
    let mut h = Harness::from_builder(|ctx| {
        fairing::Shell::builder(config)
            .services(
                fairing::Services::builder()
                    .clock(fairing::services::null::NullClock)
                    .build(),
            )
            .authenticator(authenticator)
            .build(ctx)
    })?;
    h.shell
        .add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("calibration");
        }));
    h.frames(2);
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    Ok(h)
}

/// A password method: a user field that has the focus first, a secret field, and the keyboard —
/// which has to come up above the modal.
#[test]
fn a_password_authenticator_gets_both_fields_and_the_keyboard() -> fairing::Result<()> {
    let seen = Rc::new(RefCell::new(Seen::default()));
    let mut h = custom(Accounts(Rc::clone(&seen)))?;
    assert!(h.shell.unlock_prompt_visible());
    h.frames(2);
    h.type_text("ada");
    h.frames(2);
    #[cfg(feature = "osk")]
    assert!(
        h.shell.osk().is_shown(),
        "the user field wants the keyboard"
    );
    tap_text(&mut h, "Password")?;
    h.type_text("hunter2");
    h.frames(1);
    h.key(egui::Key::Enter);
    h.frames(2);
    assert_eq!(seen.borrow().user.as_deref(), Some("ada"));
    assert_eq!(seen.borrow().secret, "hunter2");
    assert!(!h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_some());
    assert_eq!(
        h.shell.access().session().subject.id.as_deref(),
        Some("ada")
    );
    Ok(())
}

/// A badge reader on its own thread: the prompt waits, and the answer arrives through `poll`.
struct Reader {
    badge: Rc<Cell<bool>>,
    cancelled: Rc<Cell<u32>>,
}

impl Authenticator for Reader {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::External {
            label: "Badge".to_owned(),
        }]
    }

    fn submit(&mut self, credential: Credential, _now: Instant) -> AuthOutcome {
        // A keyboard-wedge reader types the badge number and Enter.
        match credential {
            Credential::External(bytes) if bytes == b"04A1B2" => AuthOutcome::Granted(Subject {
                level: Level(2),
                ..Subject::default()
            }),
            _ => AuthOutcome::Denied {
                message: "Unknown badge".to_owned(),
            },
        }
    }

    fn poll(&mut self, _now: Instant) -> Option<AuthOutcome> {
        self.badge.take().then(|| {
            AuthOutcome::Granted(Subject {
                level: Level(2),
                ..Subject::default()
            })
        })
    }

    fn cancel(&mut self) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
}

#[test]
fn an_external_method_answers_through_poll() -> fairing::Result<()> {
    let badge = Rc::new(Cell::new(false));
    let cancelled = Rc::new(Cell::new(0));
    let mut h = custom(Reader {
        badge: Rc::clone(&badge),
        cancelled: Rc::clone(&cancelled),
    })?;
    assert!(text_rect(&mut h, "Badge").is_some());
    h.frames(5);
    assert!(h.shell.unlock_prompt_visible(), "waiting");
    badge.set(true);
    h.frames(2);
    assert!(h.shell.workspace().find("admin").is_some());
    assert_eq!(cancelled.get(), 0);
    Ok(())
}

#[test]
fn a_keyboard_wedge_reader_is_submitted_on_enter() -> fairing::Result<()> {
    let cancelled = Rc::new(Cell::new(0));
    let mut h = custom(Reader {
        badge: Rc::new(Cell::new(false)),
        cancelled: Rc::clone(&cancelled),
    })?;
    h.type_text("04A1B2");
    h.frames(1);
    h.key(egui::Key::Enter);
    h.frames(2);
    assert!(h.shell.workspace().find("admin").is_some());
    // And a cancelled wait tells the authenticator.
    let mut h = custom(Reader {
        badge: Rc::new(Cell::new(false)),
        cancelled: Rc::clone(&cancelled),
    })?;
    h.shell.back();
    h.frames(1);
    assert_eq!(cancelled.get(), 1);
    Ok(())
}

/// `prompt` mode with nothing to authenticate with — no PINs, no authenticator — behaves like
/// `routing`: the event, and no prompt with nothing behind it.
#[test]
fn prompt_mode_without_an_authenticator_only_reports() -> fairing::Result<()> {
    let mut h = shell(access_config_mode(
        &["viewer", "maintainer"],
        Some("top"),
        "prompt",
    ))?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(!h.shell.unlock_prompt_visible());
    assert!(matches!(
        access_events(&mut h).as_slice(),
        [AccessEvent::UnlockRequested { .. }]
    ));
    Ok(())
}

/// The lock screen is glass: a tap on what lies under it reaches nothing.
#[test]
fn nothing_under_the_lock_screen_takes_a_tap() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 1;
    config
        .access
        .gates
        .insert("open".to_owned(), "viewer".to_owned());
    let mut h = test_shell(config, |sh| {
        sh.add(
            screen("open", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("x");
            })
            .icon(fairing::icon::GAUGE)
            .desktop(),
        );
    })?;
    h.frames(2);
    let icon = h
        .shell
        .desktop()
        .icon_rect("open")
        .ok_or_else(|| fail("no icon"))?;
    h.run_for(1.2);
    assert!(h.shell.lock_screen_visible());
    h.tap(icon.center());
    h.frames(2);
    assert!(h.shell.workspace().find("open").is_none());
    Ok(())
}

/// An authenticator that manages its own entries — what puts `settings.credentials` on screen.
/// The second field is who it was told made each change ([`CredentialAdmin::set_actor`]).
///
/// [`CredentialAdmin::set_actor`]: fairing::access::CredentialAdmin::set_actor
#[cfg(feature = "settings")]
struct Roster(
    Rc<RefCell<Vec<(String, Level, String)>>>,
    Rc<RefCell<Vec<Subject>>>,
);

#[cfg(feature = "settings")]
impl Authenticator for Roster {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Pin {
            len: 0,
            max_len: 0,
            shuffle: false,
        }]
    }

    fn submit(&mut self, credential: Credential, _now: Instant) -> AuthOutcome {
        let Credential::Pin(pin) = credential else {
            return AuthOutcome::Pending;
        };
        match self.0.borrow().iter().find(|(_, _, p)| *p == pin) {
            Some((id, level, _)) => AuthOutcome::Granted(Subject {
                id: Some(id.clone()),
                level: *level,
                ..Subject::default()
            }),
            None => AuthOutcome::Denied {
                message: "Wrong PIN".to_owned(),
            },
        }
    }

    fn admin(&mut self) -> Option<&mut dyn fairing::access::CredentialAdmin> {
        Some(self)
    }
}

#[cfg(feature = "settings")]
impl fairing::access::CredentialAdmin for Roster {
    fn set_actor(&mut self, subject: &Subject) {
        self.1.borrow_mut().push(subject.clone());
    }

    fn list(&self) -> Vec<fairing::access::CredentialEntry> {
        self.0
            .borrow()
            .iter()
            .map(|(id, level, _)| fairing::access::CredentialEntry {
                id: id.clone(),
                label: id.clone(),
                level: *level,
                disabled: false,
            })
            .collect()
    }

    fn set_secret(&mut self, id: &str, credential: Credential) -> fairing::access::AdminResult {
        let Credential::Pin(pin) = credential else {
            return Err("PINs only".to_owned());
        };
        let mut roster = self.0.borrow_mut();
        let entry = roster
            .iter_mut()
            .find(|(i, _, _)| i == id)
            .ok_or("no such entry")?;
        entry.2 = pin;
        Ok(())
    }

    fn set_level(&mut self, id: &str, level: Level) -> fairing::access::AdminResult {
        if level == Level(0) {
            return Err("The bottom needs no entry.".to_owned());
        }
        let mut roster = self.0.borrow_mut();
        let entry = roster
            .iter_mut()
            .find(|(i, _, _)| i == id)
            .ok_or("no such entry")?;
        entry.1 = level;
        Ok(())
    }

    fn add(
        &mut self,
        id: &str,
        level: Level,
        credential: Credential,
    ) -> fairing::access::AdminResult {
        let Credential::Pin(pin) = credential else {
            return Err("PINs only".to_owned());
        };
        self.0.borrow_mut().push((id.to_owned(), level, pin));
        Ok(())
    }

    fn remove(&mut self, id: &str) -> fairing::access::AdminResult {
        let mut roster = self.0.borrow_mut();
        if roster.len() <= 1 {
            return Err("The last entry stays — it is the only way in.".to_owned());
        }
        roster.retain(|(i, _, _)| i != id);
        Ok(())
    }
}

/// An authenticator with `admin()` gets `settings.credentials`: the entries listed, a level
/// changed, an entry added and one removed — each one made by the authenticator, and its answer
/// shown.
#[cfg(feature = "settings")]
#[test]
fn an_authenticator_that_manages_its_entries_gets_the_credentials_screen() -> fairing::Result<()> {
    let roster = Rc::new(RefCell::new(vec![
        ("ada".to_owned(), Level(1), "1111".to_owned()),
        ("grace".to_owned(), Level(2), "2222".to_owned()),
    ]));
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.motion.reduce = true;
    config
        .access
        .gates
        .insert("settings.credentials".to_owned(), "viewer".to_owned());
    // At the top: nothing here is above the session's own level.
    config.access.initial = Some("maintainer".to_owned());
    let authenticator = Roster(Rc::clone(&roster), Rc::default());
    let mut h = Harness::from_builder(|ctx| {
        fairing::Shell::builder(config)
            .services(fairing::services::mock::services())
            .authenticator(authenticator)
            .build(ctx)
    })?
    // Tall enough that the whole form sits above the on-screen keyboard, which comes up with the
    // first field and would otherwise take the taps meant for the fields below it.
    .with_size(1024.0, 2600.0);
    fairing::settings::add_all(&mut h.shell, &fairing::settings::SettingsConfig::default());
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.credentials"));
    h.frames(4);
    assert!(h.shell.workspace().find("settings.credentials").is_some());
    assert!(text_rect(&mut h, "ada").is_some() && text_rect(&mut h, "grace").is_some());

    // A level changed.
    tap_text(&mut h, "ada")?;
    h.frames(2);
    tap_text(&mut h, "maintainer")?;
    tap_text(&mut h, "Apply")?;
    h.frames(3);
    assert_eq!(roster.borrow().first().map(|e| e.1), Some(Level(2)));
    assert!(text_rect(&mut h, "Level changed for ada").is_some());

    // An entry added.
    tap_text(&mut h, "Add an entry")?;
    h.frames(2);
    tap_text(&mut h, "operator-2")?;
    h.type_text("bob");
    // The hint says how long a PIN may be — the authenticator's `max_len`, 16 when it gives none.
    tap_text(&mut h, "Up to 16 digits")?;
    h.type_text("5555");
    tap_text(&mut h, "Again")?;
    h.type_text("5555");
    h.frames(1);
    tap_text(&mut h, "Apply")?;
    h.frames(3);
    assert!(roster
        .borrow()
        .iter()
        .any(|(id, level, pin)| id == "bob" && *level == Level(1) && pin == "5555"));

    // An entry removed — held, not tapped.
    tap_text(&mut h, "bob")?;
    h.frames(2);
    let remove = text_rect(&mut h, "Remove").ok_or_else(|| fail("no Remove"))?;
    h.hold(remove.center(), 70);
    h.release(remove.center());
    h.frames(3);
    assert!(!roster.borrow().iter().any(|(id, _, _)| id == "bob"));
    Ok(())
}

/// A check that answers `Pending` and then never again — the network dropped — until it is asked
/// a second time.
struct Stalled {
    submits: Rc<Cell<u32>>,
    cancelled: Rc<Cell<u32>>,
}

impl Authenticator for Stalled {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![AuthMethod::Pin {
            len: 4,
            max_len: 0,
            shuffle: false,
        }]
    }

    fn submit(&mut self, _credential: Credential, _now: Instant) -> AuthOutcome {
        self.submits.set(self.submits.get() + 1);
        if self.submits.get() == 1 {
            return AuthOutcome::Pending;
        }
        AuthOutcome::Granted(Subject {
            level: Level(2),
            ..Subject::default()
        })
    }

    fn cancel(&mut self) {
        self.cancelled.set(self.cancelled.get() + 1);
    }
}

/// The lock screen has no Cancel and the shell sets no timer: what ends a wait whose answer never
/// came is the next attempt. The keys work while "Checking…" is up, and the new PIN calls the old
/// check off before it goes in.
#[test]
fn a_new_attempt_calls_off_an_answer_that_never_came() -> fairing::Result<()> {
    let submits = Rc::new(Cell::new(0));
    let cancelled = Rc::new(Cell::new(0));
    let mut h = custom(Stalled {
        submits: Rc::clone(&submits),
        cancelled: Rc::clone(&cancelled),
    })?;
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    // The unlock prompt the lock came up over was called off.
    let before = cancelled.get();
    type_pin(&mut h, "1111");
    assert_eq!(submits.get(), 1);
    assert!(
        text_rect(&mut h, "Checking…").is_some(),
        "waiting for the answer"
    );
    // No timer of the shell's: an hour on, it is still waiting and nothing was called off.
    h.sleep(3600.0);
    h.frames(2);
    assert!(text_rect(&mut h, "Checking…").is_some(), "still waiting");
    assert_eq!(cancelled.get(), before);
    // The keys were never held: the next PIN calls the old check off and goes in.
    type_pin(&mut h, "2222");
    assert_eq!(cancelled.get(), before + 1, "the old check was called off");
    assert_eq!(submits.get(), 2);
    assert!(!h.shell.lock_screen_visible());
    Ok(())
}

/// The integrator vouching for someone above the starting level — a reader it handles itself, a
/// remote unlock — takes the lock screen away; it had done its work.
#[test]
fn an_integrator_unlock_takes_the_lock_screen_away() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 1;
    let mut h = shell(config)?;
    h.run_for(1.2);
    assert!(h.shell.lock_screen_visible());
    h.shell.handle().set_subject(Subject {
        level: Level(1),
        ..Subject::default()
    });
    h.frames(3);
    assert!(!h.shell.lock_screen_visible());
    assert_eq!(level(&h), Level(1));
    Ok(())
}

/// A timer longer than a year is a config error at start, not an overflow on the first frame.
#[test]
fn an_absurd_timer_is_a_config_error() {
    let mut config = pins();
    config.access.idle_lock_secs = u64::MAX / 2;
    assert!(shell(config).is_err());
}

// ── The second review: rules that hold whatever the authenticator does ─────────────

/// What a [`Scripted`] authenticator was asked.
#[derive(Default)]
struct Asked {
    /// The gates `begin` was told about, in order.
    begun: Vec<String>,
    /// What came to `submit`: a PIN's digits, a password's secret, a badge's bytes.
    submitted: Vec<String>,
    /// Every call, in order: `begin <gate>`, `submit <what>`, `cancel`.
    log: Vec<String>,
}

/// One answer a [`Scripted`] authenticator gives.
#[derive(Clone, Copy)]
enum Answer {
    Grant(u16),
    Lock,
    Wait,
}

/// An authenticator that offers `methods` and answers each submit with the next of `answers` —
/// and refuses once they run out.
struct Scripted {
    methods: Vec<AuthMethod>,
    answers: std::collections::VecDeque<Answer>,
    asked: Rc<RefCell<Asked>>,
}

impl Authenticator for Scripted {
    fn methods(&self) -> Vec<AuthMethod> {
        self.methods.clone()
    }

    fn begin(&mut self, gate: &fairing::access::Gate, _now: Instant) {
        let mut asked = self.asked.borrow_mut();
        asked.begun.push(gate.as_str().to_owned());
        asked.log.push(format!("begin {}", gate.as_str()));
    }

    fn cancel(&mut self) {
        self.asked.borrow_mut().log.push("cancel".to_owned());
    }

    fn submit(&mut self, credential: Credential, now: Instant) -> AuthOutcome {
        let what = match credential {
            Credential::Pin(pin) => pin,
            Credential::Password { secret, .. } => secret,
            Credential::External(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Credential::Pattern(dots) => format!("{dots:?}"),
            _ => String::new(),
        };
        let mut asked = self.asked.borrow_mut();
        asked.log.push(format!("submit {what}"));
        asked.submitted.push(what);
        drop(asked);
        match self.answers.pop_front() {
            Some(Answer::Grant(level)) => AuthOutcome::Granted(Subject {
                level: Level(level),
                ..Subject::default()
            }),
            Some(Answer::Lock) => AuthOutcome::Locked {
                until: now + Duration::from_secs(60),
                message: "Locked out".to_owned(),
            },
            Some(Answer::Wait) => AuthOutcome::Pending,
            None => AuthOutcome::Denied {
                message: "No".to_owned(),
            },
        }
    }
}

/// [`custom`] with a [`Scripted`] authenticator: the prompt is up for `admin`.
fn scripted(
    methods: Vec<AuthMethod>,
    answers: &[Answer],
) -> fairing::Result<(Harness, Rc<RefCell<Asked>>)> {
    let asked = Rc::new(RefCell::new(Asked::default()));
    let h = custom(Scripted {
        methods,
        answers: answers.iter().copied().collect(),
        asked: Rc::clone(&asked),
    })?;
    Ok((h, asked))
}

fn pin4() -> AuthMethod {
    AuthMethod::Pin {
        len: 4,
        max_len: 0,
        shuffle: false,
    }
}

fn badge() -> AuthMethod {
    AuthMethod::External {
        label: "Badge".to_owned(),
    }
}

/// The first drawn text that starts with `prefix`.
fn text_starting(h: &mut Harness, prefix: &str) -> Option<String> {
    fn walk(shape: &egui::Shape, prefix: &str, found: &mut Option<String>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text().starts_with(prefix) => {
                found.get_or_insert_with(|| text.galley.text().to_owned());
            }
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    walk(s, prefix, found);
                }
            }
            _ => {}
        }
    }
    let mut found = None;
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, prefix, &mut found);
    }
    found
}

/// One wrong PIN locks the table for 30 s; the prompt is up for `admin`, on the countdown.
fn locked_out() -> fairing::Result<Harness> {
    let mut config = pins();
    config.access.pin_table.attempt_limit = Some(1);
    config.access.pin_table.lock_secs = Some(30);
    let mut h = shell(config)?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    type_pin(&mut h, "0000");
    if text_starting(&mut h, "Too many attempts").is_none() {
        return Err(fail("the table did not lock"));
    }
    Ok(h)
}

/// A lockout outlasts the prompt: closed and asked again, it opens on the countdown — not on a
/// keypad that looks live and only refuses.
#[test]
fn a_lockout_is_still_on_when_the_prompt_comes_back() -> fairing::Result<()> {
    let mut h = locked_out()?;
    h.shell.back();
    h.frames(2);
    assert!(!h.shell.unlock_prompt_visible());
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(h.shell.unlock_prompt_visible());
    let line = text_starting(&mut h, "Too many attempts");
    assert!(
        line.is_some_and(|l| l.ends_with(" s")),
        "the countdown shows before anything is typed"
    );
    Ok(())
}

/// A second request over a locked-out prompt — a heads-up tapped, another locked icon — keeps
/// the countdown on show; the keys under it are still dead.
#[test]
fn a_second_request_keeps_the_countdown_on_show() -> fairing::Result<()> {
    let mut h = locked_out()?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(text_starting(&mut h, "Too many attempts").is_some());
    assert!(text_rect(&mut h, "Enter PIN").is_none());
    Ok(())
}

/// A lockout holds for a password as it does for the keypad: neither Enter nor Unlock submits
/// under it.
#[test]
fn a_password_waits_out_a_lockout() -> fairing::Result<()> {
    let (mut h, asked) = scripted(
        vec![AuthMethod::Password { needs_user: false }],
        &[Answer::Lock],
    )?;
    h.frames(2);
    h.type_text("first");
    h.frames(1);
    h.key(egui::Key::Enter);
    h.frames(2);
    assert_eq!(asked.borrow().submitted.len(), 1);
    assert!(text_starting(&mut h, "Locked out").is_some());
    tap_text(&mut h, "Password")?;
    h.type_text("second");
    h.frames(1);
    h.key(egui::Key::Enter);
    h.frames(2);
    tap_text(&mut h, "Unlock")?;
    h.frames(2);
    assert_eq!(
        asked.borrow().submitted.len(),
        1,
        "nothing goes under a lockout"
    );
    Ok(())
}

/// Two badge reads, the second after the first was answered with `first`.
fn two_reads(first: Answer) -> fairing::Result<Rc<RefCell<Asked>>> {
    let (mut h, asked) = scripted(vec![badge()], &[first])?;
    h.type_text("AAAA");
    h.key(egui::Key::Enter);
    h.frames(2);
    h.type_text("BBBB");
    h.key(egui::Key::Enter);
    h.frames(2);
    Ok(asked)
}

/// A badge read waits out a lockout, as every way in does. Over an answer still awaited it goes
/// in, and the check it replaces is called off first.
#[test]
fn a_badge_read_waits_out_a_lockout_and_calls_off_a_wait() -> fairing::Result<()> {
    let locked = two_reads(Answer::Lock)?;
    assert_eq!(locked.borrow().submitted, vec!["AAAA".to_owned()]);
    let waiting = two_reads(Answer::Wait)?;
    assert_eq!(
        waiting.borrow().log,
        vec!["begin admin", "submit AAAA", "cancel", "submit BBBB"]
    );
    Ok(())
}

/// A reader that ends a read with Tab is heard, and a pause longer than a read takes starts a
/// new one — half a read that lost its end does not spoil the next badge.
#[test]
fn a_badge_read_ends_on_tab_and_a_pause_starts_over() -> fairing::Result<()> {
    let (mut h, asked) = scripted(vec![badge()], &[])?;
    h.type_text("04A1");
    h.frames(1);
    h.run_for(1.0);
    h.type_text("B2");
    h.key(egui::Key::Tab);
    h.frames(2);
    assert_eq!(asked.borrow().submitted, vec!["B2".to_owned()]);
    Ok(())
}

/// Half a read belongs to no tab: switching away and back starts the next one clean.
#[test]
fn half_a_badge_read_does_not_survive_a_tab_switch() -> fairing::Result<()> {
    let (mut h, asked) = scripted(vec![badge(), pin4()], &[])?;
    h.type_text("04A1");
    h.frames(1);
    tap_text(&mut h, "PIN")?;
    tap_text(&mut h, "Badge")?;
    h.type_text("B2");
    h.key(egui::Key::Enter);
    h.frames(2);
    assert_eq!(asked.borrow().submitted, vec!["B2".to_owned()]);
    Ok(())
}

/// A badge wait can last all night on a lock screen: its ring asks for a frame a few times a
/// second, not sixty.
#[test]
fn the_badge_ring_does_not_ask_for_every_frame() -> fairing::Result<()> {
    let (mut h, _) = scripted(vec![badge()], &[])?;
    h.frames(5);
    assert!(h.shell.unlock_prompt_visible());
    assert!(
        !h.repaint_requested,
        "no immediate repaint while a badge is awaited"
    );
    Ok(())
}

/// `begin` is told each time a prompt opens — the lock screen's gate is `session.lock` — so a
/// reader can drop what it picked up while nothing asked.
#[test]
fn begin_is_told_when_a_prompt_opens() -> fairing::Result<()> {
    let (mut h, asked) = scripted(vec![pin4()], &[])?;
    assert_eq!(asked.borrow().begun, vec!["admin".to_owned()]);
    h.shell.back();
    h.frames(2);
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    assert_eq!(
        asked.borrow().begun,
        vec!["admin".to_owned(), "session.lock".to_owned()]
    );
    Ok(())
}

/// An authenticator that offers nothing behaves as `routing`: the lock and the logout
/// are reported, and the session is left as it is — there would be no way back in.
#[test]
fn an_authenticator_offering_nothing_leaves_lock_and_logout_to_the_integrator(
) -> fairing::Result<()> {
    let (mut h, _) = scripted(Vec::new(), &[])?;
    assert!(!h.shell.unlock_prompt_visible());
    h.shell.handle().set_subject(Subject {
        level: Level(2),
        ..Subject::default()
    });
    h.frames(2);
    let _ = h.shell.poll_events();
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(!h.shell.lock_screen_visible());
    assert!(h.shell.poll_events().contains(&ShellEvent::LockRequested));
    h.shell.launch(LaunchAction::Logout);
    h.frames(2);
    assert!(h.shell.poll_events().contains(&ShellEvent::LogoutRequested));
    assert_eq!(level(&h), Level(2), "only reported");
    Ok(())
}

/// A grant has to be worth something: one no higher than the session already holds, or a level
/// the table does not have, is answered as a refusal and changes nothing.
#[test]
fn a_grant_that_is_no_step_up_is_refused() -> fairing::Result<()> {
    let (mut h, _) = scripted(
        vec![pin4()],
        &[Answer::Grant(0), Answer::Grant(7), Answer::Grant(2)],
    )?;
    type_pin(&mut h, "1111");
    assert_eq!(level(&h), Level(0));
    assert!(text_rect(&mut h, "That is not enough for this").is_some());
    type_pin(&mut h, "2222");
    assert_eq!(level(&h), Level(0), "a level the table does not have");
    assert!(h.shell.workspace().find("admin").is_none());
    type_pin(&mut h, "3333");
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// What was asked for behind the lock screen is asked again once it goes: it runs where the
/// session now passes, and the prompt comes up where it does not.
#[test]
fn a_request_behind_the_lock_screen_is_asked_again_after_it() -> fairing::Result<()> {
    for (pin, opens) in [("9876", true), ("1234", false)] {
        let mut config = pins();
        config.access.idle_lock_secs = 1;
        let mut h = shell(config)?;
        h.run_for(1.2);
        assert!(h.shell.lock_screen_visible());
        h.shell.launch(LaunchAction::open("admin"));
        h.frames(2);
        assert!(h.shell.workspace().find("admin").is_none());
        type_pin(&mut h, pin);
        h.frames(2);
        assert!(!h.shell.lock_screen_visible());
        assert_eq!(h.shell.workspace().find("admin").is_some(), opens, "{pin}");
        assert_eq!(h.shell.unlock_prompt_visible(), !opens, "{pin}");
    }
    Ok(())
}

/// A new authenticator while a prompt is up: the prompt opens again for it — its methods, and
/// nothing the old one owed or locked. One that offers nothing takes the lock screen away, as in
/// `routing`.
#[test]
fn a_new_authenticator_gets_a_fresh_prompt() -> fairing::Result<()> {
    let mut h = locked_out()?;
    let asked = Rc::new(RefCell::new(Asked::default()));
    h.shell.set_authenticator(Scripted {
        methods: vec![badge()],
        answers: [Answer::Grant(2)].into_iter().collect(),
        asked: Rc::clone(&asked),
    });
    h.frames(2);
    assert!(h.shell.unlock_prompt_visible());
    assert!(
        text_starting(&mut h, "Too many attempts").is_none(),
        "the old one's lockout"
    );
    assert_eq!(asked.borrow().begun, vec!["admin".to_owned()]);
    h.type_text("04A1B2");
    h.key(egui::Key::Enter);
    h.frames(2);
    assert!(
        h.shell.workspace().find("admin").is_some(),
        "the request it was up for goes on"
    );

    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    h.shell.set_authenticator(Scripted {
        methods: Vec::new(),
        answers: std::collections::VecDeque::new(),
        asked: Rc::default(),
    });
    h.frames(2);
    assert!(!h.shell.lock_screen_visible());
    Ok(())
}

/// The integrator's own way past the lock screen — a reader it handles, a remote unlock — asks
/// what waited behind it too, then and there: left waiting, it would run at some later lock.
#[test]
fn an_integrator_unlock_asks_what_waited_behind_the_lock_screen() -> fairing::Result<()> {
    let mut config = pins();
    config.access.idle_lock_secs = 1;
    let mut h = shell(config)?;
    h.run_for(1.2);
    assert!(h.shell.lock_screen_visible());
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    h.shell.handle().set_subject(Subject {
        level: Level(2),
        ..Subject::default()
    });
    h.frames(2);
    assert!(!h.shell.lock_screen_visible());
    assert!(h.shell.workspace().find("admin").is_some());
    Ok(())
}

/// `settings.credentials` over a [`Roster`] of `ada` (operator) and `grace` (maintainer), with
/// the session starting at `initial`: the screen open, and who each change was made by.
#[cfg(feature = "settings")]
#[allow(clippy::type_complexity)]
fn credentials(
    initial: &str,
    size: egui::Vec2,
) -> fairing::Result<(
    Harness,
    Rc<RefCell<Vec<(String, Level, String)>>>,
    Rc<RefCell<Vec<Subject>>>,
)> {
    let roster = Rc::new(RefCell::new(vec![
        ("ada".to_owned(), Level(1), "1111".to_owned()),
        ("grace".to_owned(), Level(2), "2222".to_owned()),
    ]));
    let actors = Rc::new(RefCell::new(Vec::new()));
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.motion.reduce = true;
    config
        .access
        .gates
        .insert("settings.credentials".to_owned(), "viewer".to_owned());
    config
        .access
        .gates
        .insert("settings.home".to_owned(), "viewer".to_owned());
    config.access.initial = Some(initial.to_owned());
    let authenticator = Roster(Rc::clone(&roster), Rc::clone(&actors));
    let mut h = Harness::from_builder(|ctx| {
        fairing::Shell::builder(config)
            .services(fairing::services::mock::services())
            .authenticator(authenticator)
            .build(ctx)
    })?
    .with_size(size.x, size.y);
    fairing::settings::add_all(&mut h.shell, &fairing::settings::SettingsConfig::default());
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.credentials"));
    h.frames(4);
    Ok((h, roster, actors))
}

#[cfg(feature = "settings")]
const TALL: egui::Vec2 = egui::vec2(1024.0, 2600.0);

/// Back once for the on-screen keyboard a field brought up: it goes first, as for any field.
#[cfg(feature = "settings")]
fn hide_keyboard(h: &mut Harness) {
    #[cfg(feature = "osk")]
    if h.shell.osk().is_shown() {
        h.shell.back();
        h.frames(2);
    }
    #[cfg(not(feature = "osk"))]
    let _ = h;
}

/// The form is the screen's own and goes with it: Back closes it before it leaves the screen,
/// and leaving the screen with it open drops it and what was typed — the next visit, the next
/// person's maybe, starts from the list.
#[cfg(feature = "settings")]
#[test]
fn the_credentials_form_goes_with_the_screen() -> fairing::Result<()> {
    let (mut h, _, _) = credentials("maintainer", TALL)?;
    tap_text(&mut h, "Add an entry")?;
    h.frames(2);
    tap_text(&mut h, "operator-2")?;
    h.type_text("bob");
    h.frames(2);
    hide_keyboard(&mut h);
    h.shell.back();
    h.frames(2);
    assert!(h.shell.workspace().find("settings.credentials").is_some());
    assert!(
        text_rect(&mut h, "Add an entry").is_some(),
        "the list again"
    );

    tap_text(&mut h, "Add an entry")?;
    h.frames(2);
    tap_text(&mut h, "operator-2")?;
    h.type_text("eve");
    h.frames(2);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("settings.credentials"));
    h.frames(4);
    assert!(text_rect(&mut h, "Add an entry").is_some(), "not the form");
    assert!(text_rect(&mut h, "eve").is_none());

    // Someone else's session: what the last one typed is not theirs.
    tap_text(&mut h, "Add an entry")?;
    h.frames(2);
    tap_text(&mut h, "operator-2")?;
    h.type_text("mallory");
    h.frames(2);
    h.shell.handle().set_subject(Subject {
        id: Some("someone".to_owned()),
        level: Level(2),
        ..Subject::default()
    });
    h.frames(3);
    assert!(text_rect(&mut h, "mallory").is_none());
    Ok(())
}

/// On the settings home's right the screen is drawn, never opened: another entry picked and
/// this one picked again starts from the list too.
#[cfg(feature = "settings")]
#[test]
fn the_credentials_form_goes_when_another_setting_is_picked() -> fairing::Result<()> {
    let (mut h, _, _) = credentials("maintainer", egui::vec2(2200.0, 1300.0))?;
    h.shell.back();
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.home"));
    h.frames(4);
    tap_text(&mut h, "Users & access")?;
    h.frames(2);
    tap_text(&mut h, "Add an entry")?;
    h.frames(2);
    tap_text(&mut h, "operator-2")?;
    h.type_text("eve");
    h.frames(2);
    assert!(text_rect(&mut h, "eve").is_some());
    tap_text(&mut h, "Display")?;
    h.frames(2);
    hide_keyboard(&mut h);
    tap_text(&mut h, "Users & access")?;
    h.frames(2);
    assert!(text_rect(&mut h, "eve").is_none());
    assert!(text_rect(&mut h, "Add an entry").is_some());
    Ok(())
}

/// One Apply can be two calls; each is answered on its own. A refused level beside a saved
/// secret keeps the form up with both lines and what was typed — the old form closed on Apply
/// and showed only the last answer, a green one.
#[cfg(feature = "settings")]
#[test]
fn a_refused_change_keeps_the_form_and_says_what_went_through() -> fairing::Result<()> {
    let (mut h, roster, _) = credentials("maintainer", TALL)?;
    tap_text(&mut h, "ada")?;
    h.frames(2);
    tap_text(&mut h, "viewer")?;
    tap_text(&mut h, "Up to 16 digits")?;
    h.type_text("4321");
    tap_text(&mut h, "Again")?;
    h.type_text("4321");
    h.frames(1);
    tap_text(&mut h, "Apply")?;
    h.frames(3);
    assert_eq!(
        roster.borrow().first().map(|e| (e.1, e.2.clone())),
        Some((Level(1), "4321".to_owned()))
    );
    assert!(text_rect(&mut h, "Level for ada not changed").is_some());
    assert!(text_rect(&mut h, "The bottom needs no entry.").is_some());
    assert!(text_rect(&mut h, "New secret saved for ada").is_some());
    assert!(text_rect(&mut h, "Apply").is_some(), "the form is still up");
    tap_text(&mut h, "Cancel")?;
    h.frames(2);
    assert!(
        text_rect(&mut h, "Level for ada not changed").is_some(),
        "and the list says so too"
    );
    Ok(())
}

/// Nobody changes more than they have: an operator sees the maintainer's entry but
/// cannot open it, is offered no level above their own, and every change says who made it.
#[cfg(feature = "settings")]
#[test]
fn nobody_changes_more_than_they_have() -> fairing::Result<()> {
    let (mut h, roster, actors) = credentials("operator", TALL)?;
    tap_text(&mut h, "grace")?;
    h.frames(2);
    assert!(
        text_rect(&mut h, "Apply").is_none(),
        "no form for an entry above the session"
    );
    tap_text(&mut h, "Add an entry")?;
    h.frames(2);
    assert!(text_rect(&mut h, "operator").is_some());
    assert!(text_rect(&mut h, "maintainer").is_none(), "not offered");
    tap_text(&mut h, "Cancel")?;
    h.frames(2);

    tap_text(&mut h, "ada")?;
    h.frames(2);
    tap_text(&mut h, "Up to 16 digits")?;
    h.type_text("4321");
    tap_text(&mut h, "Again")?;
    h.type_text("4321");
    h.frames(1);
    tap_text(&mut h, "Apply")?;
    h.frames(3);
    assert_eq!(
        roster.borrow().first().map(|e| e.2.clone()),
        Some("4321".to_owned())
    );
    assert_eq!(
        actors.borrow().last().map(|s| s.level),
        Some(Level(1)),
        "the authenticator was told who"
    );
    Ok(())
}

// ── One flag for a wait ───────────────────────────────────────────────────────────

/// Every way in works while an answer is awaited — a password and a pattern as well as the
/// keypad and a badge — and the next attempt calls the old check off before it goes in.
#[test]
fn every_way_in_works_while_an_answer_is_awaited() -> fairing::Result<()> {
    let (mut h, asked) = scripted(
        vec![AuthMethod::Password { needs_user: false }],
        &[Answer::Wait],
    )?;
    h.frames(2);
    h.type_text("first");
    h.frames(1);
    h.key(egui::Key::Enter);
    h.frames(2);
    assert!(text_rect(&mut h, "Checking…").is_some());
    tap_text(&mut h, "Password")?;
    h.type_text("second");
    h.frames(1);
    h.key(egui::Key::Enter);
    h.frames(2);
    assert_eq!(
        asked.borrow().log,
        vec!["begin admin", "submit first", "cancel", "submit second"]
    );

    let (mut h, asked) = scripted(
        vec![AuthMethod::Pattern {
            grid: 3,
            min_points: 4,
            show_path: true,
        }],
        &[Answer::Wait],
    )?;
    // Counted from 1 here, as `[access.pattern_table]` does; the credential counts from 0.
    draw_pattern(&mut h, &[1, 2, 3, 6])?;
    assert!(text_rect(&mut h, "Checking…").is_some());
    draw_pattern(&mut h, &[1, 4, 7, 8])?;
    assert_eq!(
        asked.borrow().log,
        vec![
            "begin admin",
            "submit [0, 1, 2, 5]",
            "cancel",
            "submit [0, 3, 6, 7]"
        ]
    );
    Ok(())
}

/// The lock coming up over a check still under way calls it off first — before the lock
/// screen's `begin` — and the lock screen starts with nothing awaited.
#[test]
fn the_lock_calls_off_a_check_before_it_comes_up() -> fairing::Result<()> {
    let (mut h, asked) = scripted(vec![pin4()], &[Answer::Wait])?;
    type_pin(&mut h, "1111");
    assert!(text_rect(&mut h, "Checking…").is_some());
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    assert_eq!(
        asked.borrow().log,
        vec!["begin admin", "submit 1111", "cancel", "begin session.lock"]
    );
    assert!(text_rect(&mut h, "Checking…").is_none(), "nothing awaited");
    Ok(())
}

/// Every way a prompt goes without an answer tells the authenticator: replaced by another, and
/// the lock screen taken away by the integrator's own unlock.
#[test]
fn every_close_without_an_answer_calls_the_check_off() -> fairing::Result<()> {
    let (mut h, asked) = scripted(vec![pin4()], &[Answer::Wait])?;
    type_pin(&mut h, "1111");
    h.shell.set_authenticator(Scripted {
        methods: vec![pin4()],
        answers: std::collections::VecDeque::new(),
        asked: Rc::default(),
    });
    h.frames(2);
    assert_eq!(
        asked.borrow().log.last().map(String::as_str),
        Some("cancel"),
        "the one going out"
    );

    let (mut h, asked) = scripted(vec![pin4()], &[Answer::Wait])?;
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    type_pin(&mut h, "1111");
    h.shell.handle().set_subject(Subject {
        level: Level(2),
        ..Subject::default()
    });
    h.frames(2);
    assert!(!h.shell.lock_screen_visible());
    assert_eq!(
        asked.borrow().log,
        vec![
            "begin admin",
            "cancel",
            "begin session.lock",
            "submit 1111",
            "cancel"
        ]
    );
    Ok(())
}

/// An authenticator that answers through `poll` whenever the test says — awaited or not.
struct Late {
    submits: u32,
    /// Set, and the next `poll` grants.
    answer: Rc<Cell<bool>>,
}

impl Authenticator for Late {
    fn methods(&self) -> Vec<AuthMethod> {
        vec![pin4()]
    }

    fn submit(&mut self, _credential: Credential, _now: Instant) -> AuthOutcome {
        self.submits += 1;
        if self.submits == 1 {
            AuthOutcome::Pending
        } else {
            AuthOutcome::Denied {
                message: "No".to_owned(),
            }
        }
    }

    fn poll(&mut self, _now: Instant) -> Option<AuthOutcome> {
        self.answer.take().then(|| {
            AuthOutcome::Granted(Subject {
                level: Level(2),
                ..Subject::default()
            })
        })
    }
}

/// The shell follows what the authenticator reports: an answer from `poll` is the
/// state to move to, whether one was awaited or not. Which request it answers is the
/// authenticator's business, not the shell's.
#[test]
fn the_shell_follows_what_poll_reports() -> fairing::Result<()> {
    let answer = Rc::new(Cell::new(false));
    let mut h = custom(Late {
        submits: 0,
        answer: Rc::clone(&answer),
    })?;
    type_pin(&mut h, "1111");
    answer.set(true);
    h.frames(2);
    assert!(
        h.shell.workspace().find("admin").is_some(),
        "awaited, and applied"
    );

    let answer = Rc::new(Cell::new(false));
    let mut h = custom(Late {
        submits: 0,
        answer: Rc::clone(&answer),
    })?;
    type_pin(&mut h, "1111");
    // The second PIN is refused at once, so nothing is awaited any more.
    type_pin(&mut h, "2222");
    answer.set(true);
    h.frames(2);
    assert!(
        h.shell.workspace().find("admin").is_some(),
        "applied all the same"
    );
    Ok(())
}
