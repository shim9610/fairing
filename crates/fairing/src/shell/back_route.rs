//! Total partition of the state read by the shell's Back dispatcher.
//! This covers dispatch priority, not egui hit testing or arbitrary screen callbacks.

#[allow(
    clippy::struct_excessive_bools,
    reason = "raw independent state predicates must be covered before grouping"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BackState {
    pub faulted: bool,
    pub prompt: bool,
    pub lock: bool,
    pub info: bool,
    pub overlay: bool,
    pub keyboard: bool,
    pub overview: bool,
    pub home: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackGroup {
    Faulted,
    Locked,
    Unlock,
    Info,
    Overlay,
    Keyboard,
    Overview,
    Home,
    Workspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BackTarget {
    Ignore,
    CancelPrompt,
    CloseInfo,
    CloseOverlay,
    HideKeyboard,
    CloseOverview,
    Workspace,
}

pub(crate) fn classify(s: BackState) -> BackGroup {
    if s.faulted {
        BackGroup::Faulted
    } else if s.prompt {
        if s.lock {
            BackGroup::Locked
        } else {
            BackGroup::Unlock
        }
    } else if s.info {
        BackGroup::Info
    } else if s.overlay {
        BackGroup::Overlay
    } else if s.keyboard {
        BackGroup::Keyboard
    } else if s.overview {
        BackGroup::Overview
    } else if s.home {
        BackGroup::Home
    } else {
        BackGroup::Workspace
    }
}

pub(crate) fn dispatch(group: BackGroup) -> BackTarget {
    match group {
        BackGroup::Faulted | BackGroup::Locked | BackGroup::Home => BackTarget::Ignore,
        BackGroup::Unlock => BackTarget::CancelPrompt,
        BackGroup::Info => BackTarget::CloseInfo,
        BackGroup::Overlay => BackTarget::CloseOverlay,
        BackGroup::Keyboard => BackTarget::HideKeyboard,
        BackGroup::Overview => BackTarget::CloseOverview,
        BackGroup::Workspace => BackTarget::Workspace,
    }
}
