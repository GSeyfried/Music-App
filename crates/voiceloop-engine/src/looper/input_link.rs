//! Attaching and detaching the microphone while the output stream runs
//! ("mic only when needed", docs/projects/hardware-io/README.md).
//!
//! The output stream (looper, metronome, preview) starts on its own. Input
//! reaches it through an [`InputLink`]: the SPSC ring the input callback
//! fills plus that stream's clock stats. The control thread hands links to
//! the output callback through a small ring ([`InputLinkControl::attach`] /
//! [`InputLinkControl::detach`]); the callback swaps them in at the start of
//! a block ([`InputPort::begin_block`]) and hands the old one back, so it is
//! freed on the control thread, never on the audio thread.
//!
//! While no link is attached the looper receives silence: no underrun, no
//! discontinuity, no drift correction. Each swap restarts the drift bridge
//! ([`InputBridge::reattach`]): priming again, a new target fill, input
//! positions from 0, and a new mapping epoch (anchors made before no longer
//! apply), without counting a discontinuity.
//!
//! Realtime: [`InputPort`] methods do no allocation, locking, or I/O.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, PushError, RingBuffer};
use serde::Serialize;

use super::bridge::{BridgeReport, InputBridge};
use super::IoCounters;
use crate::clock::StreamStats;
use crate::error::EngineError;

/// Pending link swaps the control thread may queue (and links waiting to be freed).
const LINK_RING: usize = 4;

/// Capacity of the input→looper sample ring at `sample_rate` (0.5 s, at least 4096).
pub fn input_ring_capacity(sample_rate: u32) -> usize {
    ((sample_rate / 2) as usize).max(4096)
}

/// One input stream's connection to the output callback.
pub struct InputLink {
    pub rx: Consumer<f32>,
    /// The input stream's stats: its anchor gives the bridge a burst-free fill measure.
    pub clock: Option<Arc<StreamStats>>,
}

impl InputLink {
    /// A new ring sized for `sample_rate`: the producer goes to the input
    /// callback (`capture::LooperFeed`), the link to [`InputLinkControl::attach`].
    pub fn new(sample_rate: u32, clock: Option<Arc<StreamStats>>) -> (Producer<f32>, Self) {
        let (tx, rx) = RingBuffer::<f32>::new(input_ring_capacity(sample_rate));
        (tx, Self { rx, clock })
    }
}

/// What the looper is receiving from the microphone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InputFlow {
    /// No input stream attached: the looper hears silence.
    Detached,
    /// Attached, waiting for the first cushion of input (recording would capture silence).
    Priming,
    /// Real input is reaching the looper.
    Flowing,
}

const FLOW_DETACHED: u64 = 0;
const FLOW_PRIMING: u64 = 1;
const FLOW_FLOWING: u64 = 2;

/// `(swaps applied << 2) | flow`, written by the output callback.
fn encode(applied: u64, flow: u64) -> u64 {
    (applied << 2) | flow
}

/// Control-thread side.
pub struct InputLinkControl {
    tx: Producer<Option<InputLink>>,
    back: Consumer<InputLink>,
    state: Arc<AtomicU64>,
    /// Swaps sent (the callback reports how many it applied).
    sent: u64,
    attached: bool,
}

/// Output-callback side.
pub struct InputPort {
    links: Consumer<Option<InputLink>>,
    back: Producer<InputLink>,
    link: Option<InputLink>,
    bridge: InputBridge,
    overrun_seen: u64,
    applied: u64,
    state: Arc<AtomicU64>,
}

/// A connected pair. `initial` is attached from the first block (no swap).
pub fn input_link_pair(sample_rate: u32, initial: Option<InputLink>) -> (InputLinkControl, InputPort) {
    let (tx, links) = RingBuffer::new(LINK_RING);
    let (back_tx, back) = RingBuffer::new(LINK_RING);
    let attached = initial.is_some();
    let state = Arc::new(AtomicU64::new(encode(0, if attached { FLOW_PRIMING } else { FLOW_DETACHED })));
    let capacity = initial.as_ref().map_or(input_ring_capacity(sample_rate), |l| l.rx.buffer().capacity());
    let control = InputLinkControl { tx, back, state: state.clone(), sent: 0, attached };
    let port = InputPort {
        links,
        back: back_tx,
        link: initial,
        bridge: InputBridge::new(sample_rate, capacity),
        overrun_seen: 0,
        applied: 0,
        state,
    };
    (control, port)
}

impl InputLinkControl {
    /// Connect a (new) input stream. Replaces any link still attached.
    pub fn attach(&mut self, link: InputLink) -> Result<(), EngineError> {
        self.send(Some(link))?;
        self.attached = true;
        Ok(())
    }

    /// Disconnect the input; the looper hears silence from the next block.
    /// Stop the input stream only after [`InputLinkControl::wait_applied`],
    /// so the bridge never sees the ring run dry while still attached.
    pub fn detach(&mut self) -> Result<(), EngineError> {
        if !self.attached {
            return Ok(());
        }
        self.send(None)?;
        self.attached = false;
        Ok(())
    }

    fn send(&mut self, msg: Option<InputLink>) -> Result<(), EngineError> {
        self.collect();
        self.tx.push(msg).map_err(|_| EngineError::Busy("the output stream is not taking input changes".into()))?;
        self.sent += 1;
        Ok(())
    }

    pub fn is_attached(&self) -> bool {
        self.attached
    }

    /// Free links the callback has handed back. Returns how many.
    pub fn collect(&mut self) -> usize {
        let mut n = 0;
        while self.back.pop().is_ok() {
            n += 1;
        }
        n
    }

    /// The callback has applied every swap sent so far.
    pub fn applied(&self) -> bool {
        self.state.load(Ordering::Relaxed) >> 2 == self.sent
    }

    /// Wait (sleeping, control thread only) until the callback has applied
    /// every swap, at most `timeout`. Frees returned links. Returns whether it did.
    pub fn wait_applied(&mut self, timeout: Duration) -> bool {
        let start = Instant::now();
        while !self.applied() {
            if start.elapsed() >= timeout {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.collect();
        true
    }

    /// What the looper receives now, as last reported by the callback.
    pub fn flow(&self) -> InputFlow {
        let raw = self.state.load(Ordering::Relaxed);
        if !self.attached {
            return InputFlow::Detached;
        }
        if raw >> 2 != self.sent {
            return InputFlow::Priming; // the newest link has not reached the callback yet
        }
        match raw & 3 {
            FLOW_FLOWING => InputFlow::Flowing,
            FLOW_PRIMING => InputFlow::Priming,
            _ => InputFlow::Detached,
        }
    }
}

impl InputPort {
    /// Once per callback, before [`InputPort::fill`]: apply queued link
    /// swaps, account for input lost before the ring, and pass the input
    /// clock (`host_now_ns`) to the bridge. Realtime-safe.
    #[inline]
    pub fn begin_block(&mut self, host_now_ns: u64, sample_rate: u32, counters: &IoCounters) {
        let overrun = counters.input_overrun_frames.load(Ordering::Relaxed);
        while let Ok(msg) = self.links.pop() {
            if let Some(old) = self.link.take() {
                if let Err(PushError::Full(old)) = self.back.push(old) {
                    // Never free on the audio thread; the control thread drains
                    // this ring before every send, so this does not happen.
                    std::mem::forget(old);
                }
            }
            self.link = msg;
            self.bridge.reattach();
            self.overrun_seen = overrun;
            self.applied += 1;
        }
        let Some(link) = self.link.as_ref() else { return };
        // Input frames lost before the ring (input overrun) break the mapping.
        if overrun > self.overrun_seen {
            self.bridge.note_input_gap(overrun - self.overrun_seen);
            self.overrun_seen = overrun;
        }
        // Input frames captured by now, from the input callback's newest anchor
        // (both streams run at `sample_rate` whenever the looper runs).
        if let Some(input) = link.clock.as_ref() {
            let captured = input
                .anchor
                .read()
                .map(|a| a.frame as f64 + (host_now_ns as f64 - a.host_ns as f64) * sample_rate as f64 / 1e9);
            self.bridge.set_input_clock(captured);
        }
    }

    /// Fill `dst` with bridged input, or silence while detached. Realtime-safe.
    #[inline]
    pub fn fill(&mut self, dst: &mut [f32]) -> BridgeReport {
        match self.link.as_mut() {
            Some(link) => self.bridge.fill(&mut link.rx, dst),
            None => {
                dst.fill(0.0);
                BridgeReport::default()
            }
        }
    }

    /// Publish the flow state (end of the callback). Realtime-safe.
    #[inline]
    pub fn end_block(&self) {
        let flow = match &self.link {
            None => FLOW_DETACHED,
            Some(_) if self.bridge.has_started() => FLOW_FLOWING,
            Some(_) => FLOW_PRIMING,
        };
        self.state.store(encode(self.applied, flow), Ordering::Relaxed);
    }

    pub fn bridge(&self) -> &InputBridge {
        &self.bridge
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::looper::{LooperCommand, LooperConfig, LooperCore, RunState, TrackState};
    use crate::model::LoopMode;
    use crate::transport::{OverdubStart, TransportState};

    const SR: u32 = 48_000;
    const BLOCK: usize = 256;

    /// One output callback: optionally push a block of input first (the input
    /// callback running), then pull a block through the port.
    fn block(port: &mut InputPort, counters: &IoCounters, feed: Option<(&mut Producer<f32>, f32)>) -> BridgeReport {
        if let Some((tx, value)) = feed {
            for _ in 0..BLOCK {
                let _ = tx.push(value);
            }
        }
        let mut buf = [1.0f32; BLOCK];
        port.begin_block(0, SR, counters);
        let r = port.fill(&mut buf);
        port.end_block();
        if r.delivered == 0 {
            assert!(buf.iter().all(|&v| v == 0.0), "missing input is silence");
        }
        r
    }

    #[test]
    fn output_only_delivers_silence_without_discontinuities() {
        let counters = IoCounters::default();
        let (control, mut port) = input_link_pair(SR, None);
        assert_eq!(control.flow(), InputFlow::Detached);
        for _ in 0..1_000 {
            assert_eq!(block(&mut port, &counters, None), BridgeReport::default());
        }
        assert_eq!((port.bridge().discontinuities(), port.bridge().epoch()), (0, 0));
        assert_eq!(control.flow(), InputFlow::Detached);
    }

    #[test]
    fn attach_detach_and_reattach_never_count_a_discontinuity() {
        let counters = IoCounters::default();
        let (mut control, mut port) = input_link_pair(SR, None);
        block(&mut port, &counters, None);

        let (mut tx, link) = InputLink::new(SR, None);
        control.attach(link).unwrap();
        assert_eq!(control.flow(), InputFlow::Priming, "sent, not yet applied");
        block(&mut port, &counters, Some((&mut tx, 0.25)));
        assert!(control.applied());
        let mut flowing = false;
        for _ in 0..20 {
            flowing |= block(&mut port, &counters, Some((&mut tx, 0.25))).delivered == BLOCK;
        }
        assert!(flowing);
        assert_eq!(control.flow(), InputFlow::Flowing);

        // Detach first, then the input stream stops: no underrun is seen.
        control.detach().unwrap();
        assert_eq!(control.flow(), InputFlow::Detached);
        block(&mut port, &counters, Some((&mut tx, 0.25)));
        drop(tx);
        assert!(control.wait_applied(Duration::from_millis(10)));
        for _ in 0..100 {
            assert_eq!(block(&mut port, &counters, None), BridgeReport::default());
        }
        assert_eq!(control.collect(), 0, "wait_applied already freed the old link");

        // A new input stream (new ring, frames from 0) attaches cleanly.
        let (mut tx2, link2) = InputLink::new(SR, None);
        control.attach(link2).unwrap();
        let mut delivered = 0;
        for _ in 0..20 {
            let r = block(&mut port, &counters, Some((&mut tx2, -0.5)));
            assert!(!r.discontinuity && r.underrun == 0, "{r:?}");
            delivered += r.delivered;
        }
        assert!(delivered > 0);
        assert_eq!(control.flow(), InputFlow::Flowing);
        assert_eq!(port.bridge().discontinuities(), 0);
        assert!(port.bridge().epoch() >= 2, "each swap starts a new mapping epoch");
    }

    #[test]
    fn overruns_from_before_an_attach_are_not_a_gap() {
        let counters = IoCounters::default();
        counters.input_overrun_frames.store(500, Ordering::Relaxed);
        let (mut control, mut port) = input_link_pair(SR, None);
        let (mut tx, link) = InputLink::new(SR, None);
        control.attach(link).unwrap();
        for _ in 0..10 {
            block(&mut port, &counters, Some((&mut tx, 0.1)));
        }
        assert_eq!(port.bridge().discontinuities(), 0);
        counters.input_overrun_frames.store(600, Ordering::Relaxed);
        block(&mut port, &counters, Some((&mut tx, 0.1)));
        assert_eq!(port.bridge().discontinuities(), 1, "a real overrun while attached still counts");
    }

    /// The output callback's loop around the looper, with an optional input feed.
    struct Out {
        core: LooperCore,
        h: crate::looper::LooperHandles,
        port: InputPort,
        counters: IoCounters,
        out: Vec<f32>,
    }

    impl Out {
        fn run(&mut self, blocks: usize, mut feed: Option<&mut Producer<f32>>, value: f32) {
            let mut inb = [0.0f32; BLOCK];
            let mut outb = [0.0f32; BLOCK];
            for _ in 0..blocks {
                if let Some(tx) = feed.as_deref_mut() {
                    for _ in 0..BLOCK {
                        let _ = tx.push(value);
                    }
                }
                self.port.begin_block(0, SR, &self.counters);
                let r = self.port.fill(&mut inb);
                assert!(!r.discontinuity, "no input discontinuity");
                self.core.process(&inb, &mut outb);
                self.port.end_block();
                self.out.extend_from_slice(&outb);
                while self.h.snapshots.pop().is_ok() {}
            }
            self.h.recycler.service();
        }

        fn loudest_since(&self, from: usize) -> f32 {
            self.out[from..].iter().fold(0.0f32, |m, v| m.max(v.abs()))
        }
    }

    #[test]
    fn loops_keep_playing_while_the_mic_closes_and_reopens() {
        let mut t = TransportState { sample_rate: SR, mode: LoopMode::Free, ..Default::default() };
        t.metronome.enabled = false;
        t.metronome.count_in_bars = 0;
        t.overdub_start = OverdubStart::Immediate;
        let (core, h) = LooperCore::new(&LooperConfig::default(), t, SR).unwrap();
        let (mut control, port) = input_link_pair(SR, None);
        let mut o = Out { core, h, port, counters: IoCounters::default(), out: Vec::new() };

        // Output-only: the transport runs with no input at all.
        o.h.commands.push(LooperCommand::Play).unwrap();
        o.run(20, None, 0.0);
        assert_eq!(o.core.snapshot().run, RunState::Running);

        // Open the mic, wait for input to flow, record one loop.
        let (mut tx, link) = InputLink::new(SR, None);
        control.attach(link).unwrap();
        while control.flow() != InputFlow::Flowing {
            o.run(1, Some(&mut tx), 0.5);
        }
        o.h.commands.push(LooperCommand::Record { track: 0 }).unwrap();
        o.run(40, Some(&mut tx), 0.5);
        o.h.commands.push(LooperCommand::StopRecording { track: 0 }).unwrap();
        o.run(1, Some(&mut tx), 0.5);
        let period = o.core.snapshot().loop_period.expect("a loop exists");
        assert!(period > 0.0);

        // Close the mic: loops keep playing, nothing is lost.
        control.detach().unwrap();
        o.run(1, Some(&mut tx), 0.0);
        drop(tx);
        let mark = o.out.len();
        o.run(200, None, 0.0);
        assert!(o.loudest_since(mark) > 0.3, "the loop is still heard with the mic closed");
        let snap = o.core.snapshot();
        assert_eq!((snap.tracks[0].state, snap.tracks[0].layers), (TrackState::Playing, 1));
        assert!(!snap.tracks[0].timing_suspect);
        assert_eq!(snap.loop_period, Some(period));

        // Reopen: same loop, still playing, and an overdub records the new input.
        let (mut tx2, link2) = InputLink::new(SR, None);
        control.attach(link2).unwrap();
        while control.flow() != InputFlow::Flowing {
            o.run(1, Some(&mut tx2), 0.25);
        }
        let mark = o.out.len();
        o.run(100, Some(&mut tx2), 0.25);
        assert!(o.loudest_since(mark) > 0.3);
        o.h.commands.push(LooperCommand::Overdub { track: 0 }).unwrap();
        o.run(200, Some(&mut tx2), 0.25);
        o.h.commands.push(LooperCommand::StopRecording { track: 0 }).unwrap();
        o.run(2, Some(&mut tx2), 0.25);
        let snap = o.core.snapshot();
        assert_eq!(snap.tracks[0].layers, 2);
        assert!(!snap.tracks[0].timing_suspect, "no discontinuity reached the looper");
        assert_eq!(o.port.bridge().discontinuities(), 0);
    }
}
