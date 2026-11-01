//! Inline fixed-capacity object storage. The container itself never allocates.
//!
//! Stored values may own their own allocations; a `String` in this pool still owns
//! its character buffer. Choose inline payloads or the application's fixed global
//! heap when those allocations also need a hard bound.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_POOL: AtomicUsize = AtomicUsize::new(1);

pub(crate) const fn next_generation(current: u64) -> Option<u64> {
    current.checked_add(1)
}

/// Pool identity space is exhausted. Identities are never recycled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolIdentityExhausted;
impl fmt::Display for PoolIdentityExhausted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("fixed pool identity space exhausted")
    }
}
impl std::error::Error for PoolIdentityExhausted {}

/// A pool-specific generational handle. Moves of the originating pool preserve it.
/// Handles from another pool or a removed/replaced object cannot resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolKey {
    pool: usize,
    index: usize,
    generation: u64,
}

struct Slot<T> {
    generation: u64,
    value: Option<T>,
}

/// N inline slots, no heap fallback and no generation wrap.
/// A slot whose generation reaches `u64::MAX` is retired after removal.
///
/// ```
/// use fairing_widgets::memory::FixedPool;
/// let mut pool = FixedPool::<u32, 2>::new()?;
/// let key = pool.try_insert(42).map_err(|_| "pool full")?;
/// assert_eq!(pool.get(key), Some(&42));
/// assert_eq!(pool.remove(key), Some(42));
/// assert!(pool.get(key).is_none());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct FixedPool<T, const N: usize> {
    id: usize,
    slots: [Slot<T>; N],
    len: usize,
}
impl<T, const N: usize> FixedPool<T, N> {
    /// Obtain an identity and initialize N inline slots. No allocation is performed.
    ///
    /// # Errors
    /// All process-wide pool identities have been consumed.
    pub fn new() -> Result<Self, PoolIdentityExhausted> {
        let mut id = NEXT_POOL.load(Ordering::Relaxed);
        loop {
            let next = id.checked_add(1).ok_or(PoolIdentityExhausted)?;
            match NEXT_POOL.compare_exchange(id, next, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => break,
                Err(actual) => id = actual,
            }
        }
        Ok(Self {
            id,
            slots: [const {
                Slot {
                    generation: 0,
                    value: None,
                }
            }; N],
            len: 0,
        })
    }

    /// Insert without allocation or eviction; return the value if no slot remains.
    ///
    /// # Errors
    /// Full or all free slots have retired generations. The error is the original value.
    pub fn try_insert(&mut self, value: T) -> Result<PoolKey, T> {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.value.is_none() {
                if let Some(next) = next_generation(slot.generation) {
                    slot.generation = next;
                    slot.value = Some(value);
                    self.len += 1;
                    return Ok(PoolKey {
                        pool: self.id,
                        index,
                        generation: next,
                    });
                }
            }
        }
        Err(value)
    }

    /// Resolve a live handle belonging to this pool.
    #[must_use]
    pub fn get(&self, key: PoolKey) -> Option<&T> {
        if key.pool != self.id {
            return None;
        }
        self.slots
            .get(key.index)
            .filter(|s| s.generation == key.generation)?
            .value
            .as_ref()
    }
    /// Resolve a live handle mutably.
    pub fn get_mut(&mut self, key: PoolKey) -> Option<&mut T> {
        if key.pool != self.id {
            return None;
        }
        self.slots
            .get_mut(key.index)
            .filter(|s| s.generation == key.generation)?
            .value
            .as_mut()
    }
    /// Remove an object and return its ownership. Old handles remain invalid.
    pub fn remove(&mut self, key: PoolKey) -> Option<T> {
        if key.pool != self.id {
            return None;
        }
        let slot = self
            .slots
            .get_mut(key.index)
            .filter(|s| s.generation == key.generation)?;
        let value = slot.value.take()?;
        self.len -= 1;
        Some(value)
    }
    /// Drop all live values. Generations and retired slots are retained.
    pub fn clear(&mut self) {
        for slot in &mut self.slots {
            if let Some(value) = slot.value.take() {
                self.len -= 1;
                drop(value);
            }
        }
    }
    /// Number of live objects.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether there are no live objects.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Compile-time slot count, including retired slots.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }
    /// Free slots that can still receive another generation.
    #[must_use]
    pub fn available(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.value.is_none() && s.generation != u64::MAX)
            .count()
    }
}

impl<T, const N: usize> fmt::Debug for FixedPool<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixedPool")
            .field("capacity", &N)
            .field("len", &self.len)
            .field("available", &self.available())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test fixture operations are assertions")]
mod tests {
    use super::FixedPool;
    #[test]
    fn clearing_preserves_counts_if_a_payload_destructor_unwinds() {
        #[derive(Debug)]
        struct Value(bool);
        impl Drop for Value {
            fn drop(&mut self) {
                assert!(!self.0, "synthetic payload destructor failure");
            }
        }
        let mut pool = FixedPool::<Value, 2>::new().unwrap();
        let first = pool.try_insert(Value(true)).unwrap();
        let second = pool.try_insert(Value(false)).unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pool.clear())).is_err());
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.available(), 1);
        assert!(pool.get(first).is_none());
        assert!(pool.remove(second).is_some());
        assert_eq!(pool.len(), 0);
    }
    #[test]
    fn handles_survive_moves_but_never_cross_pools_or_replaced_objects() {
        let mut a = FixedPool::<u8, 1>::new().unwrap();
        let mut b = FixedPool::<u8, 1>::new().unwrap();
        let first = a.try_insert(7).unwrap();
        let foreign = b.try_insert(9).unwrap();
        assert!(a.get(foreign).is_none());
        assert!(b.get(first).is_none());
        let mut a = std::hint::black_box(a);
        assert_eq!(a.get(first), Some(&7));
        assert_eq!(a.remove(first), Some(7));
        let second = a.try_insert(8).unwrap();
        assert!(a.get(first).is_none());
        assert_eq!(a.get(second), Some(&8));
        assert_eq!(a.try_insert(99), Err(99));
        a.clear();
        assert!(a.get(second).is_none());
        assert_eq!(a.len(), 0);
        assert_eq!(a.available(), 1);
    }
    #[test]
    fn the_last_generation_is_retired_instead_of_wrapping() {
        let mut pool = FixedPool::<u8, 1>::new().unwrap();
        pool.slots.first_mut().unwrap().generation = u64::MAX - 1;
        let key = pool.try_insert(1).unwrap();
        assert_eq!(pool.remove(key), Some(1));
        assert_eq!(pool.available(), 0);
        assert_eq!(pool.try_insert(2), Err(2));
        assert!(pool.get(key).is_none());
    }
    #[test]
    fn a_zero_capacity_pool_preserves_rejected_values() {
        let mut pool = FixedPool::<String, 0>::new().unwrap();
        assert_eq!(pool.try_insert("owned".into()), Err("owned".into()));
        assert!(pool.is_empty());
    }
}
