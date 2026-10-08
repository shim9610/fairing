//! **Drawing the lock screen and the unlock prompt yourself** — rung 5 of the override ladder.
//! A lock screen painter draws the ground, the clock and the date; an
//! unlock prompt painter draws the backdrop and the card. The way in is still the shell's, drawn
//! over them, and so are what is typed, the answer and the motion.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::access::{LockScreenCx, PromptPiece, UnlockPromptCx};
use fairing::testing::{access_config, Harness};
use fairing::{screen, ColorRole, Cx, LaunchAction, Level, Shell, ShellBuilder, ShellConfig};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// The painters' own fills — colours nothing built in uses.
const GROUND: egui::Color32 = egui::Color32::from_rgb(3, 141, 59);
const CARD: egui::Color32 = egui::Color32::from_rgb(141, 3, 59);

/// Three levels, everything at the top, and a PIN for the two upper ones. `reduce` turns the
/// motion off, so the modal is there whole two frames after it is asked for.
fn pins(reduce: bool) -> ShellConfig {
    let mut config = access_config(&["viewer", "operator", "maintainer"], Some("top"));
    config.access.pin_table.pins = [("operator", "1234"), ("maintainer", "9876")]
        .iter()
        .map(|(level, pin)| ((*level).to_owned(), (*pin).to_owned()))
        .collect();
    config.motion.reduce = reduce;
    config
}

/// A shell on a fixed clock, with an `admin` screen behind the top level, built with `build`.
fn shell_with(
    reduce: bool,
    build: impl FnOnce(ShellBuilder) -> ShellBuilder,
) -> fairing::Result<Harness> {
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::from_builder(move |ctx| {
        build(Shell::builder(pins(reduce)).services(services)).build(ctx)
    })?;
    h.shell
        .add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("calibration");
        }));
    h.frames(2);
    Ok(h)
}

/// Every shape drawn in one frame, nested ones taken out, in paint order.
fn shapes(h: &mut Harness) -> Vec<egui::Shape> {
    fn flat(shape: egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    flat(shape, out);
                }
            }
            shape => out.push(shape),
        }
    }
    let mut out = Vec::new();
    for clipped in h.frame_shapes() {
        flat(clipped.shape, &mut out);
    }
    out
}

/// Where the text reading exactly `wanted` was drawn, the topmost if more than once.
fn text_rect(shapes: &[egui::Shape], wanted: &str) -> Option<egui::Rect> {
    shapes.iter().rev().find_map(|shape| match shape {
        egui::Shape::Text(text) if text.galley.text() == wanted => {
            Some(text.galley.rect.translate(text.pos.to_vec2()))
        }
        _ => None,
    })
}

fn same(a: egui::Rect, b: egui::Rect) -> bool {
    (a.min - b.min).length() < 0.5 && (a.max - b.max).length() < 0.5
}

fn near(a: egui::Color32, b: egui::Color32) -> bool {
    a.to_array()
        .iter()
        .zip(b.to_array())
        .all(|(x, y)| x.abs_diff(y) <= 2)
}

/// Whether a rect filled with `color` was drawn over `rect`.
fn filled(shapes: &[egui::Shape], rect: egui::Rect, color: egui::Color32) -> bool {
    shapes.iter().any(|shape| {
        matches!(shape, egui::Shape::Rect(drawn) if same(drawn.rect, rect) && near(drawn.fill, color))
    })
}

/// Where a rect filled with exactly `color` landed on the glass, if one did — the painters' fills
/// are colours nothing else uses, so a fill found at `color.gamma_multiply(alpha)` was drawn at that
/// alpha, and not faded again on the way out.
fn landed(shapes: &[egui::Shape], color: egui::Color32) -> Option<egui::Rect> {
    shapes.iter().find_map(|shape| match shape {
        egui::Shape::Rect(drawn) if near(drawn.fill, color) => Some(drawn.rect),
        _ => None,
    })
}

/// The keypad's keys, where they are on the glass.
fn keys(h: &Harness) -> fairing::Result<Vec<egui::Rect>> {
    (0..10)
        .map(|digit| {
            h.shell
                .prompt_digit_rect(digit)
                .ok_or_else(|| fail(format!("no key {digit}")))
        })
        .collect()
}

/// What a lock screen painter was told, once.
#[derive(Debug, Clone)]
struct ToldLock {
    screen: egui::Rect,
    clock: egui::Rect,
    room: egui::Rect,
    time: String,
    date: String,
    alpha: f32,
    leaving: bool,
}

/// A lock screen painter that fills the screen with [`GROUND`] at its fade, writes the time where
/// the clock goes, and keeps what it was told.
fn lock_painter(
    told: &Rc<RefCell<Vec<ToldLock>>>,
) -> impl FnMut(&egui::Painter, &mut LockScreenCx<'_>) + 'static {
    let told = Rc::clone(told);
    move |painter: &egui::Painter, lock: &mut LockScreenCx<'_>| {
        painter.rect_filled(lock.screen, 0.0, GROUND.gamma_multiply(lock.alpha));
        painter.text(
            lock.clock.center(),
            egui::Align2::CENTER_CENTER,
            format!("painted {}", lock.time),
            egui::FontId::proportional(24.0),
            egui::Color32::WHITE,
        );
        told.borrow_mut().push(ToldLock {
            screen: lock.screen,
            clock: lock.clock,
            room: lock.room,
            time: lock.time.to_owned(),
            date: lock.date.to_owned(),
            alpha: lock.alpha,
            leaving: lock.leaving,
        });
    }
}

fn locked(h: &mut Harness) -> fairing::Result<()> {
    h.shell.launch(LaunchAction::Lock);
    h.frames(2);
    if h.shell.lock_screen_visible() {
        Ok(())
    } else {
        Err(fail("the lock screen is not up"))
    }
}

/// **A lock screen painter draws the ground, the clock and the date**, and the built-in ones are
/// not drawn: the date the shell would write is told to the painter and is nowhere on the glass.
#[test]
fn a_lock_screen_painter_draws_the_ground_and_the_clock() -> fairing::Result<()> {
    let mut plain = shell_with(true, |b| b)?;
    locked(&mut plain)?;
    let built_in = shapes(&mut plain);

    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(true, |b| b.lock_screen_painter(lock_painter(&told)))?;
    locked(&mut h)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let last = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called"))?;
    let screen = h.screen_rect();
    assert!(same(last.screen, screen), "{:?}", last.screen);
    assert!((last.alpha - 1.0).abs() < 1e-3 && !last.leaving, "{last:?}");
    assert!(
        !last.time.is_empty() && text_rect(&drawn, &format!("painted {}", last.time)).is_some(),
        "the painter's clock is not drawn: {last:?}"
    );
    // The built-in lock screen writes the time and, under it, the date: told the date, the
    // painter has a line the shell draws that is not the time.
    assert!(
        last.date != last.time && text_rect(&built_in, &last.date).is_some(),
        "the painter is not told the date the shell writes: {last:?}"
    );
    assert!(
        text_rect(&drawn, &last.date).is_none(),
        "the built-in date is drawn under the painter"
    );
    assert!(filled(&drawn, screen, GROUND), "no ground");
    let background = h.shell.theme().color(ColorRole::Background);
    assert!(
        filled(&built_in, screen, background) && !filled(&drawn, screen, background),
        "the built-in ground is drawn under the painter"
    );
    // The way in is centred in `room`, clear of `clock`.
    let five = h
        .shell
        .prompt_digit_rect(5)
        .ok_or_else(|| fail("the keypad is not up"))?;
    assert!(last.room.contains_rect(five), "{five:?} in {:?}", last.room);
    assert!(!last.clock.intersects(five), "{five:?} in {:?}", last.clock);
    assert!(screen.contains_rect(last.clock) && last.clock.is_positive());
    Ok(())
}

/// **The keypad still unlocks a painted lock screen**: the way in is the shell's, over the ground.
#[test]
fn the_keypad_still_unlocks_a_painted_lock_screen() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(true, |b| b.lock_screen_painter(lock_painter(&told)))?;
    locked(&mut h)?;
    let drawn = shapes(&mut h);
    assert!(text_rect(&drawn, "Enter PIN").is_some(), "no way in");
    for digit in "1234".chars() {
        let rect = text_rect(&shapes(&mut h), &digit.to_string())
            .ok_or_else(|| fail(format!("no key {digit}")))?;
        h.tap(rect.center());
    }
    h.frames(2);
    assert!(!h.shell.lock_screen_visible());
    assert_eq!(h.shell.access().session().subject.level, Level(1));
    Ok(())
}

/// **A lock screen painter is told the fade, and is not faded again.** Coming in, it is told an
/// alpha between 0 and 1 and what it draws comes out at that alpha — not at its square, as it
/// would on a layer faded too. Leaving, it is told so, fading.
#[test]
fn a_lock_screen_painter_is_told_the_fade_and_draws_at_it() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(false, |b| b.lock_screen_painter(lock_painter(&told)))?;
    h.shell.launch(LaunchAction::Lock);
    h.frames(4);
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let alpha = told
        .borrow()
        .last()
        .map(|t| t.alpha)
        .ok_or_else(|| fail("the painter was not called"))?;
    assert!(alpha > 0.05 && alpha < 0.95, "not mid-fade: {alpha}");
    let ground = landed(&drawn, GROUND.gamma_multiply(alpha));
    assert!(
        ground.is_some_and(|rect| same(rect, h.screen_rect())),
        "the ground is not drawn at the alpha it was told ({alpha})"
    );

    h.run_for(0.5);
    for digit in "1234".chars() {
        let rect = text_rect(&shapes(&mut h), &digit.to_string())
            .ok_or_else(|| fail(format!("no key {digit}")))?;
        h.tap(rect.center());
    }
    told.borrow_mut().clear();
    h.run_for(0.5);
    let told = told.borrow();
    assert!(
        told.iter()
            .any(|t| t.leaving && t.alpha > 0.0 && t.alpha < 1.0),
        "not told it is leaving: {told:?}"
    );
    assert!(told.iter().all(|t| t.leaving), "{told:?}");
    Ok(())
}

/// What an unlock prompt painter was told, once.
type ToldPiece = (PromptPiece, egui::Rect, f32, bool);

/// An unlock prompt painter that fills the backdrop with [`GROUND`] and the card with [`CARD`], at
/// their fades, and keeps what it was told.
fn prompt_painter(
    told: &Rc<RefCell<Vec<ToldPiece>>>,
) -> impl FnMut(&egui::Painter, &mut UnlockPromptCx<'_>) + 'static {
    let told = Rc::clone(told);
    move |painter: &egui::Painter, prompt: &mut UnlockPromptCx<'_>| {
        let color = match prompt.piece {
            PromptPiece::Backdrop => GROUND,
            _ => CARD,
        };
        painter.rect_filled(prompt.rect, 0.0, color.gamma_multiply(prompt.alpha));
        told.borrow_mut()
            .push((prompt.piece, prompt.rect, prompt.alpha, prompt.leaving));
    }
}

fn prompted(h: &mut Harness) -> fairing::Result<()> {
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(2);
    if h.shell.unlock_prompt_visible() {
        Ok(())
    } else {
        Err(fail("the unlock prompt is not up"))
    }
}

fn last_piece(told: &[ToldPiece], piece: PromptPiece) -> fairing::Result<ToldPiece> {
    told.iter()
        .rev()
        .find(|t| t.0 == piece)
        .copied()
        .ok_or_else(|| fail(format!("the painter was not called for {piece:?}")))
}

/// **An unlock prompt painter draws the backdrop over the screen and the card under the way in**,
/// and the built-in scrim and plate are not drawn. The card it draws lands where the built-in one
/// does — on the card's layer, shrunk whole with it on a screen too short for the keypad — with the
/// keys on it, and Cancel still closes the prompt.
#[test]
fn an_unlock_prompt_painter_draws_the_backdrop_and_the_card() -> fairing::Result<()> {
    let mut plain = shell_with(true, |b| b)?;
    prompted(&mut plain)?;
    let built_in = shapes(&mut plain);
    let theme = plain.shell.theme();
    let (scrim, surface) = (
        theme.color(ColorRole::Scrim),
        theme.color(ColorRole::Surface),
    );
    let plain_keys = keys(&plain)?;
    // The built-in card: the smallest plate in the surface colour with every key on it.
    let built_in_card = built_in
        .iter()
        .filter_map(|shape| match shape {
            egui::Shape::Rect(drawn)
                if near(drawn.fill, surface)
                    && plain_keys.iter().all(|key| drawn.rect.contains_rect(*key)) =>
            {
                Some(drawn.rect)
            }
            _ => None,
        })
        .min_by(|a, b| a.area().total_cmp(&b.area()))
        .ok_or_else(|| fail("no built-in card"))?;

    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(true, |b| b.unlock_prompt_painter(prompt_painter(&told)))?;
    prompted(&mut h)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let screen = h.screen_rect();
    let backdrop = last_piece(&told.borrow(), PromptPiece::Backdrop)?;
    let card = last_piece(&told.borrow(), PromptPiece::Card)?;
    assert!(same(backdrop.1, screen), "{backdrop:?}");
    assert!((backdrop.2 - 1.0).abs() < 1e-3 && (card.2 - 1.0).abs() < 1e-3);
    assert!(!backdrop.3 && !card.3, "told it is leaving");
    assert!(
        landed(&drawn, GROUND).is_some_and(|rect| same(rect, screen)),
        "no backdrop"
    );
    let painted = landed(&drawn, CARD).ok_or_else(|| fail("no card"))?;
    assert!(
        same(painted, built_in_card),
        "the card lands at {painted:?}, the built-in one at {built_in_card:?}"
    );
    assert!(
        (card.1.center() - painted.center()).length() < 0.5,
        "{card:?} vs {painted:?}"
    );
    for key in keys(&h)? {
        assert!(
            painted.contains_rect(key),
            "{key:?} off the card {painted:?}"
        );
    }
    assert!(
        filled(&built_in, screen, scrim) && !filled(&drawn, screen, scrim),
        "the built-in scrim is drawn under the painter"
    );
    assert!(
        !filled(&drawn, built_in_card, surface),
        "the built-in card is drawn under the painter"
    );
    assert!(text_rect(&drawn, "Enter PIN").is_some(), "no way in");
    let cancel = text_rect(&drawn, "Cancel").ok_or_else(|| fail("no Cancel"))?;
    h.tap(cancel.center());
    h.frames(2);
    assert!(!h.shell.unlock_prompt_visible());
    Ok(())
}

/// **The card painter is told the card's fade and draws at it**, on a layer the shell does not
/// fade again — and the backdrop is told its own, which runs on a different curve.
#[test]
fn an_unlock_prompt_painter_is_told_each_pieces_fade() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(false, |b| b.unlock_prompt_painter(prompt_painter(&told)))?;
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(3);
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let backdrop = last_piece(&told.borrow(), PromptPiece::Backdrop)?;
    let card = last_piece(&told.borrow(), PromptPiece::Card)?;
    assert!(card.2 > 0.05 && card.2 < 0.95, "not mid-fade: {card:?}");
    assert!(
        backdrop.2 > 0.0 && (backdrop.2 - card.2).abs() > 0.02,
        "the backdrop is told the card's fade: {backdrop:?} {card:?}"
    );
    assert!(
        landed(&drawn, CARD.gamma_multiply(card.2)).is_some(),
        "the card is not drawn at the alpha it was told ({})",
        card.2
    );
    assert!(
        landed(&drawn, GROUND.gamma_multiply(backdrop.2)).is_some(),
        "the backdrop is not drawn at the alpha it was told ({})",
        backdrop.2
    );

    h.run_for(0.5);
    let cancel = text_rect(&shapes(&mut h), "Cancel").ok_or_else(|| fail("no Cancel"))?;
    h.tap(cancel.center());
    told.borrow_mut().clear();
    h.run_for(0.5);
    let told = told.borrow();
    assert!(
        told.iter()
            .any(|t| t.0 == PromptPiece::Card && t.3 && t.2 > 0.0 && t.2 < 1.0),
        "the card is not told it is leaving: {told:?}"
    );
    Ok(())
}

/// **Each painter draws its own modal only**: the lock screen's is not called for an unlock
/// prompt, nor the prompt's for the lock screen.
#[test]
fn each_painter_draws_only_its_own_modal() -> fairing::Result<()> {
    let (locks, prompts) = (Rc::new(Cell::new(0_u32)), Rc::new(Cell::new(0_u32)));
    let (l, p) = (Rc::clone(&locks), Rc::clone(&prompts));
    let mut h = shell_with(true, move |b| {
        b.lock_screen_painter(move |_: &egui::Painter, _: &mut LockScreenCx<'_>| {
            l.set(l.get() + 1);
        })
        .unlock_prompt_painter(move |_: &egui::Painter, _: &mut UnlockPromptCx<'_>| {
            p.set(p.get() + 1);
        })
    })?;
    assert_eq!((locks.get(), prompts.get()), (0, 0), "nothing is up");
    prompted(&mut h)?;
    assert!(prompts.get() > 0);
    assert_eq!(
        locks.get(),
        0,
        "the lock screen painter drew an unlock prompt"
    );
    h.shell.back();
    h.frames(2);
    let before = prompts.get();
    locked(&mut h)?;
    h.frames(2);
    assert!(locks.get() > 0);
    assert_eq!(
        prompts.get(),
        before,
        "the prompt painter drew the lock screen"
    );
    Ok(())
}
