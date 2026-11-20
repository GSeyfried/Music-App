//! Timbre of percussive attacks, for voice drums (beatboxing into a mic).
//!
//! For every onset found by [`crate::onset::OnsetDetector`], the first
//! `window_seconds` of sound after the attack are described by a handful of
//! numbers: how the energy splits between low, mid and high bands, the
//! spectral centroid ("brightness"), the zero-crossing rate ("noisiness"),
//! and how much quieter the second half is than the first ("decay").
//!
//! These are enough to tell the three starter voice-drum sounds apart:
//!
//! | Mouth sound          | Typical shape                                  |
//! | -------------------- | ---------------------------------------------- |
//! | "B"/"boom" (kick)    | most energy below ~250 Hz, low centroid        |
//! | "K"/"psh" (snare)    | broadband noise, energy mostly in the mid band |
//! | "ts"/"t" (hi-hat)    | energy above ~3 kHz, high centroid, short      |
//!
//! Classification itself is done by the caller (the frontend keeps trained
//! per-user profiles); this module only measures. The result is a small
//! summary, never audio.
//!
//! Not realtime: it runs on the analysis thread. Buffers are allocated once
//! at construction; describing a hit performs one FFT of a fixed size.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::onset::Onset;
use crate::spectrum::Fft;

/// Onsets waiting for their window to fill (onsets are ≥ 60 ms apart by
/// default and the window is 40 ms, so more than two is unusual).
const MAX_PENDING: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HitConfig {
    /// When false no hit descriptions are produced.
    pub enabled: bool,
    /// Length of sound after the onset that is described.
    pub window_seconds: f32,
    /// Upper edge of the low band (Hz).
    pub low_hz: f32,
    /// Lower edge of the high band (Hz).
    pub high_hz: f32,
}

impl Default for HitConfig {
    fn default() -> Self {
        Self { enabled: true, window_seconds: 0.04, low_hz: 250.0, high_hz: 3_000.0 }
    }
}

impl HitConfig {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(self.window_seconds.is_finite() && (0.005..=0.25).contains(&self.window_seconds)) {
            return Err("hits.windowSeconds must be in 0.005..=0.25");
        }
        if !(self.low_hz.is_finite() && self.high_hz.is_finite() && self.low_hz > 0.0 && self.high_hz > self.low_hz) {
            return Err("hits.lowHz must be > 0 and below hits.highHz");
        }
        Ok(())
    }
}

/// What one attack sounded like.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HitTimbre {
    /// Input-clock frame of the onset (same as [`Onset::frame`]).
    pub frame: u64,
    pub sample_rate: u32,
    /// Onset level (dBFS RMS) and rise (dB), copied from the onset.
    pub level_db: f32,
    pub rise_db: f32,
    /// Fractions of the window's spectral energy below `low_hz`, between the
    /// edges, and above `high_hz`. They sum to 1 (or are all 0 for silence).
    pub low: f32,
    pub mid: f32,
    pub high: f32,
    /// Energy-weighted mean frequency, Hz.
    pub centroid_hz: f32,
    /// Sign changes per sample, 0..=1 (noise is high, a low thump is low).
    pub zero_crossing_rate: f32,
    /// Energy of the window's second half over its first, in dB (≤ 0 for a
    /// sound that dies away; short "t"/"ts" sounds are strongly negative).
    pub decay_db: f32,
}

/// Collects the window after each onset and describes it.
pub struct HitDescriber {
    config: HitConfig,
    sample_rate: u32,
    window: usize,
    fft: Fft,
    fft_size: usize,
    hann: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    /// Recent input, circular; holds at least the window plus one input chunk.
    history: Vec<f32>,
    /// Input-clock frame of the next sample written to `history`.
    position: u64,
    pending: VecDeque<Onset>,
}

impl HitDescriber {
    /// `max_chunk`: the largest slice ever passed to [`HitDescriber::push`].
    pub fn new(config: HitConfig, sample_rate: u32, max_chunk: usize) -> Result<Self, &'static str> {
        config.validate()?;
        if sample_rate == 0 {
            return Err("sample rate must be positive");
        }
        let window = ((config.window_seconds * sample_rate as f32).round() as usize).max(16);
        let fft_size = window.next_power_of_two();
        let denom = window.max(2) as f64 - 1.0;
        let hann = (0..window).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / denom).cos()) as f32).collect();
        Ok(Self {
            config,
            sample_rate,
            window,
            fft: Fft::new(fft_size),
            fft_size,
            hann,
            re: vec![0.0; fft_size],
            im: vec![0.0; fft_size],
            history: vec![0.0; window + max_chunk.max(1) + 1],
            position: 0,
            pending: VecDeque::with_capacity(MAX_PENDING),
        })
    }

    /// Samples after the onset that are described.
    pub fn window_frames(&self) -> usize {
        self.window
    }

    /// Forget everything and restart the clock at `frame`.
    pub fn reset(&mut self, frame: u64) {
        self.pending.clear();
        self.position = frame;
    }

    /// Note an onset found in samples already (or about to be) pushed.
    /// Onsets older than the history are ignored; beyond `MAX_PENDING` the
    /// oldest waiting onset is dropped.
    pub fn onset(&mut self, onset: Onset) {
        if !self.config.enabled {
            return;
        }
        let oldest = self.position.saturating_sub(self.history.len() as u64);
        if onset.frame < oldest {
            return;
        }
        if self.pending.len() == MAX_PENDING {
            self.pending.pop_front();
        }
        self.pending.push_back(onset);
    }

    /// Feed the next input samples (contiguous with the previous call), then
    /// describe every onset whose window is now complete.
    pub fn push(&mut self, samples: &[f32], mut emit: impl FnMut(HitTimbre)) {
        let len = self.history.len();
        // Only the most recent `len` samples can matter.
        let skip = samples.len().saturating_sub(len);
        self.position += skip as u64;
        for &s in &samples[skip..] {
            self.history[(self.position % len as u64) as usize] = s;
            self.position += 1;
        }
        self.flush_ready(&mut emit);
    }

    fn flush_ready(&mut self, emit: &mut impl FnMut(HitTimbre)) {
        while let Some(&onset) = self.pending.front() {
            if onset.frame + self.window as u64 > self.position {
                break;
            }
            self.pending.pop_front();
            let oldest = self.position.saturating_sub(self.history.len() as u64);
            if onset.frame >= oldest {
                emit(self.describe(onset));
            }
        }
    }

    fn describe(&mut self, onset: Onset) -> HitTimbre {
        let len = self.history.len() as u64;
        let n = self.window;
        let mut crossings = 0usize;
        let mut prev = 0.0f32;
        let (mut first, mut second) = (0.0f64, 0.0f64);
        for i in 0..n {
            let s = self.history[((onset.frame + i as u64) % len) as usize];
            if i > 0 && (s >= 0.0) != (prev >= 0.0) {
                crossings += 1;
            }
            prev = s;
            let e = f64::from(s) * f64::from(s);
            if i < n / 2 {
                first += e;
            } else {
                second += e;
            }
            self.re[i] = s * self.hann[i];
            self.im[i] = 0.0;
        }
        for i in n..self.fft_size {
            self.re[i] = 0.0;
            self.im[i] = 0.0;
        }
        self.fft.forward(&mut self.re, &mut self.im);

        let bin_hz = self.sample_rate as f32 / self.fft_size as f32;
        let (mut low, mut mid, mut high, mut weighted) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
        // Skip DC (bin 0): a microphone offset is not a kick drum.
        for k in 1..=self.fft_size / 2 {
            let p = f64::from(self.re[k] * self.re[k] + self.im[k] * self.im[k]);
            let hz = k as f32 * bin_hz;
            if hz < self.config.low_hz {
                low += p;
            } else if hz < self.config.high_hz {
                mid += p;
            } else {
                high += p;
            }
            weighted += p * f64::from(hz);
        }
        let total = low + mid + high;
        let (low, mid, high, centroid) = if total > 1e-18 {
            ((low / total) as f32, (mid / total) as f32, (high / total) as f32, (weighted / total) as f32)
        } else {
            (0.0, 0.0, 0.0, 0.0)
        };
        let decay_db = if first > 1e-18 { (10.0 * ((second + 1e-18) / first).log10()) as f32 } else { 0.0 };
        HitTimbre {
            frame: onset.frame,
            sample_rate: self.sample_rate,
            level_db: onset.level_db,
            rise_db: onset.rise_db,
            low,
            mid,
            high,
            centroid_hz: centroid,
            zero_crossing_rate: crossings as f32 / (n.max(2) - 1) as f32,
            decay_db: decay_db.clamp(-60.0, 20.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::onset::{OnsetConfig, OnsetDetector};
    use crate::signal;

    const SR: u32 = 48_000;

    /// Run onset detection and description over a whole signal, in chunks.
    fn describe_all(samples: &[f32], chunk: usize) -> Vec<HitTimbre> {
        let mut onsets = OnsetDetector::new(OnsetConfig::default(), SR).unwrap();
        let mut hits = HitDescriber::new(HitConfig::default(), SR, chunk).unwrap();
        let mut out = Vec::new();
        for part in samples.chunks(chunk) {
            let mut found = Vec::new();
            onsets.push(part, |o| found.push(o));
            for o in found {
                hits.onset(o);
            }
            hits.push(part, |h| out.push(h));
        }
        out
    }

    /// A decaying low sine "boom" with a falling pitch, like a voiced "B".
    fn kick() -> Vec<f32> {
        let n = (0.15 * SR as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SR as f32;
                let hz = 60.0 + 90.0 * (-t * 30.0).exp();
                0.8 * (std::f32::consts::TAU * hz * t).sin() * (-t * 18.0).exp()
            })
            .collect()
    }

    /// High-passed noise burst, like "ts".
    fn hat() -> Vec<f32> {
        let raw = signal::burst(7, (0.06 * SR as f32) as usize, 0.6, 600.0);
        // First difference twice: a steep crude high-pass.
        let mut d: Vec<f32> = raw.windows(2).map(|w| w[1] - w[0]).collect();
        d = d.windows(2).map(|w| w[1] - w[0]).collect();
        d
    }

    /// Noise burst through a crude band-pass (difference then smoothing), like "psh"/"k".
    fn snare() -> Vec<f32> {
        let raw = signal::burst(11, (0.12 * SR as f32) as usize, 0.7, 2_400.0);
        let diff: Vec<f32> = raw.windows(2).map(|w| w[1] - w[0]).collect();
        let mut out = Vec::with_capacity(diff.len());
        let mut y = 0.0f32;
        for x in diff {
            y += 0.35 * (x - y);
            out.push(y * 2.0);
        }
        out
    }

    fn gap() -> Vec<f32> {
        signal::silence((0.25 * SR as f32) as usize)
    }

    #[test]
    fn describes_one_hit_per_attack_with_onset_frames() {
        let s = signal::concat(&[&gap(), &kick(), &gap(), &kick(), &gap()]);
        let hits = describe_all(&s, 512);
        assert_eq!(hits.len(), 2);
        let first = (0.25 * SR as f32) as u64;
        assert!(hits[0].frame.abs_diff(first) < 480, "frame {}", hits[0].frame);
        assert!(hits.iter().all(|h| (h.low + h.mid + h.high - 1.0).abs() < 1e-3));
    }

    #[test]
    fn kick_snare_and_hat_have_distinct_shapes() {
        let s = signal::concat(&[&gap(), &kick(), &gap(), &snare(), &gap(), &hat(), &gap()]);
        let hits = describe_all(&s, 1_024);
        assert_eq!(hits.len(), 3, "{hits:?}");
        let (k, sn, h) = (hits[0], hits[1], hits[2]);
        assert!(k.low > 0.6, "kick low {}", k.low);
        assert!(k.centroid_hz < 400.0, "kick centroid {}", k.centroid_hz);
        assert!(h.high > 0.6, "hat high {}", h.high);
        assert!(h.centroid_hz > sn.centroid_hz && sn.centroid_hz > k.centroid_hz);
        assert!(sn.mid > k.mid && sn.mid > h.mid, "snare mid {} vs {} {}", sn.mid, k.mid, h.mid);
        assert!(h.zero_crossing_rate > k.zero_crossing_rate * 4.0);
    }

    #[test]
    fn chunk_size_does_not_change_the_result() {
        let s = signal::concat(&[&gap(), &snare(), &gap(), &hat(), &gap()]);
        let a = describe_all(&s, 128);
        let b = describe_all(&s, 4_096);
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.frame, y.frame);
            assert!((x.centroid_hz - y.centroid_hz).abs() < 1.0);
        }
    }

    #[test]
    fn disabled_and_invalid_configs() {
        let mut off = HitDescriber::new(HitConfig { enabled: false, ..HitConfig::default() }, SR, 256).unwrap();
        off.onset(Onset { frame: 0, level_db: -10.0, rise_db: 30.0 });
        let mut n = 0;
        off.push(&vec![0.5; 4_096], |_| n += 1);
        assert_eq!(n, 0);
        assert!(HitConfig { low_hz: 5_000.0, ..HitConfig::default() }.validate().is_err());
        assert!(HitConfig { window_seconds: 0.0, ..HitConfig::default() }.validate().is_err());
    }
}
