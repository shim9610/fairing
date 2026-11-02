#[path = "../crates/fairing/src/shell/input_route.rs"]
mod input_route;

#[kani::proof]
fn fresh_conflicting_press_never_reaches_ordinary_ui() {
    assert!(input_route::defer_ordinary_ui(true, true, true));
}

#[kani::proof]
fn deferral_cannot_block_an_unclaimed_or_unchanged_input() {
    let changed: bool = kani::any();
    let fresh: bool = kani::any();
    let claimed: bool = kani::any();
    let deferred = input_route::defer_ordinary_ui(changed, fresh, claimed);
    if !claimed || !changed || !fresh {
        assert!(!deferred);
    }
}

#[kani::proof]
fn one_quiet_pass_restores_ui_after_any_conflict() {
    let _previous = input_route::defer_ordinary_ui(kani::any(), kani::any(), kani::any());
    // Geometry may keep animating; only a fresh conflicting press must cease for this pass.
    let restored = input_route::defer_ordinary_ui(kani::any(), false, kani::any());
    assert!(!restored);
}

#[kani::proof]
fn modals_and_overviews_always_preempt_region_capture() {
    let overlay_closed: bool = kani::any();
    let prompt_open: bool = kani::any();
    let overview_open: bool = kani::any();
    let capture = input_route::regions_can_capture(overlay_closed, prompt_open, overview_open);
    if prompt_open || overview_open || !overlay_closed {
        assert!(!capture);
    } else {
        assert!(capture);
    }
}
