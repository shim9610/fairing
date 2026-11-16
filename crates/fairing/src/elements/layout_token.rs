//! Reusable authorization for immutable layout data within one frame observation.
//! It owns no executable element and cannot dispatch a callback.
use crate::access::{Permission, PermissionBinding, SessionId};
use crate::screen::Cx;
use std::time::Instant;

pub(super) struct LayoutToken<P: Permission> {
    binding: PermissionBinding<P>,
    stamp: (SessionId, u64),
    now: Instant,
    frame: u64,
    cacheable: bool,
}
impl<P: Permission> LayoutToken<P> {
    pub(super) fn new(
        binding: PermissionBinding<P>,
        stamp: (SessionId, u64),
        cx: &Cx<'_>,
    ) -> Option<Self> {
        if cx.authority_stamp() != Some(stamp)
            || !binding.allows(cx)
            || cx.authority_stamp() != Some(stamp)
        {
            return None;
        }
        Some(Self {
            binding,
            stamp,
            now: cx.now,
            frame: cx.layout_frame(),
            cacheable: cx.layout_permissions_cacheable(),
        })
    }
    pub(super) fn valid(&self, cx: &Cx<'_>) -> bool {
        self.frame == cx.layout_frame()
            && self.now == cx.now
            && cx.authority_stamp() == Some(self.stamp)
            && (self.cacheable || self.binding.allows(cx))
            && cx.authority_stamp() == Some(self.stamp)
    }
    pub(super) fn gate(&self) -> &crate::Gate {
        self.binding.gate()
    }
}
