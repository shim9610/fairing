//! Registration facade with mandatory typed permission requirements.

use super::{Shell, ShellEvent, ShellHandle};
use crate::access::{
    AuthResponse, Authenticator, Permission, PermissionBound, Protected, ProtectedDeclaration,
};
use crate::screen::LaunchAction;

/// A shell whose application declarations must carry a typed permission binding.
///
/// Construct with [`super::ShellBuilder::build_protected`]. There is no conversion
/// to a mutable legacy shell. Untyped builder hooks are rejected at construction;
/// use typed declarations for application UI. Icon/widget painter stores lent to
/// admitted callbacks are local to that invocation and cannot install later hooks.
/// Authentication, backends, destructors and arbitrary effects inside an admitted
/// callback remain trusted. This facade does not mediate arbitrary raw egui calls.
pub struct ProtectedShell {
    inner: Shell,
}

impl ProtectedShell {
    pub(super) fn new(
        builder: super::builder::CallbackFreeBuilder,
        ctx: &egui::Context,
    ) -> crate::Result<Self> {
        let mut inner = builder.build(ctx)?;
        inner.access.require_mediation();
        inner.registry.bind_initial_library_requirements();
        Ok(Self { inner })
    }

    /// Register a declaration with its mandatory requirement. The binding overrides
    /// legacy gate/visibility settings; unauthorized screen/action icons are hidden.
    pub fn add<P: Permission, D: ProtectedDeclaration>(&mut self, element: Protected<P, D>) {
        self.inner.add(element.into_decl());
    }

    /// Register an application screen whose required enum value is mandatory.
    /// Fairing creates its guarded adapter; callers do not invoke the screen guard.
    pub fn add_screen<S: crate::screen::Screen + PermissionBound + 'static>(
        &mut self,
        id: impl Into<String>,
        screen: S,
    ) {
        let required = screen.required_permission();
        self.add(Protected::new(
            required,
            crate::access::mediation::declared::declared(id, screen, required),
        ));
    }

    /// Register an owned UI tree. Filtering, measurement, layout and paint dispatch
    /// are performed by the library using every node's current requirement.
    pub fn add_elements<P: Permission>(
        &mut self,
        id: impl Into<String>,
        tree: crate::elements::ElementTree<P>,
    ) {
        self.add_screen(id, tree);
    }

    /// Observe layout, state and diagnostics without mutable registration access.
    #[must_use]
    pub fn shell(&self) -> &Shell {
        &self.inner
    }

    /// Remove a declaration and close its instances.
    pub fn remove(&mut self, id: &str) -> bool {
        self.inner.remove(id)
    }

    /// Draw a frame, checking session state and protected callbacks.
    pub fn frame(&mut self, ui: &mut egui::Ui) {
        self.inner.frame(ui);
    }

    /// Draw with the original app state available to admitted callbacks.
    pub fn frame_with(&mut self, ui: &mut egui::Ui, app: &mut dyn std::any::Any) {
        self.inner.frame_with(ui, app);
    }

    /// Process commands and session deadlines without drawing.
    pub fn logic(&mut self, ctx: &egui::Context) {
        self.inner.logic(ctx);
    }

    /// Process commands with app state available to admitted callbacks.
    pub fn logic_with(&mut self, ctx: &egui::Context, app: &mut dyn std::any::Any) {
        self.inner.logic_with(ctx, app);
    }

    /// Dispatch through the current declaration's gates and typed callback binding.
    pub fn launch(&mut self, action: LaunchAction) {
        self.inner.launch(action);
    }

    /// Dispatch with the original app/backend context.
    pub fn launch_with(&mut self, action: LaunchAction, app: &mut dyn std::any::Any) {
        self.inner.launch_with(action, app);
    }

    /// A command handle; queued actions retain execution-time authorization checks.
    #[must_use]
    pub fn handle(&self) -> ShellHandle {
        self.inner.handle()
    }

    /// Drain host events, including external session authentication requests.
    pub fn poll_events(&mut self) -> Vec<ShellEvent> {
        self.inner.poll_events()
    }

    /// Accept only a still-current, request-correlated authentication result.
    pub fn accept_auth_response(&mut self, response: AuthResponse) {
        self.inner.accept_auth_response(response);
    }

    /// Accept a response with app state available to its authorized continuation.
    pub fn accept_auth_response_with(
        &mut self,
        response: AuthResponse,
        app: &mut dyn std::any::Any,
    ) {
        self.inner.accept_auth_response_with(response, app);
    }

    /// Invalidate the current authentication request.
    pub fn cancel_authentication(&mut self) {
        self.inner.cancel_authentication();
    }

    /// Replace the issuer and detach the old session first.
    pub fn set_authenticator(&mut self, authenticator: impl Authenticator + 'static) {
        self.inner.set_authenticator(authenticator);
    }

    /// Install a policy that may only narrow the token's permissions.
    pub fn set_policy(&mut self, policy: Box<dyn crate::access::AccessPolicy>) {
        self.inner.access_mut().set_policy(policy);
    }
}
