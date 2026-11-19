//! PRODUCE arrangement: the control-side state the engine owns (document,
//! assets and their samples) and the realtime [`ArrangementPlayer`] mixed into
//! the output after the looper (the pattern of `preview.rs`).
//!
//! ```text
//!  control thread                                   output callback (ArrangementPlayer::process)
//!  ArrangementControl::play(schedule, from) ─ Box ─▶ plays regions from `from`
//!  ArrangementControl::update(schedule)     ─ Box ─▶ swaps the schedule, keeps the position
//!  ArrangementControl::stop()               ─────▶ short fade, then silent
//!  ArrangementControl::collect()            ◀─ Box ─ old schedules (and their Arc'd assets) dropped here
//!  ArrangementControl::position()           ◀─ atomics: frame, playing
//! ```
//!
//! - **No allocation, freeing, or locking on the audio thread.** A schedule
//!   (regions plus `Arc` handles on the asset samples) is built on the
//!   control thread and comes back to it to be dropped. The callback's scratch
//!   buffer is allocated when the player is made.
//! - Per-sample rules (trim, repeat, fades) are the ones in
//!   `voiceloop_project::arrangement`, shared with bounce, so a bounce is
//!   exactly what playback sounds like (before the output limiter).
//! - With a loop range set and playback started before its end, playback
//!   wraps from the range end to its start; otherwise it stops by itself at
//!   the end of the last region.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use voiceloop_project::arrangement::{audible_gain, region_envelope, region_source, scale_frames, RangeRenderer};
pub use voiceloop_project::arrangement::{ArrangementDoc, AudioAssetDoc};
use voiceloop_project::package::{AssetAudio, Samples};
use voiceloop_project::resample;

use crate::error::EngineError;
use crate::looper::io::MAX_BLOCK;
use crate::looper::soft_limit;
use crate::looper::transfer::CapturedTrack;

/// Fade applied when arrangement playback starts or stops (frames), so a
/// start in the middle of a region or a stop never clicks.
pub const ARRANGEMENT_DECLICK_FRAMES: u32 = 64;
/// Most regions in one playback schedule.
pub const MAX_SCHEDULED_REGIONS: usize = 4_096;

// ---- requests and replies ---------------------------------------------------------

/// `{ arrangement, assets }`: what `arrangement_get` and friends return.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementView {
    pub arrangement: ArrangementDoc,
    pub assets: Vec<AudioAssetDoc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRange {
    pub start_frame: u64,
    pub end_frame: u64,
}

/// `arrangement_send_to_looper` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SentToLooper {
    pub looper_track: u32,
}

/// `arrangement_bounce` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounced {
    pub frames: u64,
}

/// `arrangementPosition` event payload (~30 Hz while playing, once more on stop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrangementPosition {
    pub frame: u64,
    pub playing: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ArrangementRequest {
    Get,
    Set { arrangement: ArrangementDoc },
    CaptureFromLooper { tracks: Vec<u32>, per_layer: bool, repeats: u32, start_frame: u64 },
    SendToLooper { region_id: String, looper_track: Option<u32> },
    Play { from_frame: u64 },
    Stop,
    Bounce { path: String, range: Option<FrameRange> },
}

/// Reply to an [`ArrangementRequest`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ArrangementReply {
    View(ArrangementView),
    Sent(SentToLooper),
    Bounced(Bounced),
    Done,
}

// ---- control-side state -----------------------------------------------------------

/// The arrangement the engine owns: document, asset entries, asset samples.
#[derive(Debug, Clone, Default)]
pub struct ArrangementState {
    pub doc: ArrangementDoc,
    pub assets: Vec<AudioAssetDoc>,
    pub audio: AssetAudio,
    /// Song tempo for synced effect times (set by the engine; 0 = unsynced).
    pub bpm: f64,
}

impl ArrangementState {
    /// Take the arrangement out of an opened document (its samples in `audio`).
    pub fn from_doc(doc: &mut voiceloop_project::doc::ProjectDoc, audio: AssetAudio) -> Self {
        let mut arr = std::mem::take(&mut doc.arrangement);
        if arr.sample_rate == 0 {
            arr.sample_rate = doc.sample_rate;
        }
        Self { doc: arr, assets: std::mem::take(&mut doc.assets), audio, bpm: doc.transport.bpm }
    }

    /// An empty arrangement at `rate`.
    pub fn empty(rate: u32) -> Self {
        Self { doc: ArrangementDoc { sample_rate: rate, ..Default::default() }, ..Default::default() }
    }

    pub fn rate(&self) -> u32 {
        self.doc.sample_rate
    }

    pub fn view(&self) -> ArrangementView {
        ArrangementView { arrangement: self.doc.clone(), assets: self.assets.clone() }
    }

    /// Replace the document. Assets stay, used or not, so the UI's undo/redo
    /// (e.g. of a bring-in) can bring regions back that refer to them; they
    /// are saved with the project.
    pub fn set(&mut self, mut doc: ArrangementDoc) -> Result<(), EngineError> {
        if doc.sample_rate == 0 {
            doc.sample_rate = self.rate();
        }
        if doc.sample_rate != self.rate() {
            return Err(EngineError::Invalid(format!(
                "the arrangement is at {} Hz; frames must be sent at that rate (got {} Hz)",
                self.rate(),
                doc.sample_rate
            )));
        }
        doc.validate(&self.assets, self.rate()).map_err(|e| EngineError::Invalid(e.to_string()))?;
        self.doc = doc;
        Ok(())
    }

    /// Add a new asset for `samples` (at the arrangement's rate); returns its id.
    pub fn add_asset(&mut self, name: &str, samples: Vec<f32>) -> String {
        let asset = AudioAssetDoc::new(name, &samples, self.rate());
        let id = asset.id.clone();
        self.audio.insert(id.clone(), Samples::from(samples));
        self.assets.push(asset);
        id
    }

    /// Bring every frame value and asset to `rate` (no-op if already there).
    /// Resamples on the calling thread: rare (the device rate changed).
    pub fn ensure_rate(&mut self, rate: u32) {
        let from = self.rate();
        if from == rate || rate == 0 {
            return;
        }
        if from == 0 || self.assets.is_empty() {
            scale_frames(&mut self.doc, from.max(1), rate, &|_| None);
            self.doc.sample_rate = rate;
            return;
        }
        for a in &mut self.assets {
            if let Some(s) = self.audio.get_mut(&a.id) {
                let v = resample::resample(s, from, rate);
                *a = AudioAssetDoc { id: a.id.clone(), name: a.name.clone(), ..AudioAssetDoc::new(&a.name, &v, rate) };
                *s = Samples::from(v);
            }
        }
        let frames: std::collections::HashMap<String, u64> =
            self.assets.iter().map(|a| (a.id.clone(), a.frames)).collect();
        scale_frames(&mut self.doc, from, rate, &|id| frames.get(id).copied());
    }

    /// Mix of `[start, end)` as heard (effects starting at `start`), through the output limiter.
    pub fn render(&self, start: u64, end: u64) -> Vec<f32> {
        self.render_with(&mut self.renderer(), start, end)
    }

    /// Lane effects carried across consecutive [`Self::render_with`] calls (a bounce in chunks).
    pub fn renderer(&self) -> RangeRenderer {
        RangeRenderer::new(&self.doc, self.bpm)
    }

    pub fn render_with(&self, r: &mut RangeRenderer, start: u64, end: u64) -> Vec<f32> {
        let look = |id: &str| self.audio.get(id).map(|s| &s[..]);
        r.render(&self.doc, &look, start, end, soft_limit)
    }

    /// A playback schedule for the realtime player.
    pub fn schedule(&self) -> Result<Box<ArrangementSchedule>, EngineError> {
        ArrangementSchedule::build(&self.doc, &self.audio).map(Box::new)
    }
}

/// What one looper track sounds like over one of its loop cycles: its layers
/// at their offsets (`per_layer`: each layer on its own, else summed), at
/// unity gain. `frames` is the track's layer length (`Inventory::track_frames`).
/// Returns `(layer index or None for the mix, samples)`.
pub fn looper_track_audio(track: &CapturedTrack, frames: usize, per_layer: bool) -> Vec<(Option<u32>, Vec<f32>)> {
    let place = |layer: &[f32], offset: i64, out: &mut [f32]| {
        let n = frames.max(1) as i64;
        for (idx, o) in out.iter_mut().enumerate() {
            let source = (idx as i64 - offset).rem_euclid(n) as usize;
            *o += layer.get(source).copied().unwrap_or(0.0);
        }
    };
    let offset = |i: usize| track.plan.layer_offsets.get(i).copied().unwrap_or(0);
    if per_layer {
        track
            .layers
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let mut v = vec![0.0f32; frames];
                place(l, offset(i), &mut v);
                (Some(i as u32), v)
            })
            .collect()
    } else {
        let mut v = vec![0.0f32; frames];
        for (i, l) in track.layers.iter().enumerate() {
            place(l, offset(i), &mut v);
        }
        vec![(None, v)]
    }
}

// ---- realtime player --------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Scheduled {
    start: u64,
    len: u64,
    offset: u64,
    repeat: bool,
    fade_in: u64,
    fade_out: u64,
    /// Region gain × audible track gain.
    gain: f32,
    asset: usize,
}

/// Regions and their samples, prepared for the output callback.
#[derive(Debug)]
pub struct ArrangementSchedule {
    regions: Box<[Scheduled]>,
    assets: Box<[Samples]>,
    end: u64,
    loop_range: Option<(u64, u64)>,
}

impl ArrangementSchedule {
    /// Audible regions only (mute, solo and track gain folded in), sorted by start.
    pub fn build(doc: &ArrangementDoc, audio: &AssetAudio) -> Result<Self, EngineError> {
        let mut assets: Vec<Samples> = Vec::new();
        let mut ids: Vec<&str> = Vec::new();
        let mut regions = Vec::new();
        for t in &doc.tracks {
            let tg = audible_gain(doc, t);
            for r in t.regions.iter().filter(|r| !r.muted && tg > 0.0 && r.gain > 0.0) {
                let Some(samples) = audio.get(&r.asset_id) else {
                    return Err(EngineError::Invalid(format!("region {} has no audio loaded", r.id)));
                };
                let asset = match ids.iter().position(|&i| i == r.asset_id) {
                    Some(i) => i,
                    None => {
                        ids.push(&r.asset_id);
                        assets.push(samples.clone());
                        assets.len() - 1
                    }
                };
                regions.push(Scheduled {
                    start: r.start_frame,
                    len: r.length_frames,
                    offset: r.source_offset_frames,
                    repeat: r.repeat,
                    fade_in: r.fade_in_frames,
                    fade_out: r.fade_out_frames,
                    gain: r.gain * tg,
                    asset,
                });
            }
        }
        if regions.len() > MAX_SCHEDULED_REGIONS {
            return Err(EngineError::ResourceLimit(format!("at most {MAX_SCHEDULED_REGIONS} audible regions")));
        }
        regions.sort_by_key(|r| r.start);
        Ok(Self {
            end: doc.end_frame(),
            regions: regions.into(),
            assets: assets.into(),
            loop_range: doc.loop_range.map(|l| (l.start_frame, l.end_frame)),
        })
    }

    /// Add frames `[pos, pos + out.len())` of the schedule into `out`. Realtime-safe.
    fn mix_into(&self, pos: u64, out: &mut [f32]) {
        let to = pos + out.len() as u64;
        for r in self.regions.iter() {
            if r.start >= to {
                break; // sorted by start
            }
            let end = r.start + r.len;
            if end <= pos {
                continue;
            }
            let data = &self.assets[r.asset][..];
            let n = data.len() as u64;
            for f in r.start.max(pos)..end.min(to) {
                let k = f - r.start;
                if let Some(i) = region_source(r.offset, r.repeat, n, k) {
                    out[(f - pos) as usize] +=
                        data[i as usize] * r.gain * region_envelope(r.len, r.fade_in, r.fade_out, k);
                }
            }
        }
    }
}

enum Message {
    Play(Box<ArrangementSchedule>, u64),
    Update(Box<ArrangementSchedule>),
    Stop,
}

const RING: usize = 8;

#[derive(Debug, Default)]
struct Shared {
    frame: AtomicU64,
    playing: AtomicBool,
}

/// Control-thread side.
pub struct ArrangementControl {
    tx: Producer<Message>,
    back: Consumer<Box<ArrangementSchedule>>,
    shared: Arc<Shared>,
    /// Playing as last reported in a position event (to send the final `playing: false`).
    reported_playing: bool,
}

/// Output-callback side. Realtime-safe.
pub struct ArrangementPlayer {
    rx: Consumer<Message>,
    back: Producer<Box<ArrangementSchedule>>,
    schedule: Option<Box<ArrangementSchedule>>,
    pos: u64,
    playing: bool,
    stopping: bool,
    /// Position when Stop arrived (reported once the fade-out ends).
    stop_frame: Option<u64>,
    looping: bool,
    level: f32,
    step: f32,
    scratch: Box<[f32]>,
    shared: Arc<Shared>,
}

/// A connected control/player pair (one per output stream).
pub fn arrangement_pair() -> (ArrangementControl, ArrangementPlayer) {
    let (tx, rx) = RingBuffer::new(RING);
    let (back_tx, back) = RingBuffer::new(RING * 2 + 2);
    let shared = Arc::new(Shared::default());
    let control = ArrangementControl { tx, back, shared: shared.clone(), reported_playing: false };
    let player = ArrangementPlayer {
        rx,
        back: back_tx,
        schedule: None,
        pos: 0,
        playing: false,
        stopping: false,
        stop_frame: None,
        looping: false,
        level: 0.0,
        step: 1.0 / ARRANGEMENT_DECLICK_FRAMES.max(1) as f32,
        scratch: vec![0.0; MAX_BLOCK].into_boxed_slice(),
        shared,
    };
    (control, player)
}

fn full() -> EngineError {
    EngineError::Busy("the arrangement player queue is full".into())
}

impl ArrangementControl {
    /// Start playing `schedule` at `from` (replaces what plays).
    pub fn play(&mut self, schedule: Box<ArrangementSchedule>, from: u64) -> Result<(), EngineError> {
        self.collect();
        self.tx.push(Message::Play(schedule, from)).map_err(|_| full())?;
        self.shared.frame.store(from, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Swap in an edited schedule, keeping the position (no-op when stopped).
    pub fn update(&mut self, schedule: Box<ArrangementSchedule>) -> Result<(), EngineError> {
        self.collect();
        if !self.is_playing() {
            return Ok(());
        }
        self.tx.push(Message::Update(schedule)).map_err(|_| full())
    }

    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.collect();
        self.tx.push(Message::Stop).map_err(|_| full())
    }

    pub fn is_playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn position(&self) -> ArrangementPosition {
        ArrangementPosition {
            frame: self.shared.frame.load(Ordering::Relaxed),
            playing: self.shared.playing.load(Ordering::Relaxed),
        }
    }

    /// The position event to send now, if any: every call while playing, and
    /// once after playback ended.
    pub fn position_event(&mut self) -> Option<ArrangementPosition> {
        let p = self.position();
        let send = p.playing || self.reported_playing;
        self.reported_playing = p.playing;
        send.then_some(p)
    }

    /// Drop schedules the callback has finished with. Call regularly.
    pub fn collect(&mut self) {
        while let Ok(s) = self.back.pop() {
            drop(s);
        }
    }
}

impl ArrangementPlayer {
    fn retire(&mut self) {
        if let Some(s) = self.schedule.take() {
            if let Err(rtrb::PushError::Full(s)) = self.back.push(s) {
                // Never free on the audio thread: keep it until there is room.
                self.schedule = Some(s);
            }
        }
    }

    fn set_schedule(&mut self, s: Box<ArrangementSchedule>) {
        self.retire();
        if let Some(old) = self.schedule.take() {
            // The return ring is sized for every message, so this is unreachable;
            // if it ever happens, leak the old schedule rather than free it here.
            std::mem::forget(old);
        }
        self.looping = s.loop_range.is_some_and(|(_, e)| self.pos < e);
        self.schedule = Some(s);
    }

    fn drain(&mut self) {
        while let Ok(m) = self.rx.pop() {
            match m {
                Message::Play(s, from) => {
                    self.pos = from;
                    self.set_schedule(s);
                    self.playing = true;
                    self.stopping = false;
                    self.stop_frame = None;
                    self.level = 0.0;
                }
                Message::Update(s) => self.set_schedule(s),
                Message::Stop => {
                    if !self.stopping {
                        self.stop_frame = Some(self.pos);
                    }
                    self.stopping = true;
                }
            }
        }
    }

    /// Add the arrangement into `out` (mono, output rate). Realtime-safe.
    pub fn process(&mut self, out: &mut [f32]) {
        self.drain();
        for chunk in out.chunks_mut(self.scratch.len()) {
            if !self.playing {
                break;
            }
            self.process_chunk(chunk);
        }
        if !self.playing {
            self.retire();
        }
        self.shared.frame.store(self.pos, Ordering::Relaxed);
        self.shared.playing.store(self.playing, Ordering::Relaxed);
    }

    fn process_chunk(&mut self, out: &mut [f32]) {
        let Some(s) = self.schedule.take() else {
            self.playing = false;
            return;
        };
        if !self.looping && self.pos >= s.end {
            self.stopping = true; // past the last region: fade out (nothing sounds anyway)
        }
        let n = out.len();
        let scratch = &mut self.scratch[..n];
        scratch.fill(0.0);
        let mut i = 0;
        while i < n {
            let mut m = n - i;
            let wrap = match s.loop_range {
                Some((ls, le)) if self.looping && self.pos < le => {
                    m = m.min((le - self.pos) as usize);
                    Some((ls, le))
                }
                _ => None,
            };
            s.mix_into(self.pos, &mut scratch[i..i + m]);
            self.pos += m as u64;
            i += m;
            if let Some((ls, le)) = wrap {
                if self.pos >= le {
                    self.pos = ls;
                }
            }
        }
        let target = if self.stopping { 0.0 } else { 1.0 };
        for (o, &v) in out.iter_mut().zip(scratch.iter()) {
            self.level = crate::looper::io::slew(self.level, target, self.step);
            *o = soft_limit(*o + v * self.level);
        }
        if self.stopping && self.level == 0.0 {
            if let Some(at) = self.stop_frame.take() {
                self.pos = at; // the playhead stays where Stop was asked for
            }
            self.playing = false;
        }
        self.schedule = Some(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voiceloop_project::arrangement::{ArrangementTrackDoc, LoopRangeDoc, RegionDoc};

    const D: usize = ARRANGEMENT_DECLICK_FRAMES as usize;

    fn region(id: &str, asset: &str, start: u64, len: u64) -> RegionDoc {
        RegionDoc {
            id: id.into(),
            name: String::new(),
            asset_id: asset.into(),
            start_frame: start,
            source_offset_frames: 0,
            length_frames: len,
            repeat: false,
            gain: 1.0,
            fade_in_frames: 0,
            fade_out_frames: 0,
            muted: false,
            origin: None,
            extra: Default::default(),
        }
    }

    /// One lane, one asset of 1_000 frames counting up in small steps.
    fn state() -> (ArrangementState, String) {
        let mut st = ArrangementState::empty(48_000);
        let id = st.add_asset("ramp", (0..1_000).map(|i| i as f32 * 1e-4).collect());
        st.doc.tracks.push(ArrangementTrackDoc::new("Lane"));
        (st, id)
    }

    fn run(player: &mut ArrangementPlayer, frames: usize, block: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        for b in out.chunks_mut(block) {
            player.process(b);
        }
        out
    }

    #[test]
    fn regions_start_sample_accurately_with_trim_repeat_fades_and_gain() {
        let (mut st, id) = state();
        let mut r = region("r1", &id, 300, 2_500);
        r.source_offset_frames = 200;
        r.repeat = true;
        r.gain = 0.5;
        r.fade_in_frames = 100;
        r.fade_out_frames = 50;
        st.doc.tracks[0].regions.push(r);
        let mut r2 = region("r2", &id, 2_000, 100);
        r2.source_offset_frames = 900; // the last 100 frames of the asset, over r1
        st.doc.tracks[0].regions.push(r2);
        st.doc.tracks[0].gain = 1.5;
        st.set(st.doc.clone()).unwrap();
        let expected = st.render(0, 3_000);
        let (mut c, mut p) = arrangement_pair();
        c.play(st.schedule().unwrap(), 0).unwrap();
        // Irregular block sizes must not change the result.
        let mut out = Vec::new();
        for n in [7usize, 64, 333, 1, 512, 2_083] {
            out.extend(run(&mut p, n, n));
        }
        assert_eq!(out.len(), 3_000);
        assert!(out[..300].iter().all(|&v| v == 0.0), "nothing before the first region");
        assert_eq!(out[300], 0.0, "fade-in starts at zero");
        for f in D..3_000 {
            assert!((out[f] - expected[f]).abs() < 1e-7, "frame {f}: {} vs {}", out[f], expected[f]);
        }
        // Repeat: frame 300 + 800 + 5 plays source 200 + 5 again (span 800).
        let k = 805u64;
        let want = (200 + 5) as f32 * 1e-4 * 0.5 * 1.5;
        assert!((out[300 + k as usize] - want).abs() < 1e-7);
        assert!(!c.position().playing || c.position().frame >= 2_800);
    }

    #[test]
    fn starting_mid_region_fades_in_and_stop_fades_out() {
        let (mut st, id) = state();
        st.doc.tracks[0].regions.push(region("r1", &id, 0, 1_000));
        let (mut c, mut p) = arrangement_pair();
        c.play(st.schedule().unwrap(), 500).unwrap();
        let out = run(&mut p, 200, 64);
        assert!(
            out[0] < st.render(500, 501)[0] / 10.0
                && (out[D] - st.render(500 + D as u64, 501 + D as u64)[0]).abs() < 1e-7
        );
        assert_eq!(c.position_event(), Some(ArrangementPosition { frame: 700, playing: true }));
        c.stop().unwrap();
        let out = run(&mut p, 256, 64);
        assert!(out[D..].iter().all(|&v| v == 0.0), "silent after the declick fade");
        assert!(!c.position().playing);
        assert_eq!(
            c.position_event(),
            Some(ArrangementPosition { frame: 700, playing: false }),
            "where Stop was asked"
        );
        assert_eq!(c.position_event(), None, "only one final event");
        c.collect();
    }

    #[test]
    fn stops_at_the_end_or_loops_in_the_loop_range() {
        let (mut st, id) = state();
        st.doc.tracks[0].regions.push(region("r1", &id, 0, 1_000));
        let (mut c, mut p) = arrangement_pair();
        c.play(st.schedule().unwrap(), 900).unwrap();
        run(&mut p, 400, 128);
        assert!(!c.position().playing, "ended after the last region");
        st.doc.loop_range = Some(LoopRangeDoc { start_frame: 100, end_frame: 300 });
        c.play(st.schedule().unwrap(), 250).unwrap();
        let out = run(&mut p, 600, 128);
        assert!(c.position().playing);
        assert_eq!(c.position().frame, 100 + (250 + 600 - 300) % 200);
        // Output frames 50 and 250 are timeline frame 100 (wrapped once, then twice).
        assert!((out[250] - 100.0 * 1e-4).abs() < 1e-7);
        assert!((out[249] - 299.0 * 1e-4).abs() < 1e-7);
        // Muted lane: an update while playing silences it without moving the playhead.
        st.doc.tracks[0].muted = true;
        c.update(st.schedule().unwrap()).unwrap();
        let out = run(&mut p, 128, 128);
        assert!(out.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn solo_mute_and_missing_audio() {
        let (mut st, id) = state();
        st.doc.tracks[0].regions.push(region("r1", &id, 0, 10));
        let mut t2 = ArrangementTrackDoc::new("Two");
        t2.regions.push(region("r2", &id, 0, 10));
        t2.solo = true;
        st.doc.tracks.push(t2);
        assert_eq!(st.schedule().unwrap().regions.len(), 1, "solo excludes the other lane");
        st.audio.clear();
        assert!(st.schedule().is_err());
    }

    #[test]
    fn set_validates_and_keeps_unused_assets_for_undo() {
        let (mut st, id) = state();
        let mut doc = st.doc.clone();
        doc.tracks[0].regions.push(region("r1", &id, 0, 2_000));
        assert!(st.set(doc.clone()).is_err(), "past the asset end");
        doc.tracks[0].regions[0].length_frames = 1_000;
        st.set(doc.clone()).unwrap();
        assert_eq!(st.assets.len(), 1);
        doc.tracks[0].regions.clear();
        st.set(doc.clone()).unwrap();
        assert_eq!((st.assets.len(), st.audio.len()), (1, 1), "kept: an undo may bring the region back");
        doc.tracks[0].regions.push(region("r1", &id, 0, 1_000));
        st.set(doc).unwrap();
    }

    #[test]
    fn rate_conversion_resamples_assets_and_scales_frames() {
        let (mut st, id) = state();
        let mut r = region("r1", &id, 4_800, 1_000);
        r.repeat = true;
        st.doc.tracks[0].regions.push(r);
        st.ensure_rate(24_000);
        assert_eq!(st.rate(), 24_000);
        assert_eq!(st.assets[0].frames, 500);
        assert_eq!(st.assets[0].id, id, "ids are kept");
        assert_eq!(st.doc.tracks[0].regions[0].start_frame, 2_400);
        st.doc.validate(&st.assets, 24_000).unwrap();
    }

    #[test]
    fn request_json_matches_the_contract() {
        let r: ArrangementRequest = serde_json::from_value(serde_json::json!(
            {"type": "captureFromLooper", "tracks": [0, 2], "perLayer": true, "repeats": 4, "startFrame": 96000}))
        .unwrap();
        assert_eq!(
            r,
            ArrangementRequest::CaptureFromLooper {
                tracks: vec![0, 2],
                per_layer: true,
                repeats: 4,
                start_frame: 96_000
            }
        );
        let r: ArrangementRequest =
            serde_json::from_value(serde_json::json!({"type": "sendToLooper", "regionId": "x", "looperTrack": null}))
                .unwrap();
        assert_eq!(r, ArrangementRequest::SendToLooper { region_id: "x".into(), looper_track: None });
        let v = serde_json::to_value(ArrangementReply::Sent(SentToLooper { looper_track: 3 })).unwrap();
        assert_eq!(v, serde_json::json!({"looperTrack": 3}));
        let v = serde_json::to_value(ArrangementReply::View(ArrangementState::empty(48_000).view())).unwrap();
        assert_eq!(v["arrangement"]["sampleRate"], 48_000);
        assert!(v["assets"].as_array().unwrap().is_empty());
    }
}
