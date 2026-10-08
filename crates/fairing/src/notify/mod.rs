//! Notifications, toasts and heads-up banners (A6).
//!
//! - A [`Notification`] is sent with [`crate::ShellHandle::notify`] from any thread (mpsc). The
//!   shell picks it up in stage 2 of the frame, puts it in the [`NotificationCenter`], and if it
//!   is new shows it briefly as a heads-up banner, subject to `[notify] heads_up`.
//! - A [`Toast`] goes [`crate::ShellHandle::toast`] → the toast queue. No gate.
//! - Notifications live **in memory only**. A restart clears them.

mod center;
mod heads_up;
mod hooks;
mod model;
mod toast;

pub use center::{shows_content, CenterChange, NotificationCenter};
pub(crate) use heads_up::HeadsUpAction;
#[doc(hidden)]
pub use heads_up::{HeadsUp, HeadsUpPhase};
pub use hooks::{
    HeadsUpCx, HeadsUpLayout, HeadsUpLayoutCx, HeadsUpPainter, ToastCx, ToastLayout, ToastLayoutCx,
    ToastPainter,
};
pub use model::{Level, Notification, NotificationId, Toast};
pub(crate) use toast::ToastAction;
#[doc(hidden)]
pub use toast::{ActiveToast, ToastPhase, ToastQueue};
