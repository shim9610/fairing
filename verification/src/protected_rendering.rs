//! Public protected-host histories observe actual callback entries and painted output.
use fairing::access::{
    AccessEvent, AuthOutcome, Permission, PermissionBound, SessionController, SessionGrant, Subject,
};
use fairing::elements::{Element, ElementTree};
use fairing::testing::Harness;
use fairing::{egui, Cx, Gate, LaunchAction, ProtectedShell, Shell, ShellConfig, ShellEvent};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Right {
    Read,
    Write,
}
impl Permission for Right {
    fn gate(self) -> Gate {
        Gate::borrowed(match self {
            Self::Read => "render.read",
            Self::Write => "render.write",
        })
    }
}
fn host() -> Harness<ProtectedShell> {
    Harness::from_builder(|ctx| {
        let config = ShellConfig::default().configure(|c| {
            c.access.session_tokens = true;
            c.motion.reduce = true;
        });
        Shell::builder(config).build_protected(ctx)
    })
    .unwrap()
}
fn login(h: &mut Harness<ProtectedShell>, id: &str, expires: bool) -> SessionController {
    h.shell.cancel_authentication();
    let _ = h.shell.poll_events();
    h.shell.launch(LaunchAction::open(id));
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
    let (owner, token) = SessionController::issue(
        Subject::default(),
        vec![
            SessionGrant::base_for(Right::Read),
            SessionGrant::inactive_for(Right::Write),
        ],
        expires.then(|| now + Duration::from_secs(1)),
        now,
        fairing::services::Waker::new(&h.ctx),
    )
    .unwrap();
    h.shell
        .accept_auth_response(request.respond(AuthOutcome::Session(token)));
    owner
}
fn has_text(h: &mut Harness<ProtectedShell>, marker: &str) -> bool {
    h.frame_shapes().iter().any(|shape| {
        matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.text().contains(marker))
    })
}
struct Leaf {
    right: Right,
    marker: &'static str,
    entries: Rc<Cell<usize>>,
    revoke_on_measure: Option<Rc<RefCell<Option<SessionController>>>>,
}
impl PermissionBound for Leaf {
    type Required = Right;
    fn required_permission(&self) -> Right {
        self.right
    }
}
impl Element for Leaf {
    fn measure(&mut self, _: egui::Vec2, _: &mut Cx<'_>) -> egui::Vec2 {
        self.entries.set(self.entries.get() + 1);
        if let Some(owner) = &self.revoke_on_measure {
            if let Some(owner) = owner.borrow_mut().as_mut() {
                owner.revoke();
            }
        }
        egui::vec2(120.0, 32.0)
    }
    fn paint(&mut self, ui: &mut egui::Ui, _: &mut Cx<'_>) {
        self.entries.set(self.entries.get() + 1);
        ui.label(self.marker);
    }
}
fn leaf(right: Right, marker: &'static str, entries: &Rc<Cell<usize>>) -> ElementTree<Right> {
    ElementTree::leaf(Leaf {
        right,
        marker,
        entries: Rc::clone(entries),
        revoke_on_measure: None,
    })
}

#[test]
fn nested_public_tree_reflows_grants_and_removes_revoked_output() {
    let mut h = host();
    let read = Rc::new(Cell::new(0));
    let write = Rc::new(Cell::new(0));
    h.shell.add_elements(
        "tree",
        ElementTree::column(
            Right::Read,
            8.0,
            vec![ElementTree::row(
                Right::Read,
                12.0,
                vec![
                    leaf(Right::Read, "READ_MARKER", &read),
                    leaf(Right::Write, "WRITE_MARKER", &write),
                ],
            )
            .unwrap()],
        )
        .unwrap(),
    );
    h.frame();
    assert_eq!((read.get(), write.get()), (0, 0));
    let mut owner = login(&mut h, "tree", false);
    h.frames(3);
    assert!(has_text(&mut h, "READ_MARKER"));
    assert!(!has_text(&mut h, "WRITE_MARKER"));
    assert!(read.get() > 0);
    assert_eq!(write.get(), 0, "denied leaf entered measure or paint");
    owner
        .replace_grants(
            vec![
                SessionGrant::base_for(Right::Read),
                SessionGrant::base_for(Right::Write),
            ],
            None,
            h.now(),
        )
        .unwrap();
    h.frames(2);
    assert!(has_text(&mut h, "WRITE_MARKER"));
    assert!(write.get() > 0);
    owner
        .replace_grants(vec![SessionGrant::base_for(Right::Read)], None, h.now())
        .unwrap();
    let before = write.get();
    h.frames(2);
    assert!(!has_text(&mut h, "WRITE_MARKER"));
    assert_eq!(write.get(), before);
    owner.revoke();
    let before = read.get();
    h.frames(2);
    assert!(!has_text(&mut h, "READ_MARKER"));
    assert_eq!(read.get(), before);
    assert!(h.shell.observe().session_token().is_none());
}

struct DeclaredScreen {
    right: Rc<Cell<Right>>,
    entries: Rc<Cell<usize>>,
    hooks: Rc<Cell<u8>>,
}
impl PermissionBound for DeclaredScreen {
    type Required = Right;
    fn required_permission(&self) -> Right {
        self.right.get()
    }
}
impl fairing::Screen for DeclaredScreen {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut Cx<'_>) {
        self.entries.set(self.entries.get() + 1);
        ui.label("DECLARED_MARKER");
    }
    fn on_back(&mut self, _: &mut Cx<'_>) -> fairing::screen::BackAction {
        self.hooks.set(self.hooks.get() | 1);
        fairing::screen::BackAction::Consumed
    }
    fn on_lifecycle(&mut self, _: fairing::screen::Lifecycle, _: &mut Cx<'_>) {
        self.hooks.set(self.hooks.get() | 2);
    }
    fn on_result(&mut self, _: &str, value: fairing::screen::ScreenValue, _: &mut Cx<'_>) {
        assert_eq!(value, fairing::screen::ScreenValue::Int(42));
        self.hooks.set(self.hooks.get() | 4);
    }
}
#[test]
fn declared_public_screen_rejects_metadata_drift_and_session_expiry() {
    let mut h = host();
    let right = Rc::new(Cell::new(Right::Read));
    let entries = Rc::new(Cell::new(0));
    let hooks = Rc::new(Cell::new(0));
    h.shell.add_screen(
        "declared",
        DeclaredScreen {
            right: Rc::clone(&right),
            entries: Rc::clone(&entries),
            hooks: Rc::clone(&hooks),
        },
    );
    let _owner = login(&mut h, "declared", true);
    h.frames(2);
    assert!(has_text(&mut h, "DECLARED_MARKER"));
    h.shell.handle().back();
    h.frame();
    h.shell.add(fairing::access::Protected::new(
        Right::Read,
        fairing::screen("child", |_: &mut egui::Ui, cx: &mut Cx<'_>| {
            cx.finish_with(fairing::screen::ScreenValue::Int(42));
        }),
    ));
    h.shell.launch(LaunchAction::open("child"));
    h.frames(3);
    assert_eq!(
        hooks.get(),
        7,
        "declared UI lifecycle, Back and result paths were not exercised"
    );
    right.set(Right::Write);
    let before = entries.get();
    h.frames(2);
    assert!(!has_text(&mut h, "DECLARED_MARKER"));
    assert_eq!(entries.get(), before);
    right.set(Right::Read);
    assert!(has_text(&mut h, "DECLARED_MARKER"));
    h.sleep(2.0);
    let before = entries.get();
    h.frames(2);
    assert!(!has_text(&mut h, "DECLARED_MARKER"));
    assert_eq!(entries.get(), before);
    assert!(h.shell.observe().session_token().is_none());
}

#[test]
fn revocation_during_public_tree_measurement_blocks_sibling_and_paint() {
    let mut h = host();
    let owner = Rc::new(RefCell::new(None));
    let first = Rc::new(Cell::new(0));
    let sibling = Rc::new(Cell::new(0));
    h.shell.add_elements(
        "tree",
        ElementTree::row(
            Right::Read,
            8.0,
            vec![
                ElementTree::leaf(Leaf {
                    right: Right::Read,
                    marker: "REVOKING_MARKER",
                    entries: Rc::clone(&first),
                    revoke_on_measure: Some(Rc::clone(&owner)),
                }),
                leaf(Right::Read, "SIBLING_MARKER", &sibling),
            ],
        )
        .unwrap(),
    );
    *owner.borrow_mut() = Some(login(&mut h, "tree", false));
    h.frames(2);
    assert!(first.get() > 0);
    assert_eq!(
        sibling.get(),
        0,
        "sibling entered after measurement revoked authority"
    );
    assert!(!has_text(&mut h, "REVOKING_MARKER"));
    assert!(!has_text(&mut h, "SIBLING_MARKER"));
}

#[test]
fn public_factory_is_lazy_and_rechecks_authority_before_screen_hooks() {
    for revoke_in_factory in [false, true] {
        let mut h = host();
        let owner = Rc::new(RefCell::new(None::<SessionController>));
        let constructors = Rc::new(Cell::new(0));
        let entries = Rc::new(Cell::new(0));
        let hooks = Rc::new(Cell::new(0));
        h.shell.add(fairing::access::Protected::new(
            Right::Read,
            fairing::screen("landing", |_: &mut egui::Ui, _: &mut Cx<'_>| {}),
        ));
        let (issuer, built, painted, called) = (
            Rc::clone(&owner),
            Rc::clone(&constructors),
            Rc::clone(&entries),
            Rc::clone(&hooks),
        );
        h.shell.add(fairing::access::Protected::new(
            Right::Read,
            fairing::screen_with("factory", move || {
                built.set(built.get() + 1);
                if revoke_in_factory {
                    issuer.borrow_mut().as_mut().unwrap().revoke();
                }
                DeclaredScreen {
                    right: Rc::new(Cell::new(Right::Read)),
                    entries: Rc::clone(&painted),
                    hooks: Rc::clone(&called),
                }
            }),
        ));
        h.frame();
        assert_eq!(constructors.get(), 0, "registration eagerly ran a factory");
        *owner.borrow_mut() = Some(login(&mut h, "landing", false));
        assert_eq!(constructors.get(), 0, "another launch ran a factory");
        h.shell.launch(LaunchAction::open("factory"));
        h.frames(2);
        assert_eq!(constructors.get(), 1);
        if revoke_in_factory {
            assert_eq!(
                hooks.get(),
                0,
                "factory revocation admitted lifecycle hooks"
            );
            assert_eq!(entries.get(), 0, "factory revocation admitted drawing");
            assert!(!has_text(&mut h, "DECLARED_MARKER"));
        } else {
            assert_eq!(hooks.get(), 2);
            assert!(entries.get() > 0);
            assert!(has_text(&mut h, "DECLARED_MARKER"));
            owner.borrow_mut().as_mut().unwrap().revoke();
            let before = entries.get();
            h.frames(2);
            assert_eq!(entries.get(), before);
            assert!(!has_text(&mut h, "DECLARED_MARKER"));
        }
        assert_eq!(
            constructors.get(),
            1,
            "cached screen repeated its constructor"
        );
    }
}
