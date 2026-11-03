#[path = "../crates/fairing-widgets/src/interaction/concrete.rs"]
mod concrete;
#[path = "../crates/fairing-widgets/src/interaction/rules.rs"]
mod rules;
use concrete::*;

fn any_pointer() -> Pointer {
    Pointer {
        fresh: kani::any(),
        down: kani::any(),
        held: kani::any(),
        clicked: kani::any(),
        dragged: kani::any(),
        stopped: kani::any(),
    }
}
fn any_pass() -> Pass {
    Pass {
        now: kani::any(),
        enabled: kani::any(),
        lifetime: if kani::any() {
            Lifetime::Mounted
        } else {
            Lifetime::Press
        },
        fresh_press: kani::any(),
    }
}
fn any_capture() -> Option<Session<Capture<u64>>> {
    if kani::any() {
        Some(Session {
            seen: kani::any(),
            payload: Capture {
                payload: kani::any(),
                captured: kani::any(),
            },
        })
    } else {
        None
    }
}
fn any_hold() -> Option<HoldPhase> {
    match kani::any::<u8>() % 4 {
        0 => None,
        1 => Some(HoldPhase::Idle),
        2 => Some(HoldPhase::Armed),
        _ => Some(HoldPhase::Done),
    }
}
// Independent arithmetic specification: no use of lease_group/restore/adjacent.
fn live<T>(s: Option<&Session<T>>, p: Pass) -> bool {
    p.enabled
        && s.is_some_and(|s| {
            s.seen <= p.now
                && p.now - s.seen <= 1
                && !(p.lifetime == Lifetime::Press && p.fresh_press && s.seen != p.now)
        })
}
fn lease_member<T>(s: Option<&Session<T>>, p: Pass, g: LeaseGroup) -> bool {
    match g {
        LeaseGroup::Disabled => !p.enabled,
        LeaseGroup::Missing => p.enabled && s.is_none(),
        LeaseGroup::Future => p.enabled && s.is_some_and(|s| s.seen > p.now),
        LeaseGroup::Expired => {
            p.enabled && s.is_some_and(|s| s.seen <= p.now && p.now - s.seen > 1)
        }
        LeaseGroup::ReplacedPress => {
            p.enabled
                && s.is_some_and(|s| {
                    s.seen < p.now
                        && p.now - s.seen == 1
                        && p.lifetime == Lifetime::Press
                        && p.fresh_press
                })
        }
        LeaseGroup::Live => live(s, p),
    }
}
fn concrete_member(s: Option<&Session<Capture<u64>>>, p: Pass, g: ConcreteGroup) -> bool {
    match g {
        ConcreteGroup::Disabled => lease_member(s, p, LeaseGroup::Disabled),
        ConcreteGroup::Missing => lease_member(s, p, LeaseGroup::Missing),
        ConcreteGroup::Future => lease_member(s, p, LeaseGroup::Future),
        ConcreteGroup::Expired => lease_member(s, p, LeaseGroup::Expired),
        ConcreteGroup::ReplacedPress => lease_member(s, p, LeaseGroup::ReplacedPress),
        ConcreteGroup::Idle => live(s, p) && s.is_some_and(|s| !s.payload.captured),
        ConcreteGroup::Captured => live(s, p) && s.is_some_and(|s| s.payload.captured),
    }
}
fn expected_capture(active: bool, enabled: bool, p: Pointer) -> CaptureOutcome {
    let start = p.fresh && (p.held || p.clicked);
    let finish = !p.fresh && (p.stopped || p.clicked);
    let continuing = !p.fresh && !finish && (p.held || p.dragged);
    CaptureOutcome {
        admitted: enabled && (start || active && (finish || continuing)),
        captured: enabled && p.down && (start || active && continuing),
    }
}

#[kani::proof]
#[kani::unwind(7)]
fn every_actual_session_and_pass_has_one_lease_group_and_exact_restore() {
    let saved = any_capture();
    let s = saved.as_ref();
    let p = any_pass();
    let mut count = 0u8;
    for g in [
        LeaseGroup::Disabled,
        LeaseGroup::Missing,
        LeaseGroup::Future,
        LeaseGroup::Expired,
        LeaseGroup::ReplacedPress,
        LeaseGroup::Live,
    ] {
        if lease_member(s, p, g) {
            count += 1;
            assert_eq!(lease_group(s, p), g);
        }
    }
    assert_eq!(count, 1);
    let restored = restore(s, p);
    assert_eq!(restored.is_some(), live(s, p));
    if let Some(restored) = restored {
        assert_eq!(Some(restored), s.map(|s| &s.payload));
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn every_timestamped_capture_has_one_independent_concrete_group() {
    let saved = any_capture();
    let s = saved.as_ref();
    let p = any_pass();
    let mut count = 0u8;
    for g in [
        ConcreteGroup::Disabled,
        ConcreteGroup::Missing,
        ConcreteGroup::Future,
        ConcreteGroup::Expired,
        ConcreteGroup::ReplacedPress,
        ConcreteGroup::Idle,
        ConcreteGroup::Captured,
    ] {
        if concrete_member(s, p, g) {
            count += 1;
            assert_eq!(classify_capture(s, p), g);
        }
    }
    assert_eq!(count, 1);
    assert_eq!(restore_capture(s, p), restore(s, p));
}

#[kani::proof]
fn hold_admission_decodes_the_actual_stored_phase_not_cache_presence() {
    let saved = any_hold();
    let enabled = kani::any();
    let p = any_pointer();
    let active = matches!(saved, Some(HoldPhase::Armed | HoldPhase::Done));
    assert_eq!(
        hold_admitted(saved, enabled, p),
        enabled && p.held && (p.fresh || active)
    );
    assert_eq!(hold_snapshot(saved, enabled).active, active);
}

#[kani::proof]
fn capture_admission_refines_the_actual_stored_state_and_raw_pointer() {
    let saved = any_capture();
    let mut pass = any_pass();
    let p = any_pointer();
    pass.fresh_press = p.fresh;
    let restored = restore_capture(saved.as_ref(), pass);
    let active = live(saved.as_ref(), pass) && saved.as_ref().is_some_and(|s| s.payload.captured);
    assert_eq!(
        capture_step(restored, pass.enabled, p),
        expected_capture(active, pass.enabled, p)
    );
}

#[kani::proof]
fn an_unowned_fresh_press_cannot_complete_or_continue_an_old_capture() {
    let saved = Capture {
        payload: kani::any::<u64>(),
        captured: kani::any(),
    };
    let mut p = any_pointer();
    p.fresh = true;
    p.held = false;
    p.clicked = false;
    let next = capture_step(Some(&saved), kani::any(), p);
    assert!(
        !next.admitted && !next.captured,
        "a new unowned press reused old capture"
    );
}

#[kani::proof]
fn capture_admission_is_independent_of_every_animation_payload_and_preserves_it() {
    let active = kani::any();
    let enabled = kani::any();
    let p = any_pointer();
    let a = Capture {
        captured: active,
        payload: kani::any::<u64>(),
    };
    let b = Capture {
        captured: active,
        payload: kani::any::<u64>(),
    };
    let next = capture_step(Some(&a), enabled, p);
    assert_eq!(next, capture_step(Some(&b), enabled, p));
    let applied = apply_capture(a, next);
    assert_eq!(applied.payload, a.payload);
    assert_eq!(applied.captured, next.captured);
}

#[kani::proof]
fn completed_hold_stays_done_under_arbitrary_animation_progress_until_release() {
    let full = kani::any();
    let next = hold_finish(HoldPhase::Done, true, full);
    assert_eq!(next.phase, HoldPhase::Done);
    assert!(!next.completed);
    let old = any_hold().unwrap_or(HoldPhase::Idle);
    let admitted = kani::any();
    let first = hold_finish(old, admitted, full);
    assert_eq!(first.completed, admitted && full && old != HoldPhase::Done);
    if first.completed {
        let second = hold_finish(first.phase, true, kani::any());
        assert_eq!(second.phase, HoldPhase::Done);
        assert!(!second.completed);
    }
    assert_eq!(hold_finish(old, false, full).phase, HoldPhase::Idle);
}

#[kani::proof]
fn fresh_start_and_finish_work_after_every_raw_stored_history() {
    let saved = any_capture();
    let mut pass = any_pass();
    pass.enabled = true;
    pass.fresh_press = true;
    let p = Pointer {
        fresh: true,
        down: true,
        held: true,
        clicked: false,
        dragged: false,
        stopped: false,
    };
    let old = restore_capture(saved.as_ref(), pass);
    let started = capture_step(old, true, p);
    assert!(started.admitted && started.captured);
    let current = Session {
        seen: pass.now,
        payload: Capture {
            payload: 0u64,
            captured: started.captured,
        },
    };
    pass.fresh_press = false;
    let end = Pointer {
        fresh: false,
        down: false,
        held: false,
        clicked: false,
        dragged: false,
        stopped: true,
    };
    let ended = capture_step(restore_capture(Some(&current), pass), true, end);
    assert!(ended.admitted && !ended.captured);
}

#[kani::proof]
fn invalid_retained_owners_cannot_forward_any_continuation_or_completion() {
    let saved = any_capture();
    let mut pass = any_pass();
    let mut p = any_pointer();
    p.fresh = false;
    pass.fresh_press = false;
    if !live(saved.as_ref(), pass) {
        let next = capture_step(restore_capture(saved.as_ref(), pass), pass.enabled, p);
        assert!(!next.admitted && !next.captured);
    }
}
