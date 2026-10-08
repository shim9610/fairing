//! M6 — the string table: what a Korean panel draws, the language changing while
//! the shell runs, and a table of the integrator's own.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

#![cfg(all(feature = "settings", feature = "mock", feature = "overlay"))]

use fairing::i18n::Translations;
use fairing::settings::{add_all, keys, SettingValue, SettingsConfig};
use fairing::testing::{access_config, single_level_access, test_shell, Harness};
use fairing::{screen, Cx, LaunchAction, Shell, ShellConfig};

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
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
    texts(h).iter().any(|t| t == wanted)
}

/// Tap the topmost text that reads `wanted`.
fn tap_text(h: &mut Harness, wanted: &str) -> fairing::Result<()> {
    fn walk(shape: &egui::Shape, wanted: &str, found: &mut Option<egui::Rect>) {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                *found = Some(text.galley.rect.translate(text.pos.to_vec2()));
            }
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, wanted, found)),
            _ => {}
        }
    }
    let mut found = None;
    for clipped in h.frame_shapes() {
        walk(&clipped.shape, wanted, &mut found);
    }
    let rect = found.ok_or_else(|| fail(format!("`{wanted}` is not drawn")))?;
    h.tap(rect.center());
    Ok(())
}

/// The built-in settings on the mock backends, in `locale`, with `translations` added.
fn settings_shell(locale: &str, translations: Option<Translations>) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    locale.clone_into(&mut config.shell.locale);
    config.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        let mut builder = Shell::builder(config).services(fairing::services::mock::services());
        if let Some(translations) = translations {
            builder = builder.translations(translations);
        }
        let mut shell = builder.build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        Ok(shell)
    })?;
    h.frames(2);
    Ok(h)
}

/// A Korean panel: the desktop's labels, the shade, the settings list and the recent screens'
/// cards in Korean.
#[test]
fn a_korean_panel_draws_the_built_in_screens_in_korean() -> fairing::Result<()> {
    let mut h = settings_shell("ko", None)?;
    assert!(drawn(&mut h, "설정"), "the settings icon's label");
    // The shade's tiles.
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    for korean in ["블루투스", "다크 모드", "알림 없음"] {
        assert!(drawn(&mut h, korean), "{korean} is not drawn");
    }
    assert!(!drawn(&mut h, "Bluetooth"));
    h.shell.back();
    h.frames(3);
    h.shell.launch(LaunchAction::open("settings.home"));
    h.frames(3);
    for korean in [
        "설정",
        "디스플레이",
        "소리",
        "날짜 및 시간",
        "언어",
        "기기 정보",
    ] {
        assert!(drawn(&mut h, korean), "{korean} is not drawn");
    }
    assert!(!drawn(&mut h, "Display"), "no English left on the list");
    // A recent screen's card is titled the same way.
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
    assert!(h.shell.workspace().is_overview_open());
    assert!(drawn(&mut h, "설정"), "the card's title");
    assert!(!drawn(&mut h, "Settings"));
    Ok(())
}

/// On a phone-sized panel `settings.home` is the list alone, and that list is in Korean too.
#[test]
fn a_narrow_panel_lists_the_settings_in_korean_too() -> fairing::Result<()> {
    let mut h = settings_shell("ko", None)?;
    h.set_size(400.0, 800.0);
    h.frames(2);
    h.shell.launch(LaunchAction::open("settings.home"));
    h.frames(3);
    for korean in ["디스플레이", "소리", "언어"] {
        assert!(drawn(&mut h, korean), "{korean} in {:?}", texts(&mut h));
    }
    assert!(!drawn(&mut h, "Display"));
    Ok(())
}

/// `settings.locale` lists the languages there are and switches on the spot: the next frame is
/// in the new one, with no restart.
#[test]
fn the_language_screen_switches_the_panel_on_the_next_frame() -> fairing::Result<()> {
    let mut h = settings_shell("en", None)?;
    assert!(drawn(&mut h, "Settings"));
    h.shell.launch(LaunchAction::open("settings.locale"));
    h.frames(3);
    assert!(drawn(&mut h, "English") && drawn(&mut h, "한국어"));
    tap_text(&mut h, "한국어")?;
    h.frames(2);
    h.shell.home();
    h.frames(3);
    assert!(drawn(&mut h, "설정"), "the desktop in Korean");
    assert!(!drawn(&mut h, "Settings"));
    Ok(())
}

/// The half of the day goes where the language puts it: before the time in Korean, on the
/// status bar clock as everywhere.
#[test]
fn a_twelve_hour_clock_says_the_half_of_the_day_first_in_korean() -> fairing::Result<()> {
    let mut h = settings_shell("ko", None)?;
    h.shell
        .set_setting(keys::UI_CLOCK_12H.into(), SettingValue::Bool(true));
    h.frames(2);
    let clock = texts(&mut h)
        .into_iter()
        .find(|t| t.starts_with("오전 ") || t.starts_with("오후 "));
    assert!(
        clock.is_some(),
        "no Korean 12-hour clock in {:?}",
        texts(&mut h)
    );
    Ok(())
}

/// The status bar keeps its clock text until the minute turns; a new language writes it again
/// on the next frame rather than a minute later.
#[test]
fn a_language_switch_respells_the_clock_at_once() -> fairing::Result<()> {
    let mut h = settings_shell("en", None)?;
    h.shell
        .set_setting(keys::UI_CLOCK_12H.into(), SettingValue::Bool(true));
    h.frames(2);
    let english = |t: &String| t.ends_with(" AM") || t.ends_with(" PM");
    assert!(texts(&mut h).iter().any(english), "{:?}", texts(&mut h));
    h.shell
        .set_setting(keys::UI_LOCALE.into(), SettingValue::Text("ko".into()));
    h.frames(1);
    let all = texts(&mut h);
    assert!(
        all.iter()
            .any(|t| t.starts_with("오전 ") || t.starts_with("오후 ")),
        "{all:?}"
    );
    assert!(!all.iter().any(english), "{all:?}");
    Ok(())
}

/// The lock screen's date is a template of the language's: `1970년 1월 1일`, the numbers as a
/// Korean date writes them — not `1970년 01월 01일`. Its 12-hour clock puts the half of the day
/// first, as the status bar's does, and `PinTable`'s refusal comes out in Korean.
#[test]
fn the_lock_screen_dates_itself_in_the_language() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "maintainer"], Some("viewer"));
    config.access.pin_table.pins = [("maintainer".to_owned(), "1234".to_owned())]
        .into_iter()
        .collect();
    config.shell.locale = "ko".to_owned();
    let mut h = test_shell(config, |_| {})?;
    h.frames(2);
    h.shell
        .set_setting(keys::UI_CLOCK_12H.into(), SettingValue::Bool(true));
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    assert!(h.shell.lock_screen_visible());
    let all = texts(&mut h);
    assert!(
        all.iter().any(|t| t == "1970년 1월 1일"),
        "the date in {all:?}"
    );
    // Its clock speaks the language too, as the status bar's does.
    assert!(all.iter().any(|t| t == "오전 12:00"), "{all:?}");
    assert!(!all.iter().any(|t| t.ends_with(" AM")), "{all:?}");
    assert!(drawn(&mut h, "PIN 입력"), "the prompt's own words");
    // The authenticator's refusal is a key like any other.
    for _ in 0..4 {
        h.key(egui::Key::Num9);
    }
    h.frames(2);
    assert!(drawn(&mut h, "PIN이 틀렸어요"), "{:?}", texts(&mut h));
    Ok(())
}

/// A table of the integrator's adds a language the crate has none for — the language list names
/// it, the built-in screens take its entries, and so do the integrator's own screens through
/// `cx.strings` and the level labels of its config.
#[test]
fn a_table_of_yours_reaches_the_built_in_screens_and_your_own() -> fairing::Result<()> {
    let german = Translations::new("de", "Deutsch")
        .entry("Settings", "Einstellungen")
        .entry("Pump pressure", "Pumpendruck")
        .entry("only", "Nur");
    let mut h = settings_shell("de", Some(german))?;
    h.shell.add(
        screen("pump", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            ui.label(cx.strings.get("Pump pressure"));
        })
        .title("Pump pressure"),
    );
    h.frames(1);
    h.shell.launch(LaunchAction::open("pump"));
    h.frames(3);
    assert!(
        drawn(&mut h, "Pumpendruck"),
        "your screen, through cx.strings"
    );
    h.shell.launch(LaunchAction::open("settings.locale"));
    h.frames(3);
    assert!(drawn(&mut h, "Deutsch"), "the language list names it");
    h.shell.home();
    h.frames(3);
    assert!(
        drawn(&mut h, "Einstellungen"),
        "a built-in key, from your table"
    );
    // A level's label is a key too: the shade names the session by it.
    h.shell.launch(LaunchAction::OpenOverlay);
    h.frames(3);
    assert!(drawn(&mut h, "Nur"), "{:?}", texts(&mut h));
    h.shell.back();
    h.frames(3);
    // Its other keys have no entry, so they read as written.
    h.shell.launch(LaunchAction::open("settings.home"));
    h.frames(3);
    assert!(drawn(&mut h, "Display"));
    Ok(())
}

/// A slot painter of the integrator's gets the table too, so the labels it draws follow the
/// language as the built-in ones do.
#[test]
fn a_slot_painter_draws_its_labels_in_the_language() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.shell.locale = "ko".to_owned();
    config.motion.reduce = true;
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(config)
            .slot_painter(|ui: &mut egui::Ui, slot: fairing::SlotCx<'_>| {
                ui.painter().text(
                    slot.cell.center(),
                    egui::Align2::CENTER_CENTER,
                    slot.strings.get(&slot.slot.label),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                );
            })
            .build(ctx)?;
        add_all(&mut shell, &SettingsConfig::default());
        Ok(shell)
    })?;
    h.frames(2);
    assert!(drawn(&mut h, "설정"), "{:?}", texts(&mut h));
    Ok(())
}

/// A locale with no table is not an error: the panel reads as English.
#[test]
fn a_locale_with_no_table_reads_as_english() -> fairing::Result<()> {
    let mut h = settings_shell("xx", None)?;
    assert!(drawn(&mut h, "Settings"));
    let mut config = ShellConfig::default();
    config.shell.locale = "ko-KR".to_owned();
    let mut korean = test_shell(config, |sh| add_all(sh, &SettingsConfig::default()))?;
    korean.frames(2);
    assert!(drawn(&mut korean, "설정"), "ko-KR finds ko");
    Ok(())
}
