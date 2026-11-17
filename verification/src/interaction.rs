//! Native finite-state checks and generated histories for the production lifecycle kernel.
#[path = "../../crates/fairing-widgets/src/interaction/concrete.rs"]
mod concrete;
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
    concrete_contract(history);
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

/// Execute the concrete retained carriers against independent integer/phase oracles.
/// The payload may change after every prefix without changing input ownership.
pub fn concrete_contract(history: &[u8]) {
    use concrete::*;
    let mut capture: Option<Session<Capture<u64>>> = None;
    let mut hold: Option<Session<HoldPhase>> = None;
    for (index, &event) in history.iter().take(128).enumerate() {
        let p = Pointer {
            fresh: event & 1 != 0,
            down: event & 2 != 0,
            held: event & 4 != 0,
            clicked: event & 8 != 0,
            dragged: event & 16 != 0,
            stopped: event & 32 != 0,
        };
        let pass = Pass {
            now: index as u64,
            enabled: event & 64 == 0,
            lifetime: Lifetime::Mounted,
            fresh_press: p.fresh,
        };
        let valid = |seen: u64, press: bool| {
            pass.enabled
                && seen <= pass.now
                && pass.now - seen <= 1
                && !(press && p.fresh && seen != pass.now)
        };
        let active = capture
            .as_ref()
            .is_some_and(|s| valid(s.seen, false) && s.payload.captured);
        let start = p.fresh && (p.held || p.clicked);
        let finish = !p.fresh && (p.stopped || p.clicked);
        let continuing = !p.fresh && !finish && (p.held || p.dragged);
        let next = capture_step(restore_capture(capture.as_ref(), pass), pass.enabled, p);
        assert_eq!(
            next.admitted,
            pass.enabled && (start || active && (finish || continuing))
        );
        assert_eq!(
            next.captured,
            pass.enabled && p.down && (start || active && continuing)
        );
        let value = Capture {
            payload: u64::from(event),
            captured: active,
        };
        let applied = apply_capture(value, next);
        assert_eq!(
            applied.payload, value.payload,
            "capture transition changed animation/value data"
        );
        let phase = hold
            .as_ref()
            .filter(|s| valid(s.seen, true))
            .map(|s| s.payload);
        let restored_phase = restore(
            hold.as_ref(),
            Pass {
                lifetime: Lifetime::Press,
                ..pass
            },
        )
        .copied();
        assert_eq!(
            restored_phase, phase,
            "stored hold phase lost its lease correspondence"
        );
        let running = matches!(phase, Some(HoldPhase::Armed | HoldPhase::Done));
        let admitted = hold_admitted(restored_phase, pass.enabled, p);
        assert_eq!(admitted, pass.enabled && p.held && (p.fresh || running));
        let previous = phase.unwrap_or(HoldPhase::Idle);
        let full = event & 128 != 0;
        let completed = hold_finish(previous, admitted, full);
        assert_eq!(
            completed.completed,
            admitted && full && previous != HoldPhase::Done
        );
        assert_eq!(
            completed.phase,
            if !admitted {
                HoldPhase::Idle
            } else if previous == HoldPhase::Done || full {
                HoldPhase::Done
            } else {
                HoldPhase::Armed
            }
        );
        // A skipped drawing opportunity retains old storage without renewing its stamp.
        if event != 0 {
            capture = Some(Session {
                seen: pass.now,
                payload: applied,
            });
            hold = Some(Session {
                seen: pass.now,
                payload: completed.phase,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_carriers_cover_raw_pointer_combinations_and_animation_regressions() {
        use concrete::*;
        for pointer in 0u8..64 {
            concrete_contract(&[pointer, pointer | 128, pointer, 0, pointer | 64, pointer]);
        }
        let done = hold_finish(HoldPhase::Done, true, false);
        assert_eq!(done.phase, HoldPhase::Done);
        assert!(!hold_finish(done.phase, true, true).completed);
        let saved = Capture {
            payload: u64::MAX,
            captured: true,
        };
        for down in [false, true] {
            let p = Pointer {
                fresh: true,
                down,
                held: false,
                clicked: false,
                dragged: true,
                stopped: true,
            };
            assert_eq!(
                capture_step(Some(&saved), true, p),
                CaptureOutcome {
                    captured: false,
                    admitted: false
                }
            );
        }
    }

    #[test]
    fn restoring_actual_sessions_preserves_values_without_renewing_timestamps() {
        use concrete::*;
        for seen in [0, 1, 2, u64::MAX - 1, u64::MAX] {
            for now in [0, 1, 2, u64::MAX - 1, u64::MAX] {
                for lifetime in [Lifetime::Mounted, Lifetime::Press] {
                    for enabled in [false, true] {
                        for fresh_press in [false, true] {
                            let saved = Session {
                                seen,
                                payload: Capture {
                                    payload: u64::MAX,
                                    captured: true,
                                },
                            };
                            let p = Pass {
                                now,
                                enabled,
                                lifetime,
                                fresh_press,
                            };
                            let valid = enabled
                                && seen <= now
                                && now - seen <= 1
                                && !(lifetime == Lifetime::Press && fresh_press && seen != now);
                            assert_eq!(restore(Some(&saved), p).is_some(), valid);
                            assert_eq!(restore_capture(Some(&saved), p).is_some(), valid);
                            assert_eq!(saved.seen, seen);
                            assert_eq!(saved.payload.payload, u64::MAX);
                        }
                    }
                }
            }
        }
    }
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
