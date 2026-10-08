//! **Drawing the input widgets yourself** — the second part of the widget painters:
//! the slider, the stepper, the number field, the text field, the wheel, the
//! dropdown and the two pads. Each painter is told the widget's look; each widget keeps its
//! press, its drag, its value and its motion.
//!
//! The rules are the other integration tests': `fairing::Result<()>`, no `panic!`, no `unwrap`.

use fairing::testing::Harness;
use fairing::widgets::{
    Dropdown, DropdownLook, DropdownPart, NumberField, NumberFieldLook, PatternPad, PatternPadLook,
    PinKey, PinPad, PinPadLook, PinPart, SliderLook, StepEnd, Stepper, StepperLook, TextField,
    TextFieldLook, TouchSlider, WheelLook, WheelPicker, WidgetPainters,
};
use fairing::{screen, ColorRole, Cx, LaunchAction, Shell, ShellConfig};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(what: impl Into<String>) -> fairing::Error {
    fairing::Error::Config(what.into())
}

/// A painter's own fill — a colour nothing built in uses.
const MARK: egui::Color32 = egui::Color32::from_rgb(141, 3, 59);

/// The dropdown's options.
const OPTIONS: [&str; 3] = ["Low", "Mid", "High"];
/// The wheel's — words no other widget here writes.
const TURNS: [&str; 3] = ["One", "Two", "Three"];

/// What the gallery's widgets hold.
#[derive(Debug)]
struct Inputs {
    level: f32,
    count: i32,
    setpoint: f64,
    name: String,
    wheel: usize,
    pick: usize,
    pin: String,
    pattern: Vec<u8>,
    /// Whether the pattern pad keeps its path off the glass.
    hide_path: bool,
}

impl Default for Inputs {
    fn default() -> Self {
        Self {
            level: 20.0,
            count: 1,
            setpoint: 2.5,
            name: String::new(),
            wheel: 0,
            pick: 0,
            pin: String::new(),
            pattern: Vec::new(),
            hide_path: false,
        }
    }
}

/// The screen: one of each input, top to bottom.
fn gallery(state: Rc<RefCell<Inputs>>) -> impl FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static {
    move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
        let mut s = state.borrow_mut();
        let mut w = cx.widgets();
        let _ = TouchSlider::new(&mut s.level, 0.0..=100.0).show(ui, &mut w);
        let _ = Stepper::new(&mut s.count).range(0..=2).show(ui, &mut w);
        let _ = NumberField::new(&mut s.setpoint)
            .range(0.0..=10.0)
            .step(0.5)
            .decimals(1)
            .unit("mm")
            .show(ui, &mut w);
        let _ = TextField::new(&mut s.name).id_salt("name").show(ui, &mut w);
        let _ = WheelPicker::new("wheel", &TURNS, &mut s.wheel).show(ui, &mut w);
        let _ = Dropdown::new("pick", &OPTIONS, &mut s.pick).show(ui, &mut w);
        let _ = PinPad::new(&mut s.pin).len(4).show(ui, &mut w);
        let hide = s.hide_path;
        let _ = PatternPad::new(&mut s.pattern)
            .show_path(!hide)
            .show(ui, &mut w);
    }
}

/// A tall shell on a fixed clock with the gallery open, motion reduced, built with `painters`.
fn shell_with(
    painters: WidgetPainters,
    hide_path: bool,
) -> fairing::Result<(Harness, Rc<RefCell<Inputs>>)> {
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
    let state = Rc::new(RefCell::new(Inputs {
        hide_path,
        ..Inputs::default()
    }));
    h.shell.add(screen("inputs", gallery(Rc::clone(&state))));
    h.shell.launch(LaunchAction::open("inputs"));
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

fn last<T: Clone>(told: &Told<T>) -> fairing::Result<T> {
    told.borrow()
        .last()
        .cloned()
        .ok_or_else(|| fail("the painter was not called"))
}

/// **A slider painter is told the value and where the mover is**; the built-in track is not
/// drawn, and a tap on the axis still sets the value.
#[test]
fn a_slider_painter_follows_the_value_and_a_tap_still_sets_it() -> fairing::Result<()> {
    type ToldSlider = (egui::Rect, egui::Rect, egui::Rect, f32, f32, egui::Pos2);
    let (mut plain, _) = shell_with(WidgetPainters::new(), false)?;
    let built_in = shapes(&mut plain);
    let empty_side = plain.shell.theme().color(ColorRole::SurfaceVariant);

    let told: Told<ToldSlider> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().slider(
        move |painter: &egui::Painter, slider: &mut SliderLook<'_>| {
            painter.circle_filled(slider.handle, 6.0, MARK);
            record.borrow_mut().push((
                slider.rect,
                slider.track,
                slider.axis,
                slider.value,
                slider.fraction,
                slider.handle,
            ));
        },
    );
    let (mut h, state) = shell_with(painters, false)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (rect, track, axis, value, fraction, handle) = last(&told)?;
    assert!((value - 20.0).abs() < 1e-3 && (fraction - 0.2).abs() < 1e-3);
    let x = axis.min.x + axis.width() * 0.2;
    assert!((handle.x - x).abs() < 0.5, "{handle:?} not at {x}");
    assert!(
        filled(&built_in, track, empty_side) && !filled(&drawn, track, empty_side),
        "the built-in track is drawn under the painter"
    );
    let at = egui::pos2(axis.min.x + axis.width() * 0.75, rect.center().y);
    h.tap(at);
    h.frames(2);
    let level = state.borrow().level;
    assert!(
        (level - 75.0).abs() < 1.0,
        "the tap did not set it: {level}"
    );
    let (_, _, _, value, ..) = last(&told)?;
    assert!((value - level).abs() < 1e-3, "not told the new value");
    Ok(())
}

/// **A stepper painter is told both ends, live or spent**; a tap on `+` still steps, and at the
/// top of the range `+` is told it is spent.
#[test]
fn a_stepper_painter_draws_the_ends_and_a_tap_still_steps() -> fairing::Result<()> {
    type ToldStepper = (StepEnd, StepEnd, i32);
    let told: Told<ToldStepper> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().stepper(
        move |painter: &egui::Painter, stepper: &mut StepperLook<'_>| {
            painter.rect_filled(stepper.rect, 0.0, MARK);
            record
                .borrow_mut()
                .push((stepper.minus, stepper.plus, stepper.value));
        },
    );
    let (mut h, state) = shell_with(painters, false)?;
    let (minus, plus, value) = last(&told)?;
    assert_eq!(value, 1);
    assert!(minus.live && plus.live, "{minus:?} {plus:?}");
    assert!(minus.rect.max.x <= plus.rect.min.x, "{minus:?} {plus:?}");
    h.tap(plus.rect.center());
    h.frames(2);
    assert_eq!(state.borrow().count, 2, "the tap did not step");
    let (_, plus, value) = last(&told)?;
    assert_eq!(value, 2);
    assert!(
        !plus.live,
        "`+` at the top of the range is not told it is spent"
    );
    Ok(())
}

/// **A number field painter draws the track, the ends and the unit**; the figure stays the
/// field's to type into, and a tap on `+` still steps it.
#[test]
fn a_number_field_painter_draws_all_but_the_figure() -> fairing::Result<()> {
    type ToldField = (String, Option<(String, egui::Rect)>, StepEnd, f64);
    let told: Told<ToldField> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().number_field(
        move |painter: &egui::Painter, field: &mut NumberFieldLook<'_>| {
            painter.rect_filled(field.track, 0.0, MARK);
            record.borrow_mut().push((
                field.figure_text.to_owned(),
                field.unit.map(|(unit, at)| (unit.to_owned(), at)),
                field.plus,
                field.value,
            ));
        },
    );
    let (mut h, state) = shell_with(painters, false)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (figure, unit, plus, value) = last(&told)?;
    assert_eq!(figure, "2.5");
    assert!((value - 2.5).abs() < 1e-9);
    assert_eq!(unit.as_ref().map(|u| u.0.as_str()), Some("mm"));
    assert!(
        !wrote(&drawn, "mm"),
        "the built-in unit is drawn under the painter"
    );
    assert!(wrote(&drawn, "2.5"), "the figure is the field's to draw");
    h.tap(plus.rect.center());
    h.frames(2);
    assert!(
        (state.borrow().setpoint - 3.0).abs() < 1e-9,
        "the tap did not step"
    );
    Ok(())
}

/// **A text field painter draws the field and is told its focus**; what is typed still goes in.
#[test]
fn a_text_field_painter_draws_the_field_and_typing_still_types() -> fairing::Result<()> {
    type ToldText = (egui::Rect, bool, bool);
    let told: Told<ToldText> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().text_field(
        move |painter: &egui::Painter, field: &mut TextFieldLook<'_>| {
            painter.rect_filled(field.rect, 0.0, MARK);
            record
                .borrow_mut()
                .push((field.rect, field.focused, field.empty));
        },
    );
    let (mut h, state) = shell_with(painters, false)?;
    let (rect, focused, empty) = last(&told)?;
    assert!(!focused && empty);
    h.tap(rect.center());
    h.frames(2);
    h.type_text("pump 3");
    h.frames(2);
    assert_eq!(state.borrow().name, "pump 3", "typing did not go in");
    let (_, focused, empty) = last(&told)?;
    assert!(
        focused && !empty,
        "not told the field has the focus and the text"
    );
    Ok(())
}

/// Where the text reading exactly `wanted` was drawn.
fn text_at(shapes: &[egui::Shape], wanted: &str) -> Option<egui::Pos2> {
    shapes.iter().find_map(|shape| match shape {
        egui::Shape::Text(text) if text.galley.text() == wanted => Some(text.pos),
        _ => None,
    })
}

/// Types `text` into the gallery's text field and reports where it was drawn.
fn typed_at(h: &mut Harness, field: egui::Rect, text: &str) -> fairing::Result<egui::Pos2> {
    h.tap(field.center());
    h.frames(2);
    h.type_text(text);
    h.frames(2);
    text_at(&shapes(h), text).ok_or_else(|| fail(format!("{text:?} was not drawn")))
}

/// **What is typed in a painted field keeps the built-in padding** — it stands where the built-in
/// field puts it, not in the field's corner.
#[test]
fn a_painted_text_field_keeps_the_padding() -> fairing::Result<()> {
    let told: Told<egui::Rect> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().text_field(
        move |painter: &egui::Painter, field: &mut TextFieldLook<'_>| {
            painter.rect_filled(field.rect, 0.0, MARK);
            record.borrow_mut().push(field.rect);
        },
    );
    let (mut h, _) = shell_with(painters, false)?;
    let field = last(&told)?;
    let painted = typed_at(&mut h, field, "pump 3")?;
    let (mut plain, _) = shell_with(WidgetPainters::new(), false)?;
    let built_in = typed_at(&mut plain, field, "pump 3")?;
    assert!(
        (painted - built_in).length() < 3.0,
        "the text is at {painted:?} in the painted field and at {built_in:?} in the built-in one"
    );
    assert!(
        painted.x > field.min.x + 4.0,
        "the text is against the field's edge"
    );
    Ok(())
}

/// **A wheel painter is told the rows within reach and where they are**; a drag up still turns
/// the drum to the next option.
#[test]
fn a_wheel_painter_draws_the_rows_and_a_drag_still_turns_it() -> fairing::Result<()> {
    type ToldWheel = (egui::Rect, Vec<(usize, egui::Rect)>, usize);
    let told: Told<ToldWheel> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters =
        WidgetPainters::new().wheel(move |painter: &egui::Painter, wheel: &mut WheelLook<'_>| {
            painter.rect_filled(wheel.window, 0.0, MARK);
            record.borrow_mut().push((
                wheel.window,
                wheel.rows.iter().map(|r| (r.index, r.rect)).collect(),
                wheel.selected,
            ));
        });
    let (mut h, state) = shell_with(painters, false)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let (window, rows, selected) = last(&told)?;
    assert_eq!(selected, 0);
    assert!(filled(&drawn, window, MARK));
    let first = rows.iter().find(|r| r.0 == 0).map(|r| r.1);
    assert!(
        first.is_some_and(|r| same(r, window)),
        "the selected row is not in the window: {rows:?}"
    );
    assert!(
        rows.iter().any(|r| r.0 == 1),
        "the next row is not within reach"
    );
    assert!(
        !wrote(&drawn, "One"),
        "the built-in rows are drawn under the painter"
    );
    // A flick up turns the drum on to a later row — how far is the release's speed.
    let from = window.center();
    h.drag(from, from - egui::vec2(0.0, window.height()), 8);
    h.frames(30);
    let turned = state.borrow().wheel;
    assert!(turned > 0, "the drag did not turn the drum");
    let (_, _, selected) = last(&told)?;
    assert_eq!(selected, turned, "not told where the drum settled");
    Ok(())
}

/// What a dropdown painter was told, once.
#[derive(Debug, Clone)]
struct ToldPiece {
    part: DropdownPart,
    rect: egui::Rect,
    text: String,
}

/// **A dropdown painter draws the control, the open panel and each option**; a tap still opens
/// it and a tap on an option still picks it.
#[test]
fn a_dropdown_painter_draws_every_piece_and_a_tap_still_picks() -> fairing::Result<()> {
    let told: Told<ToldPiece> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters = WidgetPainters::new().dropdown(
        move |painter: &egui::Painter, piece: &mut DropdownLook<'_>| {
            painter.rect_filled(piece.rect, 0.0, MARK);
            record.borrow_mut().push(ToldPiece {
                part: piece.part,
                rect: piece.rect,
                text: piece.text.to_owned(),
            });
        },
    );
    let (mut h, state) = shell_with(painters, false)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let closed = last(&told)?;
    assert!(
        matches!(
            closed.part,
            DropdownPart::Trigger {
                open: false,
                head: false,
                ..
            }
        ),
        "{closed:?}"
    );
    assert_eq!(closed.text, "Low");
    assert!(
        !wrote(&drawn, "Low"),
        "the built-in value is drawn under the painter"
    );
    h.tap(closed.rect.center());
    h.frames(3);
    told.borrow_mut().clear();
    h.frame();
    let pieces = told.borrow().clone();
    assert!(
        pieces
            .iter()
            .any(|p| matches!(p.part, DropdownPart::Panel { .. })),
        "no panel: {pieces:?}"
    );
    let options: Vec<&ToldPiece> = pieces
        .iter()
        .filter(|p| matches!(p.part, DropdownPart::Option { .. }))
        .collect();
    let texts: Vec<&str> = options.iter().map(|p| p.text.as_str()).collect();
    assert_eq!(texts, OPTIONS, "{pieces:?}");
    let high = options
        .iter()
        .find(|p| p.text == "High")
        .ok_or_else(|| fail("no High"))?;
    assert!(matches!(
        high.part,
        DropdownPart::Option {
            index: 2,
            selected: false,
            ..
        }
    ));
    h.tap(high.rect.center());
    h.frames(3);
    assert_eq!(state.borrow().pick, 2, "the tap did not pick");
    Ok(())
}

/// What a PIN pad painter was told, once.
#[derive(Debug, Clone, Copy)]
struct ToldPin {
    part: PinPart,
    rect: egui::Rect,
}

/// **A PIN pad painter draws the dot row and every key**; the keys still type.
#[test]
fn a_pin_pad_painter_draws_the_keys_and_the_keys_still_type() -> fairing::Result<()> {
    let told: Told<ToldPin> = Rc::new(RefCell::new(Vec::new()));
    let record = Rc::clone(&told);
    let painters =
        WidgetPainters::new().pin_pad(move |painter: &egui::Painter, pin: &mut PinPadLook<'_>| {
            painter.rect_filled(pin.drawn, 0.0, MARK);
            record.borrow_mut().push(ToldPin {
                part: pin.part,
                rect: pin.rect,
            });
        });
    let (mut h, state) = shell_with(painters, false)?;
    told.borrow_mut().clear();
    let drawn = shapes(&mut h);
    let pieces = told.borrow().clone();
    let keys: Vec<PinKey> = pieces
        .iter()
        .filter_map(|p| match p.part {
            PinPart::Key { key, .. } => Some(key),
            _ => None,
        })
        .collect();
    assert_eq!(keys.len(), 12, "{keys:?}");
    for digit in 0..10 {
        assert!(keys.contains(&PinKey::Digit(digit)), "no key {digit}");
    }
    assert!(keys.contains(&PinKey::Erase) && keys.contains(&PinKey::Ok));
    assert!(
        pieces.iter().any(|p| matches!(
            p.part,
            PinPart::Dots {
                entered: 0,
                length: Some(4)
            }
        )),
        "no dot row: {pieces:?}"
    );
    assert!(
        !wrote(&drawn, "5"),
        "the built-in digits are drawn under the painter"
    );
    let key = |digit: u8| {
        pieces
            .iter()
            .find(|p| matches!(p.part, PinPart::Key { key: PinKey::Digit(d), .. } if d == digit))
            .map(|p| p.rect)
            .ok_or_else(|| fail(format!("no key {digit}")))
    };
    for digit in [1, 2] {
        h.tap(key(digit)?.center());
        h.frames(1);
    }
    h.frames(1);
    assert_eq!(state.borrow().pin, "12", "the painted keys did not type");
    let dots = told.borrow().iter().rev().find_map(|p| match p.part {
        PinPart::Dots { entered, .. } => Some(entered),
        _ => None,
    });
    assert_eq!(dots, Some(2), "the dot row is not told what is in");
    Ok(())
}

/// What a pattern pad painter was told, once.
type ToldPattern = (egui::Rect, Vec<egui::Pos2>, Vec<u8>, Option<egui::Pos2>);

fn pattern_painter(
    told: &Told<ToldPattern>,
) -> impl FnMut(&egui::Painter, &mut PatternPadLook<'_>) + 'static {
    let told = Rc::clone(told);
    move |painter: &egui::Painter, pad: &mut PatternPadLook<'_>| {
        for dot in pad.dots {
            painter.circle_filled(*dot, pad.dot, MARK);
        }
        told.borrow_mut()
            .push((pad.rect, pad.dots.to_vec(), pad.path.to_vec(), pad.finger));
    }
}

/// Draw through dots `path` (indices into `dots`), holding the finger at the end.
fn stroke(h: &mut Harness, dots: &[egui::Pos2], path: &[usize]) -> fairing::Result<egui::Pos2> {
    let at = |i: usize| {
        dots.get(i)
            .copied()
            .ok_or_else(|| fail(format!("no dot {i}")))
    };
    let first = path.first().copied().ok_or_else(|| fail("no path"))?;
    let mut pos = at(first)?;
    h.press(pos);
    h.frame();
    for &i in path.iter().skip(1) {
        let to = at(i)?;
        for step in 1..=4 {
            // Step counts are tiny, so the conversion is exact.
            #[allow(clippy::cast_precision_loss)]
            let k = step as f32 / 4.0;
            h.move_to(pos + (to - pos) * k);
            h.frame();
        }
        pos = to;
    }
    Ok(pos)
}

/// **A pattern pad painter is told the dots, the path and the finger**; the stroke still draws
/// the pattern.
#[test]
fn a_pattern_pad_painter_follows_the_stroke() -> fairing::Result<()> {
    let told: Told<ToldPattern> = Rc::new(RefCell::new(Vec::new()));
    let (mut h, state) = shell_with(
        WidgetPainters::new().pattern_pad(pattern_painter(&told)),
        false,
    )?;
    let (rect, dots, path, finger) = last(&told)?;
    assert_eq!(dots.len(), 9);
    assert!(dots.iter().all(|d| rect.contains(*d)));
    assert!(path.is_empty() && finger.is_none());
    let end = stroke(&mut h, &dots, &[0, 1, 2, 5])?;
    let (_, _, path, finger) = last(&told)?;
    assert_eq!(path, [0, 1, 2, 5], "not told the path");
    assert!(finger.is_some(), "not told where the finger is");
    h.release(end);
    h.frames(2);
    assert_eq!(
        state.borrow().pattern,
        [0, 1, 2, 5],
        "the stroke did not draw it"
    );
    Ok(())
}

/// **A path kept off the glass is kept from the painter**: with `show_path(false)` the painter is
/// told no path and no finger, though the pattern is still drawn.
#[test]
fn a_hidden_path_is_kept_from_the_painter() -> fairing::Result<()> {
    let told: Told<ToldPattern> = Rc::new(RefCell::new(Vec::new()));
    let (mut h, state) = shell_with(
        WidgetPainters::new().pattern_pad(pattern_painter(&told)),
        true,
    )?;
    let (_, dots, ..) = last(&told)?;
    let end = stroke(&mut h, &dots, &[0, 1, 2, 5])?;
    let (_, _, path, finger) = last(&told)?;
    assert!(
        path.is_empty() && finger.is_none(),
        "the hidden path reached the painter"
    );
    h.release(end);
    h.frames(2);
    assert_eq!(
        state.borrow().pattern,
        [0, 1, 2, 5],
        "the stroke did not draw it"
    );
    Ok(())
}
