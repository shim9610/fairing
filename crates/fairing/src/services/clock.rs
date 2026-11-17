//! Two clock backends — the default [`SystemClock`], and `ChronoClock` behind feature
//! `chrono`.
//!
//! `SystemClock` uses `SystemTime` plus a configured UTC offset. The offset does
//! not change automatically for daylight-saving transitions. `ChronoClock` uses
//! the system's local time-zone configuration through the optional `jiff` dependency.

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
/// Each read uses the local offset reported through jiff, including the
/// daylight-saving rules available in the system configuration.
/// The type and `chrono` feature names are retained for compatibility.
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
        local_wall_time(&jiff::Zoned::now())
    }
}

#[cfg(feature = "chrono")]
fn local_wall_time(local: &jiff::Zoned) -> WallTime {
    WallTime {
        utc_secs: u64::try_from(local.timestamp().as_second()).unwrap_or(0),
        // Preserve fractional-hour zones in the public minute-based representation.
        offset_min: local.offset().seconds() / 60,
    }
}

#[cfg(all(test, feature = "chrono"))]
mod chrono_tests {
    use super::{local_wall_time, ChronoClock, ClockSource};

    #[test]
    fn local_wall_time_preserves_dst_and_fractional_hour_offsets(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for (text, expected) in [
            ("2026-01-15T12:00:00-05:00[America/New_York]", -300),
            ("2026-07-15T12:00:00-04:00[America/New_York]", -240),
            ("2026-01-15T12:00:00+05:45[Asia/Kathmandu]", 345),
        ] {
            let local: jiff::Zoned = text.parse()?;
            let wall = local_wall_time(&local);
            assert_eq!(wall.utc_secs, u64::try_from(local.timestamp().as_second())?);
            assert_eq!(wall.offset_min, expected, "{text}");
        }
        Ok(())
    }

    #[test]
    fn local_wall_time_clamps_pre_epoch_time_to_zero() -> Result<(), jiff::Error> {
        let local: jiff::Zoned = "1969-12-31T23:59:59+00:00[UTC]".parse()?;
        assert_eq!(local_wall_time(&local).utc_secs, 0);
        Ok(())
    }

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
