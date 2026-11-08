#![allow(
    clippy::panic,
    reason = "Security regressions inject callback unwinds and forbid denied invocation"
)]
use super::*;
use crate::access::{Access, SessionController, SessionGrant, Subject};
use crate::config::AccessConfig;
use crate::screen::cx::fixture::{pane, Fixture};
use crate::screen::{BackAction, Lifecycle, Screen, ScreenValue};
use crate::services::Waker;
use crate::workspace::InstanceId;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy)]
enum AppPermission {
    View,
    Edit,
}
use AppPermission::{Edit, View};
impl Permission for AppPermission {
    fn gate(self) -> Gate {
        Gate::borrowed(match self {
            Self::View => "results.view",
            Self::Edit => "device.configure",
        })
    }
}

fn fixture() -> crate::Result<(Fixture, SessionController)> {
    let mut fixture = Fixture::new()?;
    let mut access = Access::from_config(&AccessConfig {
        session_tokens: true,
        ..AccessConfig::default()
    })?;
    let (controller, token) = SessionController::issue(
        Subject::default(),
        vec![
            SessionGrant::base_for(View),
            SessionGrant::inactive_for(Edit),
        ],
        None,
        fixture.now,
        Waker::new(&egui::Context::default()),
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    assert!(access.install_session(token, fixture.now));
    fixture.set_access(access);
    Ok((fixture, controller))
}

#[test]
fn protected_scope_uses_current_context_and_never_caches_admission() -> crate::Result<()> {
    let (mut fixture, mut owner) = fixture()?;
    let calls = Cell::new(0);
    let mut view = Protected::new(View, |_: &mut Cx<'_>| {
        calls.set(calls.get() + 1);
        41
    });
    let mut edit = Protected::new(Edit, |_: &mut Cx<'_>| {
        calls.set(calls.get() + 100);
        42
    });
    {
        let mut parts = fixture.parts();
        let mut cx = parts.cx(pane(InstanceId::NONE), None);
        assert_eq!(view.run(&mut cx), Some(41));
        assert_eq!(edit.run(&mut cx), None);
        owner
            .replace_grants(vec![SessionGrant::base_for(Edit)], None, cx.now)
            .map_err(|error| crate::Error::Config(error.to_string()))?;
        assert_eq!(
            view.run(&mut cx),
            None,
            "typed scope retained an obsolete permission"
        );
        assert_eq!(edit.run(&mut cx), Some(42));
        owner.revoke();
        assert_eq!(
            edit.run(&mut cx),
            None,
            "revoked typed scope invoked its callback"
        );
    }
    assert_eq!(calls.get(), 101);
    // Even a legacy context that allows every gate cannot admit a typed callback.
    let mut legacy = Fixture::new()?;
    let mut parts = legacy.parts();
    assert_eq!(view.run(&mut parts.cx(pane(InstanceId::NONE), None)), None);
    assert_eq!(calls.get(), 101);
    Ok(())
}

struct Recorder(Rc<Cell<usize>>);
impl Screen for Recorder {
    fn ui(&mut self, _: &mut egui::Ui, _: &mut Cx<'_>) {
        self.0.set(self.0.get() + 1);
    }
    fn on_back(&mut self, _: &mut Cx<'_>) -> BackAction {
        self.0.set(self.0.get() + 10);
        BackAction::Consumed
    }
    fn on_lifecycle(&mut self, _: Lifecycle, _: &mut Cx<'_>) {
        self.0.set(self.0.get() + 100);
    }
    fn on_result(&mut self, _: &str, _: ScreenValue, _: &mut Cx<'_>) {
        self.0.set(self.0.get() + 1000);
    }
}

#[test]
fn every_screen_hook_is_guarded_even_when_called_without_shell_navigation() -> crate::Result<()> {
    let (mut fixture, mut owner) = fixture()?;
    let calls = Rc::new(Cell::new(0));
    let element = Protected::new(
        View,
        crate::screen::screen_of("report", Recorder(Rc::clone(&calls)))
            .gate("wrong")
            .visibility(Visibility::Locked),
    );
    let Decl::Screen(decl) = element.into_decl() else {
        return Err(crate::Error::Config("not a screen".into()));
    };
    assert_eq!(decl.gate_name().as_str(), View.gate().as_str());
    assert_eq!(decl.visibility, Visibility::Hidden);
    let mut screen = decl
        .into_resident()
        .ok_or_else(|| crate::Error::Config("not resident".into()))?;
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    assert_eq!(screen.on_back(&mut cx), BackAction::Consumed);
    screen.on_lifecycle(Lifecycle::Created, &mut cx);
    screen.on_result("child", ScreenValue::None, &mut cx);
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |ui| {
        screen.ui(ui, &mut cx);
        assert_eq!(calls.get(), 1111);
        owner.revoke();
        screen.ui(ui, &mut cx);
        assert_eq!(screen.on_back(&mut cx), BackAction::Pop);
        screen.on_lifecycle(Lifecycle::Destroyed, &mut cx);
        screen.on_result("child", ScreenValue::None, &mut cx);
        assert_eq!(calls.get(), 1111, "revoked screen hook invoked user code");
    })
    .drop_without_applying_deltas();
    Ok(())
}

#[test]
fn denied_render_scope_emits_no_user_shapes_or_backend_effects() -> crate::Result<()> {
    let (mut fixture, mut owner) = fixture()?;
    let ctx = egui::Context::default();
    let calls = Cell::new(0);
    let marker = egui::Color32::from_rgb(21, 43, 65);
    let mut element = Protected::new(View, |ui: &mut egui::Ui, _: &mut Cx<'_>| {
        calls.set(calls.get() + 1);
        ui.painter().rect_filled(ui.max_rect(), 0.0, marker);
    });
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    let mut observed = 0;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(element.render(ui, &mut cx).is_some());
        observed = calls.get();
        owner.revoke();
    })
    .drop_without_applying_deltas();
    assert!(observed > 0);
    let output = ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(element.render(ui, &mut cx).is_none());
    });
    assert_eq!(calls.get(), observed, "denied renderer executed user code");
    assert!(
        !output.shapes.iter().any(
            |shape| matches!(&shape.shape, egui::epaint::Shape::Rect(rect) if rect.fill == marker)
        ),
        "denied renderer exposed a protected shape"
    );
    output.drop_without_applying_deltas();
    Ok(())
}

#[test]
fn factory_is_guarded_and_revocation_during_construction_blocks_all_hooks() -> crate::Result<()> {
    let (mut fixture, owner) = fixture()?;
    let owner = Rc::new(RefCell::new(owner));
    let constructors = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let (issuer, built, hooks) = (
        Rc::clone(&owner),
        Rc::clone(&constructors),
        Rc::clone(&calls),
    );
    let decl = crate::screen::screen_with("factory", move || {
        built.set(built.get() + 1);
        issuer.borrow_mut().revoke();
        Recorder(Rc::clone(&hooks))
    });
    let Decl::Screen(mut decl) = Protected::new(View, decl).into_decl() else {
        return Err(crate::Error::Config("not screen".into()));
    };
    let crate::workspace::InstanceScreen::Owned(mut screen) = decl.spawn() else {
        return Err(crate::Error::Config("not factory".into()));
    };
    assert_eq!(constructors.get(), 0);
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    screen.on_lifecycle(Lifecycle::Created, &mut cx);
    assert_eq!(constructors.get(), 1);
    assert_eq!(
        calls.get(),
        0,
        "factory revocation admitted a screen callback"
    );
    screen.on_lifecycle(Lifecycle::Destroyed, &mut cx);
    assert_eq!(constructors.get(), 1);
    assert_eq!(calls.get(), 0);
    Ok(())
}

#[test]
fn erased_action_and_chrome_callbacks_keep_their_typed_binding() -> crate::Result<()> {
    let (mut fixture, mut owner) = fixture()?;
    let calls = Rc::new(Cell::new(0));
    let c = Rc::clone(&calls);
    let Decl::Action(mut action) =
        Protected::new(View, crate::action("op", move |_| c.set(c.get() + 1))).into_decl()
    else {
        return Err(crate::Error::Config("not action".into()));
    };
    let c = Rc::clone(&calls);
    let Decl::StatusItem(mut status) = Protected::new(
        View,
        crate::status_item("status", crate::chrome::Slot::Right, move |_, _| {
            c.set(c.get() + 10);
        }),
    )
    .into_decl() else {
        return Err(crate::Error::Config("not status".into()));
    };
    let c = Rc::clone(&calls);
    let Decl::NavItem(mut nav) = Protected::new(
        View,
        crate::nav_item("nav", move |_, _| c.set(c.get() + 100)),
    )
    .into_decl() else {
        return Err(crate::Error::Config("not nav".into()));
    };
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    let ctx = egui::Context::default();
    ctx.run_ui(egui::RawInput::default(), |ui| {
        action.run(&mut cx);
        status.ui.paint(ui, &mut cx);
        nav.ui.paint(ui, &mut cx);
    })
    .drop_without_applying_deltas();
    let before = calls.get();
    assert!(before >= 111);
    owner.revoke();
    ctx.run_ui(egui::RawInput::default(), |ui| {
        action.run(&mut cx);
        status.ui.paint(ui, &mut cx);
        nav.ui.paint(ui, &mut cx);
    })
    .drop_without_applying_deltas();
    assert_eq!(calls.get(), before, "erased callback lost its typed guard");
    Ok(())
}

#[test]
fn protected_shell_requires_token_mode_and_dispatches_only_current_grants() -> crate::Result<()> {
    let ctx = egui::Context::default();
    assert!(crate::Shell::builder(crate::ShellConfig::default())
        .build_protected(&ctx)
        .is_err());
    let mut config = crate::ShellConfig::default();
    config.access.session_tokens = true;
    let mut shell = crate::Shell::builder(config).build_protected(&ctx)?;
    let calls = Rc::new(Cell::new(0));
    let c = Rc::clone(&calls);
    shell.add(Protected::new(
        View,
        crate::action("run", move |_| c.set(c.get() + 1)).gate("wrong"),
    ));
    shell.launch(crate::LaunchAction::run("run"));
    assert_eq!(calls.get(), 0);
    let request = shell
        .poll_events()
        .into_iter()
        .find_map(|event| match event {
            crate::ShellEvent::Access(crate::access::AccessEvent::SessionRequested {
                request,
                ..
            }) => Some(request),
            _ => None,
        })
        .ok_or_else(|| crate::Error::Config("no session challenge".into()))?;
    let now = shell.now();
    let (mut owner, token) = SessionController::issue(
        Subject::default(),
        vec![SessionGrant::base_for(View)],
        None,
        now,
        Waker::new(&ctx),
    )
    .map_err(|error| crate::Error::Config(error.to_string()))?;
    shell.accept_auth_response(request.respond(crate::access::AuthOutcome::Session(token)));
    assert_eq!(calls.get(), 1);
    owner.revoke();
    shell.launch(crate::LaunchAction::run("run"));
    assert_eq!(calls.get(), 1);
    Ok(())
}

#[cfg(feature = "overlay")]
#[test]
fn tile_panel_keeps_its_guard_after_declaration_erasure() -> crate::Result<()> {
    let (mut fixture, mut owner) = fixture()?;
    let calls = Rc::new(Cell::new(0));
    let c = Rc::clone(&calls);
    let panel = crate::tile_panel("panel", 100.0, move |_, _| c.set(c.get() + 1)).gate("wrong");
    let Decl::Tile(panel) = Protected::new(View, panel).into_decl() else {
        return Err(crate::Error::Config("not a tile".into()));
    };
    assert_eq!(panel.gate_name().as_str(), View.gate().as_str());
    let ctx = egui::Context::default();
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    ctx.run_ui(egui::RawInput::default(), |ui| {
        panel.draw_panel(ui, &mut cx);
    })
    .drop_without_applying_deltas();
    let before = calls.get();
    assert!(before > 0);
    owner.revoke();
    ctx.run_ui(egui::RawInput::default(), |ui| {
        panel.draw_panel(ui, &mut cx);
    })
    .drop_without_applying_deltas();
    assert_eq!(calls.get(), before, "revoked panel invoked user code");
    Ok(())
}

#[test]
fn admitted_callbacks_cannot_install_persistent_icon_or_widget_callbacks() -> crate::Result<()> {
    let (mut fixture, mut owner) = fixture()?;
    let calls = Rc::new(Cell::new(0usize));
    let widget_calls = Rc::clone(&calls);
    let icon_calls = Rc::clone(&calls);
    let ctx = egui::Context::default();
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    let binding = PermissionBinding::new(View);
    let id = binding
        .run(&mut cx, |cx| {
            if let Some(painters) = cx.widgets().painters {
                *painters = crate::widgets::WidgetPainters::new().button(move |_, _| {
                    widget_calls.set(widget_calls.get() + 100);
                });
            }
            cx.icons.register_painter(Box::new(move |_, _, _, _| {
                icon_calls.set(icon_calls.get() + 1);
            }))
        })
        .ok_or_else(|| crate::Error::Config("not admitted".into()))?;
    owner.revoke();
    assert!(binding
        .run(&mut cx, |_| panic!("revoked callback"))
        .is_none());
    ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(
            !cx.icons.paint(
                ui.painter(),
                ui.max_rect(),
                &crate::icons::IconRef::Custom(id),
                &crate::icons::IconStyle::sized(20.0),
                cx.theme
            ),
            "callback escaped its admission scope"
        );
        crate::widgets::BigButton::new("probe").show(ui, &mut cx.widgets());
    })
    .drop_without_applying_deltas();
    assert_eq!(calls.get(), 0);
    Ok(())
}

#[test]
fn panicking_admitted_callback_cannot_leave_painters_in_the_shell() -> crate::Result<()> {
    let binding = PermissionBinding::new(View);
    let (mut fixture, owner) = fixture()?;
    let mut parts = fixture.parts();
    let mut cx = parts.cx(pane(InstanceId::NONE), None);
    let id = Rc::new(Cell::new(None));
    let written = Rc::clone(&id);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        binding.run(&mut cx, |cx| {
            written.set(Some(
                cx.icons
                    .register_painter(Box::new(|_, _, _, _| panic!("escaped"))),
            ));
            panic!("user callback failed");
        });
    }));
    assert!(panic.is_err());
    let ctx = egui::Context::default();
    let id = id
        .get()
        .ok_or_else(|| crate::Error::Config("callback not admitted".into()))?;
    ctx.run_ui(egui::RawInput::default(), |ui| {
        assert!(!cx.icons.paint(
            ui.painter(),
            ui.max_rect(),
            &crate::icons::IconRef::Custom(id),
            &crate::icons::IconStyle::sized(20.0),
            cx.theme
        ));
    })
    .drop_without_applying_deltas();
    drop(owner);
    Ok(())
}
