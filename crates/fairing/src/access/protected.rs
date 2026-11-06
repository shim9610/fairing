//! Typed permission requirements with live, framework-owned callback admission.
//!
//! A binding names a requirement; it never grants authority or caches a successful
//! check. Every invocation consults the calling context's current token session.
//! Use [`crate::shell::ProtectedShell`] for registration that requires these types.
//!
//! ```
//! use fairing::access::{Permission, PermissionBound, Protected};
//! struct ReadResults;
//! impl Permission for ReadResults { const GATE: &'static str = "results.view"; }
//! let element = Protected::<ReadResults, _>::new(fairing::screen("results", |ui, _| {
//!     ui.label("Results");
//! }));
//! assert_eq!(element.permission().gate().as_str(), "results.view");
//! ```
//!
//! An untyped declaration cannot enter the protected registration API:
//! ```compile_fail
//! fn register(shell: &mut fairing::shell::ProtectedShell) {
//!     shell.add(fairing::action("unguarded", |_| {}));
//! }
//! ```
//! A permission requirement must be implemented:
//! ```compile_fail
//! struct Missing;
//! let _ = fairing::access::Protected::<Missing, _>::new(());
//! ```
//! A protected shell does not expose mutable legacy registration:
//! ```compile_fail
//! fn escape(shell: &mut fairing::shell::ProtectedShell) -> &mut fairing::Shell {
//!     &mut shell.inner
//! }
//! ```
//! A binding is not a reusable authorization result:
//! ```compile_fail
//! fn forge(binding: &fairing::access::PermissionBinding<MyPermission>) {
//!     binding.allowed = true;
//! }
//! struct MyPermission;
//! impl fairing::access::Permission for MyPermission {
//!     const GATE: &'static str = "private";
//! }
//! ```

use super::{protected_dispatch, Gate, Visibility};
use crate::screen::{BackAction, Cx, Decl, Lifecycle, Screen, ScreenSource, ScreenValue};
use std::marker::PhantomData;

#[cfg(test)]
#[path = "protected_tests.rs"]
mod tests;

/// A static requirement, resolved against explicit gates in the current token.
///
/// Types with the same `GATE` intentionally name the same permission. This trait
/// declares policy; it does not establish that an external role assignment is correct.
pub trait Permission: 'static {
    /// The issuer's gate name. No numeric level or role name implies this grant.
    const GATE: &'static str;
}

/// A requirement bound to the invoking context, without cached authority.
pub struct PermissionBinding<P: Permission> {
    required: PhantomData<fn() -> P>,
}

impl<P: Permission> PermissionBinding<P> {
    fn new() -> Self {
        Self {
            required: PhantomData,
        }
    }

    /// The required gate, without allocation.
    #[must_use]
    pub fn gate(&self) -> Gate {
        Gate::borrowed(P::GATE)
    }

    /// Invoke only after a fresh token-mode check. The callback keeps the ordinary
    /// app/backend context. Icon/widget extension stores are callback-local; keep an
    /// application-owned store if it must persist across calls. Arbitrary effects
    /// inside an admitted callback remain trusted.
    pub fn run<R>(&self, cx: &mut Cx<'_>, callback: impl FnOnce(&mut Cx<'_>) -> R) -> Option<R> {
        let permission = cx.token_mode() && cx.allows(self.gate());
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

/// The required binding carried by a framework-owned protected element.
///
/// Sealed so that a client cannot substitute a method that reports cached authority.
pub trait PermissionBound: sealed::Bound {
    /// The statically declared requirement.
    type Required: Permission;
    /// A requirement, not evidence that the requirement currently passes.
    fn permission(&self) -> &PermissionBinding<Self::Required>;
}

/// An element and its mandatory permission binding. The body cannot be extracted.
#[must_use]
pub struct Protected<P: Permission, T> {
    binding: PermissionBinding<P>,
    body: T,
}

impl<P: Permission, T> Protected<P, T> {
    /// Declare a requirement. This constructor never grants or tests a permission.
    pub fn new(body: T) -> Self {
        Self {
            binding: PermissionBinding::new(),
            body,
        }
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

impl<P: Permission, T> sealed::Bound for Protected<P, T> {}
impl<P: Permission, T> PermissionBound for Protected<P, T> {
    type Required = P;
    fn permission(&self) -> &PermissionBinding<P> {
        &self.binding
    }
}

/// A framework declaration that can be registered with a typed requirement.
///
/// Sealed to the built-in screen/action/status/nav/tile declaration types.
pub trait ProtectedDeclaration: sealed::Declaration {}

pub(crate) mod sealed {
    use super::{Decl, Permission, PermissionBinding};
    pub trait Bound {}
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
        self.gate = Some(binding.gate());
        self.visibility = Visibility::Hidden;
        self.source = match self.source {
            ScreenSource::Resident(body) => ScreenSource::Resident(Some(Box::new(GuardedScreen {
                binding,
                body,
                factory: None,
            }))),
            ScreenSource::Factory(factory) => {
                let factory = std::rc::Rc::new(std::cell::RefCell::new(factory));
                ScreenSource::Factory(Box::new(move || {
                    let factory = std::rc::Rc::clone(&factory);
                    Box::new(GuardedScreen::<P> {
                        binding: PermissionBinding::new(),
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
        self.gate = Some(binding.gate());
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
        self.gate = Some(binding.gate());
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
        self.gate = Some(binding.gate());
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
