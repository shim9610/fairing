// Pure acknowledgement decision shared by the shell and the assurance harnesses.
// Keep backend reads and command dispatch in Shell so their order stays explicit.

pub(crate) enum Decision {
    Complete,
    Keep(bool),
    Send(bool),
}

#[allow(clippy::fn_params_excessive_bools)] // The finite protocol state and backend observations.
pub(crate) fn reconcile(
    sent: bool,
    desired: bool,
    was_failed: bool,
    reported: Option<bool>,
    failed: bool,
) -> Decision {
    if reported == Some(sent) {
        if desired == sent {
            Decision::Complete
        } else {
            Decision::Send(desired)
        }
    } else if !failed || was_failed {
        Decision::Keep(failed)
    } else {
        Decision::Complete
    }
}
