//! Exhaustive native and generated-history checks of the production lifecycle kernel.
#[path = "../../crates/fairing-widgets/src/interaction/rules.rs"]
mod rules;
use rules::{Effect as E, Operation as O, Snapshot as S};

fn promised(s: S, o: O) -> bool {
    match o {
        O::Observe | O::Cancel => true,
        O::Start => s.present && s.enabled,
        O::Continue | O::Finish => s.present && s.enabled && s.continuous && s.active,
    }
}

/// Every prefix can leave arbitrary owner/enable/lease predicates. Admission is
/// checked against the independent promise, never against implementation refusal.
pub fn contract(history: &[u8]) {
    let seen = u64::from_le_bytes(std::array::from_fn(|i| {
        history.get(i).copied().unwrap_or(0)
    }));
    let now = u64::from_le_bytes(std::array::from_fn(|i| {
        history.get(i + 8).copied().unwrap_or(0)
    }));
    let recent = seen <= now && now - seen <= 1;
    assert_eq!(rules::adjacent(seen, now), recent);
    assert_eq!(rules::live(seen, now, true, true), seen == now);
    let mut active = false;
    for &event in history.iter().take(128) {
        let s = S {
            present: event & 1 != 0,
            enabled: event & 2 != 0,
            continuous: event & 4 != 0,
            active,
        };
        let o = match (event >> 3) % 5 {
            0 => O::Observe,
            1 => O::Start,
            2 => O::Continue,
            3 => O::Finish,
            _ => O::Cancel,
        };
        let t = rules::step(s, o);
        assert_eq!(rules::permitted(s, o), promised(s, o));
        assert_eq!(t.effect != E::Refused, promised(s, o));
        active = t.active;
        // Probe the universal recovery suffix after EACH prefix, without changing history.
        let ready = S {
            present: true,
            enabled: true,
            ..s
        };
        let start = rules::step(ready, O::Start);
        assert_eq!(
            start.effect,
            E::Began,
            "prior state blocked a promised start"
        );
        let running = S {
            continuous: true,
            active: start.active,
            ..ready
        };
        assert_eq!(rules::step(running, O::Finish).effect, E::Completed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_raw_state_and_operation_has_the_promised_outcome() {
        for bits in 0u8..16 {
            let s = S {
                present: bits & 1 != 0,
                enabled: bits & 2 != 0,
                continuous: bits & 4 != 0,
                active: bits & 8 != 0,
            };
            for o in [O::Observe, O::Start, O::Continue, O::Finish, O::Cancel] {
                let t = rules::step(s, o);
                assert_eq!(t.effect != E::Refused, promised(s, o));
            }
        }
        for seen in [0, 1, u64::MAX - 1, u64::MAX] {
            for now in [0, 1, 2, u64::MAX - 1, u64::MAX] {
                assert_eq!(rules::adjacent(seen, now), seen <= now && now - seen <= 1);
                assert_eq!(rules::live(seen, now, true, true), seen == now);
            }
        }
        contract(&(0u8..=255).collect::<Vec<_>>());
    }

    #[cfg(not(miri))]
    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config { failure_persistence: None, ..Default::default() })]
        #[test]
        fn every_history_prefix_preserves_the_recovery_suffix(events in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..129)) {
            contract(&events);
        }
    }
}
