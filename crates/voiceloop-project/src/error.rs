//! Typed errors for project and file operations.

use std::path::Path;

/// Every failure this crate reports. `kind()` gives a stable machine-readable tag.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProjectError {
    /// The operating system refused a file operation.
    #[error("{action} {path}: {message}")]
    Io { action: &'static str, path: String, message: String },
    #[error("not found: {0}")]
    NotFound(String),
    /// The folder or file is not something this crate wrote (wrong `format`, not a RIFF/WAVE file ...).
    #[error("not a VoiceLoop project or supported file: {0}")]
    NotRecognised(String),
    /// Recognised, but damaged: truncated, unparsable, or failing an integrity check.
    #[error("{file} is damaged: {reason}")]
    Corrupt { file: String, reason: String },
    /// Written by a newer app whose format this reader cannot open safely.
    #[error("project needs format version {required}, but this app reads up to version {supported}")]
    UnsupportedVersion { required: u32, supported: u32 },
    /// Well-formed but breaks a rule of the format (absolute path, bad sample rate ...).
    #[error("invalid project: {0}")]
    Invalid(String),
    /// A valid file in an encoding this crate does not handle.
    #[error("unsupported audio or MIDI encoding: {0}")]
    Unsupported(String),
    #[error("too large: {0}")]
    TooLarge(String),
}

impl ProjectError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::NotFound(_) => "notFound",
            Self::NotRecognised(_) => "notRecognised",
            Self::Corrupt { .. } => "corrupt",
            Self::UnsupportedVersion { .. } => "unsupportedVersion",
            Self::Invalid(_) => "invalid",
            Self::Unsupported(_) => "unsupported",
            Self::TooLarge(_) => "tooLarge",
        }
    }

    pub(crate) fn io(action: &'static str, path: &Path, e: std::io::Error) -> Self {
        if e.kind() == std::io::ErrorKind::NotFound {
            return Self::NotFound(path.display().to_string());
        }
        Self::Io { action, path: path.display().to_string(), message: e.to_string() }
    }

    pub(crate) fn corrupt(file: &Path, reason: impl Into<String>) -> Self {
        Self::Corrupt { file: file.display().to_string(), reason: reason.into() }
    }
}

pub type Result<T> = std::result::Result<T, ProjectError>;
