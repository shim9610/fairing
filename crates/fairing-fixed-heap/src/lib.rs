//! Explicit process-wide fixed-arena allocation, with no system-heap fallback.
//!
//! Enable `global` directly or `fairing/fixed-heap` in the device binary.
//! `FAIRING_HEAP_BYTES` chooses 128 KiB..=1 GiB at build time (default 512 MiB
//! for parallel development tests). Every Rust allocation routed through the
//! selected global allocator uses this one arena, including Vec/String/Box in
//! fairing and egui. Native C, driver, GPU and some runtime allocations are outside
//! that contract. Infallible allocations can abort on exhaustion; this does not
//! turn an arbitrary allocating UI into a fallible or allocation-free program.
//!
//! The allocation algorithm is Talc; synchronization is `spinning_top`. The only
//! local unsafe operation gives Talc exclusive ownership of static arena memory.

#![deny(unsafe_code)]
#![forbid(unsafe_op_in_unsafe_fn)]

include!(concat!(env!("OUT_DIR"), "/heap_size.rs"));

/// A point-in-time snapshot of the fixed arena's allocation counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapStats {
    /// Static arena size.
    pub capacity: usize,
    /// Bytes claimed from that arena, including allocator metadata.
    pub claimed_bytes: usize,
    /// Bytes requested by currently live allocations.
    pub allocated_bytes: usize,
    /// Allocator-reported free bytes; fragmentation can limit an individual request.
    pub available_bytes: usize,
    /// Currently live allocation count.
    pub live_allocations: usize,
    /// Total allocations observed by Talc's counters.
    pub total_allocations: u64,
}

/// Read counters if the allocator is not currently busy, otherwise return None.
/// This does not allocate, wait for a lock or reserve memory.
#[cfg(feature = "global")]
#[must_use]
pub fn try_stats() -> Option<HeapStats> {
    let allocator = ALLOCATOR.try_lock()?;
    let c = allocator.counters();
    Some(HeapStats {
        capacity: HEAP_BYTES,
        claimed_bytes: c.claimed_bytes,
        allocated_bytes: c.allocated_bytes,
        available_bytes: c.available_bytes,
        live_allocations: c.allocation_count,
        total_allocations: c.total_allocation_count,
    })
}

#[cfg(feature = "global")]
#[expect(
    unsafe_code,
    reason = "unique static arena ownership transferred once to the upstream allocator"
)]
#[global_allocator]
static ALLOCATOR: talc::TalcLock<spinning_top::RawSpinlock, talc::source::Claim> =
    talc::TalcLock::new(unsafe {
        // SAFETY: this allocation has static lifetime; no reference to its contents
        // is formed or exposed, it is never used by another allocator, resized,
        // reclaimed or accessed outside Talc. Claim is the only memory source,
        // so exhaustion cannot grow the arena or fall back to System.
        static mut ARENA: [u8; HEAP_BYTES] = [0; HEAP_BYTES];
        talc::source::Claim::array(&raw mut ARENA)
    });
