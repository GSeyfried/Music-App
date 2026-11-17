//! Streaming pitch tracking: feeds overlapping frames to [`PitchDetector`]
//! and adds what a single frame cannot know.
//!
//! - **Smoothing.** A short median over voiced frames removes single-frame
//!   outliers without blurring note changes (a median, unlike an average,
//!   never invents in-between pitches). It delays changes by
//!   `median_frames / 2` hops.
//! - **Octave-jump correction.** A frame that lands a whole number of octaves
//!   from the recent pitch is almost always an estimator slip, not a
//!   performance: it is folded back. If the jump persists for
//!   `octave_hold_frames` it is accepted as a real leap (so genuine octave
//!   leaps are reported, `octave_hold_frames` hops late).
//! - **Stability.** Standard deviation, in cents, of the recent smoothed
//!   pitch within the current note. Vibrato reads as its extent; a held
//!   steady note reads near zero.
//! - **Honest gaps.** Silence and unpitched sound produce frames with no
//!   pitch and say why; nothing is interpolated across them.
//!
//! Not realtime-safe by contract (it is cheap and allocation-free after
//! construction, but too slow for an audio callback). Run it on an analysis
//! thread fed from a ring buffer.

use serde::{Deserialize, Serialize};

use crate::note::{NoteReading, Tuning};
use crate::pitch::{PitchConfig, PitchDetector, PitchEstimate, Unpitched};

/// Longest supported median window.
pub const MAX_MEDIAN: usize = 15;
/// Pitch history survives this many consecutive unpitched frames (one
/// dropout). Any longer gap is a rest: carrying the old pitch across it made
/// the median drag the first frame of the next note back to the previous one.
const GAP_RESET_FRAMES: u32 = 1;
/// Tolerance, in semitones, for calling a jump "a whole number of octaves".
const OCTAVE_TOLERANCE: f32 = 0.7;
/// A smoothed-pitch step larger than this (semitones) starts a new stability window.
const NOTE_CHANGE_SEMITONES: f32 = 0.8;
const MIN_STABILITY_FRAMES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackerConfig {
    pub pitch: PitchConfig,
    /// Time between pitch frames.
    pub hop_seconds: f32,
    /// Median window in frames (odd, 1 = off, at most [`MAX_MEDIAN`]).
    pub median_frames: usize,
    /// Frames an octave jump must persist before it is believed (0 = never correct).
    pub octave_hold_frames: u32,
    /// Frames of history for the stability measure.
    pub stability_frames: usize,
    pub tuning: Tuning,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        Self {
            pitch: PitchConfig::default(),
            hop_seconds: 0.01,
            median_frames: 3,
            octave_hold_frames: 8,
            stability_frames: 25,
            tuning: Tuning::default(),
        }
    }
}

/// One analysed frame, ready for display or note segmentation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PitchFrame {
    /// Input-clock frame at the centre of the analysed window.
    pub frame: u64,
    /// Smoothed fundamental, when confidently pitched.
    pub hz: Option<f32>,
    /// Same pitch as a fractional MIDI note number.
    pub midi: Option<f32>,
    /// Nearest note and deviation in cents.
    pub note: Option<NoteReading>,
    /// Periodicity 0..=1 of this frame (the confidence of the estimate).
    pub clarity: f32,
    pub rms_db: f32,
    /// Standard deviation of recent pitch in cents (absent until enough history).
    pub stability_cents: Option<f32>,
    pub unpitched: Option<Unpitched>,
    /// True when this frame was folded back by octave-jump correction.
    pub octave_corrected: bool,
}

pub struct PitchTracker {
    detector: PitchDetector,
    config: TrackerConfig,
    hop: usize,
    buf: Vec<f32>,
    /// Input-clock frame of `buf[0]`.
    buf_start: u64,
    recent: Vec<f32>,
    stability: Vec<f32>,
    gap: u32,
    octave_run: u32,
}

impl PitchTracker {
    pub fn new(config: TrackerConfig, sample_rate: u32) -> Result<Self, &'static str> {
        if config.median_frames == 0 || config.median_frames > MAX_MEDIAN || config.median_frames.is_multiple_of(2) {
            return Err("median_frames must be odd and between 1 and 15");
        }
        if !(config.hop_seconds.is_finite() && config.hop_seconds > 0.0 && config.hop_seconds <= 0.1) {
            return Err("hop_seconds must be in (0, 0.1]");
        }
        if config.stability_frames < MIN_STABILITY_FRAMES || config.stability_frames > 1_000 {
            return Err("stability_frames must be between 5 and 1000");
        }
        if !(config.tuning.a4_hz.is_finite() && (300.0..=600.0).contains(&config.tuning.a4_hz)) {
            return Err("a4_hz must be between 300 and 600");
        }
        let detector = PitchDetector::new(config.pitch, sample_rate)?;
        let frame_len = detector.frame_len();
        let hop = ((config.hop_seconds * sample_rate as f32).round() as usize).clamp(1, frame_len);
        Ok(Self {
            detector,
            config,
            hop,
            buf: Vec::with_capacity(frame_len),
            buf_start: 0,
            recent: Vec::with_capacity(config.median_frames),
            stability: Vec::with_capacity(config.stability_frames),
            gap: 0,
            octave_run: 0,
        })
    }

    pub fn config(&self) -> &TrackerConfig {
        &self.config
    }

    /// Input frames between consecutive pitch frames.
    pub fn hop_frames(&self) -> usize {
        self.hop
    }

    /// Input frames needed before the first pitch frame (the analysis latency).
    pub fn frame_len(&self) -> usize {
        self.detector.frame_len()
    }

    /// Forget all audio and pitch history, and restart the frame clock at `frame`.
    pub fn reset(&mut self, frame: u64) {
        self.buf.clear();
        self.buf_start = frame;
        self.forget();
    }

    fn forget(&mut self) {
        self.recent.clear();
        self.stability.clear();
        self.octave_run = 0;
    }

    /// Feed mono samples; `emit` is called once per completed hop.
    pub fn push(&mut self, mut samples: &[f32], mut emit: impl FnMut(PitchFrame)) {
        let frame_len = self.detector.frame_len();
        while !samples.is_empty() {
            let take = (frame_len - self.buf.len()).min(samples.len());
            self.buf.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.buf.len() == frame_len {
                let estimate = self.detector.detect(&self.buf);
                let centre = self.buf_start + (self.detector.window_len() / 2) as u64;
                emit(self.post(estimate, centre));
                self.buf.copy_within(self.hop.., 0);
                self.buf.truncate(frame_len - self.hop);
                self.buf_start += self.hop as u64;
            }
        }
    }

    fn post(&mut self, est: PitchEstimate, frame: u64) -> PitchFrame {
        let mut out = PitchFrame {
            frame,
            hz: None,
            midi: None,
            note: None,
            clarity: est.clarity,
            rms_db: est.rms_db,
            stability_cents: None,
            unpitched: est.unpitched,
            octave_corrected: false,
        };
        let Some(raw) = est.hz.and_then(|hz| self.config.tuning.hz_to_midi(hz)) else {
            self.gap += 1;
            if self.gap > GAP_RESET_FRAMES {
                self.forget();
            }
            out.unpitched = Some(est.unpitched.unwrap_or(Unpitched::Aperiodic));
            return out;
        };
        self.gap = 0;

        let mut midi = raw;
        if self.config.octave_hold_frames > 0 && self.recent.len() >= self.config.median_frames.min(3) {
            let jump = midi - median(&self.recent);
            let octaves = (jump / 12.0).round();
            if octaves != 0.0 && (jump - octaves * 12.0).abs() <= OCTAVE_TOLERANCE {
                if self.octave_run < self.config.octave_hold_frames {
                    self.octave_run += 1;
                    midi -= octaves * 12.0;
                    out.octave_corrected = true;
                } else {
                    // It persisted: a real leap. Start over in the new register.
                    self.forget();
                }
            } else {
                self.octave_run = 0;
            }
        }

        if self.recent.len() == self.config.median_frames {
            self.recent.remove(0);
        }
        self.recent.push(midi);
        let smoothed = median(&self.recent);

        if let Some(&last) = self.stability.last() {
            if (smoothed - last).abs() > NOTE_CHANGE_SEMITONES {
                self.stability.clear();
            }
        }
        if self.stability.len() == self.config.stability_frames {
            self.stability.remove(0);
        }
        self.stability.push(smoothed);
        if self.stability.len() >= MIN_STABILITY_FRAMES {
            out.stability_cents = Some(std_dev(&self.stability) * 100.0);
        }

        out.midi = Some(smoothed);
        out.hz = Some(self.config.tuning.midi_to_hz(smoothed));
        out.note = Some(NoteReading::from_midi(smoothed));
        out
    }
}

/// Median of up to [`MAX_MEDIAN`] values (upper middle for even counts). No allocation.
fn median(values: &[f32]) -> f32 {
    let n = values.len().min(MAX_MEDIAN);
    let mut sorted = [0.0f32; MAX_MEDIAN];
    sorted[..n].copy_from_slice(&values[..n]);
    sorted[..n].sort_unstable_by(f32::total_cmp);
    sorted[n / 2]
}

/// Population standard deviation.
pub(crate) fn std_dev(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let n = values.len() as f32;
    let mean = values.iter().sum::<f32>() / n;
    (values.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n).sqrt()
}

#[cfg(test)]
mod tests {
    //! Expected values are from the numerical model of this crate (noted
    //! beside each assertion); tolerances leave a margin of at least 2x.

    use super::*;
    use crate::signal::{self, VOICE_PARTIALS};

    const SR: u32 = 48_000;

    fn run(signal: &[f32]) -> Vec<PitchFrame> {
        let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
        let mut frames = Vec::new();
        // Odd-sized chunks: results must not depend on how audio is delivered.
        for chunk in signal.chunks(333) {
            tracker.push(chunk, |f| frames.push(f));
        }
        frames
    }

    fn estimate(hz: f32) -> PitchEstimate {
        PitchEstimate { hz: Some(hz), clarity: 0.95, rms_db: -12.0, unpitched: None }
    }

    #[test]
    fn steady_note_is_steady() {
        let frames = run(&signal::harmonic(220.0, SR, SR as usize, &VOICE_PARTIALS));
        // (48000 - 1700) / 480 + 1 frames, the first centred on half a window.
        assert_eq!(frames.len(), 97);
        assert_eq!(frames[0].frame, 480);
        assert_eq!(frames[1].frame - frames[0].frame, 480);
        for f in &frames {
            let note = f.note.expect("every frame is pitched");
            assert_eq!(note.note, 57, "A3");
            assert!(note.cents.abs() < 1.0, "model: 0.02 cents, got {}", note.cents);
            assert!((f.hz.unwrap() - 220.0).abs() < 0.2);
            assert!(!f.octave_corrected);
            assert_eq!(f.unpitched, None);
        }
        assert_eq!(frames[3].stability_cents, None, "not enough history yet");
        let worst = frames.iter().filter_map(|f| f.stability_cents).fold(0.0, f32::max);
        assert!(worst < 0.5, "model: 0.01 cents, got {worst}");
    }

    #[test]
    fn chunking_does_not_change_results() {
        let signal = signal::vibrato(330.0, 30.0, 5.0, SR, 20_000, &VOICE_PARTIALS);
        let mut whole = Vec::new();
        PitchTracker::new(TrackerConfig::default(), SR).unwrap().push(&signal, |f| whole.push(f));
        let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
        let mut pieces = Vec::new();
        for chunk in signal.chunks(1) {
            tracker.push(chunk, |f| pieces.push(f));
        }
        assert_eq!(whole, pieces);
        assert!(whole.len() > 30);
    }

    #[test]
    fn vibrato_is_followed_and_reads_as_instability() {
        // A4 with +-50 cents of vibrato at 5.5 Hz.
        let frames = run(&signal::vibrato(440.0, 50.0, 5.5, SR, 2 * SR as usize, &VOICE_PARTIALS));
        let cents: Vec<f32> = frames.iter().map(|f| (f.midi.expect("pitched") - 69.0) * 100.0).collect();
        let (lo, hi) = cents.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &c| (lo.min(c), hi.max(c)));
        // Model: -48.1 .. +48.5 (the analysis window averages a little of the swing).
        assert!((-56.0..=-40.0).contains(&lo), "lowest {lo}");
        assert!((40.0..=56.0).contains(&hi), "highest {hi}");
        let mean = cents.iter().sum::<f32>() / cents.len() as f32;
        assert!(mean.abs() < 5.0, "centred on the note (model 0.7): {mean}");
        // A full 250 ms window covers more than one vibrato cycle: about 50 / sqrt(2) = 35 cents.
        for f in &frames[40..] {
            let s = f.stability_cents.unwrap();
            assert!((25.0..=45.0).contains(&s), "model 33-36, got {s}");
        }
    }

    #[test]
    fn glide_is_tracked_continuously() {
        // C4 to C5 in two seconds (6 semitones per second).
        let frames = run(&signal::glide(261.63, 523.25, SR, 2 * SR as usize, &VOICE_PARTIALS));
        let mut previous = 0.0;
        for f in &frames {
            let midi = f.midi.expect("pitched throughout");
            let truth = 60.0 + 12.0 * (f.frame as f32 / SR as f32 / 2.0);
            // Model worst case 5.8 cents (the median lags a rising pitch by one hop).
            assert!((midi - truth).abs() < 0.15, "at {}: {midi} vs {truth}", f.frame);
            assert!(midi >= previous, "monotonic");
            assert!(!f.octave_corrected);
            previous = midi;
        }
    }

    #[test]
    fn silence_and_noise_produce_no_pitch_and_say_why() {
        for f in run(&signal::silence(20_000)) {
            assert_eq!((f.hz, f.midi, f.note, f.stability_cents), (None, None, None, None));
            assert_eq!(f.unpitched, Some(Unpitched::Silence));
        }
        for f in run(&signal::noise(99, 20_000, 0.5)) {
            assert_eq!(f.midi, None);
            assert_eq!(f.unpitched, Some(Unpitched::Aperiodic));
        }
    }

    #[test]
    fn isolated_octave_slips_are_folded_back() {
        let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
        let mut input = [220.0f32; 40];
        input[20] = 440.0; // two frames an octave up
        input[21] = 440.0;
        input[30] = 110.0; // one frame an octave down
        let out: Vec<PitchFrame> =
            input.iter().enumerate().map(|(i, &hz)| tracker.post(estimate(hz), i as u64 * 480)).collect();
        for f in &out {
            assert!((f.midi.unwrap() - 57.0).abs() < 1e-3, "stays on A3: {:?}", f.midi);
        }
        let flagged: Vec<usize> = out.iter().enumerate().filter(|(_, f)| f.octave_corrected).map(|(i, _)| i).collect();
        assert_eq!(flagged, vec![20, 21, 30]);
    }

    #[test]
    fn a_sustained_octave_leap_is_accepted_after_the_hold() {
        let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
        let input = [vec![220.0f32; 20], vec![440.0f32; 30]].concat();
        let out: Vec<f32> =
            input.iter().enumerate().map(|(i, &hz)| tracker.post(estimate(hz), i as u64 * 480).midi.unwrap()).collect();
        // Frames 20..28 are held at A3 (octave_hold_frames = 8); from 28 on it is A4.
        assert!(out[..28].iter().all(|m| (m - 57.0).abs() < 1e-3));
        assert!(out[28..].iter().all(|m| (m - 69.0).abs() < 1e-3));

        // With correction disabled the leap is reported as soon as the median allows.
        let config = TrackerConfig { octave_hold_frames: 0, ..TrackerConfig::default() };
        let mut tracker = PitchTracker::new(config, SR).unwrap();
        let out: Vec<f32> =
            input.iter().enumerate().map(|(i, &hz)| tracker.post(estimate(hz), i as u64 * 480).midi.unwrap()).collect();
        assert!((out[21] - 69.0).abs() < 1e-3, "median of 3 flips on the second new frame");
    }

    #[test]
    fn history_does_not_leak_across_a_rest() {
        let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
        let silent = PitchEstimate { hz: None, clarity: 0.0, rms_db: -120.0, unpitched: Some(Unpitched::Silence) };
        for i in 0..10 {
            tracker.post(estimate(261.63), i * 480);
        }
        tracker.post(silent, 4_800);
        tracker.post(silent, 5_280);
        // First frame of the next note, a major third up: reported as itself,
        // not dragged to the old note by the median.
        let f = tracker.post(estimate(329.63), 5_760);
        assert_eq!(f.note.unwrap().note, 64);
        assert_eq!(f.stability_cents, None);
    }

    #[test]
    fn reset_restarts_the_clock_and_rejects_bad_config() {
        let mut tracker = PitchTracker::new(TrackerConfig::default(), SR).unwrap();
        let tone = signal::sine(440.0, SR, 4_000, 0.5);
        let mut frames = Vec::new();
        tracker.push(&tone, |f| frames.push(f));
        tracker.reset(1_000_000);
        tracker.push(&tone, |f| frames.push(f));
        assert_eq!(frames.len(), 10);
        assert_eq!(frames[5].frame, 1_000_480);

        let bad = |c: TrackerConfig| PitchTracker::new(c, SR).is_err();
        assert!(bad(TrackerConfig { median_frames: 4, ..TrackerConfig::default() }));
        assert!(bad(TrackerConfig { median_frames: 17, ..TrackerConfig::default() }));
        assert!(bad(TrackerConfig { hop_seconds: 0.0, ..TrackerConfig::default() }));
        assert!(bad(TrackerConfig { stability_frames: 2, ..TrackerConfig::default() }));
        assert!(bad(TrackerConfig { tuning: Tuning { a4_hz: 0.0 }, ..TrackerConfig::default() }));
    }
}
