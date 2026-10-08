//! **Drawing the recent screens yourself** — rung 5 of the override ladder.
//! A card painter draws each card, a ground painter what the cards stand on; the shell
//! keeps the deck, the drag, the taps, the throw, the split buttons and "Close all".
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::access::Subject;
use fairing::testing::Harness;
use fairing::workspace::{DrawnCard, RecentCardCx, RecentsGroundCx, RecentsOver};
use fairing::{
    screen, ColorRole, Cx, LaunchAction, Level, Shell, ShellBuilder, ShellConfig, ShellEvent,
};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// The painters' own fills — colours nothing built in uses.
const CARD: egui::Color32 = egui::Color32::from_rgb(141, 3, 59);
const GROUND: egui::Color32 = egui::Color32::from_rgb(3, 141, 59);

fn config(reduce: bool) -> ShellConfig {
    let mut config = ShellConfig::default();
    config.motion.reduce = reduce;
    config
}

/// A shell on a fixed clock with screens `a`, `b` and `c` titled `A`, `B` and `C`, built from
/// `config` with `build`.
fn shell_with(
    config: ShellConfig,
    build: impl FnOnce(ShellBuilder) -> ShellBuilder,
) -> fairing::Result<Harness> {
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::from_builder(move |ctx| {
        build(Shell::builder(config).services(services)).build(ctx)
    })?;
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

/// `a` then `b` opened, each from home — two tasks, `b` on show.
fn two_tasks(h: &mut Harness) {
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    h.shell.home();
    h.frames(3);
    h.shell.launch(LaunchAction::open("b"));
    h.frames(3);
}

fn recents(h: &mut Harness) {
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
}

/// The card of the task whose root is `id`, as the shell drew it last frame.
fn card_of(h: &Harness, id: &str) -> fairing::Result<DrawnCard> {
    let key = h
        .shell
        .workspace()
        .tasks()
        .iter()
        .find(|t| t.root_id() == Some(id))
        .and_then(|t| t.iter().next().map(fairing::workspace::Instance::id))
        .ok_or_else(|| fail(format!("no task `{id}`")))?;
    h.shell
        .workspace()
        .overview_cards_drawn()
        .iter()
        .copied()
        .find(|c| c.key == key)
        .ok_or_else(|| fail(format!("no card for `{id}`")))
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

/// Whether a text reading exactly `wanted` was drawn.
fn wrote(shapes: &[egui::Shape], wanted: &str) -> bool {
    shapes
        .iter()
        .any(|shape| matches!(shape, egui::Shape::Text(text) if text.galley.text() == wanted))
}

/// What a card painter was told, once.
#[derive(Debug, Clone)]
struct ToldCard {
    rect: egui::Rect,
    title: String,
    when: String,
    level: Option<String>,
    current: bool,
    split_button: Option<egui::Rect>,
    alpha: f32,
}

type Told<T> = Rc<RefCell<Vec<T>>>;

/// A card painter that fills the card with [`CARD`] at its fade, writes "painted" and the title,
/// and keeps what it was told.
fn card_painter(told: &Told<ToldCard>) -> impl FnMut(&egui::Painter, &mut RecentCardCx<'_>) {
    let told = Rc::clone(told);
    move |painter: &egui::Painter, card: &mut RecentCardCx<'_>| {
        painter.rect_filled(card.rect, 0.0, CARD.gamma_multiply(card.alpha));
        painter.text(
            card.rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("painted {}", card.title),
            egui::FontId::proportional(16.0),
            egui::Color32::WHITE,
        );
        told.borrow_mut().push(ToldCard {
            rect: card.rect,
            title: card.title.to_owned(),
            when: card.when.to_owned(),
            level: card.level.map(str::to_owned),
            current: card.current,
            split_button: card.split_button,
            alpha: card.alpha,
        });
    }
}

/// The last thing a painter was told about the card titled `title`.
fn told_card(told: &Told<ToldCard>, title: &str) -> fairing::Result<ToldCard> {
    told.borrow()
        .iter()
        .rev()
        .find(|t| t.title == title)
        .cloned()
        .ok_or_else(|| fail(format!("the painter was not called for `{title}`")))
}

/// **A card painter draws each card**, told where it is, what it says and whether it is the task
/// that was on show; the built-in card is not drawn under it, and a tap still brings its task
/// forward.
#[test]
fn a_card_painter_draws_each_card() -> fairing::Result<()> {
    let mut plain = shell_with(config(true), |b| b)?;
    two_tasks(&mut plain);
    recents(&mut plain);
    let built_in = shapes(&mut plain);
    let surface = plain.shell.theme().color(ColorRole::Surface);

    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(config(true), |b| {
        b.recents_card_painter(card_painter(&told))
    })?;
    two_tasks(&mut h);
    recents(&mut h);
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    for (id, title, current) in [("a", "A", false), ("b", "B", true)] {
        let card = card_of(&h, id)?;
        let seen = told_card(&told, title)?;
        assert!(same(seen.rect, card.rect), "{seen:?} vs {card:?}");
        assert_eq!(seen.current, current, "{seen:?}");
        assert_eq!(seen.split_button, card.beside, "{seen:?} vs {card:?}");
        assert_eq!(seen.when, "Just now");
        assert_eq!(seen.level, None);
        assert!((seen.alpha - 1.0).abs() < 1e-3, "{seen:?}");
        assert!(filled(&drawn, card.rect, CARD), "`{title}` is not painted");
        assert!(wrote(&drawn, &format!("painted {title}")));
        assert!(
            filled(&built_in, card.rect, surface) && !filled(&drawn, card.rect, surface),
            "the built-in card `{title}` is drawn under the painter"
        );
        assert!(
            wrote(&built_in, title) && !wrote(&drawn, title),
            "the built-in title `{title}` is written under the painter"
        );
    }
    let a = card_of(&h, "a")?;
    assert!(a.beside.is_some(), "`a` can go beside `b`");
    assert!(card_of(&h, "b")?.beside.is_none(), "`b` is on show");
    let content = h.shell.workspace().last_content();
    let at = a.rect.intersect(content).center();
    h.tap(at);
    h.frames(3);
    let ws = h.shell.workspace();
    assert!(!ws.is_overview_open());
    assert_eq!(
        ws.focused().map(fairing::workspace::Instance::decl_id),
        Some("a"),
        "the tapped card did not bring its task forward"
    );
    Ok(())
}

/// **A card painter is told the level a card's screen needs**, apart from when it was used — the
/// built-in card writes them on one line.
#[test]
fn a_card_painter_is_told_the_level_its_screen_needs() -> fairing::Result<()> {
    let mut gated = fairing::testing::access_config(&["viewer", "maintainer"], Some("bottom"));
    gated
        .access
        .gates
        .insert("a".to_owned(), "maintainer".to_owned());
    gated.motion.reduce = true;
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(gated, |b| b.recents_card_painter(card_painter(&told)))?;
    h.shell.handle().set_subject(Subject {
        level: Level(1),
        ..Subject::default()
    });
    h.frames(2);
    two_tasks(&mut h);
    recents(&mut h);
    told.borrow_mut().clear();
    h.frame();
    let a = told_card(&told, "A")?;
    assert_eq!(a.level.as_deref(), Some("maintainer"), "{a:?}");
    assert_eq!(a.when, "Just now", "{a:?}");
    assert_eq!(told_card(&told, "B")?.level, None, "`b` is everyone's");
    Ok(())
}

/// **The split button stays the shell's**: it is drawn over a painted card where the painter was
/// told, and a tap on it puts the task beside the one on show.
#[test]
fn the_split_button_is_drawn_over_a_painted_card() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(config(true), |b| {
        b.recents_card_painter(card_painter(&told))
    })?;
    two_tasks(&mut h);
    recents(&mut h);
    let a = card_of(&h, "a")?;
    let content = h.shell.workspace().last_content();
    if !a
        .beside
        .is_some_and(|button| content.contains(button.center()))
    {
        // Off to the side: bring the card in first, as a finger would.
        let from = content.center();
        let dx = a.rect.center().x - content.center().x;
        h.drag(from, from - egui::vec2(dx, 0.0), 8);
        h.frames(30);
    }
    told.borrow_mut().clear();
    h.frame();
    let a = card_of(&h, "a")?;
    let button = a.beside.ok_or_else(|| fail("`a` has no split button"))?;
    assert_eq!(told_card(&told, "A")?.split_button, Some(button));
    h.tap(button.center());
    h.frames(4);
    let ws = h.shell.workspace();
    assert!(ws.is_split() && !ws.is_overview_open());
    assert_eq!(ws.pane_task(1).and_then(|t| t.root_id()), Some("a"));
    Ok(())
}

/// **A card thrown up is drawn where the finger has it**, fading as it goes — and thrown far
/// enough, its task ends as it always did.
#[test]
fn a_thrown_card_is_painted_where_the_finger_has_it() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(config(true), |b| {
        b.recents_card_painter(card_painter(&told))
    })?;
    two_tasks(&mut h);
    recents(&mut h);
    let b = card_of(&h, "b")?;
    let from = b.rect.center();
    h.press(from);
    h.frame();
    let mut at = from;
    for _ in 0..8 {
        at -= egui::vec2(0.0, 25.0);
        h.move_to(at);
        h.frame();
    }
    told.borrow_mut().clear();
    h.frame();
    let seen = told_card(&told, "B")?;
    let lifted = b.rect.min.y - seen.rect.min.y;
    assert!(
        lifted > 150.0,
        "the card did not follow the finger: {seen:?}"
    );
    assert!(seen.alpha < 0.95, "a card going up does not fade: {seen:?}");
    assert!(
        seen.split_button.is_none(),
        "a card in the hand has no button"
    );
    h.release(at);
    h.frames(4);
    assert!(h.shell.workspace().find("b").is_none(), "`b` did not end");
    let gone: Vec<String> = h
        .shell
        .poll_events()
        .into_iter()
        .filter_map(|e| match e {
            ShellEvent::ScreenClosed { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert_eq!(gone, vec!["b".to_owned()]);
    Ok(())
}

/// **A card painter is told the cards' fade, and draws at it** — not at its square, as it would
/// on a painter faded too.
#[test]
fn a_card_painter_is_told_the_fade_and_draws_at_it() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(config(false), |b| {
        b.recents_card_painter(card_painter(&told))
    })?;
    two_tasks(&mut h);
    h.frames(30);
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(2);
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let a = told_card(&told, "A")?;
    assert!(a.alpha > 0.05 && a.alpha < 0.95, "not mid-fade: {a:?}");
    let faded = CARD.gamma_multiply(a.alpha);
    assert!(
        filled(&drawn, a.rect, faded),
        "the card is not drawn at the alpha it was told ({})",
        a.alpha
    );
    Ok(())
}

/// What a ground painter was told, once.
type ToldGround = (egui::Rect, RecentsOver, f32);

/// A ground painter that fills the ground with [`GROUND`] at its fade and keeps what it was told.
fn ground_painter(told: &Told<ToldGround>) -> impl FnMut(&egui::Painter, &mut RecentsGroundCx<'_>) {
    let told = Rc::clone(told);
    move |painter: &egui::Painter, ground: &mut RecentsGroundCx<'_>| {
        painter.rect_filled(ground.rect, 0.0, GROUND.gamma_multiply(ground.alpha));
        told.borrow_mut()
            .push((ground.rect, ground.over, ground.alpha));
    }
}

/// **Over a task the ground is whole, behind the shrinking screen** — the painter is told so, and
/// the built-in plain ground is not drawn.
#[test]
fn a_ground_painter_draws_under_the_cards_over_a_task() -> fairing::Result<()> {
    let mut plain = shell_with(config(true), |b| b)?;
    two_tasks(&mut plain);
    recents(&mut plain);
    let built_in = shapes(&mut plain);
    let background = plain.shell.theme().color(ColorRole::Background);

    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(config(true), |b| {
        b.recents_ground_painter(ground_painter(&told))
    })?;
    two_tasks(&mut h);
    assert!(told.borrow().is_empty(), "drawn with no recent screens up");
    recents(&mut h);
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let content = h.shell.workspace().last_content();
    let last = told
        .borrow()
        .last()
        .copied()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert!(same(last.0, content), "{last:?} vs {content:?}");
    assert_eq!(last.1, RecentsOver::Task);
    assert!((last.2 - 1.0).abs() < 1e-3, "{last:?}");
    assert!(filled(&drawn, content, GROUND), "no ground");
    assert!(
        filled(&built_in, content, background) && !filled(&drawn, content, background),
        "the built-in ground is drawn under the painter"
    );
    h.shell.back();
    h.frames(3);
    told.borrow_mut().clear();
    h.frames(2);
    assert!(told.borrow().is_empty(), "drawn after the cards went");
    Ok(())
}

/// **Over the desktop the ground fades in over it** — the painter is told so, and what it draws
/// comes out at the alpha it was told.
#[test]
fn a_ground_painter_fades_in_over_the_desktop() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(config(false), |b| {
        b.recents_ground_painter(ground_painter(&told))
    })?;
    two_tasks(&mut h);
    h.shell.home();
    h.frames(40);
    assert!(h.shell.workspace().is_home());
    h.shell.launch(LaunchAction::OpenOverview);
    h.frames(3);
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let last = told
        .borrow()
        .last()
        .copied()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert_eq!(last.1, RecentsOver::Desktop);
    assert!(last.2 > 0.05 && last.2 < 0.95, "not mid-fade: {last:?}");
    assert!(
        filled(&drawn, last.0, GROUND.gamma_multiply(last.2)),
        "the ground is not drawn at the alpha it was told ({})",
        last.2
    );
    h.frames(60);
    told.borrow_mut().clear();
    h.frame();
    let whole = told.borrow().last().map(|t| t.2);
    assert!(whole.is_some_and(|a| (a - 1.0).abs() < 1e-3), "{whole:?}");
    Ok(())
}

/// **A screen lifted from the bottom edge stands on the same ground** — the recent screens it may
/// become (gesture navigation).
#[test]
fn a_lifted_screen_stands_on_the_ground() -> fairing::Result<()> {
    let mut gestures = config(true);
    "gesture".clone_into(&mut gestures.nav_bar.style);
    let told = Rc::new(RefCell::new(Vec::new()));
    let mut h = shell_with(gestures, |b| {
        b.recents_ground_painter(ground_painter(&told))
    })?;
    h.shell.launch(LaunchAction::open("a"));
    h.frames(3);
    assert!(told.borrow().is_empty(), "drawn under a screen at rest");
    let from = egui::pos2(400.0, h.screen_rect().max.y - 6.0);
    h.press(from);
    h.frame();
    let mut at = from;
    for _ in 0..6 {
        at -= egui::vec2(0.0, 20.0);
        h.move_to(at);
        h.frame();
    }
    let last = told
        .borrow()
        .last()
        .copied()
        .ok_or_else(|| fail("the painter was not called under the lift"))?;
    assert_eq!(last.1, RecentsOver::Task);
    h.release(at);
    h.frames(4);
    Ok(())
}
