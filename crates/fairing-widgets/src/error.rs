//! The crate's error type.
//!
//! Library code does not panic. A configuration error fails at startup as
//! [`Error::Config`], and backend errors come back separately as `fairing::services::ServiceError`.

use std::fmt;

/// The error the public `fairing` API returns.
#[derive(Debug, Clone, PartialEq, Eq)]
// `#[non_exhaustive]`: the error set grows with the shell, and a caller handles an error as a
// whole rather than variant by variant.
#[non_exhaustive]
pub enum Error {
    /// Parsing or validating the configuration (`fairing.toml`) failed. Access-control config
    /// errors land here too ("an unassigned gate") — they are never opened up
    /// quietly.
    Config(String),
    /// Reading a file failed (loading the config). A file that is simply **absent** is not an error; it means the defaults.
    Io {
        /// The path that failed.
        path: String,
        /// The message the OS gave.
        message: String,
    },
    /// A window or graphics error from the `runner` feature.
    Runner(String),
    /// An image could not be decoded (a corrupt file, an unknown format, over a limit) — the
    /// error an integrator's image loader (`fairing::ShellBuilder::image_loader`) returns.
    ///
    /// Failing to **read** the file is [`Error::Io`]; this is reading it and failing to decode.
    Image(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(f, "fairing config error: {message}"),
            Self::Io { path, message } => write!(f, "fairing io error at {path}: {message}"),
            Self::Runner(message) => write!(f, "fairing runner error: {message}"),
            Self::Image(message) => write!(f, "fairing image error: {message}"),
        }
    }
}

impl std::error::Error for Error {}

/// This crate's `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;
