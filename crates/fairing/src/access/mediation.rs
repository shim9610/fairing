//! The sole owner and invoker of application payloads. No extraction or Deref API.
#[macro_use]
pub(crate) mod contract;

use super::{protected_dispatch, Gate, Permission, PermissionBinding, Visibility};
use crate::screen::{BackAction, Cx, Lifecycle, Screen, ScreenDecl, ScreenValue};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone)]
enum Requirement {
    Legacy,
    Required(Gate),
}
impl Requirement {
    fn admission(&self) -> Admission<'_> {
        match self {
            Self::Legacy => Admission::Legacy,
            Self::Required(gate) => Admission::Gate(gate),
        }
    }
}

type Stamp = (super::SessionId, u64);
/// Immutable requirements along a live dispatch path, never an authorization result.
pub(crate) struct AuthorityPath<'a> {
    gate: &'a Gate,
    stamp: Stamp,
    parent: Option<&'a AuthorityPath<'a>>,
}
impl<'a> AuthorityPath<'a> {
    fn new(gate: &'a Gate, stamp: Stamp, parent: Option<&'a Self>) -> Self {
        Self {
            gate,
            stamp,
            parent,
        }
    }
    pub(crate) fn allows(&self, cx: &Cx<'_>) -> bool {
        if !cx.token_mode() || cx.authority_stamp() != Some(self.stamp) {
            return false;
        }
        let mut next = Some(self);
        while next.is_some() {
            let Some(path) = next else { return false };
            if path.stamp != self.stamp || !cx.allows_gate(path.gate) {
                return false;
            }
            next = path.parent;
        }
        cx.authority_stamp() == Some(self.stamp)
    }
}
#[derive(Clone, Copy)]
enum Admission<'a> {
    Legacy,
    Gate(&'a Gate),
    Path(&'a AuthorityPath<'a>),
}
impl Admission<'_> {
    fn allows(&self, cx: &Cx<'_>) -> bool {
        match self {
            Self::Legacy => !cx.requires_mediation(),
            Self::Gate(gate) => {
                let permission = cx.token_mode() && cx.allows_gate(gate);
                protected_dispatch::admitted(cx.token_mode(), permission)
            }
            Self::Path(path) => path.allows(cx),
        }
    }
}
// All payload operations and adapters use this single primitive.
fn invoke<R>(
    admission: Admission<'_>,
    cx: &mut Cx<'_>,
    operation: impl FnOnce(&mut Cx<'_>) -> R,
) -> Option<R> {
    if let Admission::Gate(gate) = admission {
        return invoke_bound(gate, None, cx, operation);
    }
    let allowed = admission.allows(cx);
    protected_dispatch::invoke(allowed, || {
        if let Admission::Path(path) = admission {
            cx.with_protected_resources(path, operation)
        } else {
            operation(cx)
        }
    })
}
// Cx supplies the inherited chain. Callers cannot omit or replace its ancestors.
fn invoke_bound<R>(
    gate: &Gate,
    expected: Option<Stamp>,
    cx: &mut Cx<'_>,
    operation: impl FnOnce(&mut Cx<'_>) -> R,
) -> Option<R> {
    let stamp = cx.authority_stamp()?;
    if expected.is_some_and(|expected| expected != stamp) {
        return None;
    }
    let path = AuthorityPath::new(gate, stamp, cx.authority_path());
    invoke(Admission::Path(&path), cx, operation)
}

pub(crate) fn invoke_requirement<R>(
    gate: &Gate,
    stamp: Stamp,
    cx: &mut Cx<'_>,
    operation: impl FnOnce(&mut Cx<'_>) -> R,
) -> Option<R> {
    invoke_bound(gate, Some(stamp), cx, operation)
}
pub(super) fn invoke_gate<R>(
    gate: &Gate,
    cx: &mut Cx<'_>,
    operation: impl FnOnce(&mut Cx<'_>) -> R,
) -> Option<R> {
    invoke(Admission::Gate(gate), cx, operation)
}

protected_contract! { carrier
/// A payload can be constructed and bound, but never extracted or borrowed.
pub(crate) struct Callback<T: ?Sized> : T {
    body: Box<T>,
    requirement: Requirement,
}
}

impl<T: ?Sized> Callback<T> {
    pub(crate) fn legacy(body: Box<T>) -> Self {
        Self {
            body,
            requirement: Requirement::Legacy,
        }
    }

    // Used only to bind library-created initial declarations before publication.

    fn invoke<R>(
        &mut self,
        cx: &mut Cx<'_>,
        operation: impl FnOnce(&mut T, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let Self { body, requirement } = self;
        invoke(requirement.admission(), cx, |cx| operation(body, cx))
    }
}

pub(crate) type ActionCallback = Callback<dyn FnMut(&mut Cx<'_>)>;
pub(crate) type UiCallback = Callback<dyn FnMut(&mut egui::Ui, &mut Cx<'_>)>;

// One operation declaration emits its mandatory mediated adapter.
protected_contract!(operation ActionCallback, run(), |body, cx| body(cx));
protected_contract!(operation UiCallback, paint(ui: &mut egui::Ui), |body, cx| body(
    ui, cx
));

type ScreenFactory = dyn FnMut() -> Box<dyn Screen>;
protected_contract! { carrier
/// Shared factory storage stays inside the mediation module.
pub(crate) struct FactoryCallback : ScreenFactory {
    body: Rc<RefCell<Box<ScreenFactory>>>,
    requirement: Requirement,
}
}

impl FactoryCallback {
    pub(crate) fn legacy(body: Box<ScreenFactory>) -> Self {
        Self {
            body: Rc::new(RefCell::new(body)),
            requirement: Requirement::Legacy,
        }
    }

    pub(crate) fn instance(&self) -> ScreenHandle {
        ScreenHandle {
            body: None,
            factory: Some(Rc::clone(&self.body)),
            requirement: self.requirement.clone(),
        }
    }
}

protected_contract! { carrier
/// Opaque screen ownership used by declarations, instances and resident loans.
///
/// Application code cannot extract the original screen or its factory.
pub struct ScreenHandle : dyn Screen {
    body: Option<Box<dyn Screen>>,
    factory: Option<Rc<RefCell<Box<ScreenFactory>>>>,
    requirement: Requirement,
}
}

impl ScreenHandle {
    pub(crate) fn legacy(body: Box<dyn Screen>) -> Self {
        Self {
            body: Some(body),
            factory: None,
            requirement: Requirement::Legacy,
        }
    }

    fn invoke<R>(
        &mut self,
        cx: &mut Cx<'_>,
        operation: impl FnOnce(&mut dyn Screen, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let Self {
            body,
            factory,
            requirement,
        } = self;
        invoke(requirement.admission(), cx, |cx| {
            if body.is_none() {
                *body = factory.as_ref().map(|factory| (factory.borrow_mut())());
            }
            body.as_mut().and_then(|body| {
                invoke(requirement.admission(), cx, |cx| operation(&mut **body, cx))
            })
        })
        .flatten()
    }
}

protected_contract!(screen ui(ui: &mut egui::Ui));
protected_contract!(screen on_back() -> BackAction, BackAction::Pop);
protected_contract!(screen on_lifecycle(event: Lifecycle));
protected_contract!(screen on_result(from: &str, value: ScreenValue));

pub(crate) mod declared;
#[path = "protected.rs"]
pub mod protected;

mod element;
pub(crate) use element::ElementHandle;

protected_contract!(binding [T: ?Sized] Callback<T>);

protected_contract!(binding [] FactoryCallback);

protected_contract!(binding [] ScreenHandle);
