//! Input→output clock bridge with adaptive-resampling drift correction.
//!
//! The input and output devices run separate callbacks, often on separate
//! hardware clocks (two crystals differ by tens of ppm). Input frames reach
//! the output callback through an SPSC ring; [`InputBridge`] turns them into
//! exactly one input value per output frame. It is pure logic over a ring, so
//! it is unit-tested without audio hardware (including multi-minute
//! simulations of mismatched clocks below).
//!
//! - **Cushion / priming.** Nothing is consumed until `cushion` frames are
//!   buffered. The cushion absorbs callback jitter and block-size mismatch.
//! - **Target fill.** For the first second after priming the bridge passes
//!   samples through 1:1 and measures the mean ring fill; that mean becomes
//!   the target. The bridge's delay is therefore constant (≈ the target fill)
//!   and is part of the round trip that loopback calibration measures.
//! - **Fill measure.** When the output callback can see the input stream's
//!   clock anchor it passes "input frames captured by now"
//!   ([`InputBridge::set_input_clock`]); the fill is then that minus the read
//!   position, a time-continuous quantity free of the burst sawtooth (whose
//!   slowly sliding phase between two clocks would otherwise look like fill
//!   steps of a whole block). Without an anchor the ring's slot count is used.
//! - **Drift.** A PI controller on the (smoothed) fill error sets the read
//!   ratio `1 + ppm·1e-6` (bounded to ±[`MAX_PPM`], slew-limited to
//!   [`SLEW_PPM_PER_SECOND`]). A 16-tap windowed-sinc fractional-delay reader
//!   (precomputed polyphase table, [`PHASES`] phases, linearly interpolated)
//!   reads the ring at that ratio. Nothing is dropped or repeated: the fill,
//!   and thus the input→output latency, stays constant. At ratio exactly 1 and
//!   phase 0 the reader is a bit-exact copy.
//! - **Discontinuities** (only when concealment is impossible): an underrun
//!   (ring ran dry: silence, cushion grows by half, re-prime) or a backlog
//!   above 8 × cushion (skipped). Each one is counted, increments the
//!   mapping `epoch`, and restarts target measurement.
//!
//! Realtime-safe: the table and history are allocated in [`InputBridge::new`];
//! `fill` does no allocation, locking, or I/O.

use std::sync::atomic::{AtomicU64, Ordering};

use rtrb::Consumer;
use serde::Serialize;

/// Upper bound for the adaptive cushion.
pub const MAX_CUSHION_SECONDS: f32 = 0.1;
/// Half the interpolation filter length (taps = 2 × this).
pub const HALF_TAPS: usize = 8;
const TAPS: usize = 2 * HALF_TAPS;
/// Fractional-delay phases in the coefficient table.
pub const PHASES: usize = 256;
/// Time constant of the fill smoothing, seconds.
pub const FILL_TAU_SECONDS: f64 = 0.5;
/// Pass-through period after priming used to measure the target fill, seconds.
pub const MEASURE_SECONDS: f64 = 1.0;
/// Proportional gain, ppm per frame of fill error.
pub const KP: f64 = 2.0;
/// Integral gain, ppm per (frame of error × second).
pub const KI: f64 = 0.05;
/// Ratio bound (1000 ppm is 1.7 cents: inaudible).
pub const MAX_PPM: f64 = 1_000.0;
/// Fastest change of the ratio.
pub const SLEW_PPM_PER_SECOND: f64 = 200.0;

/// What one [`InputBridge::fill`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BridgeReport {
    /// Frames of the block that were real input (the rest is silence).
    pub delivered: usize,
    /// Frames of silence substituted for missing input (not counted while first priming).
    pub underrun: usize,
    /// Buffered input frames skipped because the backlog exceeded 8 × cushion.
    pub dropped: usize,
    /// The input→output mapping broke in this block (underrun or skip).
    pub discontinuity: bool,
}

/// Bridge state published to the control thread (single writer: the output callback).
#[derive(Debug, Default)]
pub struct BridgeStats {
    ppm: AtomicU64,
    mean_fill: AtomicU64,
    target: AtomicU64,
    pub discontinuities: AtomicU64,
}

/// Snapshot of [`BridgeStats`] for status and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeInfo {
    /// Current read-ratio correction: input frames per output frame = 1 + ppm·1e-6.
    /// Converges to the input clock's offset from the output clock.
    pub ratio_ppm: f64,
    /// Mean ring fill in frames: the bridge's contribution to round-trip latency.
    pub mean_fill_frames: f64,
    /// Fill the controller holds (`None` while measuring after (re)priming).
    pub target_fill_frames: Option<f64>,
    /// Breaks in the input→output mapping (underruns, skips, input overruns).
    pub discontinuities: u64,
}

impl BridgeStats {
    pub fn info(&self) -> BridgeInfo {
        let target = f64::from_bits(self.target.load(Ordering::Relaxed));
        BridgeInfo {
            ratio_ppm: f64::from_bits(self.ppm.load(Ordering::Relaxed)),
            mean_fill_frames: f64::from_bits(self.mean_fill.load(Ordering::Relaxed)),
            target_fill_frames: (target > 0.0).then_some(target),
            discontinuities: self.discontinuities.load(Ordering::Relaxed),
        }
    }

    /// Publish from the output callback. Realtime-safe.
    #[inline]
    pub fn publish(&self, bridge: &InputBridge) {
        self.ppm.store(bridge.ppm.to_bits(), Ordering::Relaxed);
        self.mean_fill.store(bridge.fill_ema.to_bits(), Ordering::Relaxed);
        self.target.store(bridge.target.unwrap_or(0.0).to_bits(), Ordering::Relaxed);
        self.discontinuities.store(bridge.discontinuities, Ordering::Relaxed);
    }
}

/// Blackman-windowed sinc table: row `j` holds the taps for fraction `j / PHASES`,
/// tap `m` weights input `idx + m − (HALF_TAPS − 1)`. Rows sum to 1.
fn build_table() -> Box<[f32]> {
    let mut table = vec![0.0f32; (PHASES + 1) * TAPS];
    let l = HALF_TAPS as f64;
    for j in 0..=PHASES {
        let f = j as f64 / PHASES as f64;
        let row = &mut table[j * TAPS..(j + 1) * TAPS];
        let mut sum = 0.0f64;
        let mut c = [0.0f64; TAPS];
        for (m, cm) in c.iter_mut().enumerate() {
            let k = m as f64 - (l - 1.0);
            let t = f - k;
            let sinc = if t == 0.0 {
                1.0
            } else if f == 0.0 || f == 1.0 {
                0.0 // exactly zero at the other integers
            } else {
                (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
            };
            let x = t / l; // -1..1
            let w = 0.42 + 0.5 * (std::f64::consts::PI * x).cos() + 0.08 * (2.0 * std::f64::consts::PI * x).cos();
            *cm = sinc * w.max(0.0);
            sum += *cm;
        }
        for (r, cm) in row.iter_mut().zip(c) {
            *r = (cm / sum) as f32;
        }
    }
    table.into_boxed_slice()
}

#[derive(Debug, Clone)]
pub struct InputBridge {
    sample_rate: f64,
    cushion: usize,
    max_cushion: usize,
    primed: bool,
    /// False until the stream has primed once; start-up silence is not an underrun.
    started: bool,
    first_call: bool,
    // Resampler.
    table: Box<[f32]>,
    /// Last `HALF_TAPS` consumed input samples, oldest first.
    history: [f32; HALF_TAPS],
    /// Read position between ring head and the next sample, 0..1.
    frac: f64,
    ratio: f64,
    // Controller.
    ppm: f64,
    integral: f64,
    fill_ema: f64,
    target: Option<f64>,
    measure_sum: f64,
    measure_frames: f64,
    /// Input frames captured by now (from the input anchor), for this block.
    input_clock: Option<f64>,
    /// Whether the current target was measured with the input clock.
    clock_mode: bool,
    /// Lowest pre-block fill seen while measuring, and the largest block.
    measure_min: f64,
    max_block: usize,
    // Mapping.
    /// Input frames consumed from the ring, plus gaps reported by the input side.
    consumed: u64,
    epoch: u64,
    discontinuities: u64,
}

impl InputBridge {
    /// `ring_capacity` bounds the cushion so priming is always reachable.
    pub fn new(sample_rate: u32, ring_capacity: usize) -> Self {
        let sr = sample_rate.max(1) as usize;
        let reachable = (ring_capacity / 8).max(1);
        let max_cushion = ((sr as f32 * MAX_CUSHION_SECONDS) as usize).clamp(1, reachable);
        let cushion = (sr / 100).max(256).min(max_cushion);
        Self {
            sample_rate: sr as f64,
            cushion,
            max_cushion,
            primed: false,
            started: false,
            first_call: true,
            table: build_table(),
            history: [0.0; HALF_TAPS],
            frac: 0.0,
            ratio: 1.0,
            ppm: 0.0,
            integral: 0.0,
            fill_ema: 0.0,
            target: None,
            measure_sum: 0.0,
            measure_frames: 0.0,
            measure_min: f64::MAX,
            max_block: 0,
            input_clock: None,
            clock_mode: false,
            consumed: 0,
            epoch: 0,
            discontinuities: 0,
        }
    }

    /// Frames buffered before input is consumed (current, adaptive).
    pub fn cushion(&self) -> usize {
        self.cushion
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    /// Current correction in ppm (input frames per output frame = 1 + ppm·1e-6).
    pub fn ppm(&self) -> f64 {
        self.ppm
    }

    pub fn ratio(&self) -> f64 {
        self.ratio
    }

    /// Smoothed ring fill in frames (the bridge's delay).
    pub fn mean_fill(&self) -> f64 {
        self.fill_ema
    }

    pub fn target_fill(&self) -> Option<f64> {
        self.target
    }

    /// Input position the next output frame will deliver (input frames since
    /// the stream started, counting reported gaps).
    pub fn input_position(&self) -> f64 {
        self.consumed as f64 + self.frac
    }

    /// Increments at every discontinuity; anchors carry it.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn discontinuities(&self) -> u64 {
        self.discontinuities
    }

    /// Input frames captured by now according to the input stream's anchor
    /// (`anchor.frame + (now − anchor.host_ns)·rate`), or `None`. Call before
    /// each [`InputBridge::fill`]. Realtime-safe.
    pub fn set_input_clock(&mut self, captured: Option<f64>) {
        self.input_clock = captured;
    }

    /// The input side lost `frames` before they reached the ring (input ring
    /// overrun): keep positions in input-clock frames and mark a discontinuity.
    pub fn note_input_gap(&mut self, frames: u64) {
        if frames > 0 {
            self.consumed = self.consumed.saturating_add(frames);
            self.break_mapping();
        }
    }

    fn break_mapping(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.discontinuities = self.discontinuities.saturating_add(1);
        self.target = None;
        self.measure_sum = 0.0;
        self.measure_frames = 0.0;
        self.measure_min = f64::MAX;
    }

    /// Update the fill estimate and the read ratio once per block.
    fn control(&mut self, available: usize, n: usize) {
        let dt = n as f64 / self.sample_rate;
        let ring_fill = available as f64 - self.frac;
        if self.target.is_none() && self.measure_frames == 0.0 {
            // The measure is fixed for a whole target period.
            self.clock_mode = self.input_clock.is_some();
            self.fill_ema = self.measured_fill(ring_fill).unwrap_or(ring_fill);
        }
        let Some(fill) = self.measured_fill(ring_fill) else { return };
        let alpha = (dt / FILL_TAU_SECONDS).min(1.0);
        self.fill_ema += alpha * (fill - self.fill_ema);
        self.max_block = self.max_block.max(n);
        let Some(target) = self.target else {
            self.measure_sum += fill * n as f64;
            self.measure_frames += n as f64;
            // The safety floor is about real samples in the ring.
            self.measure_min = self.measure_min.min(ring_fill);
            if self.measure_frames >= MEASURE_SECONDS * self.sample_rate {
                // Hold the measured mean, raised if its low points would leave
                // less than one block plus the filter look-ahead buffered.
                let required = (self.max_block + HALF_TAPS + 2) as f64;
                let t = self.measure_sum / self.measure_frames + (required - self.measure_min).max(0.0);
                self.target = Some(t);
                self.fill_ema = self.measure_sum / self.measure_frames;
            }
            return;
        };
        let e = self.fill_ema - target;
        self.integral = (self.integral + KI * e * dt).clamp(-MAX_PPM, MAX_PPM);
        let want = (KP * e + self.integral).clamp(-MAX_PPM, MAX_PPM);
        let step = SLEW_PPM_PER_SECOND * dt;
        self.ppm += (want - self.ppm).clamp(-step, step);
        self.ratio = 1.0 + self.ppm * 1e-6;
    }

    fn measured_fill(&self, ring_fill: f64) -> Option<f64> {
        if self.clock_mode {
            self.input_clock.map(|c| c - self.input_position())
        } else {
            Some(ring_fill)
        }
    }

    /// Fill `dst` with the next input frames, or silence where none are due.
    pub fn fill(&mut self, rx: &mut Consumer<f32>, dst: &mut [f32]) -> BridgeReport {
        let n = dst.len();
        let mut report = BridgeReport::default();
        if self.first_call {
            // The cushion must at least cover one output block plus the filter's look-ahead.
            self.first_call = false;
            self.cushion = self.cushion.max(n + HALF_TAPS + 1).min(self.max_cushion);
        }
        let mut available = rx.slots();

        if !self.primed && available >= self.cushion {
            self.primed = true;
            self.started = true;
            self.fill_ema = available as f64;
        }
        if self.primed && available > self.cushion * 8 {
            let skip = available - self.cushion;
            if let Ok(chunk) = rx.read_chunk(skip) {
                let (a, b) = chunk.as_slices();
                self.remember(a, b);
                chunk.commit_all();
                report.dropped = skip;
                available -= skip;
                self.consumed = self.consumed.saturating_add(skip as u64);
                self.fill_ema = available as f64;
                report.discontinuity = true;
                self.break_mapping();
            }
        }
        if !self.primed {
            dst.fill(0.0);
            if self.started {
                report.underrun = n;
            }
            return report;
        }

        self.control(available, n);

        // Input needed for this block: n·ratio frames plus the filter look-ahead.
        let need = ((n as f64) * self.ratio + self.frac).ceil() as usize + HALF_TAPS + 1;
        let take = available.min(need);
        let mut idx = 0usize;
        if let Ok(chunk) = rx.read_chunk(take) {
            let (a, b) = chunk.as_slices();
            let history = self.history;
            let x = |j: isize| -> f32 {
                if j < 0 {
                    history[(HALF_TAPS as isize + j) as usize]
                } else {
                    let j = j as usize;
                    if j < a.len() {
                        a[j]
                    } else {
                        b[j - a.len()]
                    }
                }
            };
            for slot in dst.iter_mut() {
                let y = if self.frac == 0.0 {
                    if idx >= take {
                        break;
                    }
                    x(idx as isize)
                } else if idx + HALF_TAPS >= take {
                    // Too little look-ahead for the full filter (only while the
                    // fill settles): linear interpolation needs one sample ahead.
                    if idx + 1 >= take {
                        break;
                    }
                    let f = self.frac as f32;
                    x(idx as isize) * (1.0 - f) + x(idx as isize + 1) * f
                } else {
                    let pos = self.frac * PHASES as f64;
                    let j = (pos as usize).min(PHASES - 1);
                    let w = (pos - j as f64) as f32;
                    let r0 = &self.table[j * TAPS..(j + 1) * TAPS];
                    let r1 = &self.table[(j + 1) * TAPS..(j + 2) * TAPS];
                    let base = idx as isize - (HALF_TAPS as isize - 1);
                    let mut acc = 0.0f32;
                    for m in 0..TAPS {
                        acc += (r0[m] + w * (r1[m] - r0[m])) * x(base + m as isize);
                    }
                    acc
                };
                *slot = y;
                report.delivered += 1;
                self.frac += self.ratio;
                let adv = self.frac.floor();
                self.frac -= adv;
                idx += adv as usize;
            }
            // Keep the samples just before the new read position for the next block.
            let mut next = [0.0f32; HALF_TAPS];
            for (m, v) in next.iter_mut().enumerate() {
                *v = x(idx as isize - HALF_TAPS as isize + m as isize);
            }
            self.history = next;
            chunk.commit(idx);
            self.consumed = self.consumed.saturating_add(idx as u64);
        }
        dst[report.delivered..].fill(0.0);

        if self.started && report.delivered < n {
            report.underrun = n - report.delivered;
            if self.primed {
                // Fell off the cushion: rebuild it, larger, and re-measure the target.
                self.primed = false;
                self.frac = 0.0;
                self.cushion = (self.cushion + self.cushion / 2).clamp(1, self.max_cushion);
                report.discontinuity = true;
                self.break_mapping();
            }
        }
        report
    }

    /// History update for skipped samples (only the last `HALF_TAPS` matter).
    fn remember(&mut self, a: &[f32], b: &[f32]) {
        let total = a.len() + b.len();
        let mut next = self.history;
        for (m, slot) in next.iter_mut().enumerate() {
            // Sample `total - HALF_TAPS + m` of the skipped run, or older history.
            let j = total as isize - HALF_TAPS as isize + m as isize;
            *slot = if j < 0 {
                self.history[(HALF_TAPS as isize + j) as usize]
            } else if (j as usize) < a.len() {
                a[j as usize]
            } else {
                b[j as usize - a.len()]
            };
        }
        self.history = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::{Producer, RingBuffer};

    const SR: u32 = 48_000;
    const CAP: usize = 24_000;

    fn rig() -> (InputBridge, Producer<f32>, Consumer<f32>) {
        let (tx, rx) = RingBuffer::new(CAP);
        (InputBridge::new(SR, CAP), tx, rx)
    }

    /// Push `n` frames valued `start, start+1, ...` (exact in f32 below 2^24).
    fn push(tx: &mut Producer<f32>, start: usize, n: usize) {
        for i in 0..n {
            tx.push((start + i) as f32).expect("ring has space");
        }
    }

    #[test]
    fn waits_for_the_cushion_then_delivers_in_order() {
        let (mut bridge, mut tx, mut rx) = rig();
        assert_eq!(bridge.cushion(), 480);
        let mut block = [9.0f32; 128];

        push(&mut tx, 1, 300);
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport::default(), "start-up silence is not an underrun");
        assert!(block.iter().all(|&v| v == 0.0));
        assert_eq!(rx.slots(), 300, "nothing consumed while priming");

        push(&mut tx, 301, 200);
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport { delivered: 128, ..BridgeReport::default() });
        assert!(block.iter().enumerate().all(|(i, &v)| v == (i + 1) as f32), "bit-exact while the ratio is 1");
    }

    #[test]
    fn steady_flow_never_underruns_or_drops_and_stays_continuous() {
        // The input is a ramp that wraps every 2^14 frames (so f32 rounding in
        // the 16-tap sum stays far below one step); a dropped or repeated
        // sample would show up as a step of 0 or 2.
        const WRAP: usize = 1 << 14;
        let (mut bridge, mut tx, mut rx) = rig();
        let mut block = [0.0f32; 256];
        let mut pushed = 0usize;
        let (mut last, mut steps) = (None::<f32>, 0usize);
        // Input arrives in 441-frame bursts, output pulls 256-frame blocks:
        // mismatched sizes at the same average rate (441 * 256 == 256 * 441).
        let (mut in_due, mut out_due) = (0usize, 0usize);
        for tick in 0..441 * 256 * 4 {
            if tick >= in_due {
                for i in 0..441 {
                    tx.push(((pushed + i) % WRAP) as f32).unwrap();
                }
                pushed += 441;
                in_due += 441;
            }
            if tick >= out_due {
                let r = bridge.fill(&mut rx, &mut block);
                assert_eq!((r.underrun, r.dropped, r.discontinuity), (0, 0, false), "at tick {tick}");
                for &v in &block[..r.delivered] {
                    if let Some(prev) = last {
                        // Skip the wrap itself and the filter's ringing around it.
                        let edge = |x: f32| !(24.0..=(WRAP - 24) as f32).contains(&x);
                        let near_wrap = edge(prev) || edge(v);
                        if !near_wrap {
                            assert!((v - prev - 1.0).abs() < 0.05, "ramp continuity at tick {tick}: {prev} -> {v}");
                            steps += 1;
                        }
                    }
                    last = Some(v);
                }
                out_due += 256;
            }
        }
        assert!(steps > 390_000, "{steps}");
        assert!(bridge.ppm().abs() < 50.0, "equal clocks need no correction: {}", bridge.ppm());
    }

    #[test]
    fn underrun_pads_with_silence_counts_and_reprimes_with_a_larger_cushion() {
        let (mut bridge, mut tx, mut rx) = rig();
        let mut block = [0.0f32; 256];
        push(&mut tx, 1, 480 + 100);
        assert_eq!(bridge.fill(&mut rx, &mut block).delivered, 256);
        assert_eq!(bridge.fill(&mut rx, &mut block).delivered, 256);
        // 68 frames left: the next block is short.
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport { delivered: 68, underrun: 188, dropped: 0, discontinuity: true });
        assert_eq!(block[67], 580.0);
        assert!(block[68..].iter().all(|&v| v == 0.0));
        assert!(!bridge.is_primed());
        assert_eq!(bridge.cushion(), 720, "cushion grows by half");
        assert_eq!((bridge.epoch(), bridge.discontinuities()), (1, 1));

        // While re-priming, input is held back and the gap is still counted.
        push(&mut tx, 581, 500);
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport { delivered: 0, underrun: 256, dropped: 0, discontinuity: false });
        push(&mut tx, 1_081, 300);
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r.delivered, 256);
        assert_eq!(block[0], 581.0, "no input is lost across an underrun");
    }

    #[test]
    fn cushion_growth_is_capped() {
        let (mut bridge, mut tx, mut rx) = rig();
        let mut block = [0.0f32; 64];
        for round in 0..20 {
            push(&mut tx, round * 10_000, bridge.cushion());
            while bridge.fill(&mut rx, &mut block).underrun == 0 {}
        }
        // Capped by both MAX_CUSHION_SECONDS (4800) and ring capacity / 8 (3000).
        assert_eq!(bridge.cushion(), 3_000);
    }

    #[test]
    fn a_gross_backlog_is_trimmed_to_the_cushion_as_a_counted_discontinuity() {
        let (mut bridge, mut tx, mut rx) = rig();
        let mut block = [0.0f32; 128];
        push(&mut tx, 1, 480 * 8 + 1); // one frame over the high-water mark
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport { delivered: 128, underrun: 0, dropped: 480 * 7 + 1, discontinuity: true });
        assert_eq!(block[0], (480 * 7 + 2) as f32, "the newest cushion of input is kept");
        assert_eq!(rx.slots(), 480 - 128);
        assert_eq!(bridge.input_position(), (480 * 7 + 1 + 128) as f64);
    }

    #[test]
    fn interpolation_table_is_exact_at_integer_phases_and_has_unity_gain() {
        let table = build_table();
        let row = |j: usize| &table[j * TAPS..(j + 1) * TAPS];
        assert_eq!(row(0)[HALF_TAPS - 1], 1.0);
        assert_eq!(row(0).iter().filter(|&&c| c != 0.0).count(), 1);
        assert_eq!(row(PHASES)[HALF_TAPS], 1.0);
        for j in 0..=PHASES {
            assert!((row(j).iter().sum::<f32>() - 1.0).abs() < 1e-5);
        }
        // Half-sample row is symmetric.
        let h = row(PHASES / 2);
        for m in 0..TAPS {
            assert!((h[m] - h[TAPS - 1 - m]).abs() < 1e-6);
        }
    }

    #[test]
    fn input_gaps_advance_the_position_and_the_epoch() {
        let (mut bridge, _tx, _rx) = rig();
        bridge.note_input_gap(0);
        assert_eq!(bridge.epoch(), 0);
        bridge.note_input_gap(100);
        assert_eq!((bridge.input_position(), bridge.epoch(), bridge.discontinuities()), (100.0, 1, 1));
    }

    // ----- Multi-minute clock-mismatch simulations -----

    /// Impulses every `IMPULSE_PERIOD` input frames (index multiple of it).
    const IMPULSE_PERIOD: u64 = 4_801;

    struct Sim {
        discontinuities: u64,
        underrun: usize,
        dropped: usize,
        /// (seconds, fill) after convergence.
        fill_range: (f64, f64),
        target: f64,
        /// Mean correction over the last minute.
        final_ppm: f64,
        /// Peak-to-peak of (output time − capture time) of impulses after convergence, frames.
        latency_p2p: f64,
        /// Largest |measured − predicted| impulse output frame using block-start mappings.
        max_prediction_error: f64,
        impulses_checked: usize,
    }

    /// Input clock runs at `SR·(1 + ppm·1e-6)` and delivers `in_block`-frame
    /// bursts; the output pulls `out_block` frames at exactly `SR`.
    /// With `clock`, the output side sees the input stream's anchor (host time
    /// of each input callback, with up to 0.2 ms of deterministic jitter), as
    /// `looper/io.rs` does; without it the bridge measures ring slots.
    fn simulate(ppm: f64, seconds: f64, in_block: usize, out_block: usize, settle: f64, clock: bool) -> Sim {
        let (mut tx, mut rx) = RingBuffer::<f32>::new(CAP);
        let mut bridge = InputBridge::new(SR, CAP);
        let in_rate = SR as f64 * (1.0 + ppm * 1e-6);
        let sr = SR as f64;
        let mut pushed: u64 = 0;
        let mut out_frames: u64 = 0;
        let mut block = vec![0.0f32; out_block];
        let mut sim = Sim {
            discontinuities: 0,
            underrun: 0,
            dropped: 0,
            fill_range: (f64::MAX, f64::MIN),
            target: 0.0,
            final_ppm: 0.0,
            latency_p2p: 0.0,
            max_prediction_error: 0.0,
            impulses_checked: 0,
        };
        let (mut lat_min, mut lat_max) = (f64::MAX, f64::MIN);
        // Pending impulse search: (input index, predicted output frame).
        let mut window: Vec<(u64, f32)> = Vec::new(); // (output frame, |value|) near a peak
        let mut predictions: std::collections::VecDeque<(u64, f64)> = std::collections::VecDeque::new();
        let mut next_impulse_predicted: u64 = 0;
        let total_out = (seconds * sr) as u64;
        let (mut ppm_sum, mut ppm_n) = (0.0f64, 0.0f64);
        let mut anchor = (0.0f64, 0u64); // (host seconds, frames before that callback)
        let mut seed = 12_345u32;
        while out_frames < total_out {
            let t_out = out_frames as f64 / sr;
            // Deliver every input burst whose last frame was captured by now.
            while ((pushed + in_block as u64) as f64) / in_rate <= t_out {
                for i in 0..in_block as u64 {
                    let idx = pushed + i;
                    let v = if idx.is_multiple_of(IMPULSE_PERIOD) { 1.0 } else { 0.0 };
                    tx.push(v).expect("ring has space");
                }
                // The input callback runs once its block is captured, a little late.
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let jitter = (seed >> 8) as f64 / (1u64 << 24) as f64 * 0.000_2;
                anchor = (((pushed + in_block as u64) as f64) / in_rate + jitter, pushed);
                pushed += in_block as u64;
            }
            if clock && pushed > 0 {
                bridge.set_input_clock(Some(anchor.1 as f64 + (t_out - anchor.0) * sr));
            }
            let fill_before = rx.slots() as f64;
            // Predict where impulses land, from the block-start mapping (what the anchor publishes).
            let pos = bridge.input_position();
            let ratio = bridge.ratio();
            if bridge.is_primed() && t_out > settle {
                while next_impulse_predicted * IMPULSE_PERIOD < pos as u64 {
                    next_impulse_predicted += 1;
                }
                let i = next_impulse_predicted * IMPULSE_PERIOD;
                let predicted = out_frames as f64 + (i as f64 - pos) / ratio;
                if predicted < (out_frames + out_block as u64) as f64 + 0.5
                    && predictions.back().is_none_or(|p| p.0 != i)
                {
                    predictions.push_back((i, predicted));
                }
            }
            let r = bridge.fill(&mut rx, &mut block);
            if t_out > seconds - 60.0 {
                ppm_sum += bridge.ppm();
                ppm_n += 1.0;
            }
            sim.underrun += r.underrun;
            sim.dropped += r.dropped;
            if t_out > settle {
                sim.fill_range.0 = sim.fill_range.0.min(fill_before);
                sim.fill_range.1 = sim.fill_range.1.max(fill_before);
            }
            // Impulse detection: energy centroid of each isolated peak.
            for (k, &v) in block.iter().enumerate() {
                let f = out_frames + k as u64;
                if v.abs() > 0.02 {
                    window.push((f, v.abs()));
                } else if !window.is_empty() && f > window.last().unwrap().0 + HALF_TAPS as u64 {
                    let peak = window.iter().map(|w| w.1).fold(0.0f32, f32::max);
                    if peak > 0.5 {
                        let e: f64 = window.iter().map(|w| (w.1 as f64).powi(2)).sum();
                        let c: f64 = window.iter().map(|w| w.0 as f64 * (w.1 as f64).powi(2)).sum::<f64>() / e;
                        if t_out > settle {
                            // Which impulse? The prediction nearest this centroid.
                            if let Some(p) = predictions.iter().position(|p| (p.1 - c).abs() < 50.0) {
                                let (i, predicted) = predictions[p];
                                sim.max_prediction_error = sim.max_prediction_error.max((predicted - c).abs());
                                sim.impulses_checked += 1;
                                let capture_time_frames = i as f64 / in_rate * sr;
                                let latency = c - capture_time_frames;
                                lat_min = lat_min.min(latency);
                                lat_max = lat_max.max(latency);
                                predictions.drain(..=p);
                            }
                        }
                    }
                    window.clear();
                }
            }
            out_frames += out_block as u64;
        }
        sim.discontinuities = bridge.discontinuities();
        sim.target = bridge.target_fill().unwrap_or(0.0);
        sim.final_ppm = ppm_sum / ppm_n.max(1.0);
        sim.latency_p2p = lat_max - lat_min;
        sim
    }

    fn check(ppm: f64, in_block: usize, out_block: usize) {
        check_with(ppm, in_block, out_block, true);
    }

    fn check_with(ppm: f64, in_block: usize, out_block: usize, clock: bool) {
        let minutes = 6.0;
        let s = simulate(ppm, minutes * 60.0, in_block, out_block, 120.0, clock);
        let label = format!("{ppm:+} ppm, in {in_block} / out {out_block}, input clock {clock}");
        assert_eq!((s.discontinuities, s.underrun, s.dropped), (0, 0, 0), "{label}: no discontinuities");
        assert!((s.final_ppm - ppm).abs() < 3.0, "{label}: ratio converges to the clock offset, got {}", s.final_ppm);
        // The ring holds a steady amount: its range is the burst sawtooth, with no growth.
        let span = (in_block + out_block + 40) as f64;
        assert!(
            s.fill_range.1 - s.fill_range.0 < span && s.fill_range.0 >= out_block as f64,
            "{label}: ring fill {:?} bounded (target {})",
            s.fill_range,
            s.target
        );
        assert!(s.impulses_checked > 2_000, "{label}: {} impulses", s.impulses_checked);
        // < 4 frames (0.08 ms) peak-to-peak over minutes; the residue is the
        // controller following the slowly sliding burst phase of the two clocks.
        assert!(s.latency_p2p < 4.0, "{label}: input→output latency stable, varied by {} frames", s.latency_p2p);
        assert!(
            s.max_prediction_error < 1.0,
            "{label}: anchor mapping predicts impulses, off by {}",
            s.max_prediction_error
        );
    }

    #[test]
    fn plus_50_ppm_input_runs_for_minutes_without_discontinuity() {
        check(50.0, 441, 256);
    }

    #[test]
    fn minus_50_ppm_input_runs_for_minutes_without_discontinuity() {
        check(-50.0, 512, 512);
    }

    #[test]
    fn plus_200_ppm_input_runs_for_minutes_without_discontinuity() {
        check(200.0, 256, 441);
    }

    #[test]
    fn minus_200_ppm_input_runs_for_minutes_without_discontinuity() {
        check(-200.0, 441, 128);
    }

    #[test]
    fn ring_measured_fill_also_tracks_drift_when_no_input_anchor_is_available() {
        check_with(50.0, 441, 256, false);
    }

    #[test]
    fn impulses_keep_their_exact_spacing_through_a_drifting_bridge() {
        // Equal clocks, no bursts mismatch: impulses arrive exactly IMPULSE_PERIOD apart.
        let s = simulate(0.0, 30.0, 256, 256, 2.0, false);
        assert_eq!(s.discontinuities, 0);
        assert!(s.latency_p2p < 0.01, "{}", s.latency_p2p);
        assert!(s.max_prediction_error < 0.01, "{}", s.max_prediction_error);
    }
}
