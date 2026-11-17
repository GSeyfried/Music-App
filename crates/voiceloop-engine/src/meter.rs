//! RMS and peak metering. Everything here is allocation-free and safe to call
//! from the realtime audio callback.
//!
//! A [`MeterFrame`] carries three views of the same window:
//! - `rms`/`peak`: across every channel of the device (all samples together);
//! - `routed_rms`/`routed_peak`: the signal the engine actually uses after
//!   the [`InputRoute`](crate::routing::InputRoute) (what the looper records);
//! - `channel_rms`/`channel_peak`: each device channel on its own (the first
//!   [`MAX_METER_CHANNELS`]), so a user can see which interface input has signal.
//!
//! Frame alignment: callers pass whole interleaved frames (CPAL delivers
//! buffers whose length is a multiple of the channel count). [`MeterAccumulator::process`]
//! also tolerates a split frame across calls by carrying the channel position.

use serde::Serialize;

pub use crate::routing::MAX_METER_CHANNELS;

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
    /// RMS of the routed (engine) signal.
    pub routed_rms: f32,
    /// Peak of the routed (engine) signal.
    pub routed_peak: f32,
    /// Channels with valid entries in `channel_rms` / `channel_peak`.
    pub channel_count: u16,
    pub channel_rms: [f32; MAX_METER_CHANNELS],
    pub channel_peak: [f32; MAX_METER_CHANNELS],
}

impl MeterFrame {
    /// RMS in dBFS, floored at -100 dB.
    pub fn rms_db(&self) -> f32 {
        amplitude_to_db(self.rms)
    }
}

pub fn amplitude_to_db(a: f32) -> f32 {
    if a <= 1e-5 || a.is_nan() {
        -100.0
    } else {
        20.0 * a.log10()
    }
}

/// One device channel's level in a UI reading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelLevel {
    pub rms_db: f32,
    /// Highest peak since the previous reading.
    pub peak_db: f32,
    /// The peak reached full scale (≥ -0.1 dBFS).
    pub clipped: bool,
}

/// Peaks at or above this are reported as clipped (-0.1 dBFS).
pub const CLIP_LEVEL: f32 = 0.988_6;

impl ChannelLevel {
    /// Per-channel levels from the latest frame's RMS and held peaks.
    pub fn list(frame: &MeterFrame, held_peaks: &[f32; MAX_METER_CHANNELS]) -> Vec<ChannelLevel> {
        (0..frame.channel_count as usize)
            .map(|ch| ChannelLevel {
                rms_db: amplitude_to_db(frame.channel_rms[ch]),
                peak_db: amplitude_to_db(held_peaks[ch]),
                clipped: held_peaks[ch] >= CLIP_LEVEL,
            })
            .collect()
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
    routed_sum_sq: f64,
    routed_peak: f32,
    ch_sum_sq: [f64; MAX_METER_CHANNELS],
    ch_peak: [f32; MAX_METER_CHANNELS],
    /// Channel position within the current frame (carried across `process` calls).
    ch: usize,
    /// Sum of the current partial frame for `process`'s all-channel routed view.
    frame_sum: f32,
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
            routed_sum_sq: 0.0,
            routed_peak: 0.0,
            ch_sum_sq: [0.0; MAX_METER_CHANNELS],
            ch_peak: [0.0; MAX_METER_CHANNELS],
            ch: 0,
            frame_sum: 0.0,
            frames_in_window: 0,
            total_frames: 0,
        }
    }

    #[inline]
    fn sample(&mut self, ch: usize, s: f32) {
        let sq = (s as f64) * (s as f64);
        let a = s.abs();
        self.sum_sq += sq;
        if a > self.peak {
            self.peak = a;
        }
        if ch < MAX_METER_CHANNELS {
            self.ch_sum_sq[ch] += sq;
            if a > self.ch_peak[ch] {
                self.ch_peak[ch] = a;
            }
        }
    }

    #[inline]
    fn end_frame<F: FnMut(MeterFrame)>(&mut self, routed: f32, emit: &mut F) {
        self.routed_sum_sq += (routed as f64) * (routed as f64);
        if routed.abs() > self.routed_peak {
            self.routed_peak = routed.abs();
        }
        self.frames_in_window += 1;
        self.total_frames = self.total_frames.saturating_add(1);
        if self.frames_in_window == self.window_frames {
            let frames = self.frames_in_window as f64;
            let n = frames * (self.channels as f64);
            let count = self.channels.min(MAX_METER_CHANNELS);
            let mut channel_rms = [0.0f32; MAX_METER_CHANNELS];
            for (out, &sq) in channel_rms.iter_mut().zip(self.ch_sum_sq.iter()).take(count) {
                *out = (sq / frames).sqrt() as f32;
            }
            emit(MeterFrame {
                rms: (self.sum_sq / n).sqrt() as f32,
                peak: self.peak,
                end_frame: self.total_frames,
                routed_rms: (self.routed_sum_sq / frames).sqrt() as f32,
                routed_peak: self.routed_peak,
                channel_count: count as u16,
                channel_rms,
                channel_peak: self.ch_peak,
            });
            self.sum_sq = 0.0;
            self.peak = 0.0;
            self.routed_sum_sq = 0.0;
            self.routed_peak = 0.0;
            self.ch_sum_sq = [0.0; MAX_METER_CHANNELS];
            self.ch_peak = [0.0; MAX_METER_CHANNELS];
            self.frames_in_window = 0;
        }
    }

    /// Feed one whole interleaved frame plus the routed sample derived from it.
    /// Never allocates. This is what the capture callback uses.
    #[inline]
    pub fn process_frame<F: FnMut(MeterFrame)>(&mut self, frame: &[f32], routed: f32, mut emit: F) {
        for (ch, &s) in frame.iter().enumerate() {
            self.sample(ch, s);
        }
        self.end_frame(routed, &mut emit);
    }

    /// Feed interleaved samples; call `emit` for every completed window. The
    /// routed view is the average of all channels. Never allocates.
    #[inline]
    pub fn process<I, F>(&mut self, samples: I, mut emit: F)
    where
        I: IntoIterator<Item = f32>,
        F: FnMut(MeterFrame),
    {
        for s in samples {
            self.sample(self.ch, s);
            self.frame_sum += s;
            self.ch += 1;
            if self.ch == self.channels {
                let routed = self.frame_sum / self.channels as f32;
                self.ch = 0;
                self.frame_sum = 0.0;
                self.end_frame(routed, &mut emit);
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

    #[test]
    fn each_channel_is_metered_separately_and_the_routed_signal_on_its_own() {
        // Four-channel interface: only input 3 (index 2) carries signal.
        let mut m = MeterAccumulator::new(4, 2);
        let mut out = vec![];
        for _ in 0..2 {
            let frame = [0.0f32, 0.0, 0.5, 0.0];
            m.process_frame(&frame, frame[2], |f| out.push(f));
        }
        let f = out[0];
        assert_eq!(f.channel_count, 4);
        assert_eq!(&f.channel_peak[..4], &[0.0, 0.0, 0.5, 0.0]);
        assert!((f.channel_rms[2] - 0.5).abs() < 1e-6 && f.channel_rms[0] == 0.0);
        assert!((f.routed_rms - 0.5).abs() < 1e-6, "Mono route on channel 2 records at full level");
        assert!((f.rms - 0.25).abs() < 1e-6, "all-channel rms is diluted by the silent inputs");
    }

    #[test]
    fn a_frame_split_across_calls_keeps_channel_alignment() {
        let mut m = MeterAccumulator::new(2, 2);
        let mut out = vec![];
        m.process([1.0f32, 0.0, 1.0], |f| out.push(f));
        m.process([0.0f32], |f| out.push(f));
        assert_eq!(out.len(), 1);
        assert_eq!(&out[0].channel_peak[..2], &[1.0, 0.0]);
    }

    #[test]
    fn more_channels_than_the_meter_array_still_count_in_the_total() {
        let mut m = MeterAccumulator::new(40, 1);
        let mut out = vec![];
        let frame = [0.5f32; 40];
        m.process_frame(&frame, 0.5, |f| out.push(f));
        assert_eq!(out[0].channel_count as usize, MAX_METER_CHANNELS);
        assert!((out[0].rms - 0.5).abs() < 1e-6);
    }
}
