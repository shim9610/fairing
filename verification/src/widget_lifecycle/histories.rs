//! Stateful histories drive the real widgets; the oracle describes public outcomes.
//! Never clear Context, focus, area memory or animation storage between operations.
use super::{pointer, Menu, Runner};
use fairing::egui::{self, Event, Modifiers, Pos2, Rect};
use fairing::fairing_widgets::WidgetCx;
use fairing::widgets::{
    BigButton, FieldLook, Keys, NumberField, Opener, TextField, Trigger, WheelPicker,
};
use proptest::prelude::*;
use std::sync::OnceLock;
use std::time::Duration;

type Outcome = (usize, String, u64, usize, usize);

const WHEEL: [&str; 5] = ["zero", "one", "two", "three", "four"];
// The bottom-right lies inside a full-width Sheet. Use this fixture's blank top-right.
const OUTSIDE: Pos2 = Pos2::new(940.0, 100.0);
const SETTLE: usize = 24;

fn key(key: egui::Key, modifiers: Modifiers) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        })
        .collect()
}

fn pane(ui: &mut egui::Ui, name: &'static str, at: Pos2, draw: impl FnOnce(&mut egui::Ui)) {
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(name)
            .max_rect(Rect::from_min_size(at, egui::vec2(300.0, 300.0))),
        draw,
    );
}

struct Scene {
    menu: Menu,
    mounted: bool,
    enabled: bool,
    parent: bool,
    text: String,
    number: f64,
    wheel: usize,
    text_rect: Rect,
    number_rect: Rect,
    hold_rect: Rect,
    wheel_rect: Rect,
    text_id: egui::Id,
    hold_count: usize,
}

impl Scene {
    fn new(opener: Opener, kind: Trigger) -> Self {
        let mut menu = Menu::new(opener);
        menu.kind = kind;
        Self {
            menu,
            mounted: true,
            enabled: true,
            parent: true,
            text: String::new(),
            number: 10.0,
            wheel: 2,
            text_rect: Rect::NOTHING,
            number_rect: Rect::NOTHING,
            hold_rect: Rect::NOTHING,
            wheel_rect: Rect::NOTHING,
            text_id: egui::Id::NULL,
            hold_count: 0,
        }
    }

    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) {
        self.menu.draw(ui, cx);
        ui.add_enabled_ui(self.parent, |ui| {
            if !self.mounted {
                return;
            }
            pane(ui, "text-owner", egui::pos2(380.0, 20.0), |ui| {
                let response = TextField::new(&mut self.text)
                    .id_salt("text")
                    .enabled(self.enabled)
                    .keys(Keys::Latin)
                    .show(ui, cx);
                self.text_rect = response.rect;
                self.text_id = response.id;
            });
            pane(ui, "number-owner", egui::pos2(380.0, 120.0), |ui| {
                self.number_rect = NumberField::new(&mut self.number)
                    .enabled(self.enabled)
                    .show(ui, cx)
                    .rect;
            });
            pane(ui, "hold-owner", egui::pos2(380.0, 220.0), |ui| {
                let response = BigButton::new("hold")
                    .enabled(self.enabled)
                    .long_press(Duration::from_millis(100))
                    .show(ui, cx);
                self.hold_rect = response.response.rect;
                self.hold_count += usize::from(response.long_pressed());
            });
            pane(ui, "wheel-owner", egui::pos2(380.0, 360.0), |ui| {
                self.wheel_rect = WheelPicker::new("wheel", &WHEEL, &mut self.wheel)
                    .enabled(self.enabled)
                    .show(ui, cx)
                    .rect;
            });
        });
    }

    fn frame(&mut self, runner: &mut Runner, events: Vec<Event>) {
        runner.frame(events, |ui, cx| self.draw(ui, cx));
        assert!(
            self.number.is_finite(),
            "history produced a nonfinite number"
        );
        assert!(
            self.wheel < WHEEL.len(),
            "history produced an invalid wheel index"
        );
    }

    fn idle(&mut self, runner: &mut Runner, count: usize) {
        for _ in 0..count {
            self.frame(runner, vec![]);
        }
    }

    fn tap(&mut self, runner: &mut Runner, at: Pos2) {
        self.frame(runner, pointer(at, true));
        self.frame(runner, pointer(at, false));
    }

    fn popup_visible(&self, runner: &Runner) -> bool {
        runner.text("Bravo", self.menu.trigger).is_some()
    }

    fn retire(&mut self, runner: &mut Runner, how: u8) -> (bool, bool, bool) {
        let before = self.hold_count;
        match how % 3 {
            0 => {
                self.mounted = false;
                self.menu.mounted = false;
            }
            1 => {
                self.enabled = false;
                self.menu.enabled = false;
            }
            _ => {
                self.parent = false;
                self.menu.parent_enabled = false;
            }
        }
        self.idle(runner, 2);
        let observed = (self.mounted, self.enabled, self.parent);
        self.mounted = true;
        self.enabled = true;
        self.parent = true;
        self.menu.mounted = true;
        self.menu.enabled = true;
        self.menu.parent_enabled = true;
        self.idle(runner, 2);
        assert!(
            !self.popup_visible(runner),
            "retired popup revived in history"
        );
        assert_eq!(
            self.hold_count, before,
            "retired press completed in history"
        );
        observed
    }

    /// Contract preconditions are explicit: visible/enabled controls, released pointer,
    /// dismissed popup, and ordinary frame opportunities. No hidden storage is reset.
    fn recover(&mut self, runner: &mut Runner) {
        runner.dt = 1.0 / 60.0;
        self.frame(runner, pointer(OUTSIDE, false));
        self.tap(runner, OUTSIDE);
        self.idle(runner, SETTLE);
        assert!(!self.popup_visible(runner), "outside dismissal was blocked");
    }

    /// All suffix outcomes are checked, not merely whether a widget was drawn.
    fn promised_suffix(&mut self, runner: &mut Runner) -> Outcome {
        self.recover(runner);
        let hits = self.menu.hits;
        self.tap(runner, self.menu.peer.center());
        self.idle(runner, 2);
        assert_eq!(self.menu.hits, hits + 1, "history blocked a peer click");

        self.tap(runner, self.text_rect.center());
        self.idle(runner, 2);
        assert!(
            runner.ctx.memory(|m| m.has_focus(self.text_id)),
            "history blocked text focus"
        );
        assert_eq!(Keys::of(&runner.ctx, self.text_id), Keys::Latin);
        self.frame(runner, key(egui::Key::A, Modifiers::COMMAND));
        self.frame(runner, vec![Event::Text("recovered".into())]);
        assert_eq!(self.text, "recovered", "history blocked text replacement");

        self.tap(runner, self.number_rect.center());
        self.frame(runner, key(egui::Key::A, Modifiers::COMMAND));
        self.frame(runner, vec![Event::Text("42".into())]);
        self.tap(runner, OUTSIDE);
        self.idle(runner, 2);
        assert_eq!(self.number, 42.0, "history blocked numeric commit");

        let completed = self.hold_count;
        self.frame(runner, pointer(self.hold_rect.center(), true));
        self.idle(runner, SETTLE);
        assert_eq!(
            self.hold_count,
            completed + 1,
            "history blocked a fresh hold"
        );
        self.idle(runner, SETTLE);
        assert_eq!(self.hold_count, completed + 1, "one hold completed twice");
        self.frame(runner, pointer(self.hold_rect.center(), false));

        // This is an authoritative caller update, not an internal memory reset.
        self.wheel = 2;
        self.idle(runner, 120);
        assert_eq!(self.wheel, 2, "caller wheel update was overwritten");
        let start = self.wheel_rect.center();
        let moved = start - egui::vec2(0.0, self.wheel_rect.height() / 5.0);
        self.frame(runner, pointer(start, true));
        self.frame(runner, vec![Event::PointerMoved(moved)]);
        self.frame(runner, pointer(moved, false));
        assert_ne!(self.wheel, 2, "history blocked a fresh wheel drag");
        self.idle(runner, 120);

        self.menu.selected = 0;
        self.idle(runner, 2);
        self.tap(runner, self.menu.trigger.center());
        self.idle(runner, SETTLE);
        let option = runner
            .text("Bravo", self.menu.trigger)
            .expect("history blocked dropdown reopening");
        self.tap(runner, option);
        self.idle(runner, SETTLE);
        assert_eq!(self.menu.selected, 1, "history blocked dropdown selection");
        assert!(!self.popup_visible(runner), "selection did not close popup");

        (
            self.menu.selected,
            self.text.clone(),
            self.number.to_bits(),
            self.wheel,
            self.hold_count - completed,
        )
    }
}

fn profile(index: u8) -> (Opener, Trigger) {
    let opener = [
        Opener::Anchored,
        Opener::Grid,
        Opener::Sheet,
        Opener::Search,
    ][usize::from(index % 4)];
    let kind = [
        Trigger::Button,
        Trigger::Field(FieldLook::Outlined),
        Trigger::Field(FieldLook::Filled),
        Trigger::Field(FieldLook::Underlined),
        Trigger::Inline,
        Trigger::Tile,
        Trigger::Chip,
    ][usize::from(index / 4 % 7)];
    (opener, kind)
}

// The pristine comparator is independent of each subject's retained Context.
// Cache only its already-asserted public outcomes, never a subject or UI storage.
fn expected(profile_index: u8) -> &'static Outcome {
    static OUTCOMES: [OnceLock<Outcome>; 28] = [const { OnceLock::new() }; 28];
    OUTCOMES[usize::from(profile_index)].get_or_init(|| {
        let (opener, kind) = profile(profile_index);
        let mut runner = Runner::new();
        let mut fresh = Scene::new(opener, kind);
        fresh.idle(&mut runner, 3);
        fresh.promised_suffix(&mut runner)
    })
}

/// Each byte has a documented operation and timing; shrinking retains exact replay.
fn exercise(profile_index: u8, history: &[u8]) {
    let result = std::panic::catch_unwind(|| exercise_inner(profile_index, history));
    if let Err(payload) = result {
        eprintln!("UI history replay: profile={profile_index}, history={history:?}");
        std::panic::resume_unwind(payload);
    }
}

fn exercise_inner(profile_index: u8, history: &[u8]) -> Vec<(bool, bool, bool)> {
    let (opener, kind) = profile(profile_index);
    let mut runner = Runner::new();
    let mut scene = Scene::new(opener, kind);
    scene.idle(&mut runner, 3);
    let mut retired = Vec::new();
    for &operation in history {
        runner.dt = [1.0 / 120.0, 1.0 / 60.0, 1.0 / 15.0, 0.25][usize::from(operation / 16 % 4)];
        match operation % 16 {
            0 => scene.tap(&mut runner, scene.menu.trigger.center()),
            1 => scene.tap(&mut runner, OUTSIDE),
            2..=4 => {
                retired.push(scene.retire(&mut runner, operation % 16 - 2));
            }
            5 => scene.tap(&mut runner, scene.text_rect.center()),
            6 => scene.frame(&mut runner, vec![Event::Text("x".into())]),
            7 => scene.frame(&mut runner, pointer(scene.hold_rect.center(), true)),
            8 => scene.frame(&mut runner, pointer(OUTSIDE, false)),
            9 => scene.frame(&mut runner, pointer(scene.wheel_rect.center(), true)),
            10 => scene.frame(
                &mut runner,
                vec![Event::PointerMoved(
                    scene.wheel_rect.center() - egui::vec2(0.0, 55.0),
                )],
            ),
            11 => scene.frame(&mut runner, key(egui::Key::Escape, Modifiers::NONE)),
            12 => {
                scene.number = 10.00001;
                scene.wheel = 1;
                scene.frame(&mut runner, vec![]);
            }
            13 => scene.tap(&mut runner, scene.number_rect.center()),
            14 => scene.frame(&mut runner, key(egui::Key::Tab, Modifiers::NONE)),
            _ => scene.idle(&mut runner, 16),
        }
        scene.idle(&mut runner, usize::from(operation / 64));
    }
    let actual = scene.promised_suffix(&mut runner);

    assert_eq!(
        &actual,
        expected(profile_index),
        "prior history changed the promised suffix outcome"
    );
    retired
}

/// Timing bits must preserve the owner interruption actually applied during replay.
#[test]
fn timing_bits_preserve_owner_interruption_modes() {
    for (opcode, expected) in [
        (2, (false, true, true)),
        (3, (true, false, true)),
        (4, (true, true, false)),
    ] {
        for timing in [
            0x00, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xa0, 0xb0, 0xc0, 0xd0,
            0xe0, 0xf0,
        ] {
            let byte = timing | opcode;
            assert_eq!(
                exercise_inner(0, &[byte]),
                vec![expected],
                "history byte {byte:#04x} changed the interrupted owner"
            );
        }
    }
}

#[test]
fn all_widget_forms_recover_after_ordered_interruption_histories() {
    for profile in 0..28 {
        for first in 0..5 {
            for second in 0..5 {
                exercise(profile, &[0, first, 0, second + 64]);
            }
        }
    }
}

#[test]
fn mixed_capture_focus_and_caller_update_regressions() {
    for profile in 0..28 {
        for history in [
            &[5, 6, 0, 5, 1, 14][..],
            &[7, 2, 15, 8, 0, 1],
            &[9, 10, 3, 10, 8, 12],
            &[13, 6, 12, 1, 0, 4],
            &[0, 0, 5, 6, 2, 0, 1],
        ] {
            exercise(profile, history);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        max_shrink_iters: 2048,
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::Direct(
                concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../target/assurance/ui-histories/proptest-regressions.txt"
                )
            )
        )),
        ..ProptestConfig::default()
    })]

    #[test]
    fn generated_histories_preserve_promised_operations(
        profile in 0u8..28,
        history in prop::collection::vec(any::<u8>(), 0..25),
    ) {
        exercise(profile, &history);
    }
}

#[test]
fn closed_menu_cleanup_preserves_the_other_fields_focus() {
    exercise(3, &[5, 6, 0, 1, 5]);
}

#[test]
fn outside_dismissal_consumes_one_click_and_releases_the_next() {
    for opener in [
        Opener::Anchored,
        Opener::Grid,
        Opener::Sheet,
        Opener::Search,
    ] {
        let mut runner = Runner::new();
        let mut scene = Scene::new(opener, Trigger::Button);
        scene.idle(&mut runner, 3);
        scene.tap(&mut runner, scene.menu.trigger.center());
        scene.idle(&mut runner, SETTLE);
        assert!(scene.popup_visible(&runner), "initial popup failed to open");
        let before = scene.menu.hits;
        scene.tap(&mut runner, scene.menu.peer.center());
        assert_eq!(scene.menu.hits, before, "popup dismissal clicked through");
        // No settling loop, pointer retry or Context reset between these clicks.
        scene.tap(&mut runner, scene.menu.peer.center());
        assert_eq!(
            scene.menu.hits,
            before + 1,
            "dismissed popup blocked the next peer click"
        );
    }
}

#[test]
fn every_profile_can_dismiss_after_its_first_open_at_each_frame_interval() {
    for profile in 0..28 {
        for operation in [0, 16, 32, 48] {
            exercise(profile, &[operation]);
        }
    }
}
