//! The `Null*` backends — everything is `Unsupported`. The default for a backend you did not specify.

use super::{
    AudioBackend, AudioSnapshot, Backend, BatterySnapshot, BluetoothBackend, BtSnapshot,
    ClockSource, DeviceInfo, DisplayBackend, DisplayInfo, IfaceSnapshot, InfoBackend,
    NetworkBackend, PowerBackend, PowerRequest, ServiceError, ServiceResult, WallTime, WifiBackend,
    WifiSnapshot,
};

/// A clock fixed at epoch 0. For headless tests that need a deterministic value.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullClock;

impl Backend for NullClock {}

impl ClockSource for NullClock {
    fn now(&self) -> WallTime {
        WallTime::default()
    }
}

/// No battery, no power control.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullPower;

impl Backend for NullPower {}

impl PowerBackend for NullPower {
    fn battery(&self) -> Option<BatterySnapshot> {
        None
    }

    fn request(&mut self, _req: PowerRequest) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

/// No Wi-Fi.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullWifi;

impl Backend for NullWifi {}

impl WifiBackend for NullWifi {
    fn snapshot(&self) -> WifiSnapshot {
        WifiSnapshot::default()
    }

    fn enabled(&self) -> bool {
        false
    }

    fn strength(&self) -> u8 {
        0
    }

    fn set_enabled(&mut self, _on: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn scan(&mut self) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn connect(&mut self, _ssid: &str, _psk: Option<&str>, _hidden: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn disconnect(&mut self) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn forget(&mut self, _ssid: &str) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

/// No Bluetooth.
#[derive(Debug, Clone, Copy, Default)]
pub struct NullBluetooth;

impl Backend for NullBluetooth {}

impl BluetoothBackend for NullBluetooth {
    fn snapshot(&self) -> BtSnapshot {
        BtSnapshot::default()
    }

    fn enabled(&self) -> bool {
        false
    }

    fn any_connected(&self) -> bool {
        false
    }

    fn set_enabled(&mut self, _on: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn set_discovering(&mut self, _on: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn pair(&mut self, _addr: &str) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn respond_pairing(&mut self, _accept: bool, _passkey: Option<u32>) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn connect(&mut self, _addr: &str, _on: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn remove(&mut self, _addr: &str) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

/// No display (M2). Requests to inhibit idle are dropped.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullDisplay;

impl Backend for NullDisplay {}

impl DisplayBackend for NullDisplay {
    fn brightness(&self) -> Option<u8> {
        None
    }

    fn set_brightness(&mut self, _v: u8) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn set_idle_inhibit(&mut self, _on: bool) {}

    fn set_power(&mut self, _on: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn info(&self) -> DisplayInfo {
        DisplayInfo::default()
    }
}

/// No audio output: the level unknown, every command `Unsupported`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullAudio;

impl Backend for NullAudio {}

impl AudioBackend for NullAudio {
    fn volume(&self) -> Option<AudioSnapshot> {
        None
    }

    fn set_volume(&mut self, _level: u8) -> ServiceResult {
        Err(ServiceError::unsupported())
    }

    fn set_muted(&mut self, _muted: bool) -> ServiceResult {
        Err(ServiceError::unsupported())
    }
}

/// No interfaces known, no hostname.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullNetwork;

impl Backend for NullNetwork {}

impl NetworkBackend for NullNetwork {
    fn interfaces(&self) -> Vec<IfaceSnapshot> {
        Vec::new()
    }

    fn ethernet_up(&self) -> Option<bool> {
        None
    }
}

/// Nothing known about the device.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullInfo;

impl Backend for NullInfo {}

impl InfoBackend for NullInfo {
    fn device(&self) -> DeviceInfo {
        DeviceInfo::default()
    }
}
