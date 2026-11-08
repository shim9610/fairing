//! Independent four-permission state oracle against public issuer/reader APIs.
use fairing::access::{Gate, SessionController, SessionError, SessionGrant, Subject};
use std::time::Duration;

pub fn exercise(events: &[u8]) {
    let host = fairing::testing::Harness::new(
        fairing::ShellConfig::default(),
        fairing::Services::builder().build(),
    )
    .unwrap();
    let origin = host.shell.now();
    let names = ["measure", "configure", "export", "accounts"];
    let (mut controller, token) = SessionController::issue(
        Subject::default(),
        names
            .iter()
            .map(|name| SessionGrant::inactive(*name))
            .collect(),
        None,
        origin,
        fairing::services::Waker::new(&host.ctx),
    )
    .unwrap();
    let (other, peer) = SessionController::issue(
        Subject::default(),
        vec![SessionGrant::base("peer")],
        None,
        origin,
        fairing::services::Waker::new(&host.ctx),
    )
    .unwrap();
    assert_ne!(controller.id(), other.id());
    let mut base = [false; 4];
    let mut temporary = [0u64; 4];
    let mut revoked = false;
    let mut ticks = 0u64;
    for &event in events.iter().take(64) {
        ticks += u64::from(event % 3);
        let now = origin + Duration::from_secs(ticks);
        if event % 11 == 0 {
            controller.revoke();
            revoked = true;
        } else if event % 11 == 1 {
            assert_eq!(
                controller.replace_grants(vec![SessionGrant::base("outside-catalog")], None, now),
                Err(SessionError::UnknownGate)
            );
        } else {
            let index = usize::from(event % 4);
            let mut candidate_base = base;
            let mut candidate_temporary = temporary;
            candidate_base[index] = event & 16 != 0;
            candidate_temporary[index] = if event & 32 != 0 {
                ticks + u64::from(event % 7)
            } else {
                0
            };
            let grants = names
                .iter()
                .enumerate()
                .map(|(i, name)| SessionGrant {
                    gate: Gate::from(*name),
                    base: candidate_base[i],
                    temporary_until: (candidate_temporary[i] > 0)
                        .then(|| origin + Duration::from_secs(candidate_temporary[i])),
                })
                .collect();
            let result = controller.replace_grants(grants, None, now);
            if revoked {
                assert_eq!(result, Err(SessionError::Ended));
            } else {
                result.unwrap();
                base = candidate_base;
                temporary = candidate_temporary;
            }
        }
        for (i, name) in names.iter().enumerate() {
            let actual = token
                .snapshot(now)
                .is_ok_and(|s| s.allows(&Gate::from(*name)));
            assert_eq!(
                actual,
                !revoked && (base[i] || temporary[i] > ticks),
                "issuer/reader permission oracle diverged"
            );
        }
        assert!(
            peer.snapshot(now).unwrap().allows(&Gate::from("peer")),
            "one controller affected an unrelated session"
        );
    }
}

#[test]
fn public_session_history_covers_grants_expiry_revocation_and_isolation() {
    exercise(&[16, 48, 19, 50, 35, 64, 18, 1, 77, 11, 51, 32, 7]);
}
