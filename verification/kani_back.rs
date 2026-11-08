#[path = "../crates/fairing/src/shell/back_route.rs"]
mod back_route;
use back_route::{BackGroup as G, BackState as S, BackTarget as T};

fn any_state() -> S {
    S {
        faulted: kani::any(),
        prompt: kani::any(),
        lock: kani::any(),
        info: kani::any(),
        overlay: kani::any(),
        keyboard: kani::any(),
        overview: kani::any(),
        home: kani::any(),
    }
}

// Independent specification: explicit predicates, not calls to the classifier.
fn belongs(s: S, group: G) -> bool {
    match group {
        G::Faulted => s.faulted,
        G::Locked => !s.faulted && s.prompt && s.lock,
        G::Unlock => !s.faulted && s.prompt && !s.lock,
        G::Info => !s.faulted && !s.prompt && s.info,
        G::Overlay => !s.faulted && !s.prompt && !s.info && s.overlay,
        G::Keyboard => !s.faulted && !s.prompt && !s.info && !s.overlay && s.keyboard,
        G::Overview => {
            !s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && s.overview
        }
        G::Home => {
            !s.faulted && !s.prompt && !s.info && !s.overlay && !s.keyboard && !s.overview && s.home
        }
        G::Workspace => {
            !s.faulted
                && !s.prompt
                && !s.info
                && !s.overlay
                && !s.keyboard
                && !s.overview
                && !s.home
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn every_back_state_has_exactly_one_specified_group() {
    let s = any_state();
    let groups = [
        G::Faulted,
        G::Locked,
        G::Unlock,
        G::Info,
        G::Overlay,
        G::Keyboard,
        G::Overview,
        G::Home,
        G::Workspace,
    ];
    let mut count = 0u8;
    for group in groups {
        if belongs(s, group) {
            count += 1;
            assert_eq!(back_route::classify(s), group);
        }
    }
    assert_eq!(count, 1);
}

#[kani::proof]
fn every_permitted_back_is_dispatched_to_its_specified_handler() {
    let s = any_state();
    let group = back_route::classify(s);
    let result = back_route::dispatch(group);
    // The promise comes from the independent specification, not implementation refusal.
    let promised = !s.faulted
        && !(s.prompt && s.lock)
        && (s.prompt || s.info || s.overlay || s.keyboard || s.overview || !s.home);
    assert_eq!(result != T::Ignore, promised);
    match group {
        G::Faulted | G::Locked | G::Home => assert_eq!(result, T::Ignore),
        G::Unlock => assert_eq!(result, T::CancelPrompt),
        G::Info => assert_eq!(result, T::CloseInfo),
        G::Overlay => assert_eq!(result, T::CloseOverlay),
        G::Keyboard => assert_eq!(result, T::HideKeyboard),
        G::Overview => assert_eq!(result, T::CloseOverview),
        G::Workspace => assert_eq!(result, T::Workspace),
    }
}

#[kani::proof]
fn removing_a_top_owner_exposes_the_next_owner_without_a_history_latch() {
    let old = any_state();
    let _previous_group = back_route::classify(old);
    let now = S {
        faulted: false,
        prompt: false,
        lock: old.lock,
        info: false,
        overlay: false,
        keyboard: false,
        overview: false,
        home: old.home,
    };
    assert_eq!(
        back_route::classify(now),
        if now.home { G::Home } else { G::Workspace }
    );
    assert_eq!(
        back_route::dispatch(back_route::classify(now)),
        if now.home { T::Ignore } else { T::Workspace }
    );
}
