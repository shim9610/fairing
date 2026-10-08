//! **The shell says so when something the integrator wrote cannot show**: a nav item
//! the bar will never draw, titles that will come out in the regular weight for want of a bold
//! face, and bar sizes the TOML gives that the code overrides. All of them used to fail without
//! a word.

use fairing::chrome::{NavItem, NavStyle};
use fairing::fonts::{FontFamilies, FontPriority};
use fairing::testing::{single_level_access, Harness};
use fairing::{nav_item, FontSource, Shell, ShellConfig};
use std::cell::RefCell;

thread_local! {
    /// What this test's thread logged at `warn` or worse. The logger is process-wide, but every
    /// test runs on its own thread, so each reads only its own lines.
    static SEEN: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

struct Capture;

impl log::Log for Capture {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            SEEN.with(|seen| seen.borrow_mut().push(record.args().to_string()));
        }
    }

    fn flush(&self) {}
}

static CAPTURE: Capture = Capture;

/// The warnings logged while `work` ran on this thread.
fn warnings(work: impl FnOnce() -> fairing::Result<()>) -> fairing::Result<Vec<String>> {
    // Only the first test to get here installs it; the rest find it in place.
    let _ = log::set_logger(&CAPTURE);
    log::set_max_level(log::LevelFilter::Warn);
    SEEN.with(|seen| seen.borrow_mut().clear());
    work()?;
    Ok(SEEN.with(|seen| seen.borrow_mut().drain(..).collect()))
}

/// The bundled bold the examples carry — a real face, so it is not left out as unreadable.
const BOLD: &[u8] = include_bytes!("../../../assets/fonts/NotoSansKR-Bold.ttf");

fn bold() -> FontSource {
    FontSource::from_static("bold", BOLD)
        .families(FontFamilies::Strong)
        .priority(FontPriority::First)
}

/// A shell on `config` with a bold face, so only the nav item can be what is warned about.
fn kbd_shell(config: ShellConfig) -> fairing::Result<Harness> {
    let mut h = Harness::from_builder(move |ctx| Shell::builder(config).font(bold()).build(ctx))?;
    h.shell.add(nav_item("kbd", |ui, _cx| {
        let _ = ui.button("KBD");
    }));
    Ok(h)
}

/// The same, run for a frame: the check is made on the frame after the `add`.
fn shell_with_kbd(config: ShellConfig) -> fairing::Result<Vec<String>> {
    warnings(|| {
        let mut h = kbd_shell(config)?;
        h.frames(2);
        Ok(())
    })
}

fn mentions(lines: &[String], words: &[&str]) -> bool {
    lines
        .iter()
        .any(|line| words.iter().all(|word| line.contains(word)))
}

/// The README's own example: a `nav_item` the default `[nav_bar] items` does not name.
#[test]
fn a_nav_item_the_bar_does_not_list_says_so() -> fairing::Result<()> {
    let seen = shell_with_kbd(single_level_access())?;
    assert!(
        mentions(&seen, &["kbd", "[nav_bar] items"]),
        "no warning for the unlisted nav item: {seen:?}"
    );
    Ok(())
}

/// Listed, it is drawn, and nothing is said.
#[test]
fn a_listed_nav_item_says_nothing() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.nav_bar.items.push("kbd".to_owned());
    let seen = shell_with_kbd(config)?;
    assert!(
        !mentions(&seen, &["kbd"]),
        "a warning for a listed item: {seen:?}"
    );
    Ok(())
}

/// Added twice before a frame has run — one item, so one warning.
#[test]
fn a_nav_item_added_twice_says_so_once() -> fairing::Result<()> {
    let seen = warnings(|| {
        let mut h = kbd_shell(single_level_access())?;
        h.shell.add(nav_item("kbd", |ui, _cx| {
            let _ = ui.button("KBD");
        }));
        h.frames(2);
        Ok(())
    })?;
    let count = seen.iter().filter(|line| line.contains("`kbd`")).count();
    assert_eq!(count, 1, "{seen:?}");
    Ok(())
}

/// Declared first and listed in code afterwards, before a frame has run — the order the
/// repository's own OSK tests use. The item is drawn, so a warning at the `add` would be wrong.
#[test]
fn a_nav_item_listed_in_code_after_its_add_says_nothing() -> fairing::Result<()> {
    let seen = warnings(|| {
        let mut h = kbd_shell(single_level_access())?;
        h.shell.nav_bar_mut().style = NavStyle::Buttons {
            items: vec![
                NavItem::Back,
                NavItem::Home,
                NavItem::Custom("kbd".to_owned()),
            ],
        };
        h.frames(2);
        assert!(
            h.shell.nav_bar().item_rect_by_id("kbd").is_some(),
            "the item listed in code is not drawn"
        );
        Ok(())
    })?;
    assert!(
        !mentions(&seen, &["kbd"]),
        "a warning for an item listed in code: {seen:?}"
    );
    Ok(())
}

/// Taken out again before a frame has run: there is nothing left to warn about.
#[test]
fn a_nav_item_removed_before_the_frame_says_nothing() -> fairing::Result<()> {
    let seen = warnings(|| {
        let mut h = kbd_shell(single_level_access())?;
        assert!(h.shell.remove("kbd"));
        h.frames(2);
        Ok(())
    })?;
    assert!(
        !mentions(&seen, &["kbd"]),
        "a warning for an item that is gone: {seen:?}"
    );
    Ok(())
}

/// The gesture-style bar draws no items at all, listed or not.
#[test]
fn a_nav_item_on_the_gesture_bar_says_so() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.nav_bar.style = "gesture".to_owned();
    config.nav_bar.items.push("kbd".to_owned());
    let seen = shell_with_kbd(config)?;
    assert!(
        mentions(&seen, &["kbd", "gesture"]),
        "no warning for a nav item on the gesture bar: {seen:?}"
    );
    Ok(())
}

/// And neither does a bar that is off.
#[test]
fn a_nav_item_on_a_bar_that_is_off_says_so() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.nav_bar.enabled = false;
    config.nav_bar.items.push("kbd".to_owned());
    let seen = shell_with_kbd(config)?;
    assert!(
        mentions(&seen, &["kbd", "enabled = false"]),
        "no warning for a nav item on a bar that is off: {seen:?}"
    );
    Ok(())
}

/// No bold face: the titles will be flat, and the shell says how to fix it.
#[test]
fn no_bold_face_says_so() -> fairing::Result<()> {
    let seen = warnings(|| {
        let _ = Harness::from_builder(|ctx| Shell::builder(single_level_access()).build(ctx))?;
        Ok(())
    })?;
    assert!(
        mentions(&seen, &["no bold face", "FontFamilies::Strong"]),
        "no warning without a bold face: {seen:?}"
    );
    Ok(())
}

/// A bold face, and nothing is said about fonts.
#[test]
fn a_bold_face_says_nothing() -> fairing::Result<()> {
    let seen = warnings(|| {
        let _ = Harness::from_builder(|ctx| {
            Shell::builder(single_level_access())
                .font(bold())
                .build(ctx)
        })?;
        Ok(())
    })?;
    assert!(
        !mentions(&seen, &["bold"]),
        "a font warning with a bold face: {seen:?}"
    );
    Ok(())
}

/// A bold that cannot be read is left out — and so it is no bold at all.
#[test]
fn an_unreadable_bold_still_says_so() -> fairing::Result<()> {
    let seen = warnings(|| {
        let broken = FontSource::from_static("bold", b"not a font").families(FontFamilies::Strong);
        let _ = Harness::from_builder(move |ctx| {
            Shell::builder(single_level_access())
                .font(broken)
                .build(ctx)
        })?;
        Ok(())
    })?;
    assert!(
        mentions(&seen, &["no bold face"]),
        "an unreadable bold counted as a bold: {seen:?}"
    );
    Ok(())
}

/// The TOML pins a bar height and the code sets the metrics too: the code wins, and
/// the shell says the TOML was ignored rather than dropping it silently.
#[test]
fn a_bar_size_the_code_overrides_says_so() -> fairing::Result<()> {
    let seen = warnings(|| {
        let mut config = single_level_access();
        config.status_bar.height = Some(48.0);
        config.nav_bar.height = Some(80.0);
        let _ = Harness::from_builder(move |ctx| {
            Shell::builder(config)
                .font(bold())
                .metrics_spec(fairing::theme::MetricsSpec::legacy_du())
                .build(ctx)
        })?;
        Ok(())
    })?;
    assert!(
        mentions(
            &seen,
            &["[status_bar] height", "[nav_bar] height", "ignored"]
        ),
        "no warning for the overridden TOML sizes: {seen:?}"
    );
    Ok(())
}

/// The same with a whole theme injected: its metrics win, and the TOML size is said to be ignored.
#[test]
fn a_bar_size_an_injected_theme_overrides_says_so() -> fairing::Result<()> {
    let seen = warnings(|| {
        let mut config = single_level_access();
        config.status_bar.icon_size = Some(26.0);
        let _ = Harness::from_builder(move |ctx| {
            Shell::builder(config)
                .font(bold())
                .theme(fairing::Theme::dark())
                .build(ctx)
        })?;
        Ok(())
    })?;
    assert!(
        mentions(&seen, &["[status_bar] icon_size", "ignored", "Theme"]),
        "no warning for the TOML size the theme overrides: {seen:?}"
    );
    Ok(())
}

/// Pinned and not overridden: they take effect, and nothing is said.
#[test]
fn a_bar_size_that_takes_effect_says_nothing() -> fairing::Result<()> {
    let seen = warnings(|| {
        let mut config = single_level_access();
        config.status_bar.height = Some(48.0);
        let _ = Harness::from_builder(move |ctx| Shell::builder(config).font(bold()).build(ctx))?;
        Ok(())
    })?;
    assert!(
        !mentions(&seen, &["ignored"]),
        "a warning for a size that applies: {seen:?}"
    );
    Ok(())
}
