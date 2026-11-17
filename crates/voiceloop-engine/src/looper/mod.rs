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

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::transport::{MusicalPosition, TransportState};

pub use click::Accent;
pub use pool::{LayerRecycler, StorageSink};
pub use processor::{
soft_limit, LooperCommand, LooperCore, LooperHandles, LooperNotice, LooperSnapshot, RejectReason, RunState, TrackSnapshot,
    TrackState, LIMIT_KNEE,
};

/// Hard upper bound on tracks (snapshot arrays are sized by this).
pub const MAX_TRACKS: usize = 8;

/// Resource limits. Everything the looper can ever use is allocated once, up
/// front, from these numbers; nothing grows while audio runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LooperConfig {
    /// Tracks available (1..=MAX_TRACKS). Milestone 1A UI shows 2.
    pub max_tracks: usize,
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
}

impl Default for LooperConfig {
    fn default() -> Self {
        Self {
            max_tracks: 2,
            max_loop_seconds: 30.0,
            pool_layers: 8,
            max_layers_per_track: 6,
            memory_limit_bytes: 256 * 1024 * 1024,
            fade_frames: 64,
            gain_smoothing_frames: 256,
        }
    }
}

impl LooperConfig {
    /// Validate and return the layer length in frames at `sample_rate`.
    pub fn max_loop_frames(&self, sample_rate: u32) -> Result<usize, EngineError> {
        if self.max_tracks == 0 || self.max_tracks > MAX_TRACKS {
            return Err(EngineError::Invalid(format!("max_tracks must be 1..={MAX_TRACKS}")));
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
