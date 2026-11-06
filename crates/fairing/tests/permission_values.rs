//! External implementations use their enum values; Fairing dispatches their UI.
#![allow(
    clippy::unwrap_used,
    reason = "Integration failures retain the violated public contract"
)]
use fairing::access::{
    AccessEvent, AuthOutcome, Permission, PermissionBound, Protected, SessionController,
    SessionGrant, Subject,
};
use fairing::elements::{Element, ElementTree};
use fairing::{egui, Cx, Gate, LaunchAction, ProtectedShell, Shell, ShellConfig, ShellEvent};
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppPermission {
    View,
    Edit,
}
impl Permission for AppPermission {
    fn gate(self) -> Gate {
        Gate::borrowed(match self {
            Self::View => "customer.results",
            Self::Edit => "customer.calibrate",
        })
    }
}

fn config() -> ShellConfig {
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    config.motion.reduce = true;
    config
}
fn authenticate(shell: &mut ProtectedShell, ctx: &egui::Context) -> SessionController {
    shell.add(Protected::new(
        AppPermission::View,
        fairing::action("login-primer", |_| {}),
    ));
    shell.launch(LaunchAction::run("login-primer"));
    let request = shell
        .poll_events()
        .into_iter()
        .find_map(|event| match event {
            ShellEvent::Access(AccessEvent::SessionRequested { request, .. }) => Some(request),
            _ => None,
        })
        .unwrap();
    let (owner, token) = SessionController::issue(
        Subject::default(),
        vec![
            SessionGrant::base_for(AppPermission::View),
            SessionGrant::inactive_for(AppPermission::Edit),
        ],
        None,
        shell.shell().now(),
        fairing::services::Waker::new(ctx),
    )
    .unwrap();
    shell.accept_auth_response(request.respond(AuthOutcome::Session(token)));
    owner
}
fn frame(shell: &mut ProtectedShell, ctx: &egui::Context) {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| shell.frame(ui),
    )
    .drop_without_applying_deltas();
}

struct CustomerLeaf {
    permission: AppPermission,
    calls: Rc<Cell<usize>>,
    measurements: Rc<Cell<usize>>,
    rect: Rc<Cell<Option<egui::Rect>>>,
}
impl PermissionBound for CustomerLeaf {
    type Required = AppPermission;
    fn required_permission(&self) -> AppPermission {
        self.permission
    }
}
impl Element for CustomerLeaf {
    fn measure(&mut self, _: egui::Vec2, _: &mut Cx<'_>) -> egui::Vec2 {
        self.measurements.set(self.measurements.get() + 1);
        egui::vec2(40.0, 25.0)
    }
    fn paint(&mut self, ui: &mut egui::Ui, _: &mut Cx<'_>) {
        self.calls.set(self.calls.get() + 1);
        self.rect.set(Some(ui.max_rect()));
        ui.painter()
            .rect_filled(ui.max_rect(), 0.0, egui::Color32::RED);
    }
}

#[test]
fn customer_enum_and_trait_elements_are_dispatched_without_manual_child_guards(
) -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut shell = Shell::builder(config()).build_protected(&ctx)?;
    let mut owner = authenticate(&mut shell, &ctx);
    let calls = [Rc::new(Cell::new(0usize)), Rc::new(Cell::new(0usize))];
    let measures = [Rc::new(Cell::new(0usize)), Rc::new(Cell::new(0usize))];
    let rects = [Rc::new(Cell::new(None)), Rc::new(Cell::new(None))];
    let children = [AppPermission::Edit, AppPermission::View]
        .into_iter()
        .enumerate()
        .map(|(index, permission)| {
            ElementTree::leaf(CustomerLeaf {
                permission,
                calls: Rc::clone(calls.get(index).unwrap()),
                measurements: Rc::clone(measures.get(index).unwrap()),
                rect: Rc::clone(rects.get(index).unwrap()),
            })
        })
        .collect();
    shell.add_elements(
        "results",
        ElementTree::row(AppPermission::View, 7.0, children)?,
    );
    shell.launch(LaunchAction::open("results"));
    for _ in 0..4 {
        frame(&mut shell, &ctx);
    }
    assert!(
        calls[1].get() > 0,
        "library never dispatched the permitted external element"
    );
    assert_eq!((calls[0].get(), measures[0].get()), (0, 0));
    assert!(rects[0].get().is_none());
    let compact = rects[1].get().unwrap();
    let token = shell.shell().access().session_token().unwrap();
    let snapshot = token.snapshot(shell.shell().now()).unwrap();
    assert!(snapshot.allows_permission(AppPermission::View));
    assert!(
        !snapshot.allows_permission(AppPermission::Edit),
        "enum variants collapsed to one type-wide permission"
    );
    owner
        .replace_grants(
            vec![
                SessionGrant::base_for(AppPermission::View),
                SessionGrant::base_for(AppPermission::Edit),
            ],
            None,
            shell.shell().now(),
        )
        .unwrap();
    frame(&mut shell, &ctx);
    assert!(calls[0].get() > 0);
    assert!(rects[1].get().unwrap().min.x > compact.min.x);
    owner.revoke();
    let before = (calls[0].get(), calls[1].get());
    frame(&mut shell, &ctx);
    assert_eq!((calls[0].get(), calls[1].get()), before);
    Ok(())
}

#[test]
fn denied_custom_nav_values_do_not_reserve_cells_before_layout() -> fairing::Result<()> {
    let ctx = egui::Context::default();
    let mut config = config();
    config.nav_bar.items = vec!["home".into(), "private".into()];
    let mut shell = Shell::builder(config.clone()).build_protected(&ctx)?;
    let calls = Rc::new(Cell::new(0));
    let painted = Rc::clone(&calls);
    shell.add(Protected::new(
        AppPermission::Edit,
        fairing::nav_item("private", move |_, _| painted.set(painted.get() + 1)),
    ));
    let mut owner = authenticate(&mut shell, &ctx);
    frame(&mut shell, &ctx);
    let compact = shell
        .shell()
        .nav_bar()
        .item_rect(&fairing::chrome::NavItem::Home)
        .unwrap();
    assert_eq!(calls.get(), 0);
    config.nav_bar.items = vec!["home".into()];
    let baseline_ctx = egui::Context::default();
    let mut baseline = Shell::builder(config).build_protected(&baseline_ctx)?;
    frame(&mut baseline, &baseline_ctx);
    assert_eq!(
        Some(compact),
        baseline
            .shell()
            .nav_bar()
            .item_rect(&fairing::chrome::NavItem::Home),
        "denied custom navigation reserved a cell"
    );
    owner
        .replace_grants(
            vec![
                SessionGrant::base_for(AppPermission::View),
                SessionGrant::base_for(AppPermission::Edit),
            ],
            None,
            shell.shell().now(),
        )
        .unwrap();
    frame(&mut shell, &ctx);
    assert!(calls.get() > 0);
    assert_ne!(
        Some(compact),
        shell
            .shell()
            .nav_bar()
            .item_rect(&fairing::chrome::NavItem::Home)
    );
    owner
        .replace_grants(
            vec![SessionGrant::base_for(AppPermission::View)],
            None,
            shell.shell().now(),
        )
        .unwrap();
    let count = calls.get();
    frame(&mut shell, &ctx);
    assert_eq!(calls.get(), count);
    assert_eq!(
        Some(compact),
        shell
            .shell()
            .nav_bar()
            .item_rect(&fairing::chrome::NavItem::Home)
    );
    assert!(shell
        .shell()
        .nav_bar()
        .item_rect(&fairing::chrome::NavItem::Custom("private".into()))
        .is_none());
    Ok(())
}

struct CustomerScreen {
    requirement: Rc<Cell<AppPermission>>,
    calls: Rc<Cell<usize>>,
}
impl PermissionBound for CustomerScreen {
    type Required = AppPermission;
    fn required_permission(&self) -> AppPermission {
        self.requirement.get()
    }
}
impl fairing::Screen for CustomerScreen {
    fn ui(&mut self, _: &mut egui::Ui, _: &mut Cx<'_>) {
        self.calls.set(self.calls.get() + 1);
    }
}

#[test]
fn external_screens_need_no_manual_guard_and_requirement_drift_fails_closed() -> fairing::Result<()>
{
    let ctx = egui::Context::default();
    let mut shell = Shell::builder(config()).build_protected(&ctx)?;
    let _owner = authenticate(&mut shell, &ctx);
    let requirement = Rc::new(Cell::new(AppPermission::View));
    let calls = Rc::new(Cell::new(0));
    shell.add_screen(
        "customer",
        CustomerScreen {
            requirement: Rc::clone(&requirement),
            calls: Rc::clone(&calls),
        },
    );
    shell.launch(LaunchAction::open("customer"));
    for _ in 0..4 {
        frame(&mut shell, &ctx);
    }
    assert!(calls.get() > 0);
    requirement.set(AppPermission::Edit);
    let before = calls.get();
    frame(&mut shell, &ctx);
    assert_eq!(
        calls.get(),
        before,
        "changed screen declaration retained an earlier permission"
    );
    Ok(())
}
