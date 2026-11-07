//! Runtime unwind containment. This cannot intercept allocator aborts or panic=abort.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

static CAUGHT: AtomicUsize = AtomicUsize::new(0);
static RELEASED: AtomicUsize = AtomicUsize::new(0);
static LAST_CODE: AtomicU32 = AtomicU32::new(0);
static RETAINED: AtomicUsize = AtomicUsize::new(0);

#[allow(deprecated, reason = "fetch_update supports the declared MSRV")]
fn increment(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_add(1))
    });
}

pub(crate) fn panic_cleanup_stats() -> crate::recovery::PanicCleanupStats {
    crate::recovery::PanicCleanupStats {
        caught: CAUGHT.load(Ordering::Relaxed),
        released: RELEASED.load(Ordering::Relaxed),
        last_code: match LAST_CODE.load(Ordering::Relaxed) {
            0 => None,
            encoded => u16::try_from(encoded - 1)
                .ok()
                .map(crate::recovery::PanicCode),
        },
        retained: RETAINED.load(Ordering::Relaxed),
    }
}

/// Contain an unwind and dispose of its payload without formatting or logging it.
pub(crate) fn contain(work: impl FnOnce()) -> bool {
    attempt(work).is_ok()
}

/// Only exact, library-approved types may run drop glue during payload cleanup.
/// In particular, do not call an unknown destructor even under `catch_unwind`: it can
/// abort, cause a double panic, allocate, block or create another arbitrary payload.
fn dispose_payload(payload: Box<dyn std::any::Any + Send>) {
    if let Some(code) = payload.downcast_ref::<crate::recovery::PanicCode>() {
        LAST_CODE.store(u32::from(code.0) + 1, Ordering::Relaxed);
    }
    if payload.is::<String>()
        || payload.is::<&'static str>()
        || payload.is::<crate::recovery::PanicCode>()
    {
        drop(payload);
        increment(&RELEASED);
    } else {
        // No portable, safe way exists to release an arbitrary object's owned
        // resources without executing its destructor. Record the contract violation
        // without formatting, retaining a callable handle, or running that code.
        increment(&RETAINED);
        std::mem::forget(payload);
    }
}

/// Return a value without exposing a panic payload to callers.
pub(crate) fn attempt<T>(work: impl FnOnce() -> T) -> Result<T, ()> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
        Ok(value) => Ok(value),
        Err(payload) => {
            increment(&CAUGHT);
            dispose_payload(payload);
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
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[test]
    #[allow(
        clippy::panic,
        clippy::disallowed_methods,
        reason = "supported payload fault injection"
    )]
    fn supported_payloads_are_released_and_numeric_codes_are_observable() {
        let before = super::panic_cleanup_stats().released;
        assert!(!super::contain(|| std::panic::panic_any("static message")));
        assert!(!super::contain(|| std::panic::panic_any(String::from(
            "owned message"
        ))));
        for code in [0, 7, u16::MAX] {
            assert!(!super::contain(|| std::panic::panic_any(
                crate::recovery::PanicCode(code)
            )));
            assert_eq!(
                super::panic_cleanup_stats().last_code,
                Some(crate::recovery::PanicCode(code))
            );
        }
        assert!(super::panic_cleanup_stats().released >= before + 5);
        assert!(super::contain(|| {}));
    }

    #[test]
    #[allow(
        clippy::panic,
        clippy::disallowed_methods,
        reason = "unknown payload destructor fault injection"
    )]
    fn unknown_payload_destructor_is_never_invoked() {
        struct Payload(Arc<AtomicUsize>);
        impl Drop for Payload {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
                panic!("unknown destructor must not execute");
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let owned = Arc::clone(&drops);
        let before = super::panic_cleanup_stats().retained;
        assert!(!super::contain(|| std::panic::panic_any(Payload(owned))));
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        assert!(super::panic_cleanup_stats().retained > before);
        assert!(super::contain(|| {}));
    }
}
