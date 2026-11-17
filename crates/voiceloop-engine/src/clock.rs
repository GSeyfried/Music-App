//! Host-clock anchors for the audio streams.
//!
//! Each stream callback publishes "at host time `h` my frame counter was `f`"
//! once per block through a wait-free seqlock ([`ClockAnchor`]). Other threads
//! use the newest anchor to convert a host time (for example a MIDI event's
//! timestamp) into a frame on that stream's clock, and the control thread
//! compares the input and output anchors over time to estimate clock drift
//! ([`DriftEstimator`]).
//!
//! Host time is nanoseconds since the first call to [`host_now_ns`] in this
//! process (a monotonic `Instant`; on macOS that is `mach_absolute_time`, the
//! same clock CoreAudio and CoreMIDI stamp with).
//!
//! Realtime-safe on the writer side: relaxed/ordered atomics only.

use std::sync::atomic::{fence, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static EPOCH: OnceLock<Instant> = OnceLock::new();

/// Monotonic nanoseconds since the engine's host-clock epoch. Does not
/// allocate or lock after the first call (which the control thread makes
/// before any stream exists; see [`init_epoch`]).
#[inline]
pub fn host_now_ns() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

/// Fix the epoch now, off the realtime threads.
pub fn init_epoch() {
    let _ = host_now_ns();
}

/// One published anchor: "at host time `host_ns` this stream was at `frame`".
///
/// The input stream anchors at callback start. The output stream anchors at
/// the **end** of each callback (host time at callback start + block
/// duration), after the looper has applied that block's commands, so the
/// transport frame and running state describe the block that just ran and
/// extrapolate correctly into the next one (see `looper/io.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Anchor {
    pub host_ns: u64,
    /// Frames this stream had processed at `host_ns`.
    pub frame: u64,
    /// Output stream only: looper transport frame at `host_ns`.
    pub transport_frame: i64,
    /// Output stream only: the transport is running (advancing one per frame).
    pub transport_running: bool,
    /// Output stream only: input position (input-clock frames, fractional) the
    /// drift bridge will deliver at output frame `frame`. Maps input frames to
    /// output/transport frames: `out = frame + (input − input_pos) / ratio`.
    pub input_pos: f64,
    /// Output stream only: input frames consumed per output frame (1 ± drift).
    pub ratio: f64,
    /// Output stream only: increments at every input discontinuity the bridge
    /// could not conceal (underrun, overflow skip, input overrun). Mappings made
    /// with an older epoch must be re-anchored.
    pub epoch: u64,
}

/// Single-writer seqlock around an [`Anchor`]. The writer (an audio callback)
/// never waits; readers retry if they raced a write.
#[derive(Debug, Default)]
pub struct ClockAnchor {
    seq: AtomicU64,
    host_ns: AtomicU64,
    frame: AtomicU64,
    transport: AtomicU64,
    running: AtomicU64,
    input_pos: AtomicU64,
    ratio: AtomicU64,
    epoch: AtomicU64,
}

impl ClockAnchor {
    /// Publish a new anchor. Realtime-safe; call from one thread only.
    #[inline]
    pub fn write(&self, a: Anchor) {
        let s = self.seq.load(Ordering::Relaxed);
        self.seq.store(s.wrapping_add(1), Ordering::Relaxed); // odd: write in progress
        fence(Ordering::Release);
        self.host_ns.store(a.host_ns, Ordering::Relaxed);
        self.frame.store(a.frame, Ordering::Relaxed);
        self.transport.store(a.transport_frame as u64, Ordering::Relaxed);
        self.running.store(a.transport_running as u64, Ordering::Relaxed);
        self.input_pos.store(a.input_pos.to_bits(), Ordering::Relaxed);
        self.ratio.store(a.ratio.to_bits(), Ordering::Relaxed);
        self.epoch.store(a.epoch, Ordering::Relaxed);
        self.seq.store(s.wrapping_add(2), Ordering::Release);
    }

    /// Latest anchor, or `None` before the first write (or if every retry raced the writer).
    pub fn read(&self) -> Option<Anchor> {
        for _ in 0..64 {
            let s1 = self.seq.load(Ordering::Acquire);
            if s1 == 0 {
                return None;
            }
            if s1 & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let a = Anchor {
                host_ns: self.host_ns.load(Ordering::Relaxed),
                frame: self.frame.load(Ordering::Relaxed),
                transport_frame: self.transport.load(Ordering::Relaxed) as i64,
                transport_running: self.running.load(Ordering::Relaxed) != 0,
                input_pos: f64::from_bits(self.input_pos.load(Ordering::Relaxed)),
                ratio: f64::from_bits(self.ratio.load(Ordering::Relaxed)),
                epoch: self.epoch.load(Ordering::Relaxed),
            };
            fence(Ordering::Acquire);
            if self.seq.load(Ordering::Relaxed) == s1 {
                return Some(a);
            }
        }
        None
    }
}

impl Anchor {
    /// Frame on this stream's clock at `host_ns`, extrapolating at the nominal
    /// `sample_rate`. Times before the anchor clamp to frame arithmetic that
    /// never goes below zero.
    pub fn frame_at(&self, host_ns: u64, sample_rate: u32) -> u64 {
        let delta_ns = host_ns as i128 - self.host_ns as i128;
        let delta_frames = delta_ns * sample_rate as i128 / 1_000_000_000;
        (self.frame as i128 + delta_frames).max(0) as u64
    }

    /// Output frame (fractional) at which input position `input` is delivered
    /// to the looper, valid while the epoch is unchanged.
    pub fn output_frame_for_input(&self, input: f64) -> f64 {
        let ratio = if self.ratio > 0.0 { self.ratio } else { 1.0 };
        self.frame as f64 + (input - self.input_pos) / ratio
    }

    /// Transport frame (fractional) at which input position `input` reaches the
    /// looper. Subtract the calibrated input latency to get the frame at which
    /// it was performed. Only meaningful while the transport runs.
    pub fn transport_frame_for_input(&self, input: f64) -> f64 {
        self.transport_frame as f64 + (self.output_frame_for_input(input) - self.frame as f64)
    }

    /// Transport frame at `host_ns`: extrapolated while the transport runs,
    /// otherwise the anchored value.
    pub fn transport_frame_at(&self, host_ns: u64, sample_rate: u32) -> i64 {
        if !self.transport_running {
            return self.transport_frame;
        }
        let delta_ns = host_ns as i128 - self.host_ns as i128;
        self.transport_frame + (delta_ns * sample_rate as i128 / 1_000_000_000) as i64
    }
}

/// Add to a counter that has a single writer, saturating instead of wrapping.
/// Realtime-safe (two relaxed atomic operations, no read-modify-write loop).
#[inline]
pub fn add_saturating(counter: &AtomicU64, n: u64) {
    if n > 0 {
        let v = counter.load(Ordering::Relaxed);
        counter.store(v.saturating_add(n), Ordering::Relaxed);
    }
}

/// Stream error codes stored by CPAL error callbacks (formatted on the control thread).
pub const STREAM_OK: u32 = 0;
/// The device disappeared (unplugged, or taken by another app).
pub const STREAM_DEVICE_LOST: u32 = 1;
/// Any other backend error.
pub const STREAM_BACKEND_ERROR: u32 = 2;

/// Live facts about one stream, written by its callbacks.
#[derive(Debug, Default)]
pub struct StreamStats {
    pub anchor: ClockAnchor,
    /// Frames in the most recent callback (the buffer size actually delivered).
    pub block_frames: AtomicU32,
    /// Largest callback so far.
    pub max_block_frames: AtomicU32,
    /// Latency the backend reported for the most recent callback, in nanoseconds
    /// (capture→callback for input, callback→playback for output).
    pub reported_latency_ns: AtomicU64,
    /// Callback timestamps that jumped further than the previous block could
    /// explain: the backend skipped time (an xrun). See [`XrunDetector`].
    pub discontinuities: AtomicU64,
    /// Frames of time lost in those discontinuities (estimated from timestamps).
    pub lost_frames: AtomicU64,
    /// Non-finite input samples replaced by silence (input stream only).
    pub non_finite_samples: AtomicU64,
    /// First error reported by the stream's error callback (`STREAM_*`).
    pub error: AtomicU32,
}

impl StreamStats {
    /// Record one callback. Realtime-safe.
    #[inline]
    pub fn on_block(&self, anchor: Anchor, frames: usize, latency_ns: Option<u64>) {
        self.anchor.write(anchor);
        let frames = frames.min(u32::MAX as usize) as u32;
        self.block_frames.store(frames, Ordering::Relaxed);
        if frames > self.max_block_frames.load(Ordering::Relaxed) {
            self.max_block_frames.store(frames, Ordering::Relaxed);
        }
        if let Some(ns) = latency_ns {
            self.reported_latency_ns.store(ns, Ordering::Relaxed);
        }
    }

    /// Record a discontinuity found by an [`XrunDetector`]. Realtime-safe.
    #[inline]
    pub fn on_discontinuity(&self, lost_frames: u64) {
        add_saturating(&self.discontinuities, 1);
        add_saturating(&self.lost_frames, lost_frames);
    }

    /// Called from a CPAL error callback. Stores a code only: no formatting,
    /// allocation, or channel send (CPAL may call this on the realtime thread).
    #[inline]
    pub fn on_error(&self, code: u32) {
        let _ = self.error.compare_exchange(STREAM_OK, code, Ordering::Relaxed, Ordering::Relaxed);
    }

    /// The stored error code, if any (`STREAM_*`).
    pub fn error_code(&self) -> Option<u32> {
        match self.error.load(Ordering::Relaxed) {
            STREAM_OK => None,
            code => Some(code),
        }
    }

    pub fn block(&self) -> Option<u32> {
        match self.block_frames.load(Ordering::Relaxed) {
            0 => None,
            n => Some(n),
        }
    }

    pub fn reported_latency_frames(&self, sample_rate: u32) -> Option<u32> {
        // No callback yet means no report.
        self.block()?;
        let ns = self.reported_latency_ns.load(Ordering::Relaxed);
        Some((ns as u128 * sample_rate as u128 / 1_000_000_000) as u32)
    }
}

/// Finds callbacks whose hardware timestamp moved further than the previous
/// block's duration explains, meaning the backend dropped time (an xrun or a
/// skipped callback). Pure and realtime-safe; one per stream.
///
/// The threshold is half a block plus 1 ms of tolerance for timestamp jitter,
/// so a skipped block (a gap of about two blocks) is always caught and normal
/// jitter is not.
#[derive(Debug, Clone, Copy, Default)]
pub struct XrunDetector {
    prev_ns: Option<u64>,
    prev_frames: u64,
}

pub const XRUN_TOLERANCE_NS: u64 = 1_000_000;
/// Gaps in the first half second are start-up irregularity (the backend
/// settling), not lost audio, and are not counted.
pub const XRUN_WARMUP_NS: u64 = 500_000_000;

impl XrunDetector {
    /// `callback_ns`: the callback's hardware timestamp in nanoseconds since
    /// the stream's first callback. Returns the number of frames of time lost, if any.
    #[inline]
    pub fn observe(&mut self, callback_ns: u64, frames: usize, sample_rate: u32) -> Option<u64> {
        let sr = sample_rate.max(1) as u64;
        let lost = match self.prev_ns {
            Some(prev) if callback_ns > prev && self.prev_frames > 0 && callback_ns >= XRUN_WARMUP_NS => {
                let elapsed = callback_ns - prev;
                let expected = self.prev_frames.saturating_mul(1_000_000_000) / sr;
                if elapsed > expected + expected / 2 + XRUN_TOLERANCE_NS {
                    Some(((elapsed - expected) as u128 * sr as u128 / 1_000_000_000) as u64)
                } else {
                    None
                }
            }
            _ => None,
        };
        self.prev_ns = Some(callback_ns);
        self.prev_frames = frames as u64;
        lost
    }
}

/// Estimates how fast the input device's clock runs relative to the output
/// device's, in parts per million, from their anchors. Runs on the control thread.
///
/// Each anchor is accurate to the callback's scheduling jitter (about a
/// millisecond), so the estimate's error is roughly `2 × jitter / window`:
/// about ±200 ppm after 10 s, ±30 ppm after a minute, ±7 ppm after five minutes.
#[derive(Debug, Clone, Default)]
pub struct DriftEstimator {
    first: Option<(Anchor, Anchor)>,
    last: Option<(Anchor, Anchor)>,
    started_ns: Option<u64>,
}

/// Anchors are ignored for this long after the streams start (start-up is irregular).
pub const DRIFT_WARMUP_NS: u64 = 2_000_000_000;
/// Minimum observation window before an estimate is reported.
pub const DRIFT_MIN_WINDOW_NS: u64 = 10_000_000_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriftEstimate {
    /// Positive: the input device delivers frames faster than the output consumes them.
    pub ppm: f64,
    pub window_seconds: f64,
}

impl DriftEstimator {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Feed the newest anchors of both streams.
    pub fn observe(&mut self, input: Anchor, output: Anchor) {
        let now = input.host_ns.max(output.host_ns);
        let started = *self.started_ns.get_or_insert(now);
        if now.saturating_sub(started) < DRIFT_WARMUP_NS {
            return;
        }
        if self.first.is_none() {
            self.first = Some((input, output));
        }
        self.last = Some((input, output));
    }

    pub fn estimate(&self) -> Option<DriftEstimate> {
        let ((i0, o0), (i1, o1)) = (self.first?, self.last?);
        let (dt_in, dt_out) = (i1.host_ns.checked_sub(i0.host_ns)?, o1.host_ns.checked_sub(o0.host_ns)?);
        if dt_in < DRIFT_MIN_WINDOW_NS || dt_out < DRIFT_MIN_WINDOW_NS {
            return None;
        }
        let (df_in, df_out) = (i1.frame.checked_sub(i0.frame)?, o1.frame.checked_sub(o0.frame)?);
        if df_in == 0 || df_out == 0 {
            return None;
        }
        let rate_in = df_in as f64 / dt_in as f64;
        let rate_out = df_out as f64 / dt_out as f64;
        Some(DriftEstimate { ppm: (rate_in / rate_out - 1.0) * 1e6, window_seconds: dt_in.min(dt_out) as f64 / 1e9 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_round_trips_and_is_none_before_the_first_write() {
        let a = ClockAnchor::default();
        assert_eq!(a.read(), None);
        let v = Anchor {
            host_ns: 5,
            frame: 7,
            transport_frame: -3,
            transport_running: true,
            input_pos: 12.25,
            ratio: 1.0001,
            epoch: 2,
        };
        a.write(v);
        assert_eq!(a.read(), Some(v));
    }

    #[test]
    fn frame_extrapolation_uses_the_sample_rate_in_both_directions() {
        let a = Anchor {
            host_ns: 1_000_000_000,
            frame: 48_000,
            transport_frame: 100,
            transport_running: true,
            ..Anchor::default()
        };
        assert_eq!(a.frame_at(1_010_000_000, 48_000), 48_480);
        assert_eq!(a.frame_at(990_000_000, 48_000), 47_520);
        assert_eq!(a.frame_at(0, 48_000), 0);
        assert_eq!(a.transport_frame_at(1_010_000_000, 48_000), 580);
        let stopped = Anchor { transport_running: false, ..a };
        assert_eq!(stopped.transport_frame_at(1_010_000_000, 48_000), 100);
    }

    #[test]
    fn concurrent_reads_never_see_a_torn_anchor() {
        let anchor = std::sync::Arc::new(ClockAnchor::default());
        let writer = {
            let anchor = anchor.clone();
            std::thread::spawn(move || {
                for i in 1..200_000u64 {
                    anchor.write(Anchor {
                        host_ns: i,
                        frame: i * 2,
                        transport_frame: i as i64 * 3,
                        ..Anchor::default()
                    });
                }
            })
        };
        while !writer.is_finished() {
            if let Some(a) = anchor.read() {
                assert_eq!(a.frame, a.host_ns * 2);
                assert_eq!(a.transport_frame, a.host_ns as i64 * 3);
            }
        }
        writer.join().unwrap();
    }

    #[test]
    fn drift_estimate_recovers_a_known_offset_and_needs_a_window() {
        let mut d = DriftEstimator::default();
        // Input runs 100 ppm fast relative to the output; callbacks every 10 ms.
        let at = |ms: u64| {
            let ns = ms * 1_000_000;
            let out = Anchor { host_ns: ns, frame: ms * 48, ..Anchor::default() };
            let inp = Anchor { host_ns: ns, frame: (ms as f64 * 48.0 * 1.0001) as u64, ..Anchor::default() };
            (inp, out)
        };
        for ms in (0..8_000).step_by(10) {
            let (i, o) = at(ms);
            d.observe(i, o);
        }
        assert_eq!(d.estimate(), None, "under ten seconds of data");
        for ms in (8_000..64_000).step_by(10) {
            let (i, o) = at(ms);
            d.observe(i, o);
        }
        let e = d.estimate().unwrap();
        assert!((e.ppm - 100.0).abs() < 1.0, "{e:?}");
        assert!(e.window_seconds > 60.0);
    }

    #[test]
    fn xrun_detector_flags_skipped_blocks_but_not_jitter() {
        let mut d = XrunDetector::default();
        let block_ns = 256 * 1_000_000_000 / 48_000; // 5.33 ms
        assert_eq!(d.observe(0, 256, 48_000), None);
        assert_eq!(d.observe(4 * block_ns, 256, 48_000), None, "start-up gaps are not xruns");
        let mut d = XrunDetector::default();
        let t0 = XRUN_WARMUP_NS;
        let d_obs = |d: &mut XrunDetector, t: u64| d.observe(t0 + t, 256, 48_000);
        assert_eq!(d_obs(&mut d, 0), None);
        assert_eq!(d_obs(&mut d, block_ns + 400_000), None, "0.4 ms of jitter");
        assert_eq!(d_obs(&mut d, 2 * block_ns), None, "early callback");
        // A whole block skipped.
        let lost = d_obs(&mut d, 4 * block_ns).expect("a skipped block is a discontinuity");
        assert!((255..=257).contains(&lost), "{lost}");
        // Timestamps that go backwards are ignored rather than counted.
        assert_eq!(d_obs(&mut d, block_ns), None);
    }

    #[test]
    fn counters_saturate_and_errors_keep_the_first_code() {
        let c = AtomicU64::new(u64::MAX - 1);
        add_saturating(&c, 5);
        assert_eq!(c.load(Ordering::Relaxed), u64::MAX);
        let s = StreamStats::default();
        assert_eq!(s.error_code(), None);
        s.on_error(STREAM_DEVICE_LOST);
        s.on_error(STREAM_BACKEND_ERROR);
        assert_eq!(s.error_code(), Some(STREAM_DEVICE_LOST));
        s.on_discontinuity(100);
        assert_eq!((s.discontinuities.load(Ordering::Relaxed), s.lost_frames.load(Ordering::Relaxed)), (1, 100));
    }

    #[test]
    fn stream_stats_track_block_size_and_latency() {
        let s = StreamStats::default();
        assert_eq!(s.block(), None);
        assert_eq!(s.reported_latency_frames(48_000), None);
        s.on_block(Anchor::default(), 256, Some(10_000_000));
        s.on_block(Anchor::default(), 128, None);
        assert_eq!(s.block(), Some(128));
        assert_eq!(s.max_block_frames.load(Ordering::Relaxed), 256);
        assert_eq!(s.reported_latency_frames(48_000), Some(480));
    }
}
