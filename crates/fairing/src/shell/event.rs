//! `ShellEvent` — what the shell tells the integrator. Collected with `Shell::poll_events`.

use crate::access::AccessEvent;
use crate::notify::NotificationId;
use crate::services::PowerRequest;
use crate::settings::{SettingKey, SettingValue};
use crate::workspace::InstanceId;

/// A shell event.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ShellEvent {
    /// A screen opened.
    ScreenOpened {
        /// The declaration id.
        id: String,
        /// The instance.
        instance: InstanceId,
    },
    /// A screen closed (pop, remove, or the task ending).
    ScreenClosed {
        /// The declaration id.
        id: String,
        /// The instance.
        instance: InstanceId,
    },
    /// Went home.
    WentHome,
    /// **A hidden entry point opened**. It fires the moment the knock completes,
    /// whether or not the gate passes — entering a service menu is auditable and must not go by
    /// quietly.
    ///
    /// If there is a gate and it fails,
    /// [`AccessEvent::UnlockRequested`](crate::access::AccessEvent::UnlockRequested) follows
    /// **after** this event.
    HiddenEntry {
        /// The entry point id.
        id: String,
    },
    /// **A gesture on a gesture handle completed** — see
    /// [`GestureHandle`](crate::gesture::GestureHandle). Like [`Self::HiddenEntry`], it fires
    /// whether or not the gate passes; where the gate fails,
    /// [`AccessEvent::UnlockRequested`](crate::access::AccessEvent::UnlockRequested) follows.
    Gesture {
        /// The handle's id.
        handle: String,
        /// Which gesture.
        gesture: crate::gesture::HandleGesture,
    },
    /// Access control.
    Access(AccessEvent),
    /// The integrator's chance to clean up before a power request (M4).
    PowerRequest(PowerRequest),
    /// A setting changed — the integrator's cue to store it, since the shell writes no files.
    /// [`Shell::restore_settings`](crate::Shell::restore_settings) hands stored values back.
    SettingChanged {
        /// The key.
        key: SettingKey,
        /// The value.
        value: SettingValue,
    },
    /// A declaration was removed.
    DeclRemoved(String),
    /// A notification was tapped (M2). The shell has already `launch`ed the notification's `action`.
    NotificationTapped(NotificationId),
    /// A notification was dismissed (M2, by the user or `dismiss_notification`).
    NotificationDismissed(NotificationId),
    /// The shade opened (true) or closed (false) (M2, measured at the `Closed` boundary).
    OverlayToggled(bool),
    /// The OSK was shown (true) or hidden (false) (M2, measured on the target).
    OskToggled(bool),
    /// A quick-settings tile was **long-pressed** (M2). If the tile carries a
    /// [`long_press`](crate::overlay::QuickTile::long_press) the shell has already opened it —
    /// take this event to do anything else (a menu, a dialog).
    TileLongPressed {
        /// The tile id.
        id: String,
    },
    /// A desktop icon was **long-pressed**. The shell does nothing with it — rearranging and
    /// context menus are the integrator's.
    IconLongPressed {
        /// The declaration id.
        id: String,
    },
    /// A network row on the built-in `settings.wifi` was **tapped** (M2b).
    ///
    /// **The shell does not connect — that is the integrator's.** Whether to ask for a password,
    /// what to ask with, where the profile is stored and whether a saved network should reconnect
    /// on a tap at all are policy for the device, and the shell is a drawing layer. It draws the
    /// list and reports the tap; nothing else happens.
    ///
    /// Everything needed to decide comes with the event, and
    /// [`WifiBackend::connect`](crate::services::WifiBackend::connect) takes the PSK by `&str`, so
    /// **the shell never holds the secret**: keep the buffer yourself and hand it straight over.
    ///
    /// ```no_run
    /// # use fairing::{Shell, ShellEvent};
    /// # fn ask_for_a_password(ssid: &str) {}
    /// # fn handle(shell: &mut Shell) {
    /// for event in shell.poll_events() {
    ///     if let ShellEvent::WifiNetworkTapped { ssid, secured, known } = event {
    ///         if secured && !known {
    ///             ask_for_a_password(&ssid); // …your own sheet; connect with what it collects
    ///         } else {
    ///             let _ = shell.services_mut().wifi.connect(&ssid, None, false);
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    ///
    /// Not taking this event leaves the list drawing correctly and doing nothing on a tap.
    WifiNetworkTapped {
        /// The SSID that was tapped.
        ssid: String,
        /// Whether the network is secured, so a PSK is needed.
        secured: bool,
        /// Whether the backend already reports a saved profile for it
        /// ([`WifiSnapshot::known`](crate::services::WifiSnapshot::known)).
        known: bool,
    },
    /// A network row on the built-in `settings.wifi` was **held** (M2g).
    ///
    /// The shell does nothing with it, as with [`ShellEvent::IconLongPressed`] — this is where
    /// "forget this network" goes, and forgetting is
    /// [`WifiBackend::forget`](crate::services::WifiBackend::forget), an integrator's call. The
    /// release that ends the hold does **not** also arrive as
    /// [`ShellEvent::WifiNetworkTapped`], so one press never means two things.
    WifiNetworkLongPressed {
        /// The SSID that was held.
        ssid: String,
        /// Whether the backend reports a saved profile for it — there is nothing to forget without one.
        known: bool,
    },
    /// **The panel is to lock**.
    ///
    /// `tile.lock`, the shade footer's lock button, [`LaunchAction::Lock`](crate::LaunchAction::Lock)
    /// and `[access] idle_lock_secs` running out all arrive here.
    ///
    /// **In `prompt` mode with an authenticator the shell carries it out**: the session goes back
    /// to the starting subject and its own lock screen comes up — then
    /// [`AccessEvent::LockScreenToggled`](crate::access::AccessEvent::LockScreenToggled) follows.
    /// Everywhere else the shell does not lock anything: what locking means is the device's — a
    /// lock screen of your own, a drop to a lower access level, a screensaver, a relay cut. The
    /// shell draws the control, closes the shade so whatever you do is visible, and reports. (The
    /// idle lock still takes the session back to its start; that is the timer's job.)
    ///
    /// [`ShellHandle::set_subject`](crate::ShellHandle::set_subject) is the lever if it means an
    /// access level.
    ///
    /// ```no_run
    /// # use fairing::{Shell, ShellEvent};
    /// # fn my_lock_screen(_: &mut Shell) {}
    /// # fn handle(shell: &mut Shell) {
    /// for event in shell.poll_events() {
    ///     if event == ShellEvent::LockRequested {
    ///         my_lock_screen(shell);
    ///     }
    /// }
    /// # }
    /// ```
    ///
    /// Outside `prompt` mode, not taking it leaves the control drawn and pressing it doing
    /// nothing. Where that is not what you want, take `"lock"` out of `[overlay] footer` and
    /// `tile.lock` out of the tile list — the device chooses which controls exist.
    LockRequested,
    /// **Log out was asked for**.
    ///
    /// [`LaunchAction::Logout`](crate::LaunchAction::Logout),
    /// [`ShellHandle::logout`](crate::ShellHandle::logout) and a tap on the `status.lock` padlock
    /// arrive here. In `prompt` mode with an authenticator the shell also takes the session back to
    /// the starting subject (`[access] initial`). Elsewhere it does not act: which subject "logged
    /// out" is, is the device's, and [`ShellHandle::set_subject`](crate::ShellHandle::set_subject)
    /// puts it there.
    LogoutRequested,
    /// **The recent-screens control was used**.
    ///
    /// The nav bar's `"recents"` item and
    /// [`LaunchAction::OpenOverview`](crate::LaunchAction::OpenOverview) arrive here. Always
    /// reported; with `[workspace] overview` on (the default) the shell's own overview opens too,
    /// behind the `nav.recents` gate — off, for a device with an overview of its own,
    /// what they open is yours.
    OverviewRequested,
    /// **The split control was used**.
    ///
    /// The nav bar's `"split"` item, the split tile and
    /// [`LaunchAction::ToggleSplit`](crate::LaunchAction::ToggleSplit). Always reported; with
    /// `[workspace] split` on (the default) the shell also splits, behind the `workspace.split`
    /// gate — or goes back to one pane.
    SplitRequested,
    /// Two panes came up (`true`), or the workspace went back to one (`false`).
    SplitToggled(bool),
    /// The emergency gesture completed (M2). If the gate passes the shade opened;
    /// otherwise `Access(UnlockRequested { gate: "chrome.emergency", .. })` comes with it.
    Emergency,
}
