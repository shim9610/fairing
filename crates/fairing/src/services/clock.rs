//! Two clock backends — the default [`SystemClock`], and `ChronoClock` behind feature
//! `chrono`.
//!
//! `SystemClock` uses `SystemTime` plus a configured UTC offset. The offset does
//! not change automatically for daylight-saving transitions. `ChronoClock` uses
//! the system's local time-zone configuration through the optional `chrono` dependency.

use super::{Backend, Capabilities, ClockSource, WallTime};
use std::time::{SystemTime, UNIX_EPOCH};

/// The system clock. No DST — the offset is a configured value.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock {
    /// The local offset, in minutes.
    pub offset_min: i32,
}

impl SystemClock {
    /// With a given offset.
    #[must_use]
    pub fn with_offset(offset_min: i32) -> Self {
        Self { offset_min }
    }
}

impl Backend for SystemClock {}

impl ClockSource for SystemClock {
    fn now(&self) -> WallTime {
        let utc_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        WallTime {
            utc_secs,
            offset_min: self.offset_min,
        }
    }
}

/// No capabilities (the time cannot be set).
impl SystemClock {
    /// The capabilities.
    #[must_use]
    pub fn caps() -> Capabilities {
        Capabilities::NONE
    }
}

/// A clock using the system's local time zone (feature `chrono`).
///
/// Each read uses the local offset reported through chrono, including the
/// daylight-saving rules available in the system configuration.
///
/// ```no_run
/// # #[cfg(feature = "chrono")] {
/// use fairing::services::clock::ChronoClock;
/// let clock = ChronoClock::new();
/// # let _ = clock;
/// # }
/// ```
#[cfg(feature = "chrono")]
#[derive(Debug, Clone, Copy, Default)]
pub struct ChronoClock;

#[cfg(feature = "chrono")]
impl ChronoClock {
    /// Build one.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// The capabilities. Setting the time is not among them — touching the system clock is a
    /// privilege question, so the integrator does it in their own backend.
    #[must_use]
    pub fn caps() -> Capabilities {
        Capabilities::NONE
    }
}

#[cfg(feature = "chrono")]
impl Backend for ChronoClock {}

#[cfg(feature = "chrono")]
impl ClockSource for ChronoClock {
    fn now(&self) -> WallTime {
        use chrono::{Local, Offset, Utc};
        let utc = Utc::now().timestamp();
        // `local_minus_utc` is in seconds. `WallTime::offset_min` is in minutes, so it is divided by 60 —
        // there are 30- and 45-minute time zones (India +5:30, Nepal +5:45), so it cannot be written in hours.
        let offset_sec = Local::now().offset().fix().local_minus_utc();
        WallTime {
            utc_secs: u64::try_from(utc).unwrap_or(0),
            offset_min: offset_sec / 60,
        }
    }
}

#[cfg(all(test, feature = "chrono"))]
mod chrono_tests {
    use super::{ChronoClock, ClockSource};

    /// Check that the system-provided local offset is within the supported range.
    #[test]
    fn the_chrono_clock_reads_the_system_zone() {
        let now = ChronoClock::new().now();
        // After 2001-09-09 (a billion seconds into the epoch). On a device whose clock has not been set, this test catches it.
        assert!(now.utc_secs > 1_000_000_000, "utc_secs = {}", now.utc_secs);
        // The range of real time-zone offsets: UTC−12:00 … UTC+14:00.
        assert!(
            (-720..=840).contains(&now.offset_min),
            "offset_min = {}",
            now.offset_min
        );
    }
}
