use serde::Serialize;

/// Errors surfaced to the application layer. Serialised as `{ kind, message }`
/// so the UI can branch on `kind` (e.g. show a permission hint).
#[derive(Debug, thiserror::Error, Serialize, Clone, PartialEq)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum EngineError {
    #[error("no input device is available")]
    NoInputDevice,
    #[error("input device not found: {0}")]
    DeviceNotFound(String),
    #[error("microphone access was denied or the device is unavailable: {0}")]
    PermissionOrUnavailable(String),
    #[error("unsupported sample format: {0}")]
    UnsupportedFormat(String),
    #[error("audio backend error: {0}")]
    Backend(String),
    #[error("the engine control thread has stopped")]
    EngineStopped,
}

impl From<cpal::DevicesError> for EngineError {
    fn from(e: cpal::DevicesError) -> Self {
        EngineError::Backend(e.to_string())
    }
}

impl From<cpal::DefaultStreamConfigError> for EngineError {
    fn from(e: cpal::DefaultStreamConfigError) -> Self {
        match e {
            cpal::DefaultStreamConfigError::DeviceNotAvailable => {
                EngineError::PermissionOrUnavailable(e.to_string())
            }
            other => EngineError::Backend(other.to_string()),
        }
    }
}

impl From<cpal::BuildStreamError> for EngineError {
    fn from(e: cpal::BuildStreamError) -> Self {
        match e {
            cpal::BuildStreamError::DeviceNotAvailable => {
                EngineError::PermissionOrUnavailable(e.to_string())
            }
            other => EngineError::Backend(other.to_string()),
        }
    }
}

impl From<cpal::PlayStreamError> for EngineError {
    fn from(e: cpal::PlayStreamError) -> Self {
        match e {
            cpal::PlayStreamError::DeviceNotAvailable => {
                EngineError::PermissionOrUnavailable(e.to_string())
            }
            other => EngineError::Backend(other.to_string()),
        }
    }
}
