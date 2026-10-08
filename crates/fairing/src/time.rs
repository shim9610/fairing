//! Wall-clock values and date conversion with no dependencies (`time.rs`).
//!
//! The default clock is "UTC seconds plus a local offset in minutes". The offset is **a
//! configured value**, so it does not know about summer time — somewhere with DST it is an hour
//! out twice a year. A device where that matters turns on the `chrono` feature and uses
//! [`ChronoClock`](crate::services::clock::ChronoClock). It produces the same [`WallTime`], so
//! screen code does not change by a line.

use crate::i18n::Strings;

/// One wall-clock moment: UTC epoch seconds and a local offset in minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WallTime {
    /// Seconds since 1970-01-01T00:00:00Z.
    pub utc_secs: u64,
    /// Local time = UTC plus this many minutes. KST, for instance, is 540.
    pub offset_min: i32,
}

/// How the status bar clock is written (`StatusItem::Clock { format }`).
///
/// The four shapes come in a 24- and a 12-hour spelling. Which of the two is drawn is **not** fixed
/// by the config: the `ui.clock_12h` setting ([`keys::UI_CLOCK_12H`](crate::settings::keys::UI_CLOCK_12H),
/// written by the built-in `settings.datetime` screen) turns whichever shape was configured into
/// its other half through [`ClockFormat::with_hour12`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockFormat {
    /// `HH:MM` (24-hour).
    #[default]
    Hm,
    /// `HH:MM:SS`.
    Hms,
    /// `h:MM AM/PM`.
    Hm12,
    /// `MM-DD HH:MM`.
    DateHm,
    /// `h:MM:SS AM/PM` — [`ClockFormat::Hms`] on a 12-hour clock.
    Hms12,
    /// `MM-DD h:MM AM/PM` — [`ClockFormat::DateHm`] on a 12-hour clock.
    DateHm12,
}

impl ClockFormat {
    /// Whether this spelling is a 12-hour one.
    #[must_use]
    pub const fn hour12(self) -> bool {
        matches!(self, Self::Hm12 | Self::Hms12 | Self::DateHm12)
    }

    /// Whether the seconds are shown — which is what decides how often the clock has to be redrawn.
    #[must_use]
    pub const fn shows_seconds(self) -> bool {
        matches!(self, Self::Hms | Self::Hms12)
    }

    /// **The same shape on a 12- or a 24-hour clock.** What the `ui.clock_12h` setting turns.
    ///
    /// The shape — minutes, seconds, the date in front — is the integrator's choice in
    /// `[status_bar] clock_format`; the hour convention is the **device owner's**, and it can change
    /// while the shell is running.
    ///
    /// ```
    /// use fairing::time::ClockFormat;
    /// assert_eq!(ClockFormat::Hm.with_hour12(true), ClockFormat::Hm12);
    /// assert_eq!(ClockFormat::Hms12.with_hour12(false), ClockFormat::Hms);
    /// assert_eq!(ClockFormat::DateHm.with_hour12(false), ClockFormat::DateHm);
    /// ```
    #[must_use]
    pub const fn with_hour12(self, hour12: bool) -> Self {
        match (self, hour12) {
            (Self::Hm | Self::Hm12, true) => Self::Hm12,
            (Self::Hm | Self::Hm12, false) => Self::Hm,
            (Self::Hms | Self::Hms12, true) => Self::Hms12,
            (Self::Hms | Self::Hms12, false) => Self::Hms,
            (Self::DateHm | Self::DateHm12, true) => Self::DateHm12,
            (Self::DateHm | Self::DateHm12, false) => Self::DateHm,
        }
    }
}

/// A local calendar time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    /// The year.
    pub year: i64,
    /// The month (1–12).
    pub month: u8,
    /// The day (1–31).
    pub day: u8,
    /// The hour (0–23).
    pub hour: u8,
    /// The minute (0–59).
    pub minute: u8,
    /// The second (0–59).
    pub second: u8,
}

impl WallTime {
    /// The seconds with the local offset added (`i64`, so negative epochs work too).
    #[must_use]
    pub fn local_secs(&self) -> i64 {
        i64::try_from(self.utc_secs).unwrap_or(i64::MAX) + i64::from(self.offset_min) * 60
    }

    /// The local calendar time.
    #[must_use]
    pub fn civil(&self) -> Civil {
        let local = self.local_secs();
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Civil {
            year,
            month,
            day,
            hour: u8::try_from(secs / 3600).unwrap_or(0),
            minute: u8::try_from((secs % 3600) / 60).unwrap_or(0),
            second: u8::try_from(secs % 60).unwrap_or(0),
        }
    }

    /// Seconds left until the next minute boundary (1–60). Used to schedule the clock's repaint.
    #[must_use]
    pub(crate) fn secs_to_next_minute(&self) -> u64 {
        60 - (self.utc_secs % 60)
    }

    /// Build the string for a format, a 12-hour one in English. On the render path use
    /// [`WallTime::format_into_with`], which reuses a buffer and takes the language's words.
    #[must_use]
    pub fn format(&self, format: ClockFormat) -> String {
        self.format_with(format, Meridiem::ENGLISH)
    }

    /// [`WallTime::format`] with a language's words for the half of the day.
    #[must_use]
    pub fn format_with(&self, format: ClockFormat, meridiem: Meridiem<'_>) -> String {
        let mut out = String::new();
        self.format_into_with(&mut out, format, meridiem);
        out
    }

    /// The same as [`WallTime::format`] but it clears `out` and writes there — a caller that
    /// runs whenever the displayed value changes, like the status bar clock, reuses one buffer
    /// and allocates nothing.
    pub fn format_into(&self, out: &mut String, format: ClockFormat) {
        self.format_into_with(out, format, Meridiem::ENGLISH);
    }

    /// [`WallTime::format_into`] with a language's words for the half of the day — `9:30 AM`,
    /// `오전 9:30`.
    pub fn format_into_with(&self, out: &mut String, format: ClockFormat, meridiem: Meridiem<'_>) {
        use std::fmt::Write as _;
        out.clear();
        let c = self.civil();
        // `String`'s `fmt::Write` does not fail.
        let (h12, pm) = twelve_hour(c.hour);
        let half = if pm { meridiem.pm } else { meridiem.am };
        let _ = match format {
            ClockFormat::Hm => write!(out, "{:02}:{:02}", c.hour, c.minute),
            ClockFormat::Hms => write!(out, "{:02}:{:02}:{:02}", c.hour, c.minute, c.second),
            ClockFormat::Hm12 => {
                around(out, half, |out| write!(out, "{h12}:{:02}", c.minute));
                Ok(())
            }
            ClockFormat::Hms12 => {
                around(out, half, |out| {
                    write!(out, "{h12}:{:02}:{:02}", c.minute, c.second)
                });
                Ok(())
            }
            ClockFormat::DateHm => {
                write!(
                    out,
                    "{:02}-{:02} {:02}:{:02}",
                    c.month, c.day, c.hour, c.minute
                )
            }
            ClockFormat::DateHm12 => {
                let _ = write!(out, "{:02}-{:02} ", c.month, c.day);
                around(out, half, |out| write!(out, "{h12}:{:02}", c.minute));
                Ok(())
            }
        };
    }
}

/// **The words of a 12-hour clock**: templates with `{t}` where the time goes — `"{t} AM"` in
/// English, `"오전 {t}"` in Korean — so the half of the day can go on either side of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Meridiem<'a> {
    /// Before noon.
    pub am: &'a str,
    /// From noon on.
    pub pm: &'a str,
}

impl Meridiem<'static> {
    /// `9:30 AM`.
    pub const ENGLISH: Self = Self {
        am: labels::AM,
        pm: labels::PM,
    };
}

impl<'a> Meridiem<'a> {
    /// The active language's, from the string table.
    #[must_use]
    pub fn of(strings: &'a Strings) -> Self {
        Self {
            am: strings.get(labels::AM),
            pm: strings.get(labels::PM),
        }
    }
}

/// The clock's words, through the string table.
pub(crate) mod labels {
    /// A 12-hour time before noon; `{t}` is the time.
    pub(crate) const AM: &str = "{t} AM";
    /// A 12-hour time from noon on.
    pub(crate) const PM: &str = "{t} PM";
}

/// Write `half` with the time in place of its `{t}` — the time and the word, where a table left
/// the `{t}` out.
fn around(out: &mut String, half: &str, time: impl FnOnce(&mut String) -> std::fmt::Result) {
    if let Some((before, after)) = half.split_once("{t}") {
        out.push_str(before);
        let _ = time(out);
        out.push_str(after);
    } else {
        let _ = time(out);
        out.push(' ');
        out.push_str(half);
    }
}

/// A 24-hour hour → the 12-hour hour, and whether it is after noon. Midnight is 12 AM and noon
/// 12 PM.
const fn twelve_hour(hour: u8) -> (u8, bool) {
    match hour {
        0 => (12, false),
        1..=11 => (hour, false),
        12 => (12, true),
        _ => (hour - 12, true),
    }
}

/// Epoch days → (year, month, day). Howard Hinnant's `civil_from_days`.
#[must_use]
pub(crate) fn civil_from_days(days: i64) -> (i64, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        u8::try_from(m).unwrap_or(1),
        u8::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, ClockFormat, Meridiem, WallTime};
    use crate::i18n::Strings;

    #[test]
    fn epoch_is_1970() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
    }

    #[test]
    fn century_and_leap_year_boundaries() {
        // 2000-03-01: 2000 divides by 400 and so is a leap year, and this boundary exposes the common bug
        // of looking only at "divisible by 100 means a common year" (Hinnant's algorithm's reason to exist).
        assert_eq!(civil_from_days(11_017), (2000, 3, 1));
        // 2024-02-29: the last day of an ordinary leap year divisible by 4.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(19_783), (2024, 3, 1));
    }

    #[test]
    fn offset_and_formats() {
        // 2024-01-01T00:30:00Z, KST(+540) → 09:30
        let t = WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 540,
        };
        assert_eq!(t.format(ClockFormat::Hm), "09:30");
        assert_eq!(t.format(ClockFormat::Hm12), "9:30 AM");
        assert_eq!(t.format(ClockFormat::DateHm), "01-01 09:30");
        assert_eq!(t.format(ClockFormat::Hms12), "9:30:00 AM");
        assert_eq!(t.format(ClockFormat::DateHm12), "01-01 9:30 AM");
        assert_eq!(t.secs_to_next_minute(), 60);
    }

    /// The half of the day goes where the language puts it — before the time in Korean.
    #[test]
    fn a_twelve_hour_clock_speaks_the_language() {
        let morning = WallTime {
            utc_secs: 1_704_069_000,
            offset_min: 540,
        };
        let evening = WallTime {
            utc_secs: 1_704_069_000 + 9 * 3600,
            offset_min: 540,
        };
        let korean = Strings::new("ko");
        let words = Meridiem::of(&korean);
        assert_eq!(morning.format_with(ClockFormat::Hm12, words), "오전 9:30");
        assert_eq!(evening.format_with(ClockFormat::Hm12, words), "오후 6:30");
        assert_eq!(
            morning.format_with(ClockFormat::DateHm12, words),
            "01-01 오전 9:30"
        );
        let bare = Meridiem { am: "a", pm: "p" };
        assert_eq!(morning.format_with(ClockFormat::Hm12, bare), "9:30 a");
    }
}
