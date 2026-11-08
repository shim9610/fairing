//! Permission-aware UI elements laid out and invoked by Fairing.
//!
//! Applications implement [`Element`] and return their own permission enum from
//! [`PermissionBound::required_permission`]. Register an [`ElementTree`] using
//! [`crate::ProtectedShell::add_elements`]; the library filters, measures, lays out
//! and paints children. No per-child guard calls are required in application code.

//!
//! ```
//! use fairing::{Cx, Gate, Shell, ShellConfig};
//! use fairing::access::{Permission, PermissionBound, SessionGrant};
//! use fairing::elements::{Element, ElementTree};
//! #[derive(Clone, Copy)]
//! enum AppPermission { View, Edit }
//! impl Permission for AppPermission {
//!     fn gate(self) -> Gate {
//!         Gate::borrowed(match self { Self::View => "results.view", Self::Edit => "results.edit" })
//!     }
//! }
//! struct Results(AppPermission);
//! impl PermissionBound for Results {
//!     type Required = AppPermission;
//!     fn required_permission(&self) -> AppPermission { self.0 }
//! }
//! impl Element for Results {
//!     fn measure(&mut self, _: egui::Vec2, _: &mut Cx<'_>) -> egui::Vec2 { egui::vec2(200.0, 40.0) }
//!     fn paint(&mut self, ui: &mut egui::Ui, _: &mut Cx<'_>) { ui.label("Results"); }
//! }
//! let mut config = ShellConfig::default();
//! config.access.session_tokens = true;
//! let ctx = egui::Context::default();
//! let mut shell = Shell::builder(config).build_protected(&ctx)?;
//! shell.add_elements("results", ElementTree::column(AppPermission::View, 8.0, vec![
//!     ElementTree::leaf(Results(AppPermission::View)),
//!     ElementTree::leaf(Results(AppPermission::Edit)),
//! ])?);
//! // The issuer supplies corresponding values after credential verification.
//! let _grants = [SessionGrant::base_for(AppPermission::View), SessionGrant::inactive_for(AppPermission::Edit)];
//! # Ok::<(), fairing::Error>(())
//! ```
//!
//! Every implementation must supply permission metadata:
//! ```compile_fail
//! use fairing::{Cx, elements::Element};
//! struct Missing;
//! impl Element for Missing {
//!     fn measure(&mut self, _: egui::Vec2, _: &mut Cx<'_>) -> egui::Vec2 { egui::Vec2::ZERO }
//!     fn paint(&mut self, _: &mut egui::Ui, _: &mut Cx<'_>) {}
//! }
//! ```
//!
//! The declaration method has no default:
//! ```compile_fail
//! use fairing::access::{Permission, PermissionBound};
//! #[derive(Clone, Copy)]
//! struct View;
//! impl Permission for View { fn gate(self) -> fairing::Gate { fairing::Gate::borrowed("view") } }
//! struct MissingMethod;
//! impl PermissionBound for MissingMethod { type Required = View; }
//! ```
//!
//! One tree uses one application's permission type:
//! ```compile_fail
//! use fairing::access::Permission;
//! use fairing::elements::{element, ElementTree};
//! #[derive(Clone, Copy)] struct A;
//! #[derive(Clone, Copy)] struct B;
//! impl Permission for A { fn gate(self) -> fairing::Gate { fairing::Gate::borrowed("a") } }
//! impl Permission for B { fn gate(self) -> fairing::Gate { fairing::Gate::borrowed("b") } }
//! let other = element(B, egui::vec2(20.0, 20.0), |_, _| {});
//! let _ = ElementTree::row(A, 0.0, vec![other]);
//! ```
//!
//! Executable tree storage cannot be extracted from a protected registration:
//! ```compile_fail
//! fn extract<P: fairing::access::Permission>(tree: fairing::elements::ElementTree<P>) {
//!     let _ = tree.node;
//! }
//! ```

mod dispatch;
#[cfg(test)]
mod tests;

use crate::access::mediation::ElementHandle;
use crate::access::{Permission, PermissionBinding, PermissionBound};
use crate::screen::{Cx, Screen};
use egui::Vec2;

/// A custom widget/painter with a mandatory application-owned requirement.
///
/// Requirement metadata must be deterministic and side-effect-free. Measurement
/// and painting run only after live authorization. Paint may handle input using
/// ordinary egui widgets and retains the original app/backend context.
pub trait Element: PermissionBound {
    /// Measure admitted content. Non-finite or non-positive results omit the leaf.
    fn measure(&mut self, available: Vec2, cx: &mut Cx<'_>) -> Vec2;
    /// Paint admitted content in the rectangle assigned by the library.
    fn paint(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>);
}

#[derive(Clone, Copy)]
enum Axis {
    Row,
    Column,
}

enum Node<P: Permission> {
    Leaf(ElementHandle<P>),
    Group {
        axis: Axis,
        gap: f32,
        children: Vec<ElementTree<P>>,
    },
}

/// An owned tree whose child execution and geometry are managed by Fairing.
///
/// The root has a fixed registration requirement. Child requirements are read for
/// each plan and revalidated before paint. Plans are private and never reused as
/// authorization. A changed authority revision discards the remaining frame work.
pub struct ElementTree<P: Permission> {
    required: P,
    node: Node<P>,
}

impl<P: Permission> ElementTree<P> {
    /// Own a custom leaf. No measurement or painting occurs during construction.
    pub fn leaf(element: impl Element<Required = P> + 'static) -> Self {
        let element = ElementHandle::new(element);
        Self {
            required: element.requirement(),
            node: Node::Leaf(element),
        }
    }

    /// Place admitted children from left to right, with gaps only between them.
    ///
    /// # Errors
    /// Rejects a non-finite or negative gap.
    pub fn row(required: P, gap: f32, children: Vec<Self>) -> crate::Result<Self> {
        Self::group(required, Axis::Row, gap, children)
    }

    /// Place admitted children from top to bottom, with gaps only between them.
    ///
    /// # Errors
    /// Rejects a non-finite or negative gap.
    pub fn column(required: P, gap: f32, children: Vec<Self>) -> crate::Result<Self> {
        Self::group(required, Axis::Column, gap, children)
    }

    fn group(required: P, axis: Axis, gap: f32, children: Vec<Self>) -> crate::Result<Self> {
        if !gap.is_finite() || gap < 0.0 {
            return Err(crate::Error::Config(
                "element gap must be finite and nonnegative".into(),
            ));
        }
        Ok(Self {
            required,
            node: Node::Group {
                axis,
                gap,
                children,
            },
        })
    }
}

impl<P: Permission> PermissionBound for ElementTree<P> {
    type Required = P;
    fn required_permission(&self) -> P {
        self.required
    }
}

impl<P: Permission> Screen for ElementTree<P> {
    fn ui(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        let Some(stamp) = cx.authority_stamp() else {
            return;
        };
        let available = ui.available_size();
        let Some(mut plan) = self.prepare(available, cx, stamp) else {
            return;
        };
        if !plan.validate(cx, stamp) {
            ui.ctx().request_repaint();
            return;
        }
        let (rect, _) = ui.allocate_exact_size(plan.size, egui::Sense::hover());
        if !plan.paint(ui, cx, rect, stamp, ui.scope_id().with("fairing.elements")) {
            ui.ctx().request_repaint();
        }
    }
}

struct Plan<'tree, P: Permission> {
    binding: PermissionBinding<P>,
    size: Vec2,
    node: PlanNode<'tree, P>,
}
enum PlanNode<'tree, P: Permission> {
    Leaf(&'tree mut ElementHandle<P>),
    Group(Vec<ChildPlan<'tree, P>>),
}
struct ChildPlan<'tree, P: Permission> {
    index: usize,
    offset: Vec2,
    plan: Plan<'tree, P>,
}

struct PainterElement<P: Permission, F> {
    required: P,
    size: Vec2,
    paint: F,
}
impl<P: Permission, F> PermissionBound for PainterElement<P, F> {
    type Required = P;
    fn required_permission(&self) -> P {
        self.required
    }
}
impl<P: Permission, F: FnMut(&mut egui::Ui, &mut Cx<'_>)> Element for PainterElement<P, F> {
    fn measure(&mut self, available: Vec2, _: &mut Cx<'_>) -> Vec2 {
        if valid_size(self.size) {
            self.size.min(available)
        } else {
            Vec2::ZERO
        }
    }
    fn paint(&mut self, ui: &mut egui::Ui, cx: &mut Cx<'_>) {
        (self.paint)(ui, cx);
    }
}

/// Adapt a closure to the mandatory element contract using an explicit enum value.
/// The library owns when measurement/painting occur after registration.
pub fn element<P: Permission, F: FnMut(&mut egui::Ui, &mut Cx<'_>) + 'static>(
    required: P,
    size: Vec2,
    paint: F,
) -> ElementTree<P> {
    ElementTree::leaf(PainterElement {
        required,
        size,
        paint,
    })
}

fn valid_size(size: Vec2) -> bool {
    size.x.is_finite() && size.y.is_finite() && size.x > 0.0 && size.y > 0.0
}
