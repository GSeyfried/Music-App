//! Input capture. The closure passed to CPAL here is the realtime callback:
//! it converts samples to f32 on the fly, feeds the meter accumulator, and
//! pushes completed [`MeterFrame`]s into a wait-free ring buffer. No
//! allocation, locking, or I/O happens inside it.
//!
//! Monitoring (routing input to speakers) is intentionally not implemented;
//! it is off by default to avoid feedback.

use std::sync::mpsc;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use serde::Serialize;

use crate::devices;
use crate::error::EngineError;
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
    pub fn start(
        device_id: Option<&str>,
        stream_errors: mpsc::Sender<String>,
    ) -> Result<Self, EngineError> {
        let device = devices::find_input_device(device_id)?;
        let supported = device.default_input_config()?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();

        let window = (config.sample_rate.0 / METER_WINDOWS_PER_SECOND).max(1);
        let (producer, consumer) = rtrb::RingBuffer::<MeterFrame>::new(METER_RING_CAPACITY);

        use cpal::SampleFormat as F;
        let stream = match sample_format {
            F::F32 => build::<f32>(&device, &config, window, producer, stream_errors),
            F::I16 => build::<i16>(&device, &config, window, producer, stream_errors),
            F::U16 => build::<u16>(&device, &config, window, producer, stream_errors),
            F::I32 => build::<i32>(&device, &config, window, producer, stream_errors),
            F::I8 => build::<i8>(&device, &config, window, producer, stream_errors),
            F::U8 => build::<u8>(&device, &config, window, producer, stream_errors),
            F::F64 => build::<f64>(&device, &config, window, producer, stream_errors),
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

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    window_frames: u32,
    mut producer: rtrb::Producer<MeterFrame>,
    stream_errors: mpsc::Sender<String>,
) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let mut meter = MeterAccumulator::new(config.channels, window_frames);
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _info: &cpal::InputCallbackInfo| {
            // REALTIME: no allocation, locks, or blocking below.
            meter.process(data.iter().map(|s| s.to_sample::<f32>()), |frame| {
                // If the UI side falls behind, drop the reading rather than block.
                let _ = producer.push(frame);
            });
        },
        move |err| {
            // Error callback runs on a backend thread, not the realtime callback.
            let _ = stream_errors.send(err.to_string());
        },
        None,
    )?;
    Ok(stream)
}
