//! CPAL output stream hosting [`LooperCore`]. The output device's callback is
//! the master clock.
//!
//! Input arrives from the input callback as the routed mono signal through an
//! SPSC ring. The two devices have independent clocks; [`InputBridge`]
//! (unit-tested in `bridge.rs`) handles priming, underruns, and drift.
//!
//! Per callback this also: publishes a clock anchor carrying the transport
//! frame (`StreamStats`, used to map MIDI and input times onto the transport),
//! counts timestamp discontinuities (xruns), mixes in the live input when
//! monitoring is enabled (opt-in, default off, gain slewed to avoid clicks),
//! and writes the mono mix only to the channels the [`OutputRoute`] selects
//! (others, and any trailing partial frame, are silence).
//!
//! Hardware-dependent: this file is exercised only on a real machine.

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use rtrb::Consumer;
use serde::Serialize;

use super::bridge::InputBridge;
use super::{soft_limit, IoCounters, LooperConfig, LooperCore, LooperHandles};
use crate::clock::{self, add_saturating, Anchor, StreamStats, XrunDetector};
use crate::devices;
use crate::error::EngineError;
use crate::routing::{MonitorCell, MonitorConfig, OutputRoute, RouteCell};
use crate::transport::TransportState;

/// Largest block processed in one `LooperCore::process` call (scratch buffer size).
pub const MAX_BLOCK: usize = 4096;
/// Frames for the monitor gain to travel from 0 to 1.
pub const MONITOR_SLEW_FRAMES: f32 = 512.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    pub device_id: String,
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
    /// Channels carrying the engine's mix.
    pub route: OutputRoute,
    pub monitor: MonitorConfig,
    /// Buffer size requested from the device (`None` = device default).
    pub requested_buffer_size: Option<u32>,
    /// Frames in the most recent callback (`None` until the first one).
    pub block_frames: Option<u32>,
    /// Callback-to-playback latency the backend reports, in frames (CPAL 0.16
    /// on CoreAudio assumes one buffer; real latency comes from calibration).
    pub reported_latency_frames: Option<u32>,
    /// Callback timestamp jumps that mean the device skipped output (xruns).
    pub discontinuities: u64,
    pub lost_frames: u64,
}

/// A running output stream with the looper inside. Dropping it stops audio
/// and frees the looper (on the dropping thread, not the audio thread).
pub struct LooperPlayback {
    stream: cpal::Stream,
    pub handles: LooperHandles,
    pub counters: Arc<IoCounters>,
    /// Fixed facts; see [`LooperPlayback::output_info`] for live values.
    pub info: OutputInfo,
    pub memory_bytes: usize,
    /// Clock anchor (output frames and transport frame ↔ host time), block size, xruns, errors.
    pub stats: Arc<StreamStats>,
    route: Arc<RouteCell>,
    monitor: Arc<MonitorCell>,
    monitor_config: MonitorConfig,
}

/// Default output device and its default configuration.
pub fn default_output() -> Result<(cpal::Device, cpal::SupportedStreamConfig), EngineError> {
    let host = cpal::default_host();
    let device =
        host.default_output_device().ok_or_else(|| EngineError::Backend("no output device is available".into()))?;
    let config = device.default_output_config()?;
    Ok((device, config))
}

/// Output-side choices made when audio starts.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputOptions {
    pub buffer_size: Option<u32>,
    pub route: OutputRoute,
    pub monitor: MonitorConfig,
}

impl LooperPlayback {
    pub fn start(
        device: cpal::Device,
        supported: cpal::SupportedStreamConfig,
        looper: &LooperConfig,
        transport: TransportState,
        input_rx: Consumer<f32>,
        counters: Arc<IoCounters>,
        options: OutputOptions,
    ) -> Result<Self, EngineError> {
        let sample_format = supported.sample_format();
        let (buffer, _) = devices::buffer_size_for(options.buffer_size, supported.buffer_size());
        let mut config: cpal::StreamConfig = supported.into();
        config.buffer_size = buffer;
        let sample_rate = config.sample_rate.0;
        let (core, handles) = LooperCore::new(looper, transport, sample_rate)?;
        let memory_bytes = looper.memory_bytes(sample_rate);
        let (route, _) = options.route.resolve(config.channels);
        let stats = Arc::new(StreamStats::default());
        let route_cell = Arc::new(RouteCell::output(route));
        let monitor = Arc::new(MonitorCell::new(options.monitor));
        clock::init_epoch();

        let parts = Parts {
            core,
            input_rx,
            counters: counters.clone(),
            stats: stats.clone(),
            route: route_cell.clone(),
            monitor: monitor.clone(),
        };
        use cpal::SampleFormat as F;
        let stream = match sample_format {
            F::F32 => build::<f32>(&device, &config, parts),
            F::I16 => build::<i16>(&device, &config, parts),
            F::U16 => build::<u16>(&device, &config, parts),
            F::I32 => build::<i32>(&device, &config, parts),
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
            handles,
            counters,
            info: OutputInfo {
                device_id: devices::name_of(&device),
                sample_rate,
                channels: config.channels,
                sample_format: sample_format.to_string(),
                route,
                monitor: options.monitor,
                requested_buffer_size,
                block_frames: None,
                reported_latency_frames: None,
                discontinuities: 0,
                lost_frames: 0,
            },
            memory_bytes,
            stats,
            route: route_cell,
            monitor,
            monitor_config: options.monitor,
        })
    }

    /// Current facts, including live block size, latency report, and counters.
    pub fn output_info(&self) -> OutputInfo {
        use std::sync::atomic::Ordering::Relaxed;
        let s = &self.stats;
        OutputInfo {
            route: self.route.output_route(),
            monitor: self.monitor_config,
            block_frames: s.block(),
            reported_latency_frames: s.reported_latency_frames(self.info.sample_rate),
            discontinuities: s.discontinuities.load(Relaxed),
            lost_frames: s.lost_frames.load(Relaxed),
            ..self.info.clone()
        }
    }

    /// Change the output channels while running. Returns the route applied and
    /// whether it had to be adjusted to fit the device.
    pub fn set_route(&self, route: OutputRoute) -> (OutputRoute, bool) {
        let (route, adjusted) = route.resolve(self.info.channels);
        self.route.set_output(route);
        (route, adjusted)
    }

    /// Enable/disable monitoring or change its gain while running (slewed in the callback).
    pub fn set_monitor(&mut self, monitor: MonitorConfig) {
        self.monitor.set(monitor);
        self.monitor_config = monitor;
    }

    pub fn monitor(&self) -> MonitorConfig {
        self.monitor_config
    }

    pub fn stop(self) {
        let _ = self.stream.pause();
        drop(self);
    }
}

struct Parts {
    core: LooperCore,
    input_rx: Consumer<f32>,
    counters: Arc<IoCounters>,
    stats: Arc<StreamStats>,
    route: Arc<RouteCell>,
    monitor: Arc<MonitorCell>,
}

/// Move `gain` toward `target` by at most `step`. Realtime-safe.
#[inline]
pub fn slew(gain: f32, target: f32, step: f32) -> f32 {
    if gain < target {
        (gain + step).min(target)
    } else {
        (gain - step).max(target)
    }
}

/// Mix monitored input into the looper output in place: `out = limit(out + g·in)`,
/// with `g` slewing toward `target`. Returns the final gain. With the gain at
/// rest at zero the looper output is left bit-exact. Realtime-safe.
#[inline]
pub fn mix_monitor(out: &mut [f32], input: &[f32], mut gain: f32, target: f32) -> f32 {
    if gain == 0.0 && target == 0.0 {
        return 0.0;
    }
    let step = 1.0 / MONITOR_SLEW_FRAMES;
    for (o, &i) in out.iter_mut().zip(input) {
        gain = slew(gain, target, step);
        *o = soft_limit(*o + gain * i);
    }
    gain
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, parts: Parts) -> Result<cpal::Stream, EngineError>
where
    T: SizedSample + FromSample<f32>,
{
    let Parts { mut core, mut input_rx, counters, stats, route, monitor } = parts;
    let channels = (config.channels as usize).max(1);
    let sample_rate = config.sample_rate.0;
    let mut bridge = InputBridge::new(sample_rate, input_rx.buffer().capacity());
    counters.input_cushion_frames.store(bridge.cushion() as u64, std::sync::atomic::Ordering::Relaxed);
    // Scratch buffers are allocated here, before the callback exists.
    let mut in_buf = vec![0.0f32; MAX_BLOCK];
    let mut out_buf = vec![0.0f32; MAX_BLOCK];
    let mut xrun = XrunDetector::default();
    let mut origin: Option<cpal::StreamInstant> = None;
    let mut frames_out: u64 = 0;
    let mut prev_t = core.transport_frame();
    let mut monitor_gain = 0.0f32;
    let err_stats = stats.clone();

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            // REALTIME: no allocation, locks, blocking, logging, or I/O below.
            let frames = data.len() / channels;
            let ts = info.timestamp();
            let origin = *origin.get_or_insert(ts.callback);
            let latency_ns = ts.playback.duration_since(&ts.callback).map(|d| d.as_nanos() as u64);
            let t = core.transport_frame();
            let anchor = Anchor {
                host_ns: clock::host_now_ns(),
                frame: frames_out,
                transport_frame: t,
                transport_running: t != prev_t,
            };
            prev_t = t;
            stats.on_block(anchor, frames, latency_ns);
            if let Some(cb) = ts.callback.duration_since(&origin) {
                if let Some(lost) = xrun.observe(cb.as_nanos() as u64, frames, sample_rate) {
                    stats.on_discontinuity(lost);
                }
            }
            let route = route.output_route();
            let monitor_target = monitor.target_gain();

            let mut done = 0usize;
            while done < frames {
                let n = (frames - done).min(MAX_BLOCK);

                let report = bridge.fill(&mut input_rx, &mut in_buf[..n]);
                add_saturating(&counters.drift_dropped_frames, report.dropped as u64);
                if report.underrun > 0 {
                    add_saturating(&counters.input_underrun_frames, report.underrun as u64);
                    counters.input_cushion_frames.store(bridge.cushion() as u64, std::sync::atomic::Ordering::Relaxed);
                }

                core.process(&in_buf[..n], &mut out_buf[..n]);
                monitor_gain = mix_monitor(&mut out_buf[..n], &in_buf[..n], monitor_gain, monitor_target);

                for (f, &v) in out_buf[..n].iter().enumerate() {
                    let sample = <T as FromSample<f32>>::from_sample_(v);
                    let base = (done + f) * channels;
                    for (ch, slot) in data[base..base + channels].iter_mut().enumerate() {
                        *slot = if route.carries(ch) { sample } else { T::EQUILIBRIUM };
                    }
                }
                done += n;
            }
            // A trailing partial frame (never produced by a conforming backend) must not replay stale data.
            data[frames * channels..].fill(T::EQUILIBRIUM);
            frames_out = frames_out.saturating_add(frames as u64);
        },
        move |err| {
            // May run on the realtime thread: store a code only (formatted on the control thread).
            err_stats.on_error(devices::stream_error_code(err));
        },
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitoring_off_leaves_the_looper_output_bit_exact() {
        let mut out = [0.9f32, -0.95, 0.1];
        let g = mix_monitor(&mut out, &[1.0, 1.0, 1.0], 0.0, 0.0);
        assert_eq!((g, out), (0.0, [0.9, -0.95, 0.1]));
    }

    #[test]
    fn monitor_gain_slews_in_and_out_without_jumps_and_is_limited() {
        let input = vec![0.5f32; 2_048];
        let mut out = vec![0.0f32; 2_048];
        let g = mix_monitor(&mut out, &input, 0.0, 1.0);
        assert_eq!(g, 1.0);
        assert!((out[0] - 0.5 / MONITOR_SLEW_FRAMES).abs() < 1e-6, "starts near silence");
        assert!(out.windows(2).all(|w| w[1] >= w[0] && w[1] - w[0] <= 0.5 / MONITOR_SLEW_FRAMES + 1e-6));
        assert_eq!(out[2_047], 0.5);
        let mut loud = vec![0.7f32; 1_024];
        let g = mix_monitor(&mut loud, &vec![1.0f32; 1_024], g, 2.0);
        assert!(loud.iter().all(|&v| v > 0.8 && v <= 1.0), "the soft limiter keeps the sum within full scale");
        let mut out = vec![0.0f32; 2_048];
        assert_eq!(mix_monitor(&mut out, &input, g, 0.0), 0.0);
        assert_eq!(*out.last().unwrap(), 0.0);
    }
}
