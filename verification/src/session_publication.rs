//! Exhaust actual product atomics in bounded writer/reader/revocation scenarios.
#[path = "../../crates/fairing/src/access/session_rules.rs"]
mod session_rules;
#[path = "../../crates/fairing/src/access/session_shared.rs"]
mod session_shared;

#[test]
fn permission_publication_never_combines_two_different_versions() {
    loom::model(|| {
        let shared = loom::sync::Arc::new(session_shared::Shared::new(100, &[(100, 0), (0, 0)]));
        let writer = loom::sync::Arc::clone(&shared);
        let work =
            loom::thread::spawn(move || assert!(writer.publish(0, 100, &[(0, 0), (100, 0)])));
        if let session_rules::Observation::Active(revision) = shared.observe(0) {
            let first = shared.allows(0, 0, revision);
            let second = shared.allows(1, 0, revision);
            assert!(
                !(first && second),
                "permissions from different publications were combined"
            );
        }
        work.join().unwrap();
        let session_rules::Observation::Active(revision) = shared.observe(0) else {
            panic!("finished publication unavailable")
        };
        assert!(!shared.allows(0, 0, revision));
        assert!(shared.allows(1, 0, revision));
        assert!(shared.next_deadline(0).is_some());
    });
}

#[test]
fn revocation_cannot_be_undone_by_an_inflight_publication() {
    loom::model(|| {
        let shared = loom::sync::Arc::new(session_shared::Shared::new(100, &[(100, 0)]));
        let writer = loom::sync::Arc::clone(&shared);
        let revoker = loom::sync::Arc::clone(&shared);
        let write = loom::thread::spawn(move || {
            let _ = writer.publish(0, 100, &[(100, 0)]);
        });
        let revoke = loom::thread::spawn(move || revoker.revoke());
        write.join().unwrap();
        revoke.join().unwrap();
        assert_eq!(shared.observe(0), session_rules::Observation::Revoked);
        assert!(!shared.allows(0, 0, 0));
        assert!(!shared.publish(0, 100, &[(100, 0)]));
        assert_eq!(shared.observe(0), session_rules::Observation::Revoked);
    });
}
