//! Total lifecycle protocol for retained interaction state. Payloads never decide admission.

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent raw predicates are classified before operations are admitted"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub present: bool,
    pub enabled: bool,
    pub continuous: bool,
    pub active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
    Absent,
    Disabled,
    Expired,
    Idle,
    Active,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operation {
    Observe,
    Start,
    Continue,
    Finish,
    Cancel,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Effect {
    Unchanged,
    Began,
    Continued,
    Completed,
    Retired,
    Refused,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Transition {
    pub active: bool,
    pub effect: Effect,
}

/// Equality and one successor only: future timestamps and wraparound never revive a lease.
pub(crate) fn adjacent(seen: u64, now: u64) -> bool {
    seen == now || seen.checked_add(1) == Some(now)
}

pub(crate) fn live(seen: u64, now: u64, press_scoped: bool, fresh_press: bool) -> bool {
    adjacent(seen, now) && !(press_scoped && fresh_press && seen != now)
}

pub(crate) fn classify(s: Snapshot) -> Group {
    if !s.present {
        Group::Absent
    } else if !s.enabled {
        Group::Disabled
    } else if !s.continuous {
        Group::Expired
    } else if s.active {
        Group::Active
    } else {
        Group::Idle
    }
}

pub(crate) fn permitted(s: Snapshot, operation: Operation) -> bool {
    match operation {
        Operation::Observe | Operation::Cancel => true,
        Operation::Start => s.present && s.enabled,
        Operation::Continue | Operation::Finish => {
            s.present && s.enabled && s.continuous && s.active
        }
    }
}

pub(crate) fn step(s: Snapshot, operation: Operation) -> Transition {
    match operation {
        Operation::Observe => {
            let active = matches!(classify(s), Group::Active);
            Transition {
                active,
                effect: if s.active && !active {
                    Effect::Retired
                } else {
                    Effect::Unchanged
                },
            }
        }
        Operation::Cancel => Transition {
            active: false,
            effect: Effect::Retired,
        },
        Operation::Start => {
            if permitted(s, operation) {
                Transition {
                    active: true,
                    effect: Effect::Began,
                }
            } else {
                Transition {
                    active: false,
                    effect: Effect::Refused,
                }
            }
        }
        Operation::Continue => {
            if permitted(s, operation) {
                Transition {
                    active: true,
                    effect: Effect::Continued,
                }
            } else {
                Transition {
                    active: false,
                    effect: Effect::Refused,
                }
            }
        }
        Operation::Finish => {
            if permitted(s, operation) {
                Transition {
                    active: false,
                    effect: Effect::Completed,
                }
            } else {
                Transition {
                    active: false,
                    effect: Effect::Refused,
                }
            }
        }
    }
}
