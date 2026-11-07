//! Plans borrow their actual opaque receivers; no index-to-body reconciliation.
use super::{valid_size, Axis, ChildPlan, ElementTree, Node, Plan, PlanNode};
use crate::access::{mediation::invoke_requirement, Permission, PermissionBinding, SessionId};
use crate::screen::Cx;
use egui::{Rect, Vec2};

type Stamp = (SessionId, u64);
impl<P: Permission> ElementTree<P> {
    pub(super) fn prepare<'tree>(
        &'tree mut self,
        available: Vec2,
        cx: &mut Cx<'_>,
        stamp: Stamp,
    ) -> Option<Plan<'tree, P>> {
        if cx.authority_stamp() != Some(stamp) {
            return None;
        }
        match &mut self.node {
            Node::Leaf(element) => {
                let (binding, size) = element.measure(available, cx, stamp)?;
                (valid_size(size) && cx.authority_stamp() == Some(stamp)).then_some(Plan {
                    binding,
                    size,
                    node: PlanNode::Leaf(element),
                })
            }
            Node::Group {
                axis,
                gap,
                children,
            } => {
                let binding = PermissionBinding::new(self.required);
                let (size, plans) = invoke_requirement(binding.gate(), stamp, cx, |cx| {
                    let mut plans = Vec::new();
                    let mut size = Vec2::ZERO;
                    for (index, child) in children.iter_mut().enumerate() {
                        let Some(plan) = child.prepare(available, cx, stamp) else {
                            continue;
                        };
                        let spacing = if plans.is_empty() { 0.0 } else { *gap };
                        let offset = match axis {
                            Axis::Row => {
                                let offset = egui::vec2(size.x + spacing, 0.0);
                                size.x = offset.x + plan.size.x;
                                size.y = size.y.max(plan.size.y);
                                offset
                            }
                            Axis::Column => {
                                let offset = egui::vec2(0.0, size.y + spacing);
                                size.y = offset.y + plan.size.y;
                                size.x = size.x.max(plan.size.x);
                                offset
                            }
                        };
                        plans.push(ChildPlan {
                            index,
                            offset,
                            plan,
                        });
                    }
                    (valid_size(size) && cx.authority_stamp() == Some(stamp))
                        .then_some((size, plans))
                })??;
                Some(Plan {
                    binding,
                    size,
                    node: PlanNode::Group(plans),
                })
            }
        }
    }
}
impl<P: Permission> Plan<'_, P> {
    pub(super) fn validate(&self, cx: &Cx<'_>, stamp: Stamp) -> bool {
        if cx.authority_stamp() != Some(stamp)
            || !self.binding.allows(cx)
            || cx.authority_stamp() != Some(stamp)
        {
            return false;
        }
        match &self.node {
            PlanNode::Leaf(element) => element.matches(&self.binding, cx, stamp),
            PlanNode::Group(children) => {
                children.iter().all(|child| child.plan.validate(cx, stamp))
                    && cx.authority_stamp() == Some(stamp)
            }
        }
    }
    pub(super) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        rect: Rect,
        stamp: Stamp,
        identity: egui::Id,
    ) -> bool {
        if !self.validate(cx, stamp) {
            return false;
        }
        match &mut self.node {
            PlanNode::Leaf(element) => element.paint(ui, cx, rect, identity, &self.binding, stamp),
            PlanNode::Group(children) => invoke_requirement(self.binding.gate(), stamp, cx, |cx| {
                for child in children {
                    let rect = Rect::from_min_size(rect.min + child.offset, child.plan.size);
                    if !child
                        .plan
                        .paint(ui, cx, rect, stamp, identity.with(child.index))
                    {
                        return false;
                    }
                }
                cx.authority_stamp() == Some(stamp)
            })
            .unwrap_or(false),
        }
    }
}
