//! The M1 headless integration-test skeleton (the M1 completion criteria).
//!
//! To the skeleton's minimum cases (★) it adds the launch-mode, lifecycle, threading, animation and
//! fullscreen cases and the regressions for issues raised during integration. The tests give back a `Result` — the workspace lints
//! refuse `panic!` and `unwrap` on test targets too.

use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{icon, screen, screen_with, Cx, LaunchAction, LaunchMode, ShellEvent};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The shell **really does emit** `Lifecycle::Resized`. It was in the enum with nothing emitting it,
/// so `on_lifecycle` kept an arm ready for a value that never came.
///
/// Rotation and a window resize take this path. The other way round, **it must not come on a frame where the size
/// did not change** — coming every frame makes it noise rather than an event.
#[test]
fn the_shell_tells_screens_when_the_pane_resizes() -> fairing::Result<()> {
    let sizes = Rc::new(RefCell::new(Vec::<egui::Vec2>::new()));
    let seen = Rc::clone(&sizes);
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("a", move |_ui: &mut egui::Ui, cx: &mut Cx| {
            if let Some(fairing::Lifecycle::Resized(size)) = cx.event {
                seen.borrow_mut().push(size);
            }
        }));
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(3);
    assert!(
        sizes.borrow().is_empty(),
        "opening alone does not bring one"
    );

    h.set_size(800.0, 480.0);
    h.frames(2);
    let got = sizes.borrow().clone();
    assert_eq!(
        got.len(),
        1,
        "once on the frame where the size changed: {got:?}"
    );
    let content = h.shell.layout().content.size();
    assert_eq!(
        got.first().copied(),
        Some(content),
        "it gives the new content size"
    );

    // With the size as it was, no more come.
    h.frames(4);
    assert_eq!(
        sizes.borrow().len(),
        1,
        "none on a frame that did not change"
    );
    Ok(())
}

/// A screen fills **the (inset) Rect the shell gave it**. The default inset is `Metrics::screen_inset` (12)
/// and `ChromePolicy::fullscreen()` is 0.
#[test]
fn closure_screen_fills_the_inset_pane_rect() -> fairing::Result<()> {
    let seen = Rc::new(Cell::new(egui::Rect::NOTHING));
    let pane = Rc::new(Cell::new(egui::Rect::NOTHING));
    let (s, q) = (Rc::clone(&seen), Rc::clone(&pane));
    let full = Rc::new(Cell::new(egui::Rect::NOTHING));
    let f = Rc::clone(&full);
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(screen("a", move |ui: &mut egui::Ui, cx: &mut Cx| {
            s.set(ui.max_rect());
            q.set(cx.pane.rect);
        }));
        sh.add(
            screen("b", move |ui: &mut egui::Ui, _: &mut Cx| {
                f.set(ui.max_rect());
            })
            .fullscreen(),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    let content = h.shell.layout().content;
    // **Read from the live theme, not from `Metrics::default()`.** `screen_inset` resolves from
    // `MetricsSpec` at the frame's scale since the control vocabulary's adoption step 4, so 12 is
    // its `du` floor rather than its value — at the default gloved finger it is 20.47. What this
    // test is about is that the screen gets the content rect shrunk by *the* inset, whatever the
    // panel resolved it to, so it has to ask.
    let inset = h.shell.theme().metrics.screen_inset;
    assert!(
        inset >= 12.0,
        "the inset resolved below its own du floor: {inset}"
    );
    assert_eq!(seen.get(), content.shrink(inset), "ui gets the inset Rect");
    assert_eq!(pane.get(), seen.get(), "cx.pane.rect is the same Rect");
    // The layout itself does not shrink — the inset applies only to the Rect given to the screen.
    assert_eq!(h.shell.layout().content, content);

    h.shell.handle().launch(LaunchAction::open("b"));
    h.frames(3);
    assert_eq!(
        full.get(),
        h.shell.layout().content,
        "fullscreen has an inset of 0 — the content Rect as it is"
    );
    Ok(())
}

#[test]
fn remove_closes_open_screen_and_its_icons() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("x");
            })
            .icon(icon::GAUGE)
            .desktop()
            .dock(),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    assert!(h.shell.workspace().find("a").is_some());
    h.shell.remove("a");
    h.frames(1);
    assert!(h.shell.workspace().find("a").is_none());
    assert!(h.shell.desktop().icons().is_empty() && h.shell.desktop().dock().is_empty());
    Ok(())
}

#[test]
fn locked_screen_does_not_open_and_requests_unlock() -> fairing::Result<()> {
    // `prompt` mode with nothing to ask (no PIN table, no authenticator) behaves as `routing`: it
    // does not open and only UnlockRequested comes. The prompt itself is `m3_access.rs`.
    let mut h = test_shell(
        access_config(&["viewer", "maintainer"], Some("top")),
        |sh| {
            sh.add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("x");
            }));
        },
    )?;
    h.shell.handle().launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(!h.shell.unlock_prompt_visible());
    assert!(h.shell.workspace().find("admin").is_none());
    let events = h.shell.poll_events();
    assert!(events.iter().any(|e| matches!(
        e,
        ShellEvent::Access(fairing::access::AccessEvent::UnlockRequested { .. })
    )));
    Ok(())
}

#[test]
fn back_and_home_round_trip_goes_idle() -> fairing::Result<()> {
    // Go there and back with the animations on (reduce = false), and look at whether it is idle at the end (no immediate repaint).
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    h.shell.add(
        screen("a", |ui: &mut egui::Ui, cx: &mut Cx| {
            if ui.button("b").clicked() {
                cx.open("b");
            }
        })
        .icon(icon::GAUGE)
        .desktop(),
    );
    h.shell.add(screen("b", |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label("b");
    }));
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("a"));
    h.run_for(0.5);
    assert!(!h.shell.workspace().is_home());
    h.shell.handle().launch(LaunchAction::open("b"));
    h.run_for(0.5);
    assert!(h.shell.workspace().find("b").is_some());
    h.shell.handle().back();
    h.run_for(0.5);
    assert!(h.shell.workspace().find("b").is_none());
    h.shell.handle().home();
    h.run_for(0.5);
    assert!(h.shell.workspace().is_home());
    assert!(h.shell.workspace().find("a").is_some(), "the task is alive");
    assert!(!h.shell.is_animating());
    assert!(
        !h.repaint_requested,
        "idle, it does not ask for an immediate repaint"
    );
    Ok(())
}

#[test]
fn desktop_icon_tap_opens_screen() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.frames(2);
    let Some(rect) = h.shell.desktop().icon_rect("a") else {
        return Err(fairing::Error::Config("there is no icon Rect".to_owned()));
    };
    h.tap(rect.center());
    assert!(h.shell.workspace().find("a").is_some());
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "a")));
    Ok(())
}

#[test]
fn nav_back_pops_and_root_pop_goes_home() -> fairing::Result<()> {
    use fairing::chrome::NavItem;
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
        sh.add(screen("b", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("b");
        }));
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("b"));
    h.frames(2);
    let Some(back) = h.shell.nav_bar().item_rect(&NavItem::Back) else {
        return Err(fairing::Error::Config(
            "there is no back button Rect".to_owned(),
        ));
    };
    h.tap(back.center());
    assert!(h.shell.workspace().find("b").is_none(), "b was popped");
    assert!(!h.shell.workspace().is_home());
    h.tap(back.center());
    assert!(h.shell.workspace().is_home(), "a root pop is home");
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::WentHome)));
    Ok(())
}

#[test]
fn single_reuses_screen_in_another_task() -> fairing::Result<()> {
    use fairing::workspace::Instance;
    // Single, alive in another task: that task is brought forward (no new instance).
    let mut h = test_shell(single_level_access(), |sh| {
        for id in ["a", "b"] {
            sh.add(
                screen(id, |ui: &mut egui::Ui, _: &mut Cx| {
                    ui.label("x");
                })
                .icon(icon::GAUGE)
                .desktop(),
            );
        }
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    h.shell.handle().home();
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("b"));
    h.frames(2);
    assert_eq!(h.shell.workspace().tasks().len(), 2);
    let a_id = h.shell.workspace().find("a").map(Instance::id);
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    assert_eq!(
        h.shell.workspace().tasks().len(),
        2,
        "the task count does not grow"
    );
    assert_eq!(h.shell.workspace().find("a").map(Instance::id), a_id);
    assert!(!h.shell.workspace().is_home());
    assert_eq!(
        h.shell.workspace().active_task().and_then(|t| t.root_id()),
        Some("a"),
        "the focused task is a"
    );
    Ok(())
}

/// **One owner.** A resident declaration keeps its screen for the declaration's whole
/// life and the instance holds only the declaration id, so closing the instance throws away
/// nothing: the state is still there when it is opened again. A factory screen is the other way
/// round — the state belongs to the instance and goes with it.
///
/// Before this the resident screen lived in an `Rc<RefCell<dyn Screen>>` shared between the
/// declaration and the instance. This test is what that sharing was *for*; it holds without the
/// cell, because the registry now lends the screen out one borrow at a time.
#[test]
fn a_resident_screen_keeps_its_state_and_a_factory_one_starts_over() -> fairing::Result<()> {
    use fairing::workspace::Instance;

    let resident_log = Rc::new(RefCell::new(Vec::new()));
    let factory_log = Rc::new(RefCell::new(Vec::new()));
    let (resident_in, factory_in) = (Rc::clone(&resident_log), Rc::clone(&factory_log));
    let mut h = test_shell(single_level_access(), move |sh| {
        // The screen's own state: a counter that lives in the closure and is written down each ui.
        let mut drawn = 0u32;
        sh.add(
            screen("resident", move |ui: &mut egui::Ui, _: &mut Cx| {
                drawn += 1;
                resident_in.borrow_mut().push(drawn);
                ui.label("x");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
        sh.add(
            screen_with("factory", move || {
                let log = Rc::clone(&factory_in);
                let mut drawn = 0u32;
                move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                    drawn += 1;
                    log.borrow_mut().push(drawn);
                    ui.label("x");
                }
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;

    for id in ["resident", "factory"] {
        // Open, draw a few frames, close, then open again and draw as many.
        h.shell.handle().launch(LaunchAction::open(id));
        h.frames(3);
        let instance = h
            .shell
            .workspace()
            .find(id)
            .map(Instance::id)
            .ok_or_else(|| fairing::Error::Config(format!("{id} did not open")))?;
        h.shell.handle().close_screen(instance);
        h.frames(3);
        h.shell.handle().launch(LaunchAction::open(id));
        h.frames(3);
    }

    let resident = resident_log.borrow().clone();
    assert!(
        resident.len() > 3,
        "the resident screen has to have been drawn on both openings, got {resident:?}"
    );
    assert!(
        resident
            .iter()
            .zip(resident.iter().skip(1))
            .all(|(a, b)| *b == a + 1),
        "one counter throughout — the state survived the close, got {resident:?}"
    );
    assert_eq!(
        resident.iter().filter(|&&n| n == 1).count(),
        1,
        "the declaration built the screen once and kept it, got {resident:?}"
    );

    let factory = factory_log.borrow().clone();
    assert!(
        factory.len() > 3,
        "the factory screen has to have been drawn on both openings, got {factory:?}"
    );
    assert_eq!(
        factory.iter().filter(|&&n| n == 1).count(),
        2,
        "a fresh screen per opening — its state went with the instance, got {factory:?}"
    );
    Ok(())
}

/// **A screen can draw another declaration's screen inside itself**, and a cycle stops
/// instead of blowing up.
///
/// While a resident screen is being drawn it is *out* of its declaration, so a screen that asks for
/// itself — directly, or round a longer ring — is told no and draws nothing. That is the whole
/// reason the loan is a move rather than a `&mut`: a shared cell would have panicked here.
///
/// A factory declaration (`screen_with`) owns no screen between opens, so there is nothing to lend
/// and it answers `false` too; the caller opens it instead.
#[test]
fn draw_screen_embeds_a_registered_screen_and_a_cycle_stops() -> fairing::Result<()> {
    let log = Rc::new(RefCell::new(Vec::new()));
    let answers = Rc::new(RefCell::new(Vec::new()));
    let can = Rc::new(RefCell::new(Vec::new()));
    let (guest_log, host_answers, host_can) =
        (Rc::clone(&log), Rc::clone(&answers), Rc::clone(&can));
    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("guest", move |ui: &mut egui::Ui, _: &mut Cx| {
            guest_log.borrow_mut().push("guest drew");
            ui.label("guest");
        }));
        sh.add(screen_with("factory", || {
            |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("factory");
            }
        }));
        sh.add(
            screen("host", move |ui: &mut egui::Ui, cx: &mut Cx| {
                host_answers.borrow_mut().clear();
                host_can.borrow_mut().clear();
                for id in ["guest", "host", "factory", "nope"] {
                    host_can.borrow_mut().push((id, cx.can_draw_screen(id)));
                    let drew = cx.draw_screen(ui, id);
                    host_answers.borrow_mut().push((id, drew));
                }
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("host"));
    h.frames(3);

    assert_eq!(
        answers.borrow().as_slice(),
        [
            ("guest", true),
            ("host", false),
            ("factory", false),
            ("nope", false),
        ],
        "a registered resident screen draws; itself, a factory one and a missing id do not"
    );
    assert_eq!(
        can.borrow().as_slice(),
        [
            ("guest", true),
            ("host", false),
            ("factory", false),
            ("nope", false),
        ],
        "`can_draw_screen` has to agree with `draw_screen` — asked before the press, it decides \
         whether a row embeds or opens. A screen already out on loan is still a resident \
         declaration, so answering on that alone said yes to one that then drew nothing"
    );
    assert!(
        !log.borrow().is_empty(),
        "the guest really was drawn, not just reported"
    );
    Ok(())
}

/// **The embedded screen gets a `Cx` of its own, not a borrowed lie**.
///
/// Handing the guest the host's `Cx` unchanged looked harmless and was not:
///
/// - `cx.pane.rect` was the host's whole pane, so `layout::page` built a scroll viewport taller
///   than the column it was drawn in and the overrun could never be scrolled into view.
/// - `cx.pane.instance` was the host's, so a guest calling `cx.finish()` closed **the host**.
/// - `cx.event` still carried the host's lifecycle, and `cx.event` is the only route a closure
///   screen has to one — so every guest saw `Created` and re-ran its initialisation.
#[test]
fn an_embedded_screen_gets_its_own_pane_and_no_lifecycle() -> fairing::Result<()> {
    #[derive(Clone, Copy)]
    struct Seen {
        rect: egui::Rect,
        instance: fairing::workspace::InstanceId,
        event: Option<fairing::Lifecycle>,
    }
    let guest = Rc::new(Cell::new(Seen {
        rect: egui::Rect::NOTHING,
        instance: fairing::workspace::InstanceId(u64::MAX),
        event: Some(fairing::Lifecycle::Destroyed),
    }));
    let host_event = Rc::new(Cell::new(None));
    let given = Rc::new(Cell::new(egui::Rect::NOTHING));
    let (guest_in, host_in, given_in) =
        (Rc::clone(&guest), Rc::clone(&host_event), Rc::clone(&given));

    let mut h = test_shell(single_level_access(), move |sh| {
        sh.add(screen("guest", move |_ui: &mut egui::Ui, cx: &mut Cx| {
            guest_in.set(Seen {
                rect: cx.pane.rect,
                instance: cx.pane.instance,
                event: cx.event,
            });
        }));
        sh.add(
            screen("host", move |ui: &mut egui::Ui, cx: &mut Cx| {
                host_in.set(cx.event);
                // Half the pane, so "the host's rect" and "the guest's rect" cannot be confused.
                let half = ui.available_rect_before_wrap();
                let half = egui::Rect::from_min_max(
                    half.min,
                    egui::pos2(half.center().x, half.center().y),
                );
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(half));
                given_in.set(child.available_rect_before_wrap());
                cx.draw_screen(&mut child, "guest");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("host"));
    h.frames(1);

    let seen = guest.get();
    assert_eq!(
        host_event.get(),
        Some(fairing::Lifecycle::Created),
        "the host really did have a lifecycle event to leak"
    );
    assert_eq!(
        seen.event, None,
        "the guest was not created, resumed or resized — it was drawn"
    );
    assert_eq!(
        seen.instance,
        fairing::workspace::InstanceId::NONE,
        "there is no instance behind an embedded screen, so `finish()` must not reach the host's"
    );
    assert_eq!(
        seen.rect,
        given.get(),
        "the guest's pane is the space it was given, not the host's"
    );
    assert!(
        seen.rect.width() < h.shell.workspace().last_content().width(),
        "and that space really is smaller than the pane: {:?}",
        seen.rect
    );
    Ok(())
}

/// **A host cannot wave a guest past its own gate**.
///
/// `draw_screen` used not to look at the gate at all, on the grounds that the host had drawn the
/// row that led there and so had already decided who could see it. The built-in `settings.home` is
/// such a host, and it lists whatever is registered — so a screen locked to a higher level was
/// painted for a session that `LaunchAction::open` would have refused.
#[test]
fn an_embedded_screen_still_obeys_its_own_gate() -> fairing::Result<()> {
    let drew = Rc::new(Cell::new(true));
    let body = Rc::new(Cell::new(0u32));
    let (drew_in, body_in) = (Rc::clone(&drew), Rc::clone(&body));

    let mut config = access_config(&["viewer", "service"], Some("viewer"));
    config
        .access
        .gates
        .insert("service".to_owned(), "service".to_owned());
    let mut h = test_shell(config, move |sh| {
        sh.add(
            screen("secret", move |_ui: &mut egui::Ui, _: &mut Cx| {
                body_in.set(body_in.get() + 1);
            })
            .gate("service"),
        );
        sh.add(
            screen("host", move |ui: &mut egui::Ui, cx: &mut Cx| {
                drew_in.set(cx.draw_screen(ui, "secret"));
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("host"));
    h.frames(3);

    assert!(
        !drew.get(),
        "a gated screen must not be drawn to a session that cannot open it"
    );
    assert_eq!(body.get(), 0, "and its body never ran");
    Ok(())
}

/// **The app owns its state and lends it to the screens**.
///
/// Until now the only way for three screens and the code outside the frame to reach one machine
/// console was for each closure to capture an `Rc<RefCell<Console>>` — the guide said so in as many
/// words and every one of this crate's examples did it. Then the first attempt at a fix had the
/// **shell** own the value, which is the ownership upside down: the app's domain state living
/// inside a UI library. The app owns both, and lends the state a frame at a time.
#[test]
fn the_app_owns_its_state_and_lends_it_to_the_screens() -> fairing::Result<()> {
    #[derive(Default)]
    struct Console {
        sent: Vec<&'static str>,
        temperature: f32,
    }

    let mut h = test_shell(single_level_access(), |sh| {
        // Three screens, none of them capturing anything.
        sh.add(
            screen("run", |ui: &mut egui::Ui, cx: &mut Cx| {
                if let Some(console) = cx.app_mut::<Console>() {
                    console.sent.push("run");
                }
                ui.label("run");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
        sh.add(screen("recipe", |ui: &mut egui::Ui, cx: &mut Cx| {
            if let Some(console) = cx.app_mut::<Console>() {
                console.sent.push("recipe");
            }
            ui.label("recipe");
        }));
        sh.add(screen("readout", |ui: &mut egui::Ui, cx: &mut Cx| {
            // A read-only borrow of the same value, and `with_app` where the theme is wanted too.
            let temperature = cx.app::<Console>().map_or(0.0, |c| c.temperature);
            cx.with_app::<Console, _>(|console, cx| {
                let _ = cx.theme.metrics.row_height;
                console.sent.push("readout");
            });
            ui.label(format!("{temperature:.1}"));
        }));
    })?
    .with_app(Console {
        temperature: 42.5,
        ..Console::default()
    });

    h.shell.handle().launch(LaunchAction::open("run"));
    h.frames(3);
    h.shell.handle().launch(LaunchAction::open("recipe"));
    h.frames(3);
    h.shell.handle().launch(LaunchAction::open("readout"));
    h.frames(3);

    let console = h
        .app_mut::<Console>()
        .ok_or_else(|| fairing::Error::Config("the console went missing".to_owned()))?;
    for want in ["run", "recipe", "readout"] {
        assert!(
            console.sent.contains(&want),
            "every screen wrote into the one value: {:?}",
            console.sent
        );
    }
    assert!(
        (console.temperature - 42.5).abs() < f32::EPSILON,
        "and what the app put there is still there"
    );
    // The app can write it between frames — it owns it.
    console.temperature = 10.0;
    assert!(h
        .app_mut::<Console>()
        .is_some_and(|c| (c.temperature - 10.0).abs() < f32::EPSILON));
    Ok(())
}

/// **An out-of-frame call can carry the app's state too**.
///
/// Inside a frame the shell is holding what the app lent it, so a tap that runs an `action(id, ..)`
/// closure gives it `cx.app()`. Between frames the shell is holding nothing — the app is — so a
/// direct `shell.launch(run(..))` runs that closure with `None`. `launch_with` is the same call
/// with the state lent for its length, and `back_with` does it for `on_back`.
#[test]
fn an_out_of_frame_call_can_carry_the_app_state() -> fairing::Result<()> {
    use fairing::action;

    #[derive(Default)]
    struct Console {
        seen: Vec<bool>,
    }

    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(action("calibrate", |cx: &mut Cx<'_>| {
            let had = cx.app::<Console>().is_some();
            if let Some(console) = cx.app_mut::<Console>() {
                console.seen.push(had);
            }
        }));
    })?
    .with_app(Console::default());

    // Between frames with nothing lent: the closure runs, and finds nothing.
    h.shell.launch(LaunchAction::run("calibrate"));
    assert_eq!(
        h.app_mut::<Console>().map(|c| c.seen.len()),
        Some(0),
        "it could not even write the fact down — `cx.app()` was None"
    );

    // The same call, lending the state.
    let Some(mut console) = h.app_mut::<Console>().map(std::mem::take) else {
        return Err(fairing::Error::Config("no console".to_owned()));
    };
    h.shell
        .launch_with(LaunchAction::run("calibrate"), &mut console);
    assert_eq!(
        console.seen,
        vec![true],
        "with `launch_with` the action sees the app's state"
    );
    Ok(())
}

/// **A `struct` screen can be resident**, which is what `on_back` needs.
///
/// `screen` takes a closure, which is the trait's `ui` and nothing else. A screen wanting
/// `on_back` — "back cancels the edit I am in the middle of" — has to be a type, and the only way
/// to register a type was `screen_with`, a **factory**: fresh state per open, dropped on close, and
/// nothing owned between opens for `draw_screen` to embed. So "keeps its state and handles back"
/// and "goes in the settings right column" were mutually exclusive. `screen_of` is both.
#[test]
fn a_struct_screen_can_be_resident_and_handle_back() -> fairing::Result<()> {
    use fairing::screen::{screen_of, BackAction, Screen};

    /// What the test reads, in the shell's own state slot rather than a cell.
    #[derive(Default)]
    struct Log {
        draws: u32,
        backs: Vec<&'static str>,
    }

    #[derive(Default)]
    struct Network {
        editing: Option<String>,
        draws: u32,
    }

    impl Screen for Network {
        fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
            self.draws += 1;
            if self.draws == 1 {
                self.editing = Some("10.0.0.1".to_owned());
            }
            if let Some(log) = cx.app_mut::<Log>() {
                log.draws = self.draws;
            }
            ui.label(self.editing.as_deref().unwrap_or("idle"));
        }

        fn on_back(&mut self, cx: &mut Cx<'_>) -> BackAction {
            // Back in the middle of an edit cancels the edit rather than leaving the screen.
            let cancelled = self.editing.take().is_some();
            if let Some(log) = cx.app_mut::<Log>() {
                log.backs
                    .push(if cancelled { "cancelled" } else { "popped" });
            }
            if cancelled {
                BackAction::Consumed
            } else {
                BackAction::Pop
            }
        }
    }

    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen_of("net", Network::default())
                .icon(icon::GAUGE)
                .desktop(),
        );
        sh.add(
            screen("host", |ui: &mut egui::Ui, cx: &mut Cx| {
                cx.draw_screen(ui, "net");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?
    .with_app(Log::default());
    assert!(
        h.shell
            .registry()
            .screen("net")
            .is_some_and(fairing::screen::ScreenDecl::is_resident),
        "screen_of registers a **resident** declaration, so it can be embedded and it keeps state"
    );

    h.shell.handle().launch(LaunchAction::open("net"));
    h.frames(3);
    let after_open = h.app_mut::<Log>().map_or(0, |l| l.draws);
    assert!(after_open >= 1, "it drew");

    // Back once: the edit is cancelled and the screen stays.
    h.shell.handle().back();
    h.frames(3);
    assert_eq!(
        h.app_mut::<Log>().map(|l| l.backs.as_slice()),
        Some(["cancelled"].as_slice()),
        "on_back ran and consumed the press"
    );
    assert!(!h.shell.workspace().is_home(), "so the screen is still up");

    // Back again: nothing to cancel, so it pops.
    h.shell.handle().back();
    h.frames(4);
    assert_eq!(
        h.app_mut::<Log>().map(|l| l.backs.len()),
        Some(2),
        "the second press reached it too"
    );
    assert!(h.shell.workspace().is_home(), "and this time it left");

    // Reopened, the value is the same one — its counter carries on rather than restarting.
    let before_reopen = h.app_mut::<Log>().map_or(0, |l| l.draws);
    h.shell.handle().launch(LaunchAction::open("net"));
    h.frames(3);
    assert!(
        h.app_mut::<Log>().map_or(0, |l| l.draws) > before_reopen,
        "a resident declaration keeps its screen across close and reopen"
    );

    // And it is embeddable, which a factory declaration is not.
    h.shell.handle().home();
    h.frames(3);
    let before_embed = h.app_mut::<Log>().map_or(0, |l| l.draws);
    h.shell.handle().launch(LaunchAction::open("host"));
    h.frames(3);
    assert!(
        h.app_mut::<Log>().map_or(0, |l| l.draws) > before_embed,
        "`cx.draw_screen` drew the struct screen inside another one"
    );
    Ok(())
}

#[test]
fn toggle_setting_emits_event_and_respects_gate() -> fairing::Result<()> {
    use fairing::settings::SettingValue;
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.shell
        .handle()
        .launch(LaunchAction::Toggle("app.led".into()));
    h.frames(1);
    h.shell
        .handle()
        .set_setting("app.name", SettingValue::Text("x".to_owned()));
    h.frames(1);
    let events = h.shell.poll_events();
    assert!(events.iter().any(|e| matches!(
        e,
        ShellEvent::SettingChanged { key, value: SettingValue::Bool(true) } if key.0 == "app.led"
    )));
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::SettingChanged { key, .. } if key.0 == "app.name")));
    h.shell.launch(LaunchAction::Toggle("app.led".into()));
    assert!(h.shell.poll_events().iter().any(|e| matches!(
        e,
        ShellEvent::SettingChanged {
            value: SettingValue::Bool(false),
            ..
        }
    )));
    // The gate: with a two-level table and default top, the key-name gate is not crossed → the event alone.
    let mut locked = test_shell(
        access_config(&["viewer", "maintainer"], Some("top")),
        |_| {},
    )?;
    locked.shell.launch(LaunchAction::Set(
        "app.led".into(),
        SettingValue::Bool(true),
    ));
    let events = locked.shell.poll_events();
    assert!(!events
        .iter()
        .any(|e| matches!(e, ShellEvent::SettingChanged { .. })));
    assert!(events.iter().any(|e| matches!(
        e,
        ShellEvent::Access(fairing::access::AccessEvent::UnlockRequested {
            then: Some(LaunchAction::Set(..)),
            ..
        })
    )));
    Ok(())
}

#[test]
fn evict_after_drops_stopped_owned_instance() -> fairing::Result<()> {
    // Virtual time: 100 ms after home a generative instance comes down.
    struct Blank;
    impl fairing::Screen for Blank {
        fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
            ui.label("e");
        }
    }
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            fairing::screen_with("e", || Blank)
                .icon(icon::GAUGE)
                .desktop()
                .evict_after(std::time::Duration::from_millis(100)),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("e"));
    h.frames(2);
    h.shell.handle().home();
    h.frames(2);
    assert!(
        h.shell.workspace().find("e").is_some(),
        "it is alive straight after home"
    );
    h.run_for(0.2);
    assert!(
        h.shell.workspace().find("e").is_none(),
        "it comes down 100 ms later"
    );
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "e")));
    Ok(())
}

#[test]
fn access_downgrade_closes_gated_screen_and_notifies() -> fairing::Result<()> {
    use fairing::{Level, Subject};
    let mut config = access_config(&["viewer", "maintainer"], Some("top"));
    config.access.initial = Some("maintainer".to_owned());
    config
        .access
        .gates
        .insert("open".to_owned(), "viewer".to_owned());
    let seen = Rc::new(Cell::new(false));
    let s = Rc::clone(&seen);
    let mut h = test_shell(config, |sh| {
        sh.add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("admin");
        }));
        sh.add(screen("open", move |ui: &mut egui::Ui, cx: &mut Cx| {
            if cx.event == Some(fairing::Lifecycle::AccessChanged) {
                s.set(true);
            }
            ui.label("open");
        }));
    })?;
    h.shell.handle().launch(LaunchAction::open("open"));
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("admin"));
    h.frames(2);
    assert!(h.shell.workspace().find("admin").is_some());
    h.shell.handle().set_subject(Subject {
        id: None,
        level: Level(0),
        attrs: std::collections::BTreeMap::default(),
    });
    // The closure gets one event per ui call. While `open` is not drawn under admin, `Paused` and `Stopped` pile
    // up, so: frame 1 handles it, 2 `Paused`, 3 `Stopped`, 4 `Resumed`, 5 `AccessChanged`.
    h.frames(6);
    assert!(
        h.shell.workspace().find("admin").is_none(),
        "an instance that does not pass the gate is closed"
    );
    assert!(
        h.shell.workspace().find("open").is_some(),
        "one that passes stays"
    );
    assert!(seen.get(), "the instance left gets AccessChanged");
    let events = h.shell.poll_events();
    assert!(events
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "admin")));
    Ok(())
}

#[test]
fn overlay_policy_keeps_status_bar() -> fairing::Result<()> {
    use fairing::screen::BarMode;
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("o", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("o");
            })
            .chrome(fairing::ChromePolicy {
                status_bar: BarMode::Overlay,
                ..fairing::ChromePolicy::default()
            }),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("o"));
    h.frames(3);
    let layout = *h.shell.layout();
    assert!(layout.status_overlay && layout.status.is_some());
    assert!(
        (layout.content.min.y - h.screen_rect().min.y).abs() < f32::EPSILON,
        "an Overlay does not shrink the content"
    );
    assert!(
        h.shell.status_bar().item_rect("status.clock").is_some(),
        "the status bar is still drawn"
    );
    Ok(())
}

// ───────────────────────── Launch modes, lifecycle, threads, animation and fullscreen, plus the integration regressions ─────────────────────────

/// A generative screen that writes each `on_lifecycle` down in order.
struct Recorder {
    log: Rc<RefCell<Vec<fairing::Lifecycle>>>,
}

impl fairing::Screen for Recorder {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("r");
    }

    fn on_lifecycle(&mut self, event: fairing::Lifecycle, _cx: &mut Cx<'_>) {
        self.log.borrow_mut().push(event);
    }
}

fn recorder(id: &str, log: &Rc<RefCell<Vec<fairing::Lifecycle>>>) -> fairing::screen::ScreenDecl {
    let log = Rc::clone(log);
    fairing::screen_with(id, move || Recorder {
        log: Rc::clone(&log),
    })
    .icon(icon::GAUGE)
    .desktop()
}

fn new_log() -> Rc<RefCell<Vec<fairing::Lifecycle>>> {
    Rc::new(RefCell::new(Vec::new()))
}

/// Home keeps the task alive (`Stopped`), and re-tapping the icon comes back to **the same
/// instance** (Single, alive in another task).
#[test]
fn home_keeps_task_alive_and_icon_retap_resumes() -> fairing::Result<()> {
    use fairing::workspace::Instance;
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.frames(2);
    let Some(rect) = h.shell.desktop().icon_rect("a") else {
        return Err(fairing::Error::Config("there is no icon Rect".to_owned()));
    };
    h.tap(rect.center());
    let first = h.shell.workspace().find("a").map(Instance::id);
    assert!(first.is_some() && !h.shell.workspace().is_home());
    h.shell.handle().home();
    h.frames(2);
    assert!(h.shell.workspace().is_home());
    let stopped = h.shell.workspace().find("a");
    assert!(
        stopped.is_some_and(Instance::is_stopped),
        "after home the task is alive and Stopped"
    );
    h.frames(1);
    h.tap(rect.center());
    assert!(
        !h.shell.workspace().is_home(),
        "re-tapping comes back to the task"
    );
    assert_eq!(
        h.shell.workspace().find("a").map(Instance::id),
        first,
        "it does not make a new instance"
    );
    assert_eq!(h.shell.workspace().tasks().len(), 1);
    Ok(())
}

/// Single inside the current task: after a→b→c, `open("a")` clears what is above it — the stack is `[a]` and b and c
/// get `Destroyed`.
#[test]
fn single_launch_clears_top() -> fairing::Result<()> {
    use fairing::workspace::Instance;
    use fairing::Lifecycle::Destroyed;
    let (log_b, log_c) = (new_log(), new_log());
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
        sh.add(recorder("b", &log_b));
        sh.add(recorder("c", &log_c));
    })?;
    for id in ["a", "b", "c"] {
        h.shell.handle().launch(LaunchAction::open(id));
        h.frames(2);
    }
    let a_id = h.shell.workspace().find("a").map(Instance::id);
    assert_eq!(
        h.shell
            .workspace()
            .active_task()
            .map(fairing::workspace::Task::len),
        Some(3)
    );
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    let ids: Vec<&str> = h
        .shell
        .workspace()
        .active_task()
        .map(|t| t.iter().map(Instance::decl_id).collect())
        .unwrap_or_default();
    assert_eq!(ids, ["a"], "the stack is [a]");
    assert_eq!(h.shell.workspace().find("a").map(Instance::id), a_id);
    assert!(h.shell.workspace().find("b").is_none() && h.shell.workspace().find("c").is_none());
    assert_eq!(log_b.borrow().last(), Some(&Destroyed));
    assert_eq!(log_c.borrow().last(), Some(&Destroyed));
    let events = h.shell.poll_events();
    for id in ["b", "c"] {
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ShellEvent::ScreenClosed { id: closed, .. } if closed == id)),
            "{id}'s ScreenClosed"
        );
    }
    Ok(())
}

/// `LaunchMode::Multi`: opening a generative screen twice gives two instances with different ids.
#[test]
fn multi_launch_pushes_new_instances() -> fairing::Result<()> {
    use fairing::workspace::Instance;
    struct Blank;
    impl fairing::Screen for Blank {
        fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
            ui.label("m");
        }
    }
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            fairing::screen_with("m", || Blank)
                .icon(icon::GAUGE)
                .desktop()
                .launch(LaunchMode::Multi),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("m"));
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("m"));
    h.frames(2);
    let ids: Vec<_> = h
        .shell
        .workspace()
        .active_task()
        .map(|t| t.iter().map(Instance::id).collect())
        .unwrap_or_default();
    assert_eq!(ids.len(), 2, "two instances (pushed onto the same task)");
    assert_ne!(ids.first(), ids.get(1), "different instance ids");
    assert!(h.shell.workspace().find("m").is_some());
    Ok(())
}

/// The settled `reduce = true` sequence: open a → open b → back.
///
/// A closure screen gets the same events through `cx.event`, one per ui call, and the first call is `Created`.
#[test]
fn lifecycle_order_for_trait_and_closure() -> fairing::Result<()> {
    use fairing::Lifecycle::{Created, Destroyed, Paused, Resumed, Stopped};
    let (log_a, log_b) = (new_log(), new_log());
    let closure_events = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&closure_events);
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(recorder("a", &log_a));
        sh.add(recorder("b", &log_b));
        sh.add(screen("c", move |ui: &mut egui::Ui, cx: &mut Cx| {
            seen.borrow_mut().push(cx.event);
            ui.label("c");
        }));
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    h.shell.handle().launch(LaunchAction::open("b"));
    h.frames(2);
    h.shell.handle().back();
    h.frames(2);
    assert_eq!(
        *log_a.borrow(),
        [Created, Resumed, Paused, Stopped, Resumed],
        "a's sequence"
    );
    assert_eq!(
        *log_b.borrow(),
        [Created, Resumed, Paused, Destroyed],
        "b's sequence"
    );
    // The closure: opening c gives Created on the first ui, then Resumed, and nothing after that.
    h.shell.handle().launch(LaunchAction::open("c"));
    h.frames(3);
    let events = closure_events.borrow();
    assert_eq!(
        events.first(),
        Some(&Some(Created)),
        "Created on the first ui"
    );
    assert_eq!(events.get(1), Some(&Some(Resumed)));
    assert_eq!(events.get(2), Some(&None), "no events after that");
    Ok(())
}

/// `ShellHandle: Send` — a command sent from another thread is handled on the next frame.
#[test]
fn handle_launch_from_other_thread() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("a");
        }));
    })?;
    let handle = h.shell.handle();
    std::thread::scope(|scope| {
        scope.spawn(move || handle.launch(LaunchAction::open("a")));
    });
    h.frames(2);
    assert!(h.shell.workspace().find("a").is_some());
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "a")));
    Ok(())
}

/// `cx.animate` converges inside the tween's length, and when a screen is closed and the lookups
/// stop, the entry is thrown away `AnimationStore::RETAIN_FRAMES` frames later (it is still there the frame before).
#[test]
fn cx_animate_converges_and_store_prunes() -> fairing::Result<()> {
    use fairing::motion::{AnimationStore, Tween};
    let values = Rc::new(RefCell::new(Vec::new()));
    let v = Rc::clone(&values);
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(single_level_access(), services)?; // reduce = false
    h.shell
        .add(screen("a", move |ui: &mut egui::Ui, cx: &mut Cx| {
            // An id seen for the first time goes straight to its target, so the Created frame leaves it at 0 and sends 1 from then on.
            let target = if cx.event == Some(fairing::Lifecycle::Created) {
                0.0
            } else {
                1.0
            };
            let tween = Tween::cubic_out(std::time::Duration::from_millis(100));
            v.borrow_mut()
                .push(cx.animate(egui::Id::new("fade"), target, tween));
            ui.label("a");
        }));
    h.frames(2);
    // The nav bar's A7 press scale uses the same store (one per item, looked up every frame) — it is subtracted as the baseline.
    let base = h.shell.animations().len();
    h.shell.handle().launch(LaunchAction::open("a"));
    h.run_for(0.5); // the A2 open (240 ms) plus the tween (100 ms ≈ 6 frames)
    let seen = values.borrow().clone();
    assert_eq!(seen.first(), Some(&0.0), "0 on the Created frame");
    assert!(
        seen.iter().any(|v| *v > 0.0 && *v < 1.0),
        "it passes through a middle value ({seen:?})"
    );
    assert!(
        seen.last().is_some_and(|v| (v - 1.0).abs() < 1e-4),
        "it converges ({seen:?})"
    );
    assert_eq!(h.shell.animations().len(), base + 1);
    // Closing it stops the lookups. After the last lookup frame F, F+1..F+RETAIN_FRAMES−1 stay and the
    // F+RETAIN_FRAMES-th tick throws it away — the boundary is written from the constant.
    assert!(h.shell.remove("a"));
    let keep = usize::try_from(AnimationStore::RETAIN_FRAMES - 1).unwrap_or(599);
    h.frames(keep);
    assert_eq!(
        h.shell.animations().len(),
        base + 1,
        "it is still there just before RETAIN_FRAMES"
    );
    h.frames(1);
    assert_eq!(
        h.shell.animations().len(),
        base,
        "the RETAIN_FRAMES-th tick throws it away"
    );
    Ok(())
}

/// With a `.fullscreen()` screen open there is no status bar and no nav bar, and the content is the whole screen.
#[test]
fn chrome_policy_fullscreen_expands_content() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("f", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("f");
            })
            .fullscreen(),
        );
    })?;
    h.frames(1);
    assert!(h.shell.layout().status.is_some() && h.shell.layout().nav.is_some());
    h.shell.handle().launch(LaunchAction::open("f"));
    h.frames(2);
    let layout = *h.shell.layout();
    assert!(layout.status.is_none() && layout.nav.is_none());
    assert_eq!(layout.content, h.screen_rect());
    assert!(h
        .shell
        .nav_bar()
        .item_rect(&fairing::chrome::NavItem::Back)
        .is_none());
    h.shell.handle().home();
    h.frames(2);
    assert!(h.shell.layout().status.is_some() && h.shell.layout().nav.is_some());
    Ok(())
}

/// Adding the same id again replaces it: an instance that was open is closed and `ScreenClosed` fires
/// (so an integrator can tidy up external state).
#[test]
fn add_replace_closes_open_instance_and_notifies() -> fairing::Result<()> {
    use fairing::workspace::Instance;
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("old");
        }));
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    let old = h.shell.workspace().find("a").map(Instance::id);
    let _ = h.shell.poll_events();
    h.shell.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
        ui.label("new");
    }));
    h.frames(1);
    assert!(
        h.shell.workspace().is_home(),
        "the only screen that was open closed, so home"
    );
    let events = h.shell.poll_events();
    assert!(events.iter().any(
        |e| matches!(e, ShellEvent::ScreenClosed { id, instance } if id == "a" && Some(*instance) == old)
    ));
    assert!(events.iter().any(|e| matches!(e, ShellEvent::WentHome)));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::DeclRemoved(_))),
        "a replacement is not a removal"
    );
    // The new declaration opens as it should.
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    assert!(h.shell.workspace().find("a").is_some());
    Ok(())
}

/// Back is ignored on the desktop: at home the back button is disabled (tapping does nothing), and inside a task it is enabled.
#[test]
fn nav_back_is_disabled_on_home() -> fairing::Result<()> {
    use fairing::chrome::NavItem;
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
            ui.label("a");
        }));
    })?;
    h.frames(2);
    assert!(!h.shell.nav_bar().back_enabled, "disabled at home");
    let Some(back) = h.shell.nav_bar().item_rect(&NavItem::Back) else {
        return Err(fairing::Error::Config(
            "there is no back button Rect".to_owned(),
        ));
    };
    h.tap(back.center());
    assert!(h.shell.workspace().is_home());
    assert!(
        !h.shell
            .poll_events()
            .iter()
            .any(|e| matches!(e, ShellEvent::WentHome)),
        "back does nothing at home"
    );
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    assert!(h.shell.nav_bar().back_enabled, "enabled inside a task");
    Ok(())
}

/// Whether the demo config (`examples/demo.toml`) hides the status bar outright — where `default_gate = "top"`,
/// leaving the gate assignment off `status.*` and the integrator's items draws no item at all, and the "see the
/// status bar (the clock, the Mock Wi-Fi and the battery) and the integrator's items for yourself" the demo's docs
/// promise becomes impossible. It reads **the same file** as the demo, so a config that drifts is caught here.
#[test]
fn demo_config_keeps_status_bar_visible() -> fairing::Result<()> {
    use fairing::{status_item, Slot};
    let config = fairing::ShellConfig::from_toml(include_str!("../examples/demo.toml"))?;
    let mut h = Harness::new(config, fairing::services::mock::services())?;
    h.shell.add(status_item("temp", Slot::Right, |ui, _cx| {
        ui.label("36.5°");
    }));
    h.frames(2);
    for id in [
        "status.clock",
        "status.wifi",
        "status.bluetooth",
        "status.battery",
        "temp",
    ] {
        assert!(
            h.shell.status_bar().item_rect(id).is_some(),
            "the status bar item `{id}` is not drawn from the demo config (a missing gate assignment?)"
        );
    }
    Ok(())
}

/// `remove` gives a [`ShellEvent::DeclRemoved`] whatever the kind — a built-in status item has the same shape
/// (a built-in item is on the same id scheme). An id that is not there is `false` with no event.
#[test]
fn removing_a_builtin_status_item_emits_decl_removed() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(1);
    let _ = h.shell.poll_events();
    assert!(h.shell.remove("status.clock"));
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::DeclRemoved(id) if id == "status.clock")),
        "removing a built-in status item gives a DeclRemoved too: {events:?}"
    );
    assert!(!h.shell.remove("status.clock"));
    assert!(
        h.shell.poll_events().is_empty(),
        "removing an id that is not there gives no event"
    );
    Ok(())
}

/// With the clock taken out and no backend wake either, an idle frame schedules no repaint at all — waking at the clock's
/// next minute boundary is a rule for when the clock is **visible** (repaints come only from input, a wake or an animation).
#[test]
fn idle_without_a_visible_clock_schedules_no_repaint() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |_| {})?;
    h.frames(3);
    assert!(
        h.shell.status_bar().drawn_clock().is_some(),
        "by default the clock is there"
    );
    assert!(h.shell.remove("status.clock"));
    h.frames(3);
    assert!(h.shell.status_bar().drawn_clock().is_none());
    assert!(!h.shell.is_animating());
    assert!(
        !h.ctx.has_requested_repaint(),
        "with no visible clock and no backend wake there is no reason to wake"
    );
    Ok(())
}

/// Closing a background task's instance while already at home is **not a home transition** — `WentHome` fires only
/// on going from not-home to home (`close_decl_instances`).
#[test]
fn closing_a_background_instance_at_home_does_not_repeat_went_home() -> fairing::Result<()> {
    let mut h = test_shell(single_level_access(), |sh| {
        sh.add(
            screen("a", |ui: &mut egui::Ui, _: &mut Cx| {
                ui.label("a");
            })
            .icon(icon::GAUGE)
            .desktop(),
        );
    })?;
    h.shell.handle().launch(LaunchAction::open("a"));
    h.frames(2);
    let Some(instance) = h
        .shell
        .workspace()
        .find("a")
        .map(fairing::workspace::Instance::id)
    else {
        return Err(fairing::Error::Config("there is no a instance".to_owned()));
    };
    h.shell.home();
    h.frames(1);
    assert!(h.shell.workspace().is_home());
    let events = h.shell.poll_events();
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ShellEvent::WentHome))
            .count(),
        1,
        "it went home once: {events:?}"
    );
    h.shell.handle().close_screen(instance);
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenClosed { id, .. } if id == "a")),
        "the background task's instance is closed: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, ShellEvent::WentHome)),
        "it was home from start to finish, so there is no transition: {events:?}"
    );
    Ok(())
}
