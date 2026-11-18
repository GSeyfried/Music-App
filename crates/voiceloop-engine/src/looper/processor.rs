//! `LooperCore`: the realtime looper, run inside the output callback.
//!
//! **Clock.** `t` counts transport frames and advances by exactly one per
//! output frame while running. It is the only musical time base: recording
//! boundaries, loop phase, and metronome clicks are all integer comparisons
//! against `t`. Nothing here reads wall-clock time.
//!
//! **Master loop.** All tracks share one loop: `origin` (transport frame of
//! phase 0) and an exact `period` in frames (integer in Free mode, fractional
//! in Metronome mode). Cycle `c` starts at `origin + round(c * period)`, which
//! is computed absolutely, so repeated cycles never drift.
//!
//! **Recording.** A recording writes input into the newest layer of a track
//! between `start` and `end`, which are *performance-time* frames. With input
//! latency `lat`, the input frame arriving at transport frame `t` was
//! performed at `t - lat` and is written at the loop phase of `t - lat`.
//! Layers start zeroed, so writes accumulate (`+=`), which makes multi-pass
//! overdubs sound-on-sound. Undo pops the newest layer; the base take is never
//! modified by overdubs.
//!
//! **Realtime rules.** `process` and everything it calls: no allocation, no
//! locks, no I/O, no panics on bad input (indices are bounds-checked with `get`).

use std::sync::mpsc;

use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize, Serializer};

use super::click::{accent_for, db_to_gain, ClickVoice};
use super::pool::{self, LayerBuf, LayerRecycler, LayerSource};
use super::transfer::{
    Chunk, Inventory, JobKind, LayerJob, LayerSlot, ParkedLooper, ParkedTrack, RestorePlan, TrackClock, TrackPlan,
    CHUNK_FRAMES, JOBS_PER_BLOCK, JOB_CAPACITY,
};
use super::{
    LayerTimingConfig, LooperConfig, LAYER_SUMMARY_BINS, MAX_LAYERS_PER_TRACK, MAX_POOL_LAYERS, MAX_TIMING_HITS,
    MAX_TRACKS,
};
use crate::error::EngineError;
use crate::model::LoopMode;
use crate::transport::{
    first_grid_index_at_or_after, grid_frame, LoopFit, MusicalPosition, OverdubStart, TrackLoop, TrackLoopMode,
    TransportState,
};

const COMMAND_CAPACITY: usize = 64;
const SNAPSHOT_CAPACITY: usize = 8;
const NOTICE_CAPACITY: usize = 32;
const TIMING_CAPACITY: usize = MAX_POOL_LAYERS;

/// The transport stops itself here instead of approaching `i64` overflow.
/// 2^52 frames is ~2,900 years at 48 kHz, and every frame below it is exact
/// in `f64`, which the grid and loop arithmetic rely on.
pub const MAX_TRANSPORT_FRAME: i64 = 1 << 52;

/// Commands from the control thread. `Copy`, so sending never allocates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LooperCommand {
    /// Replace transport settings. Timing changes are refused while loops exist.
    SetTransport {
        transport: TransportState,
    },
    Play,
    /// Stop and rewind to the loop start. Finishes or cancels recordings.
    Stop,
    Pause,
    Resume,
    /// Jump to the loop start and keep running (no count-in). Finishes or
    /// cancels recordings exactly like `Stop`.
    Restart,
    /// Record a new loop on an empty track. Free mode: starts now (the first
    /// take sets the loop length). Metronome mode: starts at the next loop boundary
    /// and lasts exactly one loop.
    Record {
        track: u8,
    },
    /// Add a layer on a track that has content. Free mode: starts now.
    /// Metronome mode: starts at the next bar.
    Overdub {
        track: u8,
    },
    /// End the active recording/overdub (Metronome mode: at the next bar).
    StopRecording {
        track: u8,
    },
    SetTrackPlaying {
        track: u8,
        playing: bool,
    },
    SetMute {
        track: u8,
        muted: bool,
    },
    /// While any track is soloed, only soloed tracks are audible.
    SetSolo {
        track: u8,
        solo: bool,
    },
    /// Linear gain, clamped to 0..=2.
    SetGain {
        track: u8,
        gain: f32,
    },
    /// Remove the most recent overdub layer.
    Undo {
        track: u8,
    },
    /// Restore the overdub layer most recently removed by `Undo`. A new
    /// recording on the track discards its redo history.
    Redo {
        track: u8,
    },
    /// Shift one immutable layer on the loop clock. Positive frames play later.
    NudgeLayer {
        track: u8,
        layer: u8,
        frames: i64,
    },
    /// Activate one more track (preallocated, so no stream restart). It is
    /// appended after the existing tracks. Rejected with `trackLimit` at capacity.
    AddTrack,
    /// Remove a track: its layers and redo history are released and later
    /// tracks move down one index. Rejected while it records (`trackBusy`)
    /// or when it is the only track (`trackLimit`).
    RemoveTrack {
        track: u8,
    },
    /// Remove all layers on the track.
    Clear {
        track: u8,
    },
    /// Change one track's loop settings. An empty track accepts any valid
    /// settings. On a track with content the mode cannot change
    /// (`trackNotEmpty`) and the length (bars, beats, ratio) changes only
    /// while the transport is stopped (`transportLocked`); a length change
    /// changes how the loop repeats, never its samples. Count-in, overdub
    /// start and fit can change at any time.
    SetTrackLoop {
        track: u8,
        settings: TrackLoop,
    },
    /// Add a track right after `track` with the same loop settings, clock,
    /// gain, mute, play state and layer offsets, and copy its active layers.
    /// The copy is spread over callbacks (at most `CHUNK_FRAMES` frames per
    /// block); both tracks report `busy` until it is done.
    DuplicateTrack {
        track: u8,
    },
    ClearAll,
    /// Round-trip latency compensation for recordings that follow a reference.
    SetInputLatency {
        frames: u32,
    },
    /// The input stream skipped or repeated time (bridge underrun, skip, or
    /// input overrun). Takes recording now are flagged `timingSuspect`.
    /// Internal: sent by the output callback through [`LooperCore::apply_now`],
    /// never accepted from the UI.
    #[serde(skip)]
    InputDiscontinuity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RejectReason {
    NoSuchTrack,
    TrackBusy,
    TrackNotEmpty,
    TrackEmpty,
    NotRecording,
    NothingToUndo,
    NothingToRedo,
    LayerLimit,
    PoolExhausted,
    TransportLocked,
    InvalidTransport,
    LoopTooLong,
    TooShort,
    /// No more tracks can be added, or the last track cannot be removed.
    TrackLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LooperNotice {
    Rejected {
        reason: RejectReason,
    },
    /// A free-mode first take hit `max_loop_seconds` and was closed automatically.
    MaxLengthReached {
        track: u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Stopped,
    CountIn,
    Running,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackState {
    Empty,
    /// Recording scheduled for a future boundary.
    Armed,
    Recording,
    Overdubbing,
    Playing,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackSnapshot {
    pub state: TrackState,
    pub layers: u8,
    pub muted: bool,
    pub solo: bool,
    pub playing: bool,
    pub gain: f32,
    /// Undone overdub layers that `Redo` can restore.
    pub redo_layers: u8,
    pub record_start: Option<i64>,
    pub record_end: Option<i64>,
    /// An input discontinuity happened while this track recorded its newest
    /// take: that take may be shifted in time. Cleared by the next take or Clear.
    pub timing_suspect: bool,
    /// Signed playback offsets for the active layers, in frames.
    pub layer_offsets: LayerOffsets,
    /// This track's loop settings.
    #[serde(rename = "loop")]
    pub track_loop: TrackLoop,
    /// The track's own loop period in frames (`None`: no loop yet).
    pub loop_period: Option<f64>,
    /// Frames into the track's own current cycle.
    pub loop_phase: u64,
    /// A duplicate is being copied from or into this track: commands that
    /// change its layers are refused (`trackBusy`), and a copy is silent.
    pub busy: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerOffsets {
    len: u8,
    values: [i64; MAX_LAYERS_PER_TRACK],
}

impl LayerOffsets {
    const EMPTY: Self = Self { len: 0, values: [0; MAX_LAYERS_PER_TRACK] };
    fn from_slice(values: &[i64]) -> Self {
        let mut out = Self::EMPTY;
        out.len = values.len().min(MAX_LAYERS_PER_TRACK) as u8;
        out.values[..out.len as usize].copy_from_slice(&values[..out.len as usize]);
        out
    }
}

impl Serialize for LayerOffsets {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.values[..self.len as usize].serialize(serializer)
    }
}

impl TrackSnapshot {
    pub const EMPTY: Self = Self {
        state: TrackState::Empty,
        layers: 0,
        muted: false,
        solo: false,
        playing: false,
        gain: 1.0,
        redo_layers: 0,
        record_start: None,
        record_end: None,
        timing_suspect: false,
        layer_offsets: LayerOffsets::EMPTY,
        track_loop: TrackLoop::DEFAULT,
        loop_period: None,
        loop_phase: 0,
        busy: false,
    };
}

/// Published after every processed block (dropped if the control thread lags).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LooperSnapshot {
    pub run: RunState,
    pub frame: i64,
    pub count_in_remaining: u64,
    pub position: MusicalPosition,
    pub loop_period: Option<f64>,
    pub loop_cycle: i64,
    pub loop_phase: u64,
    pub transport: TransportState,
    pub tracks: [TrackSnapshot; MAX_TRACKS],
    pub track_count: u8,
    pub track_capacity: u8,
    pub free_layers: u32,
    pub latency_frames: u32,
    pub frames_processed: u64,
    /// Changes whenever layer content or layer stacks change (unsaved-work tracking).
    pub epoch: u64,
}

/// Compact, UI-safe peak envelope for one immutable loop layer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerSummary {
    pub track: u8,
    /// Layer stack index; zero is the base take.
    pub layer: u8,
    #[serde(serialize_with = "serialize_summary_bins")]
    pub bins: [f32; LAYER_SUMMARY_BINS],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimingHit {
    pub beat: u32,
    pub offset_ms: f32,
}

/// Compact timing result for one completed layer; never contains raw audio.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerTiming {
    pub track: u8,
    pub layer: u8,
    #[serde(serialize_with = "serialize_timing_hits")]
    pub hits: [TimingHit; MAX_TIMING_HITS],
    #[serde(skip)]
    pub hit_count: u8,
    pub median_offset_ms: f32,
    pub suggested_nudge_frames: i64,
}

fn serialize_timing_hits<S>(hits: &[TimingHit; MAX_TIMING_HITS], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    // Unused entries are trailing defaults. The scan keeps valid hits packed.
    let len = hits.iter().rposition(|h| h.beat != 0 || h.offset_ms != 0.0).map_or(0, |i| i + 1);
    hits[..len].serialize(serializer)
}

fn serialize_summary_bins<S>(bins: &[f32; LAYER_SUMMARY_BINS], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    bins.as_slice().serialize(serializer)
}

/// Control-thread ends of the looper's rings.
pub struct LooperHandles {
    pub commands: Producer<LooperCommand>,
    pub snapshots: Consumer<LooperSnapshot>,
    pub notices: Consumer<LooperNotice>,
    /// Completed layer summaries; raw samples never leave the looper.
    pub summaries: Consumer<LayerSummary>,
    pub timings: Consumer<LayerTiming>,
    pub recycler: LayerRecycler,
    /// Layer export/import requests (see [`super::transfer`]); at most
    /// [`JOBS_PER_BLOCK`] are served per callback.
    pub jobs: Producer<LayerJob>,
    /// Answered jobs come back here, carrying their buffers.
    pub job_results: Consumer<LayerJob>,
    /// Receives the looper's content when the `LooperCore` is dropped (the
    /// output stream was torn down). Take it *before* stopping the stream.
    pub parked: Option<mpsc::Receiver<ParkedLooper>>,
}

/// The shared loop: phase 0 at `origin`, exact `period` in frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Master {
    pub origin: i64,
    pub period: f64,
}

impl Master {
    pub fn cycle_start(&self, cycle: i64) -> i64 {
        self.origin + (cycle as f64 * self.period).round() as i64
    }

    /// (cycle, phase index) of `frame`. Exact for any frame, including before `origin`.
    pub fn locate(&self, frame: i64) -> (i64, usize) {
        let rel = frame - self.origin;
        let mut cycle = (rel as f64 / self.period).floor() as i64;
        while self.cycle_start(cycle) > frame {
            cycle -= 1;
        }
        while self.cycle_start(cycle + 1) <= frame {
            cycle += 1;
        }
        (cycle, (frame - self.cycle_start(cycle)) as usize)
    }
}

/// A transport stop waiting for in-flight input: the performer stopped at
/// transport frame `t` (where this counter starts), but with input latency the last frames they
/// played arrive up to `lat` frames later. Arrival frame `t` keeps counting
/// (without playback) until every open recording has received its tail;
/// then the transport rewinds and enters `then`.
#[derive(Debug, Clone, Copy)]
struct Drain {
    t: i64,
    left: u64,
    then: RunState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecKind {
    /// Free mode, no loop yet: this take defines the period.
    FirstTake,
    /// A full new loop on an empty track, aligned to the existing loop.
    NewLoop,
    /// An additional layer on a track with content.
    Overdub,
}

#[derive(Debug, Clone, Copy)]
struct Recording {
    kind: RecKind,
    /// Performance-time window (frames on the transport clock).
    start: i64,
    end: Option<i64>,
    /// Input latency: the frame arriving at transport frame `t` was performed at `t - lat`.
    lat: i64,
}

#[derive(Debug, Clone, Copy)]
struct TimingScan {
    window_sum: f32,
    window_count: u32,
    previous_rms: f32,
    armed: bool,
    last_hit: i64,
    hits: [TimingHit; MAX_TIMING_HITS],
    hit_frames: [i64; MAX_TIMING_HITS],
    hit_count: u8,
}

impl TimingScan {
    const EMPTY: Self = Self {
        window_sum: 0.0,
        window_count: 0,
        previous_rms: 0.0,
        armed: true,
        last_hit: i64::MIN / 2,
        hits: [TimingHit { beat: 0, offset_ms: 0.0 }; MAX_TIMING_HITS],
        hit_frames: [0; MAX_TIMING_HITS],
        hit_count: 0,
    };
}

#[derive(Debug, Clone, Copy)]
enum SummaryState {
    Recording,
    Scanning { cursor: usize, bins: [f32; LAYER_SUMMARY_BINS], timing: TimingScan },
    Ready { bins: [f32; LAYER_SUMMARY_BINS], timing: Option<LayerTiming>, summary_emitted: bool, timing_emitted: bool },
}

struct Track {
    /// Base take first, then overdubs. Capacity fixed at construction.
    layers: Vec<LayerBuf>,
    /// Kept parallel with `layers`; inline arrays are allocated before audio starts.
    summaries: Vec<SummaryState>,
    /// Layers removed by Undo, newest last. Capacity fixed at construction.
    redo: Vec<LayerBuf>,
    /// Kept parallel with `redo`, so Undo/Redo preserves the exact summary.
    redo_summaries: Vec<SummaryState>,
    offsets: Vec<i64>,
    redo_offsets: Vec<i64>,
    rec: Option<Recording>,
    playing: bool,
    muted: bool,
    solo: bool,
    gain: f32,
    /// Smoothed gain actually applied; slews toward the audible target so
    /// mute, solo, level, and play/stop changes never click.
    level: f32,
    /// See [`TrackSnapshot::timing_suspect`].
    timing_suspect: bool,
    /// This track's loop clock (phase 0 and period). A `Song` track follows
    /// the song loop (`LooperCore::master`, kept in step by `set_master`);
    /// other modes have their own (docs/projects/looper-01/PER_TRACK_LOOPS.md).
    clock: Option<Master>,
    /// Loop settings (mode, length, count-in, overdub start, fit).
    lp: TrackLoop,
    /// A duplicate is being copied into this track: silent, and refuses edits.
    filling: bool,
}

impl Track {
    fn target_level(&self, any_solo: bool) -> f32 {
        if self.playing && !self.muted && (!any_solo || self.solo) {
            self.gain
        } else {
            0.0
        }
    }
}

/// A `DuplicateTrack` copy in progress: layer `layer` of track `src` is
/// copied into track `dst` from frame `offset`.
#[derive(Debug, Clone, Copy)]
struct DupJob {
    src: usize,
    dst: usize,
    layer: usize,
    offset: usize,
}

/// Output level above which the soft limiter starts to act (about -1.9 dBFS).
pub const LIMIT_KNEE: f32 = 0.8;

/// Soft limiter for the mix bus. Exactly transparent up to [`LIMIT_KNEE`];
/// above it the signal saturates smoothly toward ±1 instead of hard-clipping,
/// so stacked overdubs distort gently rather than crackle. Stateless (no
/// pumping, no lookahead latency). Non-finite input yields silence.
#[inline]
pub fn soft_limit(y: f32) -> f32 {
    let a = y.abs();
    if a <= LIMIT_KNEE {
        y
    } else if a.is_finite() {
        let head = 1.0 - LIMIT_KNEE;
        (LIMIT_KNEE + head * ((a - LIMIT_KNEE) / head).tanh()).copysign(y)
    } else {
        0.0
    }
}

pub struct LooperCore {
    sample_rate: u32,
    max_frames: usize,
    max_layers: usize,
    fade_frames: u32,
    capture_tail_frames: u32,
    min_loop_frames: usize,
    transport: TransportState,
    run: RunState,
    /// Transport frame: musical time since bar 1 beat 1.
    t: i64,
    count_in_total: u64,
    count_in_left: u64,
    master: Option<Master>,
    /// The base loop of `Multiple` tracks: set by the first Multiple take,
    /// kept until every Multiple track is empty.
    base: Option<Master>,
    /// A `DuplicateTrack` copy in progress (at most one).
    dup: Option<DupJob>,
    /// Active tracks, in UI order.
    tracks: Vec<Track>,
    /// Preallocated inactive tracks (empty, zero-layer).
    spare: Vec<Track>,
    any_solo: bool,
    /// Largest per-frame change of a track's applied gain.
    gain_slew: f32,
    latency: i64,
    click: ClickVoice,
    click_next_index: i64,
    click_next_frame: i64,
    pool: LayerSource,
    /// Set while a stop waits for in-flight input (commands wait meanwhile).
    drain: Option<Drain>,
    /// Bumped whenever layer content or the layer stacks change; exports
    /// compare it across chunks to prove they copied one consistent state.
    epoch: u64,
    /// A project restore is filling layers: nothing may play or record.
    loading: bool,
    job_rx: Consumer<LayerJob>,
    job_tx: Producer<LayerJob>,
    /// Where the content goes when this core is dropped (see `Drop`).
    park_tx: Option<mpsc::Sender<ParkedLooper>>,
    cmd_rx: Consumer<LooperCommand>,
    snap_tx: Producer<LooperSnapshot>,
    notice_tx: Producer<LooperNotice>,
    summary_tx: Producer<LayerSummary>,
    timing_tx: Producer<LayerTiming>,
    timing_config: LayerTimingConfig,
    frames_processed: u64,
    /// Publish a snapshot at most every this many frames (~10 ms).
    snapshot_interval: u64,
    frames_since_snapshot: u64,
}

fn validate_transport(t: &TransportState, max_frames: usize) -> Result<(), RejectReason> {
    t.validate().map_err(|_| RejectReason::InvalidTransport)?;
    if t.mode == LoopMode::Synced && t.loop_period().ceil() as usize > max_frames {
        return Err(RejectReason::LoopTooLong);
    }
    Ok(())
}

impl LooperCore {
    /// Allocates everything the looper will ever use. Call off the realtime thread.
    pub fn new(
        config: &LooperConfig,
        transport: TransportState,
        sample_rate: u32,
    ) -> Result<(Self, LooperHandles), EngineError> {
        let max_frames = config.max_loop_frames(sample_rate)?;
        let (pool, recycler) = pool::allocate(config.pool_layers, max_frames);
        let (cmd_tx, cmd_rx) = RingBuffer::new(COMMAND_CAPACITY);
        let (snap_tx, snap_rx) = RingBuffer::new(SNAPSHOT_CAPACITY);
        let (notice_tx, notice_rx) = RingBuffer::new(NOTICE_CAPACITY);
        let (summary_tx, summary_rx) = RingBuffer::new(MAX_POOL_LAYERS);
        let (timing_tx, timing_rx) = RingBuffer::new(TIMING_CAPACITY);
        let (jobs_tx, job_rx) = RingBuffer::new(JOB_CAPACITY);
        let (job_tx, results_rx) = RingBuffer::new(JOB_CAPACITY);
        let (park_tx, park_rx) = mpsc::channel();

        let mut transport = transport;
        transport.sample_rate = sample_rate;
        if validate_transport(&transport, max_frames).is_err() {
            transport = TransportState { sample_rate, ..TransportState::default() };
        }

        // Every track the looper can ever have is allocated now; `addTrack`
        // only moves one from `spare` to `tracks` (both within capacity).
        let mut tracks = Vec::with_capacity(config.track_capacity);
        let mut spare = Vec::with_capacity(config.track_capacity);
        for i in 0..config.track_capacity {
            let tr = Track {
                layers: Vec::with_capacity(config.max_layers_per_track),
                summaries: Vec::with_capacity(config.max_layers_per_track),
                redo: Vec::with_capacity(config.max_layers_per_track),
                redo_summaries: Vec::with_capacity(config.max_layers_per_track),
                offsets: Vec::with_capacity(config.max_layers_per_track),
                redo_offsets: Vec::with_capacity(config.max_layers_per_track),
                rec: None,
                playing: true,
                muted: false,
                solo: false,
                gain: 1.0,
                level: 1.0,
                timing_suspect: false,
                clock: None,
                lp: TrackLoop::DEFAULT,
                filling: false,
            };
            if i < config.max_tracks {
                tracks.push(tr);
            } else {
                spare.push(tr);
            }
        }

        let fade = config.fade_frames;
        let mut core = Self {
            sample_rate,
            max_frames,
            max_layers: config.max_layers_per_track,
            fade_frames: fade,
            capture_tail_frames: ((config.capture_tail_ms as u64 * sample_rate as u64) / 1_000).min(max_frames as u64)
                as u32,
            min_loop_frames: ((fade as usize) * 2).max(sample_rate as usize / 50).max(1),
            master: Self::synced_master(&transport),
            base: None,
            dup: None,
            transport,
            run: RunState::Stopped,
            t: 0,
            count_in_total: 0,
            count_in_left: 0,
            tracks,
            spare,
            any_solo: false,
            gain_slew: if config.gain_smoothing_frames == 0 {
                f32::INFINITY
            } else {
                1.0 / config.gain_smoothing_frames as f32
            },
            latency: 0,
            click: ClickVoice::new(sample_rate),
            click_next_index: 0,
            click_next_frame: 0,
            pool,
            drain: None,
            epoch: 0,
            loading: false,
            job_rx,
            job_tx,
            park_tx: Some(park_tx),
            cmd_rx,
            snap_tx,
            notice_tx,
            summary_tx,
            timing_tx,
            timing_config: config.timing,
            frames_processed: 0,
            snapshot_interval: (sample_rate as u64 / 100).max(1),
            // Publish on the first block so the UI sees state immediately.
            frames_since_snapshot: u64::MAX / 2,
        };
        core.set_master(core.master);
        let handles = LooperHandles {
            commands: cmd_tx,
            snapshots: snap_rx,
            notices: notice_rx,
            summaries: summary_rx,
            timings: timing_rx,
            recycler,
            jobs: jobs_tx,
            job_results: results_rx,
            parked: Some(park_rx),
        };
        Ok((core, handles))
    }

    /// Set the song loop, and the clock of every `Song` track with it (spare
    /// tracks too, so an added track joins in step). No allocation.
    fn set_master(&mut self, master: Option<Master>) {
        self.master = master;
        for tr in self.tracks.iter_mut() {
            if tr.lp.mode == TrackLoopMode::Song {
                tr.clock = master;
            }
        }
        for tr in self.spare.iter_mut() {
            tr.clock = master;
        }
    }

    /// The clock of a `Bars` track: phase 0 on bar 1 (frame 0) and an exact
    /// period of whole beats, so its cycle starts are absolute beat-grid
    /// frames (`round(n * period)`) that never drift from the click.
    fn bars_clock(&self, lp: &TrackLoop) -> Master {
        let beats = lp.beats_per_loop(self.transport.time_signature.numerator).max(1);
        Master { origin: 0, period: beats as f64 * self.transport.frames_per_beat() }
    }

    /// The period a track with settings `lp` would loop at, when it can be
    /// known before recording (Bars always; Multiple once the base exists).
    fn planned_period(&self, lp: &TrackLoop) -> Option<f64> {
        match lp.mode {
            TrackLoopMode::Bars => Some(self.bars_clock(lp).period),
            TrackLoopMode::Multiple => self.base.map(|b| lp.ratio * b.period),
            TrackLoopMode::Song | TrackLoopMode::Free => None,
        }
    }

    fn fits(&self, period: f64) -> bool {
        period.is_finite() && period >= 1.0 && period.ceil() as usize <= self.max_frames
    }

    /// Recompute the clocks that depend on the transport grid (Bars tracks).
    fn refresh_bars_clocks(&mut self) {
        for ti in 0..self.tracks.len() {
            let lp = self.tracks[ti].lp;
            if lp.mode == TrackLoopMode::Bars {
                self.tracks[ti].clock = Some(self.bars_clock(&lp));
            }
        }
    }

    fn track_busy(&self, ti: usize) -> bool {
        self.tracks.get(ti).is_some_and(|tr| tr.filling) || self.dup.is_some_and(|d| d.src == ti)
    }

    /// Any track in `mode` holds layers (a recording track holds one).
    fn mode_has_content(&self, mode: TrackLoopMode) -> bool {
        self.tracks.iter().any(|tr| tr.lp.mode == mode && !tr.layers.is_empty())
    }

    /// Forget clocks nothing depends on any more: the song loop in Free
    /// transport mode once no Song track has content, the Multiple base once
    /// no Multiple track has content, and the clock of an empty Multiple or
    /// Free track.
    fn release_unused_clocks(&mut self) {
        if self.transport.mode == LoopMode::Free && !self.mode_has_content(TrackLoopMode::Song) {
            self.set_master(None);
        }
        if !self.mode_has_content(TrackLoopMode::Multiple) {
            self.base = None;
        }
        for tr in &mut self.tracks {
            if tr.layers.is_empty() && matches!(tr.lp.mode, TrackLoopMode::Multiple | TrackLoopMode::Free) {
                tr.clock = None;
            }
        }
    }

    fn synced_master(t: &TransportState) -> Option<Master> {
        (t.mode == LoopMode::Synced).then(|| Master { origin: 0, period: t.loop_period() })
    }

    pub fn transport_frame(&self) -> i64 {
        self.t
    }

    pub fn frames_processed(&self) -> u64 {
        self.frames_processed
    }

    /// Test hook: jump the transport clock (used to exercise the overflow guard).
    #[cfg(test)]
    pub(crate) fn set_transport_frame(&mut self, t: i64) {
        self.t = t;
    }

    /// Test-only inspection of immutable captured source data. Production UI
    /// receives metadata only; raw audio never crosses the bridge.
    #[cfg(test)]
    pub(crate) fn layer_sample(&self, track: usize, layer: usize, frame: usize) -> Option<f32> {
        self.tracks.get(track)?.layers.get(layer)?.get(frame).copied()
    }

    /// Process one block of mono input into mono output. Realtime-safe.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        self.drain_commands();
        self.service_jobs();
        self.service_dup();
        let n = input.len().min(out.len());
        self.resync_click();
        for i in 0..n {
            let x = input[i];
            let mut y = 0.0f32;
            match self.run {
                RunState::CountIn => {
                    let pos = (self.count_in_total - self.count_in_left) as i64;
                    self.tick_click(pos, true);
                    self.count_in_left = self.count_in_left.saturating_sub(1);
                    if self.count_in_left == 0 {
                        self.run = RunState::Running;
                        self.resync_click();
                    }
                }
                RunState::Running => {
                    self.tick_click(self.t, false);
                    y += self.render(x);
                    self.t += 1;
                    if self.t >= MAX_TRANSPORT_FRAME {
                        self.stop(RunState::Stopped);
                    }
                }
                RunState::Stopped | RunState::Paused => {
                    if self.drain.is_some() {
                        self.drain_frame(x);
                    }
                }
            }
            y += self.click.next_sample();
            out[i] = soft_limit(y);
        }
        for o in out.iter_mut().skip(n) {
            *o = 0.0;
        }
        if self.drain.is_some() || self.any_recording() {
            self.touch(); // recorded samples changed layer content
        }
        self.service_summaries();
        self.frames_processed = self.frames_processed.saturating_add(n as u64);
        self.frames_since_snapshot = self.frames_since_snapshot.saturating_add(n as u64);
        if self.frames_since_snapshot >= self.snapshot_interval {
            self.frames_since_snapshot = 0;
            self.publish();
        }
    }

    // ---- per-frame work -------------------------------------------------

    /// Playback is read *before* this frame's recording writes, so a sample
    /// is never heard in the frame it was recorded (no implicit monitoring).
    fn render(&mut self, x: f32) -> f32 {
        let t = self.t;
        let mut y = 0.0f32;
        {
            let (any_solo, slew) = (self.any_solo, self.gain_slew);
            for tr in &mut self.tracks {
                // A copy being filled stays silent and fades in once complete.
                let target = if tr.filling { 0.0 } else { tr.target_level(any_solo) };
                let d = target - tr.level;
                if d != 0.0 {
                    tr.level = if d.abs() <= slew { target } else { tr.level + slew.copysign(d) };
                }
                let Some(m) = tr.clock else { continue };
                let (_, idx) = m.locate(t);
                if tr.level > 0.0 {
                    let mut s = 0.0f32;
                    let frames = m.period.ceil().max(1.0) as i64;
                    for (layer_index, layer) in tr.layers.iter().enumerate() {
                        let offset = tr.offsets.get(layer_index).copied().unwrap_or(0);
                        let source = (idx as i64 - offset).rem_euclid(frames) as usize;
                        s += layer.get(source).copied().unwrap_or(0.0);
                    }
                    y += s * tr.level;
                }
            }
        }
        for ti in 0..self.tracks.len() {
            if let Some(rec) = self.tracks[ti].rec {
                self.advance_recording(ti, rec, x, t);
            }
        }
        y
    }

    fn advance_recording(&mut self, ti: usize, rec: Recording, x: f32, t: i64) {
        let tp = t - rec.lat;
        if tp < rec.start {
            return; // armed
        }
        if let Some(end) = rec.end {
            if tp >= end {
                self.complete_recording(ti);
                return;
            }
        }
        let idx = match rec.kind {
            RecKind::FirstTake => {
                let i = (tp - rec.start) as usize;
                if i >= self.max_frames {
                    if self.tracks[ti].clock.is_some() {
                        self.complete_recording(ti);
                    } else {
                        self.finish_first_take(ti, tp, false);
                        let _ = self.notice_tx.push(LooperNotice::MaxLengthReached { track: ti as u8 });
                    }
                    return;
                }
                i
            }
            RecKind::NewLoop | RecKind::Overdub => match self.tracks[ti].clock {
                Some(m) => m.locate(tp).1,
                None => return,
            },
        };
        let g = self.ramp(&rec, tp, self.tracks[ti].clock);
        if let Some(layer) = self.tracks[ti].layers.last_mut() {
            if let Some(s) = layer.get_mut(idx) {
                *s += x * g;
            }
        }
    }

    /// Edge fade for click-free recording boundaries. Timing is unchanged.
    fn ramp(&self, rec: &Recording, tp: i64, clock: Option<Master>) -> f32 {
        if self.fade_frames == 0 {
            return 1.0;
        }
        let f = self.fade_frames as f32;
        let mut g = ((tp - rec.start + 1) as f32 / f).min(1.0);
        if let Some(end) = rec.end {
            g = g.min(((end - tp) as f32 / f).min(1.0));
        }
        // A fitted first take may still be capturing a hidden safety tail.
        // Fade the audible loop boundary without fading or deleting the raw
        // post-roll stored beyond that boundary.
        if rec.kind == RecKind::FirstTake {
            if let Some(master) = clock {
                let loop_end = rec.start + master.period.round() as i64;
                if tp < loop_end {
                    g = g.min(((loop_end - tp) as f32 / f).min(1.0));
                }
            }
        }
        g.max(0.0)
    }

    /// Mark layer content or layer stacks as changed.
    fn touch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    fn complete_recording(&mut self, ti: usize) {
        self.touch();
        if let Some(tr) = self.tracks.get_mut(ti) {
            tr.rec = None;
            tr.playing = true;
            if let Some(summary) = tr.summaries.last_mut() {
                *summary =
                    SummaryState::Scanning { cursor: 0, bins: [0.0; LAYER_SUMMARY_BINS], timing: TimingScan::EMPTY };
            }
        }
    }

    /// Completed buffers are immutable, so their exact peak envelope can be
    /// scanned in bounded chunks after recording. This avoids per-sample
    /// approximation (important for cancelling multi-pass overdubs) while the
    /// callback still does no allocation, locking, blocking, logging, or I/O.
    fn service_summaries(&mut self) {
        if self.loading {
            return;
        }
        let beat_frames = self.transport.frames_per_beat();
        let window_frames = self.timing_config.window_frames;
        let spacing_frames = (self.timing_config.minimum_spacing_ms as u64 * self.sample_rate as u64 / 1_000) as i64;
        let max_hits = self.timing_config.max_hits.min(MAX_TIMING_HITS);

        let mut budget = CHUNK_FRAMES;
        for (track_index, tr) in self.tracks.iter_mut().enumerate() {
            // Each track is scanned over its own loop; a copy once it is complete.
            let Some(master) = tr.clock else { continue };
            if tr.filling {
                continue;
            }
            let frames = (master.period.ceil().max(0.0) as usize).min(self.max_frames);
            if frames == 0 {
                continue;
            }
            let beats = (master.period / beat_frames).round() as i64;
            let timing_enabled = beats > 0
                && (master.period - beats as f64 * beat_frames).abs() <= self.timing_config.whole_beat_tolerance_frames;
            for (layer_index, (layer, state)) in tr.layers.iter().zip(tr.summaries.iter_mut()).enumerate() {
                if budget == 0 {
                    break;
                }
                let SummaryState::Scanning { cursor, bins, timing } = state else { continue };
                let end = cursor.saturating_add(budget).min(frames);
                for index in *cursor..end {
                    let bin = index * LAYER_SUMMARY_BINS / frames;
                    let raw = layer.get(index).copied().unwrap_or(0.0);
                    let sample = if raw.is_finite() { raw } else { 0.0 };
                    let peak = sample.abs().min(1.0);
                    bins[bin] = bins[bin].max(peak);
                    if timing_enabled {
                        timing.window_sum += sample * sample;
                        timing.window_count += 1;
                        if timing.window_count == window_frames || index + 1 == frames {
                            let rms = (timing.window_sum / timing.window_count.max(1) as f32).sqrt();
                            let center = index as i64 - timing.window_count as i64 / 2;
                            if timing.armed
                                && rms >= self.timing_config.minimum_rms
                                && rms >= timing.previous_rms.max(1.0e-6) * self.timing_config.rise_ratio
                                && center - timing.last_hit >= spacing_frames
                                && (timing.hit_count as usize) < max_hits
                            {
                                let nearest = (center as f64 / beat_frames).round() as i64;
                                let offset_frames = center - grid_frame(nearest, beat_frames);
                                let slot = timing.hit_count as usize;
                                timing.hits[slot] = TimingHit {
                                    beat: nearest.rem_euclid(beats) as u32 + 1,
                                    offset_ms: offset_frames as f32 * 1_000.0 / self.sample_rate as f32,
                                };
                                timing.hit_frames[slot] = offset_frames;
                                timing.hit_count += 1;
                                timing.last_hit = center;
                                timing.armed = false;
                            } else if !timing.armed
                                && (rms < self.timing_config.minimum_rms
                                    || rms <= timing.previous_rms * self.timing_config.release_ratio)
                            {
                                timing.armed = true;
                            }
                            timing.previous_rms = rms;
                            timing.window_sum = 0.0;
                            timing.window_count = 0;
                        }
                    }
                }
                budget -= end - *cursor;
                *cursor = end;
                if end == frames {
                    let result = if timing_enabled && timing.hit_count > 0 {
                        let count = timing.hit_count as usize;
                        let mut sorted = timing.hit_frames;
                        sorted[..count].sort_unstable();
                        let median_frames = if count % 2 == 1 {
                            sorted[count / 2] as f64
                        } else {
                            (sorted[count / 2 - 1] as f64 + sorted[count / 2] as f64) / 2.0
                        };
                        Some(LayerTiming {
                            track: track_index as u8,
                            layer: layer_index as u8,
                            hits: timing.hits,
                            hit_count: timing.hit_count,
                            median_offset_ms: (median_frames * 1_000.0 / self.sample_rate as f64) as f32,
                            suggested_nudge_frames: (-median_frames).round() as i64,
                        })
                    } else {
                        None
                    };
                    *state = SummaryState::Ready {
                        bins: *bins,
                        timing: result,
                        summary_emitted: false,
                        timing_emitted: false,
                    };
                }
            }
        }

        // Ready summaries remain queued in-place when the ring is full, so an
        // event is never silently lost. Redo marks its preserved state un-emitted.
        for (track, tr) in self.tracks.iter_mut().enumerate() {
            for (layer, state) in tr.summaries.iter_mut().enumerate() {
                let SummaryState::Ready { bins, timing, summary_emitted, timing_emitted } = state else { continue };
                if !*summary_emitted {
                    let event = LayerSummary { track: track as u8, layer: layer as u8, bins: *bins };
                    if self.summary_tx.push(event).is_err() {
                        return;
                    }
                    *summary_emitted = true;
                }
                if let Some(mut event) = *timing {
                    if !*timing_emitted {
                        event.track = track as u8;
                        event.layer = layer as u8;
                        if self.timing_tx.push(event).is_err() {
                            return;
                        }
                        *timing_emitted = true;
                    }
                }
            }
        }
    }

    /// Close a free-mode first take whose performance ended at frame `end`
    /// and establish the loop (`period = end - start`, press to press).
    ///
    /// With input latency, the last `lat` frames of the performance have not
    /// arrived yet. When `keep_tail` is set the recording stays open as a
    /// bounded take until they have; playback of the new loop starts at once
    /// and reaches those frames only a full period later.
    fn finish_first_take(&mut self, ti: usize, raw_end: i64, keep_tail: bool) {
        let Some(rec) = self.tracks.get(ti).and_then(|tr| tr.rec) else { return };
        self.touch();
        let raw_len = (raw_end - rec.start).max(0) as usize;
        let lp = self.tracks[ti].lp;
        let fit = if lp.mode == TrackLoopMode::Song { self.transport.loop_fit } else { lp.loop_fit };
        let len = if keep_tail { self.fit_first_take_frames(raw_len, fit) } else { raw_len }.min(self.max_frames);
        if len < self.min_loop_frames {
            self.discard_recording(ti);
            let _ = self.notice_tx.push(LooperNotice::Rejected { reason: RejectReason::TooShort });
            return;
        }
        // Frames [0, written) are already in the layer.
        let written = (self.t - rec.lat - rec.start).clamp(0, len as i64) as usize;
        let fade = self.fade_frames as usize;
        if fade > 0 {
            if let Some(layer) = self.tracks[ti].layers.last_mut() {
                // Fade the part of the tail that was written at full level. It
                // matches `ramp`, which fades the frames still to come. A first
                // take is a single pass into a zeroed buffer, so only this take
                // is scaled.
                for i in len.saturating_sub(fade)..written {
                    if let Some(s) = layer.get_mut(i) {
                        *s *= (len - i) as f32 / fade as f32;
                    }
                }
            }
        }
        let clock = Master { origin: rec.start, period: len as f64 };
        match lp.mode {
            TrackLoopMode::Song => self.set_master(Some(clock)),
            TrackLoopMode::Multiple => {
                // The take is `ratio` base loops long.
                self.base = Some(Master { origin: rec.start, period: len as f64 / lp.ratio });
                self.tracks[ti].clock = Some(clock);
            }
            TrackLoopMode::Free | TrackLoopMode::Bars => self.tracks[ti].clock = Some(clock),
        }
        let capture_end = raw_end
            .max(rec.start + len as i64)
            .saturating_add(self.capture_tail_frames as i64)
            .min(rec.start + self.max_frames as i64);
        if keep_tail && self.t - rec.lat < capture_end {
            self.tracks[ti].rec =
                Some(Recording { kind: RecKind::FirstTake, start: rec.start, end: Some(capture_end), lat: rec.lat });
        } else {
            self.complete_recording(ti);
        }
    }

    /// Choose a repeat length without incrementally adding rounded beats.
    /// The selected length is relative to the take start, so a musician may
    /// begin anywhere on the transport and still receive an exact N-beat loop.
    fn fit_first_take_frames(&self, raw_frames: usize, fit: LoopFit) -> usize {
        if fit == LoopFit::Exact {
            return raw_frames;
        }
        let beat = self.transport.frames_per_beat();
        let beats = raw_frames as f64 / beat;
        let whole = match fit {
            LoopFit::Exact => unreachable!(),
            LoopFit::NearestBeat => beats.round(),
            LoopFit::RoundDown => beats.floor(),
            LoopFit::RoundUp => beats.ceil(),
        }
        .max(1.0) as i64;
        grid_frame(whole, beat).max(1) as usize
    }

    /// The transport stops while a recording is open: end it at performance
    /// frame `end` (the stop point). Frames already in the layer that fall in
    /// the last `fade_frames` before `end` are faded now; frames still in
    /// flight get the same curve from [`Self::ramp`] when they arrive, so the
    /// fade is identical with or without input latency. Multi-pass overdubs
    /// are not faded retroactively: scaling would also scale earlier passes.
    fn cut_recording(&mut self, ti: usize, rec: Recording, end: i64) {
        self.touch();
        if let Some(r) = self.tracks.get_mut(ti).and_then(|tr| tr.rec.as_mut()) {
            r.end = Some(end);
        }
        let Some(m) = self.tracks.get(ti).and_then(|tr| tr.clock) else { return };
        let span = end - rec.start;
        if self.fade_frames == 0 || span <= 0 || span as f64 > m.period.floor() {
            return;
        }
        let fade = self.fade_frames as i64;
        let written_end = (self.t - rec.lat).min(end);
        let Some(layer) = self.tracks.get_mut(ti).and_then(|tr| tr.layers.last_mut()) else { return };
        for tp in (end - fade).max(rec.start)..written_end {
            let (_, idx) = m.locate(tp);
            if let Some(s) = layer.get_mut(idx) {
                *s *= (end - tp) as f32 / fade as f32;
            }
        }
    }

    /// One arrival frame while a stop drains in-flight input.
    fn drain_frame(&mut self, x: f32) {
        let Some(mut d) = self.drain else { return };
        for ti in 0..self.tracks.len() {
            if let Some(rec) = self.tracks[ti].rec {
                self.advance_recording(ti, rec, x, d.t);
            }
        }
        d.t += 1;
        d.left = d.left.saturating_sub(1);
        if d.left == 0 || !self.any_recording() {
            self.drain = None;
            self.finish_stop(d.then);
        } else {
            self.drain = Some(d);
        }
    }

    fn discard_recording(&mut self, ti: usize) {
        self.touch();
        if let Some(tr) = self.tracks.get_mut(ti) {
            if tr.rec.take().is_some() {
                if let Some(buf) = tr.layers.pop() {
                    self.pool.release(buf);
                }
                tr.summaries.pop();
                tr.offsets.pop();
            }
        }
        self.release_unused_clocks();
    }

    // ---- metronome --------------------------------------------------------

    fn click_position(&self) -> i64 {
        match self.run {
            RunState::CountIn => (self.count_in_total - self.count_in_left) as i64,
            _ => self.t,
        }
    }

    fn resync_click(&mut self) {
        let step = self.transport.frames_per_subdivision();
        let n = first_grid_index_at_or_after(self.click_position(), step);
        self.click_next_index = n;
        self.click_next_frame = grid_frame(n, step);
    }

    fn tick_click(&mut self, pos: i64, counting_in: bool) {
        let audible = counting_in || (self.transport.mode == LoopMode::Synced && self.transport.metronome.enabled);
        if !audible {
            return;
        }
        let step = self.transport.frames_per_subdivision();
        if pos > self.click_next_frame {
            self.click_next_index = first_grid_index_at_or_after(pos, step);
            self.click_next_frame = grid_frame(self.click_next_index, step);
        }
        if pos == self.click_next_frame {
            let accent = accent_for(self.click_next_index, &self.transport);
            self.click.trigger(accent, db_to_gain(self.transport.metronome.gain_db));
            self.click_next_index = self.click_next_index.saturating_add(1);
            self.click_next_frame = grid_frame(self.click_next_index, step);
        }
    }

    // ---- commands -----------------------------------------------------------

    fn drain_commands(&mut self) {
        if self.drain.is_some() {
            return; // a stop is collecting its tail: later commands wait (at most `latency` frames)
        }
        while let Ok(cmd) = self.cmd_rx.pop() {
            if self.loading {
                let _ = self.notice_tx.push(LooperNotice::Rejected { reason: RejectReason::TrackBusy });
                continue;
            }
            if let Err(reason) = self.apply(cmd) {
                let _ = self.notice_tx.push(LooperNotice::Rejected { reason });
            }
        }
    }

    /// Apply a command immediately, from the thread that owns the core (the
    /// output callback). Realtime-safe; a rejection becomes a notice.
    pub fn apply_now(&mut self, cmd: LooperCommand) {
        if let Err(reason) = self.apply(cmd) {
            let _ = self.notice_tx.push(LooperNotice::Rejected { reason });
        }
    }

    fn apply(&mut self, cmd: LooperCommand) -> Result<(), RejectReason> {
        use LooperCommand as C;
        match cmd {
            C::SetTransport { transport } => self.set_transport(transport),
            C::Play => {
                self.play();
                Ok(())
            }
            C::Stop => {
                self.stop(RunState::Stopped);
                Ok(())
            }
            C::Pause => self.pause(),
            C::Resume => {
                if self.run == RunState::Paused {
                    self.run = RunState::Running;
                }
                Ok(())
            }
            C::Restart => {
                self.stop(RunState::Running);
                Ok(())
            }
            C::Record { track } => self.record(track as usize),
            C::Overdub { track } => self.overdub(track as usize),
            C::StopRecording { track } => self.stop_recording(track as usize),
            C::SetTrackPlaying { track, playing } => self.with_track(track as usize, |tr| tr.playing = playing),
            C::SetMute { track, muted } => self.with_track(track as usize, |tr| tr.muted = muted),
            C::SetSolo { track, solo } => {
                self.with_track(track as usize, |tr| tr.solo = solo)?;
                self.any_solo = self.tracks.iter().any(|tr| tr.solo);
                Ok(())
            }
            C::SetGain { track, gain } => {
                let g = if gain.is_finite() { gain.clamp(0.0, 2.0) } else { 1.0 };
                self.with_track(track as usize, |tr| tr.gain = g)
            }
            C::Undo { track } => self.undo(track as usize),
            C::Redo { track } => self.redo(track as usize),
            C::NudgeLayer { track, layer, frames } => self.nudge_layer(track as usize, layer as usize, frames),
            C::Clear { track } => self.clear(track as usize),
            C::SetTrackLoop { track, settings } => self.set_track_loop(track as usize, settings),
            C::DuplicateTrack { track } => self.duplicate_track(track as usize),
            C::AddTrack => self.add_track(),
            C::RemoveTrack { track } => self.remove_track(track as usize),
            C::ClearAll => {
                for ti in 0..self.tracks.len() {
                    let _ = self.clear(ti);
                }
                Ok(())
            }
            C::InputDiscontinuity => {
                for tr in &mut self.tracks {
                    if tr.rec.is_some() {
                        tr.timing_suspect = true;
                    }
                }
                Ok(())
            }
            C::SetInputLatency { frames } => {
                self.latency = (frames as i64).min(self.max_frames as i64 / 2);
                Ok(())
            }
        }
    }

    fn with_track(&mut self, ti: usize, f: impl FnOnce(&mut Track)) -> Result<(), RejectReason> {
        let tr = self.tracks.get_mut(ti).ok_or(RejectReason::NoSuchTrack)?;
        f(tr);
        Ok(())
    }

    fn any_recording(&self) -> bool {
        self.tracks.iter().any(|tr| tr.rec.is_some())
    }

    fn set_transport(&mut self, mut new: TransportState) -> Result<(), RejectReason> {
        new.sample_rate = self.sample_rate;
        new.position = self.transport.position;
        new.play_state = self.transport.play_state;
        validate_transport(&new, self.max_frames)?;
        let old = self.transport;
        let mode_changed = new.mode != old.mode;
        let clock_grid_changed = new.bpm != old.bpm || new.time_signature != old.time_signature;
        let length_changed = new.loop_length != old.loop_length;
        let affects_loop = mode_changed || (new.mode == LoopMode::Synced && (clock_grid_changed || length_changed));
        // The song loop belongs to the Song tracks; Bars tracks sit on the grid.
        let content = self.mode_has_content(TrackLoopMode::Song);
        if clock_grid_changed && self.mode_has_content(TrackLoopMode::Bars) {
            return Err(RejectReason::TransportLocked);
        }
        // Existing buffers are sized to the configured maximum, so changing
        // only a synced loop's cycle length is a non-destructive window edit.
        // Require a stopped transport to avoid a mid-cycle phase jump.
        if content
            && (mode_changed
                || (new.mode == LoopMode::Synced && clock_grid_changed)
                || (length_changed && self.run != RunState::Stopped))
        {
            return Err(RejectReason::TransportLocked);
        }
        self.transport = new;
        if affects_loop {
            self.set_master(Self::synced_master(&new));
        }
        self.refresh_bars_clocks();
        self.resync_click();
        Ok(())
    }

    /// Start the transport for a recording on a non-Song track, with that
    /// track's count-in when it starts from stopped.
    fn start_for_track(&mut self, count_in_bars: u32) {
        match self.run {
            RunState::Stopped => {
                let count_in = (count_in_bars as f64 * self.transport.frames_per_bar()).round() as u64;
                if count_in > 0 {
                    self.run = RunState::CountIn;
                    self.count_in_total = count_in;
                    self.count_in_left = count_in;
                } else {
                    self.run = RunState::Running;
                }
            }
            RunState::Paused => self.run = RunState::Running,
            RunState::CountIn | RunState::Running => {}
        }
        self.resync_click();
    }

    fn play(&mut self) {
        match self.run {
            RunState::Stopped => {
                let count_in =
                    if self.transport.mode == LoopMode::Synced { self.transport.count_in_frames() } else { 0 };
                if count_in > 0 {
                    self.run = RunState::CountIn;
                    self.count_in_total = count_in;
                    self.count_in_left = count_in;
                } else {
                    self.run = RunState::Running;
                }
            }
            RunState::Paused => self.run = RunState::Running,
            RunState::CountIn | RunState::Running => {}
        }
        self.resync_click();
    }

    /// Pause is refused while any track records or is armed: a paused
    /// recording would either lose the input that arrives during the pause or
    /// splice it in at the wrong loop phase. Stop recording (or Stop) first.
    fn pause(&mut self) -> Result<(), RejectReason> {
        match self.run {
            RunState::Running if self.any_recording() => return Err(RejectReason::TrackBusy),
            RunState::Running => self.run = RunState::Paused,
            RunState::CountIn => self.stop(RunState::Stopped),
            RunState::Stopped | RunState::Paused => {}
        }
        Ok(())
    }

    /// Stop the transport (`then` = Stopped) or restart it from the loop start
    /// (`then` = Running). Open recordings end at the performer's stop point
    /// `t`, exactly like `StopRecording` at that frame: armed takes are
    /// cancelled, a free first take sets the loop to press-to-press length,
    /// other takes are cut with a fade-out. With input latency their last
    /// frames are still in flight, so playback stops now but the rewind waits
    /// (at most `latency` frames) until those frames have been recorded.
    fn stop(&mut self, then: RunState) {
        let t = self.t;
        let mut drain = 0i64;
        for ti in 0..self.tracks.len() {
            let Some(rec) = self.tracks[ti].rec else { continue };
            if t <= rec.start {
                self.discard_recording(ti);
                continue;
            }
            match rec.kind {
                RecKind::FirstTake => self.finish_first_take(ti, t, true),
                RecKind::NewLoop | RecKind::Overdub => {
                    if rec.end.is_none_or(|end| t < end) {
                        self.cut_recording(ti, rec, t);
                    }
                }
            }
            if let Some(r) = self.tracks[ti].rec {
                // Frames with performance time < end arrive until t' = end + lat.
                let pending = r.end.unwrap_or(t) + r.lat - t;
                if pending > 0 {
                    drain = drain.max(pending);
                } else {
                    self.complete_recording(ti);
                }
            }
        }
        self.count_in_left = 0;
        self.count_in_total = 0;
        if drain > 0 {
            self.run = RunState::Stopped;
            self.drain = Some(Drain { t, left: drain as u64, then });
        } else {
            self.finish_stop(then);
        }
    }

    /// Rewind to the loop start once no input is in flight.
    fn finish_stop(&mut self, then: RunState) {
        for ti in 0..self.tracks.len() {
            if self.tracks[ti].rec.is_some() {
                self.complete_recording(ti);
            }
        }
        self.run = then;
        self.t = 0;
        self.count_in_left = 0;
        self.count_in_total = 0;
        // Free loops restart from their beginning on the next Play.
        if let Some(m) = self.master {
            self.set_master(Some(Master { origin: 0, ..m }));
        }
        // Every track's own loop does too (Bars clocks already start at 0).
        self.base = self.base.map(|b| Master { origin: 0, ..b });
        for tr in &mut self.tracks {
            if tr.lp.mode != TrackLoopMode::Song {
                tr.clock = tr.clock.map(|c| Master { origin: 0, ..c });
            }
        }
        self.resync_click();
    }

    fn ensure_running_free(&mut self) {
        if matches!(self.run, RunState::Stopped | RunState::Paused) {
            self.run = RunState::Running;
            self.resync_click();
        }
    }

    fn next_bar_at_or_after(&self, frame: i64) -> i64 {
        let step = self.transport.frames_per_bar();
        grid_frame(first_grid_index_at_or_after(frame, step), step)
    }

    fn check_can_add_layer(&mut self, ti: usize) -> Result<(), RejectReason> {
        let tr = self.tracks.get(ti).ok_or(RejectReason::NoSuchTrack)?;
        if tr.rec.is_some() || self.track_busy(ti) {
            return Err(RejectReason::TrackBusy);
        }
        if tr.layers.len() >= self.max_layers {
            return Err(RejectReason::LayerLimit);
        }
        if self.pool.available() == 0 {
            // Redo history is the only reclaimable memory: give it up so the
            // next attempt (after the control thread has zeroed the buffers,
            // a few milliseconds later) can succeed.
            for tr in &mut self.tracks {
                Self::release_redo(&mut self.pool, tr);
            }
            return Err(RejectReason::PoolExhausted);
        }
        Ok(())
    }

    /// Hand every undone layer back to the pool. Buffers become available
    /// again once the control thread has zeroed them.
    fn release_redo(pool: &mut LayerSource, tr: &mut Track) {
        while let Some(buf) = tr.redo.pop() {
            pool.release(buf);
            tr.redo_summaries.pop();
            tr.redo_offsets.pop();
        }
    }

    fn begin(&mut self, ti: usize, rec: Recording) -> Result<(), RejectReason> {
        let buf = self.pool.acquire().ok_or(RejectReason::PoolExhausted)?;
        self.touch();
        let tr = &mut self.tracks[ti];
        // A new take invalidates this track's redo history.
        Self::release_redo(&mut self.pool, tr);
        tr.layers.push(buf); // within capacity: checked by check_can_add_layer
        tr.summaries.push(SummaryState::Recording);
        tr.offsets.push(0);
        tr.rec = Some(rec);
        tr.timing_suspect = false;
        tr.playing = true;
        Ok(())
    }

    fn record(&mut self, ti: usize) -> Result<(), RejectReason> {
        self.check_can_add_layer(ti)?;
        if !self.tracks[ti].layers.is_empty() {
            return Err(RejectReason::TrackNotEmpty);
        }
        let lp = self.tracks[ti].lp;
        if lp.mode != TrackLoopMode::Song {
            return self.record_own_loop(ti, lp);
        }
        let rec = match self.transport.mode {
            LoopMode::Free => {
                if self.master.is_none() && self.mode_recording(TrackLoopMode::Song) {
                    return Err(RejectReason::TrackBusy); // another first take is defining the loop
                }
                self.ensure_running_free();
                match self.master {
                    None => Recording { kind: RecKind::FirstTake, start: self.t, end: None, lat: self.latency },
                    Some(m) => {
                        let start = self.t;
                        let end = start + m.period.round() as i64;
                        Recording { kind: RecKind::NewLoop, start, end: Some(end), lat: self.latency }
                    }
                }
            }
            LoopMode::Synced => {
                let m = self.master.ok_or(RejectReason::InvalidTransport)?;
                if matches!(self.run, RunState::Stopped | RunState::Paused) {
                    self.play();
                }
                let (cycle, phase) = m.locate(self.t);
                let first = if phase == 0 { cycle } else { cycle + 1 };
                Recording {
                    kind: RecKind::NewLoop,
                    start: m.cycle_start(first),
                    end: Some(m.cycle_start(first + 1)),
                    lat: self.latency,
                }
            }
        };
        self.begin(ti, rec)
    }

    fn mode_recording(&self, mode: TrackLoopMode) -> bool {
        self.tracks.iter().any(|tr| tr.lp.mode == mode && tr.rec.is_some())
    }

    /// A new loop on an empty Bars, Multiple or Free track.
    ///
    /// - Bars: starts at the next bar line and lasts exactly one cycle of the
    ///   track's clock (each loop phase is written once, even when the start
    ///   is not the clock's phase 0).
    /// - Multiple with a base: the track's clock starts at the next base cycle
    ///   start and the take lasts one of its own cycles.
    /// - Multiple without a base, and Free: a first take that sets its own
    ///   length when it ends (and, for Multiple, the base).
    fn record_own_loop(&mut self, ti: usize, lp: TrackLoop) -> Result<(), RejectReason> {
        let lat = self.latency;
        let (rec, clock) = match lp.mode {
            TrackLoopMode::Bars => {
                let m = self.bars_clock(&lp);
                if !self.fits(m.period) {
                    return Err(RejectReason::LoopTooLong);
                }
                self.start_for_track(lp.count_in_bars);
                let start = self.next_bar_at_or_after(self.t);
                let (cycle, phase) = m.locate(start);
                let end = m.cycle_start(cycle + 1) + phase as i64;
                (Recording { kind: RecKind::NewLoop, start, end: Some(end), lat }, Some(m))
            }
            TrackLoopMode::Multiple if self.base.is_some() => {
                let base = self.base.ok_or(RejectReason::InvalidTransport)?;
                let period = lp.ratio * base.period;
                if !self.fits(period) {
                    return Err(RejectReason::LoopTooLong);
                }
                self.start_for_track(lp.count_in_bars);
                let (cycle, phase) = base.locate(self.t);
                let start = base.cycle_start(if phase == 0 { cycle } else { cycle + 1 });
                let m = Master { origin: start, period };
                (Recording { kind: RecKind::NewLoop, start, end: Some(m.cycle_start(1)), lat }, Some(m))
            }
            TrackLoopMode::Multiple | TrackLoopMode::Free => {
                if lp.mode == TrackLoopMode::Multiple && self.mode_recording(TrackLoopMode::Multiple) {
                    return Err(RejectReason::TrackBusy); // another first take is defining the base
                }
                self.start_for_track(lp.count_in_bars);
                (Recording { kind: RecKind::FirstTake, start: self.t, end: None, lat }, None)
            }
            TrackLoopMode::Song => return Err(RejectReason::InvalidTransport),
        };
        self.begin(ti, rec)?;
        if clock.is_some() {
            self.tracks[ti].clock = clock;
        }
        Ok(())
    }

    fn overdub(&mut self, ti: usize) -> Result<(), RejectReason> {
        self.check_can_add_layer(ti)?;
        if self.tracks[ti].layers.is_empty() || self.tracks[ti].clock.is_none() {
            return Err(RejectReason::TrackEmpty);
        }
        let lp = self.tracks[ti].lp;
        // Bars tracks are bar-quantized like Metronome mode; Multiple and Free
        // tracks behave like Free mode (Immediate means now).
        let (overdub_start, grid_mode) = match lp.mode {
            TrackLoopMode::Song => {
                match self.transport.mode {
                    LoopMode::Free => self.ensure_running_free(),
                    LoopMode::Synced => {
                        if matches!(self.run, RunState::Stopped | RunState::Paused) {
                            self.play();
                        }
                    }
                }
                (self.transport.overdub_start, self.transport.mode)
            }
            TrackLoopMode::Bars => {
                self.start_for_track(lp.count_in_bars);
                (lp.overdub_start, LoopMode::Synced)
            }
            TrackLoopMode::Multiple | TrackLoopMode::Free => {
                self.start_for_track(lp.count_in_bars);
                (lp.overdub_start, LoopMode::Free)
            }
        };
        let start = match (overdub_start, grid_mode) {
            (OverdubStart::Immediate, LoopMode::Free) => self.t,
            (OverdubStart::Immediate, LoopMode::Synced) | (OverdubStart::NextBar, _) => {
                self.next_bar_at_or_after(self.t)
            }
            (OverdubStart::LoopStart, _) => {
                let m = self.tracks[ti].clock.ok_or(RejectReason::TrackEmpty)?;
                let (cycle, phase) = m.locate(self.t);
                m.cycle_start(if phase == 0 { cycle } else { cycle + 1 })
            }
        };
        self.begin(ti, Recording { kind: RecKind::Overdub, start, end: None, lat: self.latency })
    }

    /// The performer asks to stop at transport frame `t` (performance time).
    /// Input for the frames up to that point is still arriving, so the
    /// recording itself closes `lat` frames later.
    fn stop_recording(&mut self, ti: usize) -> Result<(), RejectReason> {
        let tr = self.tracks.get(ti).ok_or(RejectReason::NoSuchTrack)?;
        let rec = tr.rec.ok_or(RejectReason::NotRecording)?;
        let t = self.t;
        if self.run == RunState::CountIn || t <= rec.start {
            self.discard_recording(ti); // never started: cancel
            return Ok(());
        }
        match rec.kind {
            RecKind::FirstTake => self.finish_first_take(ti, t, true),
            RecKind::NewLoop | RecKind::Overdub => {
                let bar_quantized = match tr.lp.mode {
                    TrackLoopMode::Song => self.transport.mode == LoopMode::Synced,
                    TrackLoopMode::Bars => true,
                    TrackLoopMode::Multiple | TrackLoopMode::Free => false,
                };
                let requested = if bar_quantized { self.next_bar_at_or_after(t) } else { t + self.fade_frames as i64 };
                let end = rec.end.map_or(requested, |e| e.min(requested));
                if let Some(r) = self.tracks[ti].rec.as_mut() {
                    r.end = Some(end);
                }
            }
        }
        Ok(())
    }

    fn undo(&mut self, ti: usize) -> Result<(), RejectReason> {
        let busy = self.track_busy(ti);
        let tr = self.tracks.get_mut(ti).ok_or(RejectReason::NoSuchTrack)?;
        if tr.rec.is_some() || busy {
            return Err(RejectReason::TrackBusy);
        }
        if tr.layers.len() < 2 {
            return Err(RejectReason::NothingToUndo);
        }
        self.epoch = self.epoch.wrapping_add(1);
        if let Some(buf) = tr.layers.pop() {
            let summary = tr.summaries.pop().unwrap_or(SummaryState::Recording);
            let offset = tr.offsets.pop().unwrap_or(0);
            if tr.redo.len() < tr.redo.capacity() {
                tr.redo.push(buf);
                tr.redo_summaries.push(summary);
                tr.redo_offsets.push(offset);
            } else {
                self.pool.release(buf);
            }
        }
        Ok(())
    }

    fn redo(&mut self, ti: usize) -> Result<(), RejectReason> {
        let busy = self.track_busy(ti);
        let tr = self.tracks.get_mut(ti).ok_or(RejectReason::NoSuchTrack)?;
        if tr.rec.is_some() || busy {
            return Err(RejectReason::TrackBusy);
        }
        if tr.layers.len() >= self.max_layers {
            return Err(RejectReason::LayerLimit);
        }
        let buf = tr.redo.pop().ok_or(RejectReason::NothingToRedo)?;
        let mut summary = tr.redo_summaries.pop().unwrap_or(SummaryState::Scanning {
            cursor: 0,
            bins: [0.0; LAYER_SUMMARY_BINS],
            timing: TimingScan::EMPTY,
        });
        if let SummaryState::Ready { summary_emitted, timing_emitted, .. } = &mut summary {
            *summary_emitted = false;
            *timing_emitted = false;
        }
        tr.layers.push(buf);
        tr.summaries.push(summary);
        tr.offsets.push(tr.redo_offsets.pop().unwrap_or(0));
        self.touch();
        Ok(())
    }

    fn nudge_layer(&mut self, ti: usize, li: usize, frames: i64) -> Result<(), RejectReason> {
        let tr = self.tracks.get_mut(ti).ok_or(RejectReason::NoSuchTrack)?;
        let half_loop = tr.clock.map(|m| (m.period / 2.0).floor() as i64).ok_or(RejectReason::TrackEmpty)?;
        let offset = tr.offsets.get_mut(li).ok_or(RejectReason::TrackEmpty)?;
        *offset = offset.saturating_add(frames).clamp(-half_loop, half_loop);
        self.touch();
        Ok(())
    }

    fn add_track(&mut self) -> Result<(), RejectReason> {
        let mut tr = self.spare.pop().ok_or(RejectReason::TrackLimit)?;
        tr.rec = None;
        tr.playing = true;
        tr.muted = false;
        tr.solo = false;
        tr.gain = 1.0;
        tr.level = if self.any_solo { 0.0 } else { 1.0 };
        tr.timing_suspect = false;
        tr.summaries.clear();
        tr.redo_summaries.clear();
        tr.offsets.clear();
        tr.redo_offsets.clear();
        tr.clock = self.master;
        tr.lp = TrackLoop::DEFAULT;
        tr.filling = false;
        self.tracks.push(tr); // within capacity: total tracks never exceed track_capacity
        self.touch();
        Ok(())
    }

    fn remove_track(&mut self, ti: usize) -> Result<(), RejectReason> {
        let tr = self.tracks.get(ti).ok_or(RejectReason::NoSuchTrack)?;
        // A copy in progress addresses tracks by index: none may move meanwhile.
        if tr.rec.is_some() || self.dup.is_some() {
            return Err(RejectReason::TrackBusy);
        }
        if self.tracks.len() == 1 {
            return Err(RejectReason::TrackLimit);
        }
        self.clear(ti)?; // releases layers and redo history to the recycler
        let tr = self.tracks.remove(ti); // moves at most MAX_TRACKS small structs; no allocation
        self.spare.push(tr);
        // Track indices are part of the frontend key. Re-emit every shifted
        // ready summary so replacing/removing a middle track cannot mislabel it.
        for shifted in self.tracks.iter_mut().skip(ti) {
            for summary in &mut shifted.summaries {
                if let SummaryState::Ready { summary_emitted, timing_emitted, .. } = summary {
                    *summary_emitted = false;
                    *timing_emitted = false;
                }
            }
        }
        self.any_solo = self.tracks.iter().any(|t| t.solo);
        self.release_unused_clocks();
        Ok(())
    }

    /// Make exactly `n` tracks active (clamped to 1..=capacity). Only used on
    /// an empty looper (restore), so moved tracks hold no layers.
    fn set_track_count(&mut self, n: usize) {
        let n = n.clamp(1, self.tracks.len() + self.spare.len());
        while self.tracks.len() < n {
            let _ = self.add_track();
        }
        while self.tracks.len() > n {
            if let Some(tr) = self.tracks.pop() {
                self.spare.push(tr);
            }
        }
    }

    fn clear(&mut self, ti: usize) -> Result<(), RejectReason> {
        let busy = self.track_busy(ti);
        let tr = self.tracks.get_mut(ti).ok_or(RejectReason::NoSuchTrack)?;
        if busy {
            return Err(RejectReason::TrackBusy);
        }
        self.epoch = self.epoch.wrapping_add(1);
        tr.rec = None;
        tr.timing_suspect = false;
        tr.playing = true;
        while let Some(buf) = tr.layers.pop() {
            self.pool.release(buf);
        }
        tr.summaries.clear();
        tr.offsets.clear();
        Self::release_redo(&mut self.pool, tr);
        self.release_unused_clocks();
        Ok(())
    }

    fn set_track_loop(&mut self, ti: usize, settings: TrackLoop) -> Result<(), RejectReason> {
        settings.validate().map_err(|_| RejectReason::InvalidTransport)?;
        let tr = self.tracks.get(ti).ok_or(RejectReason::NoSuchTrack)?;
        if self.track_busy(ti) {
            return Err(RejectReason::TrackBusy);
        }
        let old = tr.lp;
        let content = !tr.layers.is_empty() || tr.rec.is_some();
        if content {
            if settings.mode != old.mode {
                return Err(RejectReason::TrackNotEmpty);
            }
            if settings.length_differs(&old) && (self.run != RunState::Stopped || tr.rec.is_some()) {
                return Err(RejectReason::TransportLocked);
            }
        }
        let relevant_length = match settings.mode {
            TrackLoopMode::Bars => settings.bars != old.bars || settings.beats != old.beats || !content,
            TrackLoopMode::Multiple => settings.ratio != old.ratio || !content,
            TrackLoopMode::Song | TrackLoopMode::Free => false,
        };
        let period = if relevant_length { self.planned_period(&settings) } else { None };
        if period.is_some_and(|p| !self.fits(p)) {
            return Err(RejectReason::LoopTooLong);
        }
        let master = self.master;
        let tr = &mut self.tracks[ti];
        tr.lp = settings;
        if !content {
            tr.clock = match settings.mode {
                TrackLoopMode::Song => master,
                TrackLoopMode::Bars => period.map(|period| Master { origin: 0, period }),
                TrackLoopMode::Multiple | TrackLoopMode::Free => None,
            };
        } else if let (Some(period), Some(c)) = (period, tr.clock) {
            // Stopped: every clock is at origin 0. Only the repetition changes.
            tr.clock = Some(Master { origin: c.origin, period });
        }
        self.release_unused_clocks();
        self.touch();
        Ok(())
    }

    fn duplicate_track(&mut self, ti: usize) -> Result<(), RejectReason> {
        let src = self.tracks.get(ti).ok_or(RejectReason::NoSuchTrack)?;
        if src.rec.is_some() || self.dup.is_some() || self.track_busy(ti) {
            return Err(RejectReason::TrackBusy);
        }
        if self.spare.is_empty() {
            return Err(RejectReason::TrackLimit);
        }
        let layers = src.layers.len();
        if self.pool.available() < layers {
            return Err(RejectReason::PoolExhausted);
        }
        let Some(mut tr) = self.spare.pop() else { return Err(RejectReason::TrackLimit) };
        let src = &self.tracks[ti];
        tr.rec = None;
        tr.playing = src.playing;
        tr.muted = src.muted;
        tr.solo = false;
        tr.gain = src.gain;
        tr.level = 0.0; // fades in once the copy is complete
        tr.timing_suspect = src.timing_suspect;
        tr.clock = src.clock;
        tr.lp = src.lp;
        tr.summaries.clear();
        tr.redo_summaries.clear();
        tr.offsets.clear();
        tr.redo_offsets.clear();
        // Capacities match (`max_layers` entries per track); `take` keeps
        // these pushes within capacity regardless.
        for &offset in src.offsets.iter().take(tr.offsets.capacity()) {
            tr.offsets.push(offset);
        }
        for state in src.summaries.iter().take(tr.summaries.capacity()) {
            let copied = match *state {
                SummaryState::Ready { bins, timing, .. } => {
                    SummaryState::Ready { bins, timing, summary_emitted: false, timing_emitted: false }
                }
                _ => SummaryState::Scanning { cursor: 0, bins: [0.0; LAYER_SUMMARY_BINS], timing: TimingScan::EMPTY },
            };
            tr.summaries.push(copied);
        }
        for _ in 0..layers {
            match self.pool.acquire() {
                Some(buf) => tr.layers.push(buf),
                None => break, // unreachable: availability checked above
            }
        }
        tr.summaries.truncate(tr.layers.len());
        tr.offsets.truncate(tr.layers.len());
        tr.filling = !tr.layers.is_empty();
        let dst = ti + 1;
        self.tracks.insert(dst, tr); // within capacity; moves at most MAX_TRACKS small structs
        for shifted in self.tracks.iter_mut().skip(dst + 1) {
            for summary in &mut shifted.summaries {
                if let SummaryState::Ready { summary_emitted, timing_emitted, .. } = summary {
                    *summary_emitted = false;
                    *timing_emitted = false;
                }
            }
        }
        if self.tracks[dst].filling {
            self.dup = Some(DupJob { src: ti, dst, layer: 0, offset: 0 });
        }
        self.touch();
        Ok(())
    }

    /// Copy at most `CHUNK_FRAMES` frames of a `DuplicateTrack` per call.
    fn service_dup(&mut self) {
        let Some(mut d) = self.dup else { return };
        let mut budget = CHUNK_FRAMES;
        let mut done = false;
        while budget > 0 {
            let (a, b) = self.tracks.split_at_mut(d.dst.max(d.src));
            let (src, dst) = if d.src < d.dst { (&a[d.src], &mut b[0]) } else { (&b[0], &mut a[d.dst]) };
            let (Some(from), Some(to)) = (src.layers.get(d.layer), dst.layers.get_mut(d.layer)) else {
                done = true;
                break;
            };
            let frames = from.len().min(to.len());
            let len = budget.min(frames.saturating_sub(d.offset));
            to[d.offset..d.offset + len].copy_from_slice(&from[d.offset..d.offset + len]);
            budget -= len;
            d.offset += len;
            if d.offset >= frames {
                d.layer += 1;
                d.offset = 0;
            }
        }
        if done {
            self.dup = None;
            if let Some(tr) = self.tracks.get_mut(d.dst) {
                tr.filling = false;
            }
            self.touch();
        } else {
            self.dup = Some(d);
        }
    }

    // ---- layer export / import (see `transfer.rs`) ------------------------------

    /// Answer at most `JOBS_PER_BLOCK` transfer jobs. Realtime-safe: copies are
    /// bounded by `CHUNK_FRAMES`, buffers travel back to the control thread.
    fn service_jobs(&mut self) {
        for _ in 0..JOBS_PER_BLOCK {
            let Ok(mut job) = self.job_rx.pop() else { return };
            job.kind = match job.kind {
                JobKind::Describe(_) => JobKind::Describe(Some(self.inventory())),
                JobKind::Read(mut c) => {
                    c.epoch = self.epoch;
                    c.result = Some(self.read_chunk(&mut c));
                    JobKind::Read(c)
                }
                JobKind::Write(mut c) => {
                    c.result = Some(self.write_chunk(&c));
                    JobKind::Write(c)
                }
                JobKind::Restore(plan, _) => JobKind::Restore(plan, Some(self.restore(&plan))),
                JobKind::Finish(_) => {
                    self.loading = false;
                    self.touch();
                    JobKind::Finish(Some(Ok(())))
                }
            };
            if let Err(rtrb::PushError::Full(job)) = self.job_tx.push(job) {
                // Unreachable: the control side keeps at most JOB_CAPACITY jobs
                // outstanding. Never free on the audio thread.
                std::mem::forget(job);
            }
        }
    }

    fn inventory(&self) -> Inventory {
        let mut tracks = [TrackPlan::EMPTY; MAX_TRACKS];
        for (slot, tr) in tracks.iter_mut().zip(self.tracks.iter()) {
            *slot = TrackPlan {
                layers: tr.layers.len().min(u8::MAX as usize) as u8,
                redo: tr.redo.len().min(u8::MAX as usize) as u8,
                gain: tr.gain,
                muted: tr.muted,
                solo: tr.solo,
                playing: tr.playing,
                layer_offsets: LayerOffsets::from_slice(&tr.offsets).values,
                redo_offsets: LayerOffsets::from_slice(&tr.redo_offsets).values,
                clock: self.track_clock_plan(tr),
            };
        }
        Inventory {
            epoch: self.epoch,
            busy: self.loading || self.drain.is_some() || self.any_recording() || self.dup.is_some(),
            run: self.run,
            transport: self.transport,
            period: self.master.map(|m| m.period),
            latency: self.latency as u32,
            max_frames: self.max_frames,
            max_layers: self.max_layers,
            track_count: self.tracks.len().min(MAX_TRACKS) as u8,
            track_capacity: (self.tracks.len() + self.spare.len()).min(MAX_TRACKS) as u8,
            tracks,
        }
    }

    fn track_clock_plan(&self, tr: &Track) -> TrackClock {
        let own = tr.lp.mode != TrackLoopMode::Song;
        TrackClock {
            track_loop: tr.lp,
            period: if own { tr.clock.map(|c| c.period) } else { None },
            base: if tr.lp.mode == TrackLoopMode::Multiple { self.base.map(|b| b.period) } else { None },
        }
    }

    fn layer(&self, c: &Chunk) -> Option<&LayerBuf> {
        let tr = self.tracks.get(c.track as usize)?;
        match c.slot {
            LayerSlot::Active(i) => tr.layers.get(i as usize),
            LayerSlot::Redo(i) => tr.redo.get(i as usize),
        }
    }

    fn read_chunk(&self, c: &mut Chunk) -> Result<(), RejectReason> {
        let layer = self.layer(c).ok_or(RejectReason::TrackEmpty)?;
        let src = layer.get(c.offset..c.offset + c.buf.len()).ok_or(RejectReason::LoopTooLong)?;
        c.buf.copy_from_slice(src);
        Ok(())
    }

    fn write_chunk(&mut self, c: &Chunk) -> Result<(), RejectReason> {
        if !self.loading {
            return Err(RejectReason::TrackBusy);
        }
        let tr = self.tracks.get_mut(c.track as usize).ok_or(RejectReason::NoSuchTrack)?;
        let layer = match c.slot {
            LayerSlot::Active(i) => tr.layers.get_mut(i as usize),
            LayerSlot::Redo(i) => tr.redo.get_mut(i as usize),
        }
        .ok_or(RejectReason::TrackEmpty)?;
        let dst = layer.get_mut(c.offset..c.offset + c.buf.len()).ok_or(RejectReason::LoopTooLong)?;
        dst.copy_from_slice(&c.buf);
        self.touch();
        Ok(())
    }

    /// Replace all content with zeroed layers laid out as `plan` and enter
    /// loading mode. Releases at most `pool_layers` buffers (bounded work).
    fn restore(&mut self, plan: &RestorePlan) -> Result<(), RejectReason> {
        if self.run != RunState::Stopped || self.drain.is_some() || self.any_recording() || self.dup.is_some() {
            return Err(RejectReason::TrackBusy);
        }
        let mut transport = plan.transport;
        transport.sample_rate = self.sample_rate;
        transport.position = 0;
        transport.play_state = Default::default();
        validate_transport(&transport, self.max_frames)?;
        let master = match transport.mode {
            LoopMode::Synced => Self::synced_master(&transport),
            LoopMode::Free => match plan.period {
                Some(p) if p.is_finite() && p >= 1.0 && p.ceil() as usize <= self.max_frames => {
                    Some(Master { origin: 0, period: p })
                }
                Some(_) => return Err(RejectReason::LoopTooLong),
                None => None,
            },
        };
        // Each track's own clock (origin 0: a restore leaves the transport stopped).
        let fits = |p: f64| p.is_finite() && p >= 1.0 && p.ceil() as usize <= self.max_frames;
        let mut clocks = [None; MAX_TRACKS];
        let mut base = None;
        let mut needed = 0usize;
        for (t, tp) in plan.tracks.iter().enumerate() {
            let n = tp.layers as usize + tp.redo as usize;
            if n > 0 && t >= self.tracks.len() + self.spare.len() {
                return Err(RejectReason::TrackLimit);
            }
            if tp.layers as usize > self.max_layers || tp.redo as usize > self.max_layers {
                return Err(RejectReason::LayerLimit);
            }
            let lp = tp.clock.track_loop;
            lp.validate().map_err(|_| RejectReason::InvalidTransport)?;
            clocks[t] = match lp.mode {
                TrackLoopMode::Song => {
                    if n > 0 && master.is_none() {
                        return Err(RejectReason::InvalidTransport);
                    }
                    master
                }
                TrackLoopMode::Bars => {
                    let beats = lp.beats_per_loop(transport.time_signature.numerator).max(1);
                    let period = beats as f64 * transport.frames_per_beat();
                    if n > 0 && !fits(period) {
                        return Err(RejectReason::LoopTooLong);
                    }
                    Some(Master { origin: 0, period })
                }
                TrackLoopMode::Multiple | TrackLoopMode::Free if n == 0 => None,
                TrackLoopMode::Multiple | TrackLoopMode::Free => {
                    let period = tp.clock.period.ok_or(RejectReason::InvalidTransport)?;
                    if !fits(period) {
                        return Err(RejectReason::LoopTooLong);
                    }
                    if lp.mode == TrackLoopMode::Multiple && base.is_none() {
                        let b = tp.clock.base.ok_or(RejectReason::InvalidTransport)?;
                        if !(b.is_finite() && b >= 1.0) {
                            return Err(RejectReason::InvalidTransport);
                        }
                        base = Some(Master { origin: 0, period: b });
                    }
                    Some(Master { origin: 0, period })
                }
            };
            needed += n;
        }
        if plan.track_count as usize > self.tracks.len() + self.spare.len() {
            return Err(RejectReason::TrackLimit);
        }
        for ti in 0..self.tracks.len() {
            let _ = self.clear(ti);
        }
        // Exactly the planned tracks, including empty ones (at least every used one).
        let used = plan.tracks.iter().rposition(|tp| tp.layers > 0 || tp.redo > 0).map_or(0, |i| i + 1);
        self.set_track_count((plan.track_count as usize).max(used));
        if self.pool.available() < needed {
            return Err(RejectReason::PoolExhausted);
        }
        for (tr, tp) in self.tracks.iter_mut().zip(plan.tracks.iter()) {
            for i in 0..tp.layers {
                if let Some(buf) = self.pool.acquire() {
                    tr.layers.push(buf);
                    tr.summaries.push(SummaryState::Scanning {
                        cursor: 0,
                        bins: [0.0; LAYER_SUMMARY_BINS],
                        timing: TimingScan::EMPTY,
                    });
                    tr.offsets.push(tp.layer_offsets[i as usize]);
                }
            }
            for i in 0..tp.redo {
                if let Some(buf) = self.pool.acquire() {
                    tr.redo.push(buf);
                    tr.redo_summaries.push(SummaryState::Scanning {
                        cursor: 0,
                        bins: [0.0; LAYER_SUMMARY_BINS],
                        timing: TimingScan::EMPTY,
                    });
                    tr.redo_offsets.push(tp.redo_offsets[i as usize]);
                }
            }
            tr.gain = if tp.gain.is_finite() { tp.gain.clamp(0.0, 2.0) } else { 1.0 };
            tr.muted = tp.muted;
            tr.solo = tp.solo;
            tr.playing = tp.playing;
            tr.lp = tp.clock.track_loop;
        }
        self.any_solo = self.tracks.iter().any(|tr| tr.solo);
        // Start at the target level: nothing ramps in when playback begins.
        let any_solo = self.any_solo;
        for tr in &mut self.tracks {
            tr.level = tr.target_level(any_solo);
        }
        self.transport = transport;
        self.set_master(master);
        self.base = base;
        for (tr, clock) in self.tracks.iter_mut().zip(clocks.iter()) {
            tr.clock = *clock;
        }
        self.latency = (plan.latency as i64).min(self.max_frames as i64 / 2);
        self.t = 0;
        self.loading = true;
        self.touch();
        self.resync_click();
        Ok(())
    }

    // ---- state out ------------------------------------------------------------

    fn publish(&mut self) {
        if self.snap_tx.slots() == 0 {
            return;
        }
        let snap = self.snapshot();
        let _ = self.snap_tx.push(snap);
    }

    pub fn snapshot(&self) -> LooperSnapshot {
        let mut tracks = [TrackSnapshot::EMPTY; MAX_TRACKS];
        for (ti, (slot, tr)) in tracks.iter_mut().zip(self.tracks.iter()).enumerate() {
            *slot = self.track_snapshot(tr, self.track_busy(ti));
        }
        let (loop_cycle, loop_phase, loop_period) = match self.master {
            Some(m) => {
                let (c, i) = m.locate(self.t);
                (c, i as u64, Some(m.period))
            }
            None => (0, 0, None),
        };
        LooperSnapshot {
            run: self.run,
            frame: self.t,
            count_in_remaining: self.count_in_left,
            position: self.transport.musical_position(self.t.max(0) as u64),
            loop_period,
            loop_cycle,
            loop_phase,
            transport: self.transport,
            tracks,
            track_count: self.tracks.len().min(MAX_TRACKS) as u8,
            track_capacity: (self.tracks.len() + self.spare.len()).min(MAX_TRACKS) as u8,
            free_layers: self.pool.available() as u32,
            latency_frames: self.latency as u32,
            frames_processed: self.frames_processed,
            epoch: self.epoch,
        }
    }

    fn track_snapshot(&self, tr: &Track, busy: bool) -> TrackSnapshot {
        let state = match tr.rec {
            Some(_) if self.run == RunState::CountIn => TrackState::Armed,
            Some(r) if self.t < r.start => TrackState::Armed,
            Some(r) if r.kind == RecKind::Overdub => TrackState::Overdubbing,
            Some(_) => TrackState::Recording,
            None if tr.layers.is_empty() => TrackState::Empty,
            None if tr.playing && self.run == RunState::Running => TrackState::Playing,
            None => TrackState::Stopped,
        };
        TrackSnapshot {
            state,
            layers: tr.layers.len().min(u8::MAX as usize) as u8,
            muted: tr.muted,
            solo: tr.solo,
            playing: tr.playing,
            gain: tr.gain,
            redo_layers: tr.redo.len().min(u8::MAX as usize) as u8,
            timing_suspect: tr.timing_suspect,
            layer_offsets: LayerOffsets::from_slice(&tr.offsets),
            record_start: tr.rec.map(|r| r.start),
            record_end: tr.rec.and_then(|r| r.end),
            track_loop: tr.lp,
            loop_period: tr.clock.map(|m| m.period),
            loop_phase: tr.clock.map_or(0, |m| m.locate(self.t).1 as u64),
            busy,
        }
    }
}

/// The stream hosting this core is being torn down (device change, rate
/// change, hot-plug, iOS route change, stop). This runs on the thread that
/// drops the stream, never inside an audio callback. Open takes are closed as
/// a transport Stop would close them (input still in flight is lost with the
/// stream), and the layer buffers are *moved* (not copied) to whoever holds
/// `LooperHandles::parked`. A core in the middle of a restore parks nothing:
/// its content is incomplete and the restore's source still exists.
impl Drop for LooperCore {
    fn drop(&mut self) {
        let Some(tx) = self.park_tx.take() else { return };
        if self.loading {
            return;
        }
        // Not on the audio thread: finish a duplicate's copy so it parks whole.
        while self.dup.is_some() {
            self.service_dup();
        }
        if self.drain.is_some() || self.any_recording() {
            if self.drain.is_none() {
                self.stop(RunState::Stopped);
            }
            self.drain = None;
            self.finish_stop(RunState::Stopped);
        }
        let mut clocks = [TrackClock::SONG; MAX_TRACKS];
        for (slot, tr) in clocks.iter_mut().zip(self.tracks.iter()) {
            *slot = self.track_clock_plan(tr);
        }
        let tracks = self
            .tracks
            .iter_mut()
            .enumerate()
            .map(|(i, tr)| ParkedTrack {
                plan: TrackPlan {
                    layers: tr.layers.len().min(u8::MAX as usize) as u8,
                    redo: tr.redo.len().min(u8::MAX as usize) as u8,
                    gain: tr.gain,
                    muted: tr.muted,
                    solo: tr.solo,
                    playing: tr.playing,
                    layer_offsets: LayerOffsets::from_slice(&tr.offsets).values,
                    redo_offsets: LayerOffsets::from_slice(&tr.redo_offsets).values,
                    clock: clocks[i],
                },
                layers: std::mem::take(&mut tr.layers),
                redo: std::mem::take(&mut tr.redo),
            })
            .collect();
        let _ = tx.send(ParkedLooper {
            transport: self.transport,
            period: self.master.map(|m| m.period),
            latency: self.latency as u32,
            max_frames: self.max_frames,
            max_layers: self.max_layers,
            track_capacity: (self.tracks.len() + self.spare.len()).min(MAX_TRACKS),
            tracks,
        });
    }
}
