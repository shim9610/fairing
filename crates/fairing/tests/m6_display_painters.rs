//! **Drawing the rows, the displays and the cards yourself** — the last part of the widget
//! painters: the list row, the progress bar and ring, the meter, the
//! lamp, the badge and the two cards. Each painter is told the widget's look; each widget keeps
//! its press, its value and its motion, and a switch or a button on it is still drawn as its own
//! kind.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::testing::Harness;
use fairing::widgets::{
    BadgeLook, BadgeTone, BadgeValue, CountBadge, FeatureCard, FeatureCardLook, IconButtonLook,
    LampLook, LampState, Limit, ListRow, MediaCard, MediaCardLook, MediaShape, Meter, MeterLook,
    ProgressBar, ProgressBarLook, ProgressFill, ProgressRing, ProgressRingLook, RowLook,
    StatusLamp, SwitchLook, WidgetPainters,
};
use fairing::{icon, screen, Cx, LaunchAction, Shell, ShellConfig};
use std::cell::RefCell;
use std::ops::RangeInclusive;
use std::rc::Rc;
use std::time::Duration;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// A painter's own fill — a colour nothing built in uses.
const MARK: egui::Color32 = egui::Color32::from_rgb(141, 3, 59);

/// How long the bar and the meter wait for word before they are stale.
const STALE: Duration = Duration::from_secs(5);

/// What the gallery's widgets hold and report.
#[derive(Debug)]
struct Displays {
    /// The row's switch.
    on: bool,
    /// The meter's reading.
    meter: f32,
    /// The verdict the meter handed back.
    verdict: Option<LampState>,
    /// Whether the meter said its reading is old.
    meter_stale: bool,
    media_opened: usize,
    media_action: usize,
    feature_opened: usize,
    feature_action: usize,
}

impl Default for Displays {
    fn default() -> Self {
        Self {
            on: false,
            meter: 72.0,
            verdict: None,
            meter_stale: false,
            media_opened: 0,
            media_action: 0,
            feature_opened: 0,
            feature_action: 0,
        }
    }
}

/// The screen: one of each, top to bottom. Every word on it is written once.
fn gallery(state: Rc<RefCell<Displays>>) -> impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static {
    move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        let mut s = state.borrow_mut();
        let mut w = cx.widgets();
        let row = ListRow::new("Pump")
            .subtitle("Line 2")
            .trailing("On")
            .trailing_switch(s.on)
            .show(ui, &mut w);
        if row.clicked() {
            s.on = !s.on;
        }
        let _ = ProgressBar::determinate(0.4)
            .trailing("40 %")
            .stale_after(STALE)
            .show(ui, &mut w);
        let _ = ProgressBar::indeterminate()
            .id_salt("sweep")
            .show(ui, &mut w);
        let _ = ProgressRing::determinate(0.25)
            .value_text("25")
            .label("Done")
            .stale_after(STALE)
            .show(ui, &mut w);
        let limits = [Limit::high(80.0, LampState::Fault).label("Hot")];
        let reading = Meter::new(s.meter, 0.0..=100.0)
            .normal(40.0..=60.0)
            .setpoint(50.0)
            .limits(&limits)
            .readout("72 °C")
            .stale_after(STALE)
            .show(ui, &mut w);
        s.verdict = Some(reading.state);
        s.meter_stale = reading.stale;
        let _ = StatusLamp::new(LampState::Warn, "Door").show(ui, &mut w);
        let _ = CountBadge::count(120).show(ui, &mut w);
        let media = MediaCard::new("Latte")
            .subtitle("Oat")
            .value("4.50")
            .shape(MediaShape::Row)
            .action(icon::PLUS, "Add")
            .show(ui, &mut w);
        if media.action {
            s.media_action += 1;
        } else if media.response.clicked() {
            s.media_opened += 1;
        }
        let feature = FeatureCard::new("Rinse")
            .body("Run the clean cycle")
            .art(icon::REFRESH)
            .action(icon::ARROW_RIGHT, "Start")
            .show(ui, &mut w);
        if feature.action {
            s.feature_action += 1;
        } else if feature.response.clicked() {
            s.feature_opened += 1;
        }
    }
}

/// A tall shell on a fixed clock with the gallery open, motion reduced, built with `painters`.
fn shell_with(painters: WidgetPainters) -> fairing::Result<(Harness, Rc<RefCell<Displays>>)> {
    let mut config = ShellConfig::default();
    config.motion.reduce = true;
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .services(services)
            .widget_painters(painters)
            .build(ctx)
    })?
    .with_size(1024.0, 2400.0);
    let state = Rc::new(RefCell::new(Displays::default()));
    h.shell.add(screen("displays", gallery(Rc::clone(&state))));
    h.shell.launch(LaunchAction::open("displays"));
    h.frames(3);
    Ok((h, state))
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

/// Whether a text reading exactly `wanted` was drawn.
fn wrote(shapes: &[egui::Shape], wanted: &str) -> bool {
    shapes
        .iter()
        .any(|shape| matches!(shape, egui::Shape::Text(text) if text.galley.text() == wanted))
}

type Told<T> = Rc<RefCell<Vec<T>>>;

fn told<T>() -> Told<T> {
    Rc::new(RefCell::new(Vec::new()))
}

fn last<T: Clone>(told: &Told<T>) -> fairing::Result<T> {
    told.borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called"))
}

/// What a row painter was told.
#[derive(Debug, Clone)]
struct ToldRow {
    rect: egui::Rect,
    title: String,
    subtitle: Option<String>,
    value: Option<String>,
    switch: Option<egui::Rect>,
    chevron: bool,
    pressed: bool,
}

/// **A row painter draws the row and is told what it says**; its switch is still drawn as a
/// switch — by the switch painter here — in the slot the row names, and a tap still flips it.
#[test]
fn a_row_painter_draws_the_row_and_its_switch_still_flips() -> fairing::Result<()> {
    let rows: Told<ToldRow> = told();
    let switches: Told<egui::Rect> = told();
    let (record, record_switch) = (Rc::clone(&rows), Rc::clone(&switches));
    let painters = WidgetPainters::new()
        .row(move |painter: &egui::Painter, row: &mut RowLook<'_>| {
            painter.rect_filled(row.rect, 0.0, MARK);
            record.borrow_mut().push(ToldRow {
                rect: row.rect,
                title: row.title.to_owned(),
                subtitle: row.subtitle.map(str::to_owned),
                value: row.value.map(str::to_owned),
                switch: row.switch,
                chevron: row.chevron,
                pressed: row.pressed,
            });
        })
        .switch(move |_: &egui::Painter, switch: &mut SwitchLook<'_>| {
            record_switch.borrow_mut().push(switch.rect);
        });
    let (mut h, state) = shell_with(painters)?;
    rows.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let row = last(&rows)?;
    assert_eq!(row.title, "Pump");
    assert_eq!(row.subtitle.as_deref(), Some("Line 2"));
    assert_eq!(row.value.as_deref(), Some("On"));
    assert!(row.chevron && !row.pressed, "{row:?}");
    let slot = row
        .switch
        .ok_or_else(|| fail("not told where the switch goes"))?;
    assert!(row.rect.contains_rect(slot), "{row:?}");
    assert!(
        !wrote(&drawn, "Pump") && !wrote(&drawn, "On"),
        "the built-in row is drawn under the painter"
    );
    let switch = last(&switches)?;
    assert!(
        same(switch, slot),
        "the switch is not drawn where the row said: {switch:?} against {slot:?}"
    );
    // The slot is where the built-in row puts its switch.
    let built_in: Told<egui::Rect> = told();
    let record_built_in = Rc::clone(&built_in);
    let (mut plain, _) = shell_with(WidgetPainters::new().switch(
        move |_: &egui::Painter, switch: &mut SwitchLook<'_>| {
            record_built_in.borrow_mut().push(switch.rect);
        },
    ))?;
    plain.frame();
    let at = last(&built_in)?;
    assert!(
        same(at, slot),
        "the built-in row puts its switch at {at:?}, the painter is told {slot:?}"
    );
    h.press(row.rect.center());
    h.frame();
    assert!(last(&rows)?.pressed, "not told the press");
    h.release(row.rect.center());
    h.frames(2);
    assert!(state.borrow().on, "the tap did not flip the switch");
    Ok(())
}

/// **A progress bar painter is told what the fill covers and where the read-out goes**; the
/// built-in read-out is not written, and an indeterminate bar under reduced motion is told it
/// pulses.
#[test]
fn a_progress_bar_painter_is_told_the_fill_and_the_readout() -> fairing::Result<()> {
    type ToldBar = (egui::Rect, ProgressFill, Option<(String, egui::Rect)>);
    let bars: Told<ToldBar> = told();
    let record = Rc::clone(&bars);
    let painters = WidgetPainters::new().progress_bar(
        move |painter: &egui::Painter, bar: &mut ProgressBarLook<'_>| {
            painter.rect_filled(bar.track, 0.0, MARK);
            let readout = bar.readout.map(|(text, at)| (text.to_owned(), at));
            record.borrow_mut().push((bar.track, bar.fill, readout));
        },
    );
    let (mut h, _) = shell_with(painters)?;
    h.frames(30);
    bars.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let frame = bars.borrow().clone();
    let (track, fill, readout) = frame
        .iter()
        .find(|bar| bar.2.is_some())
        .cloned()
        .ok_or_else(|| fail("the determinate bar was not painted"))?;
    match fill {
        ProgressFill::To(t) => assert!((t - 0.4).abs() < 0.01, "the fill is at {t}"),
        other => return Err(fail(format!("told {other:?} for a determinate bar"))),
    }
    let (text, at) = readout.ok_or_else(|| fail("no read-out"))?;
    assert_eq!(text, "40 %");
    assert!(track.max.x < at.min.x, "the track runs under the read-out");
    assert!(
        !wrote(&drawn, "40 %"),
        "the built-in read-out is written under the painter"
    );
    assert!(
        frame
            .iter()
            .any(|bar| matches!(bar.1, ProgressFill::Pulse(_))),
        "the indeterminate bar is not told it pulses: {frame:?}"
    );
    Ok(())
}

/// **A progress ring painter is told the arc, the fill and the words in the hole**; the arc of a
/// ring starts at twelve o'clock, and the built-in number is not written.
#[test]
fn a_progress_ring_painter_is_told_the_arc_and_the_number() -> fairing::Result<()> {
    type ToldRing = (
        egui::Pos2,
        f32,
        f32,
        ProgressFill,
        Option<String>,
        Option<String>,
        egui::Pos2,
    );
    let rings: Told<ToldRing> = told();
    let record = Rc::clone(&rings);
    let painters = WidgetPainters::new().progress_ring(
        move |painter: &egui::Painter, ring: &mut ProgressRingLook<'_>| {
            painter.circle_filled(ring.center, ring.radius, MARK);
            record.borrow_mut().push((
                ring.center,
                ring.radius,
                ring.sweep,
                ring.fill,
                ring.value_text.map(str::to_owned),
                ring.label.map(str::to_owned),
                ring.point(0.0),
            ));
        },
    );
    let (mut h, _) = shell_with(painters)?;
    h.frames(30);
    rings.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (center, radius, sweep, fill, number, word, top) = last(&rings)?;
    assert!(
        (sweep - std::f32::consts::TAU).abs() < 1e-3,
        "a ring is a whole turn"
    );
    assert!(
        (top.x - center.x).abs() < 0.5 && (center.y - radius - top.y).abs() < 0.5,
        "the arc does not start at twelve o'clock: {top:?} about {center:?}"
    );
    match fill {
        ProgressFill::To(t) => assert!((t - 0.25).abs() < 0.01, "the fill is at {t}"),
        other => return Err(fail(format!("told {other:?} for a determinate ring"))),
    }
    assert_eq!(number.as_deref(), Some("25"));
    assert_eq!(word.as_deref(), Some("Done"));
    assert!(
        !wrote(&drawn, "25") && !wrote(&drawn, "Done"),
        "the built-in words are written under the painter"
    );
    Ok(())
}

/// What a meter painter was told.
#[derive(Debug, Clone)]
struct ToldMeter {
    pointer: Option<f32>,
    normal: Option<RangeInclusive<f32>>,
    setpoint: Option<f32>,
    limits: Vec<(f32, bool, Option<String>)>,
    readout: Option<String>,
    verdict: LampState,
    middle: f32,
    track_middle: f32,
}

/// **A meter painter is told the pointer, the band, the limits and the verdict**; the reading
/// still hands back the verdict, and a value past the limit is told `Fault`.
#[test]
fn a_meter_painter_is_told_the_pointer_and_the_verdict() -> fairing::Result<()> {
    let meters: Told<ToldMeter> = told();
    let record = Rc::clone(&meters);
    let painters =
        WidgetPainters::new().meter(move |painter: &egui::Painter, meter: &mut MeterLook<'_>| {
            painter.rect_filled(meter.track, 0.0, MARK);
            record.borrow_mut().push(ToldMeter {
                pointer: meter.pointer,
                normal: meter.normal.clone(),
                setpoint: meter.setpoint,
                limits: meter
                    .limits
                    .iter()
                    .map(|l| (l.at(), l.is_high(), l.caption().map(str::to_owned)))
                    .collect(),
                readout: meter.readout.map(|(text, _)| text.to_owned()),
                verdict: meter.verdict,
                middle: meter.x(50.0),
                track_middle: meter.track.center().x,
            });
        });
    let (mut h, state) = shell_with(painters)?;
    h.frames(30);
    meters.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let meter = last(&meters)?;
    let pointer = meter.pointer.ok_or_else(|| fail("no pointer"))?;
    assert!((pointer - 72.0).abs() < 0.1, "the pointer is at {pointer}");
    assert_eq!(meter.normal, Some(40.0..=60.0));
    assert_eq!(meter.setpoint, Some(50.0));
    assert_eq!(meter.limits, vec![(80.0, true, Some("Hot".to_owned()))]);
    assert_eq!(meter.readout.as_deref(), Some("72 °C"));
    assert_eq!(meter.verdict, LampState::Ok);
    assert!((meter.middle - meter.track_middle).abs() < 0.5, "{meter:?}");
    assert!(
        !wrote(&drawn, "72 °C") && !wrote(&drawn, "Hot"),
        "the built-in words are written under the painter"
    );
    state.borrow_mut().meter = 85.0;
    h.frames(2);
    assert_eq!(last(&meters)?.verdict, LampState::Fault);
    assert_eq!(state.borrow().verdict, Some(LampState::Fault));
    Ok(())
}

/// **A lamp painter is told the state and the word**, the disc before the word.
#[test]
fn a_lamp_painter_is_told_the_state_and_the_word() -> fairing::Result<()> {
    type ToldLamp = (egui::Rect, egui::Rect, String, LampState);
    let lamps: Told<ToldLamp> = told();
    let record = Rc::clone(&lamps);
    let painters =
        WidgetPainters::new().lamp(move |painter: &egui::Painter, lamp: &mut LampLook<'_>| {
            painter.rect_filled(lamp.lens, 0.0, MARK);
            record
                .borrow_mut()
                .push((lamp.lens, lamp.word, lamp.label.to_owned(), lamp.state));
        });
    let (mut h, _) = shell_with(painters)?;
    lamps.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (lens, word, label, state) = last(&lamps)?;
    assert_eq!(label, "Door");
    assert_eq!(state, LampState::Warn);
    assert!((lens.width() - lens.height()).abs() < 0.5, "{lens:?}");
    assert!(lens.max.x <= word.min.x, "{lens:?} {word:?}");
    assert!(
        !wrote(&drawn, "Door"),
        "the built-in word is written under the painter"
    );
    Ok(())
}

/// **A badge painter is told what the badge says** — a count past the cap as the built-in badge
/// writes it.
#[test]
fn a_badge_painter_is_told_what_the_badge_says() -> fairing::Result<()> {
    type ToldBadge = (bool, String, BadgeTone);
    let badges: Told<ToldBadge> = told();
    let record = Rc::clone(&badges);
    let painters =
        WidgetPainters::new().badge(move |painter: &egui::Painter, badge: &mut BadgeLook<'_>| {
            painter.rect_filled(badge.rect, 0.0, MARK);
            record.borrow_mut().push((
                badge.value == BadgeValue::Count(120),
                badge.text.to_owned(),
                badge.tone,
            ));
        });
    let (mut h, _) = shell_with(painters)?;
    badges.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (count, text, tone) = last(&badges)?;
    assert!(count, "not told the count");
    assert_eq!(text, "99+");
    assert_eq!(tone, BadgeTone::Alert);
    assert!(
        !wrote(&drawn, "99+"),
        "the built-in badge is drawn under the painter"
    );
    Ok(())
}

/// What a media card painter was told.
#[derive(Debug, Clone)]
struct ToldMedia {
    rect: egui::Rect,
    picture: egui::Rect,
    text: egui::Rect,
    words: (String, Option<String>, Option<String>),
    action: Option<egui::Rect>,
}

/// **A media card painter draws the card**; its corner action is still an icon button, drawn in
/// the slot the card names, and a tap on it still adds while a tap on the card opens it.
#[test]
fn a_media_card_painter_draws_the_card_and_its_action_still_adds() -> fairing::Result<()> {
    let cards: Told<ToldMedia> = told();
    let buttons: Told<egui::Rect> = told();
    let (record, record_button) = (Rc::clone(&cards), Rc::clone(&buttons));
    let painters = WidgetPainters::new()
        .media_card(
            move |painter: &egui::Painter, card: &mut MediaCardLook<'_>| {
                painter.rect_filled(card.rect, 0.0, MARK);
                record.borrow_mut().push(ToldMedia {
                    rect: card.rect,
                    picture: card.picture,
                    text: card.text,
                    words: (
                        card.title.to_owned(),
                        card.subtitle.map(str::to_owned),
                        card.value.map(str::to_owned),
                    ),
                    action: card.action,
                });
            },
        )
        .icon_button(move |_: &egui::Painter, button: &mut IconButtonLook<'_>| {
            record_button.borrow_mut().push(button.rect);
        });
    let (mut h, state) = shell_with(painters)?;
    cards.borrow_mut().clear();
    buttons.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let card = last(&cards)?;
    assert_eq!(
        card.words,
        (
            "Latte".to_owned(),
            Some("Oat".to_owned()),
            Some("4.50".to_owned())
        )
    );
    assert!(
        card.rect.contains_rect(card.picture) && card.rect.contains_rect(card.text),
        "{card:?}"
    );
    assert!(
        card.picture.max.x <= card.text.min.x,
        "a row's picture leads: {card:?}"
    );
    let slot = card
        .action
        .ok_or_else(|| fail("not told where the action goes"))?;
    assert!(card.rect.contains_rect(slot), "{card:?}");
    assert!(
        buttons.borrow().iter().any(|&button| same(button, slot)),
        "the action is not drawn as an icon button in its slot"
    );
    assert!(
        !wrote(&drawn, "Latte") && !wrote(&drawn, "4.50"),
        "the built-in card is drawn under the painter"
    );
    h.tap(slot.center());
    h.frames(2);
    assert_eq!(state.borrow().media_action, 1, "the action did not add");
    assert_eq!(state.borrow().media_opened, 0);
    h.tap(card.text.center());
    h.frames(2);
    assert_eq!(state.borrow().media_opened, 1, "the card did not open");
    Ok(())
}

/// What a feature card painter was told.
#[derive(Debug, Clone)]
struct ToldFeature {
    rect: egui::Rect,
    text: egui::Rect,
    words: (String, Option<String>),
    art: Option<egui::Rect>,
    action: Option<egui::Rect>,
}

/// **A feature card painter draws the card**; its disc is still an icon button, and a tap on the
/// card still opens it.
#[test]
fn a_feature_card_painter_draws_the_card_and_a_tap_still_opens() -> fairing::Result<()> {
    let cards: Told<ToldFeature> = told();
    let record = Rc::clone(&cards);
    let painters = WidgetPainters::new().feature_card(
        move |painter: &egui::Painter, card: &mut FeatureCardLook<'_>| {
            painter.rect_filled(card.rect, 0.0, MARK);
            record.borrow_mut().push(ToldFeature {
                rect: card.rect,
                text: card.text,
                words: (card.title.to_owned(), card.body.map(str::to_owned)),
                art: card.art.map(|(_, at)| at),
                action: card.action,
            });
        },
    );
    let (mut h, state) = shell_with(painters)?;
    cards.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let card = last(&cards)?;
    assert_eq!(
        card.words,
        ("Rinse".to_owned(), Some("Run the clean cycle".to_owned()))
    );
    let art = card.art.ok_or_else(|| fail("not told the art"))?;
    assert!(
        art.min.x >= card.text.max.x,
        "the art is over the words: {card:?}"
    );
    let slot = card
        .action
        .ok_or_else(|| fail("not told where the disc goes"))?;
    assert!(card.rect.contains_rect(slot), "{card:?}");
    assert!(
        !wrote(&drawn, "Rinse") && !wrote(&drawn, "Run the clean cycle"),
        "the built-in card is drawn under the painter"
    );
    h.tap(card.text.center());
    h.frames(2);
    assert_eq!(state.borrow().feature_opened, 1, "the card did not open");
    h.tap(slot.center());
    h.frames(2);
    assert_eq!(state.borrow().feature_action, 1, "the disc did not press");
    Ok(())
}

/// **A bar, a ring and a meter that have heard nothing are told so**: past `stale_after` the bar
/// and the ring are told the breath their fill would take, and the meter that its reading is old.
#[test]
fn a_bar_a_ring_and_a_meter_that_heard_nothing_are_told_so() -> fairing::Result<()> {
    let bars: Told<Option<f32>> = told();
    let rings: Told<Option<f32>> = told();
    let meters: Told<bool> = told();
    let (record_bar, record_ring, record_meter) =
        (Rc::clone(&bars), Rc::clone(&rings), Rc::clone(&meters));
    let painters = WidgetPainters::new()
        .progress_bar(move |_: &egui::Painter, bar: &mut ProgressBarLook<'_>| {
            if bar.readout.is_some() {
                record_bar.borrow_mut().push(bar.stale);
            }
        })
        .progress_ring(move |_: &egui::Painter, ring: &mut ProgressRingLook<'_>| {
            record_ring.borrow_mut().push(ring.stale);
        })
        .meter(move |_: &egui::Painter, meter: &mut MeterLook<'_>| {
            record_meter.borrow_mut().push(meter.stale);
        });
    let (mut h, state) = shell_with(painters)?;
    assert_eq!(last(&bars)?, None, "a fresh bar is told it is stale");
    assert_eq!(last(&rings)?, None, "a fresh ring is told it is stale");
    assert!(!last(&meters)?, "a fresh meter is told it is stale");
    h.sleep(STALE.as_secs_f64() + 1.0);
    h.frames(2);
    let breath = last(&bars)?.ok_or_else(|| fail("the quiet bar is not told it is stale"))?;
    assert!(
        breath > 0.0 && breath <= 1.0,
        "the bar's breath is {breath}"
    );
    let breath = last(&rings)?.ok_or_else(|| fail("the quiet ring is not told it is stale"))?;
    assert!(
        breath > 0.0 && breath <= 1.0,
        "the ring's breath is {breath}"
    );
    assert!(last(&meters)?, "the quiet meter is not told it is stale");
    assert!(state.borrow().meter_stale, "the reading does not say so");
    Ok(())
}

/// **The shell's own settings rows are drawn by the row painter** — the list of settings is
/// rows like any other.
#[cfg(all(feature = "settings", feature = "mock"))]
#[test]
fn the_shells_settings_rows_are_drawn_by_the_row_painter() -> fairing::Result<()> {
    let titles: Told<String> = told();
    let record = Rc::clone(&titles);
    let painters =
        WidgetPainters::new().row(move |painter: &egui::Painter, row: &mut RowLook<'_>| {
            painter.rect_filled(row.rect, 0.0, MARK);
            record.borrow_mut().push(row.title.to_owned());
        });
    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(fairing::testing::single_level_access())
            .services(fairing::services::mock::services())
            .widget_painters(painters)
            .build(ctx)?;
        fairing::settings::add_all(&mut shell, &fairing::settings::SettingsConfig::default());
        Ok(shell)
    })?
    .with_size(480.0, 800.0);
    h.shell.launch(LaunchAction::open("settings.home"));
    h.frames(3);
    titles.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let told = titles.borrow().clone();
    assert!(
        told.len() >= 3,
        "the settings rows were not painted: {told:?}"
    );
    for title in &told {
        assert!(
            !wrote(&drawn, title),
            "{title:?} is written by the built-in row under the painter"
        );
    }
    Ok(())
}
