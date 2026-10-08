//! The notification and toast model.

use crate::access::Gate;
use crate::icons::{builtin, IconRef};
use crate::screen::LaunchAction;
use crate::theme::ColorRole;
use crate::time::WallTime;
use std::time::Duration;

/// A notification id. Sending the same id again updates it (dedup). Hash a string key with [`NotificationId::of`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NotificationId(pub u64);

impl NotificationId {
    /// A stable hash of a string key (FNV-1a). `NotificationId::of("wifi.lost")`.
    #[must_use]
    pub fn of(key: &str) -> Self {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in key.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        Self(h)
    }
}

/// Severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Level {
    /// Information.
    #[default]
    Info,
    /// Success.
    Success,
    /// A warning.
    Warning,
    /// An error.
    Error,
}

impl Level {
    /// The default icon.
    #[must_use]
    pub fn icon(self) -> IconRef {
        match self {
            Self::Info => builtin::INFO,
            Self::Success => builtin::CHECK,
            Self::Warning => builtin::WARNING,
            Self::Error => builtin::ERROR,
        }
    }

    /// The accent colour role (a toast's stripe, a heads-up icon). `Info` uses no accent and
    /// keeps the body colour — colour on information notifications too would bury the warnings
    /// and errors.
    #[must_use]
    pub fn role(self) -> ColorRole {
        match self {
            Self::Info => ColorRole::OnSurface,
            Self::Success => ColorRole::Success,
            Self::Warning => ColorRole::Warning,
            Self::Error => ColorRole::Danger,
        }
    }
}

/// A notification. In memory only — a restart clears it.
#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    /// The id (the dedup key).
    pub id: NotificationId,
    /// The grouping key (a screen id, say).
    pub source: String,
    /// The icon. `None` — the default — draws [`Level::icon`], so severity always has a shape and
    /// not only a colour. Set one with [`Notification::icon`] to override it.
    pub icon: Option<IconRef>,
    /// The title.
    pub title: String,
    /// The body.
    pub body: String,
    /// Severity.
    pub level: Level,
    /// When it happened (the shell fills it from `services.clock` if `at == WallTime::default()`).
    pub at: WallTime,
    /// The user cannot dismiss it.
    pub persistent: bool,
    /// Progress, 0..=1.
    pub progress: Option<f32>,
    /// Run on tap.
    pub action: Option<LaunchAction>,
    /// A session that fails this sees "1 notification" instead of the content.
    pub gate: Option<Gate>,
}

impl Notification {
    /// The minimum notification: an id and a title. The rest is the builder.
    #[must_use]
    pub fn new(id: NotificationId, title: impl Into<String>) -> Self {
        Self {
            id,
            source: String::new(),
            icon: None,
            title: title.into(),
            body: String::new(),
            level: Level::Info,
            at: WallTime::default(),
            persistent: false,
            progress: None,
            action: None,
            gate: None,
        }
    }

    /// The body.
    #[must_use]
    pub fn body(mut self, body: impl Into<String>) -> Self {
        self.body = body.into();
        self
    }

    /// The source.
    #[must_use]
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// The icon, overriding the severity shape.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Severity.
    #[must_use]
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// The icon to draw: the one that was set, else the severity's own shape.
    ///
    /// **Severity is a shape, not only a colour.** The icon used to default to [`builtin::BELL`]
    /// and `level()` swapped it only while it was still the bell — so `.icon(…).level(…)` and
    /// `.level(…).icon(…)` disagreed, and a caller who never reached for `level()` got a bell on
    /// every row. A fault and a success then read the same at a glance, which is the defect as
    /// reported: in the shade, "Sensor 3 offline" and "Backup done" carried the same glyph. The
    /// slot is now optional and resolves here, so the order of the builder no longer matters and
    /// the severity shape is what a caller gets by default.
    #[must_use]
    pub fn shown_icon(&self) -> IconRef {
        self.icon.clone().unwrap_or_else(|| self.level.icon())
    }

    /// Persistent (cannot be dismissed).
    #[must_use]
    pub fn persistent(mut self) -> Self {
        self.persistent = true;
        self
    }

    /// Progress, clamped to 0..=1. NaN (a `0.0 / 0.0` from a job with no steps) counts as 0.
    #[must_use]
    pub fn progress(mut self, p: f32) -> Self {
        self.progress = Some(if p.is_nan() { 0.0 } else { p.clamp(0.0, 1.0) });
        self
    }

    /// The tap action.
    #[must_use]
    pub fn action(mut self, action: LaunchAction) -> Self {
        self.action = Some(action);
        self
    }

    /// The gate.
    #[must_use]
    pub fn gate(mut self, gate: impl Into<Gate>) -> Self {
        self.gate = Some(gate.into());
        self
    }
}

/// A toast. Queued, no gate.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    /// The text.
    pub text: String,
    /// Severity (the colour).
    pub level: Level,
    /// How long it shows. `Duration::ZERO` means `[notify] toast_ms`; a duration too long for the
    /// clock to count (`Duration::MAX`) holds it until it is tapped.
    pub duration: Duration,
    /// The icon.
    pub icon: Option<IconRef>,
}

impl Toast {
    /// A basic toast.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            level: Level::Info,
            duration: Duration::ZERO,
            icon: None,
        }
    }

    /// Severity.
    #[must_use]
    pub fn level(mut self, level: Level) -> Self {
        self.level = level;
        self
    }

    /// How long it shows. `Duration::MAX` keeps it up until it is tapped.
    #[must_use]
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// The icon.
    #[must_use]
    pub fn icon(mut self, icon: IconRef) -> Self {
        self.icon = Some(icon);
        self
    }
}

impl From<&str> for Toast {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for Toast {
    fn from(text: String) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use super::{builtin, IconRef, Level, Notification, NotificationId};

    fn note(title: &str) -> Notification {
        Notification::new(NotificationId::of(title), title)
    }

    /// **Every severity draws its own silhouette.** A tick, a triangle, a crossed disc, a dotted
    /// disc — four outlines a glance separates without reading the colour. The regression this
    /// guards is what the shade actually showed: "Sensor 3 offline" and "Backup done" carrying the
    /// same bell, so a fault and a success were one glyph apart from identical.
    #[test]
    fn each_level_has_its_own_shape() {
        let shapes: Vec<IconRef> = [Level::Info, Level::Success, Level::Warning, Level::Error]
            .into_iter()
            .map(|level| note("x").level(level).shown_icon())
            .collect();
        for (i, a) in shapes.iter().enumerate() {
            for b in shapes.iter().skip(i + 1) {
                assert_ne!(a, b, "two levels share a shape: {shapes:?}");
            }
            assert_ne!(*a, builtin::BELL, "a level still falls back to the bell");
        }
    }

    /// The builder is order-free. `level()` used to swap the icon only while it was still the bell,
    /// so `.icon(…).level(…)` and `.level(…).icon(…)` gave different pictures from the same words.
    #[test]
    fn the_builder_reads_the_same_in_either_order() {
        let first = note("a").icon(builtin::WIFI).level(Level::Error);
        let second = note("a").level(Level::Error).icon(builtin::WIFI);
        assert_eq!(first.shown_icon(), second.shown_icon());
        assert_eq!(first.shown_icon(), builtin::WIFI);
        assert_eq!(first.level, second.level);
    }

    /// A caller who never reaches for `level()` gets the info shape, not a bell.
    #[test]
    fn the_default_notification_shows_the_info_shape() {
        assert_eq!(note("a").shown_icon(), Level::Info.icon());
        assert_eq!(note("a").icon, None);
    }

    /// The shade and the toast read severity off one table. They disagreed: the shade painted an
    /// `Info` row in the accent — the loudest colour on screen on the least urgent row — while the
    /// toast kept it at body colour. `Level::role` is now the only table.
    #[test]
    fn info_carries_no_accent() {
        use crate::theme::ColorRole;
        assert_eq!(Level::Info.role(), ColorRole::OnSurface);
        assert_eq!(Level::Success.role(), ColorRole::Success);
        assert_eq!(Level::Warning.role(), ColorRole::Warning);
        assert_eq!(Level::Error.role(), ColorRole::Danger);
    }
}
