//! Deterministic looper tests: synthetic input, no audio hardware.
//!
//! Most tests run at 1 kHz so loop lengths are small round numbers, and with
//! `fade_frames = 0` so recorded values can be checked exactly.

use super::*;
use crate::model::LoopMode;
use crate::transport::{grid_frame, LoopFit, LoopLength, OverdubStart, TransportState};

use super::LooperCommand as C;

const BLOCK: usize = 64;

fn config(fade: u32) -> LooperConfig {
    LooperConfig {
        max_tracks: 2,
        track_capacity: MAX_TRACKS,
        max_loop_seconds: 10.0,
        pool_layers: 6,
        max_layers_per_track: 4,
        memory_limit_bytes: 64 << 20,
        fade_frames: fade,
        gain_smoothing_frames: 0,
        capture_tail_ms: 0,
    }
}

fn transport(sample_rate: u32, mode: LoopMode) -> TransportState {
    let mut t = TransportState { sample_rate, mode, ..Default::default() };
    t.loop_fit = LoopFit::Exact;
    // Most tests exercise the original timing; the overdub-start tests set it explicitly.
    t.overdub_start = OverdubStart::Immediate;
    t.metronome.enabled = false;
    t.metronome.count_in_bars = 0;
    t
}

/// 1 kHz, 120 BPM, 4/4, one-bar loop: 500 frames per beat, 2000 per loop.
fn one_bar_synced() -> TransportState {
    let mut t = transport(1_000, LoopMode::Synced);
    t.loop_length = LoopLength { bars: 1, beats: 0 };
    t
}

struct Rig {
    core: LooperCore,
    h: LooperHandles,
    /// Absolute output frames processed so far (index into `out`).
    frame: u64,
    out: Vec<f32>,
}

impl Rig {
    fn new(cfg: LooperConfig, t: TransportState) -> Self {
        let (core, h) = LooperCore::new(&cfg, t, t.sample_rate).expect("valid config");
        Self { core, h, frame: 0, out: Vec::new() }
    }

    /// Queue a command; it takes effect at the next processed frame.
    fn send(&mut self, c: LooperCommand) {
        self.h.commands.push(c).expect("command ring has space");
    }

    /// Process `frames` frames. `input` maps an absolute output frame to an input sample.
    fn run(&mut self, frames: usize, mut input: impl FnMut(u64) -> f32) {
        let mut inb = [0.0f32; BLOCK];
        let mut outb = [0.0f32; BLOCK];
        let mut left = frames;
        while left > 0 {
            let n = left.min(BLOCK);
            for (i, s) in inb[..n].iter_mut().enumerate() {
                *s = input(self.frame + i as u64);
            }
            self.core.process(&inb[..n], &mut outb[..n]);
            self.out.extend_from_slice(&outb[..n]);
            self.frame += n as u64;
            left -= n;
            // Drain like the control thread does.
            while self.h.snapshots.pop().is_ok() {}
        }
        self.h.recycler.service();
    }

    /// Process the same timeline through an irregular callback-size pattern.
    fn run_pattern(&mut self, frames: usize, pattern: &[usize], mut input: impl FnMut(u64) -> f32) {
        assert!(!pattern.is_empty() && pattern.iter().all(|&n| n > 0));
        let max = *pattern.iter().max().unwrap();
        let mut inb = vec![0.0f32; max];
        let mut outb = vec![0.0f32; max];
        let (mut left, mut block) = (frames, 0usize);
        while left > 0 {
            let n = left.min(pattern[block % pattern.len()]);
            for (i, s) in inb[..n].iter_mut().enumerate() {
                *s = input(self.frame + i as u64);
            }
            self.core.process(&inb[..n], &mut outb[..n]);
            self.out.extend_from_slice(&outb[..n]);
            self.frame += n as u64;
            left -= n;
            block += 1;
            while self.h.snapshots.pop().is_ok() {}
        }
        self.h.recycler.service();
    }

    fn silence(&mut self, frames: usize) {
        self.run(frames, |_| 0.0);
    }

    fn snap(&self) -> LooperSnapshot {
        self.core.snapshot()
    }

    fn notices(&mut self) -> Vec<LooperNotice> {
        let mut v = Vec::new();
        while let Ok(n) = self.h.notices.pop() {
            v.push(n);
        }
        v
    }
}

fn impulse(at: u64) -> impl FnMut(u64) -> f32 {
    move |f| if f == at { 1.0 } else { 0.0 }
}

fn nonzero_from(out: &[f32], from: usize) -> Vec<usize> {
    out.iter().enumerate().skip(from).filter(|(_, v)| v.abs() > 1e-6).map(|(i, _)| i).collect()
}

/// Executable reference model for the upcoming non-destructive loop editor.
///
/// The realtime API does not yet expose capture post-roll, per-layer trim, or
/// loop-duration commands. Keep these contracts independent of `LooperCore`
/// until those commands exist, then drive the engine through the same cases.
#[derive(Clone, Debug)]
struct EditableLayerContract {
    source: Vec<f32>,
    trim_start: usize,
    trim_end: usize,
}

impl EditableLayerContract {
    fn captured(source: Vec<f32>, default_loop_frames: usize) -> Self {
        assert!(default_loop_frames <= source.len());
        Self { source, trim_start: 0, trim_end: default_loop_frames }
    }

    fn sample_at(&self, absolute_frame: usize, loop_frames: usize) -> f32 {
        let phase = absolute_frame % loop_frames;
        if phase < self.trim_start || phase >= self.trim_end {
            return 0.0;
        }
        self.source.get(phase).copied().unwrap_or(0.0)
    }
}

fn rejected(reason: RejectReason) -> LooperNotice {
    LooperNotice::Rejected { reason }
}

// ---- configuration and memory ---------------------------------------------

#[test]
fn config_enforces_memory_and_count_limits() {
    let ok = LooperConfig::default();
    assert_eq!(ok.max_loop_frames(48_000).unwrap(), 30 * 48_000);
    assert_eq!(ok.memory_bytes(48_000), 30 * 48_000 * 8 * 4);

    let tiny_limit = LooperConfig { memory_limit_bytes: 1024, ..ok };
    assert!(matches!(tiny_limit.max_loop_frames(48_000), Err(EngineError::ResourceLimit(_))));
    let too_many_tracks = LooperConfig { max_tracks: MAX_TRACKS + 1, ..ok };
    assert!(matches!(too_many_tracks.max_loop_frames(48_000), Err(EngineError::Invalid(_))));
    let no_pool = LooperConfig { pool_layers: 0, ..ok };
    assert!(matches!(no_pool.max_loop_frames(48_000), Err(EngineError::Invalid(_))));
}

#[test]
fn pool_is_preallocated_and_reported() {
    let r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    let s = r.snap();
    assert_eq!(s.free_layers, 6);
    assert_eq!(s.track_count, 2);
    assert_eq!(r.h.recycler.layer_frames(), 10_000);
    assert_eq!(r.h.recycler.total_layers(), 6);
}

// ---- free loop mode ---------------------------------------------------------

#[test]
fn free_first_take_sets_exact_loop_length() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_234, |_| 0.5);
    r.send(C::StopRecording { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.loop_period, Some(1_234.0));
    assert_eq!(s.run, RunState::Running);
    assert_eq!(s.tracks[0].state, TrackState::Playing);
    assert_eq!(s.tracks[0].layers, 1);
}

#[test]
fn free_loop_repeats_at_exact_period() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, impulse(100));
    r.send(C::StopRecording { track: 0 });
    r.silence(10_000);
    let expected: Vec<usize> = (1..=10).map(|k| 100 + k * 1_000).collect();
    assert_eq!(nonzero_from(&r.out, 0), expected);
}

#[test]
fn repeated_separate_overdubs_never_change_or_accelerate_the_master_loop() {
    let cfg = LooperConfig { pool_layers: 6, max_layers_per_track: 6, ..config(0) };
    let mut r = Rig::new(cfg, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(997, impulse(100));
    r.send(C::StopRecording { track: 0 });
    r.silence(1);
    assert_eq!(r.snap().loop_period, Some(997.0));

    for expected_layers in 2..=6 {
        r.send(C::Overdub { track: 0 });
        r.silence(997);
        r.send(C::StopRecording { track: 0 });
        r.silence(1);
        let s = r.snap();
        assert_eq!(s.loop_period, Some(997.0), "layer {expected_layers} changed the period");
        assert_eq!(s.tracks[0].layers, expected_layers);
    }

    let playback_start = r.out.len();
    r.silence(997 * 32);
    let hits = nonzero_from(&r.out, playback_start);
    assert!(hits.len() >= 31, "expected a base-loop impulse on every cycle");
    assert!(hits.windows(2).all(|w| w[1] - w[0] == 997), "impulse spacing accelerated: {hits:?}");
}

#[test]
fn callback_partitioning_does_not_change_recording_or_playback_timing() {
    fn session(pattern: &[usize]) -> (LooperSnapshot, Vec<f32>) {
        let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
        r.send(C::Record { track: 0 });
        r.run_pattern(1_237, pattern, |frame| if frame % 113 == 7 { 0.25 } else { 0.0 });
        r.send(C::StopRecording { track: 0 });
        r.run_pattern(1, pattern, |_| 0.0);
        r.run_pattern(1_237 * 8, pattern, |_| 0.0);
        (r.snap(), r.out)
    }

    let fixed = session(&[64]);
    let irregular = session(&[1, 127, 3, 191, 17, 257, 5, 61]);
    assert_eq!(fixed.0.loop_period, irregular.0.loop_period);
    assert_eq!(fixed.0.tracks, irregular.0.tracks);
    assert_eq!(fixed.1, irregular.1, "callback partitioning changed rendered samples");
}

// ---- non-destructive loop editing contracts ------------------------------------

#[test]
fn contract_post_roll_is_retained_but_default_hidden() {
    let mut captured = vec![0.0; 1_100];
    captured[950] = 0.5;
    captured[1_050] = 1.0; // audio retained after the default stop boundary
    let layer = EditableLayerContract::captured(captured, 1_000);

    assert_eq!(layer.source.len(), 1_100, "post-roll must remain available for later trimming");
    assert_eq!((layer.trim_start, layer.trim_end), (0, 1_000));
    assert_eq!(layer.sample_at(950, 1_000), 0.5);
    assert_eq!(layer.sample_at(1_050, 2_000), 0.0, "post-roll is outside the default active segment");
    assert_eq!(layer.source[1_050], 1.0, "hiding post-roll must not erase captured audio");
}

#[test]
fn contract_each_layer_has_independent_non_destructive_trim_boundaries() {
    let mut source = vec![0.0; 1_000];
    source[100] = 0.25;
    source[900] = 0.75;
    let original = source.clone();
    let mut layer = EditableLayerContract::captured(source, 1_000);

    layer.trim_start = 80;
    layer.trim_end = 800;
    assert_eq!(layer.sample_at(100, 1_000), 0.25);
    assert_eq!(layer.sample_at(900, 1_000), 0.0);

    layer.trim_end = 950;
    assert_eq!(layer.sample_at(900, 1_000), 0.75, "expanding the trim reveals the original tail");
    assert_eq!(layer.source, original, "trim edits must never rewrite layer audio");
}

#[test]
fn contract_loop_duration_can_shrink_and_grow_without_accumulating_drift() {
    let mut source = vec![0.0; 1_100];
    source[100] = 1.0;
    let layer = EditableLayerContract::captured(source, 1_000);

    for period in [750, 1_250] {
        let hits: Vec<_> = (0..period * 32).filter(|&frame| layer.sample_at(frame, period) != 0.0).collect();
        assert_eq!(hits.len(), 32);
        assert!(
            hits.windows(2).all(|pair| pair[1] - pair[0] == period),
            "resized {period}-frame loop drifted: {hits:?}"
        );
    }
    assert_eq!(layer.source.len(), 1_100, "loop-duration edits must preserve the captured source");
}

#[test]
fn free_first_take_fits_to_beats_and_retains_hidden_capture_tail() {
    for (fit, expected) in [
        (LoopFit::Exact, 760usize),
        (LoopFit::NearestBeat, 1_000),
        (LoopFit::RoundDown, 500),
        (LoopFit::RoundUp, 1_000),
    ] {
        let mut t = transport(1_000, LoopMode::Free); // 500 frames per beat
        t.loop_fit = fit;
        let mut cfg = config(0);
        cfg.capture_tail_ms = 100;
        let mut r = Rig::new(cfg, t);
        r.send(C::Record { track: 0 });
        r.silence(760);
        r.send(C::StopRecording { track: 0 });
        // The chosen cycle is published immediately; capture continues only
        // into the hidden source cushion.
        r.run(1, |_| 0.0);
        assert_eq!(r.snap().loop_period, Some(expected as f64), "{fit:?}");

        let tail_frame = if expected == 1_000 { 1_020usize } else { 820usize };
        r.run(400, |frame| if frame == tail_frame as u64 { 0.75 } else { 0.0 });
        assert_eq!(r.core.layer_sample(0, 0, tail_frame), Some(0.75), "{fit:?}: tail not retained");
        assert!(nonzero_from(&r.out, 0).is_empty(), "{fit:?}: hidden tail leaked into playback");
    }
}

#[test]
fn stopped_synced_loop_can_shrink_and_grow_without_rewriting_layers() {
    let mut r = Rig::new(config(0), one_bar_synced());
    r.send(C::Record { track: 0 });
    r.run(2_100, impulse(100));
    r.send(C::Stop);
    r.silence(1);
    assert_eq!(r.snap().loop_period, Some(2_000.0));
    let original = r.core.layer_sample(0, 0, 100);

    for beats in [3, 5] {
        let mut t = r.snap().transport;
        t.loop_length = LoopLength { bars: 0, beats };
        r.send(C::SetTransport { transport: t });
        r.silence(1);
        assert_eq!(r.snap().loop_period, Some(beats as f64 * 500.0));
        assert_eq!(r.core.layer_sample(0, 0, 100), original, "resize rewrote source audio");
        assert!(r.notices().is_empty());
    }
}

#[test]
fn recording_is_not_monitored_live() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.5);
    assert!(r.out.iter().all(|&v| v == 0.0), "input must not reach the output while recording");
}

#[test]
fn too_short_first_take_is_discarded() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(5, |_| 0.5); // min loop is 20 frames at 1 kHz
    r.send(C::StopRecording { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::TooShort)));
    let s = r.snap();
    assert_eq!(s.loop_period, None);
    assert_eq!(s.tracks[0].state, TrackState::Empty);
    assert_eq!(s.free_layers, 6);
}

#[test]
fn second_track_synchronises_to_first_in_free_mode() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, impulse(0));
    r.send(C::StopRecording { track: 0 });
    r.silence(350); // abs 1000..1350
    r.send(C::Record { track: 1 }); // one full loop starting mid-cycle: t 1350..2350
    assert_eq!(r.core.transport_frame(), 1_350);
    r.run(3_000, impulse(1_600)); // phase 600 of the shared loop
    assert_eq!(nonzero_from(&r.out, 0), vec![1_000, 2_000, 2_600, 3_000, 3_600, 4_000]);
    assert_eq!(r.snap().tracks[1].state, TrackState::Playing);
}

// ---- metronome mode ----------------------------------------------------------

#[test]
fn metronome_record_starts_and_ends_on_loop_boundaries() {
    let mut r = Rig::new(config(0), one_bar_synced());
    assert_eq!(r.snap().loop_period, Some(2_000.0));
    r.send(C::Play);
    r.silence(300);
    r.send(C::Record { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.tracks[0].state, TrackState::Armed);
    assert_eq!((s.tracks[0].record_start, s.tracks[0].record_end), (Some(2_000), Some(4_000)));

    r.run(5_699, |_| 0.5); // to abs 6000; input is 0.5 everywhere
    assert!(r.out[..4_000].iter().all(|&v| v == 0.0));
    // Exactly one pass was captured (not the armed period, not more).
    assert!(r.out[4_000..6_000].iter().all(|&v| v == 0.5));
    assert_eq!(r.snap().tracks[0].state, TrackState::Playing);
}

#[test]
fn count_in_delays_transport_and_recording() {
    let mut t = one_bar_synced();
    t.metronome.count_in_bars = 1;
    let mut r = Rig::new(config(0), t);
    r.send(C::Record { track: 0 }); // from Stopped: count-in, then record bar 1
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.run, RunState::CountIn);
    assert_eq!(s.count_in_remaining, 1_999);
    assert_eq!(s.tracks[0].state, TrackState::Armed);

    r.silence(1_999); // count-in ends at abs 2000
    assert_eq!(r.snap().run, RunState::Running);
    assert_eq!(r.core.transport_frame(), 0);

    r.run(2_000, impulse(2_010)); // musical frame 10 of the recorded loop
    r.silence(2_000);
    // Count-in clicks have long decayed; the only later output is the recorded impulse.
    assert_eq!(nonzero_from(&r.out, 3_000), vec![4_010]);
}

#[test]
fn fractional_period_has_no_accumulated_drift() {
    let mut t = one_bar_synced();
    t.bpm = 113.0; // 2123.893... frames per bar at 1 kHz
    let period = t.loop_period();
    assert!(period.fract() > 0.0);
    let mut r = Rig::new(config(0), t);
    r.send(C::Play);
    r.send(C::Record { track: 0 }); // t = 0 is a loop boundary: records cycle 0
    let total = (300.0 * period) as usize + 10;
    r.run(total, impulse(0));

    let expected: Vec<usize> =
        (1u64..).map(|k| (k as f64 * period).round() as usize).take_while(|&f| f < r.out.len()).collect();
    assert!(expected.len() >= 299);
    assert_eq!(nonzero_from(&r.out, 0), expected);
    // Loop cycles coincide exactly with the bar grid used by the metronome.
    let bar = t.frames_per_bar();
    for (k, &f) in expected.iter().enumerate() {
        assert_eq!(grid_frame(k as i64 + 1, bar) as usize, f);
    }
}

#[test]
fn overdub_waits_for_the_next_loop_start_when_set() {
    let mut t = transport(1_000, LoopMode::Free);
    t.overdub_start = OverdubStart::LoopStart;
    let mut r = Rig::new(config(0), t);
    r.send(C::Record { track: 0 });
    r.silence(997);
    r.send(C::StopRecording { track: 0 });
    r.silence(300); // mid-loop
    r.send(C::Overdub { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.tracks[0].state, TrackState::Armed);
    let start = s.tracks[0].record_start.expect("armed overdub has a start");
    assert_eq!(start % 997, 0, "overdub must start on a loop boundary, got {start}");
    assert!(start > 997, "must be the next boundary, not one already passed");
    r.silence((start as usize) - 997 - 300);
    assert_eq!(r.snap().tracks[0].state, TrackState::Overdubbing);
}

#[test]
fn overdub_next_bar_in_free_mode_and_cancel_while_armed() {
    let mut t = transport(1_000, LoopMode::Free); // 120 BPM 4/4: a bar is 2000 frames
    t.overdub_start = OverdubStart::NextBar;
    let mut r = Rig::new(config(0), t);
    r.send(C::Record { track: 0 });
    r.silence(4_000);
    r.send(C::StopRecording { track: 0 });
    r.silence(500);
    r.send(C::Overdub { track: 0 });
    r.silence(1);
    let start = r.snap().tracks[0].record_start.unwrap();
    assert_eq!(start % 2_000, 0);
    // Stopping while armed cancels without adding a layer.
    r.send(C::StopRecording { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.tracks[0].layers, 1);
    assert_eq!(s.tracks[0].record_start, None);
}

#[test]
fn metronome_overdub_quantises_to_bars() {
    let mut r = Rig::new(config(0), one_bar_synced()); // bars every 2000 frames
    r.send(C::Play);
    r.send(C::Record { track: 0 });
    r.silence(2_000);
    r.silence(700);
    r.send(C::Overdub { track: 0 });
    r.silence(1);
    assert_eq!(r.snap().tracks[0].record_start, Some(4_000));
    r.silence(2_000);
    r.send(C::StopRecording { track: 0 }); // at t 4701 -> ends at the next bar, 6000
    r.silence(1);
    assert_eq!(r.snap().tracks[0].record_end, Some(6_000));
    assert_eq!(r.snap().tracks[0].state, TrackState::Overdubbing);
    r.silence(1_400);
    assert_eq!(r.snap().tracks[0].state, TrackState::Playing);
    assert_eq!(r.snap().tracks[0].layers, 2);
}

#[test]
fn stop_recording_while_armed_cancels() {
    let mut r = Rig::new(config(0), one_bar_synced());
    r.send(C::Play);
    r.silence(300);
    r.send(C::Record { track: 0 });
    r.silence(10);
    r.send(C::StopRecording { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.tracks[0].state, TrackState::Empty);
    assert_eq!(s.free_layers, 6);
}

#[test]
fn click_is_on_the_grid_and_not_recorded() {
    let mut t = one_bar_synced();
    t.metronome.enabled = true;
    let mut r = Rig::new(config(0), t);
    r.send(C::Play);
    r.send(C::Record { track: 0 });
    r.silence(4_000);
    for beat in 0..8usize {
        let f = beat * 500;
        assert!(r.out[f].abs() > 0.1, "click expected at frame {f}");
        if f > 0 {
            assert_eq!(r.out[f - 1], 0.0, "no click energy just before beat at {f}");
        }
    }
    assert!(r.out[0] > r.out[500], "downbeat is accented");
    assert!(r.out[2_000] > r.out[2_500]);

    // Disable the click (allowed while loops exist); the recorded loop is silent.
    let mut quiet = r.snap().transport;
    quiet.metronome.enabled = false;
    r.send(C::SetTransport { transport: quiet });
    r.silence(2_100); // let the last click decay, then a full loop
    assert!(r.out[4_100..].iter().all(|&v| v == 0.0));
    assert!(r.notices().is_empty());
}

#[test]
fn click_is_not_written_into_a_multi_pass_overdub() {
    let mut t = one_bar_synced();
    t.metronome.enabled = true;
    let mut r = Rig::new(config(0), t);
    r.send(C::Play);
    r.send(C::Record { track: 0 });
    r.silence(2_100); // silent base loop; click is audible only in output
    r.send(C::Overdub { track: 0 });
    r.silence(2_000 * 4); // four passes into one silent overdub layer
    r.send(C::StopRecording { track: 0 });
    r.silence(2_100);
    assert_eq!(r.snap().tracks[0].layers, 2);

    let mut quiet = r.snap().transport;
    quiet.metronome.enabled = false;
    r.send(C::SetTransport { transport: quiet });
    let start = r.out.len();
    r.silence(2_000 * 3);
    assert!(r.out[start..].iter().all(|&v| v == 0.0), "software click leaked into an overdub layer");
}

// ---- overdub, latency, undo ---------------------------------------------------

#[test]
fn overdub_aligns_with_latency_compensation() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, impulse(200));
    r.send(C::StopRecording { track: 0 });
    r.send(C::SetInputLatency { frames: 37 });
    r.silence(500); // abs 1000..1500
    r.send(C::Overdub { track: 0 });
    // The player hears phase 500 at abs 2500; their response arrives 37 frames later.
    r.run(1_500, impulse(2_537));
    r.send(C::StopRecording { track: 0 });
    r.silence(2_000);
    assert_eq!(nonzero_from(&r.out, 3_000), vec![3_200, 3_500, 4_200, 4_500]);
}

#[test]
fn multi_pass_overdub_accumulates_into_one_layer() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.1);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.run(2_000, |_| 0.1); // two passes
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000);
    let s = r.snap();
    assert_eq!(s.tracks[0].layers, 2);
    let v = r.out[r.out.len() - 500];
    assert!((v - 0.3).abs() < 1e-6, "base 0.1 + two overdub passes 0.2, got {v}");
}

#[test]
fn undo_removes_only_latest_overdub_and_recycles_clean_buffers() {
    // Pool of 2 forces the final overdub to reuse the buffer released by undo.
    let mut r = Rig::new(LooperConfig { pool_layers: 2, ..config(0) }, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.25);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.run(1_000, |_| 0.25);
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000);
    assert_eq!(r.out[2_500], 0.5);
    assert_eq!(r.snap().free_layers, 0);

    r.send(C::Undo { track: 0 });
    r.silence(1_000);
    assert_eq!(r.out[3_500], 0.25, "base take is untouched");
    let s = r.snap();
    assert_eq!(s.tracks[0].layers, 1);
    assert_eq!(s.free_layers, 0, "the undone layer is held for redo");

    r.send(C::Undo { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::NothingToUndo)));

    // With the pool empty, a new take first reclaims the redo history. The
    // buffer is zeroed off the audio thread, so this attempt is refused...
    r.send(C::Overdub { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::PoolExhausted)));
    let s = r.snap();
    assert_eq!((s.tracks[0].redo_layers, s.free_layers), (0, 1), "redo history reclaimed and recycled");

    // ...and the next one succeeds. A recycled buffer must be zeroed: a
    // silent overdub adds nothing.
    r.send(C::Overdub { track: 0 });
    r.silence(1_000);
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000);
    let n = r.out.len();
    assert!(r.out[n - 1_000..].iter().all(|&v| v == 0.25));
}

#[test]
fn redo_restores_an_undone_overdub_until_a_new_take() {
    let mut r = Rig::new(LooperConfig { pool_layers: 3, ..config(0) }, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.25);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.run(1_000, |_| 0.25);
    r.send(C::StopRecording { track: 0 });
    r.silence(10);
    assert_eq!(*r.out.last().unwrap(), 0.5);

    r.send(C::Undo { track: 0 });
    r.silence(10);
    assert_eq!(*r.out.last().unwrap(), 0.25);
    let s = r.snap();
    assert_eq!((s.tracks[0].layers, s.tracks[0].redo_layers), (1, 1));
    assert_eq!(s.free_layers, 1, "an undone layer stays reserved for redo");

    r.send(C::Redo { track: 0 });
    r.silence(10);
    assert_eq!(*r.out.last().unwrap(), 0.5, "redo restores the identical audio");
    assert_eq!(r.snap().tracks[0].redo_layers, 0);
    r.send(C::Redo { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::NothingToRedo)));

    // A new take discards redo history and returns its buffer to the pool.
    r.send(C::Undo { track: 0 });
    r.silence(10);
    r.send(C::Overdub { track: 0 });
    r.silence(10);
    let s = r.snap();
    assert_eq!((s.tracks[0].layers, s.tracks[0].redo_layers), (2, 0));
    assert_eq!(s.free_layers, 1);
    r.send(C::Redo { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::TrackBusy)));
}

#[test]
fn first_take_is_latency_compensated_including_its_tail() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::SetInputLatency { frames: 37 });
    r.send(C::Record { track: 0 });
    // Performed at 100 and 973 (inside the take); both arrive 37 frames late,
    // the second one after Stop has been pressed at 1000.
    let input = |f: u64| if f == 137 || f == 1_010 { 1.0 } else { 0.0 };
    r.run(1_000, input);
    r.send(C::StopRecording { track: 0 });
    r.run(1, input);
    assert_eq!(r.snap().loop_period, Some(1_000.0), "length is press to press");
    r.run(2_999, input);
    assert_eq!(nonzero_from(&r.out, 0), vec![1_100, 1_973, 2_100, 2_973, 3_100, 3_973]);
    assert_eq!(r.snap().tracks[0].state, TrackState::Playing);
}

// ---- mixing ---------------------------------------------------------------------

#[test]
fn mute_volume_and_track_play_stop() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.5);
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000);
    assert_eq!(r.out[1_500], 0.5);

    r.send(C::SetGain { track: 0, gain: 0.5 });
    r.silence(1_000);
    assert_eq!(r.out[2_500], 0.25);

    r.send(C::SetMute { track: 0, muted: true });
    r.silence(1_000);
    assert!(r.out[3_000..4_000].iter().all(|&v| v == 0.0));

    r.send(C::SetMute { track: 0, muted: false });
    r.send(C::SetTrackPlaying { track: 0, playing: false });
    r.silence(1_000);
    assert!(r.out[4_000..5_000].iter().all(|&v| v == 0.0));
    assert_eq!(r.snap().tracks[0].state, TrackState::Stopped);

    r.send(C::SetTrackPlaying { track: 0, playing: true });
    r.silence(10);
    assert_eq!(r.out[5_005], 0.25);

    r.send(C::SetGain { track: 0, gain: 99.0 });
    r.send(C::SetGain { track: 1, gain: f32::NAN });
    r.silence(1);
    assert_eq!(r.snap().tracks[0].gain, 2.0);
    assert_eq!(r.snap().tracks[1].gain, 1.0);
}

#[test]
fn two_tracks_mix_and_mute_independently() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.25);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Record { track: 1 });
    r.run(1_000, |_| 0.5);
    r.silence(1_000);
    assert_eq!(r.out[2_500], 0.75);
    r.send(C::SetMute { track: 0, muted: true });
    r.silence(10);
    assert_eq!(*r.out.last().unwrap(), 0.5);
}

#[test]
fn solo_silences_every_other_track() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.25);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Record { track: 1 });
    r.run(1_000, |_| 0.5);
    r.silence(100);
    fn last(r: &mut Rig, c: LooperCommand) -> f32 {
        r.send(c);
        r.silence(10);
        *r.out.last().unwrap()
    }
    assert_eq!(last(&mut r, C::SetSolo { track: 0, solo: true }), 0.25);
    assert!(r.snap().tracks[0].solo);
    assert_eq!(last(&mut r, C::SetSolo { track: 1, solo: true }), 0.75);
    assert_eq!(last(&mut r, C::SetSolo { track: 0, solo: false }), 0.5);
    assert_eq!(last(&mut r, C::SetMute { track: 1, muted: true }), 0.0, "mute wins over solo");
    assert_eq!(last(&mut r, C::SetSolo { track: 1, solo: false }), 0.25, "no solo left: unmuted tracks play");
}

// ---- limits and cleanup ----------------------------------------------------------

#[test]
fn pool_exhaustion_and_layer_limit_reject_without_allocating() {
    let mut r = Rig::new(LooperConfig { pool_layers: 2, ..config(0) }, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(100, |_| 0.1);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.silence(50);
    r.send(C::StopRecording { track: 0 });
    r.silence(10);
    r.send(C::Overdub { track: 0 });
    r.send(C::Record { track: 1 });
    r.silence(1);
    let notices = r.notices();
    assert_eq!(notices.iter().filter(|n| **n == rejected(RejectReason::PoolExhausted)).count(), 2);
    assert_eq!(r.snap().tracks[0].layers, 2);
    assert_eq!(r.snap().tracks[1].state, TrackState::Empty);

    let mut r = Rig::new(LooperConfig { max_layers_per_track: 2, ..config(0) }, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(100, |_| 0.1);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.silence(50);
    r.send(C::StopRecording { track: 0 });
    r.silence(10);
    r.send(C::Overdub { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::LayerLimit)));
}

#[test]
fn first_take_stops_at_max_loop_length() {
    let mut r = Rig::new(LooperConfig { max_loop_seconds: 1.0, ..config(0) }, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_500, |_| 0.1);
    let s = r.snap();
    assert_eq!(s.loop_period, Some(1_000.0));
    assert_eq!(s.tracks[0].state, TrackState::Playing);
    assert!(r.notices().contains(&LooperNotice::MaxLengthReached { track: 0 }));
}

#[test]
fn metronome_loop_longer_than_buffers_is_rejected() {
    let mut r = Rig::new(LooperConfig { max_loop_seconds: 1.0, ..config(0) }, transport(1_000, LoopMode::Free));
    let mut t = one_bar_synced(); // 2000 frames > 1000-frame buffers
    t.sample_rate = 1_000;
    r.send(C::SetTransport { transport: t });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::LoopTooLong)));
    assert_eq!(r.snap().transport.mode, LoopMode::Free);
}

#[test]
fn clear_releases_buffers_and_resets_free_loop() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.1);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.silence(500);
    r.send(C::ClearAll);
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.loop_period, None);
    assert_eq!(s.free_layers, 6);
    assert!(s.tracks[..2].iter().all(|t| t.state == TrackState::Empty));

    // The next first take defines a new length.
    r.send(C::Record { track: 1 });
    r.run(500, |_| 0.1);
    r.send(C::StopRecording { track: 1 });
    r.silence(1);
    assert_eq!(r.snap().loop_period, Some(500.0));
}

// ---- transport ------------------------------------------------------------------

#[test]
fn timing_changes_are_locked_while_loops_exist() {
    let mut r = Rig::new(config(0), one_bar_synced());
    r.send(C::Play);
    r.send(C::Record { track: 0 });
    r.silence(2_100);

    let mut faster = r.snap().transport;
    faster.bpm = 100.0;
    r.send(C::SetTransport { transport: faster });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::TransportLocked)));
    assert_eq!(r.snap().transport.bpm, 120.0);

    let mut louder = r.snap().transport;
    louder.metronome.gain_db = -12.0;
    r.send(C::SetTransport { transport: louder });
    r.silence(1);
    assert_eq!(r.snap().transport.metronome.gain_db, -12.0);
    assert!(r.notices().is_empty());
}

#[test]
fn pause_holds_position_and_stop_rewinds() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, impulse(100));
    r.send(C::StopRecording { track: 0 });
    r.silence(50); // t = 1050
    r.send(C::Pause);
    r.silence(500);
    assert_eq!(r.core.transport_frame(), 1_050);
    assert_eq!(r.snap().run, RunState::Paused);
    r.send(C::Resume);
    r.silence(100);
    // Phase 100 of cycle 1 (t = 1100) is heard 50 frames after resuming.
    assert_eq!(nonzero_from(&r.out, 1_001), vec![1_600]);

    r.send(C::Stop);
    r.silence(10);
    assert_eq!(r.snap().run, RunState::Stopped);
    assert_eq!(r.core.transport_frame(), 0);
    let before = r.out.len();
    r.send(C::Play);
    r.silence(200);
    assert_eq!(nonzero_from(&r.out, before), vec![before + 100], "playback restarts at the loop start");
}

#[test]
fn restart_jumps_to_the_loop_start_and_keeps_running() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, impulse(100));
    r.send(C::StopRecording { track: 0 });
    r.silence(350);
    let before = r.out.len();
    r.send(C::Restart);
    r.silence(200);
    assert_eq!(r.snap().run, RunState::Running);
    assert_eq!(r.core.transport_frame(), 200);
    assert_eq!(nonzero_from(&r.out, before), vec![before + 100]);
}

#[test]
fn musical_position_tracks_the_clock() {
    let mut t = one_bar_synced();
    t.metronome.subdivision = 2;
    let mut r = Rig::new(config(0), t);
    r.send(C::Play);
    r.silence(2_000 + 500 + 300); // bar 2, beat 2, second half
    let p = r.snap().position;
    assert_eq!((p.bar, p.beat, p.subdivision), (2, 2, 2));
}

// ---- fades ------------------------------------------------------------------------

#[test]
fn recording_edges_are_faded_without_moving_boundaries() {
    let mut r = Rig::new(config(10), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.5);
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000);
    assert_eq!(r.snap().loop_period, Some(1_000.0), "fades do not change the loop length");
    let cycle = &r.out[1_000..2_000];
    assert!((cycle[0] - 0.05).abs() < 1e-6);
    assert!((cycle[9] - 0.5).abs() < 1e-6);
    assert!((cycle[500] - 0.5).abs() < 1e-6);
    assert!((cycle[990] - 0.5).abs() < 1e-6);
    assert!((cycle[999] - 0.05).abs() < 1e-6);
}

#[test]
fn transport_stop_fades_a_recording_it_cuts_short() {
    let mut r = Rig::new(config(10), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.silence(1_000); // silent base take: only the overdub is audible
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.run(300, |_| 0.5); // overdub phases 0..300, then the transport stops mid-take
    r.send(C::Stop);
    r.silence(10);
    let before = r.out.len();
    r.send(C::Play);
    r.silence(400);
    let cycle = &r.out[before..];
    assert!((cycle[150] - 0.5).abs() < 1e-6);
    assert!((cycle[290] - 0.5).abs() < 1e-6, "ten frames before the cut are untouched");
    assert!((cycle[295] - 0.25).abs() < 1e-6);
    assert!((cycle[299] - 0.05).abs() < 1e-6, "last recorded frame is faded");
    assert_eq!(cycle[300], 0.0);
}

// ---- gain smoothing and limiting --------------------------------------------------

#[test]
fn mute_ramps_instead_of_stepping() {
    let cfg = LooperConfig { gain_smoothing_frames: 100, ..config(0) };
    let mut r = Rig::new(cfg, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.5);
    r.send(C::StopRecording { track: 0 });
    r.silence(100);
    let b = r.out.len();
    r.send(C::SetMute { track: 0, muted: true });
    r.silence(200);
    for k in 0..100 {
        let expected = 0.5 * (1.0 - (k + 1) as f32 / 100.0);
        assert!((r.out[b + k] - expected).abs() < 1e-4, "frame {k}: {} vs {expected}", r.out[b + k]);
    }
    assert!(r.out[b + 100..].iter().all(|&v| v.abs() < 1e-5));
    // The largest step is one slew increment, never the full signal.
    let max_step = r.out[b - 1..b + 101].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
    assert!(max_step < 0.006, "max step {max_step}");
}

#[test]
fn soft_limiter_is_transparent_below_the_knee_and_bounded_above() {
    for &v in &[0.0f32, 0.1, -0.5, LIMIT_KNEE, -LIMIT_KNEE] {
        assert_eq!(soft_limit(v), v);
    }
    let mut prev = soft_limit(LIMIT_KNEE);
    for i in 1..=400 {
        let x = LIMIT_KNEE + i as f32 * 0.01;
        let y = soft_limit(x);
        assert!(y >= prev && y <= 1.0, "monotonic and within full scale at {x}: {y}");
        assert_eq!(soft_limit(-x), -y, "odd symmetry");
        prev = y;
    }
    assert!(soft_limit(1.0) < 0.96, "full-scale input is pulled below full scale");
    // Continuous slope at the knee: no kink that would add harsh harmonics.
    let slope = (soft_limit(LIMIT_KNEE + 1e-3) - LIMIT_KNEE) / 1e-3;
    assert!((slope - 1.0).abs() < 0.01, "slope at knee {slope}");
    assert_eq!(soft_limit(f32::NAN), 0.0);
    assert_eq!(soft_limit(f32::INFINITY), 0.0);
}

#[test]
fn stacked_overdubs_saturate_instead_of_clipping() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.5);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.run(2_000, |_| 0.5); // two more passes: the layer sum is 1.5
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000);
    let v = *r.out.last().unwrap();
    assert!(v > LIMIT_KNEE && v < 1.0, "limited to {v}");
}

// ---- state for the UI ---------------------------------------------------------------

#[test]
fn looper_state_trims_tracks_and_serialises() {
    let r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    let state = LooperState::from_snapshot(&r.snap(), &IoCounters::default(), 1234);
    assert_eq!(state.tracks.len(), 2);
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["run"], "stopped");
    assert_eq!(json["tracks"][0]["state"], "empty");
    assert_eq!(json["memoryBytes"], 1234);
    let notice = serde_json::to_value(rejected(RejectReason::PoolExhausted)).unwrap();
    assert_eq!(notice, serde_json::json!({"kind": "rejected", "reason": "poolExhausted"}));
}

// ---- stop / restart / pause with open recordings (RT audit #2, #3) -----------------

/// Free mode, latency 100: a silent 1000-frame base loop, then an overdub
/// that starts at t = 1200 (phase 200). Returns the rig at t = 1800.
fn overdub_in_progress(input: impl FnMut(u64) -> f32 + Copy) -> Rig {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::SetInputLatency { frames: 100 });
    r.send(C::Record { track: 0 });
    r.silence(1_000);
    r.send(C::StopRecording { track: 0 }); // tail stays open until t = 1100
    r.silence(200);
    r.send(C::Overdub { track: 0 });
    r.run(600, input);
    r
}

/// Performed at 1500 (phase 500) and at 1750 (phase 750, in the last
/// `latency` frames before the stop at 1800), and at 1810 (after the stop).
fn late_hits(f: u64) -> f32 {
    if f == 1_600 || f == 1_850 || f == 1_910 {
        1.0
    } else {
        0.0
    }
}

#[test]
fn stop_collects_the_in_flight_tail_of_an_overdub() {
    let mut r = overdub_in_progress(late_hits);
    r.send(C::Stop);
    r.run(50, late_hits);
    let s = r.snap();
    assert_eq!(s.run, RunState::Stopped, "playback stops at once");
    assert_eq!(s.tracks[0].state, TrackState::Overdubbing, "while the tail is still arriving");
    r.run(250, late_hits);
    let s = r.snap();
    assert_eq!(s.tracks[0].state, TrackState::Stopped);
    assert_eq!(r.core.transport_frame(), 0, "rewound once the tail was in");
    assert!(r.out[1_800..].iter().all(|&v| v == 0.0), "nothing plays while draining");

    let b = r.out.len();
    r.send(C::Play);
    r.silence(1_000);
    assert_eq!(nonzero_from(&r.out, b), vec![b + 500, b + 750], "the hit performed after Stop is not recorded");
}

#[test]
fn restart_collects_the_tail_then_runs_from_the_loop_start() {
    let mut r = overdub_in_progress(late_hits);
    r.send(C::Restart);
    r.run(1_100, late_hits); // 100 frames of drain, then t = 0..1000
    assert_eq!(r.snap().run, RunState::Running);
    assert_eq!(r.core.transport_frame(), 1_000);
    assert_eq!(nonzero_from(&r.out, 1_800), vec![1_900 + 500, 1_900 + 750]);
}

#[test]
fn stop_closes_a_first_take_like_stop_recording() {
    // Same performance as `first_take_is_latency_compensated_including_its_tail`,
    // ended by the transport Stop instead of StopRecording.
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::SetInputLatency { frames: 37 });
    r.send(C::Record { track: 0 });
    let input = |f: u64| if f == 137 || f == 1_010 { 1.0 } else { 0.0 };
    r.run(1_000, input);
    r.send(C::Stop);
    r.run(100, input);
    assert_eq!(r.snap().loop_period, Some(1_000.0), "length is press to press");
    let b = r.out.len();
    r.send(C::Play);
    r.silence(2_000);
    assert_eq!(nonzero_from(&r.out, b), vec![b + 100, b + 973, b + 1_100, b + 1_973]);
}

#[test]
fn stop_fade_is_the_same_with_input_latency() {
    // `transport_stop_fades_a_recording_it_cuts_short` with 5 frames of latency:
    // half of the fade-out is applied to frames that arrive after Stop.
    let mut r = Rig::new(config(10), transport(1_000, LoopMode::Free));
    r.send(C::SetInputLatency { frames: 5 });
    r.send(C::Record { track: 0 });
    r.silence(1_000);
    r.send(C::StopRecording { track: 0 });
    r.silence(1_000); // t = 2000: phase 0
    r.send(C::Overdub { track: 0 });
    r.run(300, |_| 0.5);
    r.send(C::Stop);
    r.run(10, |_| 0.5);
    let before = r.out.len();
    r.send(C::Play);
    r.silence(400);
    let cycle = &r.out[before..];
    assert!((cycle[150] - 0.5).abs() < 1e-6);
    assert!((cycle[290] - 0.5).abs() < 1e-6, "ten frames before the cut are untouched");
    assert!((cycle[295] - 0.25).abs() < 1e-6, "{}", cycle[295]);
    assert!((cycle[299] - 0.05).abs() < 1e-6, "last recorded frame is faded");
    assert_eq!(cycle[300], 0.0);
}

#[test]
fn commands_wait_while_a_stop_drains() {
    let mut r = overdub_in_progress(late_hits);
    r.send(C::Stop);
    r.send(C::Play); // arrives during the drain: applied after it, not dropped
    r.run(1_100, late_hits);
    assert_eq!(r.snap().run, RunState::Running);
    assert!(r.notices().is_empty());
}

#[test]
fn pause_is_refused_while_a_track_records() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.silence(100);
    r.send(C::Pause);
    r.silence(10);
    assert_eq!(r.snap().run, RunState::Running);
    assert_eq!(r.notices(), vec![rejected(RejectReason::TrackBusy)]);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.silence(10);
    r.send(C::Pause);
    r.silence(10);
    assert_eq!(r.notices(), vec![rejected(RejectReason::TrackBusy)], "overdubs too");
    r.send(C::StopRecording { track: 0 });
    r.silence(10);
    r.send(C::Pause);
    r.silence(10);
    assert_eq!(r.snap().run, RunState::Paused);
    assert!(r.notices().is_empty());
}

// ---- realtime bounds (RT audit #6, #8) ---------------------------------------------

#[test]
fn config_caps_realtime_work() {
    let ok = config(0);
    assert!(ok.max_loop_frames(1_000).is_ok());
    for bad in [
        LooperConfig { pool_layers: MAX_POOL_LAYERS + 1, max_loop_seconds: 0.1, ..ok },
        LooperConfig { max_layers_per_track: MAX_LAYERS_PER_TRACK + 1, ..ok },
        LooperConfig { fade_frames: MAX_FADE_FRAMES + 1, max_loop_seconds: 100.0, ..ok },
        LooperConfig { fade_frames: 2_501, ..ok }, // more than a quarter of 10_000 frames
    ] {
        assert!(matches!(bad.max_loop_frames(1_000), Err(EngineError::Invalid(_))), "{bad:?}");
    }
    assert!(LooperConfig { pool_layers: MAX_POOL_LAYERS, max_loop_seconds: 0.1, ..ok }.max_loop_frames(1_000).is_ok());
}

#[test]
fn clear_all_releases_a_full_pool_in_one_block() {
    let mut r = Rig::new(LooperConfig { max_layers_per_track: 3, ..config(0) }, transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(1_000, |_| 0.1);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Record { track: 1 });
    r.silence(1_000);
    for t in 0..2 {
        for _ in 0..2 {
            r.send(C::Overdub { track: t as u8 });
            r.silence(100);
            r.send(C::StopRecording { track: t as u8 });
            r.silence(10);
        }
    }
    r.silence(10);
    let s = r.snap();
    assert_eq!((s.tracks[0].layers, s.tracks[1].layers, s.free_layers), (3, 3, 0), "the whole pool is in use");
    r.send(C::ClearAll);
    r.silence(1); // one callback releases every buffer; the control thread zeroes them
    assert_eq!(r.snap().free_layers, 6);
    assert!(r.notices().is_empty());
}

#[test]
fn transport_stops_before_its_clock_could_overflow() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Play);
    r.silence(1);
    r.core.set_transport_frame(MAX_TRANSPORT_FRAME - 10);
    r.silence(64);
    assert_eq!(r.snap().run, RunState::Stopped);
    assert_eq!(r.core.transport_frame(), 0);
}

// ---- layer export / import (persistence) -------------------------------------------

use super::transfer::{Captured, Exporter, Importer, LayerSlot, Poll, RestorePlan, TransferError};

fn export(r: &mut Rig) -> Result<Captured, TransferError> {
    let mut e = Exporter::new();
    for _ in 0..10_000 {
        match e.poll(&mut r.h) {
            Poll::Pending => r.silence(64),
            Poll::Ready(x) => return x,
        }
    }
    panic!("export did not finish");
}

fn plan_of(c: &Captured) -> (RestorePlan, Vec<(u8, LayerSlot, Vec<f32>)>) {
    let mut tracks = c.inventory.tracks;
    let mut layers = Vec::new();
    for (t, tr) in c.tracks.iter().enumerate() {
        tracks[t] = tr.plan;
        for (i, l) in tr.layers.iter().enumerate() {
            layers.push((t as u8, LayerSlot::Active(i as u8), l.clone()));
        }
        for (i, l) in tr.redo.iter().enumerate() {
            layers.push((t as u8, LayerSlot::Redo(i as u8), l.clone()));
        }
    }
    let plan = RestorePlan {
        transport: c.inventory.transport,
        period: c.inventory.period,
        latency: c.inventory.latency,
        track_count: c.inventory.track_count,
        tracks,
    };
    (plan, layers)
}

fn import(r: &mut Rig, c: &Captured) -> Result<(), TransferError> {
    let (plan, layers) = plan_of(c);
    let mut im = Importer::new(plan, layers);
    for _ in 0..10_000 {
        match im.poll(&mut r.h) {
            Poll::Pending => r.silence(64),
            Poll::Ready(x) => return x,
        }
    }
    panic!("import did not finish");
}

/// Deterministic noise in -0.5..0.5.
fn noise(seed: u64) -> impl FnMut(u64) -> f32 + Copy {
    move |f| {
        let x = (f ^ seed).wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((x >> 40) as f32 / (1u64 << 24) as f32) - 0.5
    }
}

/// Render `frames` from a stopped looper, starting with Play.
fn render_from_start(r: &mut Rig, frames: usize) -> Vec<f32> {
    let b = r.out.len();
    r.send(C::Play);
    r.silence(frames);
    let out = r.out[b..].to_vec();
    r.send(C::Stop);
    r.silence(1);
    out
}

fn session_with_layers(t: TransportState) -> Rig {
    let mut r = Rig::new(LooperConfig { pool_layers: 8, ..config(16) }, t);
    r.send(C::SetInputLatency { frames: 7 });
    r.send(C::Record { track: 0 });
    r.run(2_300, noise(1));
    r.send(C::StopRecording { track: 0 });
    r.run(3_000, noise(2)); // free: closes the take; synced: the take runs to its boundary
    for seed in [3, 4] {
        r.send(C::Overdub { track: 0 });
        r.run(1_700, noise(seed));
        r.send(C::StopRecording { track: 0 });
        r.run(2_500, noise(9));
    }
    r.send(C::Undo { track: 0 }); // keep one layer in the redo history
    r.send(C::Record { track: 1 });
    r.run(4_500, noise(5));
    r.send(C::SetGain { track: 1, gain: 0.7 });
    r.send(C::SetGain { track: 0, gain: 1.3 });
    r.send(C::Stop);
    r.silence(100);
    let s = r.snap();
    assert_eq!((s.tracks[0].layers, s.tracks[0].redo_layers, s.tracks[1].layers), (2, 1, 1), "{s:?}");
    r
}

fn assert_round_trip(t: TransportState) {
    let mut a = session_with_layers(t);
    let captured = export(&mut a).expect("export");
    assert!(captured.tracks[0].layers[0].iter().any(|&v| v != 0.0));
    let original = render_from_start(&mut a, 5_000);
    assert!(original.iter().any(|&v| v != 0.0));

    // Clear the same looper and load the captured state back.
    a.send(C::ClearAll);
    a.silence(64);
    import(&mut a, &captured).expect("import");
    assert!(render_from_start(&mut a, 5_000) == original, "bit-identical after clear + import");

    // A fresh looper (the app restarted) plays the same, and redo history survived.
    let mut wrong_rate = Rig::new(LooperConfig { pool_layers: 8, ..config(16) }, transport(2_000, LoopMode::Free));
    assert!(matches!(import(&mut wrong_rate, &captured), Err(TransferError::DoesNotFit(_))), "resample first");
    let mut b = Rig::new(LooperConfig { pool_layers: 8, ..config(16) }, transport(1_000, LoopMode::Free));
    import(&mut b, &captured).expect("import into a new looper");
    assert!(render_from_start(&mut b, 5_000) == original, "bit-identical in a fresh looper");
    let again = export(&mut b).unwrap();
    assert_eq!(again.tracks, captured.tracks, "export of the import is identical");
    for r in [&mut a, &mut b] {
        r.send(C::Redo { track: 0 });
        r.silence(1);
    }
    assert!(render_from_start(&mut a, 3_000) == render_from_start(&mut b, 3_000), "redo layer restored");
}

#[test]
fn export_clear_import_is_bit_identical_in_free_mode() {
    assert_round_trip(transport(1_000, LoopMode::Free));
}

#[test]
fn export_clear_import_is_bit_identical_with_a_fractional_metronome_loop() {
    let mut t = transport(1_000, LoopMode::Synced);
    t.bpm = 113.0; // 530.97... frames per beat: the loop period is fractional
    t.loop_length = LoopLength { bars: 1, beats: 0 };
    assert_round_trip(t);
}

#[test]
fn export_is_refused_while_recording_and_import_while_running() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(500, |_| 0.2);
    assert!(matches!(export(&mut r), Err(TransferError::Busy(_))));
    r.send(C::StopRecording { track: 0 });
    r.silence(100);
    let captured = export(&mut r).expect("export while playing is fine");
    let period = r.snap().loop_period.unwrap();
    assert_eq!(captured.inventory.content_frames(), period.ceil() as usize);
    assert!(matches!(import(&mut r, &captured), Err(TransferError::Busy(_))), "transport is running");
}

#[test]
fn import_rejects_content_that_does_not_fit_and_leaves_the_looper_usable() {
    let mut big = Rig::new(LooperConfig { max_loop_seconds: 5.0, ..config(0) }, transport(1_000, LoopMode::Free));
    big.send(C::Record { track: 0 });
    big.run(3_000, |_| 0.2);
    big.send(C::Stop);
    big.silence(10);
    let captured = export(&mut big).unwrap();
    let mut small = Rig::new(LooperConfig { max_loop_seconds: 2.0, ..config(0) }, transport(1_000, LoopMode::Free));
    assert!(matches!(import(&mut small, &captured), Err(TransferError::DoesNotFit(_))));
    small.send(C::Record { track: 0 });
    small.silence(10);
    assert_eq!(small.snap().tracks[0].state, TrackState::Recording);
}

// ---- stream restarts: park and restore (iOS route changes, device changes) ---------

/// Tear the looper down the way a stream stop does and collect what it parked.
fn park(r: Rig) -> super::transfer::ParkedLooper {
    let Rig { core, mut h, .. } = r;
    let rx = h.parked.take().expect("parking channel");
    drop(core); // the stream (and the core inside it) is dropped
    rx.try_recv().expect("the core parked its content")
}

#[test]
fn loops_survive_a_stream_restart_bit_identically() {
    for t in [transport(1_000, LoopMode::Free), {
        let mut t = transport(1_000, LoopMode::Synced);
        t.bpm = 113.0;
        t.loop_length = LoopLength { bars: 1, beats: 0 };
        t
    }] {
        let mut a = session_with_layers(t);
        let original = render_from_start(&mut a, 5_000);
        let parked = park(a);
        assert!(parked.has_content());
        let captured = parked.into_captured();
        // The new stream: a fresh looper at the same rate.
        let mut b = Rig::new(LooperConfig { pool_layers: 8, ..config(16) }, transport(1_000, LoopMode::Free));
        import(&mut b, &captured).expect("restore into the new stream");
        assert!(render_from_start(&mut b, 5_000) == original, "identical playback after the restart");
        let s = b.snap();
        assert_eq!((s.tracks[0].layers, s.tracks[0].redo_layers, s.tracks[1].layers), (2, 1, 1));
        assert_eq!((s.tracks[1].gain, s.tracks[0].gain), (0.7, 1.3));
    }
}

#[test]
fn a_take_open_at_teardown_is_closed_and_kept() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.run(800, impulse(300)); // first take still open when the stream dies
    let parked = park(r);
    assert_eq!(parked.period, Some(800.0), "closed at the teardown point like Stop");
    let captured = parked.into_captured();
    let mut b = Rig::new(config(0), transport(1_000, LoopMode::Free));
    import(&mut b, &captured).unwrap();
    let out = render_from_start(&mut b, 1_600);
    assert_eq!(nonzero_from(&out, 0), vec![300, 1_100]);

    // An overdub open at teardown keeps what was written.
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.silence(1_000);
    r.send(C::StopRecording { track: 0 });
    r.send(C::Overdub { track: 0 });
    r.run(500, impulse(1_200));
    let captured = park(r).into_captured();
    assert_eq!(captured.tracks[0].layers.len(), 2);
    assert_eq!(captured.tracks[0].layers[1][200], 1.0);
}

#[test]
fn an_empty_or_loading_looper_parks_nothing_useful() {
    let r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    assert!(!park(r).has_content());
    // Mid-restore content is incomplete: nothing is parked (the source is kept by the caller).
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    let mut tracks = [super::transfer::TrackPlan::EMPTY; MAX_TRACKS];
    tracks[0].layers = 1;
    let plan = RestorePlan {
        transport: transport(1_000, LoopMode::Free),
        period: Some(100.0),
        latency: 0,
        track_count: 2,
        tracks,
    };
    r.h.jobs
        .push(super::transfer::LayerJob { ticket: 0, kind: super::transfer::JobKind::Restore(plan, None) })
        .unwrap();
    r.silence(1);
    let Rig { core, mut h, .. } = r;
    let rx = h.parked.take().unwrap();
    drop(core);
    assert!(rx.try_recv().is_err());
}

#[test]
fn snapshots_carry_a_content_epoch() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    let e0 = r.snap().epoch;
    r.send(C::SetGain { track: 0, gain: 0.5 });
    r.silence(10);
    assert_eq!(r.snap().epoch, e0, "mixer changes are not content changes");
    r.send(C::Record { track: 0 });
    r.silence(100);
    assert_ne!(r.snap().epoch, e0);
}

// ---- input discontinuities (hardware-io bridge hook) ------------------------------

#[test]
fn an_input_discontinuity_flags_only_takes_in_progress() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    r.send(C::Record { track: 0 });
    r.silence(1_000);
    r.send(C::StopRecording { track: 0 });
    r.silence(10);
    r.core.apply_now(C::InputDiscontinuity); // nothing is recording: no flag
    assert!(!r.snap().tracks[0].timing_suspect);

    r.send(C::Overdub { track: 0 });
    r.silence(100);
    r.core.apply_now(C::InputDiscontinuity);
    r.send(C::StopRecording { track: 0 });
    r.silence(100);
    let s = r.snap();
    assert!(s.tracks[0].timing_suspect && !s.tracks[1].timing_suspect);
    let json = serde_json::to_value(s.tracks[0]).unwrap();
    assert_eq!(json["timingSuspect"], true);

    r.send(C::Overdub { track: 0 }); // the next take starts clean
    r.silence(10);
    assert!(!r.snap().tracks[0].timing_suspect);
    assert!(serde_json::from_str::<LooperCommand>(r#"{"type":"inputDiscontinuity"}"#).is_err(), "internal only");
}

// ---- adding and removing tracks at runtime -----------------------------------------

#[test]
fn tracks_are_added_and_removed_without_a_restart() {
    let mut r = Rig::new(config(0), transport(1_000, LoopMode::Free));
    assert_eq!((r.snap().track_count, r.snap().track_capacity), (2, MAX_TRACKS as u8));
    for _ in 2..MAX_TRACKS {
        r.send(C::AddTrack);
    }
    r.silence(1);
    assert_eq!(r.snap().track_count, MAX_TRACKS as u8);
    r.send(C::AddTrack);
    r.silence(1);
    assert_eq!(r.notices(), vec![rejected(RejectReason::TrackLimit)]);

    // The last track records and plays like any other.
    let last = (MAX_TRACKS - 1) as u8;
    r.send(C::Record { track: last });
    r.run(1_000, impulse(250));
    r.send(C::StopRecording { track: last });
    r.send(C::Record { track: 1 });
    r.run(1_000, impulse(1_500));
    r.silence(1_000);
    assert_eq!(nonzero_from(&r.out, 2_000), vec![2_250, 2_500]);

    // Removing track 0 shifts later tracks down; removing one with content frees its layers.
    let free = r.snap().free_layers;
    r.send(C::RemoveTrack { track: 0 });
    r.silence(1);
    let s = r.snap();
    assert_eq!(s.track_count, MAX_TRACKS as u8 - 1);
    assert_eq!(s.tracks[0].layers, 1, "old track 1 is now track 0");
    r.send(C::RemoveTrack { track: last - 1 });
    r.silence(1);
    assert_eq!(r.snap().free_layers, free + 1, "its layer went back to the pool");
    let b = r.out.len();
    r.silence(1_000);
    assert_eq!(nonzero_from(&r.out, b).len(), 1, "only the remaining loop plays");

    // A recording track cannot be removed; the last track cannot be removed.
    r.send(C::Overdub { track: 0 });
    r.send(C::RemoveTrack { track: 0 });
    r.silence(1);
    assert_eq!(r.notices(), vec![rejected(RejectReason::TrackBusy)]);
    r.send(C::StopRecording { track: 0 });
    r.silence(10);
    while r.snap().track_count > 1 {
        r.send(C::RemoveTrack { track: 0 });
        r.silence(1);
    }
    r.send(C::RemoveTrack { track: 0 });
    r.silence(1);
    assert!(r.notices().contains(&rejected(RejectReason::TrackLimit)));
    let s = r.snap();
    assert_eq!(s.free_layers + s.tracks[0].layers as u32, 6, "every removed track's buffers are back");

    let state = LooperState::from_snapshot(&r.snap(), &IoCounters::default(), 0);
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(json["maxTracks"], MAX_TRACKS as u64, "the frontend's name for the ceiling");
    let cmd: LooperCommand = serde_json::from_str(r#"{"type":"removeTrack","track":3}"#).unwrap();
    assert_eq!(cmd, C::RemoveTrack { track: 3 });
    assert_eq!(serde_json::from_str::<LooperCommand>(r#"{"type":"addTrack"}"#).unwrap(), C::AddTrack);
}

#[test]
fn restore_recreates_the_track_count_including_empty_tracks() {
    let mut a = Rig::new(config(0), transport(1_000, LoopMode::Free));
    for _ in 0..4 {
        a.send(C::AddTrack);
    }
    a.send(C::Record { track: 4 });
    a.run(1_000, impulse(10));
    a.send(C::Stop);
    a.silence(10);
    let captured = export(&mut a).unwrap();
    assert_eq!(captured.inventory.track_count, 6);
    let mut b = Rig::new(config(0), transport(1_000, LoopMode::Free)); // starts with 2
    import(&mut b, &captured).unwrap();
    let s = b.snap();
    assert_eq!((s.track_count, s.tracks[4].layers), (6, 1));
    // More tracks than this looper can hold does not fit.
    let mut small = Rig::new(LooperConfig { track_capacity: 4, ..config(0) }, transport(1_000, LoopMode::Free));
    assert!(matches!(import(&mut small, &captured), Err(TransferError::DoesNotFit(_))));
}

#[test]
fn track_capacity_is_validated() {
    let ok = config(0);
    assert!(LooperConfig { track_capacity: MAX_TRACKS + 1, ..ok }.max_loop_frames(1_000).is_err());
    assert!(LooperConfig { track_capacity: 4, max_tracks: 5, ..ok }.max_loop_frames(1_000).is_err());
    assert!(LooperConfig { track_capacity: 4, max_tracks: 4, ..ok }.max_loop_frames(1_000).is_ok());
}
