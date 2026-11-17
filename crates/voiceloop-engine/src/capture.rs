//! Input capture. The closure passed to CPAL here is the realtime callback.
//!
//! Per callback it: publishes a clock anchor and block statistics
//! ([`StreamStats`]), checks the hardware timestamp for skipped time
//! (xruns), converts every sample to `f32` and replaces non-finite values
//! with silence (counted), applies the [`InputRoute`] to get the engine's
//! signal, meters every device channel plus the routed signal, and pushes the
//! routed signal into the looper's and the analysis thread's rings when those
//! are attached. No allocation, locking, logging, or I/O happens inside it;
//! the frame scratch buffer is allocated before the stream starts.
//!
//! Frame alignment: CPAL delivers whole interleaved frames. A trailing
//! partial frame (never seen from a conforming backend) is ignored.
//!
//! Monitoring (input to speakers) happens in the output callback
//! (`looper/io.rs`), off by default.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use serde::Serialize;

use crate::analysis::AnalysisFeed;
use crate::clock::{self, add_saturating, Anchor, StreamStats, XrunDetector};
use crate::devices;
use crate::error::EngineError;
use crate::looper::IoCounters;
use crate::meter::{MeterAccumulator, MeterFrame};
use crate::routing::{InputRoute, RouteCell};

/// Meter windows per second produced by the audio thread (UI throttles further).
const METER_WINDOWS_PER_SECOND: u32 = 100;
/// Ring capacity in meter frames (~2.5 s of backlog before drops).
pub const METER_RING_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    pub device_id: String,
    pub sample_rate: u32,
    /// Channels the device delivers (all are metered; the route picks what the engine uses).
    pub channels: u16,
    pub sample_format: String,
    /// The route currently applied.
    pub route: InputRoute,
    /// Buffer size requested from the device (`None` = device default).
    pub requested_buffer_size: Option<u32>,
    /// Frames in the most recent callback (`None` until the first one).
    pub block_frames: Option<u32>,
    /// Capture-to-callback latency the backend reports, in frames. CPAL 0.16
    /// on CoreAudio assumes one buffer; real latency comes from calibration.
    pub reported_latency_frames: Option<u32>,
    /// Callback timestamp jumps that mean the backend dropped input (xruns).
    pub discontinuities: u64,
    /// Estimated input frames lost in those jumps.
    pub lost_frames: u64,
    /// Non-finite samples from the device replaced by silence.
    pub non_finite_samples: u64,
}

/// Where the input callback sends the routed signal for the looper.
pub struct LooperFeed {
    pub tx: rtrb::Producer<f32>,
    pub counters: Arc<IoCounters>,
}

/// Consumers of the routed signal, fed from the input callback.
#[derive(Default)]
pub struct InputTaps {
    pub looper: Option<LooperFeed>,
    pub analysis: Option<AnalysisFeed>,
}

/// Everything needed to open an input stream (resolved by the control thread).
pub struct CaptureRequest {
    pub device: cpal::Device,
    pub config: cpal::SupportedStreamConfig,
    /// Frames per callback to request; `None` = device default.
    pub buffer_size: Option<u32>,
    pub route: InputRoute,
}

/// A running input stream. Dropping it stops capture and releases the device.
pub struct InputCapture {
    // Field order matters: the stream must drop (stop callbacks) before the consumer.
    stream: cpal::Stream,
    pub meter_rx: rtrb::Consumer<MeterFrame>,
    /// Frames per meter window.
    pub meter_window: u64,
    /// Fixed facts about the stream; see [`InputCapture::info`] for live values.
    pub info: CaptureInfo,
    /// Clock anchor (input frames since capture start ↔ host time), block size, xruns, errors.
    pub stats: Arc<StreamStats>,
    /// Change with [`InputCapture::set_route`] while running.
    route: Arc<RouteCell>,
}

impl InputCapture {
    /// Open and start an input stream.
    pub fn open(request: CaptureRequest, taps: InputTaps) -> Result<Self, EngineError> {
        let CaptureRequest { device, config: supported, buffer_size, route } = request;
        let sample_format = supported.sample_format();
        let (buffer, _clamped) = devices::buffer_size_for(buffer_size, supported.buffer_size());
        let mut config: cpal::StreamConfig = supported.into();
        config.buffer_size = buffer;
        let (route, _) = route.resolve(config.channels);

        let window = (config.sample_rate.0 / METER_WINDOWS_PER_SECOND).max(1);
        let (producer, consumer) = rtrb::RingBuffer::<MeterFrame>::new(METER_RING_CAPACITY);
        let stats = Arc::new(StreamStats::default());
        let cell = Arc::new(RouteCell::input(route));
        clock::init_epoch();

        let parts = Parts { window, producer, taps, stats: stats.clone(), route: cell.clone() };
        use cpal::SampleFormat as F;
        let stream = match sample_format {
            F::F32 => build::<f32>(&device, &config, parts),
            F::I16 => build::<i16>(&device, &config, parts),
            F::U16 => build::<u16>(&device, &config, parts),
            F::I32 => build::<i32>(&device, &config, parts),
            F::I8 => build::<i8>(&device, &config, parts),
            F::U8 => build::<u8>(&device, &config, parts),
            F::F64 => build::<f64>(&device, &config, parts),
            other => return Err(EngineError::UnsupportedFormat(other.to_string())),
        }?;
        stream.play()?;

        let requested_buffer_size = match config.buffer_size {
            cpal::BufferSize::Fixed(n) => Some(n),
            cpal::BufferSize::Default => None,
        };
        Ok(Self {
            stream,
            meter_rx: consumer,
            meter_window: window as u64,
            info: CaptureInfo {
                device_id: devices::name_of(&device),
                sample_rate: config.sample_rate.0,
                channels: config.channels,
                sample_format: sample_format.to_string(),
                route,
                requested_buffer_size,
                block_frames: None,
                reported_latency_frames: None,
                discontinuities: 0,
                lost_frames: 0,
                non_finite_samples: 0,
            },
            stats,
            route: cell,
        })
    }

    /// Start capturing from a device id at an optional forced rate, with the
    /// default route and buffer size. Kept for callers of the original API.
    pub fn start(device_id: Option<&str>, rate: Option<u32>, taps: InputTaps) -> Result<Self, EngineError> {
        let device = devices::find_input_device(device_id)?;
        let config = match rate {
            Some(r) => devices::config_at_rate(&device, crate::model::DeviceDirection::Input, r)?,
            None => devices::default_config(&device, crate::model::DeviceDirection::Input)?,
        };
        Self::open(CaptureRequest { device, config, buffer_size: None, route: InputRoute::MixAll }, taps)
    }

    /// Current facts, including live block size, latency report, and counters.
    pub fn info(&self) -> CaptureInfo {
        use std::sync::atomic::Ordering::Relaxed;
        let s = &self.stats;
        CaptureInfo {
            route: self.route.input_route(),
            block_frames: s.block(),
            reported_latency_frames: s.reported_latency_frames(self.info.sample_rate),
            discontinuities: s.discontinuities.load(Relaxed),
            lost_frames: s.lost_frames.load(Relaxed),
            non_finite_samples: s.non_finite_samples.load(Relaxed),
            ..self.info.clone()
        }
    }

    /// Change the route while running. Returns the route applied (a route
    /// naming a channel the device lacks becomes `MixAll`) and whether it was adjusted.
    pub fn set_route(&self, route: InputRoute) -> (InputRoute, bool) {
        let (route, adjusted) = route.resolve(self.info.channels);
        self.route.set_input(route);
        (route, adjusted)
    }

    pub fn stop(self) {
        let _ = self.stream.pause();
        drop(self);
    }
}

struct Parts {
    window: u32,
    producer: rtrb::Producer<MeterFrame>,
    taps: InputTaps,
    stats: Arc<StreamStats>,
    route: Arc<RouteCell>,
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, parts: Parts) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let Parts { window, mut producer, taps, stats, route } = parts;
    let channels = (config.channels as usize).max(1);
    let sample_rate = config.sample_rate.0;
    let mut meter = MeterAccumulator::new(config.channels, window);
    let InputTaps { mut looper, mut analysis } = taps;
    // Allocated here, before the callback exists.
    let mut scratch = vec![0.0f32; channels];
    let mut xrun = XrunDetector::default();
    let mut origin: Option<cpal::StreamInstant> = None;
    let mut total_frames: u64 = 0;
    let err_stats = stats.clone();

    let stream = device.build_input_stream(
        config,
        move |data: &[T], info: &cpal::InputCallbackInfo| {
            // REALTIME: no allocation, locks, logging, or blocking below.
            let frames = data.len() / channels;
            let ts = info.timestamp();
            let origin = *origin.get_or_insert(ts.callback);
            let latency_ns = ts.callback.duration_since(&ts.capture).map(|d| d.as_nanos() as u64);
            let anchor = Anchor { host_ns: clock::host_now_ns(), frame: total_frames, ..Anchor::default() };
            stats.on_block(anchor, frames, latency_ns);
            if let Some(cb) = ts.callback.duration_since(&origin) {
                if let Some(lost) = xrun.observe(cb.as_nanos() as u64, frames, sample_rate) {
                    stats.on_discontinuity(lost);
                }
            }

            let route = route.input_route();
            let (mut looper_dropped, mut analysis_dropped, mut non_finite) = (0u64, 0u64, 0u64);
            for frame in data.chunks_exact(channels) {
                for (dst, s) in scratch.iter_mut().zip(frame) {
                    let v = s.to_sample::<f32>();
                    *dst = if v.is_finite() {
                        v
                    } else {
                        non_finite += 1;
                        0.0
                    };
                }
                let routed = route.mix(&scratch);
                meter.process_frame(&scratch, routed, |m| {
                    // If the UI side falls behind, drop the reading rather than block.
                    let _ = producer.push(m);
                });
                if let Some(feed) = looper.as_mut() {
                    if feed.tx.push(routed).is_err() {
                        looper_dropped += 1;
                    }
                }
                // Analysis is best effort: a full ring means its thread is
                // behind, and dropping here never affects recording.
                if let Some(feed) = analysis.as_mut() {
                    if feed.tx.push(routed).is_err() {
                        analysis_dropped += 1;
                    }
                }
            }
            total_frames = total_frames.saturating_add(frames as u64);
            add_saturating(&stats.non_finite_samples, non_finite);
            if let Some(feed) = looper.as_ref() {
                add_saturating(&feed.counters.input_overrun_frames, looper_dropped);
            }
            if let Some(feed) = analysis.as_ref() {
                add_saturating(&feed.dropped, analysis_dropped);
            }
        },
        move |err| {
            // May run on the realtime thread (CPAL 0.16 CoreAudio reports
            // timestamp failures from inside the callback): store a code only.
            err_stats.on_error(devices::stream_error_code(err));
        },
        None,
    )?;
    Ok(stream)
}
