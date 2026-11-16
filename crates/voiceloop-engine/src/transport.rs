//! Transport and musical time.
//!
//! The authoritative clock is a frame counter on the audio device. Musical
//! positions (bar/beat/tick) are always *derived* from frames via tempo and
//! meter, never stored as the source of truth. This module currently supports
//! a single constant tempo; a tempo map is a Milestone 1+ concern.

use serde::{Deserialize, Serialize};

use crate::model::FramePos;

/// A time signature such as 4/4, 7/8 or 5/4. `denominator` is the note value
/// that receives one beat (power of two).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl TimeSignature {
    pub const FOUR_FOUR: Self = Self { numerator: 4, denominator: 4 };

    pub fn new(numerator: u8, denominator: u8) -> Option<Self> {
        (numerator >= 1 && denominator.is_power_of_two() && denominator <= 64)
            .then_some(Self { numerator, denominator })
    }
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self::FOUR_FOUR
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PlayState {
    #[default]
    Stopped,
    Playing,
    Recording,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetronomeSettings {
    pub enabled: bool,
    /// Clicks per beat (1 = beats only, 2 = eighths in x/4, 3 = triplets ...).
    pub subdivision: u8,
    pub accent_downbeat: bool,
    pub gain_db: f32,
    /// Count-in length in bars before recording.
    pub count_in_bars: u8,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self { enabled: true, subdivision: 1, accent_downbeat: true, gain_db: -6.0, count_in_bars: 1 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportState {
    pub play_state: PlayState,
    /// Beats per minute, where a beat is the time-signature denominator note.
    pub bpm: f64,
    pub time_signature: TimeSignature,
    pub metronome: MetronomeSettings,
    /// Current position on the authoritative clock.
    pub position: FramePos,
    pub sample_rate: u32,
}

impl Default for TransportState {
    fn default() -> Self {
        Self {
            play_state: PlayState::Stopped,
            bpm: 120.0,
            time_signature: TimeSignature::FOUR_FOUR,
            metronome: MetronomeSettings::default(),
            position: 0,
            sample_rate: 48_000,
        }
    }
}

/// Bar/beat position derived from frames. `bar` and `beat` are 1-based; `fraction` is 0..1 within the beat.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalPosition {
    pub bar: u64,
    pub beat: u32,
    pub fraction: f64,
}

impl TransportState {
    pub fn frames_per_beat(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / self.bpm
    }

    pub fn frames_per_bar(&self) -> f64 {
        self.frames_per_beat() * self.time_signature.numerator as f64
    }

    /// Frame of the start of `bar` (0-based), rounded to the nearest frame.
    pub fn bar_start_frame(&self, bar: u64) -> FramePos {
        (bar as f64 * self.frames_per_bar()).round() as FramePos
    }

    pub fn musical_position(&self, frame: FramePos) -> MusicalPosition {
        let beats = frame as f64 / self.frames_per_beat();
        let per_bar = self.time_signature.numerator as f64;
        let bar = (beats / per_bar).floor();
        let beat_in_bar = beats - bar * per_bar;
        MusicalPosition {
            bar: bar as u64 + 1,
            beat: beat_in_bar.floor() as u32 + 1,
            fraction: beat_in_bar.fract(),
        }
    }

    /// Length in frames of a synced loop of `bars` bars.
    pub fn loop_length_frames(&self, bars: u32) -> FramePos {
        self.bar_start_frame(bars as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_signatures() {
        assert!(TimeSignature::new(7, 8).is_some());
        assert!(TimeSignature::new(4, 3).is_none());
        assert!(TimeSignature::new(0, 4).is_none());
    }

    #[test]
    fn bar_lengths() {
        let t = TransportState { bpm: 120.0, sample_rate: 48_000, ..Default::default() };
        assert_eq!(t.frames_per_beat(), 24_000.0);
        assert_eq!(t.loop_length_frames(2), 192_000);

        let seven_eight = TransportState {
            time_signature: TimeSignature::new(7, 8).unwrap(),
            ..t
        };
        assert_eq!(seven_eight.loop_length_frames(1), 168_000);
    }

    #[test]
    fn musical_position_from_frames() {
        let t = TransportState { bpm: 120.0, sample_rate: 48_000, ..Default::default() };
        let p = t.musical_position(0);
        assert_eq!((p.bar, p.beat), (1, 1));
        let p = t.musical_position(24_000 * 5 + 12_000);
        assert_eq!((p.bar, p.beat), (2, 2));
        assert!((p.fraction - 0.5).abs() < 1e-9);
    }
}
