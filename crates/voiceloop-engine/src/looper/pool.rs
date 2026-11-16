//! Fixed pool of layer buffers.
//!
//! All buffers are allocated (zero-filled) once, before the audio stream
//! starts. The realtime side takes clean buffers from one wait-free ring and
//! hands released buffers back through another. The control thread zeroes
//! released buffers and returns them. The realtime thread therefore never
//! allocates, frees, or clears large buffers. Both rings have capacity equal
//! to the pool size, so pushes cannot fail.

use rtrb::{Consumer, Producer, PushError, RingBuffer};

/// One layer of audio: mono f32 at the session rate, `max_loop_frames` long.
pub type LayerBuf = Box<[f32]>;

/// Realtime side of the pool (owned by `LooperCore`).
pub(crate) struct LayerSource {
    clean_rx: Consumer<LayerBuf>,
    dirty_tx: Producer<LayerBuf>,
}

impl LayerSource {
    /// Take a zeroed buffer, if any are free. Wait-free.
    pub fn acquire(&mut self) -> Option<LayerBuf> {
        self.clean_rx.pop().ok()
    }

    /// Return a used buffer for zeroing off the realtime thread. Wait-free.
    pub fn release(&mut self, buf: LayerBuf) {
        match self.dirty_tx.push(buf) {
            Ok(()) => {}
            // Unreachable (ring capacity == pool size). Never free on the
            // realtime thread: leak instead and let the pool shrink.
            Err(PushError::Full(buf)) => std::mem::forget(buf),
        }
    }

    /// Clean buffers immediately available.
    pub fn available(&self) -> usize {
        self.clean_rx.slots()
    }
}

/// Control-thread side of the pool. Call [`LayerRecycler::service`] regularly.
pub struct LayerRecycler {
    dirty_rx: Consumer<LayerBuf>,
    clean_tx: Producer<LayerBuf>,
    layer_frames: usize,
    total_layers: usize,
}

impl LayerRecycler {
    /// Zero every released buffer and return it to the pool. Returns how many were recycled.
    pub fn service(&mut self) -> usize {
        let mut recycled = 0;
        while let Ok(mut buf) = self.dirty_rx.pop() {
            buf.fill(0.0);
            if let Err(PushError::Full(buf)) = self.clean_tx.push(buf) {
                // Unreachable: capacity == pool size. Dropping here is safe (not realtime).
                drop(buf);
            }
            recycled += 1;
        }
        recycled
    }

    pub fn layer_frames(&self) -> usize {
        self.layer_frames
    }

    pub fn total_layers(&self) -> usize {
        self.total_layers
    }
}

pub(crate) fn allocate(layers: usize, frames: usize) -> (LayerSource, LayerRecycler) {
    let (mut clean_tx, clean_rx) = RingBuffer::new(layers);
    let (dirty_tx, dirty_rx) = RingBuffer::new(layers);
    for _ in 0..layers {
        let buf: LayerBuf = vec![0.0f32; frames].into_boxed_slice();
        if clean_tx.push(buf).is_err() {
            break;
        }
    }
    (
        LayerSource { clean_rx, dirty_tx },
        LayerRecycler { dirty_rx, clean_tx, layer_frames: frames, total_layers: layers },
    )
}

/// **Future interface (not implemented in 1A): background disk streaming.**
///
/// Planned shape: the input callback additionally pushes raw (unfaded) input
/// frames into a large SPSC ring. A disk-writer thread drains it into WAV files
/// under `audio/` and reports completed takes to the control thread, which
/// records them in `project.json`. Layers in memory then become a playback
/// cache that can be rebuilt from disk, so long sessions are no longer
/// bounded by `LooperConfig::pool_layers`.
///
/// Implementations run on a normal thread and may block and allocate.
pub trait StorageSink: Send {
    /// A new take begins on `track` at transport frame `start_frame`.
    fn begin_take(&mut self, track: u8, start_frame: i64, sample_rate: u32);
    /// Raw mono input frames for the active take, in order.
    fn write_frames(&mut self, frames: &[f32]);
    /// The take ended; `length` frames belong to it.
    fn end_take(&mut self, track: u8, length: u64);
}
