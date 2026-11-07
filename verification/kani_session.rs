#[path = "../crates/fairing/src/access/session_rules.rs"]
mod session_rules;
use session_rules::{ControlEvent, ControlState, Observation};

#[kani::proof]
fn session_identities_never_wrap_or_repeat() {
    let id: u64 = kani::any();
    match session_rules::next_identity(id) {
        Some(next) => {
            assert!(next > id);
            assert_eq!(next, id + 1);
        }
        None => assert_eq!(id, u64::MAX),
    }
}

#[kani::proof]
fn publication_revisions_are_even_strictly_new_and_never_wrap() {
    let revision: u64 = kani::any();
    match session_rules::next_revision(revision) {
        Some(next) => {
            assert_eq!(revision % 2, 0);
            assert_eq!(next % 2, 0);
            assert!(next > revision);
            assert_eq!(next, revision + 2);
        }
        None => assert!(revision % 2 != 0 || revision >= u64::MAX - 1),
    }
}

#[kani::proof]
fn every_observation_has_exactly_its_independent_security_partition() {
    let revoked: bool = kani::any();
    let before: u64 = kani::any();
    let after: u64 = kani::any();
    let now: u64 = kani::any();
    let until: u64 = kani::any();
    let active = !revoked && now < until && before == after && before % 2 == 0;
    let expired = !revoked && now >= until;
    let busy = !revoked && now < until && (before != after || before % 2 != 0);
    assert_eq!(
        u8::from(revoked) + u8::from(expired) + u8::from(busy) + u8::from(active),
        1
    );
    match session_rules::observe(revoked, before, after, now, until) {
        Observation::Active(revision) => {
            assert!(active);
            assert_eq!(revision, before);
        }
        Observation::Revoked => assert!(revoked),
        Observation::Expired => assert!(expired),
        Observation::Busy => assert!(busy),
    }
}

#[kani::proof]
fn permission_expiry_cannot_restore_removed_base_rights() {
    let live: bool = kani::any();
    let now: u64 = kani::any();
    let base: u64 = kani::any();
    let temporary: u64 = kani::any();
    let allowed = session_rules::permission_allows(live, now, base, temporary);
    assert_eq!(allowed, live && (base > now || temporary > now));
    assert!(!session_rules::permission_allows(
        false, now, base, temporary
    ));
    assert!(!session_rules::permission_allows(live, now, 0, now));
    if temporary <= now {
        assert_eq!(allowed, live && base > now);
    }
}

#[kani::proof]
fn policy_can_only_narrow_issuer_authority_and_faults_always_deny() {
    let fault: bool = kani::any();
    let issued: bool = kani::any();
    let policy: bool = kani::any();
    let allowed = session_rules::authorize(fault, issued, policy);
    assert_eq!(allowed, !fault && issued && policy);
    if allowed {
        assert!(issued && policy && !fault);
    }
}

#[kani::proof]
fn revocation_is_inductive_and_expired_or_exhausted_sessions_cannot_revive() {
    let before = ControlState {
        revision: kani::any(),
        revoked: kani::any(),
    };
    let now: u64 = kani::any();
    let until: u64 = kani::any();
    let event = if kani::any() {
        ControlEvent::Revoke
    } else {
        ControlEvent::Publish { now, until }
    };
    let after = session_rules::transition(before, event);
    if before.revoked {
        assert!(after.revoked);
        assert_eq!(after.revision, before.revision);
    }
    match event {
        ControlEvent::Revoke => {
            assert!(after.revoked);
            assert_eq!(after.revision, before.revision);
        }
        ControlEvent::Publish { .. } => {
            if now >= until || session_rules::next_revision(before.revision).is_none() {
                assert!(after.revoked);
            }
            if !after.revoked {
                assert!(!before.revoked);
                assert!(now < until);
                assert!(after.revision > before.revision);
                assert_eq!(after.revision % 2, 0);
            }
        }
    }
}

#[path = "../crates/fairing/src/access/session_shared.rs"]
mod session_shared;

#[kani::proof]
#[kani::unwind(16)]
fn actual_atomic_storage_preserves_independent_permission_cells() {
    let until: u64 = kani::any();
    let now: u64 = kani::any();
    let a: (u64, u64) = kani::any();
    let b: (u64, u64) = kani::any();
    let shared = session_shared::Shared::new(until, &[a, b]);
    if now < until {
        assert_eq!(shared.observe(now), Observation::Active(0));
        assert_eq!(shared.allows(0, now, 0), now < a.0 || now < a.1);
        assert_eq!(shared.allows(1, now, 0), now < b.0 || now < b.1);
    } else {
        assert_eq!(shared.observe(now), Observation::Expired);
        assert!(!shared.allows(0, now, 0));
        assert!(!shared.allows(1, now, 0));
    }
    assert!(!shared.allows(2, now, 0));
}

#[kani::proof]
#[kani::unwind(16)]
fn actual_publication_invalidates_old_snapshots_and_atomically_replaces_permissions() {
    let now: u64 = kani::any();
    kani::assume(now < u64::MAX);
    let until: u64 = kani::any();
    let a: (u64, u64) = kani::any();
    let b: (u64, u64) = kani::any();
    let shared = session_shared::Shared::new(u64::MAX, &[(u64::MAX, 0), (0, u64::MAX)]);
    assert!(shared.publish(now, until, &[a, b]));
    assert!(!shared.allows(0, now, 0));
    assert!(!shared.allows(1, now, 0));
    assert_eq!(
        shared.allows(0, now, 2),
        now < until && (now < a.0 || now < a.1)
    );
    assert_eq!(
        shared.allows(1, now, 2),
        now < until && (now < b.0 || now < b.1)
    );
}

#[kani::proof]
#[kani::unwind(16)]
fn actual_revoked_storage_never_accepts_or_exposes_a_later_publication() {
    let now: u64 = kani::any();
    let until: u64 = kani::any();
    let expected: u64 = kani::any();
    let rights: (u64, u64) = kani::any();
    let shared = session_shared::Shared::new(until, &[rights]);
    shared.revoke();
    assert_eq!(shared.observe(now), Observation::Revoked);
    assert!(!shared.publish(now, u64::MAX, &[(u64::MAX, u64::MAX)]));
    assert!(!shared.allows(0, now, expected));
    assert_eq!(shared.observe(now), Observation::Revoked);
}
