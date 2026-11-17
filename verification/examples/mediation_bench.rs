//! CPU-only, fixed-work protected-dispatch and retained-egui layout benchmark.
use fairing::access::{
    AccessEvent, AuthOutcome, Permission, Protected, SessionController, SessionGrant, Subject,
};
use fairing::elements::{Element, ElementTree};
use fairing::testing::Harness;
use fairing::{egui, Cx, Gate, LaunchAction, ProtectedShell, Shell, ShellConfig, ShellEvent};
use std::{cell::Cell, hint::black_box, rc::Rc, time::Instant};
#[derive(Clone, Copy)]
struct Right(u8);
impl Permission for Right {
    fn gate(self) -> Gate {
        Gate::borrowed(["b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7"][usize::from(self.0 % 8)])
    }
}
fn host() -> Harness<ProtectedShell> {
    Harness::from_builder(|ctx| {
        Shell::builder(ShellConfig::default().configure(|c| {
            c.access.session_tokens = true;
            c.motion.reduce = true;
        }))
        .build_protected(ctx)
    })
    .unwrap()
}
fn login(h: &mut Harness<ProtectedShell>, action: LaunchAction, denied: bool) -> SessionController {
    h.shell.cancel_authentication();
    let _ = h.shell.poll_events();
    h.shell.launch(action);
    let request = h
        .shell
        .poll_events()
        .into_iter()
        .find_map(|e| match e {
            ShellEvent::Access(AccessEvent::SessionRequested { request, .. }) => Some(request),
            _ => None,
        })
        .unwrap();
    let now = h.now();
    let grants = (0..8)
        .map(|i| {
            if denied && i % 2 == 1 {
                SessionGrant::inactive_for(Right(i))
            } else {
                SessionGrant::base_for(Right(i))
            }
        })
        .collect();
    let (owner, token) = SessionController::issue(
        Subject::default(),
        grants,
        None,
        now,
        fairing::services::Waker::new(&h.ctx),
    )
    .unwrap();
    h.shell
        .accept_auth_response(request.respond(AuthOutcome::Session(token)));
    owner
}
struct Leaf {
    right: Right,
    measure: Rc<Cell<u64>>,
    paint: Rc<Cell<u64>>,
}
impl fairing::access::PermissionBound for Leaf {
    type Required = Right;
    fn required_permission(&self) -> Right {
        self.right
    }
}
impl Element for Leaf {
    fn measure(&mut self, _: egui::Vec2, _: &mut Cx<'_>) -> egui::Vec2 {
        self.measure.set(self.measure.get() + 1);
        egui::vec2(16., 16.)
    }
    fn paint(&mut self, ui: &mut egui::Ui, _: &mut Cx<'_>) {
        self.paint.set(self.paint.get() + 1);
        ui.painter()
            .rect_filled(ui.max_rect(), 0., egui::Color32::WHITE);
    }
}
fn report(
    name: &str,
    iterations: u64,
    mut task: impl FnMut(),
    measure: &Cell<u64>,
    paint: &Cell<u64>,
) {
    let before = (measure.get(), paint.get());
    let mut ns = Vec::new();
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..iterations {
            task();
        }
        ns.push(start.elapsed().as_nanos() as f64 / iterations as f64);
    }
    ns.sort_by(f64::total_cmp);
    println!("{{\"case\":\"{name}\",\"iterations_per_sample\":{iterations},\"samples\":7,\"median_ns\":{},\"min_ns\":{},\"max_ns\":{},\"measure_calls\":{},\"paint_calls\":{}}}",ns[3],ns[0],ns[6],measure.get()-before.0,paint.get()-before.1);
}
fn main() {
    let mut h = host();
    let calls = Rc::new(Cell::new(0));
    let sink = Rc::clone(&calls);
    h.shell.add(Protected::new(
        Right(0),
        fairing::action("action", move |_| sink.set(sink.get() + 1)),
    ));
    let _owner = login(&mut h, LaunchAction::run("action"), false);
    let empty = Cell::new(0);
    report(
        "action",
        20_000,
        || h.shell.launch(black_box(LaunchAction::run("action"))),
        &calls,
        &empty,
    );
    for (count, depth, denied) in [
        (64, 1, false),
        (256, 1, false),
        (64, 8, false),
        (64, 32, false),
        (256, 8, true),
    ] {
        let mut h = host();
        let m = Rc::new(Cell::new(0));
        let p = Rc::new(Cell::new(0));
        let mut tree = ElementTree::row(
            Right(0),
            0.,
            (0..count)
                .map(|i| {
                    ElementTree::leaf(Leaf {
                        right: Right((i % 8) as u8),
                        measure: Rc::clone(&m),
                        paint: Rc::clone(&p),
                    })
                })
                .collect(),
        )
        .unwrap();
        for _ in 1..depth {
            tree = ElementTree::column(Right(0), 0., vec![tree]).unwrap();
        }
        h.shell.add_elements("tree", tree);
        let _owner = login(&mut h, LaunchAction::open("tree"), denied);
        h.frames(12);
        report(
            &format!("layout_n{count}_d{depth}_denied{denied}"),
            80,
            || h.frame(),
            &m,
            &p,
        );
    }
}
