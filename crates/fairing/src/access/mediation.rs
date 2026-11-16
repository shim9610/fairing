//! Storage and invocation of protected application payloads. No extraction or Deref API.
#[macro_use]
pub(crate) mod contract;

use super::{protected_dispatch, Gate, Permission, PermissionBinding, Visibility};
use crate::screen::{BackAction, Cx, Lifecycle, Screen, ScreenDecl, ScreenValue};

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

mod execution;
pub(crate) use execution::{ActionCallback, FactoryCallback, UiCallback};

protected_contract! { carrier
/// Opaque screen ownership. The executable storage exposes dispatch only.
pub struct ScreenHandle : dyn Screen {
    storage: execution::ScreenStorage,
    requirement: Requirement,
}
}
impl ScreenHandle {
    pub(crate) fn legacy(body: Box<dyn Screen>) -> Self {
        Self {
            storage: execution::ScreenStorage::legacy(body),
            requirement: Requirement::Legacy,
        }
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

protected_contract!(binding [] ScreenHandle);
