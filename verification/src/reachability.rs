//! UI transition witnesses: logical permission alone is not an executable input path.
//! Setup may use host APIs; each claimed transition below must occur through real pointer input.
use fairing::access::Gate;
use fairing::gesture::{GestureRegion, Phase, RegionCx, RegionPlaceCx, RegionTouch};
use fairing::testing::{access_config, Harness};
use fairing::{egui, screen, Cx, LaunchAction, Level};
use std::cell::{Cell, RefCell};
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
    AuthorizedStart,
    Lock,
    Panel,
    Other,
}

// Explicit obligations distinguish required edges from intentionally blocked input.
const REQUIRED_EDGES: [(State, State); 12] = [
    (State::Start, State::Prompt),
    (State::Prompt, State::Start),
    (State::Prompt, State::Protected),
    (State::Protected, State::AuthorizedStart),
    (State::Start, State::Panel),
    (State::Panel, State::Start),
    (State::Start, State::Lock),
    (State::Lock, State::Start),
    (State::Lock, State::AuthorizedStart),
    (State::AuthorizedStart, State::Protected),
    (State::AuthorizedStart, State::Start),
    (State::AuthorizedStart, State::Lock),
];

#[derive(Clone, Copy, Debug)]
struct InputProfile {
    style: u8,
    pixels_per_point: f32,
    shuffle: bool,
}
impl Default for InputProfile {
    fn default() -> Self {
        Self {
            style: 0,
            pixels_per_point: 1.0,
            shuffle: false,
        }
    }
}

struct Scene {
    h: Harness,
    target: Rc<Cell<Option<egui::Rect>>>,
    draws: Rc<Cell<usize>>,
    region: Rc<Cell<Option<egui::Rect>>>,
    witnessed: Vec<bool>,
    region_action: Rc<Cell<bool>>,
    settle_frames: usize,
    input: InputProfile,
    activations: Rc<RefCell<[usize; 5]>>,
}
impl Scene {
    fn new(width: u16, height: u16, animated: bool, input: InputProfile) -> Self {
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
        config.motion.reduce = !animated;
        config.access.lock_screen.allow_continue = true;
        config.access.pin_table.shuffle = input.shuffle;
        config.access.pin_table.attempt_limit = Some(2);
        config.access.pin_table.lock_secs = Some(30);
        let target = Rc::new(Cell::new(None));
        let rect = target.clone();
        let draws = Rc::new(Cell::new(0));
        let count = draws.clone();
        let activations = Rc::new(RefCell::new([0; 5]));
        let callbacks = activations.clone();
        let returns = activations.clone();
        let mut h = Harness::new(config, fairing::services::Services::null()).unwrap();
        h.ctx.set_pixels_per_point(input.pixels_per_point);
        {
            let shell = &mut h.shell;
            shell.add(
                screen("start", move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                    count.set(count.get() + 1);
                    ui.horizontal_wrapped(|ui| {
                        let response = ui.button("Protected action");
                        rect.set(Some(response.rect));
                        if response.clicked() {
                            callbacks.borrow_mut()[0] += 1;
                            cx.launch(LaunchAction::open("protected"));
                        }
                        if ui.button("Open panel").clicked() {
                            callbacks.borrow_mut()[1] += 1;
                            cx.shell.toggle_overlay();
                        }
                        if ui.button("Lock panel").clicked() {
                            callbacks.borrow_mut()[2] += 1;
                            cx.launch(LaunchAction::Lock);
                        }
                        if ui.button("Log out").clicked() {
                            callbacks.borrow_mut()[3] += 1;
                            cx.launch(LaunchAction::Logout);
                        }
                    });
                })
                .gate("public"),
            );
            shell.add(screen(
                "protected",
                move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                    if ui.button("Return to start").clicked() {
                        returns.borrow_mut()[4] += 1;
                        cx.launch(LaunchAction::open("start"));
                    }
                },
            ));
        }
        h.set_size(f32::from(width), f32::from(height));
        h.shell.launch(LaunchAction::open("start")); // Setup, not a transition witness.
        h.frames(if animated { 32 } else { 4 });
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
            settle_frames: if animated { 24 } else { 2 },
            input,
            activations,
        }
    }

    fn state(&self) -> State {
        if self.h.shell.lock_screen_visible() {
            return State::Lock;
        }
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
            Some("start") if self.h.shell.access().session().subject.level > Level(0) => {
                State::AuthorizedStart
            }
            Some("start") => State::Start,
            Some("protected") => State::Protected,
            _ => State::Other,
        }
    }

    fn record(&mut self, edge: usize) {
        assert!(
            !self.h.shell.is_faulted(),
            "fault quarantine cannot satisfy a UI edge: {edge}"
        );
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
        let before: usize = self.activations.borrow().iter().sum();
        for _ in 0..3 {
            self.tap(visible.center());
            self.h.frames(self.settle_frames);
            assert!(
                !self.h.shell.is_faulted(),
                "fault quarantine cannot satisfy an input witness: {edge}"
            );
            let after: usize = self.activations.borrow().iter().sum();
            assert!(
                after <= before + 1,
                "one input witness executed duplicate or underlying callbacks: {edge}"
            );
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
            let mut candidate = None;
            find(&clipped.shape, label, &mut candidate);
            if let Some(rect) = candidate {
                let visible = rect
                    .intersect(clipped.clip_rect)
                    .intersect(self.h.screen_rect());
                if visible.is_positive() && visible.is_finite() {
                    found = Some(visible);
                }
            }
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

    fn tap(&mut self, point: egui::Pos2) {
        match self.input.style {
            0 => self.h.tap(point),
            1 => {
                self.h.press(point);
                self.h.release(point);
                self.h.frame();
                self.h.frame();
            }
            _ => {
                self.h.press(point);
                self.h.frames(3);
                self.h.release(point);
                self.h.frame();
                self.h.frame();
            }
        }
    }

    fn perturb(&mut self, guard: bool) {
        let before = self.state();
        let subject = self.h.shell.access().session().subject.level;
        let viewport = self.h.screen_rect();
        if guard {
            self.region.set(Some(viewport));
        }
        self.h.set_size(viewport.height(), viewport.width());
        self.h.frames(2);
        if guard {
            self.region.set(Some(self.h.screen_rect()));
        }
        self.h.frames(self.settle_frames);
        assert_eq!(
            self.state(),
            before,
            "geometry alone changed navigation state"
        );
        assert_eq!(self.h.shell.access().session().subject.level, subject);
        if !matches!(before, State::Prompt | State::Lock | State::Panel) {
            self.region.set(None);
            self.h.frames(2);
        }
    }

    fn enter_pin(&mut self, digits: [u8; 4]) {
        for (index, digit) in digits.into_iter().enumerate() {
            let rect = self
                .h
                .shell
                .prompt_digit_rect(digit)
                .expect("PIN digit not rendered");
            let visible = rect.intersect(self.h.screen_rect());
            assert!(
                visible.is_positive() && visible.is_finite(),
                "PIN key outside viewport"
            );
            self.tap(visible.center());
            if index == 1 {
                self.perturb(true);
            }
        }
    }

    fn authenticate(&mut self) {
        assert_eq!(self.state(), REQUIRED_EDGES[2].0);
        self.enter_pin([1, 2, 3, 4]);
        assert!(
            self.h.shell.workspace().find("protected").is_some(),
            "UI transition remained unreachable: correct PIN -> destination"
        );
        self.record(2);
        assert_eq!(self.h.shell.access().session().subject.level, Level(1));
    }

    fn return_to_start(&mut self) {
        self.perturb(false);
        let back = self.text_target("Return to start");
        assert_eq!(self.state(), State::Protected);
        self.witness("destination -> authorized start", back, |h| {
            h.shell
                .workspace()
                .focused()
                .is_some_and(|i| i.decl_id() == "start")
        });
        self.record(3);
    }

    fn lock(&mut self, edge: usize) {
        assert_eq!(self.state(), REQUIRED_EDGES[edge].0);
        self.perturb(false);
        let button = self.text_target("Lock panel");
        self.witness("start -> lock", button, |h| h.shell.lock_screen_visible());
        self.record(edge);
        assert_eq!(self.h.shell.access().session().subject.level, Level(0));
        self.perturb(true);
    }

    fn continue_lock(&mut self) {
        let button = self.text_target("Continue");
        self.witness("lock -> continue as viewer", button, |h| {
            !h.shell.lock_screen_visible()
        });
        self.record(7);
        assert_eq!(self.h.shell.access().session().subject.level, Level(0));
        self.region.set(None);
        self.h.frames(self.settle_frames);
    }
}

/// Replay generated geometry/input history, then discharge actual UI reachability obligations.
/// The environment supplies UI passes and eventually releases the pointer; a configured
/// occluder must first stand aside before an underlying control is required to work.
pub fn exercise(width: u16, height: u16, history: &[u8], modal_occluder: bool, animated: bool) {
    exercise_with(
        width,
        height,
        history,
        modal_occluder,
        animated,
        InputProfile::default(),
    );
}

fn exercise_with(
    width: u16,
    height: u16,
    history: &[u8],
    modal_occluder: bool,
    animated: bool,
    input: InputProfile,
) {
    let mut scene = Scene::new(width, height, animated, input);
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
    scene.perturb(false);
    let open_panel = scene.text_target("Open panel");
    assert_eq!(scene.state(), REQUIRED_EDGES[4].0);
    scene.witness("start -> panel", open_panel, |h| {
        !h.shell.overlay().is_closed()
    });
    scene.record(4);
    scene.perturb(modal_occluder);
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
        "required panel-dismiss input has no visible scrim: panel={panel:?}, viewport={root:?}, input={:?}, animated={animated}", scene.input
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
    scene.perturb(modal_occluder);
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
    scene.perturb(modal_occluder);
    scene.authenticate();
    scene.region.set(None);
    scene.h.frames(scene.settle_frames);
    scene.return_to_start();
    scene.perturb(false);
    let open = scene.text_target("Protected action");
    scene.witness(
        "authorized start -> protected without prompting",
        open,
        |h| {
            h.shell
                .workspace()
                .focused()
                .is_some_and(|i| i.decl_id() == "protected")
                && !h.shell.unlock_prompt_visible()
        },
    );
    scene.record(9);
    scene.return_to_start();
    let logout = scene.text_target("Log out");
    scene.witness("authorized start -> logout", logout, |h| {
        h.shell.access().session().subject.level == Level(0)
    });
    scene.record(10);
    scene.lock(6);
    scene.continue_lock();
    scene.lock(6);
    scene.enter_pin([1, 2, 3, 4]);
    scene.record(8);
    assert_eq!(scene.h.shell.access().session().subject.level, Level(1));
    scene.region.set(None);
    scene.h.frames(scene.settle_frames);
    scene.lock(11);
    scene.continue_lock();
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
                    for animated in [false, true] {
                        for style in 0..2 {
                            for pixels_per_point in [1.0, 1.5] {
                                super::exercise_with(
                                    width,
                                    height,
                                    history,
                                    modal,
                                    animated,
                                    super::InputProfile {
                                        style,
                                        pixels_per_point,
                                        shuffle: false,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod generated {
    use proptest::prelude::*;
    proptest! {
        #![proptest_config(ProptestConfig { cases: 96, failure_persistence: None, ..ProptestConfig::default() })]
        #[test]
        fn rendered_paths_survive_generated_histories(profile in 0usize..4, history in prop::collection::vec(any::<u8>(), 0..17), modal in any::<bool>(), animated in any::<bool>(), style in 0u8..3, scale in 0u8..3, shuffle in any::<bool>()) {
            let (width, height) = [(1024, 600), (800, 480), (480, 320), (600, 800)][profile];
            super::exercise_with(width, height, &history, modal, animated, super::InputProfile { style, pixels_per_point: [1.0,1.5,2.0][usize::from(scale)], shuffle });
        }
    }
}

#[cfg(test)]
mod refusal_recovery {
    use super::*;
    use fairing::access::AccessEvent;
    use fairing::ShellEvent;

    #[test]
    fn refusal_lockout_cancel_reopen_and_retry_keep_their_input_paths() {
        for (width, height) in [(1024, 600), (800, 480), (480, 320), (600, 800)] {
            for animated in [false, true] {
                for style in 0..3 {
                    let mut scene = Scene::new(
                        width,
                        height,
                        animated,
                        InputProfile {
                            style,
                            pixels_per_point: 1.5,
                            shuffle: true,
                        },
                    );
                    scene.request_unlock();
                    let _ = scene.h.shell.poll_events();
                    scene.enter_pin([9, 9, 9, 9]);
                    assert_eq!(scene.state(), State::Prompt);
                    assert_eq!(scene.h.shell.access().session().subject.level, Level(0));
                    assert!(scene.h.shell.poll_events().iter().any(|event| matches!(
                        event,
                        ShellEvent::Access(AccessEvent::Denied { .. })
                    )));
                    // Refusal intentionally disables keys during its 320 ms shake.
                    scene.h.sleep(0.4);
                    scene.h.frames(2);
                    scene.enter_pin([0, 0, 0, 0]);
                    let second_events = scene.h.shell.poll_events();
                    let until = second_events.iter().cloned()
                        .find_map(|event| match event {
                            ShellEvent::Access(AccessEvent::Locked { until, .. }) => Some(until),
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("second refusal did not start lockout: viewport={width}x{height}, animated={animated}, style={style}, events={second_events:?}, texts={:?}", scene.h.texts()));
                    scene.enter_pin([1, 2, 3, 4]);
                    assert_eq!(
                        scene.state(),
                        State::Prompt,
                        "locked keypad accepted credentials"
                    );
                    assert_eq!(scene.h.shell.access().session().subject.level, Level(0));
                    assert!(!scene.h.shell.poll_events().iter().any(|event| matches!(
                        event,
                        ShellEvent::Access(AccessEvent::Unlocked { .. })
                    )));
                    scene.cancel_unlock();
                    scene.region.set(None);
                    scene.h.frames(scene.settle_frames);
                    scene.request_unlock();
                    scene.enter_pin([1, 2, 3, 4]);
                    assert_eq!(scene.state(), State::Prompt, "reopening bypassed lockout");
                    assert_eq!(scene.h.shell.access().session().subject.level, Level(0));
                    // Frame time uses f32 internally; cross the recorded deadline by 50 ms.
                    scene
                        .h
                        .sleep(until.saturating_duration_since(scene.h.now()).as_secs_f64() + 0.05);
                    scene.h.frames(2);
                    scene.authenticate();
                    scene.region.set(None);
                    scene.h.frames(scene.settle_frames);
                    scene.return_to_start();
                }
            }
        }
    }
}
