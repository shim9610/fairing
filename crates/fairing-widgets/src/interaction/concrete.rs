//! Concrete retained carriers and the adapters from them to the lifecycle protocol.
//! These are production types, not verifier-only replicas of widget state.
use super::rules::{self, Effect, Operation, Snapshot};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lifetime {
    Mounted,
    Press,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Session<T> {
    pub seen: u64,
    pub payload: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pass {
    pub now: u64,
    pub enabled: bool,
    pub lifetime: Lifetime,
    pub fresh_press: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LeaseGroup {
    Disabled,
    Missing,
    Future,
    Expired,
    ReplacedPress,
    Live,
}

/// The last-seen stamp, scope and current pass determine admission before payload access.
pub(crate) fn lease_group<T>(saved: Option<&Session<T>>, pass: Pass) -> LeaseGroup {
    if !pass.enabled {
        return LeaseGroup::Disabled;
    }
    match saved {
        None => LeaseGroup::Missing,
        Some(saved) => {
            if saved.seen > pass.now {
                LeaseGroup::Future
            } else if !rules::adjacent(saved.seen, pass.now) {
                LeaseGroup::Expired
            } else if matches!(pass.lifetime, Lifetime::Press)
                && pass.fresh_press
                && saved.seen != pass.now
            {
                LeaseGroup::ReplacedPress
            } else {
                LeaseGroup::Live
            }
        }
    }
}

/// Borrowing a live payload neither clones it nor updates the last-seen stamp.
pub(crate) fn restore<T>(saved: Option<&Session<T>>, pass: Pass) -> Option<&T> {
    let keep = rules::step(
        Snapshot {
            present: true,
            enabled: pass.enabled,
            continuous: matches!(lease_group(saved, pass), LeaseGroup::Live),
            active: saved.is_some(),
        },
        Operation::Observe,
    )
    .active;
    if keep {
        match saved {
            Some(saved) => Some(&saved.payload),
            None => None,
        }
    } else {
        None
    }
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "independent raw pointer observations, including inconsistent combinations"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pointer {
    pub fresh: bool,
    pub down: bool,
    pub held: bool,
    pub clicked: bool,
    pub dragged: bool,
    pub stopped: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HoldPhase {
    Idle,
    Armed,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HoldOutcome {
    pub phase: HoldPhase,
    pub completed: bool,
}

pub(crate) fn hold_snapshot(saved: Option<HoldPhase>, enabled: bool) -> Snapshot {
    Snapshot {
        present: true,
        enabled,
        continuous: saved.is_some(),
        active: matches!(saved, Some(HoldPhase::Armed | HoldPhase::Done)),
    }
}

pub(crate) fn hold_operation(pointer: Pointer) -> Operation {
    if !pointer.held {
        Operation::Cancel
    } else if pointer.fresh {
        Operation::Start
    } else {
        Operation::Continue
    }
}

pub(crate) fn hold_admitted(saved: Option<HoldPhase>, enabled: bool, pointer: Pointer) -> bool {
    rules::step(hold_snapshot(saved, enabled), hold_operation(pointer)).active
}

/// Completion stays latched for this press even if an animation's progress changes.
pub(crate) fn hold_finish(previous: HoldPhase, admitted: bool, full: bool) -> HoldOutcome {
    if !admitted {
        HoldOutcome {
            phase: HoldPhase::Idle,
            completed: false,
        }
    } else if matches!(previous, HoldPhase::Done) {
        HoldOutcome {
            phase: HoldPhase::Done,
            completed: false,
        }
    } else if full {
        HoldOutcome {
            phase: HoldPhase::Done,
            completed: true,
        }
    } else {
        HoldOutcome {
            phase: HoldPhase::Armed,
            completed: false,
        }
    }
}

/// The full animation/value payload is kept separate from input ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Capture<T> {
    pub payload: T,
    pub captured: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaptureOutcome {
    pub captured: bool,
    pub admitted: bool,
}

pub(crate) fn capture_snapshot<T>(saved: Option<&Capture<T>>, enabled: bool) -> Snapshot {
    Snapshot {
        present: true,
        enabled,
        continuous: true,
        active: match saved {
            Some(saved) => saved.captured,
            None => false,
        },
    }
}

pub(crate) fn capture_operation(pointer: Pointer) -> Operation {
    if pointer.fresh {
        if pointer.held || pointer.clicked {
            Operation::Start
        } else {
            Operation::Cancel
        }
    } else if pointer.stopped || pointer.clicked {
        Operation::Finish
    } else if pointer.held || pointer.dragged {
        Operation::Continue
    } else {
        Operation::Cancel
    }
}

pub(crate) fn capture_step<T>(
    saved: Option<&Capture<T>>,
    enabled: bool,
    pointer: Pointer,
) -> CaptureOutcome {
    let transition = rules::step(capture_snapshot(saved, enabled), capture_operation(pointer));
    CaptureOutcome {
        captured: transition.active && pointer.down,
        admitted: matches!(
            transition.effect,
            Effect::Began | Effect::Continued | Effect::Completed
        ),
    }
}

pub(crate) fn apply_capture<T>(mut saved: Capture<T>, next: CaptureOutcome) -> Capture<T> {
    saved.captured = next.captured;
    saved
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConcreteGroup {
    Disabled,
    Missing,
    Future,
    Expired,
    ReplacedPress,
    Idle,
    Captured,
}

/// Classify the actual timestamped capture and its arbitrary animation/value payload.
pub(crate) fn classify_capture<T>(
    saved: Option<&Session<Capture<T>>>,
    pass: Pass,
) -> ConcreteGroup {
    match lease_group(saved, pass) {
        LeaseGroup::Disabled => ConcreteGroup::Disabled,
        LeaseGroup::Missing => ConcreteGroup::Missing,
        LeaseGroup::Future => ConcreteGroup::Future,
        LeaseGroup::Expired => ConcreteGroup::Expired,
        LeaseGroup::ReplacedPress => ConcreteGroup::ReplacedPress,
        LeaseGroup::Live => match restore(saved, pass) {
            Some(saved) if saved.captured => ConcreteGroup::Captured,
            _ => ConcreteGroup::Idle,
        },
    }
}

pub(crate) fn restore_capture<T>(
    saved: Option<&Session<Capture<T>>>,
    pass: Pass,
) -> Option<&Capture<T>> {
    match classify_capture(saved, pass) {
        ConcreteGroup::Idle | ConcreteGroup::Captured => restore(saved, pass),
        ConcreteGroup::Disabled
        | ConcreteGroup::Missing
        | ConcreteGroup::Future
        | ConcreteGroup::Expired
        | ConcreteGroup::ReplacedPress => None,
    }
}
