//! `ShellHandle` — the handle for sending the shell commands from any thread.
//!
//! Inside it is an `mpsc::Sender<Command>` plus a [`Waker`]. `Shell` owns the receiving end as an
//! [`Inbox`](crate::inbox::Inbox) and drains it in stage 2 of the frame — the type has no blocking
//! call to reach for. No locks.

use crate::access::{Gate, Subject};
use crate::desktop::Badge;
use crate::inbox::Inbox;
use crate::notify::{Notification, NotificationId, Toast};
use crate::screen::LaunchAction;
use crate::services::Waker;
use crate::settings::{SettingKey, SettingValue};
use crate::theme::MotionTokens;
use crate::workspace::InstanceId;
use std::sync::mpsc::Sender;

/// A shell command. All of them are `Send`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Command {
    /// Launch (the gate is checked by `Shell::launch`).
    Launch(LaunchAction),
    /// Close an instance.
    CloseScreen(InstanceId),
    /// Back.
    Back,
    /// Home.
    Home,
    /// For an integrator applying the result of their own authentication.
    SetSubject(Subject),
    /// Log out — the same as [`LaunchAction::Logout`]: always reported, and carried out (back to
    /// the starting subject) where the shell draws its own prompt.
    Logout,
    /// An icon badge.
    SetBadge {
        /// The declaration id.
        id: String,
        /// `None` clears it.
        badge: Option<Badge>,
    },
    /// Write a setting (the shell checks the key's gate), as
    /// [`Shell::set_setting`](crate::Shell::set_setting) does: the in-memory view, the backend of a
    /// built-in key, and `ShellEvent::SettingChanged` for the integrator to store.
    SetSetting {
        /// The key.
        key: SettingKey,
        /// The value.
        value: SettingValue,
    },
    /// Request an unlock: `AccessEvent::UnlockRequested`, and the shell's prompt
    /// where it draws one.
    RequestUnlock {
        /// The gate.
        gate: Gate,
        /// What to run once authenticated.
        then: Option<LaunchAction>,
    },
    /// A notification (M2). The same id updates.
    Notify(Box<Notification>),
    /// A toast (M2).
    Toast(Box<Toast>),
    /// Dismiss a notification (M2). Persistent ones are ignored.
    DismissNotification(NotificationId),
    /// Toggle the shade (M2). Opening goes through the gate as `Launch(OpenOverlay)`.
    ToggleOverlay,
    /// Replace the motion tokens (M2, tuning from `examples/motion_lab.rs`). From the next frame.
    SetMotion(Box<MotionTokens>),
}

/// A `Clone + Send` handle.
#[derive(Debug, Clone)]
pub struct ShellHandle {
    tx: Sender<Command>,
    waker: Waker,
}

impl ShellHandle {
    /// Make a channel pair and return the handle and the receiving end. Called by `Shell::new`.
    ///
    /// The receiving end is an [`Inbox`](crate::inbox::Inbox) rather than a bare
    /// `std::sync::mpsc::Receiver`: the UI thread drains it every frame and must never wait on it.
    #[must_use]
    pub(crate) fn pair(waker: Waker) -> (Self, Inbox<Command>) {
        let (tx, rx) = Inbox::pair();
        (Self { tx, waker }, rx)
    }

    /// Send a command and wake the UI. `false` if the shell is gone.
    #[must_use]
    pub fn send(&self, command: Command) -> bool {
        let ok = self.tx.send(command).is_ok();
        self.waker.wake();
        ok
    }

    fn post(&self, command: Command) {
        if !self.send(command) {
            log::warn!("ShellHandle: the shell is gone, dropping the command");
        }
    }

    /// Launch (the shell checks the gate).
    pub fn launch(&self, action: LaunchAction) {
        self.post(Command::Launch(action));
    }

    /// Close an instance.
    pub fn close_screen(&self, id: InstanceId) {
        self.post(Command::CloseScreen(id));
    }

    /// Back.
    pub fn back(&self) {
        self.post(Command::Back);
    }

    /// Home.
    pub fn home(&self) {
        self.post(Command::Home);
    }

    /// Replace the subject.
    pub fn set_subject(&self, subject: Subject) {
        self.post(Command::SetSubject(subject));
    }

    /// Log out — [`LaunchAction::Logout`] from any thread.
    pub fn logout(&self) {
        self.post(Command::Logout);
    }

    /// A badge.
    pub fn set_badge(&self, id: impl Into<String>, badge: Option<Badge>) {
        self.post(Command::SetBadge {
            id: id.into(),
            badge,
        });
    }

    /// Write a setting. The shell checks the gate (`Shell::launch(LaunchAction::Set)`).
    pub fn set_setting(&self, key: impl Into<SettingKey>, value: SettingValue) {
        self.post(Command::SetSetting {
            key: key.into(),
            value,
        });
    }

    /// Request an unlock: [`AccessEvent::UnlockRequested`](crate::access::AccessEvent::UnlockRequested)
    /// goes out, and in `prompt` mode with an authenticator the shell's prompt opens; a grant runs
    /// `then` through the gate.
    pub fn request_unlock(&self, gate: impl Into<Gate>, then: Option<LaunchAction>) {
        self.post(Command::RequestUnlock {
            gate: gate.into(),
            then,
        });
    }

    /// A notification, from any thread. A new one shows briefly as a heads-up and then goes into the shade.
    pub fn notify(&self, notification: Notification) {
        self.post(Command::Notify(Box::new(notification)));
    }

    /// A toast, from any thread. Either `"text"` or a [`Toast`].
    pub fn toast(&self, toast: impl Into<Toast>) {
        self.post(Command::Toast(Box::new(toast.into())));
    }

    /// Dismiss a notification.
    pub fn dismiss_notification(&self, id: NotificationId) {
        self.post(Command::DismissNotification(id));
    }

    /// Toggle the shade (opening goes through the `overlay.open` gate).
    pub fn toggle_overlay(&self) {
        self.post(Command::ToggleOverlay);
    }

    /// Replace the motion tokens (the tuning tool). Animations in flight are left alone.
    pub fn set_motion(&self, tokens: MotionTokens) {
        self.post(Command::SetMotion(Box::new(tokens)));
    }

    /// The wake handle (hand it to backend and scenario threads).
    #[must_use]
    pub fn waker(&self) -> &Waker {
        &self.waker
    }
}

#[cfg(test)]
mod tests {
    use super::ShellHandle;

    fn assert_send<T: Send>() {}

    #[test]
    fn handle_is_send_and_clone() {
        assert_send::<ShellHandle>();
    }
}
