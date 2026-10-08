//! **The density chain's step 2 reaches the scale** — `FAIRING_*`.
//!
//! The chain is specified with five steps and the code had three; `ScaleSource::Env` sat in the
//! enum as the marker for a step that could not happen. The parsing is unit-tested beside the
//! parser, against a lookup rather than the process environment. What only an integration test can
//! show is the other half: that a variable read in `ShellBuilder::build` actually moves the scale a
//! screen is drawn at, and that the two things allowed to beat it do.
//!
//! # Why this is one test and not four
//!
//! The process environment is shared by every test thread in a binary, so four tests setting
//! `FAIRING_PPI` would race each other. The phases run in order inside one test instead, and each
//! clears up after itself.

#![cfg(feature = "mock")]

use fairing::testing::{single_level_access, Harness};
use fairing::unit::{ScalePolicy, ScaleSource};
use fairing::Shell;

/// 254 ppi is exactly 10 px/mm, which makes the assertion readable.
const PPI: &str = "254";
/// What `PPI` is in px per mm.
const PX_PER_MM: f32 = 10.0;

/// A shell at a fixed root size, with the policy the phase wants.
fn shell(policy: ScalePolicy, pin_mm: Option<(f32, f32)>) -> fairing::Result<Harness> {
    Harness::from_builder(move |ctx| {
        let mut b = Shell::builder(single_level_access()).scale_policy(policy);
        if let Some((w, h)) = pin_mm {
            b = b.physical_mm(w, h);
        }
        b.build(ctx)
    })
    .map(|mut h| {
        h.frames(2);
        h
    })
}

#[test]
fn the_environment_sets_the_density_unless_it_is_forbidden_or_outranked() -> fairing::Result<()> {
    // 1 — with nothing in the environment, the chain falls through to where it always did.
    std::env::remove_var("FAIRING_PPI");
    let bare = shell(ScalePolicy::default(), None)?;
    assert_ne!(
        bare.shell.scale().source,
        ScaleSource::Env,
        "nothing was set, so nothing should say it came from the environment"
    );

    // 2 — set it, and the scale a screen is drawn at moves.
    std::env::set_var("FAIRING_PPI", PPI);
    let env = shell(ScalePolicy::default(), None)?;
    let scale = env.shell.scale();
    assert_eq!(
        scale.source,
        ScaleSource::Env,
        "the source has to name the environment, or a diagnostic cannot tell a person why the \
         device looks different from its own configuration"
    );
    assert!(
        (scale.px_per_mm - PX_PER_MM).abs() < 0.01,
        "{PPI} ppi is {PX_PER_MM} px/mm, got {}",
        scale.px_per_mm
    );

    // 3 — the lock is a lock. The variable is still set; the policy forbids reading it.
    let locked = shell(ScalePolicy::default().with_allow_env(false), None)?;
    assert_ne!(
        locked.shell.scale().source,
        ScaleSource::Env,
        "`allow_env = false` has to shut the step off even with the variable set"
    );

    // 4 — the integrator's pin still wins. They are stating a fact about their hardware; the
    // person at the device is working around a file they cannot edit.
    let pinned = shell(ScalePolicy::default(), Some((152.4, 91.4)))?;
    assert_eq!(
        pinned.shell.scale().source,
        ScaleSource::Pin,
        "a code pin outranks the environment"
    );

    std::env::remove_var("FAIRING_PPI");
    Ok(())
}
