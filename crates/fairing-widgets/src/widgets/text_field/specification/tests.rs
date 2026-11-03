//! Reachable concrete witnesses for checking the sufficiency of the current state specification.
#![allow(
    clippy::indexing_slicing,
    reason = "private two-editor fixture uses only targets 0 and 1"
)]

use super::Keys;

use crate::interaction::concrete::{lease_group, LeaseGroup, Lifetime, Pass, Pointer, Session};
use crate::{icons::IconSet, motion::AnimationStore, theme::Theme, widgets::TextField, WidgetCx};
use egui::{Context, Event, Id, Modifiers, PointerButton, Pos2, RawInput, Rect};

struct Scene {
    ctx: Context,
    theme: Theme,
    icons: IconSet,
    anims: AnimationStore,
    frame: u64,
    text: [String; 2],
    ids: [Id; 2],
    rects: [Rect; 2],
    pointers: [Pointer; 2],
}

impl Scene {
    fn new() -> Self {
        Self {
            ctx: Context::default(),
            theme: Theme::light(),
            icons: IconSet::new(),
            anims: AnimationStore::new(),
            frame: 0,
            text: [String::new(), String::new()],
            ids: [Id::NULL; 2],
            rects: [Rect::NOTHING; 2],
            pointers: [Pointer {
                fresh: false,
                down: false,
                held: false,
                clicked: false,
                dragged: false,
                stopped: false,
            }; 2],
        }
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "the concrete trace has ten frames"
    )]
    fn frame(&mut self, events: Vec<Event>) {
        self.anims.tick(1.0 / 60.0, self.frame);
        let mut cx = WidgetCx {
            theme: &self.theme,
            icons: &mut self.icons,
            anims: &mut self.anims,
            anim_scope: Id::unique("specification-completeness"),
            frame: self.frame,
            inset_bottom: 0.0,
            painters: None,
        };
        self.ctx
            .run_ui(
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 600.0))),
                    time: Some(self.frame as f64 / 60.0),
                    events,
                    ..RawInput::default()
                },
                |ui| {
                    ui.horizontal(|ui| {
                        for (i, salt) in ["left", "right"].into_iter().enumerate() {
                            let response = TextField::new(&mut self.text[i])
                                .id_salt(salt)
                                .keys(Keys::Latin)
                                .width(300.0)
                                .show(ui, &mut cx);
                            assert!(
                                response.enabled(),
                                "fixture unexpectedly disabled an editor"
                            );
                            self.ids[i] = response.id;
                            self.rects[i] = response.rect;
                            self.pointers[i] = crate::interaction::pointer(&response);
                        }
                    });
                },
            )
            .drop_without_applying_deltas();
        self.frame += 1;
    }

    fn focus(&mut self, target: usize) {
        for _ in 0..3 {
            self.frame(Vec::new());
        }
        let at = self.rects[target].center();
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(at),
                Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
        for _ in 0..3 {
            self.frame(Vec::new());
        }
        self.frame(vec![Event::PointerMoved(egui::pos2(900.0, 500.0))]);
        self.frame(Vec::new());
    }

    /// Keep the full existing carrier inputs, not only their coarse group labels.
    /// Other retained-owner kinds are absent in this supported two-TextField scene.
    fn current_view(&self) -> CurrentView {
        let now = self.ctx.cumulative_pass_nr();
        let requests = self.ids.map(|id| {
            self.ctx.data(|data| {
                let raw = egui::util::id_type_map::RawKey::new::<Session<Keys>>(Keys::slot(id));
                data.get_temp_raw(raw)
                    .and_then(|v| v.downcast_ref::<Session<Keys>>())
                    .cloned()
            })
        });
        CurrentView {
            pass: Pass {
                now,
                enabled: true,
                lifetime: Lifetime::Mounted,
                fresh_press: self.ctx.input(|input| input.pointer.primary_pressed()),
            },
            frame: self.frame,
            ids: self.ids.map(|id| id.value()),
            requests,
            pointers: self.pointers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CurrentView {
    pass: Pass,
    frame: u64,
    ids: [u64; 2],
    requests: [Option<Session<Keys>>; 2],
    pointers: [Pointer; 2],
}

impl CurrentView {
    fn encode(&self) -> Vec<u64> {
        // Exhaustive patterns make added product fields/variants a compilation error.
        let Pass {
            now,
            enabled,
            lifetime,
            fresh_press,
        } = self.pass;
        let lifetime = match lifetime {
            Lifetime::Mounted => 0,
            Lifetime::Press => 1,
        };
        let mut out = vec![
            now,
            u64::from(enabled),
            lifetime,
            u64::from(fresh_press),
            self.frame,
        ];
        for pointer in self.pointers {
            let Pointer {
                fresh,
                down,
                held,
                clicked,
                dragged,
                stopped,
            } = pointer;
            out.extend([
                u64::from(fresh),
                u64::from(down),
                u64::from(held),
                u64::from(clicked),
                u64::from(dragged),
                u64::from(stopped),
            ]);
        }
        for (id, saved) in self.ids.iter().zip(&self.requests) {
            out.push(*id);
            let group = match lease_group(saved.as_ref(), self.pass) {
                LeaseGroup::Disabled => 0,
                LeaseGroup::Missing => 1,
                LeaseGroup::Future => 2,
                LeaseGroup::Expired => 3,
                LeaseGroup::ReplacedPress => 4,
                LeaseGroup::Live => 5,
            };
            out.push(group);
            if let Some(Session { seen, payload }) = saved {
                out.extend([1, *seen]);
                match payload {
                    Keys::Any => out.extend([0, 0, 0]),
                    Keys::Digits => out.extend([1, 0, 0]),
                    Keys::Number { decimal, sign } => {
                        out.extend([2, u64::from(*decimal), u64::from(*sign)]);
                    }
                    Keys::Latin => out.extend([3, 0, 0]),
                }
            } else {
                out.extend([0, 0, 0, 0, 0]);
            }
        }
        out
    }
}

struct Witness {
    view: CurrentView,
    focused: u64,
    receiver: u64,
}

fn witness(target: usize) -> Witness {
    let mut scene = Scene::new();
    scene.focus(target);
    assert!(
        scene
            .ctx
            .memory(|memory| memory.has_focus(scene.ids[target])),
        "pointer did not focus its target"
    );
    assert_eq!(
        scene.text,
        [String::new(), String::new()],
        "unexpected prefix edit"
    );
    let view = scene.current_view();
    assert_ne!(view.ids[0], view.ids[1], "fixture IDs collided");
    for saved in &view.requests {
        assert!(
            matches!(
                saved,
                Some(Session {
                    payload: Keys::Latin,
                    ..
                })
            ),
            "fixture did not capture the actual keyboard request"
        );
        assert_eq!(
            lease_group(saved.as_ref(), view.pass),
            LeaseGroup::Live,
            "fixture captured an expired request"
        );
    }
    let focused = scene
        .ctx
        .memory(egui::Memory::focused)
        .map_or(0, |id| id.value());
    scene.frame(vec![Event::Text("x".to_owned())]);
    assert_eq!(
        scene.text[target], "x",
        "keyboard event did not reach its focused field"
    );
    assert!(
        scene.text[1 - target].is_empty(),
        "keyboard event reached the wrong field"
    );
    Witness {
        view,
        focused,
        receiver: if scene.text[0] == "x" { 1 } else { 2 },
    }
}

fn encoded(w: &Witness) -> String {
    let view = w
        .view
        .encode()
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"view\":[{view}],\"focused\":{},\"receiver\":{}}}",
        w.focused, w.receiver
    )
}

#[test]
fn reachable_focus_states_emit_current_specification_witnesses() -> std::io::Result<()> {
    let left = witness(0);
    let right = witness(1);
    assert_eq!(
        left.view == right.view,
        left.view.encode() == right.view.encode(),
        "view encoding lost a modeled field"
    );
    assert_ne!(
        left.focused, right.focused,
        "witnesses did not change focus"
    );
    assert_ne!(
        left.receiver, right.receiver,
        "witnesses did not change text routing"
    );
    if let Some(path) = std::env::var_os("FAIRING_SPECIFICATION_WITNESS") {
        std::fs::write(
            path,
            format!(
                "{{\"schema\":1,\"scene\":\"two-editors\",\"states\":[{},{}]}}\n",
                encoded(&left),
                encoded(&right)
            ),
        )?;
    }
    Ok(())
}
