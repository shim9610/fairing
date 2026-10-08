//! **Drawing the widgets yourself** — rung 5 of the override ladder for the element layer.
//! A painter per kind of widget is told the widget's look and draws it; the
//! widget keeps its press, its value and its motion, and the shell lends the painters to every
//! widget it draws — its own prompt's among them.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::testing::{access_config, Harness};
use fairing::widgets::{
    BigButton, ButtonKind, ButtonLook, Checkbox, CheckboxLook, Chip, ChipLook, IconButton,
    IconButtonLook, RadioGroup, RadioLook, SegmentedControl, SegmentedLook, Switch, SwitchLook,
    WidgetPainters,
};
use fairing::{screen, ColorRole, Cx, LaunchAction, Shell, ShellConfig};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// A painter's own fill — a colour nothing built in uses.
const MARK: egui::Color32 = egui::Color32::from_rgb(141, 3, 59);

/// What the gallery's widgets hold and what they reported.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools)] // One value per widget, not a state machine.
struct Gallery {
    on: bool,
    checked: bool,
    radio: usize,
    segment: usize,
    chip: bool,
    clicks: u32,
    holds: u32,
    icon_clicks: u32,
    /// Give "Go" the keyboard focus on the next frame.
    focus_go: bool,
}

/// The screen: one of each widget, top to bottom.
fn gallery(state: Rc<RefCell<Gallery>>) -> impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static {
    move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        let mut s = state.borrow_mut();
        let mut w = cx.widgets();
        let go = BigButton::new("Go")
            .kind(ButtonKind::Primary)
            .show(ui, &mut w);
        if go.clicked() {
            s.clicks += 1;
        }
        if std::mem::take(&mut s.focus_go) {
            go.response.request_focus();
        }
        if BigButton::new("Hold")
            .long_press(Duration::from_millis(300))
            .show(ui, &mut w)
            .long_pressed()
        {
            s.holds += 1;
        }
        if BigButton::new("Off")
            .enabled(false)
            .show(ui, &mut w)
            .clicked()
        {
            s.clicks += 100;
        }
        if IconButton::new(fairing::icon::GAUGE, "Gauge")
            .show(ui, &mut w)
            .clicked()
        {
            s.icon_clicks += 1;
        }
        let _ = Switch::new(&mut s.on).show(ui, &mut w);
        let _ = Checkbox::new(&mut s.checked).show(ui, &mut w);
        let _ = RadioGroup::new(&mut s.radio, &["One", "Two"]).show(ui, &mut w);
        let segment = s.segment;
        if let Some(picked) = SegmentedControl::new(&["Left", "Right"], segment)
            .show(ui, &mut w)
            .picked
        {
            s.segment = picked;
        }
        let _ = Chip::new("Chip", &mut s.chip).show(ui, &mut w);
    }
}

/// A tall shell on a fixed clock with the gallery open, motion reduced, built with `painters`.
fn shell_with(painters: WidgetPainters) -> fairing::Result<(Harness, Rc<RefCell<Gallery>>)> {
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
    .with_size(1024.0, 1200.0);
    let state = Rc::new(RefCell::new(Gallery::default()));
    h.shell.add(screen("gallery", gallery(Rc::clone(&state))));
    h.shell.launch(LaunchAction::open("gallery"));
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

type Told<T> = Rc<RefCell<Vec<T>>>;

/// What a button painter was told, once.
#[derive(Debug, Clone)]
struct ToldButton {
    rect: egui::Rect,
    drawn: egui::Rect,
    label: String,
    kind: ButtonKind,
    pressed: bool,
    press: f32,
    enabled: bool,
    focused: bool,
    hold: Option<(egui::Rect, f32, bool)>,
}

fn button_painter(told: &Told<ToldButton>) -> impl FnMut(&egui::Painter, &mut ButtonLook<'_>) {
    let told = Rc::clone(told);
    move |painter: &egui::Painter, button: &mut ButtonLook<'_>| {
        painter.rect_filled(button.drawn, 0.0, MARK);
        painter.text(
            button.drawn.center(),
            egui::Align2::CENTER_CENTER,
            format!("painted {}", button.label),
            button.font.clone(),
            egui::Color32::WHITE,
        );
        told.borrow_mut().push(ToldButton {
            rect: button.rect,
            drawn: button.drawn,
            label: button.label.to_owned(),
            kind: button.kind,
            pressed: button.pressed,
            press: button.press,
            enabled: button.enabled,
            focused: button.focused,
            hold: button.hold.map(|h| (h.rect, h.progress, h.done)),
        });
    }
}

fn last_button(told: &Told<ToldButton>, label: &str) -> fairing::Result<ToldButton> {
    told.borrow()
        .iter()
        .rev()
        .find(|b| b.label == label)
        .cloned()
        .ok_or_else(|| fail(format!("the painter was not told of `{label}`")))
}

/// **A button painter draws every button**, told its label, kind and rects; the built-in face is
/// not drawn under it, and a tap still clicks it.
#[test]
fn a_button_painter_draws_the_button_and_the_tap_still_clicks() -> fairing::Result<()> {
    let (mut plain, _) = shell_with(WidgetPainters::new())?;
    let built_in = shapes(&mut plain);
    let primary = plain.shell.theme().color(ColorRole::Primary);

    let told = Rc::new(RefCell::new(Vec::new()));
    let (mut h, state) = shell_with(WidgetPainters::new().button(button_painter(&told)))?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let go = last_button(&told, "Go")?;
    assert_eq!(go.kind, ButtonKind::Primary);
    assert!(
        go.enabled && !go.pressed && !go.focused && go.hold.is_none(),
        "{go:?}"
    );
    assert!(
        same(go.rect, go.drawn) && go.press < 1e-3,
        "at rest: {go:?}"
    );
    assert!(filled(&drawn, go.drawn, MARK), "not painted");
    assert!(wrote(&drawn, "painted Go"));
    assert!(
        filled(&built_in, go.drawn, primary) && !filled(&drawn, go.drawn, primary),
        "the built-in face is drawn under the painter"
    );
    assert!(
        wrote(&built_in, "Go") && !wrote(&drawn, "Go"),
        "the built-in label is drawn under the painter"
    );
    h.press(go.rect.center());
    h.frame();
    let held = last_button(&told, "Go")?;
    assert!(held.pressed && held.press > 0.99, "{held:?}");
    assert!(held.drawn.width() > held.rect.width(), "the press grows it");
    h.release(go.rect.center());
    h.frames(2);
    assert_eq!(state.borrow().clicks, 1, "the painted button did not click");
    Ok(())
}

/// **A painted button is told it is off, and stays off**: a tap on a disabled one does nothing.
#[test]
fn a_disabled_button_is_told_so_and_takes_no_tap() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let (mut h, state) = shell_with(WidgetPainters::new().button(button_painter(&told)))?;
    let off = last_button(&told, "Off")?;
    assert!(!off.enabled, "{off:?}");
    assert!(last_button(&told, "Go")?.enabled);
    h.tap(off.rect.center());
    h.frames(2);
    assert_eq!(state.borrow().clicks, 0, "a disabled button clicked");
    Ok(())
}

/// **Focus is told**: the button with the keyboard focus is told so — the painter draws the
/// ring, the built-in one is not drawn.
#[test]
fn the_focused_widget_is_told_so() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let (mut h, state) = shell_with(WidgetPainters::new().button(button_painter(&told)))?;
    let none = told.borrow().iter().all(|b| !b.focused);
    assert!(none, "focused before anything asked");
    state.borrow_mut().focus_go = true;
    h.frames(2);
    told.borrow_mut().clear();
    h.frame();
    let focused: Vec<String> = told
        .borrow()
        .iter()
        .filter(|b| b.focused)
        .map(|b| b.label.clone())
        .collect();
    assert_eq!(focused, ["Go"]);
    Ok(())
}

/// **A long press is the button's**: the painter is told how far the hold has got and where the
/// built-in ring would be, and the hold completes as it always did.
#[test]
fn a_painted_button_still_takes_a_long_press() -> fairing::Result<()> {
    let told = Rc::new(RefCell::new(Vec::new()));
    let (mut h, state) = shell_with(WidgetPainters::new().button(button_painter(&told)))?;
    let hold = last_button(&told, "Hold")?;
    assert_eq!(hold.hold.map(|h| h.1), Some(0.0), "{hold:?}");
    h.press(hold.rect.center());
    h.frames(6);
    let half = last_button(&told, "Hold")?;
    let (ring, progress, done) = half.hold.ok_or_else(|| fail("no hold"))?;
    assert!(progress > 0.1 && progress < 0.9 && !done, "{half:?}");
    assert!(
        half.drawn.contains_rect(ring),
        "the ring sits on the button: {half:?}"
    );
    h.frames(20);
    let full = last_button(&told, "Hold")?;
    assert!(
        full.hold.is_some_and(|h| h.2),
        "not told it is done: {full:?}"
    );
    assert_eq!(state.borrow().holds, 1, "the hold did not complete");
    h.release(hold.rect.center());
    h.frames(2);
    Ok(())
}

/// **An icon button painter draws the disc and the glyph**; a tap still clicks it.
#[test]
fn an_icon_button_painter_draws_it_and_the_tap_still_clicks() -> fairing::Result<()> {
    type ToldIcon = (egui::Rect, egui::Rect, String, bool);
    let told: Told<ToldIcon> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().icon_button(
        move |painter: &egui::Painter, button: &mut IconButtonLook<'_>| {
            painter.rect_filled(button.disc, 0.0, MARK);
            record.borrow_mut().push((
                button.rect,
                button.disc,
                button.name.to_owned(),
                button.pressed,
            ));
        },
    );
    let (mut h, state) = shell_with(painters)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (rect, disc, name, pressed) = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert_eq!(name, "Gauge");
    assert!(!pressed && rect.contains_rect(disc), "{rect:?} {disc:?}");
    assert!(filled(&drawn, disc, MARK));
    h.tap(rect.center());
    h.frames(2);
    assert_eq!(state.borrow().icon_clicks, 1);
    Ok(())
}

/// **A switch painter is told where the knob is**, and the switch still flips on a tap.
#[test]
fn a_switch_painter_follows_the_knob_and_the_switch_still_flips() -> fairing::Result<()> {
    type ToldSwitch = (egui::Rect, egui::Rect, bool, f32);
    let told: Told<ToldSwitch> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().switch(
        move |painter: &egui::Painter, switch: &mut SwitchLook<'_>| {
            painter.rect_filled(switch.track, 0.0, MARK);
            record
                .borrow_mut()
                .push((switch.rect, switch.track, switch.on, switch.travel));
        },
    );
    let (mut h, state) = shell_with(painters)?;
    let (rect, track, on, travel) = told
        .borrow()
        .last()
        .copied()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert!(
        !on && travel < 1e-3 && rect.contains_rect(track),
        "{told:?}"
    );
    h.tap(rect.center());
    h.frames(2);
    assert!(state.borrow().on, "the painted switch did not flip");
    let last = told.borrow().last().copied();
    assert!(
        last.is_some_and(|(_, _, on, travel)| on && travel > 0.99),
        "{last:?}"
    );
    Ok(())
}

/// **A checkbox painter is told the box and how far it is checked**; a tap still checks it.
#[test]
fn a_checkbox_painter_draws_the_box_and_the_tap_still_checks() -> fairing::Result<()> {
    type ToldBox = (egui::Rect, egui::Rect, bool, f32);
    let told: Told<ToldBox> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters =
        WidgetPainters::new().checkbox(move |painter: &egui::Painter, b: &mut CheckboxLook<'_>| {
            painter.rect_filled(b.drawn, 0.0, MARK);
            record.borrow_mut().push((b.rect, b.drawn, b.on, b.mark));
        });
    let (mut h, state) = shell_with(painters)?;
    let (rect, _, on, mark) = told
        .borrow()
        .last()
        .copied()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert!(!on && mark < 1e-3);
    h.tap(rect.center());
    h.frames(2);
    assert!(state.borrow().checked, "the painted checkbox did not check");
    let last = told.borrow().last().copied();
    assert!(
        last.is_some_and(|(_, _, on, mark)| on && mark > 0.99),
        "{last:?}"
    );
    Ok(())
}

/// **A radio painter draws a group's marks**; the group still draws its labels and still picks
/// the row tapped.
#[test]
fn a_radio_painter_draws_a_groups_marks_and_the_group_still_picks() -> fairing::Result<()> {
    type ToldMark = (egui::Rect, bool, bool);
    let told: Told<ToldMark> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters =
        WidgetPainters::new().radio(move |painter: &egui::Painter, radio: &mut RadioLook<'_>| {
            painter.rect_filled(radio.drawn, 0.0, MARK);
            record
                .borrow_mut()
                .push((radio.rect, radio.selected, radio.focused));
        });
    let (mut h, state) = shell_with(painters)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let marks: Vec<ToldMark> = told.borrow().clone();
    let (first, second) = match marks.as_slice() {
        [first, second] => (*first, *second),
        _ => return Err(fail(format!("not two marks: {marks:?}"))),
    };
    assert!(first.1 && !second.1, "the first is selected: {marks:?}");
    assert!(
        marks.iter().all(|m| !m.2),
        "a group's mark is never focused"
    );
    assert!(
        wrote(&drawn, "One") && wrote(&drawn, "Two"),
        "the labels are the group's"
    );
    h.tap(second.0.center());
    h.frames(2);
    assert_eq!(state.borrow().radio, 1, "the group did not pick the row");
    Ok(())
}

/// **A segmented painter is told the labels, the selection and the band**, and a tap on a
/// segment still picks it.
#[test]
fn a_segmented_painter_draws_the_strip_and_a_tap_still_picks() -> fairing::Result<()> {
    type ToldStrip = (Vec<String>, usize, f32, egui::Rect, egui::Rect);
    let told: Told<ToldStrip> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().segmented(
        move |painter: &egui::Painter, strip: &mut SegmentedLook<'_>| {
            painter.rect_filled(strip.cell(strip.travel), 0.0, MARK);
            record.borrow_mut().push((
                strip.labels.iter().map(|l| (*l).to_owned()).collect(),
                strip.selected,
                strip.travel,
                strip.cell(0.0),
                strip.cell(1.0),
            ));
        },
    );
    let (mut h, state) = shell_with(painters)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (labels, selected, travel, left, right) = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert_eq!(labels, ["Left", "Right"]);
    assert!(selected == 0 && travel < 1e-3);
    assert!(
        filled(&drawn, left, MARK),
        "the face is not where it was told"
    );
    assert!(
        !wrote(&drawn, "Left") && !wrote(&drawn, "Right"),
        "the built-in labels are drawn under the painter"
    );
    assert!(left.max.x <= right.min.x + 0.5, "{left:?} {right:?}");
    h.tap(right.center());
    h.frames(2);
    assert_eq!(state.borrow().segment, 1, "the tap did not pick");
    let last = told.borrow().last().cloned();
    assert!(
        last.is_some_and(|t| t.1 == 1 && (t.2 - 1.0).abs() < 1e-3),
        "not told it moved"
    );
    Ok(())
}

/// **A chip painter draws the chip**; a tap still selects it.
#[test]
fn a_chip_painter_draws_the_chip_and_the_tap_still_selects() -> fairing::Result<()> {
    type ToldChip = (egui::Rect, egui::Rect, String, bool);
    let told: Told<ToldChip> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters =
        WidgetPainters::new().chip(move |painter: &egui::Painter, chip: &mut ChipLook<'_>| {
            painter.rect_filled(chip.drawn, 0.0, MARK);
            record
                .borrow_mut()
                .push((chip.rect, chip.drawn, chip.label.to_owned(), chip.selected));
        });
    let (mut h, state) = shell_with(painters)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (rect, chip, label, selected) = told
        .borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called"))?;
    assert_eq!(label, "Chip");
    assert!(!selected && filled(&drawn, chip, MARK));
    assert!(
        !wrote(&drawn, "Chip"),
        "the built-in label is drawn under the painter"
    );
    h.tap(rect.center());
    h.frames(2);
    assert!(state.borrow().chip, "the painted chip did not select");
    Ok(())
}

/// **A painter for one kind leaves the others alone**: with only a switch painter, the buttons
/// draw themselves.
#[test]
fn a_painter_for_one_kind_leaves_the_others_built_in() -> fairing::Result<()> {
    let switch = WidgetPainters::new().switch(|_: &egui::Painter, _: &mut SwitchLook<'_>| {});
    let (mut h, _) = shell_with(switch)?;
    let drawn = shapes(&mut h);
    assert!(
        wrote(&drawn, "Go") && wrote(&drawn, "Hold"),
        "the buttons are not built in"
    );
    assert!(wrote(&drawn, "Left") && wrote(&drawn, "One") && wrote(&drawn, "Chip"));
    Ok(())
}

/// **The shell lends the painters to its own widgets**: the unlock prompt's way out is a button,
/// and the button painter draws it. The panel is tall enough for the card to stand unscaled, so
/// the rect the painter is told is where the button is on the glass too.
#[test]
fn the_shells_own_widgets_take_the_painters() -> fairing::Result<()> {
    let mut config = access_config(&["viewer", "operator"], Some("top"));
    config.access.pin_table.pins = [("operator".to_owned(), "1234".to_owned())]
        .into_iter()
        .collect();
    config.motion.reduce = true;
    let told = Rc::new(RefCell::new(Vec::new()));
    let painters = WidgetPainters::new().button(button_painter(&told));
    let services = fairing::Services::builder()
        .clock(fairing::services::null::NullClock)
        .build();
    let mut h = Harness::from_builder(move |ctx| {
        Shell::builder(config)
            .services(services)
            .widget_painters(painters)
            .build(ctx)
    })?
    .with_size(1024.0, 1400.0);
    h.shell
        .add(screen("admin", |ui: &mut egui::Ui, _: &mut Cx<'_>| {
            ui.label("calibration");
        }));
    h.frames(2);
    h.shell.launch(LaunchAction::open("admin"));
    h.frames(3);
    assert!(h.shell.unlock_prompt_visible());
    let cancel = last_button(&told, "Cancel")?;
    h.tap(cancel.rect.center());
    h.frames(2);
    assert!(
        !h.shell.unlock_prompt_visible(),
        "the painted Cancel did not cancel"
    );
    Ok(())
}
