//! CPAL output stream hosting [`LooperCore`]. The output device's callback is
//! the master clock.
//!
//! Input arrives from the input callback as the routed mono signal through an
//! SPSC ring. The two devices have independent clocks; [`InputBridge`]
//! (unit-tested in `bridge.rs`) handles priming, underruns, and drift. The
//! output runs with no input at all (silence) until the control thread
//! attaches one, and keeps running when it is detached (`input_link.rs`).
//!
//! Per callback this also: publishes a clock anchor (stamped at the block's
//! end) carrying the transport frame and the bridge's input position, ratio,
//! and epoch (`StreamStats`, used to map MIDI and input times onto the transport),
//! runs a loopback calibration when one is handed in (`CalibrationJob`),
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

use super::bridge::{BridgeInfo, BridgeStats, InputBridge};
use super::input_link::{input_link_pair, InputLink, InputLinkControl, InputPort};
use super::{soft_limit, IoCounters, LooperConfig, LooperCore, LooperHandles, RunState};
use crate::arrangement::{arrangement_pair, ArrangementControl, ArrangementPlayer};
use crate::clock::{self, add_saturating, Anchor, StreamStats, XrunDetector};
use crate::devices;
use crate::error::EngineError;
use crate::preview::{preview_pair, PreviewControl, PreviewPlayer};
use crate::song::{song_pair, SongControl, SongPlayer};
use crate::routing::{MonitorCell, MonitorConfig, OutputRoute, RouteCell};
use crate::transport::TransportState;

/// Largest block processed in one `LooperCore::process` call (scratch buffer size).
pub const MAX_BLOCK: usize = 4096;
/// Frames for the monitor gain to travel from 0 to 1.
pub const MONITOR_SLEW_FRAMES: f32 = 512.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputInfo {
    /// Stable device id (`DeviceId` text), as accepted by `inputDevice`/`outputDevice`.
    pub device_id: String,
    pub device_name: String,
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
    /// Input→output drift bridge: correction ratio, mean fill (its delay), breaks.
    pub bridge: BridgeInfo,
}

/// A loopback calibration run travelling between the control thread and the
/// output callback. Buffers are allocated (and freed) on the control thread;
/// the callback only moves the box through rings and writes into it.
#[derive(Debug)]
pub struct CalibrationJob {
    pub signal: Box<[f32]>,
    pub record: Box<[f32]>,
    pub played: usize,
    pub recorded: usize,
}

impl CalibrationJob {
    pub fn new(signal: &[f32], record_frames: usize) -> Self {
        Self { signal: signal.into(), record: vec![0.0; record_frames].into_boxed_slice(), played: 0, recorded: 0 }
    }

    pub fn is_done(&self) -> bool {
        self.recorded >= self.record.len()
    }

    /// One block: add the next test-signal samples to `out` (soft-limited) and
    /// record `input` (the bridged input the looper sees) at the same frames,
    /// so the lag between them is exactly the looper's round trip. Realtime-safe.
    #[inline]
    pub fn process(&mut self, out: &mut [f32], input: &[f32]) {
        for (o, &i) in out.iter_mut().zip(input) {
            if let Some(&s) = self.signal.get(self.played) {
                *o = soft_limit(*o + s);
                self.played += 1;
            }
            if let Some(slot) = self.record.get_mut(self.recorded) {
                *slot = i;
                self.recorded += 1;
            }
        }
    }
}

/// The output anchor for a callback that started at `host_start_ns` and
/// processed `frames`: stamped at the block's **end**, after the looper has
/// applied this block's commands, so play/pause/stop at a block boundary are
/// reflected and MIDI events in or after the block map in the right direction.
#[allow(clippy::too_many_arguments)]
pub fn block_end_anchor(
    host_start_ns: u64,
    frames: usize,
    sample_rate: u32,
    frames_out_end: u64,
    transport_end: i64,
    running: bool,
    bridge: &InputBridge,
) -> Anchor {
    Anchor {
        host_ns: host_start_ns + (frames as u64 * 1_000_000_000) / sample_rate.max(1) as u64,
        frame: frames_out_end,
        transport_frame: transport_end,
        transport_running: running,
        input_pos: bridge.input_position(),
        ratio: bridge.ratio(),
        epoch: bridge.epoch(),
    }
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
    pub bridge: Arc<BridgeStats>,
    calibration_tx: rtrb::Producer<CalibrationJob>,
    calibration_rx: Consumer<CalibrationJob>,
    /// Reference/scale preview instrument, mixed into the output only.
    pub preview: PreviewControl,
    /// Pocket Song Builder playback, mixed into the output only.
    pub song: SongControl,
    /// PRODUCE arrangement playback, mixed into the output after the looper.
    pub arrangement: ArrangementControl,
    /// Attach / detach the input stream while this output keeps running.
    pub input: InputLinkControl,
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
#[derive(Debug, Clone, Default)]
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
        input: Option<InputLink>,
        counters: Arc<IoCounters>,
        options: OutputOptions,
    ) -> Result<Self, EngineError> {
        let sample_format = supported.sample_format();
        let (buffer, _) = devices::buffer_size_for(options.buffer_size, supported.buffer_size());
        let mut config: cpal::StreamConfig = supported.into();
        config.buffer_size = buffer;
        let sample_rate = config.sample_rate;
        let (core, handles) = LooperCore::new(looper, transport, sample_rate)?;
        let memory_bytes = looper.memory_bytes(sample_rate);
        let (route, _) = options.route.resolve(config.channels);
        let stats = Arc::new(StreamStats::default());
        let route_cell = Arc::new(RouteCell::output(route));
        let monitor = Arc::new(MonitorCell::new(options.monitor));
        let bridge = Arc::new(BridgeStats::default());
        let (calibration_tx, cal_in) = rtrb::RingBuffer::<CalibrationJob>::new(1);
        let (cal_out, calibration_rx) = rtrb::RingBuffer::<CalibrationJob>::new(1);
        let (preview, preview_player) = preview_pair(None, sample_rate); // the engine sets the bank
        let (song, song_player) = song_pair(sample_rate);
        let (arrangement, arrangement_player) = arrangement_pair();
        let (input_control, input_port) = input_link_pair(sample_rate, input);
        clock::init_epoch();

        let parts = Parts {
            preview: preview_player,
            song: song_player,
            arrangement: arrangement_player,
            input: input_port,
            bridge: bridge.clone(),
            cal_in,
            cal_out,
            core,
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
                device_id: devices::id_of(&device),
                device_name: devices::name_of(&device),
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
                bridge: bridge.info(),
            },
            memory_bytes,
            stats,
            route: route_cell,
            monitor,
            monitor_config: options.monitor,
            bridge,
            calibration_tx,
            calibration_rx,
            preview,
            song,
            arrangement,
            input: input_control,
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
            bridge: self.bridge.info(),
            ..self.info.clone()
        }
    }

    /// Hand a calibration run to the output callback. `Busy` if one is in flight.
    pub fn start_calibration(&mut self, job: CalibrationJob) -> Result<(), EngineError> {
        self.calibration_tx.push(job).map_err(|_| EngineError::Busy("a calibration is already running".into()))
    }

    /// The finished calibration run, once the callback has filled its record buffer.
    pub fn take_calibration(&mut self) -> Option<CalibrationJob> {
        self.calibration_rx.pop().ok()
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
    preview: PreviewPlayer,
    song: SongPlayer,
    arrangement: ArrangementPlayer,
    /// Input from the attached input stream (silence while none is attached).
    input: InputPort,
    bridge: Arc<BridgeStats>,
    cal_in: Consumer<CalibrationJob>,
    cal_out: rtrb::Producer<CalibrationJob>,
    core: LooperCore,
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
    let Parts {
        mut preview,
        mut song,
        mut arrangement,
        mut input,
        bridge: bridge_stats,
        mut cal_in,
        mut cal_out,
        mut core,
        counters,
        stats,
        route,
        monitor,
    } = parts;
    let channels = (config.channels as usize).max(1);
    let sample_rate = config.sample_rate;
    counters.input_cushion_frames.store(input.bridge().cushion() as u64, std::sync::atomic::Ordering::Relaxed);
    // Scratch buffers are allocated here, before the callback exists.
    let mut in_buf = vec![0.0f32; MAX_BLOCK];
    let mut out_buf = vec![0.0f32; MAX_BLOCK];
    let mut xrun = XrunDetector::default();
    let mut origin: Option<cpal::StreamInstant> = None;
    let mut frames_out: u64 = 0;
    let mut monitor_gain = 0.0f32;
    let mut calibration: Option<CalibrationJob> = None;
    let err_stats = stats.clone();

    let stream = device.build_output_stream(
        *config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            // REALTIME: no allocation, locks, blocking, logging, or I/O below.
            let frames = data.len() / channels;
            let ts = info.timestamp();
            let origin = *origin.get_or_insert(ts.callback);
            let latency_ns = ts.playback.checked_duration_since(ts.callback).map(|d| d.as_nanos() as u64);
            let host_start = clock::host_now_ns();
            if let Some(cb) = ts.callback.checked_duration_since(origin) {
                if let Some(lost) = xrun.observe(cb.as_nanos() as u64, frames, sample_rate) {
                    stats.on_discontinuity(lost);
                }
            }
            let route = route.output_route();
            let monitor_target = monitor.target_gain();
            if calibration.is_none() {
                calibration = cal_in.pop().ok();
            }
            // Input link swaps, input overruns, and the input clock (see `input_link.rs`).
            input.begin_block(host_start, sample_rate, &counters);

            let mut done = 0usize;
            while done < frames {
                let n = (frames - done).min(MAX_BLOCK);

                let report = input.fill(&mut in_buf[..n]);
                if report.discontinuity {
                    core.apply_now(super::LooperCommand::InputDiscontinuity); // flags takes in progress
                }
                add_saturating(&counters.drift_dropped_frames, report.dropped as u64);
                if report.underrun > 0 {
                    add_saturating(&counters.input_underrun_frames, report.underrun as u64);
                    counters
                        .input_cushion_frames
                        .store(input.bridge().cushion() as u64, std::sync::atomic::Ordering::Relaxed);
                }

                core.process(&in_buf[..n], &mut out_buf[..n]);
                monitor_gain = mix_monitor(&mut out_buf[..n], &in_buf[..n], monitor_gain, monitor_target);
                // After the looper: heard, never recorded.
                arrangement.process(&mut out_buf[..n]);
                preview.process(&mut out_buf[..n]);
                song.process(&mut out_buf[..n], frames_out + done as u64);
                if let Some(job) = calibration.as_mut() {
                    job.process(&mut out_buf[..n], &in_buf[..n]);
                }

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

            // Return a finished calibration (moving the box; nothing is freed here).
            if calibration.as_ref().is_some_and(CalibrationJob::is_done) {
                if let Some(job) = calibration.take() {
                    if let Err(rtrb::PushError::Full(job)) = cal_out.push(job) {
                        calibration = Some(job);
                    }
                }
            }
            input.end_block();
            let bridge = input.bridge();
            bridge_stats.publish(bridge);
            let running = core.snapshot().run == RunState::Running;
            let anchor =
                block_end_anchor(host_start, frames, sample_rate, frames_out, core.transport_frame(), running, bridge);
            stats.on_block(anchor, frames, latency_ns);
        },
        move |err| {
            // May run on the realtime thread: store a code only (formatted on the control thread).
            err_stats.on_stream_error(devices::stream_error_code(err));
        },
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::looper::LooperCommand;

    /// Run one 256-frame block of a real looper and return its end-of-block anchor.
    fn block(core: &mut LooperCore, bridge: &InputBridge, index: u64) -> Anchor {
        let input = [0.0f32; 256];
        let mut out = [0.0f32; 256];
        core.process(&input, &mut out);
        let running = core.snapshot().run == RunState::Running;
        let host_start = index * 256 * 1_000_000_000 / 48_000;
        block_end_anchor(host_start, 256, 48_000, (index + 1) * 256, core.transport_frame(), running, bridge)
    }

    #[test]
    fn midi_times_inside_play_and_pause_blocks_map_to_the_transport_state_of_that_block() {
        let (mut core, mut handles) =
            LooperCore::new(&LooperConfig::default(), TransportState::default(), 48_000).unwrap();
        let bridge = InputBridge::new(48_000, 24_000);
        let ns = |frame: u64| frame * 1_000_000_000 / 48_000;

        let a0 = block(&mut core, &bridge, 0);
        assert!(!a0.transport_running);
        assert_eq!(a0.transport_frame_at(ns(200), 48_000), 0, "stopped: frozen");

        // Play is applied at the start of block 1 (frames 256..512).
        handles.commands.push(LooperCommand::Play).unwrap();
        let a1 = block(&mut core, &bridge, 1);
        assert!(a1.transport_running, "the block that applied Play is reported running");
        // A note 100 frames into that block is 100 transport frames after Play.
        assert_eq!(a1.transport_frame_at(ns(256 + 100), 48_000), 100);
        // And one 50 frames into the next block extrapolates forward.
        assert_eq!(a1.transport_frame_at(ns(512 + 50), 48_000), 306);

        let a2 = block(&mut core, &bridge, 2);
        assert_eq!(a2.transport_frame, 512);

        // Pause applied at the start of block 3: every time in it maps to the pause point.
        handles.commands.push(LooperCommand::Pause).unwrap();
        let a3 = block(&mut core, &bridge, 3);
        assert!(!a3.transport_running, "the block that applied Pause is reported paused");
        assert_eq!(a3.transport_frame_at(ns(768 + 100), 48_000), 512);
        assert_eq!(a3.transport_frame_at(ns(1_024 + 100), 48_000), 512);
    }

    #[test]
    fn variable_callback_sizes_keep_output_and_transport_anchors_aligned() {
        const SR: u32 = 48_000;
        let sizes = [1usize, 127, 3, 191, 17, 257, 5, 511, 64];
        let (mut core, mut handles) = LooperCore::new(&LooperConfig::default(), TransportState::default(), SR).unwrap();
        let bridge = InputBridge::new(SR, 24_000);
        handles.commands.push(LooperCommand::Play).unwrap();
        let ns = |frame: u64| frame * 1_000_000_000 / SR as u64;
        let mut total = 0u64;
        let mut previous_host = 0u64;

        for index in 0..500 {
            let frames = sizes[index % sizes.len()];
            let input = vec![0.0f32; frames];
            let mut output = vec![0.0f32; frames];
            core.process(&input, &mut output);
            let start = total;
            total += frames as u64;
            let anchor = block_end_anchor(
                ns(start),
                frames,
                SR,
                total,
                core.transport_frame(),
                core.snapshot().run == RunState::Running,
                &bridge,
            );
            assert!(anchor.host_ns >= previous_host);
            assert_eq!((anchor.frame, anchor.transport_frame), (total, total as i64));
            for offset in [0u64, frames as u64 / 2, frames as u64] {
                let mapped = anchor.transport_frame_at(ns(start + offset), SR);
                let expected = (start + offset) as i64;
                assert!((mapped - expected).abs() <= 1, "block {index}, {frames} frames: {mapped} vs {expected}");
            }
            previous_host = anchor.host_ns;
        }
    }

    #[test]
    fn calibration_job_plays_and_records_on_the_same_frames() {
        let signal = [0.25f32, -0.25, 0.5];
        let mut job = CalibrationJob::new(&signal, 5);
        let mut out = [0.0f32; 4];
        job.process(&mut out, &[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(out, [0.25, -0.25, 0.5, 0.0]);
        assert_eq!((job.played, job.recorded, job.is_done()), (3, 4, false));
        let mut out = [0.1f32; 4];
        job.process(&mut out, &[5.0, 6.0, 7.0, 8.0]);
        assert_eq!(out, [0.1; 4], "nothing more is played");
        assert_eq!(&*job.record, &[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(job.is_done());
    }

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
