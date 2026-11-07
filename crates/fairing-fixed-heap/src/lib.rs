//! Diagnostic fixed-arena allocator, unpublished and isolated from product libraries.
//!
//! This is a failure-injection tool, not a supported production recovery policy.
//! Enable `global` only in an explicitly linked diagnostic executable. Infallible
//! Rust/egui allocation can abort; the product fairing/fixed-heap feature was withdrawn.
//! `FAIRING_HEAP_BYTES` chooses 128 KiB..=1 GiB (default 512 MiB for development).
//! Native C, stacks, OS/driver/GPU and explicitly different allocators are excluded.
//! Talc provides allocation and `spinning_top` synchronization; one local unsafe
//! block transfers exclusive ownership of the static arena to the upstream allocator.

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
