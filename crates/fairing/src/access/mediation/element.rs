//! Opaque element storage; inherited requirements come from the calling context.
use super::invoke_requirement;
use crate::access::{Permission, PermissionBinding, SessionId};
use crate::elements::Element;
use crate::screen::Cx;
use egui::{Rect, Vec2};

type Stamp = (SessionId, u64);
pub(crate) struct ElementHandle<P: Permission> {
    body: Box<dyn Element<Required = P>>,
}
impl<P: Permission> ElementHandle<P> {
    pub(crate) fn new(body: impl Element<Required = P> + 'static) -> Self {
        Self {
            body: Box::new(body),
        }
    }
    pub(crate) fn requirement(&self) -> P {
        self.body.required_permission()
    }
    pub(crate) fn measure(
        &mut self,
        available: Vec2,
        cx: &mut Cx<'_>,
        stamp: Stamp,
    ) -> Option<(PermissionBinding<P>, Vec2)> {
        let binding = PermissionBinding::new(self.requirement());
        let size = invoke_requirement(binding.gate(), stamp, cx, |cx| {
            self.body.measure(available, cx)
        })?;
        Some((binding, size))
    }
    pub(crate) fn matches(
        &self,
        binding: &PermissionBinding<P>,
        cx: &Cx<'_>,
        stamp: Stamp,
    ) -> bool {
        cx.authority_stamp() == Some(stamp)
            && self.requirement().gate() == *binding.gate()
            && binding.allows(cx)
            && cx.authority_stamp() == Some(stamp)
    }
    pub(crate) fn paint(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut Cx<'_>,
        rect: Rect,
        identity: egui::Id,
        binding: &PermissionBinding<P>,
        stamp: Stamp,
    ) -> bool {
        // Read metadata before the final entry check; a getter cannot revoke and
        // then rely on an earlier admission. Never lend the raw body to callers.
        if self.requirement().gate() != *binding.gate() {
            return false;
        }
        invoke_requirement(binding.gate(), stamp, cx, |cx| {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(identity.with(binding.gate().as_str()).value())
                    .max_rect(rect),
            );
            child.set_clip_rect(ui.clip_rect().intersect(rect));
            self.body.paint(&mut child, cx);
            cx.authority_stamp() == Some(stamp)
        })
        .unwrap_or(false)
    }
}
