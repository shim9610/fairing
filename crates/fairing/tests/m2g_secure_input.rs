//! Secure input — **the shell does not keep what it was shown.**
//!
//! The crate owns no secret. [`TextField::new`](fairing::widgets::TextField::new) borrows a
//! `&mut String`, and [`WifiBackend::connect`](fairing::services::WifiBackend::connect) takes the
//! PSK by `&str`, so an integrator can hold the buffer in whatever type they trust — a zeroizing
//! wrapper, say — and hand it straight to the backend without it passing through the shell.
//!
//! That leaves the crate exactly one obligation, and it is one the caller cannot discharge from
//! outside: **the widget must not keep a copy of what it drew.** egui feeds the raw text into the
//! `TextEdit` undoer every frame whatever `password` says (`text_edit/builder.rs` calls
//! `feed_state` with `text.as_str().to_owned()` on both sides of input handling). The clipboard is
//! guarded there by `copy_if_not_password` and the accessibility text by `mask_if_password`; the
//! undoer is not. Those snapshots — up to `Undoer::max_undos`, a hundred of them — live in egui's
//! memory under the widget's id and outlive the field, out of reach of anything the caller wraps
//! its own buffer in.
//!
//! The rules are the other integration tests': written as `fairing::Result<()>` with no `panic!`
//! and no `unwrap`.

#![cfg(feature = "mock")]

use fairing::screen::{screen, Cx};
use fairing::testing::{single_level_access, Harness};
use fairing::widgets::TextField;
use fairing::{LaunchAction, Shell};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What one field left behind after being typed into.
struct Typed {
    /// Whether any undo snapshot survived under the widget's id.
    undo_history: bool,
    /// What the caller's buffer holds — it is the caller's, so the widget must have filled it.
    text: String,
}

/// It taps one field, types into it, and waits past `Undoer::stable_time` (1 s) so egui has had
/// every chance to save an undo point. `password` picks which field is drawn.
fn type_into_a_field(password: bool) -> fairing::Result<Typed> {
    let text = Rc::new(RefCell::new(String::new()));
    let place = Rc::new(Cell::new(None::<(egui::Id, egui::Rect)>));
    let (buffer, sink) = (Rc::clone(&text), Rc::clone(&place));

    let mut h = Harness::from_builder(move |ctx| {
        let mut shell = Shell::builder(single_level_access())
            .services(fairing::services::Services::null())
            .build(ctx)?;
        shell.add(screen(
            "field",
            move |ui: &mut egui::Ui, cx: &mut Cx<'_>| {
                let mut held = buffer.borrow_mut();
                let response = TextField::new(&mut held)
                    .password(password)
                    .show(ui, &mut cx.widgets());
                sink.set(Some((response.id, response.rect)));
            },
        ));
        Ok(shell)
    })?;

    h.shell.launch(LaunchAction::open("field"));
    // The open transition shields the pane, so the tap has to wait for it.
    for _ in 0..120 {
        h.frame();
        if !h.shell.is_animating() {
            break;
        }
    }
    let Some((id, rect)) = place.get() else {
        return Err(fairing::Error::Config("the field was not drawn".to_owned()));
    };
    h.tap(rect.center());
    h.frames(2);
    h.type_text("hunter2");
    h.frames(2);
    // An undo point is saved once the text has been still for `Undoer::stable_time`.
    h.run_for(1.5);

    // A state that cannot equal a stored snapshot, so `has_undo` answers "is there **any**".
    let sentinel = (
        egui::text::CCursorRange::default(),
        "\u{0}not a snapshot".to_owned(),
    );
    let undo_history = egui::text_edit::TextEditState::load(&h.ctx, id)
        .is_some_and(|state| state.undoer().has_undo(&sentinel));
    let text = text.borrow().clone();
    Ok(Typed { undo_history, text })
}

/// **A password field leaves no undo history.**
///
/// The plain field is the control: without it, a version of this test where the typing never
/// arrived — no focus, no events, the wrong id — would pass while proving nothing.
#[test]
fn a_password_field_keeps_no_plaintext_in_egui_memory() -> fairing::Result<()> {
    let plain = type_into_a_field(false)?;
    assert_eq!(
        plain.text, "hunter2",
        "the typing never reached the field, so this test proves nothing"
    );
    assert!(
        plain.undo_history,
        "the control field saved no undo point either - the wait did not reach `stable_time`, \
         so the password case below would pass for the wrong reason"
    );

    let secret = type_into_a_field(true)?;
    assert_eq!(
        secret.text, "hunter2",
        "the password field did not take input"
    );
    assert!(
        !secret.undo_history,
        "the plaintext is still in egui's undo history under the widget's id - it outlives the \
         field and no wrapper round the caller's buffer can reach it"
    );
    Ok(())
}
