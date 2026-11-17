//! Input capture. The closure passed to CPAL here is the realtime callback.
//! It converts samples to f32 on the fly, feeds the meter accumulator, pushes
//! completed [`MeterFrame`]s into a wait-free ring, and pushes a mono downmix
//! into the looper's and the analysis thread's input rings when those are
//! attached. No allocation, locking, or I/O happens inside it.
//!
//! Monitoring (routing input to speakers) is intentionally not implemented;
//! it is off by default to avoid feedback.

use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use serde::Serialize;

use crate::analysis::AnalysisFeed;
use crate::devices;
use crate::error::EngineError;
use crate::looper::IoCounters;
use crate::meter::{MeterAccumulator, MeterFrame};

/// Meter windows per second produced by the audio thread (UI throttles further).
const METER_WINDOWS_PER_SECOND: u32 = 100;
/// Ring capacity in meter frames (~2.5 s of backlog before drops).
pub const METER_RING_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    pub device_id: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
}

/// Where the input callback sends mono audio for the looper.
pub struct LooperFeed {
    pub tx: rtrb::Producer<f32>,
    pub counters: Arc<IoCounters>,
}

/// Consumers of the mono downmix, fed from the input callback.
#[derive(Default)]
pub struct InputTaps {
    pub looper: Option<LooperFeed>,
    pub analysis: Option<AnalysisFeed>,
}

/// A running input stream. Dropping it stops capture and releases the device.
pub struct InputCapture {
    // Field order matters: the stream must drop (stop callbacks) before the consumer.
    stream: cpal::Stream,
    pub meter_rx: rtrb::Consumer<MeterFrame>,
    /// Frames per meter window.
    pub meter_window: u64,
    pub info: CaptureInfo,
}

impl InputCapture {
    /// Start capturing. `rate` forces a sample rate (the looper's output rate);
    /// `taps` receive a mono downmix.
    pub fn start(
        device_id: Option<&str>,
        rate: Option<u32>,
        taps: InputTaps,
        stream_errors: mpsc::Sender<String>,
    ) -> Result<Self, EngineError> {
        let device = devices::find_input_device(device_id)?;
        let supported = match rate {
            Some(r) => input_config_at_rate(&device, r)?,
            None => device.default_input_config()?,
        };
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        let window = (config.sample_rate.0 / METER_WINDOWS_PER_SECOND).max(1);
        let (producer, consumer) = rtrb::RingBuffer::<MeterFrame>::new(METER_RING_CAPACITY);

        use cpal::SampleFormat as F;
        let stream = match sample_format {
            F::F32 => build::<f32>(&device, &config, window, producer, taps, stream_errors),
            F::I16 => build::<i16>(&device, &config, window, producer, taps, stream_errors),
            F::U16 => build::<u16>(&device, &config, window, producer, taps, stream_errors),
            F::I32 => build::<i32>(&device, &config, window, producer, taps, stream_errors),
            F::I8 => build::<i8>(&device, &config, window, producer, taps, stream_errors),
            F::U8 => build::<u8>(&device, &config, window, producer, taps, stream_errors),
            F::F64 => build::<f64>(&device, &config, window, producer, taps, stream_errors),
            other => return Err(EngineError::UnsupportedFormat(other.to_string())),
        }?;
        stream.play()?;

        Ok(Self {
            stream,
            meter_rx: consumer,
            meter_window: window as u64,
            info: CaptureInfo {
                device_id: devices::name_of(&device),
                sample_rate: config.sample_rate.0,
                channels: config.channels,
                sample_format: sample_format.to_string(),
            },
        })
    }

    pub fn stop(self) {
        let _ = self.stream.pause();
        drop(self);
    }
}

/// Pick an input configuration running at `rate` (the output device's rate),
/// preferring the device's default channel count and sample format.
fn input_config_at_rate(device: &cpal::Device, rate: u32) -> Result<cpal::SupportedStreamConfig, EngineError> {
    let default = device.default_input_config()?;
    if default.sample_rate().0 == rate {
        return Ok(default);
    }
    let ranges = device.supported_input_configs().map_err(|e| EngineError::Backend(e.to_string()))?;
    let mut fallback: Option<cpal::SupportedStreamConfig> = None;
    for range in ranges {
        if range.min_sample_rate().0 <= rate && rate <= range.max_sample_rate().0 {
            let preferred = range.sample_format() == default.sample_format() && range.channels() == default.channels();
            let cfg = range.with_sample_rate(cpal::SampleRate(rate));
            if preferred {
                return Ok(cfg);
            }
            if fallback.is_none() {
                fallback = Some(cfg);
            }
        }
    }
    fallback.ok_or_else(|| {
        EngineError::UnsupportedFormat(format!(
            "input device cannot run at the output rate of {rate} Hz (its default is {} Hz)",
            default.sample_rate().0
        ))
    })
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    window_frames: u32,
    mut producer: rtrb::Producer<MeterFrame>,
    taps: InputTaps,
    stream_errors: mpsc::Sender<String>,
) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = (config.channels as usize).max(1);
    let inv_channels = 1.0 / channels as f32;
    let mut meter = MeterAccumulator::new(config.channels, window_frames);
    let InputTaps { mut looper, mut analysis } = taps;
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _info: &cpal::InputCallbackInfo| {
            // REALTIME: no allocation, locks, or blocking below.
            meter.process(data.iter().map(|s| s.to_sample::<f32>()), |frame| {
                // If the UI side falls behind, drop the reading rather than block.
                let _ = producer.push(frame);
            });
            if looper.is_some() || analysis.is_some() {
                let (mut looper_dropped, mut analysis_dropped) = (0u64, 0u64);
                for frame in data.chunks_exact(channels) {
                    let mut sum = 0.0f32;
                    for s in frame {
                        sum += s.to_sample::<f32>();
                    }
                    let mono = sum * inv_channels;
                    if let Some(feed) = looper.as_mut() {
                        if feed.tx.push(mono).is_err() {
                            looper_dropped += 1;
                        }
                    }
                    // Analysis is best effort: a full ring means its thread is
                    // behind, and dropping here never affects recording.
                    if let Some(feed) = analysis.as_mut() {
                        if feed.tx.push(mono).is_err() {
                            analysis_dropped += 1;
                        }
                    }
                }
                if looper_dropped > 0 {
                    if let Some(feed) = looper.as_ref() {
                        feed.counters.input_overrun_frames.fetch_add(looper_dropped, Ordering::Relaxed);
                    }
                }
                if analysis_dropped > 0 {
                    if let Some(feed) = analysis.as_ref() {
                        feed.dropped.fetch_add(analysis_dropped, Ordering::Relaxed);
                    }
                }
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
