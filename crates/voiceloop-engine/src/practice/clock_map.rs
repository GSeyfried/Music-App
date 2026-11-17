//! Input-clock → transport-clock mapping for Practice Lab.
//!
//! Analysis timestamps are input-device frames; the transport (click, loops)
//! runs on output-device frames. Each audio callback publishes an anchor
//! ([`crate::clock::Anchor`]: "at host time `h`, my frame counter was `f`";
//! the output anchor also carries the transport frame). The control thread
//! reads the newest input and output anchors about 30 times a second and
//! feeds the pair to [`ClockMapper::observe`].
//!
//! **Model.** At the output anchor's host time `h_o` the input clock stood at
//! `x = f_i + (h_o − h_i)·sr_in` and the transport at `y = t_o`. Within one
//! *segment* (a stretch with no discontinuity) the points satisfy
//! `y = y0 + k·(x − x0) + c + d·(x − x0)`, where `k = sr_out / sr_in` is the
//! nominal rate ratio, `c` absorbs the constant offset and `d` the clock
//! drift. `c` is the mean residual (the callback scheduling jitter of each
//! point averages out); `d` is fitted by least squares only once a segment
//! spans 10 s (before that, drift of ≤100 ppm contributes < 1 ms), and is
//! clamped to ±1000 ppm.
//!
//! This is a *wall-clock* correspondence: it maps "when this was sung" to
//! "which transport frame was being produced then". It does not go through
//! the input→looper ring, so bridge underruns, drift drops, or resampling
//! there do not affect it (RT audit finding #1 breaks ring-based mappings,
//! not this one). Physical latency is subtracted separately: the caller
//! takes `map(x) − (input latency + output latency)` as the transport frame
//! the performer *heard* while producing input frame `x`.
//!
//! **Re-anchoring.** A new segment starts when
//! - either stream's discontinuity counter moves (xrun, lost hardware time,
//!   a bridge discontinuity published through the anchors),
//! - the transport did not advance exactly as many frames as the output
//!   clock between two running observations (stop, restart, pause, jump),
//! - the transport stops running (no mapping while stopped), or
//! - two consecutive points disagree with the fit by more than the outlier
//!   tolerance but agree with each other (an undetected jump). A single
//!   disagreeing point is a late callback and is discarded.
//!
//! Frames mapped through a closed segment stay valid for the time that
//! segment covered; a span that crosses segments is reported as
//! discontinuous, never silently mapped.
//!
//! Pure arithmetic over values: deterministic and unit-tested without audio.

use std::collections::VecDeque;

use serde::Serialize;

use crate::clock::Anchor;

/// One reading of both streams' newest anchors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockSample {
    pub input: Anchor,
    pub output: Anchor,
    pub input_rate: u32,
    pub output_rate: u32,
    /// Cumulative discontinuity counters of the two streams.
    pub input_discontinuities: u64,
    pub output_discontinuities: u64,
}

/// Points needed before a segment maps anything.
pub const MIN_POINTS: u32 = 3;
/// Disagreement with the fit (beyond 4 sigma) that marks a point as an outlier, ms.
pub const OUTLIER_MS: f64 = 4.0;
/// Span after which drift is fitted, seconds.
pub const SLOPE_MIN_SPAN_SECONDS: f64 = 10.0;
pub const MAX_DRIFT_PPM: f64 = 1_000.0;
/// Closed segments kept for mapping older frames.
pub const MAX_SEGMENTS: usize = 64;
/// How far the current segment may be extrapolated (count-in pre-roll, analysis lag), seconds.
pub const EXTRAPOLATE_SECONDS: f64 = 5.0;
/// How far a closed segment may be extrapolated past its observed span, seconds.
pub const CLOSED_MARGIN_SECONDS: f64 = 0.1;

/// Why a segment ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Break {
    TransportStopped,
    TransportJump,
    StreamDiscontinuity,
    OffsetJump,
}

/// What one observation did (for tests and diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observed {
    NotRunning,
    Added,
    Started,
    Outlier,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub id: u32,
    k: f64,
    x0: f64,
    y0: f64,
    n: f64,
    su: f64,
    sv: f64,
    suu: f64,
    suv: f64,
    svv: f64,
    pub first_input: f64,
    pub last_input: f64,
    input_rate: u32,
    pub ended: Option<Break>,
}

impl Segment {
    fn new(id: u32, x: f64, y: f64, k: f64, input_rate: u32) -> Self {
        let mut s = Self {
            id,
            k,
            x0: x,
            y0: y,
            n: 0.0,
            su: 0.0,
            sv: 0.0,
            suu: 0.0,
            suv: 0.0,
            svv: 0.0,
            first_input: x,
            last_input: x,
            input_rate: input_rate.max(1),
            ended: None,
        };
        s.add(x, y);
        s
    }

    fn add(&mut self, x: f64, y: f64) {
        let u = x - self.x0;
        let v = y - self.y0 - self.k * u;
        self.n += 1.0;
        self.su += u;
        self.sv += v;
        self.suu += u * u;
        self.suv += u * v;
        self.svv += v * v;
        self.first_input = self.first_input.min(x);
        self.last_input = self.last_input.max(x);
    }

    pub fn points(&self) -> u32 {
        self.n as u32
    }

    pub fn span_seconds(&self) -> f64 {
        (self.last_input - self.first_input) / self.input_rate as f64
    }

    /// (offset c, drift d) of `v = c + d·u`.
    fn fit(&self) -> (f64, f64) {
        let n = self.n.max(1.0);
        let (mu, mv) = (self.su / n, self.sv / n);
        let var_u = self.suu / n - mu * mu;
        if self.span_seconds() >= SLOPE_MIN_SPAN_SECONDS && var_u > 0.0 {
            let d =
                ((self.suv / n - mu * mv) / var_u).clamp(-MAX_DRIFT_PPM * 1e-6 * self.k, MAX_DRIFT_PPM * 1e-6 * self.k);
            (mv - d * mu, d)
        } else {
            (mv, 0.0)
        }
    }

    /// Transport frames per input frame.
    pub fn ratio(&self) -> f64 {
        self.k + self.fit().1
    }

    /// Transport frame at input frame `x` (callback-time correspondence, no latency).
    pub fn map(&self, x: f64) -> f64 {
        let (c, d) = self.fit();
        let u = x - self.x0;
        self.y0 + self.k * u + c + d * u
    }

    /// Input frame at transport frame `y`.
    pub fn inverse(&self, y: f64) -> f64 {
        let (c, d) = self.fit();
        self.x0 + (y - self.y0 - c) / (self.k + d)
    }

    /// Standard deviation of the points about the fit, in transport frames.
    fn residual_std(&self) -> f64 {
        let n = self.n.max(1.0);
        let (c, d) = self.fit();
        // E[(v - c - d u)^2] from the running sums.
        let e = self.svv / n - 2.0 * c * self.sv / n - 2.0 * d * self.suv / n
            + c * c
            + 2.0 * c * d * self.su / n
            + d * d * self.suu / n;
        e.max(0.0).sqrt()
    }

    /// One-sigma uncertainty of a mapped frame, ms: the standard error of the
    /// fitted offset (residual std / √n), floored at 0.05 ms.
    pub fn uncertainty_ms(&self) -> f64 {
        let rate = self.input_rate as f64 * self.k;
        (self.residual_std() / self.n.max(1.0).sqrt() * 1_000.0 / rate).max(0.05)
    }

    fn ready(&self) -> bool {
        self.n >= MIN_POINTS as f64
    }

    /// Distance (input frames) from `x` to the span this segment may map, or
    /// `None` if it may not map `x`. A segment may extrapolate backwards (a
    /// pre-roll, a count-in) by up to `EXTRAPOLATE_SECONDS` but never over its
    /// predecessor's span (`floor`), and forwards past its last point only by
    /// `CLOSED_MARGIN_SECONDS` once it has ended (that time lies after a
    /// break), or `EXTRAPOLATE_SECONDS` while it is still open.
    fn reach(&self, x: f64, open: bool, floor: Option<f64>) -> Option<f64> {
        let rate = self.input_rate as f64;
        let lo = (self.first_input - EXTRAPOLATE_SECONDS * rate).max(floor.unwrap_or(f64::NEG_INFINITY));
        let hi = self.last_input + if open { EXTRAPOLATE_SECONDS } else { CLOSED_MARGIN_SECONDS } * rate;
        if !self.ready() || x < lo || x > hi {
            return None;
        }
        Some((self.first_input - x).max(x - self.last_input).max(0.0))
    }
}

/// A frame mapped onto the transport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mapped {
    pub transport_frame: f64,
    pub segment: u32,
    pub ratio: f64,
    pub uncertainty_ms: f64,
}

/// How an input span maps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpanMapping {
    /// One segment covers the span.
    Continuous(Segment),
    /// The span crosses a discontinuity.
    Discontinuous,
    /// Part of the span has no mapping (transport stopped, too few points).
    Unmapped,
}

#[derive(Debug, Default)]
pub struct ClockMapper {
    segments: VecDeque<Segment>,
    /// The last segment is still open.
    open: bool,
    last: Option<ClockSample>,
    pending_outlier: Option<(f64, f64)>,
    next_id: u32,
    /// Segments ended by something other than the transport stopping.
    pub breaks: u64,
    pub outliers: u64,
}

impl ClockMapper {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The open segment, once it has enough points to map.
    pub fn current(&self) -> Option<&Segment> {
        self.segments.back().filter(|s| self.open && s.ready())
    }

    fn close(&mut self, why: Break) {
        if self.open {
            if let Some(s) = self.segments.back_mut() {
                s.ended = Some(why);
            }
            if why != Break::TransportStopped {
                self.breaks += 1;
            }
        }
        self.open = false;
        self.pending_outlier = None;
    }

    fn start(&mut self, points: &[(f64, f64)], k: f64, input_rate: u32) {
        let mut seg = Segment::new(self.next_id, points[0].0, points[0].1, k, input_rate);
        for &(x, y) in &points[1..] {
            seg.add(x, y);
        }
        self.next_id += 1;
        self.segments.push_back(seg);
        while self.segments.len() > MAX_SEGMENTS {
            self.segments.pop_front();
        }
        self.open = true;
        self.pending_outlier = None;
    }

    pub fn observe(&mut self, s: ClockSample) -> Observed {
        let previous = self.last.replace(s);
        if !s.output.transport_running {
            self.close(Break::TransportStopped);
            return Observed::NotRunning;
        }
        let in_rate = s.input_rate.max(1);
        let k = s.output_rate.max(1) as f64 / in_rate as f64;
        let x = s.input.frame as f64 + (s.output.host_ns as f64 - s.input.host_ns as f64) * in_rate as f64 / 1e9;
        let y = s.output.transport_frame as f64;

        if let Some(p) = previous {
            if p.input_discontinuities != s.input_discontinuities
                || p.output_discontinuities != s.output_discontinuities
            {
                self.close(Break::StreamDiscontinuity);
            } else if p.output.transport_running {
                let dt = s.output.transport_frame - p.output.transport_frame;
                let df = s.output.frame as i64 - p.output.frame as i64;
                if dt != df {
                    self.close(Break::TransportJump);
                }
            }
        }
        if !self.open {
            self.start(&[(x, y)], k, in_rate);
            return Observed::Started;
        }
        let seg = *self.segments.back().expect("open segment exists");
        if seg.ready() {
            let rate_out = s.output_rate.max(1) as f64;
            let tol = (OUTLIER_MS * rate_out / 1_000.0).max(4.0 * seg.residual_std());
            if (y - seg.map(x)).abs() > tol {
                if let Some((px, py)) = self.pending_outlier {
                    if ((y - py) - k * (x - px)).abs() <= tol {
                        // Two points agree with each other, not with the fit: re-anchor.
                        self.close(Break::OffsetJump);
                        self.start(&[(px, py), (x, y)], k, in_rate);
                        return Observed::Started;
                    }
                }
                self.pending_outlier = Some((x, y));
                self.outliers += 1;
                return Observed::Outlier;
            }
        }
        self.pending_outlier = None;
        if let Some(seg) = self.segments.back_mut() {
            seg.add(x, y);
        }
        Observed::Added
    }

    /// The segment that may map `x`, preferring the one whose span is closest.
    fn segment_for(&self, x: f64) -> Option<(&Segment, bool)> {
        let last = self.segments.len().checked_sub(1)?;
        let mut best: Option<(f64, usize)> = None;
        for (i, s) in self.segments.iter().enumerate() {
            let floor = i.checked_sub(1).map(|p| self.segments[p].last_input);
            if let Some(d) = s.reach(x, self.open && i == last, floor) {
                if best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, i));
                }
            }
        }
        best.map(|(_, i)| (&self.segments[i], self.open && i == last))
    }

    /// Transport frame (callback time, no latency) of input frame `x`.
    pub fn map(&self, x: f64) -> Option<Mapped> {
        let (seg, _) = self.segment_for(x)?;
        Some(Mapped {
            transport_frame: seg.map(x),
            segment: seg.id,
            ratio: seg.ratio(),
            uncertainty_ms: seg.uncertainty_ms(),
        })
    }

    /// How the input span `[start, end]` maps.
    pub fn span(&self, start: f64, end: f64) -> SpanMapping {
        match (self.segment_for(start), self.segment_for(end)) {
            (Some((a, _)), Some((b, _))) if a.id == b.id => SpanMapping::Continuous(*a),
            (Some(_), Some(_)) => SpanMapping::Discontinuous,
            _ => {
                // A segment boundary inside the span with one end unmapped is still a break.
                let crosses = self.segments.iter().any(|s| {
                    s.ended.is_some_and(|b| b != Break::TransportStopped) && s.last_input > start && s.last_input < end
                });
                if crosses {
                    SpanMapping::Discontinuous
                } else {
                    SpanMapping::Unmapped
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;
    const BLOCK: u64 = 256;

    /// Deterministic jitter in [0, max_ns): callbacks only ever run late.
    fn jitter(i: u64, max_ns: u64) -> u64 {
        (i.wrapping_mul(2_654_435_761) >> 7) % max_ns
    }

    /// A simulated duplex pair. Input runs `ppm` fast; output is the reference.
    /// `transport_offset`: transport frame = output frame − offset (while running).
    struct Sim {
        ppm: f64,
        transport_offset: i64,
        input_lost: u64,
        input_disc: u64,
        output_disc: u64,
        running_from_ms: f64,
    }

    impl Sim {
        fn input_rate_hz(&self) -> f64 {
            SR as f64 * (1.0 + self.ppm * 1e-6)
        }

        /// Newest anchors at wall time `ms`.
        fn sample(&self, ms: f64) -> ClockSample {
            let w = ms / 1_000.0;
            // Input: blocks of BLOCK frames; anchor of the newest block started at or before w.
            let in_frames = (w * self.input_rate_hz()) as u64;
            let ib = in_frames / BLOCK;
            let in_block_wall = (ib * BLOCK) as f64 / self.input_rate_hz();
            let input = Anchor {
                host_ns: (in_block_wall * 1e9) as u64 + jitter(ib, 1_000_000),
                // After an input xrun the counter misses `input_lost` frames.
                frame: (ib * BLOCK).saturating_sub(self.input_lost),
                ..Anchor::default()
            };
            let ob = (w * SR as f64) as u64 / BLOCK;
            let out_frame = ob * BLOCK;
            let running = ms >= self.running_from_ms;
            let output = Anchor {
                host_ns: ((out_frame as f64 / SR as f64) * 1e9) as u64 + jitter(ob + 77, 1_000_000),
                frame: out_frame,
                transport_frame: if running { out_frame as i64 - self.transport_offset } else { 0 },
                transport_running: running,
                ..Anchor::default()
            };
            ClockSample {
                input,
                output,
                input_rate: SR,
                output_rate: SR,
                input_discontinuities: self.input_disc,
                output_discontinuities: self.output_disc,
            }
        }

        /// True transport frame produced at the wall time input frame `x` was captured.
        fn truth(&self, x: f64) -> f64 {
            let w = (x + self.input_lost as f64) / self.input_rate_hz();
            w * SR as f64 - self.transport_offset as f64
        }
    }

    fn run(m: &mut ClockMapper, sim: &Sim, from_ms: f64, to_ms: f64) {
        let mut ms = from_ms;
        while ms < to_ms {
            m.observe(sim.sample(ms));
            ms += 33.0;
        }
    }

    fn error_ms(m: &ClockMapper, sim: &Sim, x: f64) -> f64 {
        (m.map(x).unwrap().transport_frame - sim.truth(x)).abs() / 48.0
    }

    #[test]
    fn maps_within_half_a_millisecond_despite_callback_jitter() {
        let sim = Sim {
            ppm: 0.0,
            transport_offset: 10_000,
            input_lost: 0,
            input_disc: 0,
            output_disc: 0,
            running_from_ms: 0.0,
        };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 3_000.0);
        let seg = *m.current().expect("a segment");
        assert!(seg.points() > 80);
        for s in [0.2, 1.0, 2.5, 4.0] {
            let x = s * 48_000.0;
            assert!(error_ms(&m, &sim, x) < 0.5, "at {s} s: {} ms", error_ms(&m, &sim, x));
        }
        // Jitter up to 1 ms per anchor: the fitted offset is far better than that.
        assert!(seg.uncertainty_ms() < 0.2, "{}", seg.uncertainty_ms());
        let back = seg.inverse(seg.map(123_456.0));
        assert!((back - 123_456.0).abs() < 1e-6);
    }

    #[test]
    fn drift_is_fitted_over_long_segments() {
        // Input 80 ppm fast: 3.8 frames/s of slip, 230 frames (4.8 ms) over a minute.
        let sim =
            Sim { ppm: 80.0, transport_offset: 0, input_lost: 0, input_disc: 0, output_disc: 0, running_from_ms: 0.0 };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 60_000.0);
        let seg = *m.current().unwrap();
        let ppm = (1.0 / seg.ratio() - 1.0) * 1e6;
        assert!((ppm - 80.0).abs() < 10.0, "fitted {ppm} ppm");
        for s in [1.0, 30.0, 59.0] {
            let x = s * sim.input_rate_hz();
            assert!(error_ms(&m, &sim, x) < 0.5, "at {s} s: {} ms", error_ms(&m, &sim, x));
        }
    }

    #[test]
    fn stopped_transport_has_no_mapping_and_starting_opens_a_segment() {
        let sim = Sim {
            ppm: 0.0,
            transport_offset: 96_000,
            input_lost: 0,
            input_disc: 0,
            output_disc: 0,
            running_from_ms: 2_000.0,
        };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 1_900.0);
        assert!(m.current().is_none() && m.map(48_000.0).is_none());
        run(&mut m, &sim, 1_900.0, 3_000.0);
        // Pre-roll before the transport started is extrapolated from the running segment.
        assert!(error_ms(&m, &sim, 1.8 * 48_000.0) < 0.5);
        assert_eq!(m.breaks, 0);
    }

    #[test]
    fn transport_jumps_and_stream_discontinuities_start_new_segments() {
        let mut sim =
            Sim { ppm: 0.0, transport_offset: 0, input_lost: 0, input_disc: 0, output_disc: 0, running_from_ms: 0.0 };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 2_000.0);
        let first = m.current().unwrap().id;

        // Restart: the transport jumps back to 0.
        sim.transport_offset = (2_000.0 * 48.0) as i64;
        run(&mut m, &sim, 2_000.0, 4_000.0);
        let second = m.current().unwrap().id;
        assert_ne!(first, second);
        // Old frames still map through the old segment, new frames through the new.
        assert!((m.map(1.0 * 48_000.0).unwrap().transport_frame - 48_000.0).abs() < 30.0);
        assert!(error_ms(&m, &sim, 3.0 * 48_000.0) < 0.5);
        assert_eq!(m.span(0.5 * 48_000.0, 3.0 * 48_000.0), SpanMapping::Discontinuous);
        assert!(matches!(m.span(2.5 * 48_000.0, 3.5 * 48_000.0), SpanMapping::Continuous(_)));

        // Input xrun: 480 frames of hardware time lost, reported through the counter.
        sim.input_lost = 480;
        sim.input_disc = 1;
        run(&mut m, &sim, 4_000.0, 6_000.0);
        assert_ne!(m.current().unwrap().id, second);
        assert!(error_ms(&m, &sim, 5.0 * 48_000.0 - 480.0) < 0.5, "re-anchored after the lost time");
        assert_eq!(m.breaks, 2);
    }

    #[test]
    fn undetected_offset_jumps_re_anchor_but_single_late_callbacks_do_not() {
        let mut sim =
            Sim { ppm: 0.0, transport_offset: 0, input_lost: 0, input_disc: 0, output_disc: 0, running_from_ms: 0.0 };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 1_000.0);
        // One input anchor 15 ms late: an outlier, dropped.
        let mut late = sim.sample(1_000.0);
        late.input.host_ns += 15_000_000;
        assert_eq!(m.observe(late), Observed::Outlier);
        assert_eq!(m.observe(sim.sample(1_033.0)), Observed::Added);
        // Input loses 960 frames without the counter noticing: two agreeing outliers re-anchor.
        sim.input_lost = 960;
        assert_eq!(m.observe(sim.sample(1_066.0)), Observed::Outlier);
        assert_eq!(m.observe(sim.sample(1_099.0)), Observed::Started);
        run(&mut m, &sim, 1_132.0, 2_000.0);
        assert!(error_ms(&m, &sim, 1.5 * 48_000.0) < 0.5);
        assert_eq!((m.breaks, m.outliers), (1, 2));
    }

    #[test]
    fn bridge_underruns_and_drift_drops_do_not_move_the_mapping() {
        // The looper bridge's underruns, drift drops, or resampling change which
        // input frames the looper records, not the anchors: the anchors are
        // written by each device's own callback from its own frame counter.
        // So a session with any amount of bridge activity yields the same
        // samples, and therefore the same mapping, as one without.
        let sim =
            Sim { ppm: 300.0, transport_offset: 5, input_lost: 0, input_disc: 0, output_disc: 0, running_from_ms: 0.0 };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 20_000.0);
        for s in [0.5, 10.0, 19.5] {
            let x = s * sim.input_rate_hz();
            assert!(error_ms(&m, &sim, x) < 0.5, "at {s} s");
        }
        assert_eq!(m.breaks, 0);
    }

    #[test]
    fn segments_are_bounded() {
        let mut sim =
            Sim { ppm: 0.0, transport_offset: 0, input_lost: 0, input_disc: 0, output_disc: 0, running_from_ms: 0.0 };
        let mut m = ClockMapper::default();
        for i in 0..(MAX_SEGMENTS as u64 + 10) {
            sim.input_disc = i;
            let t = i as f64 * 200.0;
            run(&mut m, &sim, t, t + 200.0);
        }
        assert_eq!(m.segments.len(), MAX_SEGMENTS);
    }

    #[test]
    fn a_later_segment_never_maps_time_before_a_break() {
        let mut sim = Sim {
            ppm: 0.0,
            transport_offset: 0,
            input_lost: 0,
            input_disc: 0,
            output_disc: 0,
            running_from_ms: 1_000.0,
        };
        let mut m = ClockMapper::default();
        run(&mut m, &sim, 0.0, 3_000.0);
        sim.input_disc = 1;
        run(&mut m, &sim, 3_000.0, 5_000.0);
        // A take from 0.75 s (pre-roll before the first segment) to 4.5 s crosses the break.
        assert_eq!(m.span(0.75 * 48_000.0, 4.5 * 48_000.0), SpanMapping::Discontinuous);
        // The pre-roll itself maps through the first segment, extrapolated backwards.
        assert_eq!(m.map(0.75 * 48_000.0).unwrap().segment, 0);
        assert!(matches!(m.span(0.75 * 48_000.0, 2.5 * 48_000.0), SpanMapping::Continuous(s) if s.id == 0));
        // Frames shortly after the break map through the new segment only.
        assert_eq!(m.map(3.05 * 48_000.0).unwrap().segment, 1);
    }
}
