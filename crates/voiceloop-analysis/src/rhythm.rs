//! Timing analysis: how onsets sit against a tempo grid, and tempo estimation.
//!
//! All inputs are onset positions in frames on the input clock (from
//! [`crate::onset`] or [`crate::segment`]). Pure arithmetic; no audio.
//!
//! Sign convention: a **positive** deviation is **late** (behind the grid),
//! negative is early.

use serde::{Deserialize, Serialize};

/// An evenly spaced grid: point `k` is at `origin_frame + k × step`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grid {
    pub sample_rate: u32,
    pub bpm: f64,
    /// Grid points per beat (1 = beats, 2 = eighths in 4/4, 4 = sixteenths ...).
    pub subdivision: u32,
    /// Frame of grid point 0 (beat 1). Subtract the input latency here so
    /// that a perfectly played note reads as zero deviation.
    pub origin_frame: f64,
}

impl Grid {
    pub fn step_frames(&self) -> f64 {
        self.sample_rate as f64 * 60.0 / self.bpm / self.subdivision.max(1) as f64
    }

    fn valid(&self) -> bool {
        self.sample_rate > 0 && self.bpm.is_finite() && self.bpm > 0.0 && self.origin_frame.is_finite()
    }

    /// Index and exact frame of the grid point nearest to `frame`.
    pub fn nearest(&self, frame: f64) -> (i64, f64) {
        let step = self.step_frames();
        let index = ((frame - self.origin_frame) / step).round();
        (index as i64, self.origin_frame + index * step)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingDeviation {
    pub frame: u64,
    /// Nearest grid point.
    pub grid_index: i64,
    /// Milliseconds from that grid point (positive = late).
    pub deviation_ms: f32,
    /// The same as a fraction of one grid step, in `[-0.5, 0.5]`.
    pub deviation_steps: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingReport {
    pub deviations: Vec<TimingDeviation>,
    /// Mean signed deviation: the player's overall tendency (positive = dragging).
    pub mean_ms: f32,
    /// Mean absolute deviation: overall tightness.
    pub mean_abs_ms: f32,
    /// Standard deviation: consistency, independent of a constant offset.
    pub std_ms: f32,
    pub max_abs_ms: f32,
    /// Onsets within `tolerance_ms` of the grid.
    pub on_time: u32,
    pub tolerance_ms: f32,
}

/// Measure each onset against `grid`. Returns `None` for an invalid grid.
pub fn analyze_timing(onsets: &[u64], grid: &Grid, tolerance_ms: f32) -> Option<TimingReport> {
    if !grid.valid() {
        return None;
    }
    let step = grid.step_frames();
    let ms_per_frame = 1_000.0 / grid.sample_rate as f64;
    let deviations: Vec<TimingDeviation> = onsets
        .iter()
        .map(|&frame| {
            let (grid_index, grid_frame) = grid.nearest(frame as f64);
            let delta = frame as f64 - grid_frame;
            TimingDeviation {
                frame,
                grid_index,
                deviation_ms: (delta * ms_per_frame) as f32,
                deviation_steps: (delta / step) as f32,
            }
        })
        .collect();

    let n = deviations.len().max(1) as f32;
    let mean_ms = deviations.iter().map(|d| d.deviation_ms).sum::<f32>() / n;
    let mean_abs_ms = deviations.iter().map(|d| d.deviation_ms.abs()).sum::<f32>() / n;
    let variance = deviations.iter().map(|d| (d.deviation_ms - mean_ms).powi(2)).sum::<f32>() / n;
    Some(TimingReport {
        mean_ms,
        mean_abs_ms,
        std_ms: variance.sqrt(),
        max_abs_ms: deviations.iter().map(|d| d.deviation_ms.abs()).fold(0.0, f32::max),
        on_time: deviations.iter().filter(|d| d.deviation_ms.abs() <= tolerance_ms).count() as u32,
        tolerance_ms,
        deviations,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TempoEstimate {
    pub bpm: f64,
    /// Frame of a beat (the fitted position of the first onset's beat).
    pub phase_frame: f64,
    /// Fraction of onset intervals that are a whole number of beats (within 15%).
    pub confidence: f32,
}

/// Estimate a steady tempo from onset times.
///
/// The median inter-onset interval gives the pulse. It is refined by a
/// least-squares line through (pulse number, onset frame), which averages out
/// timing jitter and tolerates skipped pulses, and the result is then folded
/// by factors of two into `min_bpm..=max_bpm`. Needs at least three onsets.
/// Tempo *changes* are not modelled: this is for checking a performance
/// against a click, not for beat-tracking arbitrary music.
pub fn estimate_tempo(onsets: &[u64], sample_rate: u32, min_bpm: f64, max_bpm: f64) -> Option<TempoEstimate> {
    if onsets.len() < 3 || sample_rate == 0 || !(min_bpm > 0.0 && max_bpm >= min_bpm * 2.0) {
        return None;
    }
    let mut times: Vec<f64> = onsets.iter().map(|&f| f as f64).collect();
    times.sort_by(f64::total_cmp);
    let mut intervals: Vec<f64> = times.windows(2).map(|w| w[1] - w[0]).filter(|&d| d > 0.0).collect();
    if intervals.len() < 2 {
        return None;
    }
    intervals.sort_by(f64::total_cmp);
    let beat = intervals[intervals.len() / 2];
    let frames_per_minute = sample_rate as f64 * 60.0;

    // Assign each onset a pulse number, then fit frame = phase + number * period.
    let first = times[0];
    let numbers: Vec<f64> = times.iter().map(|t| ((t - first) / beat).round()).collect();
    let n = times.len() as f64;
    let mean_x = numbers.iter().sum::<f64>() / n;
    let mean_y = times.iter().sum::<f64>() / n;
    let sxx: f64 = numbers.iter().map(|x| (x - mean_x).powi(2)).sum();
    if sxx <= 0.0 {
        return None;
    }
    let sxy: f64 = numbers.iter().zip(&times).map(|(x, y)| (x - mean_x) * (y - mean_y)).sum();
    let period = sxy / sxx;
    if !(period.is_finite() && period > 0.0) {
        return None;
    }

    let whole = times
        .windows(2)
        .filter(|w| {
            let beats = (w[1] - w[0]) / period;
            beats.round() >= 1.0 && (beats - beats.round()).abs() <= 0.15
        })
        .count();
    let mut bpm = frames_per_minute / period;
    for _ in 0..16 {
        if bpm > max_bpm {
            bpm /= 2.0;
        } else if bpm < min_bpm {
            bpm *= 2.0;
        } else {
            break;
        }
    }
    Some(TempoEstimate {
        bpm,
        phase_frame: mean_y - period * mean_x,
        confidence: whole as f32 / (times.len() - 1) as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn grid(bpm: f64, subdivision: u32) -> Grid {
        Grid { sample_rate: SR, bpm, subdivision, origin_frame: 0.0 }
    }

    /// Deterministic jitter in `[-range, range]` frames.
    fn jitter(i: usize, range: i64) -> i64 {
        ((i as i64 * 7_919 + 13) % (2 * range + 1)) - range
    }

    #[test]
    fn perfect_playing_has_zero_deviation() {
        // 120 BPM: a beat every 24000 frames.
        let onsets: Vec<u64> = (0..16).map(|k| k * 24_000).collect();
        let r = analyze_timing(&onsets, &grid(120.0, 1), 20.0).unwrap();
        assert_eq!((r.mean_ms, r.mean_abs_ms, r.std_ms, r.max_abs_ms, r.on_time), (0.0, 0.0, 0.0, 0.0, 16));
        assert_eq!(r.deviations[5].grid_index, 5);
    }

    #[test]
    fn constant_lateness_is_bias_not_inconsistency() {
        // Every note 15 ms (720 frames) late.
        let onsets: Vec<u64> = (0..12).map(|k| k * 24_000 + 720).collect();
        let r = analyze_timing(&onsets, &grid(120.0, 1), 20.0).unwrap();
        assert!((r.mean_ms - 15.0).abs() < 1e-3, "dragging by 15 ms: {}", r.mean_ms);
        assert!(r.std_ms < 1e-3, "but perfectly consistent: {}", r.std_ms);
        assert_eq!(r.on_time, 12);
        // Compensating latency in the grid origin removes the bias.
        let compensated = Grid { origin_frame: 720.0, ..grid(120.0, 1) };
        assert!(analyze_timing(&onsets, &compensated, 20.0).unwrap().mean_abs_ms < 1e-3);
    }

    #[test]
    fn early_and_late_notes_are_signed_and_counted() {
        // Beats at 100 BPM (28800 frames): -30 ms, +10 ms, +45 ms, on time.
        let onsets = [28_800 - 1_440, 57_600 + 480, 86_400 + 2_160, 115_200];
        let r = analyze_timing(&onsets, &grid(100.0, 1), 20.0).unwrap();
        let ms: Vec<f32> = r.deviations.iter().map(|d| d.deviation_ms).collect();
        for (got, want) in ms.iter().zip([-30.0, 10.0, 45.0, 0.0]) {
            assert!((got - want).abs() < 1e-3, "{ms:?}");
        }
        assert_eq!(r.on_time, 2);
        assert!((r.max_abs_ms - 45.0).abs() < 1e-3);
        assert!((r.deviations[0].deviation_steps + 0.05).abs() < 1e-4, "30 ms of a 600 ms beat");
    }

    #[test]
    fn subdivisions_and_fractional_grids() {
        // Eighth notes at 113 BPM: the step is a fractional number of frames.
        let g = grid(113.0, 2);
        let step = g.step_frames();
        assert!((step - 12_743.362_831_858).abs() < 1e-6);
        let onsets: Vec<u64> = (0..200).map(|k| (k as f64 * step).round() as u64).collect();
        let r = analyze_timing(&onsets, &g, 5.0).unwrap();
        // Rounding to whole frames is the only error: at most half a frame (0.0104 ms).
        assert!(r.max_abs_ms <= 0.0105, "{}", r.max_abs_ms);
        assert_eq!(r.deviations[199].grid_index, 199, "no drift after 200 steps");
        // An offbeat lands on an odd index of the eighth-note grid.
        assert_eq!(g.nearest(step * 3.0 + 100.0).0, 3);
    }

    #[test]
    fn invalid_grids_and_empty_input() {
        assert!(analyze_timing(&[1, 2], &grid(0.0, 1), 10.0).is_none());
        assert!(analyze_timing(&[1, 2], &Grid { origin_frame: f64::NAN, ..grid(120.0, 1) }, 10.0).is_none());
        let r = analyze_timing(&[], &grid(120.0, 1), 10.0).unwrap();
        assert_eq!((r.deviations.len(), r.mean_ms, r.on_time), (0, 0.0, 0));
    }

    #[test]
    fn tempo_is_recovered_from_jittery_onsets() {
        // 97 BPM with up to +-8 ms (384 frames) of jitter on every onset.
        let period = SR as f64 * 60.0 / 97.0;
        let onsets: Vec<u64> =
            (0..32).map(|k| (10_000.0 + k as f64 * period).round() as i64 + jitter(k, 384)).map(|f| f as u64).collect();
        let t = estimate_tempo(&onsets, SR, 60.0, 180.0).unwrap();
        assert!((t.bpm - 97.0).abs() < 0.2, "{}", t.bpm);
        assert!(t.confidence > 0.95);
        assert!((t.phase_frame - 10_000.0).abs() < 400.0, "{}", t.phase_frame);
    }

    #[test]
    fn tempo_survives_skipped_beats_and_folds_into_range() {
        let period = 24_000u64; // 120 BPM
        let beats = [0u64, 1, 2, 4, 5, 6, 8, 9, 12, 13, 14, 15];
        let onsets: Vec<u64> = beats.iter().map(|b| 5_000 + b * period).collect();
        let t = estimate_tempo(&onsets, SR, 60.0, 180.0).unwrap();
        assert!((t.bpm - 120.0).abs() < 1e-6, "{}", t.bpm);
        assert_eq!(t.confidence, 1.0);

        // The same onsets read as half-time or double-time when the range demands it.
        assert!((estimate_tempo(&onsets, SR, 40.0, 90.0).unwrap().bpm - 60.0).abs() < 1e-6);
        assert!((estimate_tempo(&onsets, SR, 200.0, 400.0).unwrap().bpm - 240.0).abs() < 1e-6);
    }

    #[test]
    fn tempo_needs_enough_evidence() {
        assert!(estimate_tempo(&[0, 24_000], SR, 60.0, 180.0).is_none());
        assert!(estimate_tempo(&[5, 5, 5, 5], SR, 60.0, 180.0).is_none());
        assert!(estimate_tempo(&[0, 24_000, 48_000], SR, 100.0, 150.0).is_none(), "range must span an octave");
        // Irregular onsets still yield a number, but with low confidence.
        let random: Vec<u64> = [0u64, 7_000, 31_000, 36_500, 58_000, 91_000, 99_000, 131_000].to_vec();
        let t = estimate_tempo(&random, SR, 60.0, 180.0).unwrap();
        assert!(t.confidence < 0.6, "{}", t.confidence);
    }
}
