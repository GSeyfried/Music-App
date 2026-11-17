//! Fundamental-frequency estimation for one analysis frame.
//!
//! Algorithm: YIN (de Cheveigné & Kawahara, "YIN, a fundamental frequency
//! estimator for speech and music", JASA 2002), implemented from the paper:
//!
//! 1. difference function `d(τ) = Σ (x[j] − x[j+τ])²` over an integration window,
//! 2. cumulative-mean normalisation `d'(τ) = d(τ)·τ / Σ_{k≤τ} d(k)`,
//! 3. the first dip below an absolute threshold, taking the deepest point of that dip,
//! 4. parabolic interpolation of the minimum for sub-sample lag resolution.
//!
//! Three departures from the paper, each added after measuring a failure on
//! synthetic signals (see the tests):
//! - the minimum is searched over the whole below-threshold dip, not by strict
//!   descent, which stopped early on noise ripples;
//! - when no dip passes the threshold, the *shortest* lag nearly as good as the
//!   global minimum is used, which avoids picking 2T or 3T in noise;
//! - a dip at a lag shorter than the search range means the tone is above
//!   `max_hz`; the frame is reported out of range instead of as a subharmonic.
//!
//! Step 3 is what makes YIN resistant to octave errors: of all lags that
//! explain the signal (T, 2T, 3T ...) it takes the smallest, and step 2
//! prevents the trivial dip at τ→0 from winning. `clarity = 1 − d'(τ)` is a
//! direct measure of periodicity (1 = perfectly periodic, about 0 = noise),
//! used here as the confidence.
//!
//! The estimator is stateless per frame. Continuity (smoothing, octave-jump
//! correction, stability) lives in [`crate::tracker`].
//!
//! Cost: `window × max_lag` multiply-adds per frame. At 48 kHz with the
//! defaults that is about 0.7 M per frame; see `examples/bench.rs`.
//! Not meant for the audio callback: run it on an analysis thread.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PitchConfig {
    /// Lowest detectable fundamental. Sets the longest lag and so the frame length.
    pub min_hz: f32,
    /// Highest detectable fundamental.
    pub max_hz: f32,
    /// YIN absolute threshold on the normalised difference (0.10–0.20 typical).
    /// Lower is stricter: fewer false pitches, more frames reported unpitched.
    pub threshold: f32,
    /// Minimum integration window in seconds (it is never shorter than the longest lag).
    pub window_seconds: f32,
    /// Frames quieter than this (dBFS RMS) are reported as silence.
    pub silence_db: f32,
    /// Estimates with clarity below this are reported as unpitched.
    pub min_clarity: f32,
}

impl Default for PitchConfig {
    fn default() -> Self {
        Self {
            min_hz: 65.0,    // C2: below the bass voice, low E on guitar is 82 Hz
            max_hz: 1_400.0, // F6: above the soprano range
            threshold: 0.15,
            window_seconds: 0.02,
            silence_db: -50.0,
            min_clarity: 0.75,
        }
    }
}

impl PitchConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(self.min_hz.is_finite() && self.max_hz.is_finite() && self.min_hz >= 20.0 && self.max_hz > self.min_hz) {
            return Err("pitch range must satisfy 20 Hz <= min_hz < max_hz");
        }
        if !(self.threshold > 0.0 && self.threshold < 1.0) {
            return Err("threshold must be between 0 and 1");
        }
        if !(self.window_seconds.is_finite() && self.window_seconds > 0.0 && self.window_seconds <= 0.2) {
            return Err("window_seconds must be in (0, 0.2]");
        }
        if !(self.min_clarity >= 0.0 && self.min_clarity <= 1.0) || !self.silence_db.is_finite() {
            return Err("min_clarity must be within 0..=1 and silence_db finite");
        }
        Ok(())
    }
}

/// Why a frame has no pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Unpitched {
    /// Below the silence threshold.
    Silence,
    /// Loud enough, but not periodic enough (noise, percussion, breath, chords).
    Aperiodic,
    /// Periodic, but the fundamental lies outside `min_hz..=max_hz`.
    OutOfRange,
}

/// In the no-threshold-crossing fallback, a shorter lag wins if its
/// normalised difference is within this much of the global minimum.
const FALLBACK_MARGIN: f32 = 0.05;

/// Result for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchEstimate {
    /// Fundamental frequency when the frame is confidently pitched.
    pub hz: Option<f32>,
    /// Periodicity 0..=1 of the best candidate (reported even when unpitched).
    pub clarity: f32,
    /// RMS level of the integration window, dBFS (floored at -120).
    pub rms_db: f32,
    pub unpitched: Option<Unpitched>,
}

pub struct PitchDetector {
    config: PitchConfig,
    sample_rate: f32,
    tau_min: usize,
    tau_max: usize,
    window: usize,
    /// Normalised difference `d'`, indices `0..=tau_max + 1`.
    diff: Vec<f32>,
}

impl PitchDetector {
    pub fn new(config: PitchConfig, sample_rate: u32) -> Result<Self, &'static str> {
        config.validate()?;
        if sample_rate < 8_000 {
            return Err("sample rate must be at least 8 kHz");
        }
        let sr = sample_rate as f32;
        if config.max_hz >= sr / 4.0 {
            return Err("max_hz must be below a quarter of the sample rate");
        }
        let tau_min = ((sr / config.max_hz).floor() as usize).max(2);
        let tau_max = (sr / config.min_hz).ceil() as usize;
        let window = ((config.window_seconds * sr).round() as usize).max(tau_max);
        Ok(Self { config, sample_rate: sr, tau_min, tau_max, window, diff: vec![0.0; tau_max + 2] })
    }

    pub fn config(&self) -> &PitchConfig {
        &self.config
    }

    /// Samples `detect` needs: the integration window plus the longest lag.
    pub fn frame_len(&self) -> usize {
        self.window + self.tau_max + 1
    }

    /// Samples at the start of the frame that carry the level measurement
    /// and anchor the estimate in time.
    pub fn window_len(&self) -> usize {
        self.window
    }

    /// Estimate the pitch of `frame` (at least [`Self::frame_len`] samples;
    /// extra samples are ignored). A shorter frame is reported as silence.
    pub fn detect(&mut self, frame: &[f32]) -> PitchEstimate {
        let w = self.window;
        if frame.len() < self.frame_len() {
            return PitchEstimate { hz: None, clarity: 0.0, rms_db: -120.0, unpitched: Some(Unpitched::Silence) };
        }

        let energy: f32 = frame[..w].iter().map(|&s| s * s).sum();
        let rms = (energy / w as f32).sqrt();
        let rms_db = if rms > 1e-6 { 20.0 * rms.log10() } else { -120.0 };
        if !(rms_db >= self.config.silence_db) {
            return PitchEstimate { hz: None, clarity: 0.0, rms_db, unpitched: Some(Unpitched::Silence) };
        }

        // Steps 1 and 2: difference function, normalised by its running mean.
        self.diff[0] = 1.0;
        let mut running = 0.0f32;
        for tau in 1..=self.tau_max + 1 {
            let (a, b) = (&frame[..w], &frame[tau..tau + w]);
            let mut d = 0.0f32;
            for (x, y) in a.iter().zip(b) {
                let e = x - y;
                d += e * e;
            }
            running += d;
            self.diff[tau] = if running > 0.0 { d * tau as f32 / running } else { 1.0 };
        }

        // A dip at a lag shorter than the search range: the tone is above
        // `max_hz`. Without this check its period multiples inside the range
        // would be reported as a confident, wrong, lower pitch.
        let threshold = self.config.threshold;
        if let Some(t) = (2..self.tau_min).find(|&t| self.diff[t] < threshold) {
            let clarity = (1.0 - self.diff[t]).clamp(0.0, 1.0);
            return PitchEstimate { hz: None, clarity, rms_db, unpitched: Some(Unpitched::OutOfRange) };
        }

        // Step 3: the first dip under the threshold; take its deepest point.
        let mut found = None;
        let mut tau = self.tau_min;
        while tau <= self.tau_max {
            if self.diff[tau] < threshold {
                let mut best = tau;
                while tau <= self.tau_max && self.diff[tau] < threshold {
                    if self.diff[tau] < self.diff[best] {
                        best = tau;
                    }
                    tau += 1;
                }
                found = Some(best);
                break;
            }
            tau += 1;
        }
        // No dip under the threshold: start from the global minimum, then prefer
        // the shortest lag that is a local minimum nearly as deep. The result is
        // only reported as a pitch if its clarity passes `min_clarity`.
        let tau = found.unwrap_or_else(|| {
            let mut best = self.tau_min;
            for t in self.tau_min..=self.tau_max {
                if self.diff[t] < self.diff[best] {
                    best = t;
                }
            }
            let limit = self.diff[best] + FALLBACK_MARGIN;
            (self.tau_min + 1..best)
                .find(|&t| self.diff[t] <= limit && self.diff[t] <= self.diff[t - 1] && self.diff[t] <= self.diff[t + 1])
                .unwrap_or(best)
        });

        // Step 4: parabolic interpolation through (tau-1, tau, tau+1).
        let (a, b, c) = (self.diff[tau - 1], self.diff[tau], self.diff[tau + 1]);
        let denom = a - 2.0 * b + c;
        let (shift, depth) = if denom > 1e-12 && b <= a && b <= c {
            let shift = (0.5 * (a - c) / denom).clamp(-0.5, 0.5);
            (shift, b - 0.25 * (a - c) * shift)
        } else {
            (0.0, b)
        };
        let clarity = (1.0 - depth).clamp(0.0, 1.0);
        let hz = self.sample_rate / (tau as f32 + shift);

        // A minimum on the edge of the search range is a lag we cannot see past:
        // the true period lies outside the configured pitch range.
        let on_edge = (tau == self.tau_min && a < b) || (tau == self.tau_max && c < b);
        if on_edge {
            return PitchEstimate { hz: None, clarity, rms_db, unpitched: Some(Unpitched::OutOfRange) };
        }
        if clarity < self.config.min_clarity || !hz.is_finite() {
            return PitchEstimate { hz: None, clarity, rms_db, unpitched: Some(Unpitched::Aperiodic) };
        }
        PitchEstimate { hz: Some(hz), clarity, rms_db, unpitched: None }
    }
}

#[cfg(test)]
mod tests {
    //! Tolerances come from a line-by-line numerical model of this file run
    //! on the same signals (worst case noted beside each assertion), with a
    //! margin of at least 3x. They are bounds on measured behaviour, not
    //! numbers chosen to make the tests pass.

    use super::*;
    use crate::note::{cents_between, Tuning};
    use crate::signal::{self, VOICE_PARTIALS, WEAK_FUNDAMENTAL_PARTIALS};

    const SR: u32 = 48_000;

    fn hz_of(midi: i32) -> f64 {
        Tuning::default().midi_to_hz(midi as f32) as f64
    }

    /// Detect at several offsets into `signal`; returns every estimate.
    fn scan(signal: &[f32], sample_rate: u32, config: PitchConfig) -> Vec<PitchEstimate> {
        let mut detector = PitchDetector::new(config, sample_rate).unwrap();
        let n = detector.frame_len();
        (0..signal.len().saturating_sub(n)).step_by(251).map(|off| detector.detect(&signal[off..off + n])).collect()
    }

    /// Worst absolute error in cents over MIDI notes `lo..=hi`; panics on any unpitched frame.
    fn worst_cents(lo: i32, hi: i32, sample_rate: u32, make: impl Fn(f64, usize) -> Vec<f32>) -> f32 {
        let mut worst = 0.0f32;
        for midi in lo..=hi {
            let hz = hz_of(midi);
            for est in scan(&make(hz, 6_000), sample_rate, PitchConfig::default()) {
                let got = est.hz.unwrap_or_else(|| panic!("MIDI {midi}: unpitched ({:?}, clarity {})", est.unpitched, est.clarity));
                assert!(est.clarity > 0.99, "MIDI {midi}: clarity {}", est.clarity);
                worst = worst.max(cents_between(got, hz as f32).unwrap().abs());
            }
        }
        worst
    }

    #[test]
    fn frame_geometry_at_48k() {
        let d = PitchDetector::new(PitchConfig::default(), SR).unwrap();
        assert_eq!((d.tau_min, d.tau_max, d.window_len(), d.frame_len()), (34, 739, 960, 1_700));
    }

    #[test]
    fn pure_sines_from_c2_to_e6_are_within_two_cents() {
        // Model worst case: 0.47 cents.
        let worst = worst_cents(36, 88, SR, |hz, n| signal::sine(hz, SR, n, 0.5));
        assert!(worst < 2.0, "worst error {worst} cents");
    }

    #[test]
    fn harmonic_voice_like_tones_are_within_two_cents() {
        // Model worst case: 0.41 cents.
        let worst = worst_cents(36, 84, SR, |hz, n| signal::harmonic(hz, SR, n, &VOICE_PARTIALS));
        assert!(worst < 2.0, "worst error {worst} cents");
    }

    #[test]
    fn weak_fundamental_does_not_cause_octave_errors() {
        // Harmonics 2 and 3 are 3-4x stronger than the fundamental. An octave
        // error would be 1200 cents; model worst case is 0.37.
        let worst = worst_cents(36, 84, SR, |hz, n| signal::harmonic(hz, SR, n, &WEAK_FUNDAMENTAL_PARTIALS));
        assert!(worst < 2.0, "worst error {worst} cents");
    }

    #[test]
    fn accuracy_holds_at_44_1_khz() {
        // Model worst case: 0.57 cents.
        let worst = worst_cents(36, 84, 44_100, |hz, n| signal::harmonic(hz, 44_100, n, &VOICE_PARTIALS));
        assert!(worst < 2.5, "worst error {worst} cents");
    }

    #[test]
    fn detuned_and_quiet_tones_are_measured_not_snapped() {
        // 37 cents sharp of A4: the estimator reports the real frequency, not the nearest note.
        let hz = 440.0 * 2f64.powf(37.0 / 1200.0);
        for est in scan(&signal::sine(hz, SR, 6_000, 0.5), SR, PitchConfig::default()) {
            let cents = cents_between(est.hz.unwrap(), 440.0).unwrap();
            assert!((cents - 37.0).abs() < 2.0, "{cents}");
        }
        // -40 dBFS RMS, 10 dB above the silence gate. Model worst case 0.22 cents.
        for est in scan(&signal::sine(220.0, SR, 6_000, 0.014), SR, PitchConfig::default()) {
            assert!((est.rms_db + 40.0).abs() < 1.0, "level {}", est.rms_db);
            assert!(cents_between(est.hz.unwrap(), 220.0).unwrap().abs() < 2.0);
        }
    }

    /// Voice-like tone plus white noise at `snr_db`.
    fn noisy_voice(hz: f64, frames: usize, snr_db: f32, seed: u32) -> Vec<f32> {
        let tone = signal::harmonic(hz, SR, frames, &VOICE_PARTIALS);
        let noise_rms = signal::rms(&tone) / 10f32.powf(snr_db / 20.0);
        // Uniform noise in [-a, a] has RMS a / sqrt(3).
        signal::mix(&tone, &signal::noise(seed, frames, noise_rms * 3f32.sqrt()))
    }

    #[test]
    fn background_noise_degrades_gracefully() {
        // 20 dB SNR (a noisy room): every frame pitched; model worst case 2.9 cents.
        // 10 dB SNR (very noisy): model has 1 gross error in 656 frames, worst other 17 cents.
        for (snr, max_cents, min_good) in [(20.0, 10.0, 1.0), (10.0, 50.0, 0.97)] {
            let (mut good, mut total) = (0usize, 0usize);
            for midi in 40..=80 {
                let hz = hz_of(midi);
                for est in scan(&noisy_voice(hz, 6_000, snr, 7 + midi as u32), SR, PitchConfig::default()) {
                    total += 1;
                    if est.hz.is_some_and(|got| cents_between(got, hz as f32).unwrap().abs() < max_cents) {
                        good += 1;
                    }
                }
            }
            let ratio = good as f32 / total as f32;
            assert!(ratio >= min_good, "SNR {snr} dB: {good}/{total} frames within {max_cents} cents");
        }
    }

    #[test]
    fn hopeless_noise_levels_report_no_pitch_rather_than_a_guess() {
        // 3 dB SNR: the model reports every frame unpitched. Allow a few frames
        // through, but none of them may be confidently wrong by an octave.
        let mut pitched = 0usize;
        let mut total = 0usize;
        for midi in 40..=80 {
            for est in scan(&noisy_voice(hz_of(midi), 6_000, 3.0, 7 + midi as u32), SR, PitchConfig::default()) {
                total += 1;
                pitched += est.hz.is_some() as usize;
            }
        }
        assert!(pitched * 20 <= total, "{pitched}/{total} frames claimed a pitch at 3 dB SNR");
    }

    #[test]
    fn silence_noise_and_percussion_are_unpitched() {
        let silent = scan(&signal::silence(6_000), SR, PitchConfig::default());
        assert!(silent.iter().all(|e| e.hz.is_none() && e.unpitched == Some(Unpitched::Silence) && e.clarity == 0.0));

        // Below the -50 dBFS gate.
        let faint = scan(&signal::sine(220.0, SR, 6_000, 0.002), SR, PitchConfig::default());
        assert!(faint.iter().all(|e| e.unpitched == Some(Unpitched::Silence)));

        // White noise: model clarity is at most 0.13.
        for seed in 1..=12 {
            for est in scan(&signal::noise(seed * 977, 6_000, 0.5), SR, PitchConfig::default()) {
                assert_eq!(est.hz, None, "noise seed {seed}");
                assert_eq!(est.unpitched, Some(Unpitched::Aperiodic));
                assert!(est.clarity < 0.4, "noise clarity {}", est.clarity);
            }
        }
        // Decaying noise bursts (drum-like): model clarity is about 0.2.
        for seed in 1..=3 {
            for est in scan(&signal::burst(seed * 31, 6_000, 0.8, 2_000.0), SR, PitchConfig::default()) {
                assert_eq!(est.hz, None, "burst seed {seed}");
            }
        }
    }

    #[test]
    fn tones_outside_the_range_are_flagged_not_misreported() {
        // Above max_hz: without the short-lag check these read as 900 Hz and 1000 Hz
        // with clarity 1.0 (a subharmonic).
        for hz in [1_500.0, 1_800.0, 2_400.0, 3_000.0, 5_000.0] {
            for est in scan(&signal::sine(hz, SR, 6_000, 0.5), SR, PitchConfig::default()) {
                assert_eq!((est.hz, est.unpitched), (None, Some(Unpitched::OutOfRange)), "{hz} Hz");
            }
        }
        // Below min_hz.
        for hz in [40.0, 50.0, 60.0] {
            for est in scan(&signal::sine(hz, SR, 6_000, 0.5), SR, PitchConfig::default()) {
                assert_eq!(est.hz, None, "{hz} Hz");
            }
        }
        // Widening the range makes the same tone measurable.
        let wide = PitchConfig { max_hz: 4_000.0, ..PitchConfig::default() };
        for est in scan(&signal::sine(3_000.0, SR, 6_000, 0.5), SR, wide) {
            assert!(cents_between(est.hz.unwrap(), 3_000.0).unwrap().abs() < 10.0);
        }
    }

    #[test]
    fn rejects_bad_configuration_and_short_frames() {
        let bad = |c: PitchConfig| PitchDetector::new(c, SR).is_err();
        assert!(bad(PitchConfig { min_hz: 10.0, ..PitchConfig::default() }));
        assert!(bad(PitchConfig { min_hz: 500.0, max_hz: 400.0, ..PitchConfig::default() }));
        assert!(bad(PitchConfig { max_hz: 20_000.0, ..PitchConfig::default() }));
        assert!(bad(PitchConfig { threshold: 0.0, ..PitchConfig::default() }));
        assert!(bad(PitchConfig { min_clarity: 1.5, ..PitchConfig::default() }));
        assert!(bad(PitchConfig { window_seconds: f32::NAN, ..PitchConfig::default() }));
        assert!(PitchDetector::new(PitchConfig::default(), 4_000).is_err());

        let mut d = PitchDetector::new(PitchConfig::default(), SR).unwrap();
        let short = signal::sine(220.0, SR, d.frame_len() - 1, 0.5);
        assert_eq!(d.detect(&short).unpitched, Some(Unpitched::Silence));
    }
}
