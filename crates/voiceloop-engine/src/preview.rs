//! Preview instrument: plays reference melodies and scales on the engine's
//! output device, so a preview follows the output picker and route like the
//! loops and the click do.
//!
//! The preview is mixed into the output only. It is never written into
//! recordings: the looper records from the input, before this mix. (Speakers
//! can still bleed into the microphone acoustically; headphones avoid it.)
//!
//! ```text
//!  control thread                               output callback (PreviewPlayer::process)
//!  PreviewControl::play(notes) ─ Box<Schedule> ─▶ starts voices at each note's frame
//!  PreviewControl::stop()      ─ Stop ─────────▶ releases every voice (short fade)
//!  PreviewControl::set_bank()  ─ Arc<Bank> ────▶ swaps the sample bank, cuts voices
//!  PreviewControl::collect()   ◀─ finished ───── schedules and old banks, dropped here
//! ```
//!
//! - **No allocation, freeing, or locking on the audio thread.** Schedules
//!   and banks are built on the control thread and returned to it to drop.
//! - **Fixed voices:** [`MAX_PREVIEW_VOICES`]; when all are busy the quietest
//!   one is taken over.
//! - **Samples:** a [`SampleBank`] of mono recordings, one per root note
//!   (the bundled set is the Salamander Grand Piano, CC-BY 3.0, every minor
//!   third). A note plays the nearest root, resampled by linear interpolation.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::looper::soft_limit;

/// Voices sounding at once (a reference melody overlaps only by release tails).
pub const MAX_PREVIEW_VOICES: usize = 16;
/// Notes in one preview (a long reference is a few hundred at most).
pub const MAX_PREVIEW_NOTES: usize = 4_096;
/// Key-release fade time constant, seconds (damper on the string).
pub const PREVIEW_RELEASE_SECONDS: f32 = 0.06;
/// Fade time constant when a preview is stopped, seconds.
pub const PREVIEW_STOP_SECONDS: f32 = 0.02;
/// Attack ramp at a note's start, seconds (keeps a retriggered voice click-free).
pub const PREVIEW_ATTACK_SECONDS: f32 = 0.002;
/// A voice below this envelope level is silent and freed.
const SILENT: f32 = 1.0e-4;

/// One recorded note of the instrument.
#[derive(Debug, Clone)]
pub struct SampleZone {
    /// MIDI note the recording sounds at.
    pub root: f32,
    pub sample_rate: u32,
    pub data: Box<[f32]>,
}

/// The instrument's recordings, sorted by root note.
#[derive(Debug, Clone, Default)]
pub struct SampleBank {
    zones: Vec<SampleZone>,
}

impl SampleBank {
    pub fn new(mut zones: Vec<SampleZone>) -> Self {
        zones.retain(|z| z.data.len() > 1 && z.sample_rate > 0);
        zones.sort_by(|a, b| a.root.total_cmp(&b.root));
        Self { zones }
    }

    /// Load every `NNN.wav` in `dir`, where NNN is the MIDI note it sounds at.
    pub fn load_dir(dir: &Path) -> Result<Self, EngineError> {
        let entries = std::fs::read_dir(dir)
            .map_err(|e| EngineError::Invalid(format!("preview samples: cannot read {}: {e}", dir.display())))?;
        let mut zones = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(root) = path
                .file_stem()
                .and_then(|s| s.to_str())
                .filter(|_| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")))
                .and_then(|s| s.parse::<u8>().ok())
            else {
                continue;
            };
            let (info, data) = voiceloop_project::wav::read_wav(&path)
                .map_err(|e| EngineError::Invalid(format!("preview samples: {}: {e}", path.display())))?;
            let mono = voiceloop_project::wav::downmix_mono(&data, info.spec.channels as usize);
            zones.push(SampleZone { root: root as f32, sample_rate: info.spec.sample_rate, data: mono.into() });
        }
        let bank = Self::new(zones);
        if bank.zones.is_empty() {
            return Err(EngineError::Invalid(format!("preview samples: no NNN.wav files in {}", dir.display())));
        }
        Ok(bank)
    }

    pub fn len(&self) -> usize {
        self.zones.len()
    }

    pub fn is_empty(&self) -> bool {
        self.zones.is_empty()
    }

    /// Index of the zone whose root is nearest `midi`.
    fn nearest(&self, midi: f32) -> Option<usize> {
        let i = self.zones.partition_point(|z| z.root < midi);
        match (i.checked_sub(1), (i < self.zones.len()).then_some(i)) {
            (Some(lo), Some(hi)) => {
                Some(if midi - self.zones[lo].root <= self.zones[hi].root - midi { lo } else { hi })
            }
            (Some(lo), None) => Some(lo),
            (None, hi) => hi,
        }
    }
}

/// One note to play, as sent by the UI.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewNote {
    /// MIDI note (fractional values are detuned).
    pub midi: f32,
    /// Seconds from the start of the preview.
    pub start: f64,
    /// Seconds the key is held.
    pub duration: f64,
    /// 0..1; omitted = 0.8.
    #[serde(default)]
    pub velocity: Option<f32>,
}

/// A preview request: notes plus playback options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub notes: Vec<PreviewNote>,
    /// Linear output gain, 0..2.
    pub gain: f32,
    /// Pitch of A4 in Hz (default 440).
    #[serde(default)]
    pub tuning_a4: Option<f32>,
}

/// Reply to a preview request.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewStarted {
    /// Length of the preview including the last note's release, seconds.
    pub seconds: f64,
}

#[derive(Debug, Clone, Copy)]
struct Scheduled {
    start: u64,
    end: u64,
    zone: usize,
    step: f64,
    amp: f32,
}

/// A preview prepared for the output callback (frames at the output rate).
#[derive(Debug)]
pub struct PreviewSchedule {
    notes: Box<[Scheduled]>,
    gain: f32,
}

impl PreviewSchedule {
    /// Convert `request` to output frames against `bank`. Notes are sorted by start.
    pub fn build(request: &PreviewRequest, bank: &SampleBank, output_rate: u32) -> Result<Self, EngineError> {
        if request.notes.len() > MAX_PREVIEW_NOTES {
            return Err(EngineError::Invalid(format!("a preview has at most {MAX_PREVIEW_NOTES} notes")));
        }
        if !(0.0..=2.0).contains(&request.gain) {
            return Err(EngineError::Invalid("preview gain must be between 0 and 2".into()));
        }
        let a4 = request.tuning_a4.unwrap_or(440.0);
        if !(380.0..=480.0).contains(&a4) {
            return Err(EngineError::Invalid("preview tuning must be between 380 and 480 Hz".into()));
        }
        let rate = output_rate.max(1) as f64;
        let detune = (a4 as f64 / 440.0).log2() * 12.0;
        let mut notes = Vec::with_capacity(request.notes.len());
        for n in &request.notes {
            if !(n.midi.is_finite() && n.start.is_finite() && n.duration.is_finite()) || n.start < 0.0 {
                return Err(EngineError::Invalid("preview notes need finite, non-negative times".into()));
            }
            let Some(zone) = bank.nearest(n.midi) else { continue };
            let z = &bank.zones[zone];
            let semis = n.midi as f64 + detune - z.root as f64;
            let start = (n.start * rate).round() as u64;
            notes.push(Scheduled {
                start,
                end: start + (n.duration.max(0.0) * rate).round() as u64,
                zone,
                step: 2f64.powf(semis / 12.0) * z.sample_rate as f64 / rate,
                amp: n.velocity.unwrap_or(0.8).clamp(0.0, 1.0),
            });
        }
        notes.sort_by_key(|n| n.start);
        Ok(Self { notes: notes.into(), gain: request.gain })
    }

    /// Seconds until the last note has been released and faded.
    pub fn seconds(&self, output_rate: u32) -> f64 {
        let end = self.notes.iter().map(|n| n.end).max().unwrap_or(0);
        end as f64 / output_rate.max(1) as f64 + 6.0 * PREVIEW_RELEASE_SECONDS as f64
    }
}

enum Message {
    Play(Box<PreviewSchedule>),
    Stop,
    SetBank(Arc<SampleBank>),
}

/// Things the callback hands back to be dropped on the control thread.
enum Returned {
    Schedule(Box<PreviewSchedule>),
    Bank(Arc<SampleBank>),
}

const RING: usize = 8;

#[derive(Debug, Clone, Copy, Default)]
struct Voice {
    active: bool,
    zone: usize,
    pos: f64,
    step: f64,
    amp: f32,
    env: f32,
    /// Frame (on the schedule clock) the key is released.
    end: u64,
    released: bool,
    /// Per-sample envelope multiplier once released.
    decay: f32,
}

/// Control-thread side.
pub struct PreviewControl {
    tx: Producer<Message>,
    back: Consumer<Returned>,
    bank: Option<Arc<SampleBank>>,
    playing: Arc<AtomicBool>,
    output_rate: u32,
}

/// Output-callback side. Realtime-safe.
pub struct PreviewPlayer {
    rx: Consumer<Message>,
    back: Producer<Returned>,
    bank: Option<Arc<SampleBank>>,
    schedule: Option<Box<PreviewSchedule>>,
    /// Frames since the schedule started.
    clock: u64,
    next: usize,
    gain: f32,
    voices: [Voice; MAX_PREVIEW_VOICES],
    playing: Arc<AtomicBool>,
    attack_step: f32,
    release_decay: f32,
    stop_decay: f32,
}

/// A connected control/player pair for an output stream at `output_rate`.
pub fn preview_pair(bank: Option<Arc<SampleBank>>, output_rate: u32) -> (PreviewControl, PreviewPlayer) {
    let (tx, rx) = RingBuffer::new(RING);
    // Room for every message's return plus a finished schedule.
    let (back_tx, back) = RingBuffer::new(RING * 2 + 2);
    let playing = Arc::new(AtomicBool::new(false));
    let rate = output_rate.max(1) as f32;
    let decay = |seconds: f32| (-1.0 / (seconds * rate)).exp();
    let control = PreviewControl { tx, back, bank: bank.clone(), playing: playing.clone(), output_rate };
    let player = PreviewPlayer {
        rx,
        back: back_tx,
        bank,
        schedule: None,
        clock: 0,
        next: 0,
        gain: 0.0,
        voices: [Voice::default(); MAX_PREVIEW_VOICES],
        playing,
        attack_step: 1.0 / (PREVIEW_ATTACK_SECONDS * rate).max(1.0),
        release_decay: decay(PREVIEW_RELEASE_SECONDS),
        stop_decay: decay(PREVIEW_STOP_SECONDS),
    };
    (control, player)
}

impl PreviewControl {
    /// Start a preview now, replacing any preview still playing.
    pub fn play(&mut self, request: &PreviewRequest) -> Result<PreviewStarted, EngineError> {
        self.collect();
        let Some(bank) = self.bank.as_ref() else {
            return Err(EngineError::Invalid("no preview instrument is loaded".into()));
        };
        let schedule = PreviewSchedule::build(request, bank, self.output_rate)?;
        let seconds = schedule.seconds(self.output_rate);
        self.tx
            .push(Message::Play(Box::new(schedule)))
            .map_err(|_| EngineError::Busy("the preview queue is full".into()))?;
        self.playing.store(true, Ordering::Relaxed);
        Ok(PreviewStarted { seconds })
    }

    /// Stop the preview with a short fade. No-op when nothing plays.
    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.collect();
        self.tx.push(Message::Stop).map_err(|_| EngineError::Busy("the preview queue is full".into()))
    }

    /// Use `bank` from now on (voices sounding with the old one are cut).
    pub fn set_bank(&mut self, bank: Arc<SampleBank>) -> Result<(), EngineError> {
        self.collect();
        self.tx
            .push(Message::SetBank(bank.clone()))
            .map_err(|_| EngineError::Busy("the preview queue is full".into()))?;
        self.bank = Some(bank);
        Ok(())
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// Drop whatever the callback has finished with. Call regularly.
    pub fn collect(&mut self) {
        while let Ok(returned) = self.back.pop() {
            drop(returned);
        }
    }
}

impl PreviewPlayer {
    fn drain(&mut self) {
        while let Ok(message) = self.rx.pop() {
            match message {
                Message::Play(schedule) => {
                    // Any notes still sounding are released, not cut.
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.released = true;
                        v.decay = self.release_decay;
                    }
                    self.retire_schedule();
                    self.gain = schedule.gain;
                    self.schedule = Some(schedule);
                    self.clock = 0;
                    self.next = 0;
                }
                Message::Stop => {
                    for v in self.voices.iter_mut().filter(|v| v.active) {
                        v.released = true;
                        v.decay = self.stop_decay;
                    }
                    self.retire_schedule();
                }
                Message::SetBank(bank) => {
                    for v in &mut self.voices {
                        v.active = false;
                    }
                    self.retire_schedule();
                    if let Some(old) = self.bank.replace(bank) {
                        // The ring is sized for every message; if it is somehow full the
                        // old bank is kept alive by leaking one Arc count, never freed here.
                        if let Err(rtrb::PushError::Full(Returned::Bank(old))) = self.back.push(Returned::Bank(old)) {
                            std::mem::forget(old);
                        }
                    }
                }
            }
        }
    }

    fn retire_schedule(&mut self) {
        if let Some(s) = self.schedule.take() {
            if let Err(rtrb::PushError::Full(Returned::Schedule(s))) = self.back.push(Returned::Schedule(s)) {
                // Never free on the audio thread: keep it until the ring has room.
                self.schedule = Some(s);
                self.next = usize::MAX;
            }
        }
    }

    fn start_voice(&mut self, n: Scheduled) {
        let slot = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            None => {
                // Take over the quietest voice.
                let mut best = 0;
                for (i, v) in self.voices.iter().enumerate() {
                    if v.env * v.amp < self.voices[best].env * self.voices[best].amp {
                        best = i;
                    }
                }
                best
            }
        };
        self.voices[slot] = Voice {
            active: true,
            zone: n.zone,
            pos: 0.0,
            step: n.step,
            amp: n.amp,
            env: 0.0,
            end: n.end,
            released: false,
            decay: self.release_decay,
        };
    }

    /// Add the preview into `out` (mono, output rate). Realtime-safe.
    pub fn process(&mut self, out: &mut [f32]) {
        self.drain();
        // Moved out while rendering (a move, never a free), then put back.
        let Some(bank) = self.bank.take() else { return };
        let schedule = self.schedule.take();
        if schedule.is_some() || self.voices.iter().any(|v| v.active) {
            self.render(out, &bank, schedule.as_deref());
        }
        self.bank = Some(bank);
        self.schedule = schedule;
        let done = self.schedule.as_ref().is_some_and(|s| self.next >= s.notes.len());
        if done && !self.voices.iter().any(|v| v.active) {
            self.retire_schedule();
        }
        if self.schedule.is_none() && !self.voices.iter().any(|v| v.active) {
            self.playing.store(false, Ordering::Relaxed);
        }
    }

    fn render(&mut self, out: &mut [f32], bank: &SampleBank, schedule: Option<&PreviewSchedule>) {
        for o in out.iter_mut() {
            if let Some(s) = schedule {
                while let Some(&n) = s.notes.get(self.next).filter(|n| n.start <= self.clock) {
                    self.next += 1;
                    self.start_voice(n);
                }
            }
            let mut y = 0.0f32;
            for v in self.voices.iter_mut().filter(|v| v.active) {
                let data = &bank.zones[v.zone].data;
                let i = v.pos as usize;
                if i + 1 >= data.len() {
                    v.active = false;
                    continue;
                }
                if !v.released && self.clock >= v.end {
                    v.released = true;
                }
                if v.released {
                    v.env *= v.decay;
                    if v.env < SILENT {
                        v.active = false;
                        continue;
                    }
                } else if v.env < 1.0 {
                    v.env = (v.env + self.attack_step).min(1.0);
                }
                let frac = (v.pos - i as f64) as f32;
                let s = data[i] + (data[i + 1] - data[i]) * frac;
                y += s * v.amp * v.env;
                v.pos += v.step;
            }
            *o = soft_limit(*o + y * self.gain);
            self.clock = self.clock.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    /// A bank with one zone: a 440 Hz sine at A4, one second long.
    fn sine_bank() -> Arc<SampleBank> {
        let data: Vec<f32> =
            (0..SR).map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / SR as f32).sin() * 0.5).collect();
        Arc::new(SampleBank::new(vec![SampleZone { root: 69.0, sample_rate: SR, data: data.into() }]))
    }

    fn request(notes: &[(f32, f64, f64)]) -> PreviewRequest {
        PreviewRequest {
            notes: notes
                .iter()
                .map(|&(midi, start, duration)| PreviewNote { midi, start, duration, velocity: Some(1.0) })
                .collect(),
            gain: 1.0,
            tuning_a4: None,
        }
    }

    fn run(player: &mut PreviewPlayer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames];
        for block in out.chunks_mut(256) {
            player.process(block);
        }
        out
    }

    /// Zero crossings per second of `x` / 2 ≈ frequency.
    fn freq(x: &[f32]) -> f32 {
        let crossings = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        crossings as f32 * SR as f32 / x.len() as f32
    }

    #[test]
    fn nearest_zone_prefers_the_closer_root() {
        let zone = |root: f32| SampleZone { root, sample_rate: SR, data: vec![0.0; 4].into() };
        let bank = SampleBank::new(vec![zone(60.0), zone(63.0), zone(66.0)]);
        assert_eq!(bank.nearest(20.0), Some(0));
        assert_eq!(bank.nearest(61.4), Some(0));
        assert_eq!(bank.nearest(61.6), Some(1));
        assert_eq!(bank.nearest(90.0), Some(2));
        assert_eq!(SampleBank::default().nearest(60.0), None);
    }

    #[test]
    fn plays_each_note_at_its_time_and_pitch_then_goes_quiet() {
        let (mut control, mut player) = preview_pair(Some(sine_bank()), SR);
        let started = control.play(&request(&[(69.0, 0.0, 0.25), (81.0, 1.0, 0.25)])).unwrap();
        assert!(control.is_playing());
        assert!((started.seconds - (1.25 + 6.0 * PREVIEW_RELEASE_SECONDS as f64)).abs() < 1e-6);
        let out = run(&mut player, 2 * SR as usize);
        let at = |s: f64| (s * SR as f64) as usize;
        let a4 = &out[at(0.05)..at(0.2)];
        assert!((freq(a4) - 440.0).abs() < 10.0, "A4 at its own pitch: {}", freq(a4));
        // Released at 0.25 s: below the silence threshold within ~9 release time constants.
        let gap = &out[at(0.25 + 9.0 * PREVIEW_RELEASE_SECONDS as f64)..at(1.0)];
        assert!(gap.iter().all(|v| v.abs() < 1e-3), "released before the second note");
        let a5 = &out[at(1.05)..at(1.2)];
        assert!((freq(a5) - 880.0).abs() < 15.0, "an octave up is resampled: {}", freq(a5));
        assert!(out[at(1.25 + 9.0 * PREVIEW_RELEASE_SECONDS as f64)..].iter().all(|v| v.abs() < 1e-3));
        assert!(!control.is_playing(), "the callback reports the end");
        control.collect();
    }

    #[test]
    fn stop_fades_quickly_and_a_new_preview_replaces_the_old_one() {
        let (mut control, mut player) = preview_pair(Some(sine_bank()), SR);
        control.play(&request(&[(69.0, 0.0, 2.0)])).unwrap();
        run(&mut player, 4_800);
        control.stop().unwrap();
        let out = run(&mut player, 9_600);
        let fade = (8.0 * PREVIEW_STOP_SECONDS * SR as f32) as usize;
        assert!(out[fade..].iter().all(|v| v.abs() < 1e-3), "silent within 8 stop time constants");
        assert!(!control.is_playing());
        control.play(&request(&[(69.0, 0.0, 0.1)])).unwrap();
        let out = run(&mut player, 2_400);
        assert!(out.iter().any(|v| v.abs() > 0.1), "plays again after a stop");
    }

    #[test]
    fn tuning_and_gain_are_applied_and_bad_requests_are_refused() {
        let (mut control, mut player) = preview_pair(Some(sine_bank()), SR);
        let mut r = request(&[(69.0, 0.0, 0.5)]);
        r.tuning_a4 = Some(432.0);
        r.gain = 0.5;
        control.play(&r).unwrap();
        let out = run(&mut player, 12_000);
        assert!((freq(&out[2_400..]) - 432.0).abs() < 10.0);
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.2 && peak < 0.3, "gain 0.5 of a 0.5 sine: {peak}");
        r.gain = 3.0;
        assert!(control.play(&r).is_err());
        let mut bad = request(&[(69.0, -1.0, 0.5)]);
        assert!(control.play(&bad).is_err());
        bad.notes[0].start = f64::NAN;
        assert!(control.play(&bad).is_err());
        let (mut none, _) = preview_pair(None, SR);
        assert!(none.play(&request(&[(69.0, 0.0, 0.5)])).is_err(), "no bank loaded");
    }

    #[test]
    fn more_notes_than_voices_take_over_the_quietest_voice() {
        let (mut control, mut player) = preview_pair(Some(sine_bank()), SR);
        let notes: Vec<_> = (0..MAX_PREVIEW_VOICES + 4).map(|i| (60.0 + i as f32, i as f64 * 0.001, 1.0)).collect();
        control.play(&request(&notes)).unwrap();
        let out = run(&mut player, 4_800);
        assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "limited, no overflow");
        assert_eq!(player.voices.iter().filter(|v| v.active).count(), MAX_PREVIEW_VOICES);
    }

    #[test]
    fn bank_swap_and_finished_schedules_come_back_to_the_control_thread() {
        let (mut control, mut player) = preview_pair(None, SR);
        control.set_bank(sine_bank()).unwrap();
        control.play(&request(&[(69.0, 0.0, 0.01)])).unwrap();
        run(&mut player, 4_800);
        control.set_bank(sine_bank()).unwrap();
        run(&mut player, 256);
        let mut returned = 0;
        while control.back.pop().is_ok() {
            returned += 1;
        }
        assert_eq!(returned, 2, "the finished schedule and the replaced bank");
    }

    #[test]
    fn loads_the_bundled_piano_samples() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/resources/piano");
        let bank = SampleBank::load_dir(&dir).unwrap();
        assert_eq!(bank.len(), 30, "A0..C8 every minor third");
        assert_eq!(bank.zones[0].root, 21.0);
        assert_eq!(bank.zones[29].root, 108.0);
    }
}
