//! M6 — the info popover a long press brings up over a desktop icon.
//!
//! What it checks: the hold brings it up and its release opens nothing; it says the title, the
//! level the icon needs and the description; any press puts it away and does nothing else; back,
//! the shade and a removed declaration take it away too; it stands over the icon or hangs below it,
//! inside the screen; `long_press = "none"` leaves only the event; the rail has it as the grid does.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::icons::IconRef;
use fairing::testing::{access_config, single_level_access, Harness};
use fairing::{action, screen, Cx, ShellConfig, ShellEvent};

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fail(format!("{what} is missing")))
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

fn drawn(h: &mut Harness, wanted: &str) -> bool {
    texts(h).iter().any(|t| t.contains(wanted))
}

/// A desktop of six icons — the first one, `pumps`, with a description — on `config` (motion
/// reduced unless the test says otherwise).
fn desktop(mut config: ShellConfig, reduce: bool) -> fairing::Result<Harness> {
    config.motion.reduce = reduce;
    let mut h = Harness::new(config, fairing::services::Services::null())?;
    h.shell.add(
        screen("pumps", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("pumps body");
        })
        .title("Pumps")
        .description("Starts and stops the pumps.")
        .icon(IconRef::Builtin("settings"))
        .desktop(),
    );
    for id in ["tanks", "valves", "alarms", "trends"] {
        h.shell.add(
            screen(id, |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("body");
            })
            .title(id)
            .icon(IconRef::Builtin("grid"))
            .desktop(),
        );
    }
    h.shell.add(
        action("restart", |_: &mut Cx<'_>| {})
            .title("Restart")
            .description("Restarts the line.")
            .icon(IconRef::Builtin("power"))
            .desktop(),
    );
    h.frames(3);
    Ok(h)
}

/// Where a finger holds `id`: the middle of its icon.
fn icon_at(h: &Harness, id: &str) -> fairing::Result<egui::Pos2> {
    Ok(need(h.shell.desktop().icon_rect(id), id)?.center())
}

/// Hold `id` past the long press (500 ms; 40 frames are 667 ms) — the finger stays down.
fn hold_icon(h: &mut Harness, id: &str) -> fairing::Result<egui::Pos2> {
    let at = icon_at(h, id)?;
    h.hold(at, 40);
    Ok(at)
}

/// Hold `id` and let go.
fn long_press(h: &mut Harness, id: &str) -> fairing::Result<()> {
    let at = hold_icon(h, id)?;
    h.release(at);
    h.frames(2);
    Ok(())
}

fn opened(events: &[ShellEvent]) -> bool {
    events
        .iter()
        .any(|e| matches!(e, ShellEvent::ScreenOpened { .. }))
}

/// **A finger held still on an icon brings up what it is** — the title and the description — and
/// the long press goes out as an event too. The harness sends the pointer, not touch events: the
/// desktop's long press is the gesture engine's, so it no longer waits for an `Event::Touch` that
/// a panel delivering touch as the mouse never sends.
#[test]
fn holding_an_icon_brings_up_what_it_is() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    let _ = h.shell.poll_events();
    let _ = hold_icon(&mut h, "pumps")?;
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"));
    assert!(
        !h.shell.desktop().is_pressed(),
        "the held icon lets go of its press look once the popover is up"
    );
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::IconLongPressed { id } if id == "pumps")),
        "{events:?}"
    );
    assert!(drawn(&mut h, "Pumps"));
    assert!(drawn(&mut h, "Starts and stops the pumps."));
    // One level: there is no level to speak of.
    assert!(!drawn(&mut h, "Required level"));
    Ok(())
}

/// **The release of the hold opens nothing** — not the icon held, with the popover staying up.
#[test]
fn letting_go_of_the_hold_opens_nothing() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    let _ = h.shell.poll_events();
    long_press(&mut h, "pumps")?;
    let events = h.shell.poll_events();
    assert!(!opened(&events), "the release opened the icon: {events:?}");
    assert!(h.shell.workspace().is_home());
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"), "it stays up");
    Ok(())
}

/// **The next press puts it away, and does nothing else** — the icon under it does not open. The
/// press after that is an ordinary tap again.
#[test]
fn the_next_press_puts_it_away_and_does_nothing_else() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    let _ = h.shell.poll_events();
    let tanks = icon_at(&h, "tanks")?;
    h.tap(tanks);
    assert_eq!(h.shell.desktop().info_icon(), None, "the press put it away");
    let events = h.shell.poll_events();
    assert!(
        !opened(&events),
        "the press opened what it landed on: {events:?}"
    );
    assert!(!drawn(&mut h, "Starts and stops the pumps."));
    h.tap(tanks);
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::ScreenOpened { id, .. } if id == "tanks")),
        "the next tap is a tap: {events:?}"
    );
    Ok(())
}

/// A press that puts it away does not start the long press over: held, it brings nothing back up.
#[test]
fn the_press_that_puts_it_away_is_spent_whole() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    let _ = h.shell.poll_events();
    let _ = hold_icon(&mut h, "tanks")?;
    assert_eq!(h.shell.desktop().info_icon(), None);
    let events = h.shell.poll_events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::IconLongPressed { .. })),
        "{events:?}"
    );
    Ok(())
}

/// **A press on a bar only puts it away** — a status bar tap that would open the shade does not.
#[test]
fn a_press_on_a_bar_only_puts_it_away() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    let bar = need(h.shell.layout().status, "the status bar")?;
    h.tap(bar.center());
    h.frames(2);
    assert_eq!(h.shell.desktop().info_icon(), None);
    #[cfg(feature = "overlay")]
    assert!(h.shell.overlay().is_closed(), "the tap reached the bar");
    Ok(())
}

/// **Back puts it away first** — nothing under it moves. Home does too.
#[test]
fn back_and_home_put_it_away() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    h.shell.back();
    h.frame();
    assert_eq!(h.shell.desktop().info_icon(), None);
    long_press(&mut h, "pumps")?;
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"));
    h.shell.home();
    h.frame();
    assert_eq!(h.shell.desktop().info_icon(), None);
    Ok(())
}

/// A screen opening over the desktop takes the popover with it — here at once, with motion
/// reduced, where no transition runs to say so.
#[test]
fn a_screen_opening_takes_it_away() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    h.shell.launch(fairing::LaunchAction::open("tanks"));
    h.frames(2);
    assert!(!h.shell.workspace().is_home());
    assert_eq!(h.shell.desktop().info_icon(), None);
    assert!(!drawn(&mut h, "Starts and stops the pumps."));
    Ok(())
}

/// **The press that puts it away turns no page** — dragged across the grid, it is still spent.
#[test]
fn the_press_that_puts_it_away_turns_no_page() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    // Enough icons for a second page.
    for n in 7..=16 {
        h.shell.add(
            screen(format!("more{n}"), |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("body");
            })
            .icon(IconRef::Builtin("grid"))
            .desktop(),
        );
    }
    h.frames(3);
    assert!(h.shell.desktop().pages().len() > 1);
    long_press(&mut h, "pumps")?;
    let content = h.shell.layout().content;
    let from = egui::pos2(content.right() - 60.0, content.center().y);
    let to = egui::pos2(content.left() + 60.0, content.center().y);
    h.drag(from, to, 12);
    h.frames(30);
    assert_eq!(h.shell.desktop().info_icon(), None);
    assert_eq!(
        h.shell.desktop().page(),
        0,
        "the spent press turned the page"
    );
    // The next drag is a drag.
    h.drag(from, to, 12);
    h.frames(30);
    assert_eq!(h.shell.desktop().page(), 1);
    Ok(())
}

/// **What takes the desktop's place takes the popover with it** — the shade here.
#[cfg(feature = "overlay")]
#[test]
fn the_shade_takes_it_away() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    h.shell.launch(fairing::LaunchAction::OpenOverlay);
    h.frames(3);
    assert!(!h.shell.overlay().is_closed(), "the shade opened");
    assert_eq!(h.shell.desktop().info_icon(), None);
    Ok(())
}

/// A declaration removed takes its popover with it at once.
#[test]
fn a_removed_icon_takes_its_popover() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    long_press(&mut h, "pumps")?;
    assert!(h.shell.remove("pumps"));
    h.frames(2);
    assert_eq!(h.shell.desktop().info_icon(), None);
    assert!(!drawn(&mut h, "Starts and stops the pumps."));
    Ok(())
}

/// Two levels, the session at the lower one: `pumps` needs maintenance, everything else the lowest.
fn two_levels() -> ShellConfig {
    let mut config = access_config(&["operator", "maintenance"], Some("operator"));
    config
        .access
        .gates
        .insert("pumps".to_owned(), "maintenance".to_owned());
    config
}

/// **A locked icon says the level it needs** — and its long press goes out like any other.
#[test]
fn a_locked_icon_says_the_level_it_needs() -> fairing::Result<()> {
    let mut h = desktop(two_levels(), true)?;
    let _ = h.shell.poll_events();
    long_press(&mut h, "pumps")?;
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"));
    assert!(drawn(&mut h, "Required level · maintenance"));
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::IconLongPressed { id } if id == "pumps")),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, ShellEvent::Access(_))),
        "a hold is not a tap: it asks for no unlock: {events:?}"
    );
    Ok(())
}

/// **An icon anyone may open names no level** — everyone has the lowest, so saying so says
/// nothing.
#[test]
fn an_icon_anyone_may_open_names_no_level() -> fairing::Result<()> {
    let mut h = desktop(two_levels(), true)?;
    long_press(&mut h, "tanks")?;
    assert_eq!(h.shell.desktop().info_icon(), Some("tanks"));
    assert!(!drawn(&mut h, "Required level"));
    Ok(())
}

/// **With the gates off no level is named** — `mode = "off"` lets everyone through, so a level
/// would only mislead.
#[test]
fn with_the_gates_off_no_level_is_named() -> fairing::Result<()> {
    let mut config = two_levels();
    "off".clone_into(&mut config.access.mode);
    let mut h = desktop(config, true)?;
    long_press(&mut h, "pumps")?;
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"));
    assert!(!drawn(&mut h, "Required level"));
    Ok(())
}

/// A hold where the shade lies over an icon is the shade's: the desktop under it raises nothing.
#[cfg(feature = "overlay")]
#[test]
fn a_hold_under_the_shade_is_the_shades() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    let at = icon_at(&h, "pumps")?;
    h.shell.launch(fairing::LaunchAction::OpenOverlay);
    h.frames(3);
    assert!(!h.shell.overlay().is_closed());
    let _ = h.shell.poll_events();
    h.hold(at, 40);
    h.release(at);
    h.frames(2);
    let events = h.shell.poll_events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::IconLongPressed { .. })),
        "{events:?}"
    );
    assert_eq!(h.shell.desktop().info_icon(), None);
    Ok(())
}

/// **It follows the language** — the words are keys like everything else the shell draws.
#[test]
fn it_follows_the_language() -> fairing::Result<()> {
    let mut config = two_levels();
    "ko".clone_into(&mut config.shell.locale);
    let mut h = desktop(config, true)?;
    long_press(&mut h, "pumps")?;
    assert!(drawn(&mut h, "필요 단계 · maintenance"));
    Ok(())
}

/// **It stands over the icon** and points at it; on the top row, where there is no room above,
/// it hangs below — and either way it keeps inside the space between the bars.
#[test]
fn it_stands_over_the_icon_or_hangs_below() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    let content = h.shell.layout().content;
    let inset = h.shell.theme().metrics.screen_inset;
    // The first row is the top of the grid: no room above.
    long_press(&mut h, "pumps")?;
    let icon = need(h.shell.desktop().icon_rect("pumps"), "pumps")?;
    let card = need(h.shell.desktop().info_rect(), "the card")?;
    assert!(
        card.top() >= icon.bottom(),
        "hangs below: {card:?} vs {icon:?}"
    );
    assert!(
        card.left() >= content.left() + inset - 0.5,
        "{card:?} in {content:?}"
    );
    assert!(card.right() <= content.right() - inset + 0.5);
    assert!(card.bottom() <= content.bottom() - inset + 0.5);
    h.tap(content.center());

    // Fill the grid: the twelfth icon is on the bottom row, with no room below it.
    for n in 7..=12 {
        h.shell.add(
            screen(format!("more{n}"), |ui: &mut egui::Ui, _: &mut Cx<'_>| {
                ui.label("body");
            })
            .description("One of six more.")
            .icon(IconRef::Builtin("grid"))
            .desktop(),
        );
    }
    h.frames(3);
    long_press(&mut h, "more12")?;
    let icon = need(h.shell.desktop().icon_rect("more12"), "more12")?;
    let card = need(h.shell.desktop().info_rect(), "the card")?;
    assert_eq!(h.shell.desktop().info_icon(), Some("more12"));
    assert!(
        card.bottom() <= icon.top(),
        "stands over: {card:?} vs {icon:?}"
    );
    assert!(card.top() >= content.top() + inset - 0.5);
    // Centred on the icon, where it fits.
    assert!(
        (card.center().x - icon.center().x).abs() < 1.0
            || card.left() <= content.left() + inset + 0.5
            || card.right() >= content.right() - inset - 0.5,
        "{card:?} vs {icon:?}"
    );
    Ok(())
}

/// **`long_press = "none"`** leaves the press to the integrator: the event, no popover, and still
/// no launch on the release.
#[test]
fn long_press_none_leaves_only_the_event() -> fairing::Result<()> {
    let mut config = single_level_access();
    "none".clone_into(&mut config.desktop.long_press);
    let mut h = desktop(config, true)?;
    let _ = h.shell.poll_events();
    long_press(&mut h, "pumps")?;
    assert_eq!(h.shell.desktop().info_icon(), None);
    assert!(!drawn(&mut h, "Starts and stops the pumps."));
    let events = h.shell.poll_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ShellEvent::IconLongPressed { id } if id == "pumps")),
        "{events:?}"
    );
    assert!(!opened(&events), "{events:?}");
    Ok(())
}

/// A hold on an open screen is the screen's: the desktop behind it raises nothing.
#[test]
fn a_hold_on_an_open_screen_is_not_the_desktops() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), true)?;
    let at = icon_at(&h, "tanks")?;
    h.tap(at);
    h.frames(2);
    assert!(!h.shell.workspace().is_home());
    let _ = h.shell.poll_events();
    h.hold(at, 40);
    h.release(at);
    h.frames(2);
    assert_eq!(h.shell.desktop().info_icon(), None);
    let events = h.shell.poll_events();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ShellEvent::IconLongPressed { .. })),
        "{events:?}"
    );
    Ok(())
}

/// **A description can come from the config**, like a label.
#[test]
fn the_config_can_give_the_description() -> fairing::Result<()> {
    let config = ShellConfig::from_toml(
        r#"
        [access]
        levels = ["only"]

        [[desktop.pages]]
        icons = [{ id = "tanks", description = "Levels in the four tanks." }]
        "#,
    )?;
    let mut h = desktop(config, true)?;
    long_press(&mut h, "tanks")?;
    assert!(drawn(&mut h, "Levels in the four tanks."));
    Ok(())
}

/// **The rail's entries have it too** — the kiosk layout's icons are the desktop's — and the
/// hold's release opens nothing there either.
#[test]
fn a_rail_entry_has_it_too() -> fairing::Result<()> {
    let mut config = single_level_access();
    "left".clone_into(&mut config.desktop.rail);
    let mut h = desktop(config, true)?;
    let rail = need(h.shell.layout().rail, "the rail")?;
    let content = h.shell.layout().content;
    // The rail opens its first entry, `pumps`, by itself; `tanks` is the second, and not open —
    // a release that tapped it would open it. Down the rail until a hold lands on it.
    let step = h.shell.theme().metrics.touch_target * 0.5;
    let mut y = rail.top() + step;
    let mut found = None;
    while y < rail.bottom() && found.is_none() {
        let at = egui::pos2(rail.center().x, y);
        let _ = h.shell.poll_events();
        h.hold(at, 40);
        h.release(at);
        h.frames(2);
        match h.shell.desktop().info_icon() {
            Some("tanks") => found = Some(at),
            Some(_) => h.tap(content.center()),
            None => {}
        }
        y += step;
    }
    need(found, "a hold on `tanks`")?;
    let events = h.shell.poll_events();
    assert!(!opened(&events), "the release opened the entry: {events:?}");
    let card = need(h.shell.desktop().info_rect(), "the card")?;
    let screen = h.screen_rect();
    assert!(screen.contains_rect(card), "{card:?}");
    Ok(())
}

/// **It comes in and goes out** over the toast tweens — and while it does, the shell asks for the
/// frames; on its way out it is still drawn, but no longer up.
#[test]
fn it_comes_in_and_goes_out() -> fairing::Result<()> {
    let mut h = desktop(single_level_access(), false)?;
    h.frames(60);
    // The long press comes on the 30th frame of the hold (500 ms); one frame on, it is coming in.
    let at = icon_at(&h, "pumps")?;
    h.hold(at, 31);
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"));
    assert!(h.shell.is_animating(), "it is coming in");
    h.release(at);
    h.frames(40);
    assert!(!h.shell.is_animating(), "it has come in");
    assert_eq!(h.shell.desktop().info_icon(), Some("pumps"));
    let tanks = icon_at(&h, "tanks")?;
    h.press(tanks);
    h.frame();
    assert_eq!(h.shell.desktop().info_icon(), None, "no longer up");
    assert!(h.shell.is_animating(), "it is going out");
    assert!(
        drawn(&mut h, "Starts and stops the pumps."),
        "still drawn as it goes"
    );
    h.release(tanks);
    h.frames(40);
    assert!(!drawn(&mut h, "Starts and stops the pumps."), "gone");
    assert!(h.shell.workspace().is_home(), "and nothing opened");
    Ok(())
}
