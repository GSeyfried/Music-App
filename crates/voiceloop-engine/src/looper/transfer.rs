//! Realtime-safe layer export and import.
//!
//! Layer buffers belong to the audio thread (D-011), so the control thread
//! cannot read or write them directly. Instead it sends [`LayerJob`]s through
//! a wait-free ring; the output callback answers at most [`JOBS_PER_BLOCK`]
//! of them per block and returns each job (with its buffer) on a second ring.
//!
//! ```text
//!  control thread                         output callback (LooperCore::service_jobs)
//!  Describe ───────────────────────────▶  fills Inventory (layout, gains, epoch)
//!  Read{slot, offset, buf} ×N ─────────▶  copies ≤ CHUNK_FRAMES into buf, stamps epoch
//!  Restore{plan} ──────────────────────▶  releases all layers, takes zeroed ones, enters "loading"
//!  Write{slot, offset, buf} ×N ────────▶  copies buf into the layer
//!  Finish ─────────────────────────────▶  leaves "loading"
//!  Probe{slot} ────────────────────────▶  identity (buffer address), length, complete?
//!  Swap{slot, expect, buf} ────────────▶  O(1) pointer swap of one finished layer (noise reduction)
//! ```
//!
//! **Swap** replaces the samples of one complete layer while audio plays
//! (derived buffers such as noise reduction). The control thread sends a
//! full-size buffer it allocated; the audio thread checks the layer is still
//! the one the control thread rendered from (same buffer address and a few
//! probe samples), swaps the two boxes, restarts that layer's summary scan,
//! and returns the old buffer to be dropped on the control thread. The pool
//! keeps its size: one buffer leaves circulation and one joins it.
//!
//! - **No allocation, freeing, or locking on the audio thread.** Every buffer
//!   is allocated by the control thread and returned to it to be dropped.
//! - **Bounded work per callback:** at most `JOBS_PER_BLOCK × CHUNK_FRAMES`
//!   samples copied (4 × 16 384 floats = 256 KiB, roughly 20-40 µs).
//! - **Consistency:** the looper bumps an *epoch* whenever layer content or
//!   the layer stacks change (recording, undo, clear ...). An export is valid
//!   only if every chunk carries the epoch reported by `Describe`; otherwise
//!   the user changed something mid-copy and the export is retried.
//! - **Import** is accepted only while the transport is stopped and nothing
//!   records; while loading, the looper refuses commands (`trackBusy`), so a
//!   half-written layer can never be heard.
//!
//! [`Exporter`] and [`Importer`] are the control-thread state machines. They
//! never block: call `poll` repeatedly (the engine does so every few ms).

use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer};

use super::pool::{LayerBuf, LayerRecycler};
use super::processor::{LooperHandles, RejectReason, RunState};
use super::{MAX_LAYERS_PER_TRACK, MAX_TRACKS};
use crate::transport::{TrackLoop, TrackLoopMode, TransportState};

/// Capacity of the job and result rings. The control side keeps at most this
/// many jobs outstanding, so the audio side can always return them.
pub const JOB_CAPACITY: usize = 32;
/// Jobs the audio thread serves per callback.
pub const JOBS_PER_BLOCK: usize = 4;
/// Frames copied per Read/Write job.
pub const CHUNK_FRAMES: usize = 16_384;
/// A transfer that makes no progress for this long has failed (audio stopped?).
pub const STALL_TIMEOUT: Duration = Duration::from_secs(3);

/// A layer of a track: `Active(i)` is the i-th audible layer (0 = base take),
/// `Redo(i)` the i-th undone layer (oldest first; Redo restores the last one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerSlot {
    Active(u8),
    Redo(u8),
}

/// One track's loop settings and its own clock. Every clock restarts at
/// phase 0 when the transport stops, and plans are only taken or restored
/// while stopped, so the period (with origin 0) is the whole clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackClock {
    pub track_loop: TrackLoop,
    /// The track's own loop period in frames. `None` for a Song track (it
    /// follows the song loop) or a track that has no loop yet.
    pub period: Option<f64>,
    /// Multiple mode: the base loop period the ratio refers to.
    pub base: Option<f64>,
}

impl TrackClock {
    /// A track that follows the song loop (what plans without loop info mean).
    pub const SONG: Self = Self { track_loop: TrackLoop::DEFAULT, period: None, base: None };
}

impl Default for TrackClock {
    fn default() -> Self {
        Self::SONG
    }
}

/// Mixer state, layer counts and loop clock of one track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPlan {
    pub layers: u8,
    pub redo: u8,
    pub gain: f32,
    pub muted: bool,
    pub solo: bool,
    pub playing: bool,
    pub layer_offsets: [i64; MAX_LAYERS_PER_TRACK],
    pub redo_offsets: [i64; MAX_LAYERS_PER_TRACK],
    pub clock: TrackClock,
    /// The track's effect chain (a restore installs its DSP afterwards).
    pub effects: voiceloop_dsp::ChainSpec,
}

impl TrackPlan {
    pub const EMPTY: Self = Self {
        layers: 0,
        redo: 0,
        gain: 1.0,
        muted: false,
        solo: false,
        playing: true,
        layer_offsets: [0; MAX_LAYERS_PER_TRACK],
        redo_offsets: [0; MAX_LAYERS_PER_TRACK],
        clock: TrackClock::SONG,
        effects: voiceloop_dsp::ChainSpec::EMPTY,
    };
}

/// What the looper holds, as seen by one consistent `Describe`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Inventory {
    pub epoch: u64,
    /// Something is recording, draining, or loading: content is changing.
    pub busy: bool,
    pub run: RunState,
    pub transport: TransportState,
    /// Master loop period in frames (`None` before the first loop exists).
    pub period: Option<f64>,
    pub latency: u32,
    pub max_frames: usize,
    pub max_layers: usize,
    pub track_count: u8,
    /// Most tracks the looper can hold (`addTrack` limit).
    pub track_capacity: u8,
    pub tracks: [TrackPlan; MAX_TRACKS],
}

impl Inventory {
    /// Frames of each song-loop layer that can hold audio: `ceil(period)`.
    pub fn content_frames(&self) -> usize {
        self.period.map_or(0, |p| (p.ceil().max(0.0) as usize).min(self.max_frames))
    }

    /// Frames of each layer of a track with this plan: the song loop's for a
    /// Song track, otherwise `ceil` of the track's own period.
    pub fn track_frames(&self, plan: &TrackPlan) -> usize {
        match plan.clock.track_loop.mode {
            TrackLoopMode::Song => self.content_frames(),
            _ => plan.clock.period.map_or(0, |p| (p.ceil().max(0.0) as usize).min(self.max_frames)),
        }
    }
}

/// Content to load: transport, loop period, latency, and per-track layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestorePlan {
    pub transport: TransportState,
    /// Free-mode loop period. Ignored in Metronome mode (derived from the transport).
    pub period: Option<f64>,
    pub latency: u32,
    /// Active tracks after the restore (empty ones included).
    pub track_count: u8,
    pub tracks: [TrackPlan; MAX_TRACKS],
}

#[derive(Debug)]
pub struct Chunk {
    pub track: u8,
    pub slot: LayerSlot,
    pub offset: usize,
    pub buf: Box<[f32]>,
    /// Epoch at the time of the copy (set by the audio thread).
    pub epoch: u64,
    pub result: Option<Result<(), RejectReason>>,
}

/// Identity of one layer buffer, answered by the audio thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayerIdentity {
    /// Address of the layer buffer (stable while the layer exists).
    pub ptr: usize,
    /// Buffer length (`max_frames`).
    pub len: usize,
    /// Frames of the track's loop (what holds audio).
    pub frames: usize,
    /// The layer is finished (not recording): its samples no longer change.
    pub complete: bool,
}

/// Probe samples a swap checks before replacing a layer.
pub const SWAP_PROBES: usize = 16;

#[derive(Debug)]
pub struct SwapJob {
    pub track: u8,
    pub slot: LayerSlot,
    /// The layer must still be this buffer...
    pub expect_ptr: usize,
    /// ...holding these samples (frame, value).
    pub probes: [(u32, f32); SWAP_PROBES],
    /// New content, `len` frames (the pool buffer length). Comes back holding the old content.
    pub buf: Box<[f32]>,
    pub result: Option<Result<(), RejectReason>>,
}

#[derive(Debug)]
pub enum JobKind {
    Describe(Option<Inventory>),
    Read(Chunk),
    Restore(RestorePlan, Option<Result<(), RejectReason>>),
    Write(Chunk),
    Finish(Option<Result<(), RejectReason>>),
    Probe { track: u8, slot: LayerSlot, answer: Option<Result<LayerIdentity, RejectReason>> },
    Swap(Box<SwapJob>),
}

/// One request; the same value comes back with its result filled in.
#[derive(Debug)]
pub struct LayerJob {
    /// Identifies the transfer, so answers to an abandoned one are ignored.
    pub ticket: u32,
    pub kind: JobKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransferError {
    /// Recording, draining or loading in progress, or content changed mid-copy.
    Busy(&'static str),
    Rejected(RejectReason),
    /// The plan does not fit this looper (tracks, layers, length).
    DoesNotFit(String),
    /// No answer from the audio thread.
    Stalled,
}

impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(why) => write!(f, "looper is busy: {why}"),
            Self::Rejected(r) => write!(f, "looper rejected the transfer: {r:?}"),
            Self::DoesNotFit(why) => write!(f, "{why}"),
            Self::Stalled => write!(f, "the audio thread stopped answering"),
        }
    }
}

pub enum Poll<T> {
    Pending,
    Ready(Result<T, TransferError>),
}

/// Audio copied out of the looper.
#[derive(Debug, Clone, PartialEq)]
pub struct CapturedTrack {
    pub plan: TrackPlan,
    /// Audible layers, base take first. Each is `content_frames` long.
    pub layers: Vec<Vec<f32>>,
    /// Undone layers, oldest first.
    pub redo: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Captured {
    pub inventory: Inventory,
    pub tracks: Vec<CapturedTrack>,
}

fn next_ticket() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static T: AtomicU32 = AtomicU32::new(1);
    T.fetch_add(1, Ordering::Relaxed)
}

/// Sends queued jobs while keeping at most `JOB_CAPACITY` outstanding.
struct Pipe {
    ticket: u32,
    queue: std::collections::VecDeque<JobKind>,
    outstanding: usize,
    last_progress: Instant,
}

impl Pipe {
    fn new() -> Self {
        Self { ticket: next_ticket(), queue: Default::default(), outstanding: 0, last_progress: Instant::now() }
    }

    fn pump(&mut self, jobs: &mut Producer<LayerJob>) {
        while self.outstanding < JOB_CAPACITY && jobs.slots() > 0 {
            let Some(kind) = self.queue.pop_front() else { break };
            match jobs.push(LayerJob { ticket: self.ticket, kind }) {
                Ok(()) => self.outstanding += 1,
                Err(rtrb::PushError::Full(job)) => {
                    self.queue.push_front(job.kind);
                    break;
                }
            }
        }
    }

    /// Next answer belonging to this transfer.
    fn recv(&mut self, results: &mut Consumer<LayerJob>) -> Option<JobKind> {
        while let Ok(job) = results.pop() {
            if job.ticket == self.ticket {
                self.outstanding = self.outstanding.saturating_sub(1);
                self.last_progress = Instant::now();
                return Some(job.kind);
            }
        }
        None
    }

    fn stalled(&self) -> bool {
        self.last_progress.elapsed() > STALL_TIMEOUT
    }
}

/// Copies every layer out of a running looper.
pub struct Exporter {
    pipe: Pipe,
    inventory: Option<Inventory>,
    out: Vec<CapturedTrack>,
    remaining: usize,
}

impl Default for Exporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Exporter {
    pub fn new() -> Self {
        let mut pipe = Pipe::new();
        pipe.queue.push_back(JobKind::Describe(None));
        Self { pipe, inventory: None, out: Vec::new(), remaining: 0 }
    }

    fn plan_reads(&mut self, inv: Inventory) {
        for (t, &plan) in inv.tracks.iter().enumerate().take(inv.track_count as usize) {
            let frames = inv.track_frames(&plan);
            let mut track = CapturedTrack { plan, layers: Vec::new(), redo: Vec::new() };
            for (count, redo) in [(plan.layers, false), (plan.redo, true)] {
                for i in 0..count {
                    let v = vec![0.0f32; frames];
                    if redo {
                        track.redo.push(v);
                    } else {
                        track.layers.push(v);
                    }
                    let slot = if redo { LayerSlot::Redo(i) } else { LayerSlot::Active(i) };
                    let mut offset = 0;
                    while offset < frames {
                        let len = CHUNK_FRAMES.min(frames - offset);
                        self.pipe.queue.push_back(JobKind::Read(Chunk {
                            track: t as u8,
                            slot,
                            offset,
                            buf: vec![0.0; len].into_boxed_slice(),
                            epoch: 0,
                            result: None,
                        }));
                        self.remaining += 1;
                        offset += len;
                    }
                }
            }
            self.out.push(track);
        }
    }

    pub fn poll(&mut self, h: &mut LooperHandles) -> Poll<Captured> {
        self.pipe.pump(&mut h.jobs);
        while let Some(kind) = self.pipe.recv(&mut h.job_results) {
            match kind {
                JobKind::Describe(Some(inv)) => {
                    if inv.busy {
                        return Poll::Ready(Err(TransferError::Busy("finish recording before saving")));
                    }
                    self.inventory = Some(inv);
                    self.plan_reads(inv);
                }
                JobKind::Read(chunk) => {
                    let Some(inv) = self.inventory else { continue };
                    if let Some(Err(r)) = chunk.result {
                        return Poll::Ready(Err(TransferError::Rejected(r)));
                    }
                    if chunk.epoch != inv.epoch {
                        return Poll::Ready(Err(TransferError::Busy("loop content changed while it was being copied")));
                    }
                    let track = &mut self.out[chunk.track as usize];
                    let dst = match chunk.slot {
                        LayerSlot::Active(i) => &mut track.layers[i as usize],
                        LayerSlot::Redo(i) => &mut track.redo[i as usize],
                    };
                    dst[chunk.offset..chunk.offset + chunk.buf.len()].copy_from_slice(&chunk.buf);
                    self.remaining -= 1;
                }
                _ => {}
            }
            self.pipe.pump(&mut h.jobs);
        }
        match self.inventory {
            Some(inv) if self.remaining == 0 => {
                Poll::Ready(Ok(Captured { inventory: inv, tracks: std::mem::take(&mut self.out) }))
            }
            _ if self.pipe.stalled() => Poll::Ready(Err(TransferError::Stalled)),
            _ => Poll::Pending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportStage {
    Describing,
    Restoring,
    Writing,
    Finishing,
}

/// Loads layers into a stopped looper.
pub struct Importer {
    pipe: Pipe,
    plan: RestorePlan,
    /// (track, slot, samples)
    layers: Vec<(u8, LayerSlot, Vec<f32>)>,
    stage: ImportStage,
    remaining: usize,
    restore_attempts: u32,
    /// A write failed: finish loading (so the looper is usable) and then report this.
    failed: Option<TransferError>,
}

impl Importer {
    /// `layers` holds every layer named by `plan` (any order). Samples beyond
    /// the looper's layer length are an error, shorter layers are zero-padded.
    pub fn new(plan: RestorePlan, layers: Vec<(u8, LayerSlot, Vec<f32>)>) -> Self {
        let mut pipe = Pipe::new();
        pipe.queue.push_back(JobKind::Describe(None));
        Self { pipe, plan, layers, stage: ImportStage::Describing, remaining: 0, restore_attempts: 0, failed: None }
    }

    fn check_fit(&self, inv: &Inventory) -> Result<(), TransferError> {
        if inv.busy || inv.run != RunState::Stopped {
            return Err(TransferError::Busy("stop the transport before opening a project"));
        }
        if self.plan.transport.sample_rate != inv.transport.sample_rate {
            return Err(TransferError::DoesNotFit(format!(
                "content is at {} Hz but the looper runs at {} Hz (resample before importing)",
                self.plan.transport.sample_rate, inv.transport.sample_rate
            )));
        }
        if self.plan.track_count > inv.track_capacity {
            return Err(TransferError::DoesNotFit(format!(
                "the project has {} tracks but the looper holds at most {}",
                self.plan.track_count, inv.track_capacity
            )));
        }
        for (t, tp) in self.plan.tracks.iter().enumerate() {
            let used = tp.layers > 0 || tp.redo > 0;
            if used && t >= inv.track_capacity as usize {
                return Err(TransferError::DoesNotFit(format!(
                    "the project uses track {} but the looper holds at most {} tracks",
                    t + 1,
                    inv.track_capacity
                )));
            }
            if tp.layers as usize > inv.max_layers || tp.redo as usize > inv.max_layers {
                return Err(TransferError::DoesNotFit(format!(
                    "track {} has more layers than the looper allows ({})",
                    t + 1,
                    inv.max_layers
                )));
            }
        }
        let track_periods = self.plan.tracks.iter().map(|tp| tp.clock.period);
        for p in std::iter::once(self.plan.period).chain(track_periods).flatten() {
            if !(p.is_finite() && p >= 1.0) || p.ceil() as usize > inv.max_frames {
                return Err(TransferError::DoesNotFit(format!(
                    "the loop is {p:.0} frames but the looper holds at most {} (raise maxLoopSeconds)",
                    inv.max_frames
                )));
            }
        }
        if let Some((_, _, v)) = self.layers.iter().find(|(_, _, v)| v.len() > inv.max_frames) {
            return Err(TransferError::DoesNotFit(format!(
                "a layer is {} frames but the looper holds at most {}",
                v.len(),
                inv.max_frames
            )));
        }
        Ok(())
    }

    fn plan_writes(&mut self) {
        for (track, slot, data) in &self.layers {
            let mut offset = 0;
            while offset < data.len() {
                let len = CHUNK_FRAMES.min(data.len() - offset);
                self.pipe.queue.push_back(JobKind::Write(Chunk {
                    track: *track,
                    slot: *slot,
                    offset,
                    buf: data[offset..offset + len].to_vec().into_boxed_slice(),
                    epoch: 0,
                    result: None,
                }));
                self.remaining += 1;
                offset += len;
            }
        }
        if self.remaining == 0 {
            self.pipe.queue.push_back(JobKind::Finish(None));
            self.stage = ImportStage::Finishing;
        }
    }

    pub fn poll(&mut self, h: &mut LooperHandles) -> Poll<()> {
        self.pipe.pump(&mut h.jobs);
        while let Some(kind) = self.pipe.recv(&mut h.job_results) {
            match (self.stage, kind) {
                (ImportStage::Describing, JobKind::Describe(Some(inv))) => {
                    if let Err(e) = self.check_fit(&inv) {
                        return Poll::Ready(Err(e));
                    }
                    self.stage = ImportStage::Restoring;
                    self.pipe.queue.push_back(JobKind::Restore(self.plan, None));
                }
                (ImportStage::Restoring, JobKind::Restore(_, result)) => match result {
                    Some(Ok(())) => {
                        self.stage = ImportStage::Writing;
                        self.plan_writes();
                    }
                    // Old layers were released but are not zeroed yet: zero them and retry.
                    Some(Err(RejectReason::PoolExhausted)) if self.restore_attempts < 50 => {
                        self.restore_attempts += 1;
                        recycle(&mut h.recycler);
                        self.pipe.queue.push_back(JobKind::Restore(self.plan, None));
                    }
                    Some(Err(r)) => return Poll::Ready(Err(TransferError::Rejected(r))),
                    None => return Poll::Ready(Err(TransferError::Stalled)),
                },
                (ImportStage::Writing, JobKind::Write(chunk)) => {
                    if let Some(Err(r)) = chunk.result {
                        self.pipe.queue.clear();
                        self.pipe.queue.push_back(JobKind::Finish(None));
                        self.stage = ImportStage::Finishing;
                        self.failed = Some(TransferError::Rejected(r));
                        continue;
                    }
                    self.remaining -= 1;
                    if self.remaining == 0 {
                        self.stage = ImportStage::Finishing;
                        self.pipe.queue.push_back(JobKind::Finish(None));
                    }
                }
                (ImportStage::Finishing, JobKind::Finish(result)) => {
                    return Poll::Ready(match (self.failed.take(), result) {
                        (Some(e), _) => Err(e),
                        (None, Some(Err(r))) => Err(TransferError::Rejected(r)),
                        _ => Ok(()),
                    });
                }
                _ => {}
            }
            self.pipe.pump(&mut h.jobs);
        }
        if self.pipe.stalled() {
            return Poll::Ready(Err(TransferError::Stalled));
        }
        Poll::Pending
    }
}

/// A finished layer copied out of the looper.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedLayer {
    pub identity: LayerIdentity,
    /// `identity.frames` samples.
    pub samples: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FetchStage {
    Probing,
    Reading,
    Confirming,
}

/// Copies one finished layer out of a (possibly playing) looper: probe,
/// read in chunks, probe again. Finished layers are immutable, so the same
/// buffer before and after the copy proves the copy is consistent.
pub struct LayerFetch {
    pipe: Pipe,
    track: u8,
    slot: LayerSlot,
    stage: FetchStage,
    identity: Option<LayerIdentity>,
    samples: Vec<f32>,
    remaining: usize,
}

impl LayerFetch {
    pub fn new(track: u8, slot: LayerSlot) -> Self {
        let mut pipe = Pipe::new();
        pipe.queue.push_back(JobKind::Probe { track, slot, answer: None });
        Self { pipe, track, slot, stage: FetchStage::Probing, identity: None, samples: Vec::new(), remaining: 0 }
    }

    pub fn poll(&mut self, h: &mut LooperHandles) -> Poll<FetchedLayer> {
        self.pipe.pump(&mut h.jobs);
        while let Some(kind) = self.pipe.recv(&mut h.job_results) {
            match (self.stage, kind) {
                (FetchStage::Probing, JobKind::Probe { answer, .. }) => {
                    let id = match answer {
                        Some(Ok(id)) => id,
                        Some(Err(r)) => return Poll::Ready(Err(TransferError::Rejected(r))),
                        None => return Poll::Ready(Err(TransferError::Stalled)),
                    };
                    if !id.complete {
                        return Poll::Ready(Err(TransferError::Busy("the layer is still recording")));
                    }
                    self.identity = Some(id);
                    self.samples = vec![0.0; id.frames];
                    let mut offset = 0;
                    while offset < id.frames {
                        let len = CHUNK_FRAMES.min(id.frames - offset);
                        self.pipe.queue.push_back(JobKind::Read(Chunk {
                            track: self.track,
                            slot: self.slot,
                            offset,
                            buf: vec![0.0; len].into_boxed_slice(),
                            epoch: 0,
                            result: None,
                        }));
                        self.remaining += 1;
                        offset += len;
                    }
                    self.stage = FetchStage::Reading;
                    if self.remaining == 0 {
                        self.stage = FetchStage::Confirming;
                        self.pipe.queue.push_back(JobKind::Probe { track: self.track, slot: self.slot, answer: None });
                    }
                }
                (FetchStage::Reading, JobKind::Read(chunk)) => {
                    if let Some(Err(r)) = chunk.result {
                        return Poll::Ready(Err(TransferError::Rejected(r)));
                    }
                    self.samples[chunk.offset..chunk.offset + chunk.buf.len()].copy_from_slice(&chunk.buf);
                    self.remaining -= 1;
                    if self.remaining == 0 {
                        self.stage = FetchStage::Confirming;
                        self.pipe.queue.push_back(JobKind::Probe { track: self.track, slot: self.slot, answer: None });
                    }
                }
                (FetchStage::Confirming, JobKind::Probe { answer, .. }) => {
                    let same = matches!(answer, Some(Ok(id)) if Some(id) == self.identity);
                    return Poll::Ready(match (same, self.identity) {
                        (true, Some(identity)) => {
                            Ok(FetchedLayer { identity, samples: std::mem::take(&mut self.samples) })
                        }
                        _ => Err(TransferError::Busy("the layer changed while it was being copied")),
                    });
                }
                _ => {}
            }
            self.pipe.pump(&mut h.jobs);
        }
        if self.pipe.stalled() {
            return Poll::Ready(Err(TransferError::Stalled));
        }
        Poll::Pending
    }
}

/// Probe positions spread over `samples` and their values.
pub fn swap_probes(samples: &[f32]) -> [(u32, f32); SWAP_PROBES] {
    let mut out = [(0u32, 0.0f32); SWAP_PROBES];
    if samples.is_empty() {
        return out;
    }
    for (i, slot) in out.iter_mut().enumerate() {
        let at = ((2 * i + 1) * samples.len() / (2 * SWAP_PROBES)).min(samples.len() - 1);
        *slot = (at as u32, samples[at]);
    }
    out
}

/// Replaces one finished layer's samples (see the module docs). Ready with
/// the new buffer's address (its identity from now on).
pub struct LayerSwap {
    pipe: Pipe,
    new_ptr: usize,
}

impl LayerSwap {
    /// `content` is padded with zeros to `buf_len` (the pool buffer length).
    pub fn new(
        track: u8,
        slot: LayerSlot,
        expect_ptr: usize,
        probes: [(u32, f32); SWAP_PROBES],
        content: &[f32],
        buf_len: usize,
    ) -> Self {
        let mut buf = vec![0.0f32; buf_len.max(content.len())];
        buf[..content.len()].copy_from_slice(content);
        let buf = buf.into_boxed_slice();
        let new_ptr = buf.as_ptr() as usize;
        let mut pipe = Pipe::new();
        pipe.queue.push_back(JobKind::Swap(Box::new(SwapJob { track, slot, expect_ptr, probes, buf, result: None })));
        Self { pipe, new_ptr }
    }

    pub fn poll(&mut self, h: &mut LooperHandles) -> Poll<usize> {
        self.pipe.pump(&mut h.jobs);
        while let Some(kind) = self.pipe.recv(&mut h.job_results) {
            if let JobKind::Swap(job) = kind {
                // `job.buf` now holds the old layer (or our unused buffer); it drops here.
                return Poll::Ready(match job.result {
                    Some(Ok(())) => Ok(self.new_ptr),
                    Some(Err(r)) => Err(TransferError::Rejected(r)),
                    None => Err(TransferError::Stalled),
                });
            }
        }
        if self.pipe.stalled() {
            return Poll::Ready(Err(TransferError::Stalled));
        }
        Poll::Pending
    }
}

fn recycle(recycler: &mut LayerRecycler) {
    recycler.service();
}

/// One track of a parked looper.
#[derive(Debug)]
pub struct ParkedTrack {
    pub plan: TrackPlan,
    pub layers: Vec<LayerBuf>,
    pub redo: Vec<LayerBuf>,
}

/// Everything a looper held when its stream was torn down: the layer
/// buffers themselves (moved, full `max_frames` length), mixer state,
/// transport, loop and latency. Restore it with [`ParkedLooper::into_captured`]
/// and an [`Importer`] (resampling first if the new stream runs at another rate).
#[derive(Debug)]
pub struct ParkedLooper {
    pub transport: TransportState,
    pub period: Option<f64>,
    pub latency: u32,
    pub max_frames: usize,
    pub max_layers: usize,
    pub track_capacity: usize,
    pub tracks: Vec<ParkedTrack>,
}

impl ParkedLooper {
    pub fn has_content(&self) -> bool {
        self.tracks.iter().any(|t| !t.layers.is_empty() || !t.redo.is_empty())
    }

    /// The same shape an export produces: layers trimmed to `ceil(period)`.
    pub fn into_captured(self) -> Captured {
        let mut tracks_plan = [TrackPlan::EMPTY; MAX_TRACKS];
        for (slot, t) in tracks_plan.iter_mut().zip(self.tracks.iter()) {
            *slot = t.plan;
        }
        let inventory = Inventory {
            epoch: 0,
            busy: false,
            run: RunState::Stopped,
            transport: self.transport,
            period: self.period,
            latency: self.latency,
            max_frames: self.max_frames,
            max_layers: self.max_layers,
            track_count: self.tracks.len().min(MAX_TRACKS) as u8,
            track_capacity: self.track_capacity.min(MAX_TRACKS) as u8,
            tracks: tracks_plan,
        };
        let cut = |bufs: Vec<LayerBuf>, frames: usize| -> Vec<Vec<f32>> {
            bufs.into_iter().map(|b| b.get(..frames).unwrap_or(&b[..]).to_vec()).collect()
        };
        let tracks = self
            .tracks
            .into_iter()
            .map(|t| {
                let frames = inventory.track_frames(&t.plan);
                CapturedTrack { plan: t.plan, layers: cut(t.layers, frames), redo: cut(t.redo, frames) }
            })
            .collect();
        Captured { inventory, tracks }
    }
}
