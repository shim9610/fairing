//! Decisions shared by public access APIs and the verification harnesses.

pub(crate) fn level_allows(held: u16, required: u16) -> bool {
    held >= required
}

pub(crate) fn grant_allowed(known: bool, lock: bool, held: u16, candidate: u16) -> bool {
    known
        && if lock {
            candidate >= held
        } else {
            candidate > held
        }
}

pub(crate) fn next_failure(current: u32) -> u32 {
    current.saturating_add(1)
}

pub(crate) fn lockout_reached(failures: u32, limit: Option<u32>) -> bool {
    match limit {
        Some(limit) => failures >= limit,
        None => false,
    }
}

/// A response may complete only the still-active operation.
pub(crate) fn response_is_current(active: Option<usize>, response: usize) -> bool {
    active == Some(response)
}
