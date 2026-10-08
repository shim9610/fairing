//! **The touch widgets hold up under the values a real panel hands them** — a stale or sentinel
//! index, a programmatic set, a NaN reading, a reversed axis, float drift on a setpoint, two
//! icons by one name — and a password field stays masked however it is laid out.

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Response, Vec2};
use fairing_widgets::icons::IconSet;
use fairing_widgets::motion::{AnimationStore, Tween};
use fairing_widgets::theme::Theme;
use fairing_widgets::widgets::{NumberField, TextField, TouchSlider, WheelPicker, WidgetPainters};
use fairing_widgets::WidgetCx;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

const DT: f32 = 1.0 / 60.0;

type TestResult<T = ()> = Result<T, String>;

/// A headless egui context plus everything a `WidgetCx` borrows.
struct Rig {
    ctx: egui::Context,
    theme: Theme,
    icons: IconSet,
    anims: AnimationStore,
    painters: Option<WidgetPainters>,
    frame: u64,
    time: f64,
    pending: Vec<Event>,
}

impl Rig {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            theme: Theme::light(),
            icons: IconSet::new(),
            anims: AnimationStore::new(),
            painters: None,
            frame: 0,
            time: 0.0,
            pending: Vec::new(),
        }
    }

    fn with_painters(mut self, painters: WidgetPainters) -> Self {
        self.painters = Some(painters);
        self
    }

    /// One frame: what the closure returned on its last pass, and the shapes drawn.
    fn run_full<R>(
        &mut self,
        mut f: impl FnMut(&mut egui::Ui, &mut WidgetCx<'_>) -> R,
    ) -> TestResult<(R, Vec<egui::epaint::ClippedShape>)> {
        self.anims.tick(DT, self.frame);
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            time: Some(self.time),
            predicted_dt: DT,
            events: std::mem::take(&mut self.pending),
            ..Default::default()
        };
        let mut out = None;
        let Self {
            ctx,
            theme,
            icons,
            anims,
            painters,
            frame,
            ..
        } = self;
        let mut output = ctx.run_ui(input, |ui| {
            let mut cx = WidgetCx {
                theme,
                icons,
                anims,
                anim_scope: egui::Id::new("m7"),
                frame: *frame,
                inset_bottom: 0.0,
                painters: painters.as_mut(),
            };
            out = Some(f(ui, &mut cx));
        });
        let shapes = std::mem::take(&mut output.shapes);
        output.drop_without_applying_deltas();
        self.time += f64::from(DT);
        self.frame += 1;
        let out = out.ok_or_else(|| "the frame did not run the closure".to_owned())?;
        Ok((out, shapes))
    }

    fn run<R>(&mut self, f: impl FnMut(&mut egui::Ui, &mut WidgetCx<'_>) -> R) -> TestResult<R> {
        Ok(self.run_full(f)?.0)
    }

    fn press(&mut self, pos: Pos2) {
        self.pending.push(Event::PointerMoved(pos));
        self.pending.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        });
    }

    fn release(&mut self, pos: Pos2) {
        self.pending.push(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        });
        self.pending.push(Event::PointerGone);
    }
}

/// Every piece of text drawn in `shapes`.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, &mut out);
    }
    out
}

// ── WheelPicker ────────────────────────────────────────────────────────────────────────────

/// Guards against a `selected` past `u16::MAX` (a sentinel such as `usize::MAX`) hanging the UI
/// thread in the row loop.
#[test]
fn a_wheel_with_a_huge_selected_index_returns_and_pulls_it_onto_the_drum() -> TestResult {
    let worker = std::thread::spawn(|| {
        let mut rig = Rig::new();
        let options = ["a", "b", "c", "d", "e"];
        let mut selected = 70_000_usize;
        rig.run(|ui, cx| {
            WheelPicker::new("hang", &options, &mut selected).show(ui, cx);
        })
        .map(|()| selected)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !worker.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if !worker.is_finished() {
        return Err("WheelPicker::show did not return within 10 s for selected = 70 000".into());
    }
    let selected = worker
        .join()
        .map_err(|_| "the wheel thread panicked".to_owned())??;
    assert_eq!(selected, 4, "past the end of an open drum is its last row");
    Ok(())
}

/// What a wheel painter saw last: the index under the window, and each row's index and distance.
type WheelSeen = Rc<RefCell<(usize, Vec<(usize, f32)>)>>;

/// Guards that a `selected` past the end parks the drum on the last row, not off the end with
/// no row drawn.
#[test]
fn a_wheel_with_selected_past_the_end_shows_its_last_row() -> TestResult {
    let seen: WheelSeen = Rc::default();
    let sink = Rc::clone(&seen);
    let painters = WidgetPainters::new().wheel(move |_, look| {
        *sink.borrow_mut() = (
            look.selected,
            look.rows.iter().map(|r| (r.index, r.distance)).collect(),
        );
    });
    let mut rig = Rig::new().with_painters(painters);
    let options = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let mut selected = 20_usize;
    for _ in 0..60 {
        rig.run(|ui, cx| {
            WheelPicker::new("past", &options, &mut selected).show(ui, cx);
        })?;
    }
    let (shown, rows) = seen.borrow().clone();
    assert_eq!(selected, 9, "selected is pulled to the last option");
    assert_eq!(shown, 9);
    assert!(
        rows.iter().any(|(index, d)| *index == 9 && *d < 0.5),
        "the window should show option 9; rows drawn: {rows:?}"
    );
    Ok(())
}

/// Guards that a value the caller sets is turned to and kept, not overwritten with the rows the
/// drum passes and reported back as the operator's changes.
#[test]
fn a_wheel_keeps_a_value_the_caller_set_and_reports_no_change() -> TestResult {
    let mut rig = Rig::new();
    let options = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];
    let mut selected = 0_usize;
    for _ in 0..30 {
        rig.run(|ui, cx| {
            WheelPicker::new("prog", &options, &mut selected).show(ui, cx);
        })?;
    }
    selected = 9;
    let mut changes = Vec::new();
    let mut passed = Vec::new();
    for _ in 0..30 {
        let reported = rig.run(|ui, cx| {
            WheelPicker::new("prog", &options, &mut selected)
                .show(ui, cx)
                .changed()
        })?;
        passed.push(selected);
        if reported {
            changes.push(selected);
        }
    }
    assert!(
        passed.iter().all(|s| *s == 9),
        "the caller's value was overwritten on the way: {passed:?}"
    );
    assert!(changes.is_empty(), "reported as changes: {changes:?}");
    Ok(())
}

/// Guards that a joined drum turns to a caller's value the short way: 23 to 00 is one row.
#[test]
fn a_wrapped_wheel_turns_the_short_way_round() -> TestResult {
    let seen: Rc<RefCell<Vec<usize>>> = Rc::default();
    let sink = Rc::clone(&seen);
    let painters = WidgetPainters::new().wheel(move |_, look| {
        sink.borrow_mut().push(look.selected);
    });
    let mut rig = Rig::new().with_painters(painters);
    let hours: Vec<String> = (0..24).map(|h| format!("{h:02}")).collect();
    let options: Vec<&str> = hours.iter().map(String::as_str).collect();
    let mut selected = 23_usize;
    for _ in 0..30 {
        rig.run(|ui, cx| {
            WheelPicker::new("hours", &options, &mut selected)
                .wrap(true)
                .show(ui, cx);
        })?;
    }
    seen.borrow_mut().clear();
    selected = 0;
    for _ in 0..60 {
        rig.run(|ui, cx| {
            WheelPicker::new("hours", &options, &mut selected)
                .wrap(true)
                .show(ui, cx);
        })?;
    }
    let under_window = seen.borrow().clone();
    assert_eq!(selected, 0);
    assert!(
        under_window.iter().all(|s| *s == 0 || *s == 23),
        "23 -> 0 on a joined drum went the long way round: {under_window:?}"
    );
    Ok(())
}

// ── TouchSlider ────────────────────────────────────────────────────────────────────────────

fn tap_slider_at(
    rig: &mut Rig,
    value: &mut f32,
    range: &std::ops::RangeInclusive<f32>,
    frac: f32,
) -> TestResult {
    let rect = rig.run(|ui, cx| TouchSlider::new(value, range.clone()).show(ui, cx).rect)?;
    let at = Pos2::new(rect.min.x + rect.width() * frac, rect.center().y);
    rig.press(at);
    rig.run(|ui, cx| TouchSlider::new(value, range.clone()).show(ui, cx))?;
    rig.release(at);
    rig.run(|ui, cx| TouchSlider::new(value, range.clone()).show(ui, cx))?;
    rig.run(|ui, cx| TouchSlider::new(value, range.clone()).show(ui, cx))?;
    Ok(())
}

/// Guards that a NaN value (a bad reading upstream) is put right by a touch.
#[test]
fn a_slider_holding_nan_recovers_on_a_tap() -> TestResult {
    let mut rig = Rig::new();
    let mut value = f32::NAN;
    tap_slider_at(&mut rig, &mut value, &(0.0..=100.0), 0.5)?;
    assert!(
        (value - 50.0).abs() < 5.0,
        "a tap in the middle of 0..=100 left {value}"
    );
    Ok(())
}

/// Guards that a range given high to low still edits instead of pinning every tap to one end.
#[test]
fn a_slider_with_a_reversed_range_still_edits() -> TestResult {
    let mut rig = Rig::new();
    let reversed = 100.0_f32..=0.0;
    let mut value = 50.0_f32;
    tap_slider_at(&mut rig, &mut value, &reversed, 0.2)?;
    let left = value;
    tap_slider_at(&mut rig, &mut value, &reversed, 0.8)?;
    let right = value;
    assert!(
        (left - right).abs() > 1.0,
        "taps at 20 % and 80 % of a 100..=0 slider gave {left} and {right}"
    );
    Ok(())
}

// ── NumberField ────────────────────────────────────────────────────────────────────────────

/// How a field is built.
#[derive(Clone, Copy)]
struct Cfg {
    range: (f64, f64),
    step: f64,
    decimals: usize,
}

const ONE_DECIMAL: Cfg = Cfg {
    range: (0.0, 100.0),
    step: 1.0,
    decimals: 1,
};

fn show_field(ui: &mut egui::Ui, cx: &mut WidgetCx<'_>, value: &mut f64, build: &Cfg) -> Response {
    NumberField::new(value)
        .range(build.range.0..=build.range.1)
        .step(build.step)
        .decimals(build.decimals)
        .show(ui, cx)
}

/// Tap the figure in the middle of the field, so it takes the focus.
fn focus_figure(rig: &mut Rig, value: &mut f64, build: &Cfg) -> TestResult<(Rect, bool)> {
    let rect = rig.run(|ui, cx| show_field(ui, cx, value, build).rect)?;
    let middle = rect.center();
    let mut changed = false;
    rig.press(middle);
    changed |= rig.run(|ui, cx| show_field(ui, cx, value, build).changed())?;
    rig.release(middle);
    changed |= rig.run(|ui, cx| show_field(ui, cx, value, build).changed())?;
    changed |= rig.run(|ui, cx| show_field(ui, cx, value, build).changed())?;
    Ok((rect, changed))
}

/// Guards that focusing the figure and leaving it untouched does not round the setpoint to the
/// decimals shown.
#[test]
fn a_number_field_focused_and_left_keeps_its_value() -> TestResult {
    let mut rig = Rig::new();
    let mut value = 12.34_f64;
    let build = ONE_DECIMAL;
    let (_, mut changed) = focus_figure(&mut rig, &mut value, &build)?;
    let away = Pos2::new(700.0, 550.0);
    rig.press(away);
    changed |= rig.run(|ui, cx| show_field(ui, cx, &mut value, &build).changed())?;
    rig.release(away);
    for _ in 0..3 {
        changed |= rig.run(|ui, cx| show_field(ui, cx, &mut value, &build).changed())?;
    }
    assert!(
        (value - 12.34).abs() < 1e-9 && !changed,
        "focus-and-leave changed the value to {value} (changed = {changed})"
    );
    Ok(())
}

/// Guards that a `+` tap while the figure has focus is not undone by the stale figure being
/// committed when the edit lets go.
#[test]
fn a_number_field_step_while_editing_sticks() -> TestResult {
    let mut rig = Rig::new();
    let mut value = 10.0_f64;
    let build = ONE_DECIMAL;
    let (rect, _) = focus_figure(&mut rig, &mut value, &build)?;
    let plus = Pos2::new(rect.max.x - rect.height() * 0.5, rect.center().y);
    rig.press(plus);
    rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    rig.release(plus);
    for _ in 0..3 {
        rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    }
    assert!(
        (value - 11.0).abs() < 1e-9,
        "a + tap while editing left the value at {value}"
    );
    Ok(())
}

/// Guards that a `+` tap on a figure typed but not yet committed steps from the typed number,
/// rather than the typed figure being committed over the step when the edit lets go.
#[test]
fn a_number_field_step_on_a_typed_figure_steps_from_it() -> TestResult {
    let mut rig = Rig::new();
    let mut value = 10.0_f64;
    let build = ONE_DECIMAL;
    let (rect, _) = focus_figure(&mut rig, &mut value, &build)?;
    rig.pending.push(Event::Key {
        key: egui::Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::COMMAND,
    });
    rig.pending.push(Event::Text("20".into()));
    rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    assert!(
        (value - 10.0).abs() < 1e-9,
        "typing alone committed {value}"
    );
    let plus = Pos2::new(rect.max.x - rect.height() * 0.5, rect.center().y);
    rig.press(plus);
    rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    rig.release(plus);
    for _ in 0..3 {
        rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    }
    assert!(
        (value - 21.0).abs() < 1e-9,
        "+ on a typed 20 left the value at {value}"
    );
    Ok(())
}

type FieldSeen = Rc<RefCell<(String, bool, bool)>>;

fn field_painters() -> (WidgetPainters, FieldSeen) {
    let seen: FieldSeen = Rc::default();
    let sink = Rc::clone(&seen);
    let painters = WidgetPainters::new().number_field(move |_, look| {
        *sink.borrow_mut() = (look.figure_text.to_owned(), look.minus.live, look.plus.live);
    });
    (painters, seen)
}

fn tap_end(rig: &mut Rig, value: &mut f64, build: &Cfg, plus: bool) -> TestResult {
    let rect = rig.run(|ui, cx| show_field(ui, cx, value, build).rect)?;
    let x = if plus {
        rect.max.x - rect.height() * 0.5
    } else {
        rect.min.x + rect.height() * 0.5
    };
    let at = Pos2::new(x, rect.center().y);
    rig.press(at);
    rig.run(|ui, cx| show_field(ui, cx, value, build))?;
    rig.release(at);
    rig.run(|ui, cx| show_field(ui, cx, value, build))?;
    rig.run(|ui, cx| show_field(ui, cx, value, build))?;
    Ok(())
}

/// Guards against `-0.0` on a setpoint after float drift (0.3 − 0.1 − 0.1 − 0.1).
#[test]
fn a_number_field_never_shows_negative_zero() -> TestResult {
    let (painters, seen) = field_painters();
    let mut rig = Rig::new().with_painters(painters);
    let mut value = 0.3_f64;
    let build = Cfg {
        range: (-1.0, 1.0),
        step: 0.1,
        decimals: 1,
    };
    for _ in 0..3 {
        tap_end(&mut rig, &mut value, &build, false)?;
    }
    let shown = seen.borrow().0.clone();
    assert_eq!(shown, "0.0", "three steps down from 0.3 (value {value:e})");
    // A value a float error below zero, as a caller's arithmetic leaves it, reads zero too.
    let mut value = -1e-12_f64;
    rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    let shown = seen.borrow().0.clone();
    assert_eq!(shown, "0.0", "a value of {value:e}");
    Ok(())
}

/// Guards that ten `0.1` steps land on the range's end exactly, so `+` is spent where the figure
/// reads the maximum.
#[test]
fn a_number_field_plus_is_spent_at_the_displayed_maximum() -> TestResult {
    let (painters, seen) = field_painters();
    let mut rig = Rig::new().with_painters(painters);
    let mut value = 0.0_f64;
    let build = Cfg {
        range: (0.0, 1.0),
        step: 0.1,
        decimals: 1,
    };
    for _ in 0..10 {
        tap_end(&mut rig, &mut value, &build, true)?;
    }
    let (shown, _, plus_live) = seen.borrow().clone();
    assert_eq!(shown, "1.0");
    assert!(!plus_live, "+ is still live at the end (value {value:?})");
    assert_eq!(value.to_bits(), 1.0_f64.to_bits(), "value {value:?}");
    // A value a float error short of the top, as a caller's arithmetic leaves it, is at the top.
    let mut value = 1.0 - 1e-12_f64;
    rig.run(|ui, cx| show_field(ui, cx, &mut value, &build))?;
    let (shown, _, plus_live) = seen.borrow().clone();
    assert_eq!(shown, "1.0");
    assert!(!plus_live, "+ is still live at {value:?}");
    Ok(())
}

/// A `+` tap with no focus steps by one: the tap lands on the end the tests above aim at.
#[test]
fn a_number_field_plus_tap_steps_once() -> TestResult {
    let mut rig = Rig::new();
    let mut value = 10.0_f64;
    tap_end(&mut rig, &mut value, &ONE_DECIMAL, true)?;
    assert!((value - 11.0).abs() < 1e-9, "value {value}");
    Ok(())
}

// ── TextField ──────────────────────────────────────────────────────────────────────────────

fn drawn_field(password: bool, multiline: bool) -> TestResult<Vec<String>> {
    let mut rig = Rig::new();
    let mut secret = String::from("hunter2-secret");
    let (_, shapes) = rig.run_full(|ui, cx| {
        TextField::new(&mut secret)
            .password(password)
            .multiline(multiline)
            .show(ui, cx)
    })?;
    Ok(texts(&shapes))
}

/// Guards that a multi-line password field stays masked (its rebuilt `TextEdit` dropped the mask).
#[test]
fn a_password_field_is_masked_single_line_and_multi_line() -> TestResult {
    let plain = drawn_field(false, true)?;
    assert!(
        plain.iter().any(|t| t.contains("hunter2")),
        "the plain field did not draw its text at all: {plain:?}"
    );
    for multiline in [false, true] {
        let drawn = drawn_field(true, multiline)?;
        assert!(
            !drawn.iter().any(|t| t.contains("hunter2")),
            "multiline = {multiline}: the password was drawn in plain text: {drawn:?}"
        );
    }
    Ok(())
}

// ── AnimationStore ─────────────────────────────────────────────────────────────────────────

/// Guards that one NaN target does not poison an id for good.
#[test]
fn the_animation_store_recovers_after_a_nan_target() {
    let mut store = AnimationStore::new();
    let id = egui::Id::new("nan");
    let tween = Tween::cubic_out(Duration::from_millis(100));
    let _ = store.animate(id, f32::NAN, tween, 0);
    let first = store.animate(id, 1.0, tween, 0);
    assert!(
        (first - 1.0).abs() < 1e-6,
        "there is nothing to tween from, so it lands: {first}"
    );
    let mut last = f32::NAN;
    for frame in 1..60 {
        store.tick(DT, frame);
        last = store.animate(id, 1.0, tween, frame);
    }
    assert!(
        (last - 1.0).abs() < 1e-3,
        "after a second of asking for 1.0 the value is {last}"
    );
}

// ── IconSet ────────────────────────────────────────────────────────────────────────────────

/// Guards that two registered icons sharing a name each draw their own path (the polyline cache
/// was keyed on the name).
#[test]
fn custom_icons_sharing_a_name_draw_as_themselves() -> TestResult {
    use fairing_widgets::icons::{CustomIconId, IconDef, IconRef, IconStyle, Seg};
    static FLAT: [Seg; 2] = [Seg::M(4.0, 12.0), Seg::L(20.0, 12.0)];
    static TALL: [Seg; 2] = [Seg::M(12.0, 4.0), Seg::L(12.0, 20.0)];
    let mut rig = Rig::new();
    let flat = rig.icons.register(IconDef {
        name: "shared-glyph",
        segs: &FLAT,
        fill: false,
    });
    let tall = rig.icons.register(IconDef {
        name: "shared-glyph",
        segs: &TALL,
        fill: false,
    });
    let rect = Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::splat(48.0));
    let draw = |rig: &mut Rig, id: CustomIconId| -> TestResult<Option<[Pos2; 2]>> {
        let (_, shapes) = rig.run_full(|ui, cx| {
            cx.icons.paint(
                ui.painter(),
                rect,
                &IconRef::Custom(id),
                &IconStyle::sized(48.0),
                cx.theme,
            )
        })?;
        Ok(shapes.iter().find_map(|c| match &c.shape {
            egui::Shape::LineSegment { points, .. } => Some(*points),
            _ => None,
        }))
    };
    let first = draw(&mut rig, flat)?;
    let Some([a0, a1]) = first else {
        return Err("the first icon drew no line segment".into());
    };
    let Some([b0, b1]) = draw(&mut rig, tall)? else {
        return Err("the second icon drew no line segment".into());
    };
    assert!((a0.y - a1.y).abs() < 0.5, "the flat icon: {a0:?}-{a1:?}");
    assert!(
        (b0.x - b1.x).abs() < 0.5,
        "the tall icon was drawn as {b0:?}-{b1:?}, the flat icon's line"
    );
    Ok(())
}
