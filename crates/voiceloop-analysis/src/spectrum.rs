//! Magnitude spectrum and spectral peaks of a block of mono samples.
//!
//! A Hann-windowed, zero-padded radix-2 FFT (written here; the crate has no
//! FFT dependency), local-maximum peak picking, and parabolic interpolation of
//! each peak on the log magnitude. Magnitudes are scaled so a sine of
//! amplitude `A` produces a peak of about `A`.
//!
//! Buffers are reused between calls of the same block length. It still
//! allocates when the length changes, so it belongs on an analysis thread,
//! never in an audio callback.

use serde::Serialize;

/// One spectral peak (a local maximum of the magnitude spectrum).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpectralPeak {
    /// Interpolated frequency in Hz.
    pub hz: f32,
    /// Interpolated linear magnitude (a sine of amplitude `A` gives about `A`).
    pub magnitude: f32,
}

/// In-place iterative radix-2 complex FFT of a fixed power-of-two size.
pub(crate) struct Fft {
    n: usize,
    /// `(cos, sin)` of `2πk/n` for `k < n/2`.
    twiddles: Vec<(f32, f32)>,
}

impl Fft {
    pub(crate) fn new(n: usize) -> Self {
        debug_assert!(n.is_power_of_two() && n >= 2);
        let twiddles = (0..n / 2)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .collect();
        Self { n, twiddles }
    }

    /// Forward transform (`X[k] = Σ x[t]·e^(-2πikt/n)`) of `re + i·im`, in place.
    pub(crate) fn forward(&self, re: &mut [f32], im: &mut [f32]) {
        let n = self.n;
        debug_assert!(re.len() == n && im.len() == n);
        // Bit-reversal permutation.
        let mut j = 0usize;
        for i in 1..n {
            let mut bit = n >> 1;
            while j & bit != 0 {
                j ^= bit;
                bit >>= 1;
            }
            j |= bit;
            if i < j {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        // Butterflies.
        let mut len = 2;
        while len <= n {
            let half = len / 2;
            let step = n / len;
            for start in (0..n).step_by(len) {
                for k in 0..half {
                    let (c, s) = self.twiddles[k * step];
                    let (a, b) = (start + k, start + k + half);
                    // (re + i·im)·(c − i·s)
                    let tr = re[b] * c + im[b] * s;
                    let ti = im[b] * c - re[b] * s;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len <<= 1;
        }
    }
}

/// Reusable spectrum analyser: windowing, FFT, and peak picking.
pub struct Spectrum {
    fft: Fft,
    window: Vec<f32>,
    window_gain: f32,
    re: Vec<f32>,
    im: Vec<f32>,
    magnitude: Vec<f32>,
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::new()
    }
}

impl Spectrum {
    pub fn new() -> Self {
        Self {
            fft: Fft::new(2),
            window: Vec::new(),
            window_gain: 1.0,
            re: vec![0.0; 2],
            im: vec![0.0; 2],
            magnitude: vec![0.0; 2],
        }
    }

    fn prepare(&mut self, block_len: usize, fft_size: usize) {
        if self.window.len() != block_len {
            let denom = block_len.max(2) as f64 - 1.0;
            self.window =
                (0..block_len).map(|i| (0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / denom).cos()) as f32).collect();
            self.window_gain = self.window.iter().sum::<f32>().max(f32::EPSILON);
        }
        if self.fft.n != fft_size {
            self.fft = Fft::new(fft_size);
            self.re = vec![0.0; fft_size];
            self.im = vec![0.0; fft_size];
            self.magnitude = vec![0.0; fft_size / 2 + 1];
        }
    }

    /// Spectral peaks of `samples` between `min_hz` and `max_hz`, sorted by
    /// frequency. The block is zero-padded to the next power of two that is
    /// at least `min_fft_size`. Peaks weaker than `floor_db` below the
    /// strongest peak in range are dropped.
    pub fn peaks(
        &mut self,
        samples: &[f32],
        sample_rate: u32,
        min_fft_size: usize,
        min_hz: f32,
        max_hz: f32,
        floor_db: f32,
    ) -> Vec<SpectralPeak> {
        if samples.len() < 4 || sample_rate == 0 {
            return Vec::new();
        }
        let n = samples.len().max(min_fft_size).max(4).next_power_of_two();
        self.prepare(samples.len(), n);
        for (i, r) in self.re.iter_mut().enumerate() {
            *r = samples.get(i).map_or(0.0, |&s| s * self.window[i]);
        }
        self.im.fill(0.0);
        self.fft.forward(&mut self.re, &mut self.im);
        let scale = 2.0 / self.window_gain;
        for (k, m) in self.magnitude.iter_mut().enumerate() {
            *m = (self.re[k] * self.re[k] + self.im[k] * self.im[k]).sqrt() * scale;
        }

        let bin_hz = sample_rate as f32 / n as f32;
        let k_lo = ((min_hz / bin_hz).floor() as usize).max(1);
        let k_hi = ((max_hz / bin_hz).ceil() as usize).min(n / 2 - 1);
        if k_lo >= k_hi {
            return Vec::new();
        }
        let mag = &self.magnitude;
        let strongest = mag[k_lo..=k_hi].iter().copied().fold(0.0f32, f32::max);
        if strongest <= 0.0 {
            return Vec::new();
        }
        let floor = strongest * 10f32.powf(floor_db / 20.0);
        let mut peaks = Vec::new();
        for k in k_lo..=k_hi {
            let m = mag[k];
            if m < floor || m <= mag[k - 1] || m < mag[k + 1] {
                continue;
            }
            // Parabolic interpolation on the log magnitude.
            let ln = |x: f32| (x.max(1e-12)).ln();
            let (a, b, c) = (ln(mag[k - 1]), ln(m), ln(mag[k + 1]));
            let denom = a - 2.0 * b + c;
            let p = if denom.abs() > 1e-12 { (0.5 * (a - c) / denom).clamp(-0.5, 0.5) } else { 0.0 };
            let hz = (k as f32 + p) * bin_hz;
            if hz < min_hz || hz > max_hz {
                continue;
            }
            peaks.push(SpectralPeak { hz, magnitude: (b - 0.25 * (a - c) * p).exp() });
        }
        peaks
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal;

    #[test]
    fn fft_matches_a_direct_dft() {
        let n = 16;
        let x: Vec<f32> = (0..n).map(|i| ((i * 7 % 5) as f32 - 2.0) * 0.3).collect();
        let (mut re, mut im) = (x.clone(), vec![0.0; n]);
        Fft::new(n).forward(&mut re, &mut im);
        for k in 0..n {
            let (mut dr, mut di) = (0.0f64, 0.0f64);
            for (t, &v) in x.iter().enumerate() {
                let a = -std::f64::consts::TAU * (k * t) as f64 / n as f64;
                dr += v as f64 * a.cos();
                di += v as f64 * a.sin();
            }
            assert!((re[k] as f64 - dr).abs() < 1e-4 && (im[k] as f64 - di).abs() < 1e-4, "bin {k}");
        }
    }

    #[test]
    fn sine_peak_has_its_frequency_and_amplitude() {
        let x = signal::sine(330.0, 48_000, 8192, 0.5);
        let peaks = Spectrum::new().peaks(&x, 48_000, 8192, 55.0, 2000.0, -40.0);
        let top = peaks.iter().max_by(|a, b| a.magnitude.total_cmp(&b.magnitude)).unwrap();
        assert!((top.hz - 330.0).abs() < 0.5, "{}", top.hz);
        assert!((top.magnitude - 0.5).abs() < 0.03, "{}", top.magnitude);
    }
}
