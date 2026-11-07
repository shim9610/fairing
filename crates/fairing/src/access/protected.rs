//! Application-owned permission values with live framework-owned admission.
//!
//! ```
//! use fairing::access::{Gate, Permission, PermissionBound, Protected};
//! #[derive(Clone, Copy, Debug, PartialEq)]
//! enum AppPermission { ViewResults, Calibrate }
//! impl Permission for AppPermission {
//!     fn gate(self) -> Gate {
//!         Gate::borrowed(match self {
//!             Self::ViewResults => "results.view",
//!             Self::Calibrate => "device.calibrate",
//!         })
//!     }
//! }
//! let element = Protected::new(AppPermission::ViewResults, fairing::screen("results", |ui, _| {
//!     ui.label("Results");
//! }));
//! assert_eq!(element.required_permission(), AppPermission::ViewResults);
//! assert_eq!(element.permission().gate().as_str(), "results.view");
//! ```
//!
//! Untyped declarations cannot enter protected registration:
//! ```compile_fail
//! fn register(shell: &mut fairing::shell::ProtectedShell) {
//!     shell.add(fairing::action("unguarded", |_| {}));
//! }
//! ```
//! Missing permission mappings fail compilation:
//! ```compile_fail
//! #[derive(Clone, Copy)]
//! struct Missing;
//! let _ = fairing::access::Protected::new(Missing, ());
//! ```
//! There is no mutable legacy-shell escape:
//! ```compile_fail
//! fn escape(shell: &mut fairing::shell::ProtectedShell) -> &mut fairing::Shell {
//!     &mut shell.inner
//! }
//! ```
//! Bindings contain no writable cached authorization:
//! ```compile_fail
//! fn forge<P: fairing::access::Permission>(binding: &mut fairing::access::PermissionBinding<P>) {
//!     binding.allowed = true;
//! }
//! ```

use super::{protected_dispatch, Gate, Visibility};
use crate::screen::{BackAction, Cx, Decl, Lifecycle, Screen, ScreenSource, ScreenValue};

#[cfg(test)]
#[path = "protected_tests.rs"]
mod tests;

/// An application-owned permission value and its token-grant identifier.
///
/// Implement this for a small enum or value struct. Different values of the same
/// type may name different grants. Role expansion belongs to the issuer. This
/// metadata mapping must be deterministic and side-effect-free; it cannot grant
/// authority. Values with the same identifier intentionally share a requirement.
pub trait Permission: Copy + 'static {
    /// Normalize this value to the issuer's grant catalog. Called when binding the
    /// value, not as a user-defined authorization decision.
    fn gate(self) -> Gate;
}

/// Required permission metadata for an application-defined protected element.
///
/// The method is mandatory and has no default allow. It must be a side-effect-free
/// metadata lookup. Registered screens keep a fixed requirement until replacement;
/// element trees additionally detect a changed value between prepare and paint.
/// Authorization is always evaluated by Fairing against the current token.
pub trait PermissionBound {
    /// The application's enum or value struct, not a Fairing role vocabulary.
    type Required: Permission;
    /// Declare the permission required by this element.
    fn required_permission(&self) -> Self::Required;
}

/// An immutable permission value and normalized identifier, without cached authority.
pub struct PermissionBinding<P: Permission> {
    required: P,
    gate: Gate,
}

impl<P: Permission> PermissionBinding<P> {
    /// Bind metadata. No permission is granted or authorization result retained.
    pub fn new(required: P) -> Self {
        Self {
            gate: required.gate(),
            required,
        }
    }

    /// The application-owned permission value.
    #[must_use]
    pub fn required_permission(&self) -> P {
        self.required
    }

    /// The immutable normalized grant identifier, without per-call allocation.
    #[must_use]
    pub fn gate(&self) -> &Gate {
        &self.gate
    }

    /// Observe the calling context's current token and narrowing policy.
    #[must_use]
    pub fn allows(&self, cx: &Cx<'_>) -> bool {
        cx.token_mode() && cx.allows_gate(&self.gate)
    }

    /// Invoke only after a fresh token-mode check. The callback keeps the ordinary
    /// app/backend context. Icon/widget extension stores are callback-local; keep an
    /// application-owned store if it must persist across calls. Arbitrary effects
    /// inside an admitted callback remain trusted.
    pub fn run<R>(&self, cx: &mut Cx<'_>, callback: impl FnOnce(&mut Cx<'_>) -> R) -> Option<R> {
        let permission = self.allows(cx);
        let allowed = protected_dispatch::admitted(cx.token_mode(), permission);
        protected_dispatch::invoke(allowed, || cx.with_protected_resources(callback))
    }

    /// Draw only after a fresh check; denied callbacks receive neither UI nor context.
    pub fn render<R>(
        &self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        callback: impl FnOnce(&mut egui::Ui, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        self.run(cx, |cx| callback(ui, cx))
    }
}

/// An element and its mandatory permission binding. The body cannot be extracted.
#[must_use]
pub struct Protected<P: Permission, T> {
    binding: PermissionBinding<P>,
    body: T,
}

impl<P: Permission, T> Protected<P, T> {
    /// Declare a requirement. This constructor never grants or tests a permission.
    pub fn new(required: P, body: T) -> Self {
        Self {
            binding: PermissionBinding::new(required),
            body,
        }
    }

    /// Inspect the immutable binding. It contains metadata, not a reusable permit.
    #[must_use]
    pub fn permission(&self) -> &PermissionBinding<P> {
        &self.binding
    }

    /// Execute a callable element with a new check on every call.
    pub fn run<R>(&mut self, cx: &mut Cx<'_>) -> Option<R>
    where
        T: FnMut(&mut Cx<'_>) -> R,
    {
        self.binding.run(cx, &mut self.body)
    }

    /// Render a callable element with a new check on every call.
    pub fn render<R>(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) -> Option<R>
    where
        T: FnMut(&mut egui::Ui, &mut Cx<'_>) -> R,
    {
        self.binding.render(ui, cx, &mut self.body)
    }
}

impl<P: Permission, T> PermissionBound for Protected<P, T> {
    type Required = P;
    fn required_permission(&self) -> P {
        self.binding.required_permission()
    }
}

/// A framework declaration that can be registered with a typed requirement.
///
/// Sealed to the built-in screen/action/status/nav/tile declaration types.
pub trait ProtectedDeclaration: sealed::Declaration {}

pub(crate) mod sealed {
    use super::{Decl, Permission, PermissionBinding};
    pub trait Declaration {
        fn protect<P: Permission>(self, binding: PermissionBinding<P>) -> Decl;
    }
}

impl<P: Permission, T: ProtectedDeclaration> Protected<P, T> {
    pub(crate) fn into_decl(self) -> Decl {
        sealed::Declaration::protect(self.body, self.binding)
    }
}

struct GuardedScreen<P: Permission> {
    binding: PermissionBinding<P>,
    body: Option<Box<dyn Screen>>,
    factory: Option<Box<dyn FnMut() -> Box<dyn Screen>>>,
}

impl<P: Permission> GuardedScreen<P> {
    fn with_body<R>(
        &mut self,
        cx: &mut Cx<'_>,
        callback: impl FnOnce(&mut dyn Screen, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let Self {
            binding,
            body,
            factory,
        } = self;
        binding
            .run(cx, |cx| {
                if body.is_none() {
                    if let Some(factory) = factory {
                        *body = Some(factory());
                    }
                }
                // A factory can revoke its session. Recheck before the screen callback.
                body.as_mut()
                    .and_then(|body| binding.run(cx, |cx| callback(&mut **body, cx)))
            })
            .flatten()
    }
}

impl<P: Permission> Screen for GuardedScreen<P> {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        self.with_body(cx, |body, cx| body.ui(ui, cx));
    }
    fn on_back(&mut self, cx: &mut Cx<'_>) -> BackAction {
        self.with_body(cx, |body, cx| body.on_back(cx))
            .unwrap_or(BackAction::Pop)
    }
    fn on_lifecycle(&mut self, event: Lifecycle, cx: &mut Cx<'_>) {
        self.with_body(cx, |body, cx| body.on_lifecycle(event, cx));
    }
    fn on_result(&mut self, from: &str, value: ScreenValue, cx: &mut Cx<'_>) {
        self.with_body(cx, |body, cx| body.on_result(from, value, cx));
    }
}

impl ProtectedDeclaration for crate::screen::ScreenDecl {}
impl sealed::Declaration for crate::screen::ScreenDecl {
    fn protect<P: Permission>(mut self, binding: PermissionBinding<P>) -> Decl {
        self.gate = Some(binding.gate().clone());
        self.visibility = Visibility::Hidden;
        self.source = match self.source {
            ScreenSource::Resident(body) => ScreenSource::Resident(Some(Box::new(GuardedScreen {
                binding,
                body,
                factory: None,
            }))),
            ScreenSource::Factory(factory) => {
                let required = binding.required_permission();
                let factory = std::rc::Rc::new(std::cell::RefCell::new(factory));
                ScreenSource::Factory(Box::new(move || {
                    let factory = std::rc::Rc::clone(&factory);
                    Box::new(GuardedScreen::<P> {
                        binding: PermissionBinding::new(required),
                        body: None,
                        factory: Some(Box::new(move || (factory.borrow_mut())())),
                    })
                }))
            }
        };
        Decl::Screen(self)
    }
}

impl ProtectedDeclaration for crate::screen::ActionDecl {}
impl sealed::Declaration for crate::screen::ActionDecl {
    fn protect<P: Permission>(mut self, binding: PermissionBinding<P>) -> Decl {
        self.gate = Some(binding.gate().clone());
        self.visibility = Visibility::Hidden;
        let mut body = self.run;
        self.run = Box::new(move |cx| {
            binding.run(cx, &mut body);
        });
        Decl::Action(self)
    }
}

impl ProtectedDeclaration for crate::chrome::StatusItemDecl {}
impl sealed::Declaration for crate::chrome::StatusItemDecl {
    fn protect<P: Permission>(mut self, binding: PermissionBinding<P>) -> Decl {
        self.gate = Some(binding.gate().clone());
        let mut body = self.ui;
        self.ui = Box::new(move |ui, cx| {
            binding.render(ui, cx, &mut body);
        });
        Decl::StatusItem(self)
    }
}

impl ProtectedDeclaration for crate::chrome::NavItemDecl {}
impl sealed::Declaration for crate::chrome::NavItemDecl {
    fn protect<P: Permission>(mut self, binding: PermissionBinding<P>) -> Decl {
        self.gate = Some(binding.gate().clone());
        let mut body = self.ui;
        self.ui = Box::new(move |ui, cx| {
            binding.render(ui, cx, &mut body);
        });
        Decl::NavItem(self)
    }
}

#[cfg(feature = "overlay")]
impl ProtectedDeclaration for crate::overlay::TileDecl {}
#[cfg(feature = "overlay")]
impl sealed::Declaration for crate::overlay::TileDecl {
    fn protect<P: Permission>(self, binding: PermissionBinding<P>) -> Decl {
        Decl::Tile(self.protect_panel(binding))
    }
}
