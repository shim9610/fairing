//! Input admission shared with progress proofs. This decision deliberately has no latch:
//! after any history, one pass without a fresh conflicting press must admit ordinary UI.

pub(crate) fn defer_ordinary_ui(
    geometry_changed: bool,
    fresh_primary_press: bool,
    region_claimed: bool,
) -> bool {
    geometry_changed && fresh_primary_press && region_claimed
}

pub(crate) fn regions_can_capture(
    overlay_closed: bool,
    prompt_open: bool,
    overview_open: bool,
) -> bool {
    overlay_closed && !prompt_open && !overview_open
}
