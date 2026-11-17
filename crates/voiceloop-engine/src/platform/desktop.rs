//! Desktop (macOS, Windows, Linux): there is no app-wide audio session.
//! CPAL/CoreAudio negotiate per device; device and default-device changes are
//! detected by the hardware poller (`hardware.rs`) and CPAL error callbacks.

use std::sync::mpsc;

use super::{MicPermission, SessionEvent, SessionInfo, SessionRequest};
use crate::error::EngineError;

pub fn prepare_audio(_request: &SessionRequest) -> Result<Option<SessionInfo>, EngineError> {
    Ok(None)
}

pub fn release_audio() {}

/// macOS reports record permission only through AVFoundation APIs that would
/// add a desktop dependency; denied access shows up as digital silence, which
/// the engine's `silentInput` warning already explains.
pub fn mic_permission() -> MicPermission {
    MicPermission::Unknown
}

/// macOS prompts on first capture; answer at once with what we know.
pub fn request_mic_permission(events: mpsc::Sender<SessionEvent>) {
    let _ = events.send(SessionEvent::MicPermission { permission: mic_permission() });
}

pub struct Observers;

pub fn observe(_events: mpsc::Sender<SessionEvent>) -> Observers {
    Observers
}
