#[path = "../crates/fairing-widgets/src/memory.rs"]
mod memory;

#[kani::proof]
fn generations_do_not_wrap_into_a_stale_handle() {
    let before: u64 = kani::any();
    match memory::next_generation(before) {
        Some(after) => {
            assert!(after > before);
            assert_eq!(after, before + 1);
        }
        None => assert_eq!(before, u64::MAX),
    }
}

#[kani::proof]
#[kani::unwind(16)]
fn bounded_pool_operations_preserve_size_and_reject_removed_and_foreign_keys() {
    let mut pool = memory::FixedPool::<u8, 2>::new().unwrap();
    let mut other = memory::FixedPool::<u8, 2>::new().unwrap();
    let foreign = other.try_insert(99).unwrap();
    assert!(pool.get(foreign).is_none());
    let mut key = None;
    let mut stale = None;
    for _ in 0..4 {
        let operation: u8 = kani::any();
        match operation % 3 {
            0 => {
                let value: u8 = kani::any();
                if let Ok(inserted) = pool.try_insert(value) {
                    assert_eq!(pool.get(inserted), Some(&value));
                    key = Some(inserted);
                }
            }
            1 => {
                if let Some(k) = key.take() {
                    let _ = pool.remove(k);
                    stale = Some(k);
                }
            }
            _ => pool.clear(),
        }
        assert!(pool.len() <= 2);
        assert_eq!(pool.len() + pool.available(), 2);
        if let Some(old) = stale {
            assert!(pool.get(old).is_none());
        }
    }
}

#[kani::proof]
#[kani::unwind(32)]
fn fixed_display_retains_latest_data_without_overflow_storage() {
    let samples: [u8; 4] = kani::any();
    let mut view = memory::DisplayWindow::<u8, 2>::new();
    if kani::any() {
        let status = view.extend(&samples);
        assert_eq!(status.discarded, 2);
    } else {
        for value in samples {
            view.push(value);
        }
    }
    assert_eq!(view.len(), 2);
    let mut all = view.recent(usize::MAX);
    assert_eq!(all.next(), samples.get(2));
    assert_eq!(all.next(), samples.get(3));
    assert_eq!(all.next(), None);
    assert_eq!(view.recent(1).next(), samples.get(3));
    assert_eq!(view.view_status(1).discarded, 1);
    let mut zero = memory::DisplayWindow::<u8, 0>::new();
    assert_eq!(zero.extend(&samples).discarded, 4);
    assert_eq!(zero.recent(usize::MAX).next(), None);
}
