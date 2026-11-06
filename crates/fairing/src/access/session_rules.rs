//! Exact session decisions shared by the product and independent verifiers.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Observation {
    Active(u64),
    Revoked,
    Expired,
    Busy,
}

pub(crate) fn next_identity(current: u64) -> Option<u64> {
    if current < u64::MAX {
        Some(current + 1)
    } else {
        None
    }
}

#[allow(
    clippy::manual_is_multiple_of,
    reason = "Exact arithmetic is shared with Verus"
)]
pub(crate) fn next_revision(current: u64) -> Option<u64> {
    if current % 2 == 0 && current < u64::MAX - 1 {
        Some(current + 2)
    } else {
        None
    }
}

#[allow(
    clippy::manual_is_multiple_of,
    reason = "Exact arithmetic is shared with Verus"
)]
pub(crate) fn observe(revoked: bool, before: u64, after: u64, now: u64, until: u64) -> Observation {
    if revoked {
        Observation::Revoked
    } else if now >= until {
        Observation::Expired
    } else if before != after || before % 2 != 0 {
        Observation::Busy
    } else {
        Observation::Active(before)
    }
}

pub(crate) fn permission_allows(
    live: bool,
    now: u64,
    base_until: u64,
    temporary_until: u64,
) -> bool {
    live && (now < base_until || now < temporary_until)
}

pub(crate) fn authorize(quarantined: bool, issued: bool, policy: bool) -> bool {
    !quarantined && issued && policy
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControlState {
    pub revision: u64,
    pub revoked: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum ControlEvent {
    Publish { now: u64, until: u64 },
    Revoke,
}

pub(crate) fn transition(state: ControlState, event: ControlEvent) -> ControlState {
    match event {
        ControlEvent::Revoke => ControlState {
            revision: state.revision,
            revoked: true,
        },
        ControlEvent::Publish { now, until } => {
            if state.revoked || now >= until {
                ControlState {
                    revision: state.revision,
                    revoked: true,
                }
            } else {
                match next_revision(state.revision) {
                    Some(revision) => ControlState {
                        revision,
                        revoked: false,
                    },
                    None => ControlState {
                        revision: state.revision,
                        revoked: true,
                    },
                }
            }
        }
    }
}
