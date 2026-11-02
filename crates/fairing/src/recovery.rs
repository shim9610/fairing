//! Developer-controlled shell replacement and optional fault guidance.
//!
//! No restart schedule or retry policy is installed by this module. Business state and
//! side effects belong to the application; replacing a shell does not roll them back.

use std::{borrow::Cow, fmt};

/// Process-wide, saturating unwind-cleanup counters. Reads are individually atomic;
/// a snapshot can include activity from other threads. No payload contents are exposed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PanicCleanupStats {
    /// Unwinds caught at runtime containment boundaries.
    pub caught: usize,
    /// Payload disposal attempts which themselves unwound.
    pub cleanup_panics: usize,
    /// Final payloads retained after the finite disposal budget was exhausted.
    /// This counts objects, not bytes or resources; it is not a memory bound.
    pub retained: usize,
}

/// Inspect cumulative cleanup diagnostics without allocating or invoking payload code.
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
