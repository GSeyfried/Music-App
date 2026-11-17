//! Onset detection from signal energy.
//!
//! The signal is cut into short hops, and the level is measured over the
//! last four hops (20 ms by default). An onset is reported when that level
//! rises by at least `rise_db` above the *quietest* level in the preceding
//! `lookback_seconds`, provided it is above `floor_db` and no onset was
//! reported in the last `min_interval_seconds`.
//!
//! The level window spans about a period of the lowest notes. Measured over
//! a single 5 ms hop, the energy of a sustained 65 Hz tone swings by about
//! 4 dB with the phase of the waveform; over 20 ms the swing is under 1 dB,
//! which leaves a wide margin below `rise_db`.
//!
//! Comparing against the recent minimum (rather than the previous hop)
//! catches attacks that take several hops to build, such as a sung note,
//! while a sustained or slowly swelling sound never triggers.
//!
//! **What this detects:** attacks with a level jump: percussion, plucked and
//! struck notes, claps, consonants, notes after a rest. **What it does not:**
//! legato pitch changes at constant level; those come from
//! [`crate::segment::NoteSegmenter`], which sees pitch. A spectral-flux
//! detector would catch more soft onsets and is the planned upgrade if this
//! proves insufficient on real recordings.
//!
//! Time resolution is one hop (5 ms by default): the reported frame is the
//! start of the hop in which the rise was seen.
//!
//! Allocation-free after construction.

use serde::{Deserialize, Serialize};

/// Longest supported lookback, in hops.
const MAX_LOOKBACK: usize = 32;
/// Hops in the level-measurement window.
const LEVEL_HOPS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OnsetConfig {
    pub hop_seconds: f32,
    /// Required level rise over the quietest recent hop.
    pub rise_db: f32,
    /// Hops quieter than this never count as onsets (dBFS RMS).
    pub floor_db: f32,
    /// How far back the quietest hop is searched.
    pub lookback_seconds: f32,
    /// Refractory time after an onset.
    pub min_interval_seconds: f32,
}

impl Default for OnsetConfig {
    fn default() -> Self {
        Self { hop_seconds: 0.005, rise_db: 9.0, floor_db: -45.0, lookback_seconds: 0.02, min_interval_seconds: 0.06 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Onset {
    /// Input-clock frame at the start of the hop where the attack was seen.
    pub frame: u64,
    /// Level of that hop, dBFS RMS.
    pub level_db: f32,
    /// Rise over the quietest recent hop, dB.
    pub rise_db: f32,
}

pub struct OnsetDetector {
    config: OnsetConfig,
    hop: usize,
    lookback: usize,
    min_interval: u64,
    /// Levels of the most recent hops, newest last.
    recent: [f32; MAX_LOOKBACK],
    recent_len: usize,
    /// Energy of the last hops, newest last.
    hop_energy: [f64; LEVEL_HOPS],
    hop_energy_len: usize,
    energy: f64,
    filled: usize,
    hop_start: u64,
    last_onset: Option<u64>,
}

const SILENCE_DB: f32 = -120.0;

impl OnsetDetector {
    pub fn new(config: OnsetConfig, sample_rate: u32) -> Result<Self, &'static str> {
        let sr = sample_rate as f32;
        let ok = |v: f32| v.is_finite() && v > 0.0;
        if sample_rate == 0 || !ok(config.hop_seconds) || !ok(config.lookback_seconds) || !ok(config.rise_db) {
            return Err("sample rate, hop, lookback, and rise must be positive");
        }
        if !config.floor_db.is_finite() || config.min_interval_seconds.is_nan() || config.min_interval_seconds < 0.0 {
            return Err("floor_db must be finite and min_interval_seconds non-negative");
        }
        let hop = ((config.hop_seconds * sr).round() as usize).max(1);
        let lookback = ((config.lookback_seconds / config.hop_seconds).round() as usize).clamp(1, MAX_LOOKBACK);
        Ok(Self {
            config,
            hop,
            lookback,
            min_interval: (config.min_interval_seconds * sr).round() as u64,
            recent: [SILENCE_DB; MAX_LOOKBACK],
            recent_len: 0,
            hop_energy: [0.0; LEVEL_HOPS],
            hop_energy_len: 0,
            energy: 0.0,
            filled: 0,
            hop_start: 0,
            last_onset: None,
        })
    }

    pub fn hop_frames(&self) -> usize {
        self.hop
    }

    /// Forget history and restart the frame clock at `frame`.
    pub fn reset(&mut self, frame: u64) {
        self.recent_len = 0;
        self.hop_energy_len = 0;
        self.energy = 0.0;
        self.filled = 0;
        self.hop_start = frame;
        self.last_onset = None;
    }

    /// Feed mono samples; `emit` is called for each onset.
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(Onset)) {
        for &s in samples {
            self.energy += (s as f64) * (s as f64);
            self.filled += 1;
            if self.filled == self.hop {
                if self.hop_energy_len == LEVEL_HOPS {
                    self.hop_energy.copy_within(1.., 0);
                    self.hop_energy_len -= 1;
                }
                self.hop_energy[self.hop_energy_len] = self.energy;
                self.hop_energy_len += 1;
                let window = &self.hop_energy[..self.hop_energy_len];
                let mean_square = (window.iter().sum::<f64>() / (window.len() * self.hop) as f64) as f32;
                let level = if mean_square > 1e-12 { 10.0 * mean_square.log10() } else { SILENCE_DB };
                if let Some(onset) = self.hop_done(level) {
                    emit(onset);
                }
                self.energy = 0.0;
                self.filled = 0;
                self.hop_start += self.hop as u64;
            }
        }
    }

    fn hop_done(&mut self, level: f32) -> Option<Onset> {
        // Before any history exists, the reference is silence: sound that is
        // already playing when analysis starts counts as an onset.
        let quietest = self.recent[..self.recent_len].iter().copied().fold(f32::MAX, f32::min);
        let quietest = if self.recent_len == 0 { SILENCE_DB } else { quietest };
        let rise = level - quietest;

        if self.recent_len == self.lookback {
            self.recent.copy_within(1..self.lookback, 0);
            self.recent_len -= 1;
        }
        self.recent[self.recent_len] = level;
        self.recent_len += 1;

        let clear = self.last_onset.is_none_or(|last| self.hop_start - last >= self.min_interval);
        if level >= self.config.floor_db && rise >= self.config.rise_db && clear {
            self.last_onset = Some(self.hop_start);
            return Some(Onset { frame: self.hop_start, level_db: level, rise_db: rise });
        }
        None
    }
}

/// Detect every onset in a whole signal (offline convenience).
pub fn detect_onsets(samples: &[f32], sample_rate: u32, config: OnsetConfig) -> Result<Vec<Onset>, &'static str> {
    let mut detector = OnsetDetector::new(config, sample_rate)?;
    let mut onsets = Vec::new();
    detector.push(samples, |o| onsets.push(o));
    Ok(onsets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::{self, VOICE_PARTIALS};

    const SR: u32 = 48_000;
    const HOP: i64 = 240;

    fn frames(signal: &[f32]) -> Vec<i64> {
        detect_onsets(signal, SR, OnsetConfig::default()).unwrap().iter().map(|o| o.frame as i64).collect()
    }

    /// Place `sound` at each of `starts` in a silent signal of `total` frames.
    fn place(total: usize, starts: &[usize], sound: &[f32]) -> Vec<f32> {
        let mut out = signal::silence(total);
        for &start in starts {
            for (i, &s) in sound.iter().enumerate() {
                if start + i < total {
                    out[start + i] += s;
                }
            }
        }
        out
    }

    fn assert_near(found: &[i64], expected: &[usize]) {
        assert_eq!(found.len(), expected.len(), "found {found:?}, expected {expected:?}");
        for (f, &e) in found.iter().zip(expected) {
            // The onset is reported at the start of the hop containing the attack.
            let error = f - e as i64;
            assert!((-HOP..=0).contains(&error), "onset at {f}, attack at {e} (error {error} frames)");
        }
    }

    #[test]
    fn percussive_hits_are_found_within_one_hop() {
        // Hits at 120 BPM, deliberately not aligned to the hop grid.
        let starts = [5_000usize, 29_011, 53_023, 77_077, 101_101];
        let hit = signal::burst(42, 4_800, 0.8, 600.0);
        assert_near(&frames(&place(125_000, &starts, &hit)), &starts);
    }

    #[test]
    fn sung_notes_after_rests_are_found_once_each() {
        // 30 ms attack: slower than one hop, so the rise spans several hops.
        let starts = [4_800usize, 33_600, 62_400];
        let note = signal::shape(signal::harmonic(220.0, SR, 19_200, &VOICE_PARTIALS), 1_440, 1_440);
        let found = frames(&place(90_000, &starts, &note));
        assert_eq!(found.len(), 3, "{found:?}");
        for (f, &e) in found.iter().zip(&starts) {
            // A slow attack is seen once it has risen 9 dB above the floor:
            // early in the ramp, never before it and well before its end.
            assert!((0..1_440).contains(&(f - e as i64)), "onset {f} for a note starting at {e}");
        }
    }

    #[test]
    fn sustained_and_vibrato_tones_trigger_only_at_their_start() {
        let steady = signal::harmonic(330.0, SR, 96_000, &VOICE_PARTIALS);
        assert_eq!(frames(&steady), vec![0], "sound already playing when analysis starts is one onset");
        // Low notes, where a hop is a fraction of one period.
        for hz in [65.4, 82.4, 110.0] {
            assert_eq!(frames(&signal::sine(hz, SR, 96_000, 0.5)), vec![0], "{hz} Hz");
        }
        let vibrato = signal::vibrato(330.0, 80.0, 6.0, SR, 96_000, &VOICE_PARTIALS);
        assert_eq!(frames(&vibrato), vec![0]);
        // A slow swell (20 dB over two seconds) is not an attack.
        let swell: Vec<f32> = steady.iter().enumerate().map(|(i, s)| s * (0.1 + 0.9 * i as f32 / 96_000.0)).collect();
        assert_eq!(frames(&swell), vec![0]);
    }

    #[test]
    fn silence_and_steady_background_noise_trigger_nothing_further() {
        assert!(frames(&signal::silence(48_000)).is_empty());
        // Below the floor: never an onset.
        assert!(frames(&signal::noise(3, 48_000, 0.005)).is_empty());
        // Audible steady noise: only the start.
        assert_eq!(frames(&signal::noise(3, 96_000, 0.2)), vec![0]);
    }

    #[test]
    fn a_new_attack_over_a_ringing_note_is_found() {
        // Plucks 250 ms apart whose tails still ring (decay constant 100 ms).
        let starts = [2_400usize, 14_400, 26_400, 38_400];
        let pluck: Vec<f32> = signal::harmonic(196.0, SR, 24_000, &VOICE_PARTIALS)
            .iter()
            .enumerate()
            .map(|(i, s)| s * (-(i as f32) / 4_800.0).exp())
            .collect();
        assert_near(&frames(&place(72_000, &starts, &pluck)), &starts);
    }

    #[test]
    fn refractory_time_merges_a_flam_and_chunking_is_irrelevant() {
        // Two hits 20 ms apart count once; a third 100 ms later counts again.
        let starts = [4_800usize, 5_760, 10_560];
        let hit = signal::burst(7, 480, 0.8, 120.0);
        let signal = place(24_000, &starts, &hit);
        let whole = detect_onsets(&signal, SR, OnsetConfig::default()).unwrap();
        assert_eq!(whole.len(), 2, "{whole:?}");

        let mut detector = OnsetDetector::new(OnsetConfig::default(), SR).unwrap();
        let mut pieces = Vec::new();
        for chunk in signal.chunks(7) {
            detector.push(chunk, |o| pieces.push(o));
        }
        assert_eq!(whole, pieces);

        detector.reset(1_000_000);
        detector.push(&signal, |o| pieces.push(o));
        assert_eq!(pieces[2].frame, 1_000_000 + whole[0].frame);
    }

    #[test]
    fn rejects_bad_configuration() {
        let bad = |c: OnsetConfig| OnsetDetector::new(c, SR).is_err();
        assert!(bad(OnsetConfig { hop_seconds: 0.0, ..OnsetConfig::default() }));
        assert!(bad(OnsetConfig { rise_db: -3.0, ..OnsetConfig::default() }));
        assert!(bad(OnsetConfig { floor_db: f32::NAN, ..OnsetConfig::default() }));
        assert!(OnsetDetector::new(OnsetConfig::default(), 0).is_err());
    }
}
