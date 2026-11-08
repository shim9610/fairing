//! Owned-operation vocabulary for trusted host backend commands.

/// A concrete backend command from the trusted ordinary-shell host.
///
/// This replaces mutable access to the shell's entire backend store. It has the
/// same trusted-host authority as installing backends: it does not authorize user
/// requests. User-driven settings must use `Shell::set_setting` or admitted `Cx`
/// operations. `ProtectedShell` deliberately exposes no service command method.
/// Borrowed credentials are used synchronously and are not retained or formatted.
#[non_exhaustive]
pub enum ServiceRequest<'a> {
    /// Set UTC seconds on the clock backend.
    SetTime(u64),
    /// Request power management.
    Power(super::PowerRequest),
    /// Enable or disable Wi-Fi directly as the trusted host.
    WifiEnabled(bool),
    /// Start Wi-Fi discovery.
    WifiScan,
    /// Connect to a network.
    WifiConnect {
        /// Network name.
        ssid: &'a str,
        /// Optional network credential.
        psk: Option<&'a str>,
        /// Whether the network is hidden.
        hidden: bool,
    },
    /// Disconnect Wi-Fi.
    WifiDisconnect,
    /// Forget a saved network.
    WifiForget(&'a str),
    /// Enable or disable Bluetooth directly as the trusted host.
    BluetoothEnabled(bool),
    /// Start or stop Bluetooth discovery.
    BluetoothDiscovering(bool),
    /// Start Bluetooth pairing.
    BluetoothPair(&'a str),
    /// Answer a pairing request.
    BluetoothRespondPairing {
        /// Accept or reject the request.
        accept: bool,
        /// Optional pairing code.
        passkey: Option<u32>,
    },
    /// Connect or disconnect a Bluetooth device.
    BluetoothConnect {
        /// Device address.
        addr: &'a str,
        /// Desired connection state.
        connected: bool,
    },
    /// Remove a Bluetooth device.
    BluetoothRemove(&'a str),
    /// Set display brightness directly as the trusted host.
    DisplayBrightness(u8),
    /// Set display power.
    DisplayPower(bool),
    /// Set audio volume directly as the trusted host.
    AudioVolume(u8),
    /// Set audio mute directly as the trusted host.
    AudioMuted(bool),
    /// Play an audio cue.
    AudioCue(super::Cue),
    /// Configure an interface.
    NetworkConfigure {
        /// Interface name.
        iface: &'a str,
        /// Address configuration.
        config: super::IpConfig,
    },
    /// Set the network hostname.
    NetworkHostname(&'a str),
}
