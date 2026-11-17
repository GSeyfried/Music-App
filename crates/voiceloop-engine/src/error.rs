use serde::Serialize;

/// Errors surfaced to the application layer. Serialised as `{ kind, message }`
/// so the UI can branch on `kind` (e.g. show a permission hint).
#[derive(Debug, thiserror::Error, Serialize, Clone, PartialEq)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum EngineError {
    #[error("no input device is available")]
    NoInputDevice,
    #[error("audio device not found: {0}")]
    DeviceNotFound(String),
    /// A device in use disappeared (unplugged, powered off, or taken exclusively).
    #[error("audio device disconnected: {0}")]
    DeviceLost(String),
    /// MIDI port enumeration, open, or close failed.
    #[error("MIDI error: {0}")]
    Midi(String),
    /// Reading or writing a settings file failed.
    #[error("file error: {0}")]
    Io(String),
    #[error("microphone access was denied or the device is unavailable: {0}")]
    PermissionOrUnavailable(String),
    #[error("unsupported sample format: {0}")]
    UnsupportedFormat(String),
    #[error("audio backend error: {0}")]
    Backend(String),
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("resource limit: {0}")]
    ResourceLimit(String),
    #[error("engine busy: {0}")]
    Busy(String),
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
            cpal::DefaultStreamConfigError::DeviceNotAvailable => EngineError::PermissionOrUnavailable(e.to_string()),
            other => EngineError::Backend(other.to_string()),
        }
    }
}

impl From<cpal::BuildStreamError> for EngineError {
    fn from(e: cpal::BuildStreamError) -> Self {
        match e {
            cpal::BuildStreamError::DeviceNotAvailable => EngineError::PermissionOrUnavailable(e.to_string()),
            other => EngineError::Backend(other.to_string()),
        }
    }
}

impl From<cpal::PlayStreamError> for EngineError {
    fn from(e: cpal::PlayStreamError) -> Self {
        match e {
            cpal::PlayStreamError::DeviceNotAvailable => EngineError::PermissionOrUnavailable(e.to_string()),
            other => EngineError::Backend(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_errors_serialise_with_their_kind() {
        let json = serde_json::to_value(EngineError::DeviceLost("H4n Pro".into())).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "deviceLost", "message": "H4n Pro"}));
        assert_eq!(serde_json::to_value(EngineError::Midi("x".into())).unwrap()["kind"], "midi");
        assert_eq!(serde_json::to_value(EngineError::Io("x".into())).unwrap()["kind"], "io");
    }
}
