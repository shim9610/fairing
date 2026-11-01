#[path = "../crates/fairing/src/access/rules.rs"]
mod rules;

#[kani::proof]
fn ordered_gates_are_monotone_and_reflexive() {
    let held: u16 = kani::any();
    let required: u16 = kani::any();
    let higher: u16 = kani::any();
    assert_eq!(rules::level_allows(held, required), held >= required);
    assert!(rules::level_allows(held, held));
    if higher >= held && rules::level_allows(held, required) {
        assert!(rules::level_allows(higher, required));
    }
}

#[kani::proof]
fn grants_cannot_use_unknown_or_lower_levels() {
    let known: bool = kani::any();
    let lock: bool = kani::any();
    let held: u16 = kani::any();
    let candidate: u16 = kani::any();
    let allowed = rules::grant_allowed(known, lock, held, candidate);
    assert_eq!(
        allowed,
        known && (candidate > held || (lock && candidate == held))
    );
}

#[kani::proof]
fn failure_accounting_never_wraps() {
    let before: u32 = kani::any();
    let after = rules::next_failure(before);
    assert!(after >= before);
    assert_eq!(after == before, before == u32::MAX);
}

#[kani::proof]
fn lockout_is_inclusive_and_cannot_be_disabled_by_more_failures() {
    let count: u32 = kani::any();
    let limit: Option<u32> = kani::any();
    assert_eq!(
        rules::lockout_reached(count, limit),
        limit.is_some_and(|n| count >= n)
    );
    if rules::lockout_reached(count, limit) {
        assert!(rules::lockout_reached(rules::next_failure(count), limit));
    }
}
