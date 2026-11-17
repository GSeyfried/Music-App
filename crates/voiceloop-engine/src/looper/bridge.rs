//! Input→output clock bridge.
//!
//! The input and output devices run separate callbacks, often on separate
//! hardware clocks. Input frames reach the output callback through an SPSC
//! ring; [`InputBridge`] decides, block by block, which frames the looper
//! consumes. It is pure logic over a ring, so it is unit-tested without
//! audio hardware.
//!
//! - **Cushion.** Nothing is consumed until `cushion` frames are buffered
//!   ("priming"). The cushion absorbs callback jitter and is part of the
//!   round-trip latency the user compensates for.
//! - **Underrun.** If a block needs more frames than are buffered, the
//!   missing frames are silence and are counted. The bridge then re-primes:
//!   without that, the buffer would hover near empty and underrun again on
//!   every bit of jitter, putting a gap into the recording each time. The
//!   cushion also grows by half (up to [`MAX_CUSHION_SECONDS`]) because the
//!   underrun proved it was too small for this device pair.
//! - **Drift.** If the backlog exceeds `8 × cushion` (input clock faster than
//!   output), the excess is skipped and counted, restoring the cushion.
//!
//! Each underrun or drift correction shifts input timing by a few
//! milliseconds; both are reported so the UI can show that the session
//! is no longer sample-tight. Resampling drift correction is future work.
//!
//! Realtime-safe: no allocation, locks, or I/O.

use rtrb::Consumer;

/// Upper bound for the adaptive cushion.
pub const MAX_CUSHION_SECONDS: f32 = 0.1;

/// What one [`InputBridge::fill`] call did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BridgeReport {
    /// Frames of the block that were real input (the rest is silence).
    pub delivered: usize,
    /// Frames of silence substituted for missing input (not counted while first priming).
    pub underrun: usize,
    /// Buffered input frames skipped to correct drift.
    pub dropped: usize,
}

#[derive(Debug, Clone)]
pub struct InputBridge {
    cushion: usize,
    max_cushion: usize,
    primed: bool,
    /// False until the stream has primed once; start-up silence is not an underrun.
    started: bool,
}

impl InputBridge {
    /// `ring_capacity` bounds the cushion so priming is always reachable.
    pub fn new(sample_rate: u32, ring_capacity: usize) -> Self {
        let sr = sample_rate.max(1) as usize;
        let reachable = (ring_capacity / 8).max(1);
        let max_cushion = ((sr as f32 * MAX_CUSHION_SECONDS) as usize).clamp(1, reachable);
        let cushion = (sr / 100).max(256).min(max_cushion);
        Self { cushion, max_cushion, primed: false, started: false }
    }

    /// Frames buffered before input is consumed (current, adaptive).
    pub fn cushion(&self) -> usize {
        self.cushion
    }

    pub fn is_primed(&self) -> bool {
        self.primed
    }

    /// Fill `dst` with the next input frames, or silence where none are due.
    pub fn fill(&mut self, rx: &mut Consumer<f32>, dst: &mut [f32]) -> BridgeReport {
        let n = dst.len();
        let mut report = BridgeReport::default();
        let mut available = rx.slots();

        if !self.primed && available >= self.cushion {
            self.primed = true;
            self.started = true;
        }
        if self.primed && available > self.cushion * 8 {
            let skip = available - self.cushion;
            if let Ok(chunk) = rx.read_chunk(skip) {
                chunk.commit_all();
                report.dropped = skip;
                available -= skip;
            }
        }

        let take = if self.primed { available.min(n) } else { 0 };
        if take > 0 {
            match rx.read_chunk(take) {
                Ok(chunk) => {
                    let (a, b) = chunk.as_slices();
                    dst[..a.len()].copy_from_slice(a);
                    dst[a.len()..a.len() + b.len()].copy_from_slice(b);
                    chunk.commit_all();
                    report.delivered = take;
                }
                // Unreachable: `take <= slots()` and this is the only consumer.
                Err(_) => {}
            }
        }
        dst[report.delivered..].fill(0.0);

        if self.started && report.delivered < n {
            report.underrun = n - report.delivered;
            if self.primed {
                // Fell off the cushion: rebuild it, larger.
                self.primed = false;
                self.cushion = (self.cushion + self.cushion / 2).clamp(1, self.max_cushion);
            }
        }
        report
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
        assert_eq!(r, BridgeReport { delivered: 128, underrun: 0, dropped: 0 });
        assert!(block.iter().enumerate().all(|(i, &v)| v == (i + 1) as f32));
    }

    #[test]
    fn steady_flow_never_underruns_or_drops() {
        let (mut bridge, mut tx, mut rx) = rig();
        let mut block = [0.0f32; 256];
        let mut pushed = 0;
        let mut next_expected = 1.0f32;
        // Input arrives in 441-frame bursts, output pulls 256-frame blocks:
        // mismatched sizes at the same average rate (441 * 256 == 256 * 441).
        let (mut in_due, mut out_due) = (0usize, 0usize);
        for tick in 0..441 * 256 * 4 {
            if tick >= in_due {
                push(&mut tx, pushed + 1, 441);
                pushed += 441;
                in_due += 441;
            }
            if tick >= out_due {
                let r = bridge.fill(&mut rx, &mut block);
                assert_eq!((r.underrun, r.dropped), (0, 0), "at tick {tick}");
                for &v in &block[..r.delivered] {
                    assert_eq!(v, next_expected);
                    next_expected += 1.0;
                }
                out_due += 256;
            }
        }
        assert!(next_expected > 400_000.0);
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
        assert_eq!(r, BridgeReport { delivered: 68, underrun: 188, dropped: 0 });
        assert_eq!(block[67], 580.0);
        assert!(block[68..].iter().all(|&v| v == 0.0));
        assert!(!bridge.is_primed());
        assert_eq!(bridge.cushion(), 720, "cushion grows by half");

        // While re-priming, input is held back and the gap is still counted.
        push(&mut tx, 581, 500);
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport { delivered: 0, underrun: 256, dropped: 0 });
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
    fn drift_backlog_is_trimmed_to_the_cushion() {
        let (mut bridge, mut tx, mut rx) = rig();
        let mut block = [0.0f32; 128];
        push(&mut tx, 1, 480 * 8 + 1); // one frame over the high-water mark
        let r = bridge.fill(&mut rx, &mut block);
        assert_eq!(r, BridgeReport { delivered: 128, underrun: 0, dropped: 480 * 7 + 1 });
        assert_eq!(block[0], (480 * 7 + 2) as f32, "the newest cushion of input is kept");
        assert_eq!(rx.slots(), 480 - 128);
    }
}
