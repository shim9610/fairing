//! UI transition witnesses: logical permission alone is not an executable input path.
//! Setup may use host APIs; each claimed transition below must occur through real pointer input.
use fairing::access::Gate;
use fairing::gesture::{GestureRegion, Phase, RegionCx, RegionPlaceCx, RegionTouch};
use fairing::testing::{access_config, test_shell, Harness};
use fairing::{egui, screen, Cx, LaunchAction, Level};
use std::cell::Cell;
use std::rc::Rc;

struct Occluder {
    rect: Rc<Cell<Option<egui::Rect>>>,
    open: Rc<Cell<bool>>,
}
impl GestureRegion for Occluder {
    fn place(&mut self, _: &RegionPlaceCx<'_>) -> Option<egui::Rect> {
        self.rect.get()
    }
    fn touch(&mut self, touch: &RegionTouch, cx: &mut RegionCx<'_>) {
        if touch.phase == Phase::Ended && self.open.replace(false) {
            cx.launch_gated(LaunchAction::open("protected"), "protected");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Prompt,
    Protected,
    Panel,
    Other,
}

// Explicit obligations distinguish required edges from intentionally blocked input.
const REQUIRED_EDGES: [(State, State); 6] = [
    (State::Start, State::Prompt),
    (State::Prompt, State::Start),
    (State::Prompt, State::Protected),
    (State::Protected, State::Start),
    (State::Start, State::Panel),
    (State::Panel, State::Start),
];

struct Scene {
    h: Harness,
    target: Rc<Cell<Option<egui::Rect>>>,
    draws: Rc<Cell<usize>>,
    region: Rc<Cell<Option<egui::Rect>>>,
    witnessed: Vec<bool>,
    region_action: Rc<Cell<bool>>,
}
impl Scene {
    fn new(width: u16, height: u16) -> Self {
        let mut config = access_config(&["viewer", "admin"], Some("top"));
        config.access.gates.insert("public".into(), "bottom".into());
        config
            .access
            .gates
            .insert("overlay.open".into(), "bottom".into());
        config
            .access
            .pin_table
            .pins
            .insert("admin".into(), "1234".into());
        let target = Rc::new(Cell::new(None));
        let rect = target.clone();
        let draws = Rc::new(Cell::new(0));
        let count = draws.clone();
        let mut h = test_shell(config, move |shell| {
            shell.add(
                screen("start", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                    count.set(count.get() + 1);
                    let response = ui.button("Protected action");
                    rect.set(Some(response.rect));
                    if response.clicked() {
                        cx.launch(LaunchAction::open("protected"));
                    }
                    if ui.button("Open panel").clicked() {
                        cx.shell.toggle_overlay();
                    }
                })
                .gate("public"),
            );
            shell.add(screen("protected", |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                if ui.button("Return to start").clicked() {
                    cx.launch(LaunchAction::open("start"));
                }
            }));
        })
        .unwrap();
        h.set_size(f32::from(width), f32::from(height));
        h.shell.launch(LaunchAction::open("start")); // Setup, not a transition witness.
        h.frames(4);
        let region = Rc::new(Cell::new(None));
        let region_action = Rc::new(Cell::new(false));
        h.shell
            .add_gesture_region(
                "occluder",
                Occluder {
                    rect: region.clone(),
                    open: region_action.clone(),
                },
            )
            .unwrap();
        Self {
            h,
            target,
            draws,
            region,
            witnessed: vec![false; REQUIRED_EDGES.len()],
            region_action,
        }
    }

    fn state(&self) -> State {
        if self.h.shell.unlock_prompt_visible() {
            return State::Prompt;
        }
        if !self.h.shell.overlay().is_closed() {
            return State::Panel;
        }
        match self
            .h
            .shell
            .workspace()
            .focused()
            .map(|instance| instance.decl_id())
        {
            Some("start") => State::Start,
            Some("protected") => State::Protected,
            _ => State::Other,
        }
    }

    fn record(&mut self, edge: usize) {
        assert_eq!(
            self.state(),
            REQUIRED_EDGES[edge].1,
            "UI transition remained unreachable: edge={edge}"
        );
        self.witnessed[edge] = true;
    }

    fn witness(&mut self, edge: &str, target: egui::Rect, reached: impl Fn(&Harness) -> bool) {
        let visible = target.intersect(self.h.screen_rect());
        assert!(
            visible.is_positive() && visible.is_finite(),
            "UI target outside viewport: edge={edge}, target={target:?}"
        );
        // Bounded retries allow egui's previous-pass geometry to settle. No host action,
        // keyboard shortcut, or direct state mutation is allowed to satisfy the edge.
        for _ in 0..3 {
            self.h.tap(visible.center());
            if reached(&self.h) {
                return;
            }
        }
        panic!("UI transition remained unreachable: edge={edge}, target={target:?}, viewport={:?}, layer={:?}, prompt={}, overlay_closed={}, faulted={}, frames={}",
            self.h.screen_rect(), self.h.ctx.layer_id_at(visible.center()), self.h.shell.unlock_prompt_visible(),
            self.h.shell.overlay().is_closed(), self.h.shell.is_faulted(), self.h.frames);
    }

    fn text_target(&mut self, label: &str) -> egui::Rect {
        fn find(shape: &egui::Shape, label: &str, found: &mut Option<egui::Rect>) {
            match shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    *found = Some(text.galley.rect.translate(text.pos.to_vec2()))
                }
                egui::Shape::Vec(items) => {
                    for item in items {
                        find(item, label, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = None;
        for clipped in self.h.frame_shapes() {
            find(&clipped.shape, label, &mut found);
        }
        found.unwrap_or_else(|| panic!("Required UI control is not rendered: {label}"))
    }

    fn request_unlock(&mut self) {
        assert!(!self.h.shell.access().allows(&Gate::from("protected")));
        let rect = self.target.get().expect("start button not rendered");
        assert_eq!(self.state(), REQUIRED_EDGES[0].0);
        self.witness("start -> unlock prompt", rect, |h| {
            h.shell.unlock_prompt_visible()
        });
        self.record(0);
    }

    fn cancel_unlock(&mut self) {
        assert_eq!(self.state(), REQUIRED_EDGES[1].0);
        let cancel = self.text_target("Cancel");
        self.witness("unlock prompt -> cancel", cancel, |h| {
            !h.shell.unlock_prompt_visible()
        });
        self.record(1);
        assert_eq!(self.h.shell.access().session().subject.level, Level(0));
    }

    fn authenticate(&mut self) {
        assert_eq!(self.state(), REQUIRED_EDGES[2].0);
        for digit in [1, 2, 3, 4] {
            let rect = self
                .h
                .shell
                .prompt_digit_rect(digit)
                .expect("PIN digit not rendered");
            assert!(
                rect.intersect(self.h.screen_rect()).is_positive(),
                "PIN key outside viewport"
            );
            self.h.tap(rect.intersect(self.h.screen_rect()).center());
        }
        assert!(
            self.h.shell.workspace().find("protected").is_some(),
            "UI transition remained unreachable: correct PIN -> destination"
        );
        self.record(2);
        assert_eq!(self.h.shell.access().session().subject.level, Level(1));
    }
}

/// Replay generated geometry/input history, then discharge actual UI reachability obligations.
/// The environment supplies UI passes and eventually releases the pointer; a configured
/// occluder must first stand aside before an underlying control is required to work.
pub fn exercise(width: u16, height: u16, history: &[u8], modal_occluder: bool) {
    let mut scene = Scene::new(width, height);
    for operation in history.iter().take(16) {
        let target = scene.target.get().unwrap();
        match operation % 5 {
            0 => {
                scene.region.set(Some(target));
            }
            1 => {
                scene
                    .region
                    .set(Some(target.translate(egui::vec2(90.0, 60.0))));
            }
            2 => {
                scene.region.set(None);
            }
            3 => {
                scene.region.set(Some(target));
                scene.h.press(target.center());
                scene.h.release(target.center());
            }
            _ => {
                let (w, h) = if operation & 0x80 == 0 {
                    (height, width)
                } else {
                    (width, height)
                };
                scene.h.set_size(f32::from(w), f32::from(h));
            }
        }
        scene.h.frame();
        assert!(
            !scene.h.shell.unlock_prompt_visible(),
            "occluded input activated the underlying transition"
        );
    }
    scene.region.set(None);
    let before = scene.draws.get();
    scene.h.frame(); // A quiet pass, even if geometry changed.
    assert!(
        scene.draws.get() > before,
        "UI transition remained unreachable: ordinary UI did not resume after a quiet pass"
    );
    let open_panel = scene.text_target("Open panel");
    assert_eq!(scene.state(), REQUIRED_EDGES[4].0);
    scene.witness("start -> panel", open_panel, |h| {
        !h.shell.overlay().is_closed()
    });
    scene.record(4);
    if modal_occluder {
        scene.region.set(Some(scene.h.screen_rect()));
    }
    scene.h.frames(2);
    // The notification card's close target dismisses notifications, not the panel.
    // Use the documented scrim input to satisfy this navigation edge.
    let panel = scene
        .h
        .shell
        .overlay()
        .frame()
        .panel
        .expect("panel not rendered");
    let root = scene.h.screen_rect();
    let scrim = egui::Rect::from_min_max(egui::pos2(root.min.x, panel.max.y), root.max);
    assert!(
        scrim.is_positive(),
        "required panel-dismiss input has no visible scrim"
    );
    scene.witness("panel -> start", scrim, |h| h.shell.overlay().is_closed());
    scene.record(5);
    scene.region.set(None);
    scene.h.frames(2);
    scene.request_unlock();
    if modal_occluder {
        // A full-screen custom region is intentionally inactive behind the prompt.
        scene.region.set(Some(scene.h.screen_rect()));
        scene.h.frames(2);
    }
    scene.cancel_unlock();
    // The occluder intentionally blocks the underlying screen after modal dismissal.
    scene.region.set(None);
    scene.h.frames(2);
    if modal_occluder {
        // A real region gesture, not a host shortcut, opens the next gated prompt.
        // Its guard was already active before the prompt and must stand aside.
        scene.region.set(Some(scene.h.screen_rect()));
        scene.h.frames(2);
        scene.region_action.set(true);
        let glass = scene.h.screen_rect();
        scene.witness("region -> unlock prompt", glass, |h| {
            h.shell.unlock_prompt_visible()
        });
        scene.record(0);
    } else {
        scene.request_unlock();
    }
    scene.authenticate();
    scene.region.set(None);
    scene.h.frames(3);
    assert_eq!(scene.state(), REQUIRED_EDGES[3].0);
    let back = scene.text_target("Return to start");
    scene.witness("destination -> start", back, |h| {
        h.shell
            .workspace()
            .focused()
            .is_some_and(|instance| instance.decl_id() == "start")
    });
    scene.record(3);
    assert!(
        scene.witnessed.iter().all(|seen| *seen),
        "required UI transition lacks an input witness"
    );
}

#[cfg(test)]
pub mod regressions {
    #[test]
    fn actual_ui_edges_remain_reachable_after_input_obstacles() {
        for (width, height) in [(1024, 600), (800, 480), (480, 320), (600, 800)] {
            for history in [&[][..], &[0, 3, 1, 2][..], &[3, 3, 3, 1, 0][..]] {
                for modal in [false, true] {
                    super::exercise(width, height, history, modal);
                }
            }
        }
    }
}

#[cfg(test)]
mod generated {
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { cases: 32, failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn rendered_paths_survive_generated_histories(profile in 0usize..4, history in prop::collection::vec(any::<u8>(), 0..17), modal in any::<bool>()) {
            let (width, height) = [(1024, 600), (800, 480), (480, 320), (600, 800)][profile];
            super::exercise(width, height, &history, modal);
        }
    }
}
