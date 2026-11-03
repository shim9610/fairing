//! Concrete input witnesses for the lifecycle protocol. These do not model egui internals.
use fairing::egui::{self, Event, Modifiers, PointerButton, Pos2, Rect};
use fairing::fairing_widgets::WidgetCx;
use fairing::widgets::{Dropdown, FieldLook, NumberField, Opener, PatternPad, TextField, Trigger};
use fairing::{icons::IconSet, motion::AnimationStore, theme::Theme};

struct Runner {
    ctx: egui::Context,
    theme: Theme,
    icons: IconSet,
    anims: AnimationStore,
    frame: u64,
    shapes: Vec<egui::epaint::ClippedShape>,
}
impl Runner {
    fn new() -> Self {
        Self {
            ctx: egui::Context::default(),
            theme: Theme::light(),
            icons: IconSet::new(),
            anims: AnimationStore::new(),
            frame: 0,
            shapes: vec![],
        }
    }
    fn frame(
        &mut self,
        events: Vec<Event>,
        mut draw: impl FnMut(&mut egui::Ui, &mut WidgetCx<'_>),
    ) {
        self.anims.tick(1.0 / 60.0, self.frame);
        let mut cx = WidgetCx {
            theme: &self.theme,
            icons: &mut self.icons,
            anims: &mut self.anims,
            anim_scope: egui::Id::unique("lifecycle"),
            frame: self.frame,
            inset_bottom: 0.0,
            painters: None,
        };
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 900.0))),
                time: Some(self.frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |ui| draw(ui, &mut cx),
        );
        self.shapes = std::mem::take(&mut output.shapes);
        output.drop_without_applying_deltas();
        self.frame += 1;
    }
    fn text(&self, word: &str, outside: Rect) -> Option<Pos2> {
        self.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.job.text == word => {
                let rect = t.galley.rect.translate(t.pos.to_vec2());
                (!outside.contains_rect(rect) && shape.clip_rect.intersects(rect))
                    .then_some(rect.center())
            }
            _ => None,
        })
    }
}
fn pointer(pos: Pos2, pressed: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        },
    ]
}
const OPTIONS: [&str; 3] = ["Alpha", "Bravo", "Charlie"];
struct Menu {
    mounted: bool,
    enabled: bool,
    parent_enabled: bool,
    selected: usize,
    trigger: Rect,
    peer: Rect,
    hits: usize,
    opener: Opener,
    kind: Trigger,
}
impl Menu {
    fn new(opener: Opener) -> Self {
        Self {
            mounted: true,
            enabled: true,
            parent_enabled: true,
            selected: 0,
            trigger: Rect::NOTHING,
            peer: Rect::NOTHING,
            hits: 0,
            opener,
            kind: Trigger::Button,
        }
    }
    fn draw(&mut self, ui: &mut egui::Ui, cx: &mut WidgetCx<'_>) {
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt("owner")
                .max_rect(Rect::from_min_size(
                    egui::pos2(20.0, 20.0),
                    egui::vec2(300.0, 800.0),
                )),
            |ui| {
                if self.mounted {
                    ui.add_enabled_ui(self.parent_enabled, |ui| {
                        self.trigger = Dropdown::new("menu", &OPTIONS, &mut self.selected)
                            .enabled(self.enabled)
                            .trigger(self.kind)
                            .opener(self.opener)
                            .show(ui, cx)
                            .rect;
                    });
                }
            },
        );
        self.peer = Rect::from_min_size(egui::pos2(730.0, 60.0), egui::vec2(120.0, 60.0));
        if ui.put(self.peer, egui::Button::new("peer")).clicked() {
            self.hits += 1;
        }
    }
    fn frames(&mut self, r: &mut Runner, n: usize) {
        for _ in 0..n {
            r.frame(vec![], |ui, cx| self.draw(ui, cx));
        }
    }
    fn tap(&mut self, r: &mut Runner, pos: Pos2) {
        for pressed in [true, false] {
            r.frame(pointer(pos, pressed), |ui, cx| self.draw(ui, cx));
        }
        self.frames(r, 16);
    }
    fn open(&mut self, r: &mut Runner) {
        self.tap(r, self.trigger.center());
        assert!(
            r.text("Bravo", self.trigger).is_some(),
            "promised open was blocked: {:?}",
            self.opener
        );
    }
    fn probe(&mut self, r: &mut Runner) {
        assert!(
            r.text("Bravo", self.trigger).is_none(),
            "a retired popup revived"
        );
        let before = self.hits;
        self.tap(r, self.peer.center());
        assert_eq!(
            self.hits,
            before + 1,
            "a retired area blocked a promised peer click"
        );
        self.selected = 0;
        self.frames(r, 2);
        self.open(r);
        let target = r.text("Bravo", self.trigger).expect("open witness missing");
        self.tap(r, target);
        assert_eq!(self.selected, 1, "a promised selection was blocked");
    }
}

/// The same owner/context survives each prefix and each probe. Only public business
/// inputs change; clearing egui memory/focus between steps would hide the defect.
#[test]
fn every_interruption_prefix_preserves_popup_and_peer_operations() {
    for kind in [
        Trigger::Button,
        Trigger::Field(FieldLook::Outlined),
        Trigger::Field(FieldLook::Filled),
        Trigger::Field(FieldLook::Underlined),
        Trigger::Inline,
        Trigger::Tile,
        Trigger::Chip,
    ] {
        for opener in [
            Opener::Anchored,
            Opener::Grid,
            Opener::Sheet,
            Opener::Search,
        ] {
            for first in 0..4 {
                for second in 0..4 {
                    let mut r = Runner::new();
                    let mut m = Menu::new(opener);
                    m.kind = kind;
                    m.frames(&mut r, 3);
                    for action in [first, second] {
                        m.open(&mut r);
                        match action {
                            0 => {
                                // Explicit outside dismissal, including its old focus state.
                                m.tap(&mut r, egui::pos2(600.0, 100.0));
                            }
                            1 => {
                                m.mounted = false;
                                m.frames(&mut r, 1);
                                m.mounted = true;
                            }
                            2 => {
                                m.enabled = false;
                                m.frames(&mut r, 1);
                                m.enabled = true;
                            }
                            _ => {
                                m.parent_enabled = false;
                                m.frames(&mut r, 1);
                                m.parent_enabled = true;
                            }
                        }
                        m.frames(&mut r, 3);
                        m.probe(&mut r);
                    }
                }
            }
        }
    }
}

#[test]
fn an_interrupted_pattern_cannot_submit_on_a_later_release_and_can_restart() {
    for disabled in [false, true] {
        let mut r = Runner::new();
        let mut mounted = true;
        let mut enabled = true;
        let mut path = vec![];
        let mut submitted = 0;
        let mut a = Pos2::ZERO;
        let mut b = Pos2::ZERO;
        let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>, mounted: bool, enabled: bool| {
            if mounted {
                let result = PatternPad::new(&mut path)
                    .min_points(2)
                    .enabled(enabled)
                    .show(ui, cx);
                a = result.dot_center(0).expect("dot 0");
                b = result.dot_center(1).expect("dot 1");
                submitted += usize::from(result.submitted);
            }
        };
        r.frame(vec![], |ui, cx| draw(ui, cx, mounted, enabled));
        // End the borrow before reading the positions; the same data is kept below.
        drop(draw);
        let start = a;
        let end = b;
        let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>, mounted: bool, enabled: bool| {
            if mounted {
                submitted += usize::from(
                    PatternPad::new(&mut path)
                        .min_points(2)
                        .enabled(enabled)
                        .show(ui, cx)
                        .submitted,
                );
            }
        };
        r.frame(pointer(start, true), |ui, cx| {
            draw(ui, cx, mounted, enabled)
        });
        r.frame(vec![Event::PointerMoved(end)], |ui, cx| {
            draw(ui, cx, mounted, enabled)
        });
        if disabled {
            enabled = false;
        } else {
            mounted = false;
        }
        r.frame(vec![], |ui, cx| draw(ui, cx, mounted, enabled));
        enabled = true;
        mounted = true;
        r.frame(pointer(end, false), |ui, cx| draw(ui, cx, mounted, enabled));
        drop(draw);
        assert_eq!(
            submitted, 0,
            "an interrupted trace submitted after its owner retired"
        );
        let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>| {
            submitted += usize::from(
                PatternPad::new(&mut path)
                    .min_points(2)
                    .show(ui, cx)
                    .submitted,
            );
        };
        r.frame(pointer(start, true), &mut draw);
        r.frame(vec![Event::PointerMoved(end)], &mut draw);
        r.frame(pointer(end, false), &mut draw);
        drop(draw);
        assert_eq!(
            submitted, 1,
            "a fresh pattern could not complete after interruption"
        );
    }
}

#[test]
fn stale_numeric_drafts_cannot_override_authoritative_values() {
    for interruption in 0..6 {
        let mut r = Runner::new();
        let mut value = 10.0;
        let mut rect = Rect::NOTHING;
        let mut draw =
            |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>, value: &mut f64, enabled: bool| {
                rect = NumberField::new(value).enabled(enabled).show(ui, cx).rect;
            };
        r.frame(vec![], |ui, cx| draw(ui, cx, &mut value, true));
        drop(draw);
        let center = rect.center();
        for pressed in [true, false] {
            r.frame(pointer(center, pressed), |ui, cx| {
                NumberField::new(&mut value).show(ui, cx);
            });
        }
        let select_all = Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::COMMAND,
        };
        r.frame(vec![select_all, Event::Text("87".into())], |ui, cx| {
            NumberField::new(&mut value).show(ui, cx);
        });
        assert!(
            r.text("87", Rect::NOTHING).is_some(),
            "numeric typing did not reach the live draft"
        );
        assert_eq!(value, 10.0, "draft committed before its promised blur");
        match interruption {
            0 => r.frame(vec![], |_, _| {}),
            1 => r.frame(vec![], |ui, cx| {
                NumberField::new(&mut value).enabled(false).show(ui, cx);
            }),
            _ => {}
        }
        if interruption != 3 {
            value = if interruption == 5 { 10.00001 } else { 30.0 };
        }
        if interruption != 4 && interruption != 5 {
            r.frame(vec![], |ui, cx| {
                NumberField::new(&mut value).show(ui, cx);
            });
        }
        let outside = egui::pos2(700.0, 700.0);
        for pressed in [true, false] {
            r.frame(pointer(outside, pressed), |ui, cx| {
                NumberField::new(&mut value).show(ui, cx);
            });
        }
        assert_eq!(
            value,
            if interruption == 3 {
                87.0
            } else if interruption == 5 {
                10.00001
            } else {
                30.0
            },
            "a stale numeric draft overwrote the authoritative value, or valid editing stopped"
        );
    }
}

#[test]
fn absent_or_disabled_fields_cannot_keep_a_keyboard_request_alive() {
    use fairing::widgets::Keys;
    let mut r = Runner::new();
    let mut text = String::new();
    let mut id = egui::Id::NULL;
    r.frame(vec![], |ui, cx| {
        id = TextField::new(&mut text).keys(Keys::Digits).show(ui, cx).id;
    });
    assert_eq!(Keys::of(&r.ctx, id), Keys::Digits);
    r.frame(vec![], |_, _| {});
    let _ = Keys::of(&r.ctx, id); // A keyboard consumer is not the owner.
    r.frame(vec![], |_, _| {});
    assert_eq!(
        Keys::of(&r.ctx, id),
        Keys::Any,
        "a keyboard reader renewed an absent field"
    );
}

#[test]
fn a_disabled_field_releases_only_its_own_keyboard_and_focus() {
    use fairing::widgets::Keys;
    let mut r = Runner::new();
    let mut text = String::new();
    let mut id = egui::Id::NULL;
    r.frame(vec![], |ui, cx| {
        id = TextField::new(&mut text).keys(Keys::Digits).show(ui, cx).id;
    });
    assert_eq!(Keys::of(&r.ctx, id), Keys::Digits);
    r.ctx.memory_mut(|m| m.request_focus(id));
    r.frame(vec![], |ui, cx| {
        TextField::new(&mut text)
            .enabled(false)
            .keys(Keys::Digits)
            .show(ui, cx);
    });
    assert_eq!(
        Keys::of(&r.ctx, id),
        Keys::Any,
        "disabled input retained its keyboard request"
    );
    assert!(
        !r.ctx.memory(|m| m.has_focus(id)),
        "disabled input kept focus"
    );
}

#[test]
fn a_new_hold_cannot_inherit_a_completed_or_interrupted_hold() {
    use fairing::widgets::BigButton;
    use std::time::Duration;
    let mut r = Runner::new();
    let mut rect = Rect::NOTHING;
    let mut completions = 0;
    r.frame(vec![], |ui, cx| {
        rect = BigButton::new("hold")
            .long_press(Duration::from_millis(100))
            .show(ui, cx)
            .response
            .rect;
    });
    let at = rect.center();
    let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>| {
        completions += usize::from(
            BigButton::new("hold")
                .long_press(Duration::from_millis(100))
                .show(ui, cx)
                .long_pressed(),
        );
    };
    r.frame(pointer(at, true), &mut draw);
    for _ in 0..16 {
        r.frame(vec![], &mut draw);
    }
    drop(draw);
    assert_eq!(completions, 1, "initial hold did not complete exactly once");
    r.frame(pointer(at, false), |_, _| {});
    r.frame(vec![], |_, _| {});
    r.frame(vec![], |ui, cx| {
        BigButton::new("hold")
            .long_press(Duration::from_millis(100))
            .show(ui, cx);
    });
    let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>| {
        completions += usize::from(
            BigButton::new("hold")
                .long_press(Duration::from_millis(100))
                .show(ui, cx)
                .long_pressed(),
        );
    };
    r.frame(pointer(at, true), &mut draw);
    drop(draw);
    assert_eq!(
        completions, 1,
        "old progress completed a fresh hold immediately"
    );
    for _ in 0..16 {
        r.frame(vec![], |ui, cx| {
            completions += usize::from(
                BigButton::new("hold")
                    .long_press(Duration::from_millis(100))
                    .show(ui, cx)
                    .long_pressed(),
            );
        });
    }
    assert_eq!(
        completions, 2,
        "a fresh hold remained blocked after its owner returned"
    );
}

#[test]
fn an_old_held_pointer_cannot_arm_a_returned_button() {
    use fairing::widgets::BigButton;
    use std::time::Duration;
    for interruption in 0..3 {
        let mut r = Runner::new();
        let mut rect = Rect::NOTHING;
        let mut completed = 0;
        let mut draw = |ui: &mut egui::Ui,
                        cx: &mut WidgetCx<'_>,
                        visible: bool,
                        enabled: bool,
                        parent: bool| {
            ui.add_enabled_ui(parent, |ui| {
                if visible {
                    let result = BigButton::new("hold")
                        .enabled(enabled)
                        .long_press(Duration::from_millis(100))
                        .show(ui, cx);
                    rect = result.response.rect;
                    completed += usize::from(result.long_pressed());
                }
            });
        };
        r.frame(vec![], |ui, cx| draw(ui, cx, true, true, true));
        drop(draw);
        let at = rect.center();
        let mut draw = |ui: &mut egui::Ui,
                        cx: &mut WidgetCx<'_>,
                        visible: bool,
                        enabled: bool,
                        parent: bool| {
            ui.add_enabled_ui(parent, |ui| {
                if visible {
                    completed += usize::from(
                        BigButton::new("hold")
                            .enabled(enabled)
                            .long_press(Duration::from_millis(100))
                            .show(ui, cx)
                            .long_pressed(),
                    );
                }
            });
        };
        r.frame(pointer(at, true), |ui, cx| draw(ui, cx, true, true, true));
        r.frame(vec![], |ui, cx| {
            draw(
                ui,
                cx,
                interruption != 0,
                interruption != 1,
                interruption != 2,
            )
        });
        for _ in 0..16 {
            r.frame(vec![], |ui, cx| draw(ui, cx, true, true, true));
        }
        drop(draw);
        assert_eq!(
            completed, 0,
            "an old held pointer armed a retired button: {interruption}"
        );
        let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>| {
            completed += usize::from(
                BigButton::new("hold")
                    .long_press(Duration::from_millis(100))
                    .show(ui, cx)
                    .long_pressed(),
            );
        };
        r.frame(pointer(at, false), &mut draw);
        r.frame(pointer(at, true), &mut draw);
        for _ in 0..16 {
            r.frame(vec![], &mut draw);
        }
        drop(draw);
        assert_eq!(
            completed, 1,
            "a fresh press remained blocked after retiring old capture"
        );
    }
}

#[test]
fn a_zero_duration_hold_completes_once_per_fresh_press() {
    use fairing::widgets::BigButton;
    use std::time::Duration;
    let mut r = Runner::new();
    let mut rect = Rect::NOTHING;
    r.frame(vec![], |ui, cx| {
        rect = BigButton::new("hold")
            .long_press(Duration::ZERO)
            .show(ui, cx)
            .response
            .rect;
    });
    let at = rect.center();
    let mut count = 0;
    for expected in 1..=2 {
        let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>| {
            count += usize::from(
                BigButton::new("hold")
                    .long_press(Duration::ZERO)
                    .show(ui, cx)
                    .long_pressed(),
            );
        };
        r.frame(pointer(at, true), &mut draw);
        for _ in 0..4 {
            r.frame(vec![], &mut draw);
        }
        r.frame(pointer(at, false), &mut draw);
        drop(draw);
        assert_eq!(
            count, expected,
            "zero-duration hold must complete exactly once per fresh press"
        );
    }
}

#[test]
fn a_wheel_requires_a_fresh_drag_after_its_owner_is_interrupted() {
    use fairing::widgets::WheelPicker;
    let options = ["zero", "one", "two", "three", "four"];
    for interruption in 0..3 {
        let mut r = Runner::new();
        let mut value = 2;
        let mut rect = Rect::NOTHING;
        r.frame(vec![], |ui, cx| {
            rect = WheelPicker::new("wheel", &options, &mut value)
                .show(ui, cx)
                .rect;
        });
        let start = rect.center();
        let moved = start - egui::vec2(0.0, rect.height() / 5.0);
        let draw = |ui: &mut egui::Ui,
                    cx: &mut WidgetCx<'_>,
                    value: &mut usize,
                    visible: bool,
                    enabled: bool,
                    parent: bool| {
            ui.add_enabled_ui(parent, |ui| {
                if visible {
                    WheelPicker::new("wheel", &options, value)
                        .enabled(enabled)
                        .show(ui, cx);
                }
            });
        };
        // Use this same scope for the warm-up and all subsequent input.
        r.frame(vec![], |ui, cx| draw(ui, cx, &mut value, true, true, true));
        r.frame(pointer(start, true), |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        r.frame(vec![Event::PointerMoved(moved)], |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        r.frame(vec![], |ui, cx| {
            draw(
                ui,
                cx,
                &mut value,
                interruption != 0,
                interruption != 1,
                interruption != 2,
            )
        });
        value = 2;
        let after = moved - egui::vec2(0.0, rect.height() / 5.0);
        r.frame(vec![Event::PointerMoved(after)], |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        r.frame(pointer(after, false), |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        for _ in 0..16 {
            r.frame(vec![], |ui, cx| draw(ui, cx, &mut value, true, true, true));
        }
        assert_eq!(
            value, 2,
            "an old drag changed a returned wheel: {interruption}"
        );
        r.frame(pointer(start, true), |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        r.frame(vec![Event::PointerMoved(moved)], |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        r.frame(pointer(moved, false), |ui, cx| {
            draw(ui, cx, &mut value, true, true, true)
        });
        assert_ne!(
            value, 2,
            "a fresh wheel drag remained blocked after interruption"
        );
    }
}

#[test]
fn a_completed_hold_cannot_refire_when_its_animation_progress_changes() {
    use fairing::motion::Tween;
    use fairing::widgets::BigButton;
    use std::time::Duration;
    let mut r = Runner::new();
    let mut id = egui::Id::NULL;
    let mut rect = Rect::NOTHING;
    r.frame(vec![], |ui, cx| {
        let button = BigButton::new("hold")
            .long_press(Duration::from_millis(100))
            .show(ui, cx);
        id = button.response.id;
        rect = button.response.rect;
    });
    let mut completed = 0;
    let mut draw = |ui: &mut egui::Ui, cx: &mut WidgetCx<'_>| {
        completed += usize::from(
            BigButton::new("hold")
                .long_press(Duration::from_millis(100))
                .show(ui, cx)
                .long_pressed(),
        );
    };
    r.frame(pointer(rect.center(), true), &mut draw);
    for _ in 0..16 {
        r.frame(vec![], &mut draw);
    }
    drop(draw);
    assert_eq!(completed, 1, "initial hold did not complete");
    // Public animation storage can be changed independently of the retained press.
    r.anims.animate(
        id.with("ring").with(egui::Id::unique("lifecycle")),
        0.0,
        Tween::instant(),
        r.frame,
    );
    for _ in 0..16 {
        r.frame(vec![], |ui, cx| {
            completed += usize::from(
                BigButton::new("hold")
                    .long_press(Duration::from_millis(100))
                    .show(ui, cx)
                    .long_pressed(),
            );
        });
    }
    assert_eq!(
        completed, 1,
        "a completed hold re-fired when animation progress changed"
    );
}
