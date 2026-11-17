//! Frequency ↔ note arithmetic (12-tone equal temperament).
//!
//! Pitches are carried as *fractional MIDI note numbers* (`69.0` = A4,
//! `69.5` = a quarter-tone above). That is the natural unit for everything
//! downstream: a difference of 1.0 is a semitone, 0.01 is a cent.

use serde::{Deserialize, Serialize};

/// Concert pitch reference. 440 Hz unless the user tunes differently.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Tuning {
    pub a4_hz: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { a4_hz: 440.0 }
    }
}

impl Tuning {
    /// Fractional MIDI note number of `hz`. `None` for non-positive or non-finite input.
    pub fn hz_to_midi(&self, hz: f32) -> Option<f32> {
        (hz.is_finite() && hz > 0.0 && self.a4_hz > 0.0).then(|| 69.0 + 12.0 * (hz / self.a4_hz).log2())
    }

    pub fn midi_to_hz(&self, midi: f32) -> f32 {
        self.a4_hz * 2f32.powf((midi - 69.0) / 12.0)
    }
}

pub const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// A pitch resolved to the nearest equal-tempered note.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteReading {
    /// Nearest MIDI note number (60 = C4).
    pub note: i32,
    /// Deviation from that note in cents, in `[-50, +50)`. Positive = sharp.
    pub cents: f32,
}

impl NoteReading {
    pub fn from_midi(midi: f32) -> Self {
        let note = (midi + 0.5).floor();
        Self { note: note as i32, cents: (midi - note) * 100.0 }
    }

    /// Pitch class 0..=11 (0 = C).
    pub fn pitch_class(&self) -> u8 {
        self.note.rem_euclid(12) as u8
    }

    /// Scientific octave number (MIDI 60 = C4).
    pub fn octave(&self) -> i32 {
        self.note.div_euclid(12) - 1
    }

    /// Name such as `A4` or `C#3`.
    pub fn name(&self) -> String {
        format!("{}{}", NOTE_NAMES[self.pitch_class() as usize], self.octave())
    }
}

/// Difference `a - b` in cents between two frequencies.
pub fn cents_between(a_hz: f32, b_hz: f32) -> Option<f32> {
    (a_hz > 0.0 && b_hz > 0.0 && a_hz.is_finite() && b_hz.is_finite()).then(|| 1200.0 * (a_hz / b_hz).log2())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_pitches() {
        let t = Tuning::default();
        assert_eq!(t.hz_to_midi(440.0), Some(69.0));
        assert!((t.hz_to_midi(261.625_58).unwrap() - 60.0).abs() < 1e-4);
        assert!((t.midi_to_hz(60.0) - 261.625_58).abs() < 1e-2);
        assert!((t.midi_to_hz(57.0) - 220.0).abs() < 1e-3);
        assert_eq!(t.hz_to_midi(0.0), None);
        assert_eq!(t.hz_to_midi(-5.0), None);
        assert_eq!(t.hz_to_midi(f32::NAN), None);
    }

    #[test]
    fn alternate_tuning_shifts_everything() {
        let baroque = Tuning { a4_hz: 415.0 };
        assert_eq!(baroque.hz_to_midi(415.0), Some(69.0));
        let m = baroque.hz_to_midi(440.0).unwrap();
        assert!((m - 70.013).abs() < 0.01, "440 Hz is about a semitone sharp at A=415: {m}");
    }

    #[test]
    fn nearest_note_and_cents() {
        let r = NoteReading::from_midi(69.0);
        assert_eq!((r.note, r.cents, r.name().as_str()), (69, 0.0, "A4"));
        let r = NoteReading::from_midi(60.3);
        assert_eq!(r.note, 60);
        assert!((r.cents - 30.0).abs() < 1e-3);
        let r = NoteReading::from_midi(60.6);
        assert_eq!((r.note, r.name().as_str()), (61, "C#4"));
        assert!((r.cents + 40.0).abs() < 1e-3);
        // Exactly halfway rounds up, keeping cents in [-50, 50).
        let r = NoteReading::from_midi(60.5);
        assert_eq!((r.note, r.cents), (61, -50.0));
        assert_eq!(NoteReading::from_midi(21.0).name(), "A0");
        assert_eq!(NoteReading::from_midi(11.0).name(), "B-1");
    }

    #[test]
    fn cents_between_frequencies() {
        assert!((cents_between(880.0, 440.0).unwrap() - 1200.0).abs() < 1e-3);
        assert!((cents_between(440.0, 466.163_76).unwrap() + 100.0).abs() < 1e-2);
        assert_eq!(cents_between(440.0, 0.0), None);
    }
}
