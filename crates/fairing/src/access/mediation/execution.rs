//! Opaque executable storage. Callers can dispatch but cannot borrow a payload.
use super::{
    invoke, BackAction, Cx, Gate, Lifecycle, Permission, PermissionBinding, Requirement, Screen,
    ScreenHandle, ScreenValue,
};
use std::{cell::RefCell, rc::Rc};

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
            storage: ScreenStorage {
                body: None,
                factory: Some(Rc::clone(&self.body)),
            },
            requirement: self.requirement.clone(),
        }
    }
}

protected_contract! { carrier
pub(super) struct ScreenStorage : dyn Screen {
    body: Option<Box<dyn Screen>>,
    factory: Option<Rc<RefCell<Box<ScreenFactory>>>>,
}
}
impl ScreenStorage {
    pub(super) fn legacy(body: Box<dyn Screen>) -> Self {
        Self {
            body: Some(body),
            factory: None,
        }
    }

    fn invoke<R>(
        &mut self,
        requirement: &Requirement,
        cx: &mut Cx<'_>,
        operation: impl FnOnce(&mut dyn Screen, &mut Cx<'_>) -> R,
    ) -> Option<R> {
        let Self { body, factory } = self;
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

macro_rules! storage_method {
    ($name:ident($($arg:ident:$ty:ty),*)) => {
        pub(super) fn $name(&mut self, requirement:&Requirement, $($arg:$ty,)* cx:&mut Cx<'_>) {
            self.invoke(requirement,cx,|body,cx|body.$name($($arg,)*cx));
        }
    };
}
impl ScreenStorage {
    storage_method!(ui(ui: &mut egui::Ui));
    storage_method!(on_lifecycle(event: Lifecycle));
    storage_method!(on_result(from: &str, value: ScreenValue));
    pub(super) fn on_back(&mut self, requirement: &Requirement, cx: &mut Cx<'_>) -> BackAction {
        self.invoke(requirement, cx, |body, cx| body.on_back(cx))
            .unwrap_or(BackAction::Pop)
    }
}
protected_contract!(binding [T: ?Sized] Callback<T>);
protected_contract!(binding [] FactoryCallback);
