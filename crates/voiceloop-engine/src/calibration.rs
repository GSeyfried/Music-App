//! Loopback latency calibration: the test signal and its detector.
//!
//! Procedure (wiring is specified in `docs/projects/hardware-io/README.md`):
//! the output callback plays [`TestSignal::samples`] starting at transport
//! frame `t0` while recording the bridged input it hands the looper into a
//! preallocated buffer starting at the same frame. Both are on the output
//! clock, so the lag between them is exactly the round trip the looper sees
//! (DAC + cable + ADC + bridge cushion): the value for `setInputLatency`.
//!
//! The signal is a train of Hann-windowed linear chirps. A chirp has a sharp
//! autocorrelation peak and is robust to the band limits of speakers and
//! microphones; repeating it lets us reject one-off noise and report a spread.
//! Pure code (no audio I/O), unit-tested on synthetic loopbacks.

use serde::Serialize;

/// Chirp sweep and timing.
pub const CHIRP_SECONDS: f32 = 0.02;
pub const CHIRP_F0_HZ: f32 = 1_000.0;
pub const CHIRP_F1_HZ: f32 = 8_000.0;
pub const CHIRP_PERIOD_SECONDS: f32 = 0.25;
pub const CHIRP_REPEATS: usize = 8;
/// Peak amplitude of the played signal (-12 dBFS: audible, not alarming).
pub const CHIRP_AMPLITUDE: f32 = 0.25;
/// Longest round trip searched.
pub const MAX_LATENCY_SECONDS: f32 = 0.2;
/// A repetition counts when its normalised correlation reaches this.
pub const MIN_CORRELATION: f32 = 0.3;

/// The signal to play.
#[derive(Debug, Clone)]
pub struct TestSignal {
    pub sample_rate: u32,
    pub chirp: Vec<f32>,
    /// Chirp start offsets within `samples`.
    pub onsets: Vec<usize>,
    /// Everything to play, silence between chirps; record at least
    /// `samples.len() + max_lag()` frames.
    pub samples: Vec<f32>,
}

impl TestSignal {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.max(8_000) as f32;
        let len = (CHIRP_SECONDS * sr) as usize;
        let chirp: Vec<f32> = (0..len)
            .map(|i| {
                let t = i as f32 / sr;
                let k = (CHIRP_F1_HZ.min(sr * 0.45) - CHIRP_F0_HZ) / CHIRP_SECONDS;
                let phase = 2.0 * std::f32::consts::PI * (CHIRP_F0_HZ * t + 0.5 * k * t * t);
                let window = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (len - 1) as f32).cos();
                CHIRP_AMPLITUDE * window * phase.sin()
            })
            .collect();
        let period = (CHIRP_PERIOD_SECONDS * sr) as usize;
        let onsets: Vec<usize> = (0..CHIRP_REPEATS).map(|k| k * period).collect();
        let mut samples = vec![0.0f32; period * CHIRP_REPEATS];
        for &o in &onsets {
            samples[o..o + len].copy_from_slice(&chirp);
        }
        Self { sample_rate, chirp, onsets, samples }
    }

    pub fn max_lag(&self) -> usize {
        (MAX_LATENCY_SECONDS * self.sample_rate as f32) as usize
    }

    /// Frames to record after playback starts.
    pub fn record_frames(&self) -> usize {
        self.samples.len() + self.max_lag() + self.chirp.len()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationResult {
    /// Round-trip latency in frames: pass to `setInputLatency`.
    pub latency_frames: u32,
    pub latency_ms: f32,
    pub sample_rate: u32,
    /// 0..1: share of chirps detected × mean correlation, halved if they disagree by > 2 frames.
    pub confidence: f32,
    pub detected: u32,
    pub repeats: u32,
    /// Largest minus smallest per-chirp estimate, in frames.
    pub spread_frames: u32,
    /// The detected signal was inverted (a polarity flip somewhere in the chain).
    pub inverted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum CalibrationFailure {
    /// No chirp was found: no loopback, muted output, or wrong input route.
    NotDetected(String),
    /// Chirps were found but disagree, e.g. acoustic echoes or dropouts.
    Inconsistent(String),
    /// The run did not complete (audio stopped, or the output never played it).
    Aborted(String),
}

/// Find the lag of each chirp in `recorded` (which starts at the same output
/// frame as the played signal) and combine them.
pub fn detect(signal: &TestSignal, recorded: &[f32]) -> Result<CalibrationResult, CalibrationFailure> {
    let chirp = &signal.chirp;
    let energy: f32 = chirp.iter().map(|x| x * x).sum();
    let mut lags = Vec::new();
    let mut inverted_votes = 0;
    let mut corr_sum = 0.0f32;
    for &onset in &signal.onsets {
        let mut best = (0usize, 0.0f32, false);
        for lag in 0..=signal.max_lag() {
            let start = onset + lag;
            let Some(window) = recorded.get(start..start + chirp.len()) else { break };
            let (mut dot, mut e) = (0.0f32, 0.0f32);
            for (a, b) in window.iter().zip(chirp) {
                dot += a * b;
                e += a * a;
            }
            if e <= 1e-12 {
                continue;
            }
            let ncc = dot / (energy * e).sqrt();
            if ncc.abs() > best.1 {
                best = (lag, ncc.abs(), ncc < 0.0);
            }
        }
        if best.1 >= MIN_CORRELATION {
            lags.push(best.0);
            corr_sum += best.1;
            inverted_votes += best.2 as usize;
        }
    }
    let repeats = signal.onsets.len() as u32;
    if lags.len() < signal.onsets.len().div_ceil(2) {
        return Err(CalibrationFailure::NotDetected(format!(
            "found {} of {repeats} test chirps; connect the output to the input (cable or speaker near the mic) and check the input route",
            lags.len()
        )));
    }
    let mut sorted = lags.clone();
    sorted.sort_unstable();
    let median = sorted[sorted.len() / 2];
    // Keep estimates within 2 ms of the median (rejects echoes and stray peaks).
    let tolerance = (signal.sample_rate / 500) as usize;
    let agreeing: Vec<usize> = sorted.iter().copied().filter(|&l| l.abs_diff(median) <= tolerance).collect();
    if agreeing.len() < signal.onsets.len().div_ceil(2) {
        return Err(CalibrationFailure::Inconsistent(format!("chirp delays disagree: {sorted:?} frames")));
    }
    let spread = (agreeing[agreeing.len() - 1] - agreeing[0]) as u32;
    let mean_corr = corr_sum / lags.len() as f32;
    let mut confidence = agreeing.len() as f32 / repeats as f32 * mean_corr;
    if spread > 2 {
        confidence *= 0.5;
    }
    Ok(CalibrationResult {
        latency_frames: median as u32,
        latency_ms: median as f32 * 1_000.0 / signal.sample_rate as f32,
        sample_rate: signal.sample_rate,
        confidence: confidence.clamp(0.0, 1.0),
        detected: agreeing.len() as u32,
        repeats,
        spread_frames: spread,
        inverted: inverted_votes * 2 > lags.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic noise in -1..1.
    fn noise(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }

    /// The played signal delayed by `delay`, scaled, with an echo and noise.
    fn loopback(
        signal: &TestSignal,
        delay: usize,
        gain: f32,
        noise_level: f32,
        echo: Option<(usize, f32)>,
    ) -> Vec<f32> {
        let mut out = vec![0.0f32; signal.record_frames()];
        let mut seed = 7;
        for (i, &s) in signal.samples.iter().enumerate() {
            out[i + delay] += gain * s;
            if let Some((d, g)) = echo {
                if let Some(v) = out.get_mut(i + delay + d) {
                    *v += g * s;
                }
            }
        }
        for v in &mut out {
            *v += noise_level * noise(&mut seed);
        }
        out
    }

    #[test]
    fn finds_the_exact_delay_through_a_quiet_noisy_echoey_loop() {
        for &(sr, delay) in &[(48_000u32, 312usize), (44_100, 1_287), (48_000, 0)] {
            let signal = TestSignal::new(sr);
            let recorded = loopback(&signal, delay, 0.1, 0.01, Some((240, 0.05)));
            let r = detect(&signal, &recorded).unwrap();
            assert_eq!(r.latency_frames as usize, delay, "{sr} Hz");
            assert_eq!((r.detected, r.spread_frames, r.inverted), (CHIRP_REPEATS as u32, 0, false));
            assert!(r.confidence > 0.6, "{r:?}");
            assert!((r.latency_ms - delay as f32 * 1_000.0 / sr as f32).abs() < 1e-3);
        }
    }

    #[test]
    fn reports_polarity_inversion() {
        let signal = TestSignal::new(48_000);
        let r = detect(&signal, &loopback(&signal, 500, -0.5, 0.0, None)).unwrap();
        assert_eq!(r.latency_frames, 500);
        assert!(r.inverted);
    }

    #[test]
    fn silence_or_noise_alone_is_not_detected() {
        let signal = TestSignal::new(48_000);
        let silent = vec![0.0f32; signal.record_frames()];
        assert!(matches!(detect(&signal, &silent), Err(CalibrationFailure::NotDetected(_))));
        let noisy = loopback(&signal, 300, 0.0, 0.2, None);
        assert!(matches!(detect(&signal, &noisy), Err(CalibrationFailure::NotDetected(_))));
    }

    #[test]
    fn a_dropout_in_one_chirp_does_not_move_the_result() {
        let signal = TestSignal::new(48_000);
        let mut recorded = loopback(&signal, 700, 0.3, 0.001, None);
        // Wipe the third chirp's arrival (an xrun during calibration).
        let o = signal.onsets[2] + 700;
        recorded[o..o + signal.chirp.len()].fill(0.0);
        let r = detect(&signal, &recorded).unwrap();
        assert_eq!((r.latency_frames, r.detected), (700, CHIRP_REPEATS as u32 - 1));
        assert!(r.confidence < 1.0);
    }
}
