//! Offline sample-rate conversion for imported audio.
//!
//! Band-limited interpolation with a Kaiser-windowed sinc kernel (Smith,
//! "Digital Audio Resampling", CCRMA): 32 zero crossings per side, β = 9
//! (stopband about -90 dB), cutoff at 0.97 of the lower Nyquist frequency.
//! The kernel is tabulated at 512 phases per zero crossing and linearly
//! interpolated. Positions are computed absolutely from the rational
//! `in_rate / out_rate`, so nothing drifts over long files.
//!
//! This is for importing files (not realtime). A realtime adaptive
//! resampler for clock drift is a separate engine concern.

const ZERO_CROSSINGS: usize = 32;
const PHASES: usize = 512;
const BETA: f64 = 9.0;
const ROLLOFF: f64 = 0.97;

/// Zeroth-order modified Bessel function of the first kind (series).
fn bessel_i0(x: f64) -> f64 {
    let (mut sum, mut term, q) = (1.0, 1.0, x * x / 4.0);
    for k in 1..64 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

fn kernel_table() -> Vec<f64> {
    let n = ZERO_CROSSINGS * PHASES;
    let i0b = bessel_i0(BETA);
    (0..=n + 1)
        .map(|i| {
            let x = i as f64 / PHASES as f64; // in zero crossings
            if x >= ZERO_CROSSINGS as f64 {
                return 0.0;
            }
            let sinc = if i == 0 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
            let r = x / ZERO_CROSSINGS as f64;
            sinc * bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / i0b
        })
        .collect()
}

/// Number of output frames for `frames` input frames.
pub fn output_len(frames: usize, from: u32, to: u32) -> usize {
    ((frames as u128 * to as u128).div_ceil(from as u128)) as usize
}

/// Convert one channel from `from` Hz to `to` Hz.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() || from == 0 || to == 0 {
        return input.to_vec();
    }
    let table = kernel_table();
    let ratio = from as f64 / to as f64; // input frames per output frame
                                         // Cutoff relative to the input Nyquist: lower it when downsampling.
    let cutoff = ROLLOFF * (1.0 / ratio).min(1.0);
    let half_width = ZERO_CROSSINGS as f64 / cutoff; // in input frames
    let out_len = output_len(input.len(), from, to);
    let mut out = Vec::with_capacity(out_len);
    for n in 0..out_len {
        // Exact source position: n * from / to.
        let num = n as u128 * from as u128;
        let base = (num / to as u128) as i64;
        let frac = (num % to as u128) as f64 / to as f64;
        let lo = (base - half_width.floor() as i64).max(0);
        let hi = (base + half_width.ceil() as i64 + 1).min(input.len() as i64 - 1);
        let mut acc = 0.0f64;
        for k in lo..=hi {
            let d = ((k - base) as f64 - frac).abs() * cutoff; // in zero crossings
            let pos = d * PHASES as f64;
            let i = pos as usize;
            if i >= ZERO_CROSSINGS * PHASES {
                continue;
            }
            let f = pos - i as f64;
            let h = table[i] + (table[i + 1] - table[i]) * f;
            acc += input[k as usize] as f64 * h;
        }
        out.push((acc * cutoff) as f32);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames).map(|i| (0.5 * (2.0 * std::f64::consts::PI * hz * i as f64 / rate as f64).sin()) as f32).collect()
    }

    fn max_err_mid(a: &[f32], b: &[f32]) -> f32 {
        let (s, e) = (a.len() / 5, a.len() * 4 / 5);
        a[s..e].iter().zip(&b[s..e]).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
    }

    #[test]
    fn lengths_are_exact_ratios() {
        assert_eq!(output_len(44_100, 44_100, 48_000), 48_000);
        assert_eq!(output_len(96_000, 96_000, 44_100), 44_100);
        assert_eq!(output_len(1, 44_100, 48_000), 2);
        assert_eq!(resample(&[1.0, 2.0], 48_000, 48_000), vec![1.0, 2.0]);
    }

    #[test]
    fn tones_are_preserved_across_common_rate_pairs() {
        for (from, to) in
            [(44_100u32, 48_000u32), (48_000, 44_100), (96_000, 48_000), (88_200, 48_000), (48_000, 96_000)]
        {
            for hz in [100.0, 1_000.0, 15_000.0] {
                let y = resample(&tone(hz, from, from as usize / 4), from, to);
                let err = max_err_mid(&y, &tone(hz, to, y.len()));
                assert!(err < 2e-3, "{from}->{to} at {hz} Hz: max error {err}");
            }
        }
    }

    #[test]
    fn content_above_the_new_nyquist_is_removed() {
        let y = resample(&tone(30_000.0, 96_000, 24_000), 96_000, 44_100);
        let mid = &y[y.len() / 5..y.len() * 4 / 5];
        let peak = mid.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak < 0.5 * 1e-3, "30 kHz leaks at {peak}"); // > 60 dB down
    }
}
