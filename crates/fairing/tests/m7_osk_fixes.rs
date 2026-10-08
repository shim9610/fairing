//! **The on-screen keyboard keeps what was typed and stays usable** — a Hangul composition never
//! erases text it did not put there, the keyboard fits a short panel, its keys are as tall as
//! `min_key_px` says, the `한/영` key survives a numpad detour, ⇧ locks only on a double tap, and a
//! tap on empty space after back leaves it closed. Plus the layout facts the guide states.
#![cfg(feature = "osk")]

use fairing::osk::{Composer, HangulComposer, KeyAction, KeyDef, OskLayout};
use fairing::testing::{single_level_access, Harness};
use fairing::{screen, Cx, LaunchAction, Services};
use std::cell::RefCell;
use std::rc::Rc;

fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("missing: {what}")))
}

fn tap(h: &mut Harness, pos: egui::Pos2) {
    h.press(pos);
    h.frame();
    h.release(pos);
    h.frame();
    h.frame();
}

fn tap_key(h: &mut Harness, label: &str) -> fairing::Result<()> {
    let key = need(h.shell.osk().key_rect(label), label)?;
    tap(h, key.center());
    Ok(())
}

#[derive(Default)]
struct Field {
    text: String,
    rect: Option<egui::Rect>,
}

/// One screen with one plain `egui::TextEdit`, optionally with a `char_limit`.
fn one_field(
    initial: &str,
    limit: Option<usize>,
) -> fairing::Result<(Harness, Rc<RefCell<Field>>)> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    let mut h = Harness::new(config, Services::null())?;
    let field = Rc::new(RefCell::new(Field {
        text: initial.to_owned(),
        rect: None,
    }));
    let seen = Rc::clone(&field);
    h.shell.add(screen(
        "form",
        move |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            let state = &mut *seen.borrow_mut();
            let mut edit = egui::TextEdit::singleline(&mut state.text).id_salt("a");
            if let Some(limit) = limit {
                edit = edit.char_limit(limit);
            }
            let r = ui.add_sized([300.0, 48.0], edit);
            state.rect = Some(r.rect);
        },
    ));
    h.shell.handle().launch(LaunchAction::open("form"));
    h.run_for(0.5);
    Ok((h, field))
}

/// Tap the field, put the caret at the end, and check the keyboard came up.
fn focus(h: &mut Harness, field: &Rc<RefCell<Field>>) -> fairing::Result<()> {
    let rect = need(field.borrow().rect, "field rect")?;
    tap(h, rect.center());
    h.key(egui::Key::End);
    h.frame();
    if !h.shell.osk().is_visible() {
        return Err(fairing::Error::Config(
            "the keyboard did not come up".into(),
        ));
    }
    Ok(())
}

/// A keyboard brought up by a focused field on a screen of the given size.
fn keyboard_up(width: f32, height: f32, min_key_px: Option<f32>) -> fairing::Result<Harness> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    if let Some(px) = min_key_px {
        config.osk.min_key_px = px;
    }
    let mut h = Harness::new(config, Services::null())?.with_size(width, height);
    let mut text = String::new();
    let mut focused = false;
    h.shell.add(screen(
        "form",
        move |ui: &mut egui::Ui, _cx: &mut Cx<'_>| {
            let r = ui.add(egui::TextEdit::singleline(&mut text));
            if !focused {
                r.request_focus();
                focused = true;
            }
        },
    ));
    h.shell.launch(LaunchAction::open("form"));
    h.run_for(1.0);
    if !h.shell.osk().is_shown() {
        return Err(fairing::Error::Config(
            "the keyboard did not come up".into(),
        ));
    }
    Ok(h)
}

// ------------------------------------------------------------------ composition

/// Guards that a jamo which cannot start a syllable (`ㄳ`) fed to an empty composer is handed
/// back to be typed as it is, not swallowed.
#[test]
fn the_composer_hands_back_a_jamo_that_cannot_start_a_syllable() {
    let mut ime = HangulComposer::new();
    let out = ime.feed("ㄳ");
    assert!(
        !out.consumed && out.commit.is_empty() && out.preedit.is_empty(),
        "ㄳ on an empty composer: {out:?}"
    );
    // The same jamo after a lone vowel is handed back too.
    let mut ime = HangulComposer::new();
    let _ = ime.feed("ㅏ");
    assert!(!ime.feed("ㄳ").consumed);
}

/// Guards that text from a hardware keyboard or scanner typed after a composing syllable is not
/// erased by the next jamo's erase-and-retype.
#[test]
fn hardware_text_typed_mid_composition_is_kept() -> fairing::Result<()> {
    let (mut h, field) = one_field("", None)?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    focus(&mut h, &field)?;
    tap_key(&mut h, "ㄱ")?;
    tap_key(&mut h, "ㅏ")?;
    assert_eq!(field.borrow().text, "가");
    h.type_text("x");
    h.frames(2);
    assert_eq!(field.borrow().text, "가x");
    tap_key(&mut h, "ㄴ")?;
    assert_eq!(
        field.borrow().text,
        "가xㄴ",
        "the composition ended at the x"
    );
    // And a new composition goes on normally from there.
    tap_key(&mut h, "ㅏ")?;
    assert_eq!(field.borrow().text, "가x나");
    Ok(())
}

/// Guards that a full field (`char_limit`) keeps its committed text: the jamo it refused is not
/// counted as in the buffer.
#[test]
fn a_full_field_keeps_its_text_under_composition() -> fairing::Result<()> {
    let (mut h, field) = one_field("abc", Some(3))?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    focus(&mut h, &field)?;
    tap_key(&mut h, "ㄱ")?;
    assert_eq!(field.borrow().text, "abc", "the jamo does not fit");
    tap_key(&mut h, "ㅏ")?;
    assert_eq!(field.borrow().text, "abc");
    tap_key(&mut h, "ㄴ")?;
    assert_eq!(field.borrow().text, "abc");
    Ok(())
}

/// Composition on a field with room still stacks jamo into syllables (the buffer check above
/// does not break the ordinary case).
#[test]
fn composition_with_room_still_stacks_syllables() -> fairing::Result<()> {
    let (mut h, field) = one_field("", Some(10))?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    focus(&mut h, &field)?;
    for jamo in ["ㅎ", "ㅏ", "ㄴ", "ㄱ", "ㅡ", "ㄹ"] {
        tap_key(&mut h, jamo)?;
    }
    assert_eq!(field.borrow().text, "한글");
    tap_key(&mut h, "⌫")?;
    assert_eq!(field.borrow().text, "한그", "⌫ takes back one jamo");
    Ok(())
}

// ------------------------------------------------------------------ 한/영 pairing

/// Guards that the `한/영` key comes back after a screen borrows the numpad and restores the
/// layout it found.
#[test]
fn the_lang_key_survives_a_numpad_detour() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.osk.layout = "hangul".into();
    let mut h = Harness::new(config, Services::null())?;
    h.frames(2);
    h.shell.osk_mut().toggle_lang();
    assert_eq!(h.shell.osk().layout(), &OskLayout::Qwerty);
    assert!(h.shell.osk().has_lang_key());
    let found = h.shell.osk().layout().clone();
    h.shell.osk_mut().set_layout(OskLayout::NumPad {
        decimal: true,
        sign: false,
    });
    assert!(!h.shell.osk().has_lang_key(), "the numpad has no 한/영 key");
    h.shell.osk_mut().set_layout(found);
    assert!(
        h.shell.osk().has_lang_key(),
        "after a numpad detour the qwerty has no 한/영 key"
    );
    Ok(())
}

/// A keyboard that started on qwerty still gets no `한/영` key from a numpad detour.
#[test]
fn a_qwerty_keyboard_stays_single_language() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    h.shell.osk_mut().set_layout(OskLayout::NumPad {
        decimal: true,
        sign: false,
    });
    h.shell.osk_mut().set_layout(OskLayout::Qwerty);
    assert!(!h.shell.osk().has_lang_key());
    Ok(())
}

// ------------------------------------------------------------------ ⇧

/// Guards that ⇧, a letter, ⇧ (typing "HI" quickly) is two one-shot capitals, not a caps lock.
#[test]
fn shift_letter_shift_is_not_a_double_tap() -> fairing::Result<()> {
    let (mut h, field) = one_field("", None)?;
    focus(&mut h, &field)?;
    tap_key(&mut h, "⇧")?;
    tap_key(&mut h, "H")?;
    assert_eq!(h.shell.osk().face(), 0, "back to lowercase after H");
    tap_key(&mut h, "⇧")?;
    tap_key(&mut h, "I")?;
    assert!(!h.shell.osk().shift_locked(), "⇧, a letter, ⇧ locked caps");
    tap_key(&mut h, "q")?;
    assert_eq!(field.borrow().text, "HIq");
    Ok(())
}

// ------------------------------------------------------------------ size and placement

/// Guards that on a panel too short for the `min_key_px` floor the keyboard stays on the screen
/// above the nav bar, its top row reachable.
#[test]
fn the_keyboard_stays_on_a_short_screen() -> fairing::Result<()> {
    let h = keyboard_up(480.0, 220.0, None)?;
    let osk = need(h.shell.layout().osk, "Layout.osk")?;
    let q = need(h.shell.osk().key_rect("q"), "key q")?;
    assert!(
        osk.min.y >= -0.01 && q.min.y >= -0.01,
        "the keyboard runs off the top of a 220 px screen: osk={osk:?} q={q:?}"
    );
    if let Some(nav) = h.shell.layout().nav {
        assert!(osk.max.y <= nav.min.y + 0.01, "it sits above the nav bar");
    }
    Ok(())
}

/// Guards that `min_key_px` is the height of a key, the gaps not taken out of it.
#[test]
fn min_key_px_is_a_key_height() -> fairing::Result<()> {
    let h = keyboard_up(1024.0, 1000.0, Some(100.0))?;
    let q = need(h.shell.osk().key_rect("q"), "key q")?;
    assert!(
        (q.height() - 100.0).abs() < 0.5,
        "min_key_px = 100 but the key is {} tall",
        q.height()
    );
    Ok(())
}

// ------------------------------------------------------------------ layouts as documented

/// The numpad's sign key is a `-` that types a minus sign (03 §6.2, 07 §8, the rustdoc).
#[test]
fn the_numpad_sign_key_types_a_minus() {
    let layout = OskLayout::NumPad {
        decimal: true,
        sign: true,
    }
    .build();
    let sign = layout
        .faces
        .iter()
        .flat_map(|f| f.rows.iter())
        .flat_map(|r| r.keys.iter())
        .find(|k| k.label == "-");
    assert_eq!(
        sign.map(|k| &k.action),
        Some(&KeyAction::Text("-".into())),
        "no `-` key on the numpad with sign = true"
    );
    let unsigned = OskLayout::NumPad {
        decimal: true,
        sign: false,
    }
    .build();
    assert!(!unsigned
        .faces
        .iter()
        .flat_map(|f| f.rows.iter())
        .flat_map(|r| r.keys.iter())
        .any(|k| k.label == "-"));
}

/// A special key's span is held to `0.5..=6` and rounded to a tenth, as documented.
#[test]
fn a_special_key_span_is_held_to_its_documented_bounds() {
    let span = |s: f32| KeyDef::special(KeyAction::Space, " ", s).span();
    assert!((span(2.5) - 2.5).abs() < 1e-6);
    assert!((span(8.0) - 6.0).abs() < 1e-6, "wider than six is six");
    assert!((span(0.1) - 0.5).abs() < 1e-6, "narrower than half is half");
    assert!((span(1.04) - 1.0).abs() < 1e-6, "rounded to a tenth");
}

// ------------------------------------------------------------------ close request

/// Guards that after a programmatic back, a tap on empty space does not briefly reopen the
/// keyboard (the field still had focus on the release frame).
#[test]
fn a_tap_on_empty_space_after_back_does_not_reopen() -> fairing::Result<()> {
    let (mut h, field) = one_field("", None)?;
    focus(&mut h, &field)?;
    h.shell.back();
    h.frames(20);
    assert!(!h.shell.osk().is_shown(), "back did not close it");
    assert!(
        h.ctx.memory(egui::Memory::focused).is_some(),
        "precondition: the field keeps its focus after a programmatic back"
    );
    let rect = need(field.borrow().rect, "field rect")?;
    let empty = rect.center() + egui::vec2(0.0, 150.0);
    h.press(empty);
    h.frame();
    let mut reopened = h.shell.osk().is_shown();
    h.release(empty);
    for _ in 0..30 {
        h.frame();
        reopened |= h.shell.osk().is_shown();
    }
    assert!(!reopened, "a tap on empty space after back reopened it");
    Ok(())
}

/// After back, re-tapping the same field still reopens the keyboard.
#[test]
fn re_tapping_the_field_after_back_reopens() -> fairing::Result<()> {
    let (mut h, field) = one_field("", None)?;
    focus(&mut h, &field)?;
    h.shell.back();
    h.frames(20);
    assert!(!h.shell.osk().is_shown());
    let rect = need(field.borrow().rect, "field rect")?;
    tap(&mut h, rect.center());
    h.frames(5);
    assert!(
        h.shell.osk().is_shown(),
        "re-tapping the field did not reopen it"
    );
    Ok(())
}
