#[path = "../crates/fairing-widgets/src/interaction/rules.rs"]
mod rules;
use rules::{Effect as E, Group as G, Operation as O, Snapshot as S};
fn any_state() -> S {
    S {
        present: kani::any(),
        enabled: kani::any(),
        continuous: kani::any(),
        active: kani::any(),
    }
}
fn any_operation() -> O {
    match kani::any::<u8>() % 5 {
        0 => O::Observe,
        1 => O::Start,
        2 => O::Continue,
        3 => O::Finish,
        _ => O::Cancel,
    }
}
fn member(s: S, g: G) -> bool {
    match g {
        G::Absent => !s.present,
        G::Disabled => s.present && !s.enabled,
        G::Expired => s.present && s.enabled && !s.continuous,
        G::Idle => s.present && s.enabled && s.continuous && !s.active,
        G::Active => s.present && s.enabled && s.continuous && s.active,
    }
}
// This independent promise does not depend on the implementation's refusal result.
fn promised(s: S, o: O) -> bool {
    match o {
        O::Observe | O::Cancel => true,
        O::Start => s.present && s.enabled,
        O::Continue | O::Finish => member(s, G::Active),
    }
}
#[kani::proof]
fn every_timestamp_pair_has_nonwrapping_lease_semantics() {
    let seen: u64 = kani::any();
    let now: u64 = kani::any();
    let current = seen <= now && now - seen <= 1;
    assert_eq!(rules::adjacent(seen, now), current);
    let press = kani::any();
    let fresh = kani::any();
    assert_eq!(
        rules::live(seen, now, press, fresh),
        current && !(press && fresh && seen != now)
    );
    assert!(!rules::adjacent(u64::MAX, 0));
}
#[kani::proof]
#[kani::unwind(6)]
fn every_retained_state_belongs_to_exactly_one_specified_group() {
    let s = any_state();
    let mut count = 0u8;
    for g in [G::Absent, G::Disabled, G::Expired, G::Idle, G::Active] {
        if member(s, g) {
            count += 1;
            assert_eq!(rules::classify(s), g);
        }
    }
    assert_eq!(count, 1);
}
#[kani::proof]
fn every_permitted_operation_has_its_specified_immediate_outcome() {
    let s = any_state();
    let o = any_operation();
    let t = rules::step(s, o);
    assert_eq!(rules::permitted(s, o), promised(s, o));
    assert_eq!(t.effect != E::Refused, promised(s, o));
    if !promised(s, o) {
        assert!(!t.active);
        return;
    }
    match o {
        O::Observe => {
            assert_eq!(t.active, member(s, G::Active));
            assert_eq!(
                t.effect,
                if s.active && !member(s, G::Active) {
                    E::Retired
                } else {
                    E::Unchanged
                }
            );
        }
        O::Start => {
            assert!(t.active);
            assert_eq!(t.effect, E::Began);
        }
        O::Continue => {
            assert!(t.active);
            assert_eq!(t.effect, E::Continued);
        }
        O::Finish => {
            assert!(!t.active);
            assert_eq!(t.effect, E::Completed);
        }
        O::Cancel => {
            assert!(!t.active);
            assert_eq!(t.effect, E::Retired);
        }
    }
}
#[kani::proof]
fn stale_owners_cannot_keep_activity_and_cannot_refuse_a_new_start() {
    let mut s = any_state();
    let retired = rules::step(s, O::Observe);
    if !s.present || !s.enabled || !s.continuous {
        assert!(!retired.active);
    }
    // Arbitrary prior history, then the specification says this owner is present/enabled.
    s.present = true;
    s.enabled = true;
    let start = rules::step(s, O::Start);
    assert!(start.active);
    assert_eq!(start.effect, E::Began);
    let live = S {
        continuous: true,
        active: start.active,
        ..s
    };
    let end = rules::step(live, O::Finish);
    assert!(!end.active);
    assert_eq!(end.effect, E::Completed);
    let cancel = rules::step(s, O::Cancel);
    let retry = rules::step(
        S {
            active: cancel.active,
            ..s
        },
        O::Start,
    );
    assert!(retry.active);
    assert_eq!(retry.effect, E::Began);
}
