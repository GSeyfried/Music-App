//! Background noise reduction: noise profiles and offline spectral gating.
//!
//! Written from the general technique (short-time Fourier transform, a
//! per-bin noise threshold, a gain floor for bins below it, smoothing of the
//! gate decisions); no code from other noise reducers was used.
//!
//! ```text
//!  room audio ─▶ learn_noise ─▶ NoiseSpectrum (mean magnitude per bin)
//!                                    │
//!  layer audio ─▶ STFT (Hann, hop N/4, circular) ─▶ smoothed magnitude > profile × sensitivity ?
//!                                    │                      yes: gain 1   no: gain −amountDb
//!                                    └─▶ dilate + average the gains (time × frequency) ─▶ ISTFT
//! ```
//!
//! - **Circular processing.** Loop layers wrap around: frames that run past
//!   the end continue at the start, so the loop seam gets no edge artefacts
//!   and the output has exactly the input's length.
//! - **Unity gains reconstruct the input** (to float rounding): analysis and
//!   synthesis use a Hann window and every sample is normalised by the sum of
//!   squared windows that covered it.
//! - Deterministic, allocation per call (worker-thread code, never realtime).
//! - Reduces steady noise (hiss, hum, fans), not sudden sounds.

use crate::spectrum::Fft;

/// Smallest and largest STFT sizes accepted.
pub const MIN_FFT_SIZE: usize = 256;
pub const MAX_FFT_SIZE: usize = 16_384;
/// Default STFT size (`noise.fftSize`).
pub const DEFAULT_FFT_SIZE: usize = 2_048;

/// Average magnitude spectrum of a noise recording.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseSpectrum {
    /// STFT size the profile was measured with (processing uses the same size).
    pub fft_size: usize,
    pub sample_rate: u32,
    /// `fft_size / 2 + 1` mean magnitudes, scaled so a sine of amplitude `A` reads `A`.
    pub magnitudes: Vec<f32>,
    /// STFT frames that went into the average.
    pub frames: usize,
}

impl NoiseSpectrum {
    /// The profile's magnitude at `hz` (linear interpolation between bins).
    pub fn magnitude_at(&self, hz: f32) -> f32 {
        if self.magnitudes.is_empty() || self.sample_rate == 0 {
            return 0.0;
        }
        let pos = (hz.max(0.0) * self.fft_size as f32 / self.sample_rate as f32).min((self.magnitudes.len() - 1) as f32);
        let i = pos.floor() as usize;
        let j = (i + 1).min(self.magnitudes.len() - 1);
        let f = pos - i as f32;
        self.magnitudes[i] * (1.0 - f) + self.magnitudes[j] * f
    }

    /// Per-bin magnitudes for processing audio at `sample_rate` with this
    /// profile's STFT size (re-mapped by frequency when the rates differ).
    pub fn bins_for(&self, sample_rate: u32) -> Vec<f32> {
        if sample_rate == self.sample_rate {
            return self.magnitudes.clone();
        }
        let bins = self.fft_size / 2 + 1;
        (0..bins).map(|k| self.magnitude_at(k as f32 * sample_rate as f32 / self.fft_size as f32)).collect()
    }
}

/// How a profile is measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LearnOptions {
    pub fft_size: usize,
    /// Skip frames louder than the median frame by more than this (dB), e.g.
    /// metronome clicks bleeding into the mic. `None` keeps every frame.
    pub transient_reject_db: Option<f32>,
}

impl Default for LearnOptions {
    fn default() -> Self {
        Self { fft_size: DEFAULT_FFT_SIZE, transient_reject_db: Some(6.0) }
    }
}

/// Settings of one spectral-gating pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateSettings {
    /// How far bins judged to be noise are turned down (dB, ≥ 0; 0 = no change).
    pub amount_db: f32,
    /// How far above the noise profile a bin must be to count as signal
    /// (dB). Higher treats more as noise.
    pub sensitivity_db: f32,
    /// 0..1: how widely gate decisions are smoothed over time and frequency
    /// (fewer "musical noise" artefacts, softer edges).
    pub smoothing: f32,
}

impl Default for GateSettings {
    fn default() -> Self {
        Self { amount_db: 12.0, sensitivity_db: 6.0, smoothing: 0.5 }
    }
}

/// A valid STFT size: a power of two in range.
pub fn valid_fft_size(n: usize) -> bool {
    n.is_power_of_two() && (MIN_FFT_SIZE..=MAX_FFT_SIZE).contains(&n)
}

fn hann(n: usize) -> Vec<f32> {
    // Periodic Hann: overlaps at hop n/4 sum to a constant.
    (0..n).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / n as f64).cos()) as f32).collect()
}

/// Measure the average magnitude spectrum of `samples` (room sound, no
/// singing). `None` when the input is shorter than one STFT frame, the size
/// is invalid, or no frame survives the transient rejection.
pub fn learn_noise(samples: &[f32], sample_rate: u32, options: &LearnOptions) -> Option<NoiseSpectrum> {
    let n = options.fft_size;
    if !valid_fft_size(n) || samples.len() < n || sample_rate == 0 {
        return None;
    }
    let hop = n / 4;
    let window = hann(n);
    let scale = 2.0 / window.iter().sum::<f32>().max(f32::EPSILON);
    let fft = Fft::new(n);
    let starts: Vec<usize> = (0..=samples.len() - n).step_by(hop).collect();
    let energy: Vec<f64> = starts
        .iter()
        .map(|&s| samples[s..s + n].iter().zip(&window).map(|(x, w)| ((x * w) as f64).powi(2)).sum())
        .collect();
    let limit = options.transient_reject_db.filter(|db| db.is_finite()).map(|db| {
        let mut sorted = energy.clone();
        sorted.sort_by(f64::total_cmp);
        sorted[sorted.len() / 2] * 10f64.powf(db as f64 / 10.0)
    });
    let bins = n / 2 + 1;
    let mut sum = vec![0.0f64; bins];
    let mut used = 0usize;
    let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
    for (&s, &e) in starts.iter().zip(&energy) {
        if limit.is_some_and(|l| e > l && l > 0.0) {
            continue;
        }
        for i in 0..n {
            re[i] = samples[s + i] * window[i];
        }
        im.fill(0.0);
        fft.forward(&mut re, &mut im);
        for (k, acc) in sum.iter_mut().enumerate() {
            *acc += ((re[k] * re[k] + im[k] * im[k]).sqrt() * scale) as f64;
        }
        used += 1;
    }
    if used == 0 {
        return None;
    }
    Some(NoiseSpectrum {
        fft_size: n,
        sample_rate,
        magnitudes: sum.iter().map(|&v| (v / used as f64) as f32).collect(),
        frames: used,
    })
}

/// Smoothing radii (frequency bins, time frames) for a `smoothing` of 0..1.
pub fn smoothing_radii(smoothing: f32) -> (usize, usize) {
    let s = if smoothing.is_finite() { smoothing.clamp(0.0, 1.0) } else { 0.5 };
    ((s * 4.0).round() as usize, (s * 3.0).round() as usize)
}

/// Spectral gating of a loop layer with `noise` (offline). The output has the
/// input's length; `amount_db <= 0` returns the input unchanged (bit-exact).
/// `progress` receives 0..=1.
pub fn spectral_gate(
    samples: &[f32],
    sample_rate: u32,
    noise: &NoiseSpectrum,
    settings: &GateSettings,
    progress: &mut dyn FnMut(f32),
) -> Vec<f32> {
    let n = noise.fft_size;
    let amount = if settings.amount_db.is_finite() { settings.amount_db.clamp(0.0, 120.0) } else { 0.0 };
    if samples.is_empty() || amount <= 0.0 || !valid_fft_size(n) || noise.magnitudes.len() != n / 2 + 1 {
        progress(1.0);
        return samples.to_vec();
    }
    let len = samples.len();
    let hop = n / 4;
    let bins = n / 2 + 1;
    let frames = len.div_ceil(hop);
    let window = hann(n);
    let scale = 2.0 / window.iter().sum::<f32>().max(f32::EPSILON);
    let fft = Fft::new(n);
    let floor = 10f32.powf(-amount / 20.0);
    let sens = if settings.sensitivity_db.is_finite() { settings.sensitivity_db.clamp(-24.0, 48.0) } else { 6.0 };
    let threshold: Vec<f32> = noise.bins_for(sample_rate).iter().map(|m| m * 10f32.powf(sens / 20.0)).collect();
    let (fr, tr) = smoothing_radii(settings.smoothing);

    // 1. Analysis (circular): complex spectra and magnitudes of every frame.
    let mut spec_re = vec![0.0f32; frames * bins];
    let mut spec_im = vec![0.0f32; frames * bins];
    let mut mag = vec![0.0f32; frames * bins];
    let (mut re, mut im) = (vec![0.0f32; n], vec![0.0f32; n]);
    for f in 0..frames {
        let start = f * hop;
        for i in 0..n {
            re[i] = samples[(start + i) % len] * window[i];
        }
        im.fill(0.0);
        fft.forward(&mut re, &mut im);
        let row = f * bins;
        for k in 0..bins {
            spec_re[row + k] = re[k];
            spec_im[row + k] = im[k];
            mag[row + k] = (re[k] * re[k] + im[k] * im[k]).sqrt() * scale;
        }
        if f % 64 == 0 {
            progress(0.4 * f as f32 / frames as f32);
        }
    }

    // 2. Gate on the magnitude averaged over the time × frequency neighbourhood.
    let smoothed = box_average(&mag, frames, bins, fr, tr);
    let mut gate: Vec<f32> =
        smoothed.iter().enumerate().map(|(i, &m)| if m > threshold[i % bins] { 1.0 } else { floor }).collect();
    // 3. Dilate (keep signal edges open), then average (soft transitions).
    if fr > 0 || tr > 0 {
        gate = box_max(&gate, frames, bins, fr, tr);
        gate = box_average(&gate, frames, bins, fr, tr);
    }
    progress(0.6);

    // 4. Synthesis: weighted overlap-add, normalised by the summed squared window.
    let mut out = vec![0.0f64; len];
    let mut norm = vec![0.0f64; len];
    for f in 0..frames {
        let row = f * bins;
        for k in 0..bins {
            let g = gate[row + k];
            re[k] = spec_re[row + k] * g;
            im[k] = spec_im[row + k] * g;
        }
        // Hermitian mirror, then inverse via conj(FFT(conj(X))) / n.
        for k in 1..n / 2 {
            re[n - k] = re[k];
            im[n - k] = -im[k];
        }
        for v in im.iter_mut() {
            *v = -*v;
        }
        fft.forward(&mut re, &mut im);
        let start = f * hop;
        for i in 0..n {
            let j = (start + i) % len;
            let w = window[i] as f64;
            out[j] += re[i] as f64 / n as f64 * w;
            norm[j] += w * w;
        }
        if f % 64 == 0 {
            progress(0.6 + 0.4 * f as f32 / frames as f32);
        }
    }
    progress(1.0);
    out.iter().zip(&norm).map(|(&y, &w)| if w > 1e-12 { (y / w) as f32 } else { 0.0 }).collect()
}

/// Mean over `t ± tr` (circular) and `k ± fr` (clamped) of a frames × bins grid.
fn box_average(v: &[f32], frames: usize, bins: usize, fr: usize, tr: usize) -> Vec<f32> {
    if fr == 0 && tr == 0 {
        return v.to_vec();
    }
    // Frequency pass (prefix sums per row), then a circular time pass.
    let mut freq = vec![0.0f32; v.len()];
    let mut prefix = vec![0.0f64; bins + 1];
    for f in 0..frames {
        let row = &v[f * bins..(f + 1) * bins];
        for k in 0..bins {
            prefix[k + 1] = prefix[k] + row[k] as f64;
        }
        for k in 0..bins {
            let (lo, hi) = (k.saturating_sub(fr), (k + fr).min(bins - 1));
            freq[f * bins + k] = ((prefix[hi + 1] - prefix[lo]) / (hi - lo + 1) as f64) as f32;
        }
    }
    if tr == 0 {
        return freq;
    }
    let span = (2 * tr + 1) as f64;
    let mut out = vec![0.0f32; v.len()];
    for k in 0..bins {
        for f in 0..frames {
            let mut acc = 0.0f64;
            for d in 0..=2 * tr {
                let t = (f + frames * (tr + 1) + d - tr) % frames;
                acc += freq[t * bins + k] as f64;
            }
            out[f * bins + k] = (acc / span) as f32;
        }
    }
    out
}

/// Maximum over `t ± tr` (circular) and `k ± fr` (clamped).
fn box_max(v: &[f32], frames: usize, bins: usize, fr: usize, tr: usize) -> Vec<f32> {
    let mut freq = vec![0.0f32; v.len()];
    for f in 0..frames {
        for k in 0..bins {
            let (lo, hi) = (k.saturating_sub(fr), (k + fr).min(bins - 1));
            freq[f * bins + k] = v[f * bins + lo..=f * bins + hi].iter().copied().fold(f32::MIN, f32::max);
        }
    }
    if tr == 0 {
        return freq;
    }
    let mut out = vec![0.0f32; v.len()];
    for k in 0..bins {
        for f in 0..frames {
            let mut m = f32::MIN;
            for d in 0..=2 * tr {
                let t = (f + frames * (tr + 1) + d - tr) % frames;
                m = m.max(freq[t * bins + k]);
            }
            out[f * bins + k] = m;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal;

    const SR: u32 = 48_000;

    /// Deterministic white noise (xorshift), uniform in ±amp.
    fn noise(len: usize, amp: f32, seed: u64) -> Vec<f32> {
        let mut s = seed.max(1);
        (0..len)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                ((s >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) as f32 * amp
            })
            .collect()
    }

    /// Energy (dB) of `x` between `lo` and `hi` Hz, averaged over Hann frames.
    fn band_db(x: &[f32], lo: f32, hi: f32) -> f32 {
        let n = 4096;
        let w = hann(n);
        let fft = Fft::new(n);
        let mut total = 0.0f64;
        let mut count = 0;
        for start in (0..x.len().saturating_sub(n)).step_by(n / 2) {
            let mut re: Vec<f32> = (0..n).map(|i| x[start + i] * w[i]).collect();
            let mut im = vec![0.0; n];
            fft.forward(&mut re, &mut im);
            let (a, b) = ((lo * n as f32 / SR as f32) as usize, (hi * n as f32 / SR as f32) as usize);
            total += (a..b).map(|k| (re[k] * re[k] + im[k] * im[k]) as f64).sum::<f64>();
            count += 1;
        }
        (10.0 * (total / count.max(1) as f64 + 1e-30).log10()) as f32
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    fn profile(seed: u64) -> NoiseSpectrum {
        learn_noise(&noise(SR as usize * 2, 0.02, seed), SR, &LearnOptions::default()).unwrap()
    }

    #[test]
    fn learning_white_noise_gives_a_flat_profile() {
        let p = profile(7);
        assert_eq!(p.fft_size, DEFAULT_FFT_SIZE);
        assert_eq!(p.magnitudes.len(), DEFAULT_FFT_SIZE / 2 + 1);
        assert!(p.frames > 150, "{}", p.frames);
        // Flat within ±1.5 dB away from DC and Nyquist.
        let mid: Vec<f32> = p.magnitudes[20..1000].to_vec();
        let mean = mid.iter().sum::<f32>() / mid.len() as f32;
        assert!(mean > 0.0);
        for (k, m) in mid.iter().enumerate() {
            assert!((20.0 * (m / mean).log10()).abs() < 1.5, "bin {}: {m} vs {mean}", k + 20);
        }
        // Too short or bad size: no profile.
        assert!(learn_noise(&[0.0; 100], SR, &LearnOptions::default()).is_none());
        assert!(learn_noise(&noise(10_000, 0.1, 1), SR, &LearnOptions { fft_size: 1000, ..Default::default() }).is_none());
    }

    #[test]
    fn learning_skips_click_bleed() {
        let mut x = noise(SR as usize * 2, 0.01, 3);
        // Loud clicks every half second (5 ms bursts).
        for c in 0..4 {
            let at = c * SR as usize / 2 + 1000;
            for (i, v) in x[at..at + 240].iter_mut().enumerate() {
                *v += 0.8 * (i as f32 * 0.3).sin();
            }
        }
        let clean = learn_noise(&noise(SR as usize * 2, 0.01, 3), SR, &LearnOptions::default()).unwrap();
        let with_clicks = learn_noise(&x, SR, &LearnOptions::default()).unwrap();
        let kept_all = learn_noise(&x, SR, &LearnOptions { transient_reject_db: None, ..Default::default() }).unwrap();
        assert!(with_clicks.frames < kept_all.frames);
        let level = |p: &NoiseSpectrum| p.magnitudes[10..900].iter().sum::<f32>();
        let err = (level(&with_clicks) / level(&clean)).log10().abs();
        let err_all = (level(&kept_all) / level(&clean)).log10().abs();
        assert!(err < 0.05 && err_all > err * 2.0, "{err} {err_all}");
    }

    #[test]
    fn gating_reduces_the_noise_floor_by_the_amount_and_keeps_the_tone() {
        let len = SR as usize * 2;
        let tone = signal::sine(1000.0, SR, len, 0.5);
        let hiss = noise(len, 0.02, 11);
        let input: Vec<f32> = tone.iter().zip(&hiss).map(|(a, b)| a + b).collect();
        let p = profile(99); // a different realisation of the same noise
        for amount in [12.0f32, 20.0] {
            let settings = GateSettings { amount_db: amount, ..Default::default() };
            let out = spectral_gate(&input, SR, &p, &settings, &mut |_| {});
            assert_eq!(out.len(), input.len());
            // Noise floor away from the tone drops by at least the amount.
            let drop = band_db(&input, 3000.0, 15000.0) - band_db(&out, 3000.0, 15000.0);
            assert!(drop >= amount - 0.5, "amount {amount}: dropped {drop} dB");
            // The tone keeps its level (±0.5 dB).
            let tone_change = band_db(&out, 900.0, 1100.0) - band_db(&input, 900.0, 1100.0);
            assert!(tone_change.abs() < 0.5, "tone changed {tone_change} dB");
            // SNR against the clean tone improves.
            let err_in: Vec<f32> = input.iter().zip(&tone).map(|(a, b)| a - b).collect();
            let err_out: Vec<f32> = out.iter().zip(&tone).map(|(a, b)| a - b).collect();
            let gain_db = 20.0 * (rms(&err_in) / rms(&err_out)).log10();
            assert!(gain_db > 10.0, "SNR improved by {gain_db} dB");
        }
    }

    #[test]
    fn amount_zero_is_bit_exact_and_unity_gains_reconstruct() {
        let x: Vec<f32> = noise(30_001, 0.3, 5);
        let p = profile(1);
        let same = spectral_gate(&x, SR, &p, &GateSettings { amount_db: 0.0, ..Default::default() }, &mut |_| {});
        assert_eq!(same, x);
        // A profile of zeros: every bin is "signal", so the STFT round trip must reproduce the input.
        let silent = NoiseSpectrum { magnitudes: vec![0.0; p.magnitudes.len()], ..p.clone() };
        let y = spectral_gate(&x, SR, &silent, &GateSettings::default(), &mut |_| {});
        let worst = x.iter().zip(&y).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "{worst}");
    }

    #[test]
    fn gating_is_deterministic_and_reports_progress() {
        let x = noise(20_000, 0.1, 8);
        let p = profile(2);
        let mut seen = Vec::new();
        let a = spectral_gate(&x, SR, &p, &GateSettings::default(), &mut |f| seen.push(f));
        let b = spectral_gate(&x, SR, &p, &GateSettings::default(), &mut |_| {});
        assert_eq!(a, b);
        assert_eq!(seen.last(), Some(&1.0));
        assert!(seen.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn profiles_map_to_other_rates_by_frequency() {
        let p = NoiseSpectrum {
            fft_size: 256,
            sample_rate: 48_000,
            magnitudes: (0..129).map(|k| k as f32).collect(),
            frames: 1,
        };
        assert_eq!(p.bins_for(48_000), p.magnitudes);
        let other = p.bins_for(24_000);
        assert_eq!(other.len(), 129);
        assert!((other[64] - 32.0).abs() < 1e-4, "{}", other[64]); // 6 kHz at 24 kHz = bin 32 at 48 kHz
    }
}
