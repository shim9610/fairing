//! Guard externally declared screens without exposing their executable payload.
use super::{BackAction, Cx, Lifecycle, Screen, ScreenDecl, ScreenValue};
use crate::access::{Permission, PermissionBinding, PermissionBound};

pub(crate) fn declared<S: Screen + PermissionBound + 'static>(
    id: impl Into<String>,
    body: S,
    required: S::Required,
) -> ScreenDecl {
    crate::screen::screen_of(
        id,
        Declared {
            binding: PermissionBinding::new(required),
            body,
        },
    )
}

protected_contract! { carrier
struct Declared<S: PermissionBound> : S {
    binding: PermissionBinding<S::Required>,
    body: S,
}
}

impl<S: Screen + PermissionBound> Declared<S> {
    fn with_body<R>(
        &mut self,
        cx: &mut Cx<'_>,
        callback: impl FnOnce(&mut S, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let Self { binding, body } = self;
        binding
            .run(cx, |cx| {
                // Registration requirements are immutable. Metadata drift must not
                // silently retain the registration's earlier, weaker requirement.
                (body.required_permission().gate() == *binding.gate())
                    .then(|| binding.run(cx, |cx| callback(body, cx)))
                    .flatten()
            })
            .flatten()
    }
}
impl<S: Screen + PermissionBound> Screen for Declared<S> {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.with_body(cx, |body, cx| body.ui(ui, cx));
    }
    fn on_back(&mut self, cx: &mut Cx<'_>) -> BackAction {
        self.with_body(cx, Screen::on_back)
            .unwrap_or(BackAction::Pop)
    }
    fn on_lifecycle(&mut self, event: Lifecycle, cx: &mut Cx<'_>) {
        self.with_body(cx, |body, cx| body.on_lifecycle(event, cx));
    }
    fn on_result(&mut self, from: &str, value: ScreenValue, cx: &mut Cx<'_>) {
        self.with_body(cx, |body, cx| body.on_result(from, value, cx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{Access, SessionController, SessionGrant, Subject};
    use crate::config::AccessConfig;
    use crate::screen::cx::fixture::{pane, Fixture};
    use crate::services::Waker;
    use crate::workspace::InstanceId;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Clone, Copy)]
    struct View;
    impl Permission for View {
        fn gate(self) -> crate::Gate {
            crate::Gate::borrowed("view")
        }
    }
    struct RevokingMetadata {
        owner: Rc<RefCell<SessionController>>,
        calls: Rc<Cell<usize>>,
    }
    impl PermissionBound for RevokingMetadata {
        type Required = View;
        fn required_permission(&self) -> View {
            // Deliberately violate the metadata purity contract. Admission must
            // still recheck the token before executing the screen callback.
            self.owner.borrow_mut().revoke();
            View
        }
    }
    impl Screen for RevokingMetadata {
        fn ui(&mut self, _: &mut egui::Ui, _: &mut Cx<'_>) {
            self.calls.set(self.calls.get() + 1);
        }
    }
    #[test]
    fn metadata_cannot_revoke_authority_and_then_admit_a_screen_callback() -> crate::Result<()> {
        let mut fixture = Fixture::new()?;
        let mut access = Access::from_config(&AccessConfig {
            session_tokens: true,
            ..AccessConfig::default()
        })?;
        let (owner, token) = SessionController::issue(
            Subject::default(),
            vec![SessionGrant::base_for(View)],
            None,
            fixture.now,
            Waker::new(&egui::Context::default()),
        )
        .map_err(|e| crate::Error::Config(e.to_string()))?;
        assert!(access.install_session(token, fixture.now));
        fixture.set_access(access);
        let calls = Rc::new(Cell::new(0));
        let mut body = Declared {
            binding: PermissionBinding::new(View),
            body: RevokingMetadata {
                owner: Rc::new(RefCell::new(owner)),
                calls: Rc::clone(&calls),
            },
        };
        let mut parts = fixture.parts();
        let mut cx = parts.cx(pane(InstanceId::NONE), None);
        egui::Context::default()
            .run_ui(egui::RawInput::default(), |ui| body.ui(ui, &mut cx))
            .drop_without_applying_deltas();
        assert_eq!(
            calls.get(),
            0,
            "metadata revocation admitted a screen callback"
        );
        Ok(())
    }
}
