//! The M2 OSK integration test — the OSK, the widgets and the rest of A7 (A5 · A7).
//!
//! The cases: a `TextEdit` tap → the OSK shows
//! plus `PaneInfo.inset_bottom`, the show curve (frame 5 = `osk_h × CubicOut(83.3/180)`), the 60 ms focus-gap
//! debounce, `OskMode::Off` and `Manual` plus the nav `Custom("osk")` toggle, key taps injecting (a letter, ⌫,
//! ⏭ and a face change), the switch's 140 ms, a drag confirming, a long press completing at 500 ms and
//! cancelling at 300 ms, the slider at 1:1, the parametric crossfade at 120 ms and the theme interpolating over
//! 200 ms, and closing with back or the Hide key and staying closed.
//!
//! The rules: `fairing::Result<()>`, and positions come from `osk().key_rect` / `status_bar().item_rect` /
//! `nav_bar().item_rect` and **the `response.rect` the widget gave back** — never from a copy of the layout formula.
// Turning the `osk` feature off drops this target from the build too (checked by `--no-default-features --all-targets`).
#![cfg(feature = "osk")]

use fairing::chrome::{NavItem, NavStyle};
use fairing::motion::Easing;
use fairing::osk::OskLayout;
use fairing::screen::OskMode;
use fairing::services::mock::{MockPower, MockWifi, WifiMsg};
use fairing::services::WifiState;
use fairing::testing::{single_level_access, Harness, FRAME_DT_F32};
use fairing::theme::Palette;
use fairing::widgets::{BigButton, Switch, TouchSlider};
use fairing::{screen, ChromePolicy, Cx, LaunchAction, Services, ShellEvent};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// The long press the widget harness's button asks for.
const LONG_PRESS: Duration = Duration::from_millis(500);

/// `Option` → `Result` (the tests do not use `unwrap`).
fn need<T>(value: Option<T>, what: &str) -> fairing::Result<T> {
    value.ok_or_else(|| fairing::Error::Config(format!("missing: {what}")))
}

// ------------------------------------------------------------ the form screen

/// What a screen with two `TextEdit`s leaves behind each frame.
struct Form {
    a: String,
    b: String,
    rect_a: egui::Rect,
    rect_b: egui::Rect,
    inset: f32,
}

impl Default for Form {
    fn default() -> Self {
        Self {
            a: String::new(),
            b: String::new(),
            rect_a: egui::Rect::NOTHING,
            rect_b: egui::Rect::NOTHING,
            inset: 0.0,
        }
    }
}

/// Two `TextEdit`s plus some empty space. `reduce` turns the animations on and off.
fn form_harness(
    reduce: bool,
    policy: ChromePolicy,
) -> fairing::Result<(Harness, Rc<RefCell<Form>>)> {
    let mut config = single_level_access();
    config.motion.reduce = reduce;
    form_harness_on(config, policy)
}

/// The same on a config of the test's own.
fn form_harness_on(
    config: fairing::ShellConfig,
    policy: ChromePolicy,
) -> fairing::Result<(Harness, Rc<RefCell<Form>>)> {
    let mut h = Harness::new(config, Services::null())?;
    let form = Rc::new(RefCell::new(Form::default()));
    let seen = Rc::clone(&form);
    h.shell.add(
        screen("form", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let state = &mut *seen.borrow_mut();
            state.inset = cx.pane.inset_bottom;
            let a = ui.add_sized(
                [300.0, 48.0],
                egui::TextEdit::singleline(&mut state.a).id_salt("a"),
            );
            state.rect_a = a.rect;
            let b = ui.add_sized(
                [300.0, 48.0],
                egui::TextEdit::singleline(&mut state.b).id_salt("b"),
            );
            state.rect_b = b.rect;
        })
        .chrome(policy),
    );
    h.shell.handle().launch(LaunchAction::open("form"));
    // Until the transition (A2) is over — with `reduce = false` it takes 220 ms.
    h.run_for(0.5);
    Ok((h, form))
}

/// Press → release → settle, three frames (the same as the harness's `tap`, but it does not read the coordinates back).
fn tap(h: &mut Harness, pos: egui::Pos2) {
    h.press(pos);
    h.frame();
    h.release(pos);
    h.frame();
    h.frame();
}

/// It runs to the frame where the OSK starts to show (the tween has advanced once on that frame).
fn run_until_shown(h: &mut Harness, max: usize) -> bool {
    for _ in 0..max {
        h.frame();
        if h.shell.osk().is_shown() {
            return true;
        }
    }
    false
}

// ------------------------------------------------------------------ show and inset

/// The skeleton smoke test, widened: tapping a `TextEdit` shows the OSK and the screen gets an `inset_bottom`.
/// The inset is passed **every frame** and the content Rect does not shrink.
#[test]
fn tapping_a_text_edit_shows_the_osk_and_insets_the_pane() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    let content_before = h.shell.layout().content;
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    assert!(h.shell.osk().is_shown(), "the OSK's target is shown");
    let osk_rect = need(h.shell.layout().osk, "Layout.osk")?;
    let inset = form.borrow().inset;
    assert!(inset > 0.0, "inset_bottom = {inset}");
    assert!(
        (inset - h.shell.osk().inset_bottom()).abs() < 0.01,
        "what the screen gets = Osk::inset_bottom"
    );
    assert_eq!(
        h.shell.layout().content,
        content_before,
        "the content is not pushed"
    );
    let nav = need(h.shell.layout().nav, "Layout.nav")?;
    assert!(
        osk_rect.max.y <= nav.min.y + 0.01,
        "the OSK is above the nav bar: osk={osk_rect:?} nav={nav:?}"
    );
    // Passed every frame: one more frame and the value is still there.
    h.frame();
    assert!((form.borrow().inset - inset).abs() < 0.01);
    Ok(())
}

/// A toast while the keyboard is up stands **on the keyboard**, not over its keys: the content is
/// not pushed, so the stack's floor is the keyboard's top rather than the content's
/// bottom.
#[test]
fn a_toast_stands_on_the_keyboard() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    let osk = need(h.shell.layout().osk, "Layout.osk")?;
    h.shell.toast("Saved");
    h.frames(3);
    let toast = need(
        h.shell.toasts().visible().first().map(|t| t.rect),
        "the toast",
    )?;
    assert!(
        toast.max.y <= osk.min.y + 0.01,
        "the toast is above the keyboard: toast={toast:?} osk={osk:?}"
    );
    assert!(
        toast.max.y > osk.min.y - 40.0,
        "and stands on it rather than at the top: toast={toast:?} osk={osk:?}"
    );
    // The keyboard goes, and the toast comes back down to the content's bottom.
    h.shell.osk_mut().hide();
    h.frames(30);
    let content = h.shell.layout().content;
    let toast = need(
        h.shell.toasts().visible().first().map(|t| t.rect),
        "the toast after the keyboard went",
    )?;
    assert!(
        toast.max.y > osk.min.y + 1.0 && toast.max.y <= content.max.y,
        "back at the content's bottom: toast={toast:?} content={content:?}"
    );
    Ok(())
}

/// A keyboard that leaves no room above it — a short panel, or `height_ratio = 1` here. Pushed up
/// past the content, the toast would go under the status bar and off the top of the screen; it
/// comes down over the keyboard's top rows instead, below the status bar.
#[test]
fn a_toast_stays_below_the_status_bar_over_a_tall_keyboard() -> fairing::Result<()> {
    let mut config = single_level_access();
    config.motion.reduce = true;
    config.osk.height_ratio = 1.0;
    // Rows that big, so the keyboard outgrows the content whatever the finger size: the floor wins
    // over the one-and-a-half-finger cap.
    config.osk.min_key_px = 140.0;
    let (mut h, form) = form_harness_on(config, ChromePolicy::default())?;
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    let osk = need(h.shell.layout().osk, "Layout.osk")?;
    let content = h.shell.layout().content;
    assert!(
        osk.min.y < content.min.y,
        "the keyboard covers the content: osk={osk:?} content={content:?}"
    );
    h.shell.toast("Saved");
    h.frames(3);
    let toast = need(
        h.shell.toasts().visible().first().map(|t| t.rect),
        "the toast",
    )?;
    assert!(
        toast.min.y >= content.min.y - 0.01 && toast.max.y <= content.max.y + 0.01,
        "the toast is inside the content, below the status bar: toast={toast:?} content={content:?}"
    );
    Ok(())
}

// ------------------------------------------------------------------ show curve and debounce

/// A5: on frame 5 (≈83 ms) after the show trigger, `inset_bottom == osk_h × CubicOut(83.3/180)`; a focus gap of
/// 60 ms that comes back gives no hide (the 100 ms debounce).
#[test]
fn osk_show_curve_at_frame_five_and_debounce() -> fairing::Result<()> {
    let (mut h, form) = form_harness(false, ChromePolicy::default())?;
    let field = form.borrow().rect_a.center();
    let empty = h.shell.layout().content.center() + egui::vec2(0.0, 120.0);
    h.press(field);
    h.frame();
    h.release(field);
    assert!(run_until_shown(&mut h, 10), "a tap shows the OSK");
    // The tween has advanced once (= 1 frame) here. Four more = 5 frames.
    h.frames(4);
    let osk_h = h.shell.osk().height();
    // The show tween's length from the token, not a copy of its default.
    let show = h.shell.theme().motion.osk_show.duration.as_secs_f32();
    let expected = osk_h * Easing::CubicOut.apply(5.0 * FRAME_DT_F32 / show);
    let got = h.shell.osk().inset_bottom();
    assert!(
        (got - expected).abs() < 1.0,
        "frame 5: {got} != {expected} (osk_h = {osk_h})"
    );
    h.run_for(0.3);
    assert!(
        (h.shell.osk().inset_bottom() - osk_h).abs() < 0.5,
        "180 ms later it is fully open"
    );
    assert!(!h.shell.osk().is_animating());

    // A 60 ms focus gap and back: inside the debounce (100 ms), so it does not hide.
    tap(&mut h, empty);
    assert!(
        h.shell.osk().is_shown(),
        "tapping empty space and losing focus keeps it for 100 ms"
    );
    tap(&mut h, field);
    assert!(h.shell.osk().is_shown());
    assert!(
        (h.shell.osk().inset_bottom() - osk_h).abs() < 0.5,
        "it did not go back down"
    );
    Ok(())
}

/// `OskMode::Off` does not show even with focus, and `Manual` opens only from the nav's `Custom("osk")`.
#[test]
fn osk_mode_off_and_manual_toggle() -> fairing::Result<()> {
    let off = ChromePolicy {
        osk: OskMode::Off,
        ..ChromePolicy::default()
    };
    let (mut h, form) = form_harness(true, off)?;
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    assert!(!h.shell.osk().is_shown(), "an Off screen has no OSK");
    assert!(h.shell.layout().osk.is_none());

    let manual = ChromePolicy {
        osk: OskMode::Manual,
        ..ChromePolicy::default()
    };
    let (mut h, form) = form_harness(true, manual)?;
    h.shell.nav_bar_mut().style = NavStyle::Buttons {
        items: vec![
            NavItem::Back,
            NavItem::Home,
            NavItem::Custom("osk".to_owned()),
        ],
    };
    h.frames(2);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    assert!(!h.shell.osk().is_shown(), "Manual does not open from focus");
    let item = need(
        h.shell
            .nav_bar()
            .item_rect(&NavItem::Custom("osk".to_owned())),
        "nav Custom(osk)",
    )?;
    let _ = h.shell.poll_events();
    tap(&mut h, item.center());
    assert!(h.shell.osk().is_shown(), "a nav item tap opens it");
    assert!(h
        .shell
        .poll_events()
        .iter()
        .any(|e| matches!(e, ShellEvent::OskToggled(true))));
    tap(&mut h, item.center());
    assert!(!h.shell.osk().is_shown(), "one more tap closes it");
    Ok(())
}

// ------------------------------------------------------------------ key taps

/// A key tap → the letter goes into the focused field. ⌫ clears it, ⏭ moves to the next field, ⇧ changes face.
#[test]
fn osk_key_tap_injects_text_into_the_focused_edit() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    assert!(h.shell.osk().is_visible());

    let q = need(h.shell.osk().key_rect("q"), "key q")?;
    tap(&mut h, q.center());
    assert_eq!(
        form.borrow().a,
        "q",
        "a key tap goes into the focused field"
    );

    // Typing on keeps the focus alive.
    let w = need(h.shell.osk().key_rect("w"), "key w")?;
    tap(&mut h, w.center());
    assert_eq!(form.borrow().a, "qw");

    let back = need(h.shell.osk().key_rect("⌫"), "key ⌫")?;
    tap(&mut h, back.center());
    tap(&mut h, back.center());
    assert_eq!(form.borrow().a, "", "⌫ empties it");

    // ⇧ → the upper-case face; one letter and it comes back to lower case.
    let shift = need(h.shell.osk().key_rect("⇧"), "key ⇧")?;
    tap(&mut h, shift.center());
    assert_eq!(h.shell.osk().face(), 1, "the face changed");
    let upper_q = need(h.shell.osk().key_rect("Q"), "key Q")?;
    tap(&mut h, upper_q.center());
    assert_eq!(form.borrow().a, "Q");
    assert_eq!(
        h.shell.osk().face(),
        0,
        "the lower-case face after one letter"
    );

    // ⏭ = Tab → the focus moves to the next field.
    let next = need(h.shell.osk().key_rect("⏭"), "key ⏭")?;
    tap(&mut h, next.center());
    let q = need(h.shell.osk().key_rect("q"), "key q")?;
    tap(&mut h, q.center());
    assert_eq!(form.borrow().a, "Q", "the first field is as it was");
    assert_eq!(form.borrow().b, "q", "it goes into the second field");
    Ok(())
}

// ------------------------------------------------------------------ Hangul

/// It taps the jamo labels in turn.
fn tap_jamo(h: &mut Harness, jamo: &[&str]) -> fairing::Result<()> {
    for label in jamo {
        let key = need(h.shell.osk().key_rect(label), label)?;
        tap(h, key.center());
    }
    Ok(())
}

/// Typing dubeolsik jamo has `HangulComposer` compose them into syllables and put them in as IME
/// `Preedit`/`Commit`. It does not imitate backspace, so the `TextEdit`'s real contents are the result.
#[test]
fn hangul_keys_compose_syllables_in_the_focused_edit() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    assert!(h.shell.osk().is_visible());

    // "한" — the final piles up inside the one syllable too.
    tap_jamo(&mut h, &["ㅎ", "ㅏ", "ㄴ"])?;
    assert_eq!(
        form.borrow().a,
        "한",
        "a letter mid-composition shows in the buffer too"
    );
    assert!(h.shell.osk().is_composing());
    assert_eq!(h.shell.osk().preedit(), "한");

    // "글" — the previous syllable is committed and a new one opens.
    tap_jamo(&mut h, &["ㄱ", "ㅡ", "ㄹ"])?;
    assert_eq!(form.borrow().a, "한글");

    // Space commits the composition and goes in as it is.
    let space = need(h.shell.osk().key_rect(" "), "space")?;
    tap(&mut h, space.center());
    assert_eq!(form.borrow().a, "한글 ");
    assert!(!h.shell.osk().is_composing());
    Ok(())
}

/// Mid-composition ⌫ unwinds one jamo — it does not erase the whole syllable.
#[test]
fn backspace_unwinds_one_jamo_while_composing() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);

    tap_jamo(&mut h, &["ㄱ", "ㅏ", "ㅂ", "ㅅ"])?;
    assert_eq!(form.borrow().a, "값");

    let back = need(h.shell.osk().key_rect("⌫"), "key ⌫")?;
    tap(&mut h, back.center());
    assert_eq!(
        form.borrow().a,
        "갑",
        "a compound final unwinds one step only"
    );
    tap(&mut h, back.center());
    assert_eq!(form.borrow().a, "가");
    tap(&mut h, back.center());
    assert_eq!(form.borrow().a, "ㄱ");
    tap(&mut h, back.center());
    assert_eq!(form.borrow().a, "");
    // The composition is over, so it is a real backspace now (the field is empty, so it stays empty).
    tap(&mut h, back.center());
    assert_eq!(form.borrow().a, "");
    Ok(())
}

/// The `한/영` key crosses between dubeolsik and English. A letter mid-composition is committed on the way over.
#[test]
fn the_lang_key_switches_between_hangul_and_qwerty() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);

    tap_jamo(&mut h, &["ㄱ", "ㅏ"])?;
    assert!(h.shell.osk().is_composing());

    let lang = need(h.shell.osk().key_rect("한/영"), "the 한/영 key")?;
    tap(&mut h, lang.center());
    assert!(!h.shell.osk().is_composing(), "changing language commits");
    assert_eq!(h.shell.osk().layout(), &OskLayout::Qwerty);
    assert!(
        h.shell.osk().is_shown(),
        "a language change revives the focus too, so the keyboard stays"
    );

    // Typing on the English face goes in as it is.
    let q = need(h.shell.osk().key_rect("q"), "key q")?;
    tap(&mut h, q.center());
    assert_eq!(form.borrow().a, "가q");

    // And back again.
    let lang = need(h.shell.osk().key_rect("한/영"), "the 한/영 key")?;
    tap(&mut h, lang.center());
    assert_eq!(h.shell.osk().layout(), &OskLayout::Hangul);
    tap_jamo(&mut h, &["ㄴ", "ㅏ"])?;
    assert_eq!(form.borrow().a, "가q나");
    Ok(())
}

/// Moving to another field throws away only the composition state — the same letter does not go into the new field again.
#[test]
fn moving_focus_drops_the_composition_without_repeating_it() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field_a = form.borrow().rect_a.center();
    tap(&mut h, field_a);
    tap_jamo(&mut h, &["ㄱ", "ㅏ"])?;
    assert_eq!(form.borrow().a, "가");

    let field_b = form.borrow().rect_b.center();
    tap(&mut h, field_b);
    assert!(
        !h.shell.osk().is_composing(),
        "moving the focus throws the composition away"
    );

    tap_jamo(&mut h, &["ㄴ", "ㅏ"])?;
    assert_eq!(form.borrow().a, "가", "the previous field is as it was");
    assert_eq!(
        form.borrow().b,
        "나",
        "only the new composition goes into the new field"
    );
    Ok(())
}

// --------------------------------------------------------------- regressions

/// A regression: the keyboard used to go down after a face-change key (`?123` · ⇧) **if it was then left alone**.
/// egui drops the `TextEdit` focus on a press, and the face-change path alone did not revive it, so the panel went
/// down after `osk_hide_debounce` (100 ms). The existing test pressed the next key straight after the change and
/// never crossed the debounce window, so it missed this — here it rests for half a second.
#[test]
fn switching_face_keeps_the_osk_up_and_focused() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    assert!(h.shell.osk().is_visible());

    // Change to the numbers-and-symbols face and rest.
    let sym = need(h.shell.osk().key_rect("?123"), "key ?123")?;
    tap(&mut h, sym.center());
    assert_eq!(h.shell.osk().face(), 2, "the symbol face");
    h.run_for(0.5);
    assert!(
        h.shell.osk().is_shown(),
        "the keyboard is still there after a face change and half a second's rest"
    );
    let one = need(h.shell.osk().key_rect("1"), "key 1")?;
    tap(&mut h, one.center());
    assert_eq!(
        form.borrow().a,
        "1",
        "the focus is alive, so typing carries on"
    );

    // ⇧ takes the same path.
    let abc = need(h.shell.osk().key_rect("abc"), "key abc")?;
    tap(&mut h, abc.center());
    let shift = need(h.shell.osk().key_rect("⇧"), "key ⇧")?;
    tap(&mut h, shift.center());
    h.run_for(0.5);
    assert!(h.shell.osk().is_shown(), "still there after ⇧ too");
    let upper = need(h.shell.osk().key_rect("Q"), "key Q")?;
    tap(&mut h, upper.center());
    assert_eq!(form.borrow().a, "1Q");
    Ok(())
}

// ------------------------------------------------------------------ back and Hide close it

/// Closed with back or the Hide key it does not open again even with the focus as it was, and tapping
/// another field to take the focus afresh opens it again.
#[test]
fn back_closes_the_osk_and_it_stays_closed() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    let (field_a, field_b) = {
        let f = form.borrow();
        (f.rect_a.center(), f.rect_b.center())
    };
    tap(&mut h, field_a);
    assert!(h.shell.osk().is_shown());

    h.shell.back();
    h.frames(2);
    assert!(!h.shell.osk().is_shown(), "back closes the OSK first");
    assert!(h.shell.osk().is_dismissed());
    assert!(
        h.shell.osk().inset_bottom() < 0.5,
        "with reduce it is 0 at once"
    );
    h.frames(10);
    assert!(
        !h.shell.osk().is_shown() && h.shell.osk().is_dismissed(),
        "with the focus as it was it stays closed"
    );
    assert!(h.shell.layout().osk.is_none());

    tap(&mut h, field_b);
    assert!(
        h.shell.osk().is_shown(),
        "tapping another field = taking the focus afresh"
    );
    // Tapping the same field again opens it too (egui passes the focus between fields in one pass).
    h.shell.back();
    h.frames(2);
    assert!(!h.shell.osk().is_shown());
    tap(&mut h, field_b);
    assert!(
        h.shell.osk().is_shown(),
        "re-tapping the same field opens it again"
    );

    // The Hide key (▾) takes the same path.
    let hide = need(h.shell.osk().key_rect("▾"), "key ▾")?;
    tap(&mut h, hide.center());
    assert!(!h.shell.osk().is_shown() && h.shell.osk().is_dismissed());
    h.frames(10);
    assert!(!h.shell.osk().is_shown());
    tap(&mut h, field_a);
    assert!(h.shell.osk().is_shown());
    Ok(())
}

// ---------------------------------------------------------- the widget screen

/// What the widget screen leaves behind each frame.
struct Widgets {
    on: bool,
    value: f32,
    changed: u32,
    completions: u32,
    progress: f32,
    switch_rect: egui::Rect,
    button_rect: egui::Rect,
    slider_rect: egui::Rect,
}

impl Default for Widgets {
    fn default() -> Self {
        Self {
            on: false,
            value: 0.0,
            changed: 0,
            completions: 0,
            progress: 0.0,
            switch_rect: egui::Rect::NOTHING,
            button_rect: egui::Rect::NOTHING,
            slider_rect: egui::Rect::NOTHING,
        }
    }
}

/// One switch, one long-press button and one slider. `reduce = false` so the animations can be seen.
fn widget_harness() -> fairing::Result<(Harness, Rc<RefCell<Widgets>>)> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    let state = Rc::new(RefCell::new(Widgets::default()));
    let seen = Rc::clone(&state);
    h.shell.add(screen(
        "widgets",
        move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
            let w = &mut *seen.borrow_mut();
            let sw = Switch::new(&mut w.on).show(ui, &mut cx.widgets());
            w.switch_rect = sw.rect;
            if sw.changed() {
                w.changed += 1;
            }
            let big = BigButton::new("hold")
                .long_press(LONG_PRESS)
                .show(ui, &mut cx.widgets());
            w.button_rect = big.response.rect;
            w.progress = big.progress;
            if big.completed {
                w.completions += 1;
            }
            let sl = TouchSlider::new(&mut w.value, 0.0..=100.0).show(ui, &mut cx.widgets());
            w.slider_rect = sl.rect;
        },
    ));
    h.shell.handle().launch(LaunchAction::open("widgets"));
    h.run_for(0.5);
    Ok((h, state))
}

/// It drags slowly and lets go **after stopping** — the release velocity is 0, so only the distance rule applies.
fn drag_and_settle(h: &mut Harness, from: egui::Pos2, to: egui::Pos2, steps: usize) {
    h.press(from);
    h.frame();
    let steps = steps.max(1);
    for i in 1..=steps {
        #[allow(clippy::cast_precision_loss)]
        let t = i as f32 / steps as f32;
        h.move_to(from + (to - from) * t);
        h.frame();
    }
    // Several frames in place so the velocity window (100 ms) empties.
    for _ in 0..10 {
        h.move_to(to);
        h.frame();
    }
    h.release(to);
    h.frame();
    h.frame();
}

// ------------------------------------------------------------------ widgets

/// A7: the switch's tap at 140 ms and its drag confirming at 50 %, the long press completing once at 500 ms and
/// cancelling at 300 ms, and the slider's value at 1:1.
#[test]
fn switch_and_long_press_and_slider() -> fairing::Result<()> {
    let (mut h, w) = widget_harness()?;
    let switch = w.borrow().switch_rect;
    assert!(switch.is_positive(), "the switch Rect");

    // A tap → it flips and the knob moves for 140 ms.
    tap(&mut h, switch.center());
    {
        let state = w.borrow();
        assert!(state.on, "a tap turns it on");
        assert_eq!(state.changed, 1);
    }
    assert!(h.shell.is_animating(), "the knob tween is running");
    let knob_tween = h.shell.theme().motion.switch.duration.as_secs_f64();
    h.run_for(knob_tween + 0.06);
    assert!(
        !h.shell.is_animating(),
        "it is over inside the switch tween"
    );

    // Back again by dragging: let go short of half and it stays, past half and it flips.
    let left = switch.left_center() + egui::vec2(2.0, 0.0);
    let right = switch.right_center() - egui::vec2(2.0, 0.0);
    drag_and_settle(&mut h, right, switch.center() + egui::vec2(3.0, 0.0), 8);
    assert!(w.borrow().on, "let go past half, it stays on");
    drag_and_settle(&mut h, right, left, 8);
    assert!(
        !w.borrow().on,
        "dragged to the left end and let go, it goes off"
    );
    h.run_for(0.2);

    // The long press: filling 500 ms completes **once**.
    let button = w.borrow().button_rect;
    h.hold(button.center(), 32);
    assert_eq!(w.borrow().completions, 1, "once at 500 ms");
    assert!((w.borrow().progress - 1.0).abs() < 1e-3);
    h.frames(30);
    assert_eq!(w.borrow().completions, 1, "holding on does not repeat it");
    h.release(button.center());
    h.frames(2);
    let back = h.shell.theme().motion.press_release.duration.as_secs_f64();
    h.run_for(back + 0.04);
    assert!(
        w.borrow().progress < 0.01,
        "letting go makes the ring go inside the release tween"
    );

    // Letting go at 300 ms plays back with no completion.
    let (mut h, w) = widget_harness()?;
    let button = w.borrow().button_rect;
    h.hold(button.center(), 18);
    let at_release = w.borrow().progress;
    assert!(
        at_release > 0.4 && at_release < 0.8,
        "300 ms ≈ 0.6: {at_release}"
    );
    assert_eq!(w.borrow().completions, 0);
    h.release(button.center());
    h.frames(2);
    h.run_for(back + 0.04);
    assert!(
        w.borrow().progress < 0.01,
        "the release tween of playing back"
    );
    assert_eq!(w.borrow().completions, 0);

    // The slider: the value tracks the finger 1:1 (no easing).
    let slider = w.borrow().slider_rect;
    let thumb = h.shell.theme().metrics.slider_thumb;
    let axis = slider.width() - thumb;
    h.press(slider.center());
    h.frame();
    h.frame();
    assert!(
        (w.borrow().value - 50.0).abs() < 0.5,
        "the middle = 50: {}",
        w.borrow().value
    );
    let step = 100.0f32;
    h.move_to(slider.center() + egui::vec2(step, 0.0));
    h.frame();
    let expected = 50.0 + step / axis * 100.0;
    assert!(
        (w.borrow().value - expected).abs() < 0.5,
        "1:1: {} != {expected}",
        w.borrow().value
    );
    h.release(slider.center() + egui::vec2(step, 0.0));
    h.frames(2);
    Ok(())
}

// ------------------------------------------------------------------ crossfade and theme

/// A7: a change in the Wi-Fi strength crossfades the status bar icon over 120 ms, and a theme change interpolates
/// the palette over 200 ms.
#[test]
fn parametric_and_theme_crossfade() -> fairing::Result<()> {
    let wifi = MockWifi::new();
    let control = wifi.control();
    let services = Services::builder()
        .clock(fairing::services::null::NullClock)
        .wifi(wifi)
        .power(MockPower::new(72, false))
        .build();
    let mut h = Harness::new(single_level_access(), services)?;
    // Until the first frame's initial-value-to-real-value fade is over.
    h.run_for(0.25);
    assert!(
        h.shell.status_bar().item_rect("status.wifi").is_some(),
        "the Wi-Fi item is drawn"
    );
    assert!(
        !h.shell.status_bar().is_animating(h.now()),
        "it starts idle"
    );

    control.send(WifiMsg::State(WifiState::Connected {
        ssid: "fairing-lab".to_owned(),
        strength: 4,
    }));
    h.frames(2);
    assert!(
        h.shell.status_bar().is_animating(h.now()),
        "a change in strength crossfades"
    );
    h.run_for(0.15);
    assert!(
        !h.shell.status_bar().is_animating(h.now()),
        "it is over inside 120 ms"
    );

    // The theme: dark → light interpolating over 200 ms.
    assert_eq!(h.shell.theme().palette, Palette::dark());
    h.shell.set_theme_dark(false);
    h.run_for(0.1);
    let mid = h.shell.theme().palette;
    assert_ne!(mid, Palette::dark(), "a middle value at 100 ms");
    assert_ne!(mid, Palette::light());
    h.run_for(0.15);
    assert_eq!(
        h.shell.theme().palette,
        Palette::light(),
        "light after 200 ms"
    );
    h.frames(3);
    assert!(!h.shell.is_animating(), "idle after settling");
    Ok(())
}

/// Going to English with `set_layout` and back **keeps the `한/영` key.**
///
/// It used to attach the pair only on dubeolsik, so an integrator calling `set_layout(Qwerty)` lost the key that
/// would come back and was stuck in English. `toggle_lang`, which the `한/영` key calls, kept it all along — only
/// this path did not. The other way round, **starting on qwerty does not attach one** — a German kiosk has no
/// reason to carry a `한/영` key.
#[test]
fn the_language_key_survives_a_layout_change() -> fairing::Result<()> {
    let mut h = Harness::new(single_level_access(), Services::null())?;
    h.frames(2);
    // Starting on qwerty: no pair.
    h.shell
        .osk_mut()
        .set_layout(fairing::osk::OskLayout::Qwerty);
    assert!(
        !h.shell.osk().has_lang_key(),
        "it started on qwerty and a 한/영 key was attached"
    );

    // Dubeolsik: the pair is attached.
    h.shell
        .osk_mut()
        .set_layout(fairing::osk::OskLayout::Hangul);
    assert!(
        h.shell.osk().has_lang_key(),
        "it is dubeolsik and there is no 한/영 key"
    );

    // Moving to English keeps it — there has to be a way back.
    h.shell
        .osk_mut()
        .set_layout(fairing::osk::OskLayout::Qwerty);
    assert!(
        h.shell.osk().has_lang_key(),
        "the 한/영 key went on the way to English — there is no way back"
    );
    Ok(())
}

/// **The intermediate composition shows in the field at every jamo.** Appearing only once committed leaves
/// the user unable to see what they are typing — the most conspicuous defect there is in Hangul input.
#[test]
fn every_jamo_shows_its_half_built_syllable() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);

    for (jamo, seen) in [("ㅎ", "ㅎ"), ("ㅏ", "하"), ("ㄴ", "한")] {
        let key = need(h.shell.osk().key_rect(jamo), jamo)?;
        tap(&mut h, key.center());
        assert_eq!(form.borrow().a, seen, "straight after typing {jamo}");
        assert_eq!(
            h.shell.osk().preedit(),
            seen,
            "the composition state for {jamo}"
        );
    }
    Ok(())
}

/// **A letter mid-composition survives the integration layer sending an empty `Preedit("")`.**
///
/// This reproduces the symptom on real hardware where a letter mid-composition was not visible at all and only the
/// committed one appeared. winit can emit an empty IME event every time `set_ime_allowed` and `set_ime_cursor_area`
/// are called (= every time the caret moves, so every time a letter is typed), and `TextEdit`, mid-composition,
/// reads that as "composition cancelled" and clears the whole composition string. So a syllable being composed goes
/// in as **a real letter** rather than a `Preedit`.
#[test]
fn a_stray_empty_preedit_does_not_wipe_the_syllable() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);

    let key = need(h.shell.osk().key_rect("ㅎ"), "the ㅎ key")?;
    tap(&mut h, key.center());
    assert_eq!(form.borrow().a, "ㅎ");

    // The empty composition event the platform lets slip as it announces the caret moving.
    h.push_event(egui::Event::Ime(egui::ImeEvent::Preedit {
        text: String::new(),
        active_range_chars: None,
    }));
    h.frames(2);
    assert_eq!(
        form.borrow().a,
        "ㅎ",
        "the letter was swept away by an empty Preedit"
    );

    // The composition carries on.
    let key = need(h.shell.osk().key_rect("ㅏ"), "the ㅏ key")?;
    tap(&mut h, key.center());
    assert_eq!(form.borrow().a, "하");
    Ok(())
}

/// Pressing outside the panel ends the composition — even inside the same field the caret may have moved, and then
/// the next jamo's ⌫ erases the wrong letter.
#[test]
fn pressing_outside_the_keyboard_ends_the_composition() -> fairing::Result<()> {
    let (mut h, form) = form_harness(true, ChromePolicy::default())?;
    h.shell.osk_mut().set_layout(OskLayout::Hangul);
    let field = form.borrow().rect_a.center();
    tap(&mut h, field);
    tap_jamo(&mut h, &["ㅎ", "ㅏ"])?;
    assert_eq!(form.borrow().a, "하");
    assert!(h.shell.osk().is_composing());

    // Press the field again (the caret may have moved).
    tap(&mut h, field);
    assert!(!h.shell.osk().is_composing(), "the composition did not end");

    // Typing on piles up a new syllable without touching the previous letter.
    tap_jamo(&mut h, &["ㄱ", "ㅏ"])?;
    assert_eq!(form.borrow().a, "하가", "the previous syllable was erased");
    Ok(())
}
