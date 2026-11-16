//! RMS and peak metering. Everything here is allocation-free and safe to call
//! from the realtime audio callback.

use serde::Serialize;

/// One meter reading covering a window of input frames.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeterFrame {
    /// Root-mean-square amplitude across all channels, linear 0.0..=1.0.
    pub rms: f32,
    /// Absolute peak amplitude across all channels, linear 0.0..=1.0+.
    pub peak: f32,
    /// Authoritative input-clock position (frames since capture start) at the end of the window.
    pub end_frame: u64,
}

impl MeterFrame {
    /// RMS in dBFS, floored at -100 dB.
    pub fn rms_db(&self) -> f32 {
        amplitude_to_db(self.rms)
    }
}

pub fn amplitude_to_db(a: f32) -> f32 {
    if a <= 1e-5 {
        -100.0
    } else {
        20.0 * a.log10()
    }
}

/// Accumulates interleaved samples and emits a [`MeterFrame`] every
/// `window_frames` frames.
#[derive(Debug)]
pub struct MeterAccumulator {
    channels: usize,
    window_frames: u64,
    sum_sq: f64,
    peak: f32,
    frames_in_window: u64,
    total_frames: u64,
}

impl MeterAccumulator {
    pub fn new(channels: u16, window_frames: u32) -> Self {
        Self {
            channels: channels.max(1) as usize,
            window_frames: window_frames.max(1) as u64,
            sum_sq: 0.0,
            peak: 0.0,
            frames_in_window: 0,
            total_frames: 0,
        }
    }

    /// Feed interleaved samples; call `emit` for every completed window.
    /// Never allocates.
    #[inline]
    pub fn process<I, F>(&mut self, samples: I, mut emit: F)
    where
        I: IntoIterator<Item = f32>,
        F: FnMut(MeterFrame),
    {
        let mut ch = 0usize;
        for s in samples {
            let a = s.abs();
            self.sum_sq += (s as f64) * (s as f64);
            if a > self.peak {
                self.peak = a;
            }
            ch += 1;
            if ch == self.channels {
                ch = 0;
                self.frames_in_window += 1;
                self.total_frames += 1;
                if self.frames_in_window == self.window_frames {
                    let n = (self.frames_in_window as f64) * (self.channels as f64);
                    emit(MeterFrame {
                        rms: (self.sum_sq / n).sqrt() as f32,
                        peak: self.peak,
                        end_frame: self.total_frames,
                    });
                    self.sum_sq = 0.0;
                    self.peak = 0.0;
                    self.frames_in_window = 0;
                }
            }
        }
    }

    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_is_zero() {
        let mut m = MeterAccumulator::new(1, 4);
        let mut out = vec![];
        m.process([0.0f32; 8], |f| out.push(f));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rms, 0.0);
        assert_eq!(out[1].end_frame, 8);
        assert_eq!(out[0].rms_db(), -100.0);
    }

    #[test]
    fn full_scale_square_wave_is_unity_rms() {
        let mut m = MeterAccumulator::new(2, 4);
        let mut out = vec![];
        m.process([1.0f32, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0], |f| out.push(f));
        assert_eq!(out.len(), 1);
        assert!((out[0].rms - 1.0).abs() < 1e-6);
        assert_eq!(out[0].peak, 1.0);
        assert_eq!(out[0].end_frame, 4);
    }

    #[test]
    fn sine_rms_is_inverse_sqrt2() {
        let n = 48_000;
        let mut m = MeterAccumulator::new(1, n);
        let mut out = vec![];
        let samples = (0..n).map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin());
        m.process(samples, |f| out.push(f));
        assert!((out[0].rms - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-3);
        assert!((out[0].rms_db() - -3.01).abs() < 0.05);
    }

    #[test]
    fn partial_window_carries_over_between_calls() {
        let mut m = MeterAccumulator::new(1, 3);
        let mut out = vec![];
        m.process([0.5f32, 0.5], |f| out.push(f));
        assert!(out.is_empty());
        m.process([0.5f32], |f| out.push(f));
        assert_eq!(out.len(), 1);
        assert!((out[0].rms - 0.5).abs() < 1e-6);
    }
}
