//! Transport settings and musical-time arithmetic.
//!
//! The authoritative clock is a frame counter driven by the audio output
//! callback (see `looper::processor`). Musical positions (bar/beat/subdivision)
//! are always *derived* from frames, never stored as the source of truth.
//!
//! **No drift rule:** every grid position is computed from an absolute index,
//! `round(index * step)`, where `step` is an exact `f64` frame count. Grid
//! positions are never produced by repeatedly adding a rounded step, so
//! rounding error never accumulates, however long the transport runs.
//!
//! A *beat* is one note of the time-signature denominator: 7/8 has seven
//! eighth-note beats per bar, and BPM counts those eighths.

use serde::{Deserialize, Serialize};

use crate::model::{FramePos, LoopMode};

/// A time signature such as 3/4, 4/4, 5/4 or 7/8. `denominator` is the note
/// value that receives one beat (a power of two).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl TimeSignature {
    pub const FOUR_FOUR: Self = Self { numerator: 4, denominator: 4 };

    pub fn new(numerator: u8, denominator: u8) -> Option<Self> {
        let sig = Self { numerator, denominator };
        sig.is_valid().then_some(sig)
    }

    pub fn is_valid(&self) -> bool {
        (1..=32).contains(&self.numerator) && self.denominator.is_power_of_two() && self.denominator <= 64
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
#[serde(rename_all = "camelCase", default)]
pub struct MetronomeSettings {
    /// Audible click while the transport runs (Metronome mode only).
    pub enabled: bool,
    /// Clicks per beat (1 = beats only, 2 = halves of a beat, 3 = triplets ...).
    pub subdivision: u8,
    pub accent_downbeat: bool,
    pub gain_db: f32,
    /// Count-in length in bars before the transport starts (Metronome mode).
    pub count_in_bars: u8,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self { enabled: true, subdivision: 1, accent_downbeat: true, gain_db: -6.0, count_in_bars: 1 }
    }
}

/// Loop length in Metronome mode: whole bars plus extra beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LoopLength {
    pub bars: u32,
    pub beats: u32,
}

impl Default for LoopLength {
    fn default() -> Self {
        Self { bars: 2, beats: 0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TransportState {
    pub play_state: PlayState,
    /// Beats per minute, where a beat is the time-signature denominator note.
    pub bpm: f64,
    pub time_signature: TimeSignature,
    pub metronome: MetronomeSettings,
    /// Free: the first recording sets the loop length. Synced ("Metronome mode"):
    /// the loop length comes from `bpm`, `time_signature` and `loop_length`.
    pub mode: LoopMode,
    pub loop_length: LoopLength,
    /// Current position on the authoritative clock (informational in persisted files).
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
            mode: LoopMode::Free,
            loop_length: LoopLength::default(),
            position: 0,
            sample_rate: 48_000,
        }
    }
}

/// Bar/beat/subdivision position derived from frames. All 1-based; `fraction`
/// is 0..1 within the current subdivision.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicalPosition {
    pub bar: u64,
    pub beat: u32,
    pub subdivision: u32,
    pub fraction: f64,
}

/// Frame of grid point `index` for a grid with spacing `step` (exact, absolute: no drift).
pub fn grid_frame(index: i64, step: f64) -> i64 {
    (index as f64 * step).round() as i64
}

/// Smallest grid index whose frame is `>= frame`.
pub fn first_grid_index_at_or_after(frame: i64, step: f64) -> i64 {
    let mut n = (frame as f64 / step).ceil() as i64;
    while grid_frame(n - 1, step) >= frame {
        n -= 1;
    }
    while grid_frame(n, step) < frame {
        n += 1;
    }
    n
}

impl TransportState {
    /// Rejects settings the engine cannot run. Bounds are deliberately generous.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(self.bpm.is_finite() && (20.0..=400.0).contains(&self.bpm)) {
            return Err("bpm must be between 20 and 400");
        }
        if !self.time_signature.is_valid() {
            return Err("time signature must be 1-32 over a power of two up to 64");
        }
        if !(1..=8).contains(&self.metronome.subdivision) {
            return Err("subdivision must be 1-8");
        }
        if self.metronome.count_in_bars > 8 {
            return Err("count-in must be 0-8 bars");
        }
        if !self.metronome.gain_db.is_finite() || self.metronome.gain_db > 12.0 {
            return Err("metronome gain must be finite and at most +12 dB");
        }
        if self.loop_length.bars > 256 || self.loop_length.beats > 64 || self.beats_per_loop() == 0 {
            return Err("loop length must be at least one beat and at most 256 bars");
        }
        Ok(())
    }

    pub fn frames_per_beat(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / self.bpm
    }

    pub fn frames_per_bar(&self) -> f64 {
        self.frames_per_beat() * self.time_signature.numerator as f64
    }

    pub fn frames_per_subdivision(&self) -> f64 {
        self.frames_per_beat() / self.metronome.subdivision.max(1) as f64
    }

    pub fn beats_per_loop(&self) -> u64 {
        self.loop_length.bars as u64 * self.time_signature.numerator as u64 + self.loop_length.beats as u64
    }

    /// Exact (fractional) loop period in frames for Metronome mode.
    pub fn loop_period(&self) -> f64 {
        self.beats_per_loop() as f64 * self.frames_per_beat()
    }

    /// Count-in duration in frames (rounded once, from the exact bar length).
    pub fn count_in_frames(&self) -> u64 {
        (self.metronome.count_in_bars as f64 * self.frames_per_bar()).round() as u64
    }

    /// Frame of the start of `bar` (0-based), rounded to the nearest frame.
    pub fn bar_start_frame(&self, bar: u64) -> FramePos {
        grid_frame(bar as i64, self.frames_per_bar()) as FramePos
    }

    /// Length in frames of a loop of `bars` whole bars.
    pub fn loop_length_frames(&self, bars: u32) -> FramePos {
        self.bar_start_frame(bars as u64)
    }

    pub fn musical_position(&self, frame: FramePos) -> MusicalPosition {
        let subs_per_beat = self.metronome.subdivision.max(1) as f64;
        let per_bar = self.time_signature.numerator as f64;
        let subs = frame as f64 / self.frames_per_subdivision();
        let sub_whole = subs.floor();
        let beats = (sub_whole / subs_per_beat).floor();
        let bar = (beats / per_bar).floor();
        MusicalPosition {
            bar: bar as u64 + 1,
            beat: (beats - bar * per_bar) as u32 + 1,
            subdivision: (sub_whole - beats * subs_per_beat) as u32 + 1,
            fraction: subs - sub_whole,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(bpm: f64, num: u8, den: u8, sample_rate: u32) -> TransportState {
        TransportState {
            bpm,
            time_signature: TimeSignature::new(num, den).unwrap(),
            sample_rate,
            ..Default::default()
        }
    }

    #[test]
    fn rejects_invalid_signatures() {
        assert!(TimeSignature::new(7, 8).is_some());
        assert!(TimeSignature::new(4, 3).is_none());
        assert!(TimeSignature::new(0, 4).is_none());
        assert!(TimeSignature::new(33, 4).is_none());
    }

    #[test]
    fn bar_lengths() {
        let t = TransportState { bpm: 120.0, sample_rate: 48_000, ..Default::default() };
        assert_eq!(t.frames_per_beat(), 24_000.0);
        assert_eq!(t.loop_length_frames(2), 192_000);

        let seven_eight = TransportState { time_signature: TimeSignature::new(7, 8).unwrap(), ..t };
        assert_eq!(seven_eight.loop_length_frames(1), 168_000);
    }

    #[test]
    fn common_meters_at_120_bpm_48k() {
        // (numerator, denominator, frames per bar)
        for (num, den, bar) in [(3u8, 4u8, 72_000.0), (4, 4, 96_000.0), (5, 4, 120_000.0), (7, 8, 168_000.0)] {
            let t = at(120.0, num, den, 48_000);
            assert_eq!(t.frames_per_bar(), bar, "{num}/{den}");
        }
    }

    #[test]
    fn loop_period_counts_bars_and_extra_beats() {
        let mut t = at(90.0, 5, 4, 44_100);
        t.loop_length = LoopLength { bars: 2, beats: 3 };
        assert_eq!(t.beats_per_loop(), 13);
        let expected = 13.0 * 44_100.0 * 60.0 / 90.0;
        assert!((t.loop_period() - expected).abs() < 1e-9);
    }

    #[test]
    fn count_in_frames_round_once() {
        let mut t = at(113.0, 4, 4, 44_100);
        t.metronome.count_in_bars = 2;
        let exact: f64 = 2.0 * 4.0 * 44_100.0 * 60.0 / 113.0;
        assert_eq!(t.count_in_frames(), exact.round() as u64);
    }

    #[test]
    fn musical_position_from_frames() {
        let t = TransportState { bpm: 120.0, sample_rate: 48_000, ..Default::default() };
        let p = t.musical_position(0);
        assert_eq!((p.bar, p.beat, p.subdivision), (1, 1, 1));
        let p = t.musical_position(24_000 * 5 + 12_000);
        assert_eq!((p.bar, p.beat), (2, 2));
        assert!((p.fraction - 0.5).abs() < 1e-9);
    }

    #[test]
    fn musical_position_with_subdivisions_in_7_8() {
        let mut t = at(240.0, 7, 8, 1_000); // 250 frames per eighth
        t.metronome.subdivision = 2; // 125 frames per subdivision
        let p = t.musical_position(7 * 250 + 3 * 250 + 130); // bar 2, beat 4, second half
        assert_eq!((p.bar, p.beat, p.subdivision), (2, 4, 2));
    }

    #[test]
    fn grid_does_not_accumulate_rounding_error() {
        let step = 44_100.0 * 60.0 / 113.0; // 23415.929... frames per beat
        let n = 10_000_000i64;
        assert_eq!(grid_frame(n, step), (n as f64 * step).round() as i64);
        // Consecutive differences are always floor or ceil of the step.
        for i in 0..5_000 {
            let d = grid_frame(i + 1, step) - grid_frame(i, step);
            assert!(d == step.floor() as i64 || d == step.ceil() as i64);
        }
    }

    #[test]
    fn first_grid_index_is_exact_at_boundaries() {
        let step = 2123.893_805_309_734_6;
        for i in 0..1_000 {
            let f = grid_frame(i, step);
            assert_eq!(first_grid_index_at_or_after(f, step), i);
            assert_eq!(first_grid_index_at_or_after(f + 1, step), i + 1);
        }
        assert_eq!(first_grid_index_at_or_after(-5, 100.0), 0);
    }

    #[test]
    fn validation() {
        let mut t = TransportState::default();
        assert!(t.validate().is_ok());
        t.bpm = 5.0;
        assert!(t.validate().is_err());
        t.bpm = 120.0;
        t.loop_length = LoopLength { bars: 0, beats: 0 };
        assert!(t.validate().is_err());
    }

    #[test]
    fn deserialises_partial_json_with_defaults() {
        let t: TransportState = serde_json::from_str(r#"{"bpm":90,"mode":"synced"}"#).unwrap();
        assert_eq!(t.bpm, 90.0);
        assert_eq!(t.mode, LoopMode::Synced);
        assert_eq!(t.loop_length, LoopLength::default());
    }
}
