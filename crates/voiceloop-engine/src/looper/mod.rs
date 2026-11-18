//! Multitrack looper (Milestone 1A).
//!
//! ```text
//!  input callback ──rtrb<f32> (mono)──▶ output callback
//!                                        └─ LooperCore::process()  ← master clock (output frames)
//!                                             ├ transport + count-in + click
//!                                             ├ tracks: layer stacks (preallocated pool)
//!                                             └ snapshots / notices ──rtrb──▶ control thread ──▶ UI
//!  control thread ──rtrb<LooperCommand>──▶ LooperCore (drained at the start of each block)
//!  control thread ◀──rtrb<LayerBuf>──▶ LooperCore (released layers zeroed off the realtime thread)
//! ```
//!
//! - [`processor`]: the realtime looper (pure; unit-tested with synthetic buffers)
//! - [`pool`]: fixed pool of layer buffers recycled through wait-free rings
//! - [`click`]: metronome click synthesis and accent selection
//! - [`bridge`]: input→output clock bridge (cushion, underrun, drift; unit-tested)
//! - [`io`]: CPAL output stream that hosts the processor (hardware; not unit-tested)

pub mod bridge;
mod click;
pub mod io;
mod pool;
mod processor;
#[cfg(test)]
mod tests;
pub mod transfer;

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::transport::{MusicalPosition, TransportState};

pub use click::Accent;
pub use pool::{LayerRecycler, StorageSink};
pub use processor::{
    soft_limit, LooperCommand, LooperCore, LooperHandles, LooperNotice, LooperSnapshot, RejectReason, RunState,
    TrackSnapshot, TrackState, LIMIT_KNEE, MAX_TRANSPORT_FRAME,
};

/// Hard upper bound on tracks (snapshot and plan arrays are sized by this).
///
/// Cost of the ceiling: a track owns no audio (every layer is a pool
/// buffer), so a preallocated track is ~100 bytes plus two pointer vectors of
/// `max_layers_per_track` entries. Snapshots grow by ~40 bytes per track.
/// Per-sample work is one gain-slew step per active track plus the layer
/// reads, which stay bounded by `pool_layers` whatever the track count.
/// 16 is ample for "at least 10 usable tracks" and keeps snapshots small.
pub const MAX_TRACKS: usize = 16;

/// Realtime work bounds. Every layer is a pool buffer, so `pool_layers`
/// bounds both the layers summed per output sample and the buffers released
/// in one callback (each buffer can be released at most once per callback,
/// because a released buffer only becomes usable after the control thread
/// zeroes it). These caps keep the worst case far inside a 64-frame deadline.
pub const MAX_POOL_LAYERS: usize = 64;
/// Upper bound for `max_layers_per_track` (base take + overdubs).
pub const MAX_LAYERS_PER_TRACK: usize = 32;
/// Upper bound for `fade_frames` (~100 ms at 48 kHz). Fades are applied in
/// one callback when a take closes, so this also bounds that work.
pub const MAX_FADE_FRAMES: u32 = 4_800;

/// Resource limits. Everything the looper can ever use is allocated once, up
/// front, from these numbers; nothing grows while audio runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LooperConfig {
    /// Tracks active when audio starts (1..=`track_capacity`). More can be
    /// added at runtime with `addTrack` (no stream restart).
    pub max_tracks: usize,
    /// Tracks preallocated, i.e. the most `addTrack` can reach (1..=MAX_TRACKS).
    pub track_capacity: usize,
    /// Longest loop, in seconds. Sets the size of every layer buffer.
    pub max_loop_seconds: f32,
    /// Layer buffers in the shared pool (base takes + overdubs across all tracks).
    pub pool_layers: usize,
    /// Base take + overdubs a single track may hold.
    pub max_layers_per_track: usize,
    /// Refuse configurations whose pool would exceed this many bytes.
    pub memory_limit_bytes: usize,
    /// Edge fade length in frames applied where a recording starts and ends (0 = none).
    pub fade_frames: u32,
    /// Frames a track's gain takes to move by 1.0 when mute, solo, level, or
    /// play/stop changes (0 = instant). Prevents zipper noise and clicks.
    pub gain_smoothing_frames: u32,
    /// Extra input retained after the first-take boundary. It is hidden by
    /// the default loop length but remains available to the future clip editor.
    pub capture_tail_ms: u32,
}

impl Default for LooperConfig {
    fn default() -> Self {
        Self {
            max_tracks: 2,
            track_capacity: MAX_TRACKS,
            max_loop_seconds: 30.0,
            pool_layers: 8,
            max_layers_per_track: 6,
            memory_limit_bytes: 256 * 1024 * 1024,
            fade_frames: 64,
            gain_smoothing_frames: 256,
            capture_tail_ms: 100,
        }
    }
}

impl LooperConfig {
    /// Validate and return the layer length in frames at `sample_rate`.
    pub fn max_loop_frames(&self, sample_rate: u32) -> Result<usize, EngineError> {
        if self.track_capacity == 0 || self.track_capacity > MAX_TRACKS {
            return Err(EngineError::Invalid(format!("track_capacity must be 1..={MAX_TRACKS}")));
        }
        if self.max_tracks == 0 || self.max_tracks > self.track_capacity {
            return Err(EngineError::Invalid("max_tracks must be 1..=track_capacity".into()));
        }
        if self.pool_layers == 0 || self.pool_layers > u16::MAX as usize {
            return Err(EngineError::Invalid("pool_layers must be at least 1".into()));
        }
        if self.max_layers_per_track == 0 {
            return Err(EngineError::Invalid("max_layers_per_track must be at least 1".into()));
        }
        if !(self.max_loop_seconds.is_finite() && self.max_loop_seconds > 0.0) || sample_rate == 0 {
            return Err(EngineError::Invalid("max_loop_seconds and sample rate must be positive".into()));
        }
        let frames = (self.max_loop_seconds as f64 * sample_rate as f64).ceil() as usize;
        let bytes = self.memory_bytes(sample_rate);
        if bytes > self.memory_limit_bytes {
            return Err(EngineError::ResourceLimit(format!(
                "looper pool needs {} MiB ({} layers x {:.1} s) but the limit is {} MiB",
                bytes / (1024 * 1024),
                self.pool_layers,
                self.max_loop_seconds,
                self.memory_limit_bytes / (1024 * 1024)
            )));
        }
        // Checked after the memory budget so an oversized request reports the budget first.
        if self.pool_layers > MAX_POOL_LAYERS {
            return Err(EngineError::Invalid(format!("pool_layers must be at most {MAX_POOL_LAYERS}")));
        }
        if self.max_layers_per_track > MAX_LAYERS_PER_TRACK {
            return Err(EngineError::Invalid(format!("max_layers_per_track must be at most {MAX_LAYERS_PER_TRACK}")));
        }
        if self.fade_frames > MAX_FADE_FRAMES || (self.fade_frames as usize) * 4 > frames {
            return Err(EngineError::Invalid(format!(
                "fade_frames must be at most {MAX_FADE_FRAMES} and a quarter of the longest loop"
            )));
        }
        if self.capture_tail_ms > 2_000 {
            return Err(EngineError::Invalid("capture_tail_ms must be at most 2000".into()));
        }
        Ok(frames)
    }

    /// Bytes the layer pool allocates at `sample_rate`.
    pub fn memory_bytes(&self, sample_rate: u32) -> usize {
        let frames = (self.max_loop_seconds.max(0.0) as f64 * sample_rate as f64).ceil() as usize;
        frames.saturating_mul(self.pool_layers).saturating_mul(std::mem::size_of::<f32>())
    }
}

/// Counters written by the realtime callbacks (relaxed atomics are realtime-safe).
#[derive(Debug, Default)]
pub struct IoCounters {
    /// Input frames dropped because the input→looper ring was full.
    pub input_overrun_frames: AtomicU64,
    /// Output blocks that had to substitute silence for missing input frames.
    pub input_underrun_frames: AtomicU64,
    /// Input frames skipped to correct input/output clock drift.
    pub drift_dropped_frames: AtomicU64,
    /// Input frames currently buffered ahead of the looper (adaptive; part of round-trip latency).
    pub input_cushion_frames: AtomicU64,
}

/// Looper state for the UI, built on the control thread from the latest snapshot.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LooperState {
    pub run: RunState,
    pub frame: i64,
    pub count_in_remaining: u64,
    pub position: MusicalPosition,
    pub loop_period_frames: Option<f64>,
    pub loop_cycle: i64,
    pub loop_phase: u64,
    pub transport: TransportState,
    pub tracks: Vec<TrackSnapshot>,
    /// Most tracks this looper can run (preallocated): the `addTrack` ceiling.
    pub max_tracks: u32,
    pub free_layers: u32,
    pub latency_frames: u32,
    pub sample_rate: u32,
    pub memory_bytes: usize,
    pub input_overrun_frames: u64,
    pub input_underrun_frames: u64,
    pub drift_dropped_frames: u64,
    /// Input buffered ahead of the looper, in frames. Grows after underruns.
    pub input_cushion_frames: u64,
}

impl LooperState {
    pub fn from_snapshot(s: &LooperSnapshot, counters: &IoCounters, memory_bytes: usize) -> Self {
        let n = (s.track_count as usize).min(MAX_TRACKS);
        Self {
            run: s.run,
            frame: s.frame,
            count_in_remaining: s.count_in_remaining,
            position: s.position,
            loop_period_frames: s.loop_period,
            loop_cycle: s.loop_cycle,
            loop_phase: s.loop_phase,
            transport: s.transport,
            tracks: s.tracks[..n].to_vec(),
            max_tracks: s.track_capacity as u32,
            free_layers: s.free_layers,
            latency_frames: s.latency_frames,
            sample_rate: s.transport.sample_rate,
            memory_bytes,
            input_overrun_frames: counters.input_overrun_frames.load(Ordering::Relaxed),
            input_underrun_frames: counters.input_underrun_frames.load(Ordering::Relaxed),
            drift_dropped_frames: counters.drift_dropped_frames.load(Ordering::Relaxed),
            input_cushion_frames: counters.input_cushion_frames.load(Ordering::Relaxed),
        }
    }
}
