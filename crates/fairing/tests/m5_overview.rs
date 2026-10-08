//! M5 — the overview of recent screens (A10).
//!
//! The nav bar's `"recents"` (and `LaunchAction::OpenOverview`) bring up the shell's own overview:
//! a card per live task. A tap brings a task forward, a card thrown upward ends its task, "Close
//! all" ends them all, a card's split button puts its task beside the pane on show — and the split
//! control opens the same cards as a picker.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::testing::{test_shell, Harness};
use fairing::workspace::{DrawnCard, Instance};
use fairing::{screen, Cx, LaunchAction, ShellConfig, ShellEvent, SplitSupport};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// A shell with screens `a`, `b` and `c`.
fn shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = test_shell(config, |sh| {
        for id in ["a", "b", "c"] {
            sh.add(
                screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                    ui.label(id);
                })
                .title(id.to_uppercase()),
            );
        }
    })?;
    h.frames(2);
    Ok(h)
}

/// `a` then `b` opened (from home each time, so two tasks), `b` on show.
fn two_tasks(h: &mut Harness) {
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
}

fn overview(h: &mut Harness) {
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
}

fn cards(h: &Harness) -> Vec<DrawnCard> {
    h.shell.workspace().overview_cards_drawn().to_vec()
}

/// The card of the task whose root is `id`.
fn card_of(h: &Harness, id: &str) -> fairing::Result<DrawnCard> {
    let key = h
        .shell
        .workspace()
        .tasks()
        .iter()
        .find(|t| t.root_id() == Some(id))
        .and_then(|t| t.iter().next().map(fairing::workspace::Instance::id))
        .ok_or_else(|| fail(format!("no task `{id}`")))?;
    cards(h)
        .into_iter()
        .find(|c| c.key == key)
        .ok_or_else(|| fail(format!("no card for `{id}`")))
}

fn closed(h: &mut Harness) -> Vec<String> {
    h.shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::ScreenClosed { id, .. } => Some(id),
            _ => None,
        })
        .collect()
}

/// Recents bring the cards up — the task on show first — and back goes back to it.
#[test]
fn recents_bring_the_cards_up_and_back_goes_back() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    overview(&mut h);
    assert!(h.shell.workspace().is_overview_open());
    let drawn = cards(&h);
    assert_eq!(drawn.len(), 2);
    let first = card_of(&h, "b")?;
    assert_eq!(
        drawn.first().map(|c| c.key),
        Some(first.key),
        "the task on show first"
    );
    h.shell.back();
    h.frames(3);
    assert!(!h.shell.workspace().is_overview_open());
    assert_eq!(
        h.shell.workspace().focused().map(Instance::decl_id),
        Some("b")
    );
    Ok(())
}

/// A card tapped brings its task forward (A10's fallback — no icon to zoom from).
#[test]
fn a_tapped_card_brings_its_task_forward() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    overview(&mut h);
    // The older card is beside the first; the carousel scrolls it into reach first.
    let a = card_of(&h, "a")?;
    let content = h.shell.workspace().last_content();
    let at = egui::pos2(
        a.rect.min.x.clamp(content.min.x, content.max.x - 1.0) + 4.0,
        a.rect.center().y,
    );
    h.tap(at);
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(!ws.is_overview_open());
    assert_eq!(ws.focused().map(Instance::decl_id), Some("a"));
    Ok(())
}

/// A card thrown upward ends its task (A10) — and is reported as closed.
#[test]
fn a_card_thrown_up_ends_its_task() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    overview(&mut h);
    let b = card_of(&h, "b")?;
    let from = b.rect.center();
    h.drag(from, from - egui::vec2(0.0, 260.0), 8);
    h.frames(4);
    assert!(h.shell.workspace().find("b").is_none(), "`b` ended");
    assert!(h.shell.workspace().find("a").is_some(), "`a` did not");
    assert_eq!(closed(&mut h), vec!["b".to_owned()]);
    assert!(
        h.shell.workspace().is_overview_open(),
        "the cards stay up for the next"
    );
    Ok(())
}

/// "Close all" ends every task and the overview with them: home.
#[test]
fn close_all_ends_every_task() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    overview(&mut h);
    let button = h
        .shell
        .workspace()
        .overview_close_all_rect()
        .ok_or_else(|| fail("no Close all"))?;
    h.tap(button.center());
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(ws.tasks().is_empty() && ws.is_home() && !ws.is_overview_open());
    let mut gone = closed(&mut h);
    gone.sort();
    assert_eq!(gone, vec!["a".to_owned(), "b".to_owned()]);
    Ok(())
}

/// A card's split button puts its task beside the pane on show.
#[test]
fn a_cards_split_button_puts_it_beside() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    overview(&mut h);
    let a = card_of(&h, "a")?;
    let button = a.beside.ok_or_else(|| fail("`a` has no split button"))?;
    let content = h.shell.workspace().last_content();
    if !content.contains(button.center()) {
        // Off to the side: bring the card in first, as a finger would.
        let from = content.center();
        h.drag(
            from,
            from - egui::vec2(a.rect.center().x - content.center().x, 0.0),
            8,
        );
        h.frames(30);
    }
    let a = card_of(&h, "a")?;
    let button = a.beside.ok_or_else(|| fail("`a` has no split button"))?;
    h.tap(button.center());
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(ws.is_split() && !ws.is_overview_open());
    assert_eq!(ws.pane_task(1).and_then(|t| t.root_id()), Some("a"));
    Ok(())
}

/// The split control opens the cards as a picker — the task on show has none — and a card
/// tapped goes beside it.
#[test]
fn the_split_control_picks_from_the_cards() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    h.shell.launch(LaunchAction::ToggleSplit);
    h.frames(3);
    assert!(h.shell.workspace().is_overview_picking());
    let drawn = cards(&h);
    assert_eq!(
        drawn.len(),
        1,
        "`b` is on show, so only `a` can go beside it"
    );
    let a = card_of(&h, "a")?;
    h.tap(a.rect.center());
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(ws.is_split());
    assert_eq!(ws.pane_task(0).and_then(|t| t.root_id()), Some("b"));
    assert_eq!(ws.pane_task(1).and_then(|t| t.root_id()), Some("a"));
    Ok(())
}

/// Off in `[workspace]`, recents are only reported — a device with its own overview.
#[test]
fn with_the_overview_off_recents_are_only_reported() -> fairing::Result<()> {
    let mut config = ShellConfig::default();
    config.workspace.overview = false;
    let mut h = shell(config)?;
    two_tasks(&mut h);
    let _ = h.shell.poll_events();
    overview(&mut h);
    assert!(!h.shell.workspace().is_overview_open());
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OverviewRequested)));
    Ok(())
}

/// A viewer/maintainer table where everything is the viewer's but `gate`, which is the
/// maintainer's — and the session is the viewer's.
fn gated(gate: &str) -> ShellConfig {
    let mut config = fairing::testing::access_config(&["viewer", "maintainer"], Some("bottom"));
    config
        .access
        .gates
        .insert(gate.to_owned(), "maintainer".to_owned());
    config
}

fn unlock_asked(h: &mut Harness, gate: &str) -> bool {
    h.shell.poll_events().iter().any(|e| {
        matches!(
            e,
            ShellEvent::Access(fairing::access::AccessEvent::UnlockRequested { gate: g, .. })
                if g.as_str() == gate
        )
    })
}

/// `nav.recents` gates the overview — a session short of it is asked up instead
/// (`prompt` with nothing to ask behaves as `routing`).
#[test]
fn the_recents_gate_holds_the_overview_back() -> fairing::Result<()> {
    let mut h = shell(gated("nav.recents"))?;
    two_tasks(&mut h);
    let _ = h.shell.poll_events();
    overview(&mut h);
    assert!(!h.shell.workspace().is_overview_open());
    assert!(unlock_asked(&mut h, "nav.recents"));
    Ok(())
}

/// Short of `workspace.split`, the overview still comes up but its cards carry no split
/// buttons.
#[test]
fn short_of_the_split_gate_the_cards_have_no_split_button() -> fairing::Result<()> {
    let mut h = shell(gated("workspace.split"))?;
    two_tasks(&mut h);
    overview(&mut h);
    assert!(h.shell.workspace().is_overview_open());
    let drawn = cards(&h);
    assert_eq!(drawn.len(), 2);
    assert!(drawn.iter().all(|card| card.beside.is_none()), "{drawn:?}");
    Ok(())
}

/// A shell with `a`, `b`, `c` and `solo` — a screen that never shares the content.
fn with_solo() -> fairing::Result<Harness> {
    let mut h = test_shell(ShellConfig::default(), |sh| {
        for id in ["a", "b", "c"] {
            sh.add(
                screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                    ui.label(id);
                })
                .title(id.to_uppercase()),
            );
        }
        sh.add(
            screen("solo", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("solo");
            })
            .split(SplitSupport::No),
        );
    })?;
    h.frames(2);
    Ok(h)
}

/// Swipe the carousel along until the card of `id` is in the middle, and hand it back.
fn swipe_to(h: &mut Harness, id: &str) -> fairing::Result<DrawnCard> {
    let content = h.shell.layout().content;
    for _ in 0..6 {
        if let Ok(card) = card_of(h, id) {
            if (card.rect.center().x - content.center().x).abs() < 2.0 {
                return Ok(card);
            }
        }
        let from = content.center();
        h.drag(from, from - egui::vec2(content.width() * 0.4, 0.0), 8);
        h.frames(30);
    }
    card_of(h, id)
}

/// What the toasts on screen and in the queue say.
fn toasts(h: &Harness) -> Vec<String> {
    h.shell
        .toasts()
        .visible()
        .iter()
        .map(|t| t.toast.text.clone())
        .collect()
}

/// The picker offers only what can really go beside — a tap on a card that cannot would make the
/// cards vanish with nothing in their place.
#[test]
fn the_picker_offers_only_what_fits_beside() -> fairing::Result<()> {
    let mut h = with_solo()?;
    for id in ["solo", "a", "b"] {
        h.shell.launch(LaunchAction::open(id));
        h.frames(3);
        h.shell.home();
        h.frames(3);
    }
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    h.shell.launch(LaunchAction::ToggleSplit);
    h.frames(3);
    assert!(h.shell.workspace().is_overview_picking());
    let a = card_of(&h, "a")?;
    assert_eq!(cards(&h).len(), 1, "only `a` can go beside `b`");
    assert!(card_of(&h, "solo").is_err(), "`solo` never shares");
    h.tap(a.rect.center());
    h.frames(4);
    assert!(h.shell.workspace().is_split());
    Ok(())
}

/// Where no split can come up, the split control says why instead of doing nothing — at home,
/// over a screen that keeps the content to itself, and with nothing to put beside.
#[test]
fn the_split_control_says_why_it_cannot() -> fairing::Result<()> {
    let mut home = with_solo()?;
    home.shell.launch(LaunchAction::ToggleSplit);
    home.frames(3);
    let mut solo = with_solo()?;
    for id in ["a", "solo"] {
        solo.shell.launch(LaunchAction::open(id));
        solo.frames(3);
        solo.shell.home();
        solo.frames(3);
    }
    solo.shell.launch(LaunchAction::open("solo"));
    solo.frames(3);
    solo.shell.launch(LaunchAction::ToggleSplit);
    solo.frames(3);
    let mut lone = with_solo()?;
    lone.shell.launch(LaunchAction::open("a"));
    lone.frames(3);
    lone.shell.launch(LaunchAction::ToggleSplit);
    lone.frames(3);
    let [at_home, on_solo, alone] = [&home, &solo, &lone].map(toasts);
    for (h, words) in [(&home, &at_home), (&solo, &on_solo), (&lone, &alone)] {
        let ws = h.shell.workspace();
        assert!(!ws.is_overview_open() && !ws.is_split(), "{words:?}");
        assert_eq!(words.len(), 1, "one word on it: {words:?}");
    }
    assert!(
        at_home != on_solo && on_solo != alone && at_home != alone,
        "each says its own reason: {at_home:?} {on_solo:?} {alone:?}"
    );
    Ok(())
}

/// A screen opened from outside the cards — a launch, a notification, another thread — is on
/// show: the overview gets out of its way rather than cover it.
#[test]
fn an_open_from_outside_clears_the_overview() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    overview(&mut h);
    assert!(h.shell.workspace().is_overview_open());
    h.shell.launch(LaunchAction::open("c"));
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(!ws.is_overview_open());
    assert_eq!(ws.focused().map(Instance::decl_id), Some("c"));
    Ok(())
}

/// Home over the overview on the desktop takes the cards down.
#[test]
fn home_takes_the_overview_down_over_the_desktop() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    two_tasks(&mut h);
    h.shell.home();
    h.frames(3);
    overview(&mut h);
    assert!(h.shell.workspace().is_overview_open() && h.shell.workspace().is_home());
    h.shell.home();
    h.frames(4);
    assert!(!h.shell.workspace().is_overview_open());
    Ok(())
}

/// Cards going from under the carousel (their tasks ended elsewhere) bring it back to the last
/// card rather than leave it showing nothing.
#[test]
fn the_carousel_comes_back_when_its_cards_go() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    for id in ["a", "b", "c"] {
        h.shell.launch(LaunchAction::open(id));
        h.frames(3);
        h.shell.home();
        h.frames(3);
    }
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    overview(&mut h);
    let content = h.shell.layout().content;
    // To the far end: two swipes to the left.
    for _ in 0..2 {
        let from = content.center();
        h.drag(from, from - egui::vec2(content.width() * 0.4, 0.0), 8);
        h.frames(30);
    }
    let last = cards(&h)
        .into_iter()
        .min_by(|x, y| {
            (x.rect.center().x - content.center().x)
                .abs()
                .total_cmp(&(y.rect.center().x - content.center().x).abs())
        })
        .ok_or_else(|| fail("no card"))?;
    let gone = h
        .shell
        .workspace()
        .tasks()
        .iter()
        .find(|t| t.iter().next().map(Instance::id) == Some(last.key))
        .and_then(|t| t.root_id())
        .map(str::to_owned)
        .ok_or_else(|| fail("no task for the centred card"))?;
    assert_ne!(gone, "a", "the carousel is at its far end");
    h.shell.remove(&gone);
    h.frames(30);
    let centred = cards(&h)
        .iter()
        .any(|c| (c.rect.center().x - content.center().x).abs() < 2.0);
    assert!(centred, "no card in the middle: {:?}", cards(&h));
    Ok(())
}

/// A back swipe over the cards is back: they go down, and the screen behind its card is not
/// popped unseen.
#[test]
fn a_back_swipe_over_the_cards_takes_them_down_and_pops_nothing() -> fairing::Result<()> {
    let mut h = shell(ShellConfig::default())?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
    overview(&mut h);
    let content = h.shell.layout().content;
    let y = content.center().y;
    h.drag(
        egui::pos2(content.min.x + 2.0, y),
        egui::pos2(content.min.x + content.width() * 0.6, y),
        10,
    );
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(!ws.is_overview_open());
    assert_eq!(
        ws.focused().map(Instance::decl_id),
        Some("b"),
        "`b` was popped"
    );
    Ok(())
}

/// The cards follow the session: a drop below `nav.recents` takes them down.
#[test]
fn a_session_drop_takes_the_cards_down() -> fairing::Result<()> {
    let mut h = shell(gated("nav.recents"))?;
    two_tasks(&mut h);
    h.shell.handle().set_subject(fairing::access::Subject {
        level: fairing::Level(1),
        ..fairing::access::Subject::default()
    });
    h.frames(2);
    overview(&mut h);
    assert!(h.shell.workspace().is_overview_open());
    h.shell
        .handle()
        .set_subject(fairing::access::Subject::default());
    h.frames(3);
    assert!(!h.shell.workspace().is_overview_open());
    Ok(())
}

/// A screen that never shares, brought into a split from its card, takes the whole content.
#[test]
fn a_screen_that_never_shares_brought_into_a_split_ends_it() -> fairing::Result<()> {
    let mut h = with_solo()?;
    for id in ["solo", "a"] {
        h.shell.launch(LaunchAction::open(id));
        h.frames(3);
        h.shell.home();
        h.frames(3);
    }
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.launch(LaunchAction::Open {
        id: "b".to_owned(),
        in_other_pane: true,
    });
    h.frames(3);
    assert!(h.shell.workspace().is_split());
    overview(&mut h);
    let solo = swipe_to(&mut h, "solo")?;
    h.tap(solo.rect.center());
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(!ws.is_split(), "`solo` sits in half the content");
    assert_eq!(ws.focused().map(Instance::decl_id), Some("solo"));
    Ok(())
}

/// Everything a screen was told, in order.
type Told = Rc<RefCell<Vec<fairing::Lifecycle>>>;

struct Watch(Told);

impl fairing::Screen for Watch {
    fn ui(&mut self, ui: &mut egui::Ui, _cx: &mut Cx<'_>) {
        ui.label("watch");
    }

    fn on_lifecycle(&mut self, event: fairing::Lifecycle, _cx: &mut Cx<'_>) {
        self.0.borrow_mut().push(event);
    }
}

/// Under the cards a screen stays paused when the shade or the prompt that also covered it goes
/// — here the prompt the recents asked through: it closes and the cards come up in one go.
#[test]
fn a_screen_under_the_cards_stays_paused_when_the_prompt_goes() -> fairing::Result<()> {
    let mut config = fairing::testing::access_config(&["viewer", "operator"], Some("bottom"));
    config
        .access
        .gates
        .insert("nav.recents".to_owned(), "operator".to_owned());
    config
        .access
        .pin_table
        .pins
        .insert("operator".to_owned(), "1234".to_owned());
    let told: Told = Rc::default();
    let t = Rc::clone(&told);
    let mut h = test_shell(config, move |sh| {
        sh.add(fairing::screen_with("watch", move || Watch(Rc::clone(&t))));
    })?;
    h.frames(2);
    h.shell.launch(LaunchAction::open("watch"));
    h.frames(3);
    overview(&mut h);
    assert!(h.shell.unlock_prompt_visible());
    for key in [
        egui::Key::Num1,
        egui::Key::Num2,
        egui::Key::Num3,
        egui::Key::Num4,
    ] {
        h.key(key);
    }
    h.frames(4);
    assert!(h.shell.workspace().is_overview_open() && !h.shell.unlock_prompt_visible());
    assert_eq!(
        told.borrow().last(),
        Some(&fairing::Lifecycle::Paused),
        "resumed under the cards: {:?}",
        told.borrow()
    );
    h.shell.back();
    h.frames(3);
    assert_eq!(told.borrow().last(), Some(&fairing::Lifecycle::Resumed));
    Ok(())
}

/// A shell with the animations on.
fn animated() -> fairing::Result<Harness> {
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::new(ShellConfig::default(), services)?;
    for id in ["a", "b", "c"] {
        h.shell.add(
            screen(id, move |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label(id);
            })
            .title(id.to_uppercase()),
        );
    }
    h.frames(2);
    Ok(h)
}

/// A card thrown away is a decision, not an animation to lose: home pressed while it is still
/// flying ends its task all the same.
#[test]
fn a_card_still_flying_when_the_cards_go_ends_its_task() -> fairing::Result<()> {
    let mut h = animated()?;
    for id in ["a", "b"] {
        h.shell.launch(LaunchAction::open(id));
        h.run_for(0.6);
        h.shell.home();
        h.run_for(0.6);
    }
    h.shell.launch(LaunchAction::open("b"));
    h.run_for(0.6);
    h.shell.launch(LaunchAction::OpenOverview);
    h.run_for(0.6);
    let b = card_of(&h, "b")?;
    let from = b.rect.center();
    h.drag(from, from - egui::vec2(0.0, 300.0), 3);
    h.frames(1);
    assert!(h.shell.workspace().find("b").is_some(), "still flying");
    h.shell.home();
    h.run_for(0.6);
    assert!(
        h.shell.workspace().find("b").is_none(),
        "the throw was lost"
    );
    assert!(h.shell.workspace().find("a").is_some());
    Ok(())
}
