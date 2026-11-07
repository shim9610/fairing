//! Executable callback admission, shared verbatim with the model checker.

pub(crate) fn admitted(token_mode: bool, allowed: bool) -> bool {
    token_mode && allowed
}

pub(crate) fn invoke<R>(allowed: bool, callback: impl FnOnce() -> R) -> Option<R> {
    if allowed {
        Some(callback())
    } else {
        None
    }
}
