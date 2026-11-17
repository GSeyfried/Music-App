//! Platform audio-session layer.
//!
//! One interface over what differs between iOS and the desktop:
//!
//! | | iOS (`ios.rs`) | macOS / other (`desktop.rs`) |
//! | --- | --- | --- |
//! | [`prepare_audio`] | AVAudioSession category (PlayAndRecord), mode, options, preferred rate and IO buffer, activate; returns the granted values | No-op (`None`): CPAL and CoreAudio negotiate per device |
//! | [`release_audio`] | Deactivate the session | No-op |
//! | [`mic_permission`] / [`request_mic_permission`] | `AVAudioApplication` record permission | `Unknown` (CoreAudio delivers silence when denied; the engine's `silentInput` warning covers it) |
//! | [`observe`] | Interruption, route-change, media-services-reset notifications → [`SessionEvent`] | Nothing; the hardware poller covers device and default-device changes |
//!
//! Everything here runs on the engine's control thread (or a notification
//! thread that only sends on a channel), never in an audio callback.

use std::sync::mpsc;

use serde::Serialize;

#[cfg(not(target_os = "ios"))]
mod desktop;
#[cfg(not(target_os = "ios"))]
use desktop as imp;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
use ios as imp;

/// The platform sets the IO buffer through its session: CPAL streams must
/// then use `BufferSize::Default` (the requested size goes to the session).
pub const SESSION_MANAGES_BUFFER: bool = cfg!(target_os = "ios");

/// Microphone (record) permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MicPermission {
    Granted,
    Denied,
    /// Not asked yet: the system prompt appears on request (or first capture).
    Undetermined,
    /// The platform does not report it (macOS without AVAudioApplication).
    Unknown,
}

/// What the engine asks of the platform session before opening streams.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionRequest {
    pub sample_rate: u32,
    pub buffer_frames: u32,
    /// Measurement mode: no system voice processing / AGC (wanted for music).
    pub measurement: bool,
    pub allow_bluetooth_output: bool,
    pub default_to_speaker: bool,
}

impl SessionRequest {
    /// Request for an engine start: requested rate and buffer, else 48 kHz / 256.
    pub fn for_start(sample_rate: Option<u32>, buffer_frames: Option<u32>) -> Self {
        Self {
            sample_rate: sample_rate.unwrap_or(48_000),
            buffer_frames: buffer_frames.unwrap_or(256),
            measurement: true,
            allow_bluetooth_output: true,
            default_to_speaker: true,
        }
    }
}

/// An audio route port (iOS).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPort {
    pub uid: String,
    pub name: String,
    pub port_type: String,
}

/// The values the platform actually granted (never assume the request was honoured).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub sample_rate: f64,
    pub io_buffer_frames: u32,
    pub input_latency_frames: u32,
    pub output_latency_frames: u32,
    pub input_channels: u32,
    pub max_input_channels: u32,
    pub inputs: Vec<SessionPort>,
    pub outputs: Vec<SessionPort>,
}

/// Why the audio route changed (iOS `AVAudioSessionRouteChangeReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RouteChangeReason {
    Unknown,
    NewDeviceAvailable,
    OldDeviceUnavailable,
    CategoryChange,
    Override,
    WakeFromSleep,
    NoSuitableRouteForCategory,
    RouteConfigurationChange,
}

impl RouteChangeReason {
    pub fn from_raw(raw: usize) -> Self {
        match raw {
            1 => Self::NewDeviceAvailable,
            2 => Self::OldDeviceUnavailable,
            3 => Self::CategoryChange,
            4 => Self::Override,
            6 => Self::WakeFromSleep,
            7 => Self::NoSuitableRouteForCategory,
            8 => Self::RouteConfigurationChange,
            _ => Self::Unknown,
        }
    }

    /// Whether running streams must be rebuilt (the device set or format may
    /// have changed). A category change is usually our own `prepare_audio`.
    pub fn needs_restart(self) -> bool {
        matches!(
            self,
            Self::NewDeviceAvailable | Self::OldDeviceUnavailable | Self::Override | Self::RouteConfigurationChange
        )
    }
}

/// Platform session notifications, delivered to the control thread.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionEvent {
    /// Another app or the system (call, Siri, alarm) took the audio session.
    InterruptionBegan,
    /// The interruption is over; `should_resume` is the system's hint.
    #[serde(rename_all = "camelCase")]
    InterruptionEnded { should_resume: bool },
    /// Headphones, an interface, or Bluetooth came or went.
    RouteChanged { reason: RouteChangeReason },
    /// The media server restarted: every audio object is invalid.
    MediaServicesReset,
    /// Answer to [`request_mic_permission`].
    MicPermission { permission: MicPermission },
}

/// Configure and activate the platform session. `Ok(None)` where there is none.
pub fn prepare_audio(request: &SessionRequest) -> Result<Option<SessionInfo>, crate::error::EngineError> {
    imp::prepare_audio(request)
}

/// Deactivate the session after audio stops (lets other apps' audio resume).
pub fn release_audio() {
    imp::release_audio()
}

pub fn mic_permission() -> MicPermission {
    imp::mic_permission()
}

/// Ask for record permission; the answer arrives as [`SessionEvent::MicPermission`].
pub fn request_mic_permission(events: mpsc::Sender<SessionEvent>) {
    imp::request_mic_permission(events)
}

/// Keeps the platform's notification observers registered; dropping it removes them.
pub struct Observers {
    #[allow(dead_code)]
    inner: imp::Observers,
}

/// Start delivering [`SessionEvent`]s to `events`.
pub fn observe(events: mpsc::Sender<SessionEvent>) -> Observers {
    Observers { inner: imp::observe(events) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_change_reasons_map_and_only_device_changes_restart() {
        assert_eq!(RouteChangeReason::from_raw(1), RouteChangeReason::NewDeviceAvailable);
        assert_eq!(RouteChangeReason::from_raw(8), RouteChangeReason::RouteConfigurationChange);
        assert_eq!(RouteChangeReason::from_raw(99), RouteChangeReason::Unknown);
        assert!(RouteChangeReason::OldDeviceUnavailable.needs_restart());
        assert!(!RouteChangeReason::CategoryChange.needs_restart(), "our own session setup must not loop");
        assert!(!RouteChangeReason::WakeFromSleep.needs_restart());
    }

    #[test]
    fn session_events_serialise_for_the_frontend() {
        let e = SessionEvent::InterruptionEnded { should_resume: true };
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            serde_json::json!({"kind": "interruptionEnded", "shouldResume": true})
        );
        let e = SessionEvent::RouteChanged { reason: RouteChangeReason::OldDeviceUnavailable };
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            serde_json::json!({"kind": "routeChanged", "reason": "oldDeviceUnavailable"})
        );
        let e = SessionEvent::MicPermission { permission: MicPermission::Granted };
        assert_eq!(
            serde_json::to_value(e).unwrap(),
            serde_json::json!({"kind": "micPermission", "permission": "granted"})
        );
    }

    #[cfg(not(target_os = "ios"))]
    #[test]
    fn desktop_session_is_a_no_op() {
        assert_eq!(prepare_audio(&SessionRequest::for_start(None, None)).unwrap(), None);
        assert_eq!(mic_permission(), MicPermission::Unknown);
        let (tx, rx) = mpsc::channel();
        let _observers = observe(tx.clone());
        request_mic_permission(tx);
        assert_eq!(rx.try_recv().unwrap(), SessionEvent::MicPermission { permission: MicPermission::Unknown });
        release_audio();
    }
}
