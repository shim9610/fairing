//! Executable callback admission, shared verbatim with the model checker.

pub(crate) fn admitted(token_mode: bool, allowed: bool) -> bool {
    token_mode && allowed
}

pub(crate) fn invoke<R>(allowed: bool, callback: impl FnOnce() -> R) -> Option<R> {
    if allowed {
        Some(ExecutionToken { callback }.execute())
    } else {
        None
    }
}

/// One callback attempt, bound to its exact operation. No constructor or payload loan.
/// This is not Clone/Copy and cannot be executed by borrowing it.
pub(crate) struct ExecutionToken<F> {
    callback: F,
}
impl<F> ExecutionToken<F> {
    pub(crate) fn execute<R>(self) -> R
    where
        F: FnOnce() -> R,
    {
        let Self { callback } = self;
        callback()
    }
}
