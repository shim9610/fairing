//! Runtime unwind containment. This cannot intercept allocator aborts or panic=abort.

use std::sync::atomic::{AtomicUsize, Ordering};

static CAUGHT: AtomicUsize = AtomicUsize::new(0);
static CLEANUP_PANICS: AtomicUsize = AtomicUsize::new(0);
static RETAINED: AtomicUsize = AtomicUsize::new(0);
const CLEANUP_ATTEMPTS: usize = 8;

#[allow(deprecated, reason = "fetch_update supports the declared MSRV")]
fn increment(counter: &AtomicUsize) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
        Some(n.saturating_add(1))
    });
}

pub(crate) fn panic_cleanup_stats() -> crate::recovery::PanicCleanupStats {
    crate::recovery::PanicCleanupStats {
        caught: CAUGHT.load(Ordering::Relaxed),
        cleanup_panics: CLEANUP_PANICS.load(Ordering::Relaxed),
        retained: RETAINED.load(Ordering::Relaxed),
    }
}

/// Contain an unwind and dispose of its payload without formatting or logging it.
pub(crate) fn contain(work: impl FnOnce()) -> bool {
    attempt(work).is_ok()
}

/// Drop a payload after the original unwind has finished. A destructor which unwinds
/// produces another payload; dispose of that too, with a finite work limit. This cannot
/// contain destructors which abort or panic twice during the same unwind.
fn dispose_payload(mut payload: Box<dyn std::any::Any + Send>) {
    for _ in 0..CLEANUP_ATTEMPTS {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload))) {
            Ok(()) => return,
            Err(next) => {
                increment(&CLEANUP_PANICS);
                payload = next;
            }
        }
    }
    // An arbitrary recursively panicking destructor has no universally safe disposal.
    // Retain only the final payload and make this fallback observable to the host.
    increment(&RETAINED);
    std::mem::forget(payload);
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
        reason = "payload destructor fault injection"
    )]
    fn arbitrary_payload_is_released() {
        struct Payload(Arc<AtomicUsize>);
        impl Drop for Payload {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let owned = Arc::clone(&drops);
        assert!(!super::contain(|| std::panic::panic_any(Payload(owned))));
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert_eq!(Arc::strong_count(&drops), 1);
    }

    #[test]
    #[allow(
        clippy::panic,
        clippy::disallowed_methods,
        reason = "payload destructor fault injection"
    )]
    fn a_panicking_destructor_is_contained_and_its_replacement_is_released() {
        struct BadDrop;
        impl Drop for BadDrop {
            fn drop(&mut self) {
                panic!("payload destructor fault");
            }
        }
        assert!(!super::contain(|| std::panic::panic_any(BadDrop)));
        assert!(super::contain(|| {}));
    }

    #[test]
    #[allow(
        clippy::disallowed_methods,
        clippy::panic,
        reason = "recursive payload fault injection"
    )]
    fn recursively_panicking_payload_has_a_finite_cleanup_budget() {
        struct Recursive(Arc<AtomicUsize>);
        impl Drop for Recursive {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
                std::panic::panic_any(Self(Arc::clone(&self.0)));
            }
        }
        let drops = Arc::new(AtomicUsize::new(0));
        let owned = Arc::clone(&drops);
        let before = super::panic_cleanup_stats().retained;
        assert!(!super::contain(|| std::panic::panic_any(Recursive(owned))));
        assert_eq!(drops.load(Ordering::Relaxed), super::CLEANUP_ATTEMPTS);
        assert!(super::panic_cleanup_stats().retained > before);
        assert!(super::contain(|| {}));
    }
}
