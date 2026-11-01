//! Recoverable display admission errors. Error formatting never includes payloads.
use std::fmt;

/// Maximum waiting toasts, excluding the configured visible slots.
pub const DEFAULT_MAX_PENDING_TOASTS: usize = 64;
/// Maximum retained toast text bytes (including spare String capacity).
pub const MAX_TOAST_TEXT_BYTES: usize = 4096;
/// Maximum retained bytes in all notification strings, including spare capacity.
pub const MAX_NOTIFICATION_TEXT_BYTES: usize = 16_384;

/// A new display item could not be admitted; existing work remains available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionError {
    /// All retention slots are occupied by protected items, or the waiting queue is full.
    Full,
    /// Payload exceeds the display's fixed text budget.
    TooLarge,
    /// Fallible storage growth failed. Retry later or omit this display item.
    AllocationFailed,
}
impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Full => "display capacity exhausted",
            Self::TooLarge => "display payload exceeds byte budget",
            Self::AllocationFailed => "display storage unavailable",
        })
    }
}
impl std::error::Error for AdmissionError {}

/// A rejection preserving ownership without allocating an error wrapper.
pub struct Rejected<T> {
    pub(crate) kind: AdmissionError,
    pub(crate) value: T,
}
impl<T> Rejected<T> {
    /// Redacted failure reason.
    #[must_use]
    pub const fn kind(&self) -> AdmissionError {
        self.kind
    }
    /// Recover the item for the caller's bounded retry or discard policy.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}
impl<T> fmt::Debug for Rejected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.kind, f)
    }
}
impl<T> fmt::Display for Rejected<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.kind, f)
    }
}
impl<T> std::error::Error for Rejected<T> {}

pub(crate) fn icon_bytes(icon: Option<&crate::icons::IconRef>) -> usize {
    match icon {
        Some(crate::icons::IconRef::Glyph(text)) => text.capacity(),
        Some(
            crate::icons::IconRef::Builtin(_)
            | crate::icons::IconRef::Custom(_)
            | crate::icons::IconRef::Texture { .. },
        )
        | None => 0,
        Some(_) => usize::MAX,
    }
}
#[allow(
    clippy::ptr_arg,
    reason = "owned spare capacity is part of the byte charge; str hides it"
)]
fn cow_bytes(text: &std::borrow::Cow<'static, str>) -> usize {
    match text {
        std::borrow::Cow::Borrowed(text) => text.len(),
        std::borrow::Cow::Owned(text) => text.capacity(),
    }
}
pub(crate) fn notification_bytes(item: &super::Notification) -> usize {
    use crate::LaunchAction;
    let action = match item.action.as_ref() {
        Some(LaunchAction::Open { id, .. } | LaunchAction::Run(id)) => id.capacity(),
        Some(LaunchAction::Set(key, value)) => cow_bytes(&key.0).saturating_add(match value {
            crate::SettingValue::Text(text) => text.capacity(),
            crate::SettingValue::Bool(_)
            | crate::SettingValue::Int(_)
            | crate::SettingValue::Float(_) => 0,
        }),
        Some(LaunchAction::Toggle(key)) => cow_bytes(&key.0),
        Some(
            LaunchAction::Lock
            | LaunchAction::Logout
            | LaunchAction::OpenOverlay
            | LaunchAction::OpenOverview
            | LaunchAction::ToggleSplit,
        )
        | None => 0,
    };
    item.title
        .capacity()
        .saturating_add(item.body.capacity())
        .saturating_add(item.source.capacity())
        .saturating_add(icon_bytes(item.icon.as_ref()))
        .saturating_add(action)
        .saturating_add(item.gate.as_ref().map_or(0, |gate| cow_bytes(&gate.0)))
}
