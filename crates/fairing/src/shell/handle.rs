//! `ShellHandle` — the handle for sending the shell commands from any thread.
//!
//! A bounded sender plus a [`Waker`]. Both transport endpoints expose only immediate
//! send/receive operations. Queue capacity and per-update processing are builder choices.

use crate::access::{Gate, Subject};
use crate::desktop::Badge;
use crate::inbox::{
    BoundedInbox, BoundedSender, Inbox, QueueCapacityError, QueueErrorKind, QueueStats,
    TryBuildSendError, TrySendError,
};
use crate::notify::{Notification, NotificationId, Toast, ToastInput};
use crate::screen::LaunchAction;
use crate::services::Waker;
use crate::settings::{SettingKey, SettingValue};
use crate::theme::MotionTokens;
use crate::workspace::InstanceId;
use std::sync::{
    atomic::{AtomicU8, Ordering},
    Arc,
};

/// A shell command. All of them are `Send`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[allow(
    clippy::large_enum_variant,
    reason = "bounded transport stores payloads inline to avoid infallible Box allocation on send"
)]
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
    Notify(Notification),
    /// A toast (M2).
    Toast(Toast),
    /// Dismiss a notification (M2). Persistent ones are ignored.
    DismissNotification(NotificationId),
    /// Toggle the shade (M2). Opening goes through the gate as `Launch(OpenOverlay)`.
    ToggleOverlay,
    /// Replace the motion tokens (M2, tuning from `examples/motion_lab.rs`). From the next frame.
    SetMotion(MotionTokens),
}

/// A `Clone + Send` handle.
#[derive(Debug, Clone)]
pub struct ShellHandle {
    tx: BoundedSender<Command>,
    waker: Waker,
    display_omissions: Arc<AtomicU8>,
}

impl ShellHandle {
    /// Make a channel pair and return the handle and the receiving end. Called by `Shell::new`.
    ///
    /// The receiving end is an [`Inbox`](crate::inbox::Inbox) rather than a bare
    /// `std::sync::mpsc::Receiver`: the UI thread drains it every frame and must never wait on it.
    pub(crate) fn pair(
        waker: Waker,
        capacity: usize,
    ) -> Result<(Self, BoundedInbox<Command>), QueueCapacityError> {
        let (tx, rx) = Inbox::bounded(capacity)?;
        Ok((
            Self {
                tx,
                waker,
                display_omissions: Arc::new(AtomicU8::new(0)),
            },
            rx,
        ))
    }

    /// Send a command and wake the UI. `false` if full or the shell is gone.
    /// Use `try_send` to distinguish rejection and recover the command.
    #[must_use]
    pub fn send(&self, command: Command) -> bool {
        self.try_send(command).is_ok()
    }

    /// Send without waiting or evicting another command.
    ///
    /// # Errors
    /// Full or disconnected; the error owns the original command.
    #[allow(
        clippy::result_large_err,
        reason = "return rejected ownership inline; boxing an allocation failure would allocate again"
    )]
    pub fn try_send(&self, command: Command) -> Result<(), TrySendError<Command>> {
        let result = self.tx.try_send(command);
        if result.is_ok() {
            self.waker.wake();
        }
        result
    }

    /// Reserve capacity before constructing a command's variable-sized payload.
    ///
    /// # Errors
    /// Full or disconnected. See `BoundedSender::try_send_with` for concurrent close semantics.
    pub fn try_send_with(&self, make: impl FnOnce() -> Command) -> Result<(), QueueErrorKind> {
        let result = self.tx.try_send_with(make);
        if result.is_ok() {
            self.waker.wake();
        }
        result
    }

    /// Reserve a command slot before a fallible payload factory runs.
    /// A failed factory releases its reservation without logging or waking the UI.
    ///
    /// # Errors
    /// Queue full/closed or the factory's own error. A rejected factory is not invoked.
    pub fn try_send_fallible_with<E>(
        &self,
        make: impl FnOnce() -> Result<Command, E>,
    ) -> Result<(), TryBuildSendError<E>> {
        let result = self.tx.try_send_fallible_with(make);
        if result.is_ok() {
            self.waker.wake();
        }
        result
    }

    /// Send text using fallible bounded construction, without a per-message Box.
    /// A full command queue skips text allocation. Admission is not display completion.
    ///
    /// # Errors
    /// Queue full/closed, oversized text, or failed String allocation.
    pub fn try_toast(
        &self,
        text: &str,
    ) -> Result<(), TryBuildSendError<crate::notify::AdmissionError>> {
        let result = self
            .tx
            .try_send_fallible_with(|| Toast::try_new(text).map(Command::Toast));
        if result.is_ok() {
            self.waker.wake();
        } else {
            let bits = if matches!(
                result,
                Err(TryBuildSendError::Build(
                    crate::notify::AdmissionError::AllocationFailed
                ))
            ) {
                3
            } else {
                1
            };
            self.display_omissions.fetch_or(bits, Ordering::Relaxed);
        }
        result
    }

    pub(super) fn display_status(&self, acknowledge: bool) -> crate::memory::DisplayStatus {
        let bits = if acknowledge {
            self.display_omissions.swap(0, Ordering::Relaxed)
        } else {
            self.display_omissions.load(Ordering::Relaxed)
        };
        crate::memory::DisplayStatus {
            discarded: usize::from(bits & 1),
            allocation_failed: bits & 2 != 0,
        }
    }

    /// Queue capacity, outstanding reservations, high-water mark and full rejections.
    #[must_use]
    pub fn queue_stats(&self) -> QueueStats {
        self.tx.stats()
    }

    fn post(&self, command: Command) {
        // Rejection must not call an application logger that may allocate under pressure.
        // Try APIs and queue_stats expose refusal to hosts that need diagnostics/retry.
        let _ = self.try_send(command);
    }

    fn post_with(&self, make: impl FnOnce() -> Command) {
        let _ = self.try_send_with(make);
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
        self.post_with(|| Command::SetBadge {
            id: id.into(),
            badge,
        });
    }

    /// Write a setting. The shell checks the gate (`Shell::launch(LaunchAction::Set)`).
    pub fn set_setting(&self, key: impl Into<SettingKey>, value: SettingValue) {
        self.post_with(|| Command::SetSetting {
            key: key.into(),
            value,
        });
    }

    /// Request an unlock: [`AccessEvent::UnlockRequested`](crate::access::AccessEvent::UnlockRequested)
    /// goes out, and in `prompt` mode with an authenticator the shell's prompt opens; a grant runs
    /// `then` through the gate.
    pub fn request_unlock(&self, gate: impl Into<Gate>, then: Option<LaunchAction>) {
        self.post_with(|| Command::RequestUnlock {
            gate: gate.into(),
            then,
        });
    }

    /// A notification, from any thread. A new one shows briefly as a heads-up and then goes into the shade.
    pub fn notify(&self, notification: Notification) {
        self.post_with(|| Command::Notify(notification));
    }

    /// A toast, from any thread. Either `"text"` or a [`Toast`].
    pub fn toast<'a>(&self, toast: impl Into<ToastInput<'a>>) {
        match toast.into() {
            ToastInput::Text(text) => {
                // Reporting through a user logger could allocate after an OOM rejection.
                // Call try_toast when the producer needs the admission error.
                let _ = self.try_toast(text);
            }
            ToastInput::Value(toast) => {
                if self.try_send(Command::Toast(toast)).is_err() {
                    self.display_omissions.fetch_or(1, Ordering::Relaxed);
                }
            }
        }
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
        self.post_with(|| Command::SetMotion(tokens));
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
