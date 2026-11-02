//! Developer-controlled shell replacement and optional fault guidance.
//!
//! No restart schedule or retry policy is installed by this module. Business state and
//! side effects belong to the application; replacing a shell does not roll them back.

use std::{borrow::Cow, fmt};

/// Numeric payload for an unexpected application fault, with no owned resources or
/// custom destructor. Applications may use `std::panic::panic_any(PanicCode(code))`
/// at their own unwind boundary. Fairing does not provide a helper that starts a panic.
/// Expected failures should use `Result` or the relevant typed outcome instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanicCode(
    /// Application-defined code. Avoid encoding secrets in diagnostic identifiers.
    pub u16,
);

/// Process-wide, saturating payload-cleanup counters. Reads are individually atomic;
/// a snapshot can include activity from other threads. No string contents are exposed.
/// Supported payloads are exactly `String`, `&'static str` and [`PanicCode`].
/// An unknown payload is retained without invoking its destructor; this is a contract
/// violation, not a recovery path with bounded resource use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanicCleanupStats {
    /// Unwinds caught at runtime containment boundaries.
    pub caught: usize,
    /// Supported payloads released without invoking a custom destructor.
    pub released: usize,
    /// Unknown payloads retained without executing their destructors.
    /// This counts objects, not bytes or resources; it is not a memory bound.
    pub retained: usize,
    /// Most recently observed numeric payload, across all threads. String/unknown
    /// payloads do not clear it. This is not a code associated with a particular shell.
    pub last_code: Option<PanicCode>,
}

/// Inspect cumulative diagnostics without allocating or invoking payload code.
#[must_use]
pub fn panic_cleanup_stats() -> PanicCleanupStats {
    crate::runtime::panic_cleanup_stats()
}

/// Why [`crate::Shell::restart_with`] could not install an operational shell.
#[non_exhaustive]
pub enum RestartError {
    /// The replacement factory returned an error.
    Build(crate::Error),
    /// Construction, disposal of an old/rejected shell or the host wake callback unwound.
    Panicked,
    /// The factory returned a shell that was already quarantined.
    FaultedReplacement,
}

impl fmt::Display for RestartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Build(_) => "shell replacement construction failed",
            Self::Panicked => "shell replacement panicked",
            Self::FaultedReplacement => "replacement shell is quarantined",
        })
    }
}

impl fmt::Debug for RestartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl std::error::Error for RestartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Build(error) => Some(error),
            Self::Panicked | Self::FaultedReplacement => None,
        }
    }
}

/// Optional fault guidance. Neither this view nor its button performs a restart.
/// Install with [`crate::ShellBuilder::recovery_ui`] or [`crate::Shell::set_recovery_ui`].
/// Drawing uses egui and is not allocation-free.
#[derive(Debug, Clone)]
pub struct RecoveryUi {
    /// Application-supplied heading.
    pub heading: Cow<'static, str>,
    /// Application-supplied instructions, contact details or recovery route.
    pub message: Cow<'static, str>,
    /// Show a request button only when a label is supplied.
    pub restart_label: Option<Cow<'static, str>>,
}

impl Default for RecoveryUi {
    fn default() -> Self {
        Self {
            heading: "Interface unavailable".into(),
            message: "The interface is paused.".into(),
            restart_label: None,
        }
    }
}

impl RecoveryUi {
    /// Draw guidance, returning the optional request button's response for custom hosts.
    /// The caller decides how to act on a click.
    pub fn show(&self, ui: &mut egui::Ui) -> Option<egui::Response> {
        ui.heading(self.heading.as_ref());
        ui.label(self.message.as_ref());
        self.restart_label
            .as_ref()
            .map(|label| ui.button(label.as_ref()))
    }
}
