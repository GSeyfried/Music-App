//! Shared domain models. Kept deliberately small: they define boundaries for
//! future work, not final schemas. Mirrored in `src/models/domain.ts`.
//!
//! Principles encoded here:
//! - Original recordings are immutable: clips reference a source file and
//!   carry non-destructive edits; corrections produce new derived assets.
//! - Analysis observations live apart from audio processing data.
//! - All musical timing is anchored to the authoritative audio clock
//!   ([`FramePos`]); bars/beats are derived through the [`crate::transport`] tempo map.
//! - Reference performances and user attempts are distinct types.
//! - Time signatures live per track (optional) to allow future polymeter.

use serde::{Deserialize, Serialize};

/// Frames on the authoritative audio clock (sample frames at the session rate).
pub type FramePos = u64;
/// Stable identifier (UUID string in practice).
pub type Id = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    /// Backend-specific identifier. Currently the device name (CPAL 0.16 has no stable id).
    pub id: String,
    pub name: String,
    pub host: String,
    pub direction: DeviceDirection,
    pub is_default: bool,
    pub default_sample_rate: Option<u32>,
    pub default_channels: Option<u16>,
    pub sample_format: Option<String>,
}

// ---- Persisted project document --------------------------------------------
//
// The on-disk project (`<Name>.voiceloop/project.json`) is defined in the
// `voiceloop-project` crate so it stays usable without the audio engine
// (and on iOS). Its types are re-exported here as the engine's domain model.
// `LoopMode` and `TrackKind` live there too, with unchanged JSON.

pub use voiceloop_project::doc::{
    DeviceHints, LayerDoc, LoopDoc, LoopMode, MidiClip, MidiNote, PracticeDoc, ProjectDoc, TrackDoc, TrackKind,
    TransportDoc,
};

/// Non-destructive window onto an immutable source recording (Practice Lab
/// references and attempts; looper layers use [`LayerDoc`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioClip {
    pub id: Id,
    /// Path relative to the project folder, e.g. `audio/take-0003.wav`. Never modified.
    pub source_path: String,
    pub source_sample_rate: u32,
    pub channels: u16,
    /// Placement on the session clock.
    pub start: FramePos,
    /// Offset into the source file.
    pub source_offset: FramePos,
    pub length: FramePos,
    /// Loop length in frames when the clip repeats.
    pub loop_length: Option<FramePos>,
    pub gain_db: f32,
    /// Set when this clip is a processed derivative (e.g. pitch-corrected) of another clip.
    pub derived_from: Option<Id>,
}

/// A performance the user wants to reproduce. Never mixed with attempts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceTrack {
    pub id: Id,
    pub name: String,
    pub clip: AudioClip,
    /// Analysis results for the reference, if computed.
    pub pitch: Vec<PitchObservation>,
    pub rhythm: Vec<RhythmObservation>,
}

/// A user's attempt, optionally scored against a reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PerformanceTake {
    pub id: Id,
    pub recorded_at: String,
    pub clip: AudioClip,
    pub reference_id: Option<Id>,
    pub pitch: Vec<PitchObservation>,
    pub rhythm: Vec<RhythmObservation>,
}

/// A single pitch-tracker output. Analysis data, separate from audio.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchObservation {
    pub frame: FramePos,
    /// Fundamental frequency in Hz; `None` when unvoiced.
    pub frequency_hz: Option<f32>,
    /// Detector confidence 0..=1 (e.g. MPM clarity).
    pub confidence: f32,
    /// Nearest MIDI note number (fractional, so cents = (midi - round(midi)) * 100).
    pub midi: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RhythmEventKind {
    Onset,
    Offset,
    Beat,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RhythmObservation {
    pub frame: FramePos,
    pub kind: RhythmEventKind,
    pub strength: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_device_serialises_camel_case() {
        let d = AudioDevice {
            id: "Mic".into(),
            name: "Mic".into(),
            host: "WASAPI".into(),
            direction: DeviceDirection::Input,
            is_default: true,
            default_sample_rate: Some(48_000),
            default_channels: Some(1),
            sample_format: Some("f32".into()),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"isDefault\":true"));
        assert!(json.contains("\"direction\":\"input\""));
    }
}
