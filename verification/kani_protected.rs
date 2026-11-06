#[path = "../crates/fairing/src/access/protected_dispatch.rs"]
mod protected_dispatch;

#[kani::proof]
fn callback_entry_requires_token_mode_and_current_permission() {
    let token_mode: bool = kani::any();
    let permission: bool = kani::any();
    let allowed = protected_dispatch::admitted(token_mode, permission);
    let mut called = false;
    let output = protected_dispatch::invoke(allowed, || {
        called = true;
        17u8
    });
    assert_eq!(called, token_mode && permission);
    assert_eq!(
        output,
        if token_mode && permission {
            Some(17)
        } else {
            None
        }
    );
}

#[kani::proof]
fn denied_callback_preserves_effect_state_and_exposes_no_payload() {
    let before: u64 = kani::any();
    let secret: u64 = kani::any();
    let mut effect = before;
    let output = protected_dispatch::invoke(false, || {
        effect = secret;
        secret
    });
    assert_eq!(effect, before);
    assert_eq!(output, None);
}

#[kani::proof]
fn admitted_callback_executes_exactly_once_and_forwards_its_result() {
    let value: u64 = kani::any();
    let mut calls = 0u8;
    let output = protected_dispatch::invoke(true, || {
        calls += 1;
        value
    });
    assert_eq!(calls, 1);
    assert_eq!(output, Some(value));
}

#[kani::proof]
fn retaining_an_element_does_not_retain_an_earlier_admission() {
    let first: bool = kani::any();
    let second: bool = kani::any();
    let mut calls = 0u8;
    protected_dispatch::invoke(protected_dispatch::admitted(true, first), || calls += 1);
    protected_dispatch::invoke(protected_dispatch::admitted(true, second), || calls += 1);
    assert_eq!(calls, u8::from(first) + u8::from(second));
}
