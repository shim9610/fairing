//! Independent permission-set oracle against typed registration and real dispatch.
use fairing::access::{
    AccessEvent, AuthOutcome, Permission, Protected, SessionController, SessionGrant, Subject,
};
use fairing::{action, LaunchAction, Shell, ShellConfig, ShellEvent};

struct Read;
impl Permission for Read {
    const GATE: &'static str = "read";
}
struct Write;
impl Permission for Write {
    const GATE: &'static str = "write";
}

pub fn exercise(events: &[u8]) {
    let ctx = fairing::egui::Context::default();
    let mut config = ShellConfig::default();
    config.access.session_tokens = true;
    let mut shell = Shell::builder(config).build_protected(&ctx).unwrap();
    shell.add(Protected::<Read, _>::new(
        action("read-action", |cx| {
            cx.app_mut::<[usize; 2]>().unwrap()[0] += 1;
        })
        .gate("incorrect-untyped-gate"),
    ));
    shell.add(Protected::<Write, _>::new(action("write-action", |cx| {
        cx.app_mut::<[usize; 2]>().unwrap()[1] += 1;
    })));
    let mut actual = [0usize; 2];
    let mut expected = [1usize, 0];
    shell.launch_with(LaunchAction::run("read-action"), &mut actual);
    assert_eq!(actual, [0, 0]);
    let request = shell
        .poll_events()
        .into_iter()
        .find_map(|event| match event {
            ShellEvent::Access(AccessEvent::SessionRequested { request, .. }) => Some(request),
            _ => None,
        })
        .unwrap();
    let now = shell.shell().now();
    let (mut owner, token) = SessionController::issue(
        Subject::default(),
        vec![
            SessionGrant::base_for::<Read>(),
            SessionGrant::inactive_for::<Write>(),
        ],
        None,
        now,
        fairing::services::Waker::new(&ctx),
    )
    .unwrap();
    shell.accept_auth_response_with(request.respond(AuthOutcome::Session(token)), &mut actual);
    assert_eq!(actual, expected);
    let mut mask = 1u8;
    let mut revoked = false;
    let mut attached = true;
    for &event in events.iter().take(64) {
        match event % 8 {
            0 => {
                let next = (event >> 3) & 3;
                let grants = vec![
                    if next & 1 != 0 {
                        SessionGrant::base_for::<Read>()
                    } else {
                        SessionGrant::inactive_for::<Read>()
                    },
                    if next & 2 != 0 {
                        SessionGrant::base_for::<Write>()
                    } else {
                        SessionGrant::inactive_for::<Write>()
                    },
                ];
                let published = owner.replace_grants(grants, None, now);
                assert_eq!(published.is_ok(), !revoked);
                if !revoked {
                    mask = next;
                }
            }
            4 => {
                owner.revoke();
                revoked = true;
            }
            5 => {
                shell.launch(LaunchAction::Logout);
                attached = false;
            }
            _ => {
                let index = usize::from(event & 1);
                let id = if index == 0 {
                    "read-action"
                } else {
                    "write-action"
                };
                if attached && !revoked && mask & (1 << index) != 0 {
                    expected[index] += 1;
                }
                shell.launch_with(LaunchAction::run(id), &mut actual);
            }
        }
        assert_eq!(
            actual, expected,
            "typed callback dispatch escaped the independent permission oracle"
        );
    }
}

#[test]
fn typed_dispatch_matches_independent_permissions_and_terminal_detachment() {
    exercise(&[1, 2, 16, 1, 2, 24, 1, 2, 0, 1, 2, 8, 1, 2, 4, 24, 1, 2]);
    exercise(&[24, 1, 2, 5, 1, 2, 8, 1, 2]);
}

#[cfg(test)]
proptest::proptest! {
    #[test]
    fn generated_typed_dispatch_histories(events in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..65)) {
        exercise(&events);
    }
}
