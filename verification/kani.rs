#[path = "../crates/fairing/src/shell/bool_ack.rs"]
mod bool_ack;

#[kani::proof]
fn acknowledgement_controls_dispatch() {
    let sent: bool = kani::any();
    let desired: bool = kani::any();
    let was_failed: bool = kani::any();
    let reported: Option<bool> = kani::any();
    let failed: bool = kani::any();
    match bool_ack::reconcile(sent, desired, was_failed, reported, failed) {
        bool_ack::Decision::Send(next) => {
            assert_eq!(reported, Some(sent));
            assert_eq!(next, desired);
            assert_ne!(next, sent);
        }
        bool_ack::Decision::Keep(observed_failure) => {
            assert_ne!(reported, Some(sent));
            assert!(!failed || was_failed);
            assert_eq!(observed_failure, failed);
        }
        bool_ack::Decision::Complete => {
            assert!(
                (reported == Some(sent) && desired == sent)
                    || (reported != Some(sent) && failed && !was_failed)
            );
        }
    }
}

#[kani::proof]
fn a_preexisting_failure_cannot_reject_a_new_command() {
    let sent: bool = kani::any();
    let desired: bool = kani::any();
    let reported = if kani::any() { None } else { Some(!sent) };
    assert!(matches!(
        bool_ack::reconcile(sent, desired, true, reported, true),
        bool_ack::Decision::Keep(true)
    ));
}
