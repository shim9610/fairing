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
///
/// Replacement cannot downgrade to an ordinary shell:
/// ```compile_fail
/// fn downgrade(shell: &mut fairing::ProtectedShell, ctx: &fairing::egui::Context) {
///     let _ = shell.restart_with(|| fairing::Shell::builder(fairing::ShellConfig::default()).build(ctx));
/// }
/// ```
pub struct ProtectedShell {
    inner: Shell,
}

impl ProtectedShell {
    pub(super) fn new(mut inner: Shell) -> Self {
        inner.access.require_mediation();
        inner.registry.bind_initial_library_requirements();
        Self { inner }
    }

    /// Whether dispatch is quarantined after a runtime or host fault.
    #[must_use]
    pub const fn is_faulted(&self) -> bool {
        self.inner.is_faulted()
    }

    /// Stop dispatch and close command admission before host recovery.
    pub fn quarantine(&mut self) {
        self.inner.quarantine();
    }

    /// The shell's current monotonic observation time.
    #[must_use]
    pub fn now(&self) -> std::time::Instant {
        self.inner.now()
    }

    /// Replace with a validated protected shell, closing old admission first.
    ///
    /// # Errors
    /// Construction, disposal or wake failure leaves dispatch quarantined.
    pub fn restart_with(
        &mut self,
        build: impl FnOnce() -> crate::Result<Self>,
    ) -> Result<(), crate::recovery::RestartError> {
        self.inner
            .restart_with(|| build().map(|replacement| replacement.inner))
    }

    /// Set optional fault guidance and clear any pending restart request.
    pub fn set_recovery_ui(&mut self, view: Option<crate::recovery::RecoveryUi>) {
        self.inner.set_recovery_ui(view);
    }

    /// Consume the fault view's coalesced restart request.
    pub fn take_restart_request(&mut self) -> bool {
        self.inner.take_restart_request()
    }

    #[cfg(feature = "settings")]
    pub(crate) fn register_settings<P: Permission>(
        &mut self,
        config: &crate::settings::SettingsConfig,
        entries: Vec<crate::settings::screens::SettingsEntry>,
        mut permission_for: impl FnMut(&str) -> P,
    ) {
        for decl in crate::settings::declarations(&mut self.inner, config, entries) {
            let permission = permission_for(decl.id());
            self.add(Protected::new(permission, decl));
        }
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

    /// Observe supported host state without a legacy-shell or component loan.
    #[must_use]
    pub fn observe(&self) -> super::ShellView<'_> {
        self.inner.observe()
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
