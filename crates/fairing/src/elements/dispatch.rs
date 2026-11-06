//! The library-owned prepare/validate/paint traversal. No application-held permit.
use super::{valid_size, Axis, ChildPlan, ElementTree, Node, Plan};
use crate::access::{Permission, PermissionBinding, SessionId};
use crate::screen::Cx;
use egui::{Rect, Vec2};

type Stamp = (SessionId, u64);

impl<P: Permission> ElementTree<P> {
    fn current_permission(&self) -> P {
        match &self.node {
            Node::Leaf(element) => element.required_permission(),
            Node::Group { .. } => self.required,
        }
    }

    pub(super) fn prepare(
        &mut self,
        available: Vec2,
        cx: &mut Cx<'_>,
        stamp: Stamp,
    ) -> Option<Plan<P>> {
        if cx.authority_stamp() != Some(stamp) {
            return None;
        }
        let binding = PermissionBinding::new(self.current_permission());
        if !binding.allows(cx) || cx.authority_stamp() != Some(stamp) {
            return None;
        }
        let mut plans = Vec::new();
        let size = match &mut self.node {
            Node::Leaf(element) => binding.run(cx, |cx| element.measure(available, cx))?,
            Node::Group {
                axis,
                gap,
                children,
            } => {
                let mut size = Vec2::ZERO;
                for (index, child) in children.iter_mut().enumerate() {
                    if cx.authority_stamp() != Some(stamp) {
                        return None;
                    }
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
                size
            }
        };
        if !valid_size(size) || cx.authority_stamp() != Some(stamp) {
            return None;
        }
        Some(Plan {
            binding,
            size,
            children: plans,
        })
    }

    pub(super) fn validate(&self, plan: &Plan<P>, cx: &Cx<'_>, stamp: Stamp) -> bool {
        if cx.authority_stamp() != Some(stamp)
            || self.current_permission().gate() != *plan.binding.gate()
            || !plan.binding.allows(cx)
            || cx.authority_stamp() != Some(stamp)
        {
            return false;
        }
        match &self.node {
            Node::Leaf(_) => true,
            Node::Group { children, .. } => {
                plan.children.iter().all(|prepared| {
                    children
                        .get(prepared.index)
                        .is_some_and(|child| child.validate(&prepared.plan, cx, stamp))
                }) && cx.authority_stamp() == Some(stamp)
            }
        }
    }

    pub(super) fn paint_plan(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        plan: &Plan<P>,
        rect: Rect,
        stamp: Stamp,
        path: egui::Id,
    ) -> bool {
        if !self.validate(plan, cx, stamp) {
            return false;
        }
        match &mut self.node {
            Node::Leaf(element) => plan
                .binding
                .run(cx, |cx| {
                    if cx.authority_stamp() != Some(stamp)
                        || element.required_permission().gate() != *plan.binding.gate()
                        || cx.authority_stamp() != Some(stamp)
                    {
                        return false;
                    }
                    let mut child_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt(path.with(plan.binding.gate().as_str()).value())
                            .max_rect(rect),
                    );
                    child_ui.set_clip_rect(ui.clip_rect().intersect(rect));
                    element.paint(&mut child_ui, cx);
                    cx.authority_stamp() == Some(stamp)
                })
                .unwrap_or(false),
            Node::Group { children, .. } => {
                for prepared in &plan.children {
                    let Some(child) = children.get_mut(prepared.index) else {
                        return false;
                    };
                    let child_rect =
                        Rect::from_min_size(rect.min + prepared.offset, prepared.plan.size);
                    if !child.paint_plan(
                        ui,
                        cx,
                        &prepared.plan,
                        child_rect,
                        stamp,
                        path.with(prepared.index),
                    ) {
                        return false;
                    }
                }
                cx.authority_stamp() == Some(stamp)
            }
        }
    }
}
