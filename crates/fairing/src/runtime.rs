//! Runtime unwind containment. This cannot intercept allocator aborts or panic=abort.

/// Do not format/log a panic payload. Drop only known standard string payloads;
/// arbitrary user payload destructors may panic again.
/// Unknown payloads remain retained across developer-requested replacement/retries.
pub(crate) fn contain(work: impl FnOnce()) -> bool {
    attempt(work).is_ok()
}

/// Return a value without exposing an untrusted panic payload to callers.
pub(crate) fn attempt<T>(work: impl FnOnce() -> T) -> Result<T, ()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            if payload.is::<String>() || payload.is::<&'static str>() {
                drop(payload);
            } else {
                std::mem::forget(payload);
            }
            Err(())
        }
    }
}

/// Erase partial visual/platform output. Avoid text, callbacks, and a new warning allocation.
/// Hosts can display a prepared fault surface after observing `Shell::is_faulted`.
pub(crate) fn blank_faulted_frame(ctx: &egui::Context) {
    let _ = contain(|| {
        // Revoke platform commands before dropping paint callbacks: a user-owned callback
        // payload can panic in Drop. Both outputs must already be inaccessible then.
        let output = ctx.output_mut(std::mem::take);
        let shapes = ctx.graphics_mut(std::mem::take);
        drop(output);
        drop(shapes);
    });
}

/// Loan restoration must precede propagation to the enclosing runtime boundary.
#[allow(
    clippy::disallowed_methods,
    reason = "propagate a caught callback unwind only after restoring its loan; shell/runner boundaries quarantine it"
)]
pub(crate) fn propagate<T>(result: std::thread::Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[allow(
        clippy::panic,
        clippy::disallowed_methods,
        reason = "untrusted panic payload destructor fault injection"
    )]
    fn untrusted_panic_payload_is_never_dropped_during_recovery() {
        struct BadDrop;
        impl Drop for BadDrop {
            fn drop(&mut self) {
                panic!("payload destructor must not run");
            }
        }
        assert!(!super::contain(|| std::panic::panic_any(BadDrop)));
        assert!(super::contain(|| {}));
    }
}
