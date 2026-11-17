//! Deterministic synthetic signals for tests, benchmarks, and examples.
//!
//! Everything is generated from closed-form phase or a fixed-seed generator,
//! so results are identical on every machine.

use std::f64::consts::TAU;

/// Sine wave. Phase is accumulated in `f64` so long signals stay exact.
pub fn sine(hz: f64, sample_rate: u32, frames: usize, amplitude: f32) -> Vec<f32> {
    harmonic(hz, sample_rate, frames, &[amplitude])
}

/// Sum of harmonics: `partials[k]` is the amplitude of harmonic `k + 1`.
pub fn harmonic(hz: f64, sample_rate: u32, frames: usize, partials: &[f32]) -> Vec<f32> {
    sweep(|_| hz, sample_rate, frames, partials)
}

/// Harmonic tone whose fundamental at time `t` seconds is `hz_at(t)`.
/// Phase is the running integral of frequency, so glides and vibrato are
/// continuous.
pub fn sweep(hz_at: impl Fn(f64) -> f64, sample_rate: u32, frames: usize, partials: &[f32]) -> Vec<f32> {
    let sr = sample_rate as f64;
    let mut phase = 0.0f64;
    let mut out = Vec::with_capacity(frames);
    for i in 0..frames {
        let mut s = 0.0f64;
        for (k, &a) in partials.iter().enumerate() {
            s += a as f64 * (phase * (k + 1) as f64).sin();
        }
        out.push(s as f32);
        phase = (phase + TAU * hz_at(i as f64 / sr) / sr) % (TAU * 1024.0);
    }
    out
}

/// Tone with sinusoidal vibrato: `extent_cents` peak deviation at `rate_hz`.
pub fn vibrato(
    hz: f64,
    extent_cents: f64,
    rate_hz: f64,
    sample_rate: u32,
    frames: usize,
    partials: &[f32],
) -> Vec<f32> {
    sweep(|t| hz * 2f64.powf(extent_cents / 1200.0 * (TAU * rate_hz * t).sin()), sample_rate, frames, partials)
}

/// Exponential (constant semitones-per-second) glide from `from_hz` to `to_hz`.
pub fn glide(from_hz: f64, to_hz: f64, sample_rate: u32, frames: usize, partials: &[f32]) -> Vec<f32> {
    let seconds = frames as f64 / sample_rate as f64;
    sweep(|t| from_hz * (to_hz / from_hz).powf(t / seconds), sample_rate, frames, partials)
}

/// A plausible sung-vowel spectrum: strong low harmonics, rolling off.
pub const VOICE_PARTIALS: [f32; 8] = [0.5, 0.35, 0.25, 0.12, 0.08, 0.05, 0.03, 0.02];

/// Second and third harmonics stronger than the fundamental: the classic
/// trigger for octave errors.
pub const WEAK_FUNDAMENTAL_PARTIALS: [f32; 4] = [0.12, 0.45, 0.3, 0.15];

/// Uniform white noise in `[-amplitude, amplitude]` from a fixed-seed xorshift generator.
pub fn noise(seed: u32, frames: usize, amplitude: f32) -> Vec<f32> {
    let mut state = seed.max(1);
    (0..frames)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
        })
        .collect()
}

/// Element-wise sum; the result has the length of the shorter input.
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

/// Multiply by a linear attack/release envelope (in frames) to avoid edge clicks.
pub fn shape(mut signal: Vec<f32>, attack: usize, release: usize) -> Vec<f32> {
    let n = signal.len();
    for (i, s) in signal.iter_mut().enumerate() {
        let up = if attack > 0 { ((i + 1) as f32 / attack as f32).min(1.0) } else { 1.0 };
        let down = if release > 0 { ((n - i) as f32 / release as f32).min(1.0) } else { 1.0 };
        *s *= up.min(down);
    }
    signal
}

/// Percussive burst: noise with an exponential decay (time constant in frames).
pub fn burst(seed: u32, frames: usize, amplitude: f32, decay_frames: f32) -> Vec<f32> {
    noise(seed, frames, amplitude)
        .into_iter()
        .enumerate()
        .map(|(i, s)| s * (-(i as f32) / decay_frames.max(1.0)).exp())
        .collect()
}

/// Concatenate segments into one signal.
pub fn concat(parts: &[&[f32]]) -> Vec<f32> {
    let mut out = Vec::with_capacity(parts.iter().map(|p| p.len()).sum());
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

pub fn silence(frames: usize) -> Vec<f32> {
    vec![0.0; frames]
}

pub fn rms(signal: &[f32]) -> f32 {
    if signal.is_empty() {
        return 0.0;
    }
    (signal.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / signal.len() as f64).sqrt() as f32
}
