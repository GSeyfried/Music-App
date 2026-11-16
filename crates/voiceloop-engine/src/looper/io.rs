//! CPAL output stream hosting [`LooperCore`]. The output device's callback is
//! the master clock.
//!
//! Input arrives from the input callback as mono f32 through an SPSC ring.
//! The two devices have independent clocks, so the bridge:
//! - **primes:** waits until ~10 ms of input is buffered before consuming (no
//!   underruns counted while priming). That buffer is part of round-trip latency.
//! - **underrun:** if fewer input frames are available than the block needs,
//!   substitutes silence and counts the missing frames.
//! - **drift:** if the backlog exceeds 8× the prime level, skips the excess
//!   and counts it. (Proper drift resampling with `rubato` is future work.)
//!
//! Hardware-dependent: compiled and exercised only on a real machine.

use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use rtrb::Consumer;
use serde::Serialize;

use super::{IoCounters, LooperConfig, LooperCore, LooperHandles};
use crate::devices;
use crate::error::EngineError;
use crate::transport::TransportState;

/// Largest block processed in one `LooperCore::process` call (scratch buffer size).
pub const MAX_BLOCK: usize = 4096;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    pub device_id: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// A running output stream with the looper inside. Dropping it stops audio
/// and frees the looper (on the dropping thread, not the audio thread).
pub struct LooperPlayback {
    stream: cpal::Stream,
    pub handles: LooperHandles,
    pub counters: Arc<IoCounters>,
    pub info: OutputInfo,
    pub memory_bytes: usize,
}

/// Default output device and its default configuration.
pub fn default_output() -> Result<(cpal::Device, cpal::SupportedStreamConfig), EngineError> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| EngineError::Backend("no output device is available".into()))?;
    let config = device.default_output_config()?;
    Ok((device, config))
}

impl LooperPlayback {
    pub fn start(
        device: cpal::Device,
        supported: cpal::SupportedStreamConfig,
        looper: &LooperConfig,
        transport: TransportState,
        input_rx: Consumer<f32>,
        counters: Arc<IoCounters>,
        stream_errors: mpsc::Sender<String>,
    ) -> Result<Self, EngineError> {
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let sample_rate = config.sample_rate.0;
        let (core, handles) = LooperCore::new(looper, transport, sample_rate)?;
        let memory_bytes = looper.memory_bytes(sample_rate);

        use cpal::SampleFormat as F;
        let c = counters.clone();
        let stream = match sample_format {
            F::F32 => build::<f32>(&device, &config, core, input_rx, c, stream_errors),
            F::I16 => build::<i16>(&device, &config, core, input_rx, c, stream_errors),
            F::U16 => build::<u16>(&device, &config, core, input_rx, c, stream_errors),
            F::I32 => build::<i32>(&device, &config, core, input_rx, c, stream_errors),
            F::F64 => build::<f64>(&device, &config, core, input_rx, c, stream_errors),
            other => return Err(EngineError::UnsupportedFormat(other.to_string())),
        }?;
        stream.play()?;

        Ok(Self {
            stream,
            handles,
            counters,
            info: OutputInfo {
                device_id: devices::name_of(&device),
                sample_rate,
                channels: config.channels,
                sample_format: sample_format.to_string(),
            },
            memory_bytes,
        })
    }

    pub fn stop(self) {
        let _ = self.stream.pause();
        drop(self);
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut core: LooperCore,
    mut input_rx: Consumer<f32>,
    counters: Arc<IoCounters>,
    stream_errors: mpsc::Sender<String>,
) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = (config.channels as usize).max(1);
    let sample_rate = config.sample_rate.0 as usize;
    let prime = (sample_rate / 100).max(256);
    let high_water = prime * 8;
    let mut primed = false;
    // Scratch buffers are allocated here, before the callback exists.
    let mut in_buf = vec![0.0f32; MAX_BLOCK];
    let mut out_buf = vec![0.0f32; MAX_BLOCK];

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
            // REALTIME: no allocation, locks, blocking, or I/O below.
            let frames = data.len() / channels;
            let mut done = 0usize;
            while done < frames {
                let n = (frames - done).min(MAX_BLOCK);

                let mut available = input_rx.slots();
                if !primed && available >= prime {
                    primed = true;
                }
                if primed && available > high_water {
                    let skip = available - prime;
                    if let Ok(chunk) = input_rx.read_chunk(skip) {
                        chunk.commit_all();
                    }
                    counters.drift_dropped_frames.fetch_add(skip as u64, Ordering::Relaxed);
                    available = input_rx.slots();
                }
                let take = if primed { available.min(n) } else { 0 };
                if take > 0 {
                    if let Ok(chunk) = input_rx.read_chunk(take) {
                        let (a, b) = chunk.as_slices();
                        in_buf[..a.len()].copy_from_slice(a);
                        in_buf[a.len()..a.len() + b.len()].copy_from_slice(b);
                        chunk.commit_all();
                    }
                }
                in_buf[take..n].fill(0.0);
                if primed && take < n {
                    counters.input_underrun_frames.fetch_add((n - take) as u64, Ordering::Relaxed);
                }

                core.process(&in_buf[..n], &mut out_buf[..n]);

                for (f, &v) in out_buf[..n].iter().enumerate() {
                    let sample = <T as FromSample<f32>>::from_sample_(v);
                    let base = (done + f) * channels;
                    for ch in 0..channels {
                        data[base + ch] = sample;
                    }
                }
                done += n;
            }
        },
        move |err| {
            // Error callback runs on a backend thread, not the realtime callback.
            let _ = stream_errors.send(err.to_string());
        },
        None,
    )?;
    Ok(stream)
}
