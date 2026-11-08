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
    /// A hidden-entry trigger completed, whether or not its gate passes.
    /// Applications can record this event through their own audit/logging path.
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
    /// `QuickTile::long_press` (feature `overlay`) the shell has already opened it —
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
    /// The event supplies SSID, security and saved-profile indicators.
    /// [`WifiBackend::connect`](crate::services::WifiBackend::connect) borrows a PSK
    /// from the application; any credential retention by the backend remains its
    /// responsibility.
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
    ///             let _ = shell.request_service(fairing::services::ServiceRequest::WifiConnect { ssid: &ssid, psk: None, hidden: false });
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
    /// Token mode detaches local session authority even without a built-in prompt.
    /// In legacy mode without a prompt, the host handles explicit lock requests;
    /// idle lock still resets the legacy subject. Device locking, screensavers
    /// and physical interlocks remain application/backend operations.
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
    /// arrive here. Token mode detaches local authority. Legacy prompt mode with an
    /// authenticator resets to `[access] initial`; legacy routing mode leaves the
    /// host to apply its logout policy.
    LogoutRequested,
    /// **The recent-screens control was used**.
    ///
    /// The nav bar's `"recents"` item and
    /// [`LaunchAction::OpenOverview`](crate::LaunchAction::OpenOverview) emit this when dispatched.
    /// With `[workspace] overview` on (the default) the shell's own overview opens too,
    /// behind the `nav.recents` gate — off, for a device with an overview of its own,
    /// what they open is yours.
    OverviewRequested,
    /// **The split control was used**.
    ///
    /// The nav bar's `"split"` item, the split tile and
    /// [`LaunchAction::ToggleSplit`](crate::LaunchAction::ToggleSplit) emit this when dispatched; with
    /// `[workspace] split` on (the default) the shell also splits, behind the `workspace.split`
    /// gate — or goes back to one pane.
    SplitRequested,
    /// Two panes came up (`true`), or the workspace went back to one (`false`).
    SplitToggled(bool),
    /// The emergency gesture completed (M2). If the gate passes the shade opened;
    /// otherwise `Access(UnlockRequested { gate: "chrome.emergency", .. })` comes with it.
    Emergency,
}
