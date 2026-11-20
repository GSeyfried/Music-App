//! Song player for VoiceLoop Pocket's Song Builder (docs/projects/song-builder).
//!
//! A loop of parts (drum kits, melody instruments), each through its own
//! effect chain (`voiceloop-dsp`, the same DSP as desktop track effects),
//! mixed into the output after the looper and previews. Heard, never
//! recorded.
//!
//! ```text
//!  control thread (SongControl)                     output callback (SongPlayer::process)
//!  set(spec)  ─ Box<SongSchedule>, Box<ChainDsp> ─▶ swaps schedule / part chains, keeps position
//!  play/stop  ─ Play{count_in} / Stop ───────────▶ count-in clicks, then loops the schedule
//!  trigger    ─ Trigger{slot, voice} ────────────▶ starts a voice now (pads, voice monitoring)
//!  collect()  ◀─ old schedules and chains ──────── dropped on the control thread
//!  position() / locate()  ◀─ atomics ─────────────  playing, position, output frame of beat 0
//! ```
//!
//! The song's clock is the output frame counter, so playback is sample
//! accurate. The caller sends events at their *played* beat (Tightness is
//! applied by the app, which keeps the raw performance). `locate` maps a
//! microphone input frame, or "now" for a touch, to the beat the performer
//! heard at that moment (input and output latency removed).
//!
//! **Realtime:** no allocation, freeing, locking or I/O in `process`. Voices
//! are a fixed pool; schedules and chains are built and dropped on the
//! control thread.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};
use voiceloop_dsp::{ChainDsp, ChainSpec, EffectKind, EffectSpec};

use crate::clock::{Anchor, StreamStats};
use crate::error::EngineError;
use crate::looper::soft_limit;

/// Parts (and effect buses) at once.
pub const MAX_SONG_PARTS: usize = 8;
/// Voices sounding at once (drum tails overlap; the quietest is stolen).
pub const MAX_SONG_VOICES: usize = 64;
/// Events in one song.
/// `frames` of a held note: plays until released.
const HELD: u32 = u32::MAX;
pub const MAX_SONG_EVENTS: usize = 8_192;
/// Messages queued to the audio thread.
const QUEUE: usize = 64;
/// Gain slew per sample when a part level changes (≈ 20 ms at 48 kHz).
const GAIN_STEP: f32 = 1.0 / 960.0;

// ---- Contract --------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DrumLane {
    Kick,
    Snare,
    Hat,
    Clap,
    OpenHat,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PartSound {
    Drums { kit: String },
    Melody { instrument: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongEvent {
    /// Beat in the loop where it plays (after the app's Tightness), 0 ≤ beat < loopBeats.
    pub beat: f64,
    #[serde(default = "one")]
    pub velocity: f32,
    #[serde(default)]
    pub lane: Option<DrumLane>,
    #[serde(default)]
    pub midi: Option<u8>,
    /// Melody note length in beats.
    #[serde(default)]
    pub length_beats: f64,
    /// Do not play in this loop pass (it was just heard live while being recorded).
    #[serde(default)]
    pub skip_pass: Option<i64>,
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongPart {
    pub id: u32,
    pub sound: PartSound,
    /// Linear, 0..=2.
    #[serde(default = "one")]
    pub gain: f32,
    /// FX preset id (see [`FX_PRESETS`]), or several joined with `+` and run
    /// in that order (`"room+punch"`, at most [`MAX_FX_PER_PART`]); unknown = clean.
    #[serde(default)]
    pub fx: String,
    #[serde(default)]
    pub events: Vec<SongEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongSpec {
    pub bpm: f64,
    pub beats_per_bar: u32,
    pub loop_beats: f64,
    #[serde(default = "yes")]
    pub click: bool,
    #[serde(default = "click_gain")]
    pub click_gain: f32,
    #[serde(default = "master_gain")]
    pub master_gain: f32,
    /// Make-up gain into the master soft limiter, dB (0…12). Louder on small
    /// speakers at the cost of squashing peaks (`songBuilder.loudnessDb`).
    #[serde(default)]
    pub loudness_db: f32,
    /// Audible parts only (the app resolves mute and solo).
    #[serde(default)]
    pub parts: Vec<SongPart>,
}

fn yes() -> bool {
    true
}
fn click_gain() -> f32 {
    0.45
}
fn master_gain() -> f32 {
    0.85
}

impl SongSpec {
    pub fn validate(&self) -> Result<(), EngineError> {
        let bad = |m: &str| Err(EngineError::Invalid(format!("song: {m}")));
        if !(self.bpm.is_finite() && (20.0..=400.0).contains(&self.bpm)) {
            return bad("bpm must be 20..=400");
        }
        if !(1..=16).contains(&self.beats_per_bar) {
            return bad("beatsPerBar must be 1..=16");
        }
        if !(self.loop_beats.is_finite() && (0.25..=256.0).contains(&self.loop_beats)) {
            return bad("loopBeats must be 0.25..=256");
        }
        if self.parts.len() > MAX_SONG_PARTS {
            return bad("too many parts");
        }
        if self.parts.iter().map(|p| p.events.len()).sum::<usize>() > MAX_SONG_EVENTS {
            return bad("too many events");
        }
        Ok(())
    }
}

/// Play one sound now (touch pads, live voice-drum monitoring, piano keys).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongTrigger {
    pub part_id: u32,
    #[serde(default)]
    pub lane: Option<DrumLane>,
    #[serde(default)]
    pub midi: Option<u8>,
    #[serde(default = "one")]
    pub velocity: f32,
    #[serde(default)]
    pub length_beats: f64,
    /// Hold a melody note until `song_release` (live voice → piano). Ignores `lengthBeats`.
    #[serde(default)]
    pub hold: bool,
}

/// Release held notes of a part: one `midi`, or all of them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongRelease {
    pub part_id: u32,
    #[serde(default)]
    pub midi: Option<u8>,
}

/// Sent as the `songPosition` event (~30 Hz while playing, once on stop).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongPosition {
    pub playing: bool,
    /// Beat in the loop; negative during the count-in.
    pub beat: f64,
    pub pass: i64,
    /// Output frame at which beat 0 of pass 0 is produced.
    pub output_frame: Option<u64>,
}

/// Where a performed moment falls in the song (`song_locate`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SongLocation {
    /// Beats since beat 0 of pass 0 (negative during the count-in).
    pub absolute_beat: f64,
    /// The same, wrapped into the loop.
    pub loop_beat: f64,
    pub pass: i64,
    /// Latency removed, ms (input + output for the mic, output for "now").
    pub latency_ms: f64,
}

// ---- Sounds (defined once; SETTINGS_INDEX "Song Builder" lists these tables) ------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KickSpec {
    pub start_hz: f32,
    pub end_hz: f32,
    pub sweep_seconds: f32,
    pub decay_seconds: f32,
    pub click: f32,
    pub drive: f32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnareSpec {
    pub tone_hz: f32,
    pub tone: f32,
    pub noise: f32,
    pub noise_hz: f32,
    pub decay_seconds: f32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HatSpec {
    pub cutoff_hz: f32,
    pub decay_seconds: f32,
    pub metallic: f32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClapSpec {
    pub bursts: u8,
    pub spacing_seconds: f32,
    pub band_hz: f32,
    pub decay_seconds: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrumKit {
    pub id: &'static str,
    pub label: &'static str,
    pub gain: f32,
    /// Low-pass over the whole kit (Hz); ≥ 19 kHz = open.
    pub tone_hz: f32,
    pub kick: KickSpec,
    pub snare: SnareSpec,
    pub hat: HatSpec,
    pub open_hat: HatSpec,
    pub clap: ClapSpec,
}

pub const DRUM_KITS: [DrumKit; 3] = [
    DrumKit {
        id: "studio",
        label: "Studio",
        gain: 0.9,
        tone_hz: 20_000.0,
        kick: KickSpec { start_hz: 180.0, end_hz: 55.0, sweep_seconds: 0.06, decay_seconds: 0.42, click: 0.5, drive: 0.55 },
        snare: SnareSpec { tone_hz: 190.0, tone: 0.45, noise: 0.75, noise_hz: 1_800.0, decay_seconds: 0.18 },
        hat: HatSpec { cutoff_hz: 7_500.0, decay_seconds: 0.05, metallic: 0.3 },
        open_hat: HatSpec { cutoff_hz: 7_000.0, decay_seconds: 0.32, metallic: 0.3 },
        clap: ClapSpec { bursts: 3, spacing_seconds: 0.011, band_hz: 1_400.0, decay_seconds: 0.14 },
    },
    DrumKit {
        id: "808",
        label: "808",
        gain: 0.85,
        tone_hz: 20_000.0,
        kick: KickSpec { start_hz: 120.0, end_hz: 48.0, sweep_seconds: 0.05, decay_seconds: 0.8, click: 0.25, drive: 0.6 },
        snare: SnareSpec { tone_hz: 240.0, tone: 0.6, noise: 0.55, noise_hz: 2_600.0, decay_seconds: 0.14 },
        hat: HatSpec { cutoff_hz: 8_500.0, decay_seconds: 0.035, metallic: 0.85 },
        open_hat: HatSpec { cutoff_hz: 8_000.0, decay_seconds: 0.38, metallic: 0.85 },
        clap: ClapSpec { bursts: 4, spacing_seconds: 0.009, band_hz: 1_100.0, decay_seconds: 0.2 },
    },
    DrumKit {
        id: "lofi",
        label: "Lo-fi",
        gain: 0.95,
        tone_hz: 4_200.0,
        kick: KickSpec { start_hz: 130.0, end_hz: 56.0, sweep_seconds: 0.06, decay_seconds: 0.32, click: 0.15, drive: 0.75 },
        snare: SnareSpec { tone_hz: 175.0, tone: 0.5, noise: 0.6, noise_hz: 1_300.0, decay_seconds: 0.2 },
        hat: HatSpec { cutoff_hz: 5_500.0, decay_seconds: 0.06, metallic: 0.15 },
        open_hat: HatSpec { cutoff_hz: 5_000.0, decay_seconds: 0.25, metallic: 0.15 },
        clap: ClapSpec { bursts: 3, spacing_seconds: 0.013, band_hz: 1_000.0, decay_seconds: 0.16 },
    },
];

fn kit_index(id: &str) -> u8 {
    DRUM_KITS.iter().position(|k| k.id == id).unwrap_or(0) as u8
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wave {
    Sine,
    Saw,
    Triangle,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MelodyVoice {
    pub id: &'static str,
    pub label: &'static str,
    /// (harmonic, amplitude, wave); at most 5.
    pub partials: &'static [(f32, f32, Wave)],
    pub attack_seconds: f32,
    pub decay_seconds: f32,
    pub sustain: f32,
    pub release_seconds: f32,
    /// Low-pass cutoff as a multiple of the note frequency (capped at 12 kHz).
    pub brightness: f32,
    pub transpose: i8,
    pub gain: f32,
}

pub const MELODY_VOICES: [MelodyVoice; 3] = [
    MelodyVoice {
        id: "piano",
        label: "Piano",
        partials: &[(1.0, 1.0, Wave::Sine), (2.0, 0.55, Wave::Sine), (3.0, 0.3, Wave::Sine), (4.0, 0.16, Wave::Sine), (5.0, 0.08, Wave::Sine)],
        attack_seconds: 0.004,
        decay_seconds: 0.9,
        sustain: 0.15,
        release_seconds: 0.25,
        brightness: 9.0,
        transpose: 0,
        gain: 0.5,
    },
    MelodyVoice {
        id: "softSynth",
        label: "Soft synth",
        partials: &[(1.0, 1.0, Wave::Saw), (1.005, 0.7, Wave::Saw)],
        attack_seconds: 0.03,
        decay_seconds: 0.4,
        sustain: 0.6,
        release_seconds: 0.35,
        brightness: 4.0,
        transpose: 0,
        gain: 0.22,
    },
    MelodyVoice {
        id: "bass",
        label: "Bass",
        partials: &[(1.0, 1.0, Wave::Triangle), (2.0, 0.3, Wave::Square)],
        attack_seconds: 0.005,
        decay_seconds: 0.35,
        sustain: 0.55,
        release_seconds: 0.12,
        brightness: 6.0,
        transpose: -12,
        gain: 0.55,
    },
];

fn voice_index(id: &str) -> u8 {
    MELODY_VOICES.iter().position(|v| v.id == id).unwrap_or(0) as u8
}

/// FX preset ids, labels and one-line help (the chains are in [`fx_preset`]).
pub const FX_PRESETS: [(&str, &str, &str); 7] = [
    ("clean", "Clean", "No processing."),
    ("room", "Room", "A small room around the sound."),
    ("hall", "Big hall", "Long, wide reverb."),
    ("echo", "Echo", "Dotted-eighth echoes that follow the tempo."),
    ("punch", "Punch", "Compressed and a little driven: louder, tighter hits."),
    ("lofi", "Lo-fi", "Crunchy, dull and narrow, like an old sampler."),
    ("radio", "Radio", "Thin telephone/radio band."),
];

/// Presets that can be stacked on one part (`songBuilder.fxPerPart`).
pub const MAX_FX_PER_PART: usize = 3;

/// The chain for a part's `fx`: presets joined with `+`, in order. Effects past
/// the chain's capacity are dropped.
pub fn fx_stack(ids: &str) -> ChainSpec {
    let mut out = ChainSpec::EMPTY;
    for (i, id) in ids.split('+').map(str::trim).filter(|s| !s.is_empty()).take(MAX_FX_PER_PART).enumerate() {
        for e in fx_preset(id).effects() {
            let mut e = *e;
            // Unique ids across the stack: "0rv", "1rv", ...
            let tag = format!("{i}{}", e.id.as_str());
            e.id = voiceloop_dsp::EffectId::new(&tag).unwrap_or(e.id);
            if !out.push(e) {
                return out;
            }
        }
    }
    out
}

/// The effect chain of an FX preset (built from the desktop effect blocks).
pub fn fx_preset(id: &str) -> ChainSpec {
    let fx = |id: &str, kind: EffectKind, mix: f32, params: &[(&str, f32)], choices: &[(&str, &str)]| {
        let mut e = EffectSpec::new(id, kind);
        e.mix = mix;
        for (k, v) in params {
            e.set(k, *v);
        }
        for (k, v) in choices {
            e.set_choice(k, v);
        }
        e
    };
    match id {
        "room" => ChainSpec::with(&[fx("rv", EffectKind::Reverb, 0.22, &[("size", 0.3), ("decaySec", 0.7), ("damping", 0.5), ("preDelayMs", 5.0)], &[])]),
        "hall" => ChainSpec::with(&[
            fx("rv", EffectKind::Reverb, 0.3, &[("size", 0.85), ("decaySec", 2.4), ("damping", 0.35), ("preDelayMs", 25.0)], &[]),
            fx("lp", EffectKind::Filter, 1.0, &[("cutoffHz", 14_000.0)], &[("type", "lowpass")]),
        ]),
        "echo" => ChainSpec::with(&[fx("dl", EffectKind::Delay, 0.3, &[("feedback", 0.38), ("toneHz", 3_500.0)], &[("sync", "1/8d")])]),
        "punch" => ChainSpec::with(&[
            fx("cp", EffectKind::Compressor, 1.0, &[("thresholdDb", -20.0), ("ratio", 5.0), ("attackMs", 4.0), ("releaseMs", 120.0), ("makeupDb", 6.0)], &[]),
            fx("st", EffectKind::Saturation, 0.35, &[("driveDb", 8.0), ("outputDb", -2.0)], &[("mode", "soft")]),
        ]),
        "lofi" => ChainSpec::with(&[
            fx("hp", EffectKind::Filter, 1.0, &[("cutoffHz", 120.0)], &[("type", "highpass")]),
            fx("bc", EffectKind::Bitcrusher, 1.0, &[("bits", 7.0), ("downsample", 3.0)], &[]),
            fx("lp", EffectKind::Filter, 1.0, &[("cutoffHz", 3_800.0)], &[("type", "lowpass")]),
        ]),
        "radio" => ChainSpec::with(&[
            fx("bp", EffectKind::Filter, 1.0, &[("cutoffHz", 1_400.0), ("q", 0.9)], &[("type", "bandpass")]),
            fx("st", EffectKind::Saturation, 1.0, &[("driveDb", 14.0), ("outputDb", -6.0)], &[("mode", "hard")]),
        ]),
        _ => ChainSpec::EMPTY,
    }
}

// ---- Voices (realtime) -----------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum VoiceStart {
    Drum { kit: u8, lane: DrumLane, velocity: f32 },
    Note { voice: u8, midi: u8, velocity: f32, frames: u32 },
    Click { downbeat: bool, gain: f32 },
}

/// RBJ biquad (direct form I), coefficients set when a voice starts.
#[derive(Debug, Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn new(kind: u8, hz: f32, q: f32, sr: f32) -> Self {
        let w = std::f32::consts::TAU * (hz / sr).clamp(1e-5, 0.49);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.05));
        let (b0, b1, b2) = match kind {
            0 => ((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0), // low-pass
            1 => ((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0), // high-pass
            _ => (alpha, 0.0, -alpha),                        // band-pass (0 dB peak)
        };
        let a0 = 1.0 + alpha;
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: -2.0 * c / a0, a2: (1.0 - alpha) / a0, ..Default::default() }
    }
    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Per-sample multiplier that falls by 80 dB over `seconds`.
fn decay_coef(seconds: f32, sr: f32) -> f32 {
    (1.0e-4f32.ln() / (seconds.max(0.002) * sr)).exp()
}

const HAT_RATIOS: [f32; 6] = [205.0, 304.0, 369.0, 522.0, 540.0, 800.0];

#[derive(Debug, Clone, Copy)]
struct Voice {
    active: bool,
    /// Part bus; `u8::MAX` = master (the click).
    slot: u8,
    start: VoiceStart,
    t: u32,
    env: f32,
    env_coef: f32,
    env2: f32,
    env2_coef: f32,
    phase: f32,
    freq: f32,
    freq_end: f32,
    freq_coef: f32,
    phases: [f32; 6],
    f1: Biquad,
    f2: Biquad,
    lp: f32,
    lp_coef: f32,
    rng: u32,
    peak: f32,
}

impl Voice {
    const IDLE: Voice = Voice {
        active: false,
        slot: 0,
        start: VoiceStart::Click { downbeat: false, gain: 0.0 },
        t: 0,
        env: 0.0,
        env_coef: 0.0,
        env2: 0.0,
        env2_coef: 0.0,
        phase: 0.0,
        freq: 0.0,
        freq_end: 0.0,
        freq_coef: 0.0,
        phases: [0.0; 6],
        f1: Biquad { b0: 0.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 },
        f2: Biquad { b0: 0.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 },
        lp: 0.0,
        lp_coef: 1.0,
        rng: 0x9E37_79B9,
        peak: 0.0,
    };

    fn begin(&mut self, slot: u8, start: VoiceStart, sr: f32, seed: u32) {
        *self = Voice::IDLE;
        self.active = true;
        self.slot = slot;
        self.start = start;
        self.rng = seed | 1;
        match start {
            VoiceStart::Drum { kit, lane, velocity } => {
                let k = &DRUM_KITS[kit as usize % DRUM_KITS.len()];
                let v = velocity.clamp(0.05, 1.0) * k.gain;
                self.peak = v;
                self.lp_coef = if k.tone_hz >= 19_000.0 { 1.0 } else { 1.0 - (-std::f32::consts::TAU * k.tone_hz / sr).exp() };
                match lane {
                    DrumLane::Kick => {
                        self.freq = k.kick.start_hz;
                        self.freq_end = k.kick.end_hz;
                        self.freq_coef = decay_coef(k.kick.sweep_seconds * 0.35, sr);
                        self.env = v;
                        self.env_coef = decay_coef(k.kick.decay_seconds, sr);
                        self.env2 = v * k.kick.click;
                        self.env2_coef = decay_coef(0.012, sr);
                        self.f1 = Biquad::new(1, 2_000.0, 0.7, sr);
                    }
                    DrumLane::Snare => {
                        self.freq = k.snare.tone_hz * 1.4;
                        self.freq_end = k.snare.tone_hz;
                        self.freq_coef = decay_coef(0.02, sr);
                        self.env = v * k.snare.tone;
                        self.env_coef = decay_coef(k.snare.decay_seconds * 0.6, sr);
                        self.env2 = v * k.snare.noise;
                        self.env2_coef = decay_coef(k.snare.decay_seconds, sr);
                        self.f1 = Biquad::new(1, k.snare.noise_hz, 0.7, sr);
                    }
                    DrumLane::Hat | DrumLane::OpenHat => {
                        let h = if lane == DrumLane::Hat { k.hat } else { k.open_hat };
                        self.env = v * 0.55;
                        self.env_coef = decay_coef(h.decay_seconds, sr);
                        self.env2 = h.metallic;
                        self.f1 = Biquad::new(1, h.cutoff_hz, 0.7, sr);
                        self.f2 = Biquad::new(1, h.cutoff_hz, 0.7, sr);
                    }
                    DrumLane::Clap => {
                        self.env = v * 0.6;
                        self.env_coef = decay_coef(0.012, sr);
                        self.f1 = Biquad::new(2, k.clap.band_hz, 1.2, sr);
                    }
                }
            }
            VoiceStart::Note { voice, midi, velocity, .. } => {
                let m = &MELODY_VOICES[voice as usize % MELODY_VOICES.len()];
                let hz = 440.0 * 2f32.powf((midi as f32 + m.transpose as f32 - 69.0) / 12.0);
                self.freq = hz;
                self.peak = velocity.clamp(0.05, 1.0) * m.gain;
                self.env = 0.0;
                self.lp_coef = 1.0 - (-std::f32::consts::TAU * (hz * m.brightness).min(12_000.0) / sr).exp();
            }
            VoiceStart::Click { gain, .. } => {
                self.env = gain;
                self.env_coef = decay_coef(0.04, sr);
                self.peak = gain;
            }
        }
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Current loudness, for stealing.
    fn level(&self) -> f32 {
        self.env.max(self.env2 * 0.5)
    }

    #[inline]
    fn next(&mut self, sr: f32) -> f32 {
        let inv_sr = 1.0 / sr;
        let t = self.t;
        self.t = self.t.saturating_add(1);
        let y = match self.start {
            VoiceStart::Drum { kit, lane, .. } => {
                let k = &DRUM_KITS[kit as usize % DRUM_KITS.len()];
                let raw = match lane {
                    DrumLane::Kick => {
                        self.freq = self.freq_end + (self.freq - self.freq_end) * self.freq_coef;
                        self.phase = (self.phase + self.freq * inv_sr).fract();
                        let mut s = (std::f32::consts::TAU * self.phase).sin() * self.env;
                        if k.kick.drive > 0.0 {
                            let g = 1.0 + k.kick.drive * 12.0;
                            s = (g * s).tanh() / g.tanh();
                        }
                        let n = self.noise();
                        let click = self.f1.run(n) * self.env2;
                        self.env *= self.env_coef;
                        self.env2 *= self.env2_coef;
                        if self.env < 1e-4 && self.env2 < 1e-4 {
                            self.active = false;
                        }
                        s + click
                    }
                    DrumLane::Snare => {
                        self.freq = self.freq_end + (self.freq - self.freq_end) * self.freq_coef;
                        self.phase = (self.phase + self.freq * inv_sr).fract();
                        let tri = 1.0 - 4.0 * (self.phase - 0.5).abs();
                        let n = self.noise();
                        let s = tri * self.env + self.f1.run(n) * self.env2;
                        self.env *= self.env_coef;
                        self.env2 *= self.env2_coef;
                        if self.env < 1e-4 && self.env2 < 1e-4 {
                            self.active = false;
                        }
                        s
                    }
                    DrumLane::Hat | DrumLane::OpenHat => {
                        let mut metal = 0.0;
                        if self.env2 > 0.0 {
                            for (p, r) in self.phases.iter_mut().zip(HAT_RATIOS) {
                                *p = (*p + r * 1.7 * inv_sr).fract();
                                metal += if *p < 0.5 { 1.0 } else { -1.0 };
                            }
                            metal *= self.env2 * 0.18 / 6.0 * 3.0;
                        }
                        let n = self.noise() + metal;
                        let s = self.f2.run(self.f1.run(n)) * self.env;
                        self.env *= self.env_coef;
                        if self.env < 1e-4 {
                            self.active = false;
                        }
                        s
                    }
                    DrumLane::Clap => {
                        let spacing = (k.clap.spacing_seconds * sr) as u32;
                        let bursts = k.clap.bursts.max(1) as u32;
                        if spacing > 0 && t > 0 && t % spacing == 0 && t / spacing < bursts {
                            let last = t / spacing == bursts - 1;
                            self.env = self.peak * if last { 0.9 } else { 0.6 };
                            self.env_coef = decay_coef(if last { k.clap.decay_seconds } else { 0.012 }, sr);
                        }
                        let n = self.noise();
                        let s = self.f1.run(n) * self.env * 2.0;
                        self.env *= self.env_coef;
                        if self.env < 1e-4 && t > spacing * bursts {
                            self.active = false;
                        }
                        s
                    }
                };
                self.lp += self.lp_coef * (raw - self.lp);
                self.lp
            }
            VoiceStart::Note { voice, frames, .. } => {
                let m = &MELODY_VOICES[voice as usize % MELODY_VOICES.len()];
                let attack = (m.attack_seconds * sr).max(1.0) as u32;
                let target = if t < attack {
                    self.peak * (t as f32 / attack as f32)
                } else if t < frames {
                    let d = (t - attack) as f32 / (m.decay_seconds.max(0.01) * sr / 3.0);
                    let sus = self.peak * m.sustain;
                    sus + (self.peak - sus) * (-d).exp()
                } else {
                    -1.0
                };
                if target >= 0.0 {
                    self.env = target;
                } else {
                    self.env *= decay_coef(m.release_seconds, sr);
                    if self.env < 1e-4 {
                        self.active = false;
                    }
                }
                self.phase += self.freq * inv_sr;
                if self.phase > 4096.0 {
                    self.phase -= 4096.0;
                }
                let mut s = 0.0;
                let n = m.partials.len().max(1) as f32;
                for &(h, amp, wave) in m.partials.iter().take(5) {
                    if self.freq * h > sr * 0.45 {
                        continue;
                    }
                    let p = (self.phase * h).fract();
                    let w = match wave {
                        Wave::Sine => (std::f32::consts::TAU * p).sin(),
                        Wave::Saw => 2.0 * p - 1.0,
                        Wave::Triangle => 1.0 - 4.0 * (p - 0.5).abs(),
                        Wave::Square => {
                            if p < 0.5 {
                                1.0
                            } else {
                                -1.0
                            }
                        }
                    };
                    s += w * amp / n;
                }
                self.lp += self.lp_coef * (s - self.lp);
                self.lp * self.env
            }
            VoiceStart::Click { downbeat, .. } => {
                let hz = if downbeat { 1_500.0 } else { 1_000.0 };
                self.phase = (self.phase + hz * inv_sr).fract();
                let s = if self.phase < 0.5 { self.env } else { -self.env };
                self.env *= self.env_coef;
                if self.env < 1e-4 {
                    self.active = false;
                }
                s * if downbeat { 0.5 } else { 0.28 }
            }
        };
        if y.is_finite() {
            y
        } else {
            self.active = false;
            0.0
        }
    }
}

// ---- Schedule ------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Scheduled {
    frame: u64,
    slot: u8,
    start: VoiceStart,
    skip_pass: i64,
}

/// A song prepared for the audio thread (sorted, in output frames).
pub struct SongSchedule {
    loop_frames: u64,
    frames_per_beat: f64,
    beats_per_bar: u32,
    click: bool,
    click_gain: f32,
    master_gain: f32,
    gains: [f32; MAX_SONG_PARTS],
    used: [bool; MAX_SONG_PARTS],
    events: Vec<Scheduled>,
}

enum Msg {
    Schedule(Box<SongSchedule>),
    Bus { slot: u8, chain: Box<ChainDsp> },
    Play { count_in_frames: u64 },
    Stop,
    Trigger { slot: u8, start: VoiceStart },
    /// Release held notes on `slot` (`midi` 255 = all).
    Release { slot: u8, midi: u8 },
}

enum Garbage {
    Schedule(#[allow(dead_code)] Box<SongSchedule>),
    Chain(#[allow(dead_code)] Box<ChainDsp>),
}

#[derive(Debug, Default)]
struct Shared {
    playing: AtomicBool,
    /// Frames since beat 0 of pass 0 (negative in the count-in), at the end of the last block.
    position: AtomicI64,
    /// Output frame (stream counter) of beat 0 of pass 0; `u64::MAX` = none.
    origin: AtomicU64,
    /// f64 bits.
    frames_per_beat: AtomicU64,
    loop_frames: AtomicU64,
}

// ---- Audio side ------------------------------------------------------------------------

pub struct SongPlayer {
    rx: Consumer<Msg>,
    garbage: Producer<Garbage>,
    shared: Arc<Shared>,
    sr: f32,
    schedule: Option<Box<SongSchedule>>,
    buses: [Option<Box<ChainDsp>>; MAX_SONG_PARTS],
    gains: [f32; MAX_SONG_PARTS],
    voices: [Voice; MAX_SONG_VOICES],
    playing: bool,
    /// Frames relative to beat 0 of pass 0.
    pos: i64,
    cursor: usize,
    last_beat: i64,
    seed: u32,
}

impl SongPlayer {
    fn send_garbage(&mut self, g: Garbage) {
        // The ring is sized for every message; if it is somehow full, leak rather than free here.
        if let Err(rtrb::PushError::Full(g)) = self.garbage.push(g) {
            std::mem::forget(g);
        }
    }

    fn start_voice(&mut self, slot: u8, start: VoiceStart) {
        let mut best = 0usize;
        let mut best_level = f32::MAX;
        for (i, v) in self.voices.iter().enumerate() {
            if !v.active {
                best = i;
                break;
            }
            let l = v.level();
            if l < best_level {
                best_level = l;
                best = i;
            }
        }
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let (sr, seed) = (self.sr, self.seed);
        self.voices[best].begin(slot, start, sr, seed);
    }

    fn reseek(&mut self) {
        let Some(s) = self.schedule.as_ref() else { return };
        let off = if s.loop_frames == 0 { 0 } else { self.pos.max(0) as u64 % s.loop_frames };
        self.cursor = s.events.partition_point(|e| e.frame < off);
    }

    fn drain(&mut self) {
        while let Ok(msg) = self.rx.pop() {
            match msg {
                Msg::Schedule(next) => {
                    // Keep the musical position across tempo or length changes.
                    if let Some(old) = self.schedule.as_ref() {
                        let beats = self.pos as f64 / old.frames_per_beat;
                        let loops = old.loop_frames as f64 / old.frames_per_beat;
                        let new_loop = next.loop_frames as f64 / next.frames_per_beat;
                        let beats = if beats >= 0.0 && (loops - new_loop).abs() > 1e-9 { beats % new_loop.max(1e-9) } else { beats };
                        self.pos = (beats * next.frames_per_beat).round() as i64;
                    }
                    self.shared.frames_per_beat.store(next.frames_per_beat.to_bits(), Ordering::Relaxed);
                    self.shared.loop_frames.store(next.loop_frames, Ordering::Relaxed);
                    if let Some(old) = self.schedule.replace(next) {
                        self.send_garbage(Garbage::Schedule(old));
                    }
                    self.reseek();
                }
                Msg::Bus { slot, chain } => {
                    let i = slot as usize % MAX_SONG_PARTS;
                    if let Some(old) = self.buses[i].replace(chain) {
                        self.send_garbage(Garbage::Chain(old));
                    }
                }
                Msg::Play { count_in_frames } => {
                    self.playing = true;
                    self.pos = -(count_in_frames as i64);
                    self.cursor = 0;
                    self.last_beat = i64::MIN;
                }
                Msg::Stop => {
                    self.playing = false;
                    for v in &mut self.voices {
                        if !matches!(v.start, VoiceStart::Note { .. }) {
                            continue;
                        }
                        // Let notes release rather than click off.
                        if let VoiceStart::Note { ref mut frames, .. } = v.start {
                            *frames = (*frames).min(v.t);
                        }
                    }
                }
                Msg::Trigger { slot, start } => self.start_voice(slot, start),
                Msg::Release { slot, midi } => {
                    for v in &mut self.voices {
                        if !v.active || v.slot != slot {
                            continue;
                        }
                        if let VoiceStart::Note { midi: m, ref mut frames, .. } = v.start {
                            if *frames == HELD && (midi == u8::MAX || m == midi) {
                                *frames = v.t.max(1);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Mix the song into `out` (mono, in place). `block_frame` is the output
    /// stream frame of `out[0]`. Realtime-safe.
    pub fn process(&mut self, out: &mut [f32], block_frame: u64) {
        self.drain();
        let idle = !self.playing && self.voices.iter().all(|v| !v.active) && self.buses.iter().all(|b| b.is_none());
        if idle {
            return;
        }
        let sr = self.sr;
        let (fpb, loop_frames, bpb, click, click_gain, master) = match self.schedule.as_ref() {
            Some(s) => (s.frames_per_beat, s.loop_frames, s.beats_per_bar.max(1) as i64, s.click, s.click_gain, s.master_gain),
            None => (sr as f64 * 0.5, (sr as f64 * 2.0) as u64, 4, true, 0.45, 0.85),
        };
        let mut targets = [0.0f32; MAX_SONG_PARTS];
        let mut used = [false; MAX_SONG_PARTS];
        if let Some(s) = self.schedule.as_ref() {
            targets = s.gains;
            used = s.used;
        }
        if self.playing && self.pos <= 0 && self.pos + out.len() as i64 > 0 {
            self.shared.origin.store(block_frame + (-self.pos) as u64, Ordering::Relaxed);
        }
        for (i, o) in out.iter_mut().enumerate() {
            if self.playing {
                // Click on every beat boundary (always during the count-in).
                let beat = (self.pos as f64 / fpb).floor() as i64;
                if beat != self.last_beat {
                    let first = self.last_beat == i64::MIN;
                    self.last_beat = beat;
                    let boundary = first && (self.pos as f64 - beat as f64 * fpb) < 1.0 || !first;
                    if boundary && (click || self.pos < 0) {
                        self.start_voice(u8::MAX, VoiceStart::Click { downbeat: beat.rem_euclid(bpb) == 0, gain: click_gain });
                    }
                }
                if self.pos >= 0 && loop_frames > 0 {
                    let off = self.pos as u64 % loop_frames;
                    let pass = (self.pos as u64 / loop_frames) as i64;
                    if off == 0 {
                        self.cursor = 0;
                    }
                    loop {
                        let Some(s) = self.schedule.as_ref() else { break };
                        let Some(e) = s.events.get(self.cursor).copied() else { break };
                        if e.frame != off {
                            break;
                        }
                        self.cursor += 1;
                        if e.skip_pass != pass {
                            self.start_voice(e.slot, e.start);
                        }
                    }
                }
                self.pos += 1;
            }
            let mut bus = [0.0f32; MAX_SONG_PARTS];
            let mut direct = 0.0f32;
            for v in self.voices.iter_mut() {
                if !v.active {
                    continue;
                }
                let y = v.next(sr);
                if v.slot == u8::MAX {
                    direct += y;
                } else {
                    bus[v.slot as usize % MAX_SONG_PARTS] += y;
                }
            }
            let mut mix = direct;
            for s in 0..MAX_SONG_PARTS {
                if !used[s] && self.gains[s] == 0.0 && bus[s] == 0.0 {
                    continue;
                }
                let g = &mut self.gains[s];
                *g = if *g < targets[s] { (*g + GAIN_STEP).min(targets[s]) } else { (*g - GAIN_STEP).max(targets[s]) };
                let y = match self.buses[s].as_mut() {
                    Some(chain) => chain.process(bus[s]),
                    None => bus[s],
                };
                mix += y * *g;
            }
            let _ = i;
            *o = soft_limit(*o + mix * master);
        }
        self.shared.playing.store(self.playing, Ordering::Relaxed);
        self.shared.position.store(self.pos, Ordering::Relaxed);
    }
}

// ---- Control side ----------------------------------------------------------------------

pub struct SongControl {
    tx: Producer<Msg>,
    garbage: Consumer<Garbage>,
    shared: Arc<Shared>,
    sample_rate: u32,
    /// Part id → (bus slot, FX preset id).
    slots: HashMap<u32, (u8, String)>,
    spec: Option<SongSpec>,
}

pub fn song_pair(sample_rate: u32) -> (SongControl, SongPlayer) {
    let (tx, rx) = RingBuffer::new(QUEUE);
    // Room for every schedule and chain that can be in flight.
    let (gtx, grx) = RingBuffer::new(QUEUE + MAX_SONG_PARTS * 2);
    let shared = Arc::new(Shared { origin: AtomicU64::new(u64::MAX), ..Default::default() });
    let control =
        SongControl { tx, garbage: grx, shared: shared.clone(), sample_rate: sample_rate.max(1), slots: HashMap::new(), spec: None };
    let player = SongPlayer {
        rx,
        garbage: gtx,
        shared,
        sr: sample_rate.max(1) as f32,
        schedule: None,
        buses: Default::default(),
        gains: [0.0; MAX_SONG_PARTS],
        voices: [Voice::IDLE; MAX_SONG_VOICES],
        playing: false,
        pos: 0,
        cursor: 0,
        last_beat: i64::MIN,
        seed: 0x1234_5678,
    };
    (control, player)
}

fn busy() -> EngineError {
    EngineError::Busy("song command queue is full".into())
}

impl SongControl {
    fn send(&mut self, msg: Msg) -> Result<(), EngineError> {
        self.tx.push(msg).map_err(|_| busy())
    }

    /// Replace the song (safe while playing: position, voices and effect tails continue).
    pub fn set(&mut self, spec: &SongSpec) -> Result<(), EngineError> {
        spec.validate()?;
        self.collect();
        let sr = self.sample_rate as f64;
        let fpb = sr * 60.0 / spec.bpm;
        let loop_frames = (spec.loop_beats * fpb).round().max(1.0) as u64;
        // Keep each part on its bus; free buses of parts that are gone.
        let ids: Vec<u32> = spec.parts.iter().map(|p| p.id).collect();
        self.slots.retain(|id, _| ids.contains(id));
        let mut gains = [0.0f32; MAX_SONG_PARTS];
        let mut used = [false; MAX_SONG_PARTS];
        let mut events = Vec::with_capacity(spec.parts.iter().map(|p| p.events.len()).sum());
        let mut chains = Vec::new();
        for part in &spec.parts {
            let slot = match self.slots.get(&part.id) {
                Some(&(slot, _)) => slot,
                None => {
                    let taken: Vec<u8> = self.slots.values().map(|(s, _)| *s).collect();
                    let slot = (0..MAX_SONG_PARTS as u8).find(|s| !taken.contains(s)).ok_or_else(|| EngineError::Invalid("song: too many parts".into()))?;
                    self.slots.insert(part.id, (slot, String::from("\u{0}")));
                    slot
                }
            };
            let entry = self.slots.get_mut(&part.id).expect("inserted above");
            if entry.1 != part.fx {
                entry.1 = part.fx.clone();
                chains.push((slot, Box::new(ChainDsp::new(&fx_stack(&part.fx), self.sample_rate, spec.bpm))));
            }
            gains[slot as usize] = part.gain.clamp(0.0, 2.0);
            used[slot as usize] = true;
            for e in &part.events {
                if !(e.beat.is_finite() && e.beat >= 0.0) {
                    continue;
                }
                let frame = ((e.beat % spec.loop_beats) * fpb).round() as u64 % loop_frames;
                let velocity = if e.velocity.is_finite() { e.velocity.clamp(0.0, 1.0) } else { 1.0 };
                let start = match (&part.sound, e.lane, e.midi) {
                    (PartSound::Drums { kit }, Some(lane), _) => VoiceStart::Drum { kit: kit_index(kit), lane, velocity },
                    (PartSound::Melody { instrument }, _, Some(midi)) => VoiceStart::Note {
                        voice: voice_index(instrument),
                        midi: midi.min(127),
                        velocity,
                        frames: ((e.length_beats.max(0.05)) * fpb).round().min(u32::MAX as f64) as u32,
                    },
                    _ => continue,
                };
                events.push(Scheduled { frame, slot, start, skip_pass: e.skip_pass.unwrap_or(i64::MIN) });
            }
        }
        events.sort_by_key(|e| (e.frame, e.slot));
        for (slot, mut chain) in chains {
            chain.set_tempo(spec.bpm);
            self.send(Msg::Bus { slot, chain })?;
        }
        let schedule = SongSchedule {
            loop_frames,
            frames_per_beat: fpb,
            beats_per_bar: spec.beats_per_bar,
            click: spec.click,
            click_gain: spec.click_gain.clamp(0.0, 1.0),
            master_gain: spec.master_gain.clamp(0.0, 1.5) * 10f32.powf(spec.loudness_db.clamp(0.0, 12.0) / 20.0),
            gains,
            used,
            events,
        };
        self.send(Msg::Schedule(Box::new(schedule)))?;
        self.spec = Some(spec.clone());
        Ok(())
    }

    pub fn spec(&self) -> Option<&SongSpec> {
        self.spec.as_ref()
    }

    pub fn play(&mut self, count_in_bars: f64) -> Result<(), EngineError> {
        let spec = self.spec.as_ref().ok_or_else(|| EngineError::Invalid("song: set a song before playing".into()))?;
        let fpb = self.sample_rate as f64 * 60.0 / spec.bpm;
        let beats = (count_in_bars.clamp(0.0, 4.0) * spec.beats_per_bar as f64).round();
        self.shared.origin.store(u64::MAX, Ordering::Relaxed);
        self.send(Msg::Play { count_in_frames: (beats * fpb).round() as u64 })
    }

    pub fn stop(&mut self) -> Result<(), EngineError> {
        self.shared.origin.store(u64::MAX, Ordering::Relaxed);
        self.send(Msg::Stop)
    }

    pub fn trigger(&mut self, t: &SongTrigger) -> Result<(), EngineError> {
        let spec = self.spec.as_ref().ok_or_else(|| EngineError::Invalid("song: set a song first".into()))?;
        let part = spec.parts.iter().find(|p| p.id == t.part_id).ok_or_else(|| EngineError::Invalid("song: no such part".into()))?;
        let slot = self.slots.get(&t.part_id).map(|s| s.0).unwrap_or(0);
        let fpb = self.sample_rate as f64 * 60.0 / spec.bpm;
        let velocity = t.velocity.clamp(0.0, 1.0);
        let start = match (&part.sound, t.lane, t.midi) {
            (PartSound::Drums { kit }, Some(lane), _) => VoiceStart::Drum { kit: kit_index(kit), lane, velocity },
            (PartSound::Melody { instrument }, _, Some(midi)) => VoiceStart::Note {
                voice: voice_index(instrument),
                midi: midi.min(127),
                velocity,
                frames: if t.hold { HELD } else { ((t.length_beats.max(0.05) * fpb).round() as u32).min(HELD - 1) },
            },
            _ => return Err(EngineError::Invalid("song: a drum part needs `lane`, a melody part `midi`".into())),
        };
        self.send(Msg::Trigger { slot, start })
    }

    pub fn release(&mut self, r: &SongRelease) -> Result<(), EngineError> {
        let slot = self.slots.get(&r.part_id).map(|s| s.0).ok_or_else(|| EngineError::Invalid("song: no such part".into()))?;
        self.send(Msg::Release { slot, midi: r.midi.map(|m| m.min(127)).unwrap_or(u8::MAX) })
    }

    /// Drop finished schedules and chains (control thread).
    pub fn collect(&mut self) {
        while self.garbage.pop().is_ok() {}
    }

    pub fn position(&self) -> SongPosition {
        let fpb = f64::from_bits(self.shared.frames_per_beat.load(Ordering::Relaxed));
        let loop_frames = self.shared.loop_frames.load(Ordering::Relaxed);
        let pos = self.shared.position.load(Ordering::Relaxed);
        let origin = self.shared.origin.load(Ordering::Relaxed);
        let (beat, pass) = if fpb > 0.0 && loop_frames > 0 && pos >= 0 {
            let pass = (pos as u64 / loop_frames) as i64;
            ((pos as u64 % loop_frames) as f64 / fpb, pass)
        } else if fpb > 0.0 {
            (pos as f64 / fpb, -1)
        } else {
            (0.0, -1)
        };
        SongPosition { playing: self.shared.playing.load(Ordering::Relaxed), beat, pass, output_frame: (origin != u64::MAX).then_some(origin) }
    }

    /// The beat the performer heard when producing input frame `input_frame`
    /// (or, with `None`, the beat being heard right now: a touch). Needs the
    /// song playing and both clocks' anchors.
    pub fn locate(&self, input_frame: Option<u64>, clocks: &LocateClocks) -> Option<SongLocation> {
        let origin = self.shared.origin.load(Ordering::Relaxed);
        if origin == u64::MAX || !self.shared.playing.load(Ordering::Relaxed) {
            return self.locate_count_in(input_frame, clocks);
        }
        let out = clocks.heard_output_frame(input_frame)?;
        Some(self.location(out.0 - origin as f64, out.1))
    }

    /// During the count-in the origin is not published yet: derive it from the position.
    fn locate_count_in(&self, input_frame: Option<u64>, clocks: &LocateClocks) -> Option<SongLocation> {
        if !self.shared.playing.load(Ordering::Relaxed) {
            return None;
        }
        let pos = self.shared.position.load(Ordering::Relaxed);
        let anchor = clocks.output?;
        // The position was stored at the end of the block that ended at anchor.frame.
        let origin = anchor.frame as f64 - pos as f64;
        let out = clocks.heard_output_frame(input_frame)?;
        Some(self.location(out.0 - origin, out.1))
    }

    fn location(&self, frames: f64, latency_ms: f64) -> SongLocation {
        let fpb = f64::from_bits(self.shared.frames_per_beat.load(Ordering::Relaxed)).max(1.0);
        let loop_beats = self.shared.loop_frames.load(Ordering::Relaxed) as f64 / fpb;
        let absolute_beat = frames / fpb;
        let pass = if loop_beats > 0.0 { (absolute_beat / loop_beats).floor() as i64 } else { 0 };
        let loop_beat = if loop_beats > 0.0 { absolute_beat.rem_euclid(loop_beats) } else { 0.0 };
        SongLocation { absolute_beat, loop_beat, pass, latency_ms }
    }
}

/// The clock facts `locate` needs, read on the control thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct LocateClocks {
    pub output: Option<Anchor>,
    pub output_rate: u32,
    pub output_latency_frames: f64,
    pub input: Option<Anchor>,
    pub input_rate: u32,
    pub input_latency_frames: f64,
    /// Extra latency to remove (ms), from the app's timing-offset settings.
    pub extra_ms: f64,
    pub now_ns: u64,
}

impl LocateClocks {
    pub fn read(output: Option<(&StreamStats, u32)>, input: Option<(&StreamStats, u32)>, extra_ms: f64, now_ns: u64) -> Self {
        let mut c = LocateClocks { extra_ms, now_ns, ..Default::default() };
        if let Some((s, rate)) = output {
            c.output = s.anchor.read();
            c.output_rate = rate;
            c.output_latency_frames = s.reported_latency_frames(rate).unwrap_or(0) as f64;
        }
        if let Some((s, rate)) = input {
            c.input = s.anchor.read();
            c.input_rate = rate;
            c.input_latency_frames = s.reported_latency_frames(rate).unwrap_or(0) as f64;
        }
        c
    }

    /// Output frame heard at the moment input frame `x` was performed (or now),
    /// and the latency removed in ms.
    fn heard_output_frame(&self, input_frame: Option<u64>) -> Option<(f64, f64)> {
        let o = self.output?;
        let or = self.output_rate.max(1) as f64;
        let out_lat = self.output_latency_frames;
        let extra = self.extra_ms * or / 1_000.0;
        match input_frame {
            None => {
                let produced = o.frame as f64 + (self.now_ns as f64 - o.host_ns as f64) * or / 1e9;
                Some((produced - out_lat - extra, (out_lat + extra) * 1_000.0 / or))
            }
            Some(x) => {
                let i = self.input?;
                let ir = self.input_rate.max(1) as f64;
                // Host time at which the input callback held frame x, minus the input latency:
                // when it was actually performed.
                let in_lat_ns = self.input_latency_frames * 1e9 / ir;
                let host = i.host_ns as f64 + (x as f64 - i.frame as f64) * 1e9 / ir - in_lat_ns;
                let produced = o.frame as f64 + (host - o.host_ns as f64) * or / 1e9;
                let total_ms = in_lat_ns / 1e6 + (out_lat + extra) * 1_000.0 / or;
                Some((produced - out_lat - extra, total_ms))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    fn spec(events: Vec<SongEvent>) -> SongSpec {
        SongSpec {
            bpm: 120.0,
            beats_per_bar: 4,
            loop_beats: 4.0,
            click: false,
            click_gain: 0.45,
            master_gain: 1.0,
            loudness_db: 0.0,
            parts: vec![SongPart { id: 7, sound: PartSound::Drums { kit: "studio".into() }, gain: 1.0, fx: "clean".into(), events }],
        }
    }

    fn hit(beat: f64, lane: DrumLane) -> SongEvent {
        SongEvent { beat, velocity: 1.0, lane: Some(lane), midi: None, length_beats: 0.0, skip_pass: None }
    }

    /// Render `frames` in blocks; returns the output.
    fn render(player: &mut SongPlayer, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames];
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(256);
            player.process(&mut out[done..done + n], done as u64);
            done += n;
        }
        out
    }

    fn first_sound(out: &[f32]) -> Option<usize> {
        out.iter().position(|v| v.abs() > 1e-3)
    }

    #[test]
    fn plays_events_at_their_beat_and_loops() {
        let (mut c, mut p) = song_pair(SR);
        c.set(&spec(vec![hit(1.0, DrumLane::Kick)])).unwrap();
        c.play(0.0).unwrap();
        let out = render(&mut p, SR as usize * 3);
        // Beat 1 at 120 BPM = 0.5 s; the loop is 2 s, so again at 2.5 s.
        assert_eq!(first_sound(&out), Some(SR as usize / 2));
        let second = first_sound(&out[(SR as usize)..]).map(|i| i + SR as usize);
        assert_eq!(second, Some(SR as usize * 5 / 2));
        let pos = c.position();
        assert!(pos.playing);
        assert_eq!(pos.pass, 1);
        assert_eq!(pos.output_frame, Some(0));
    }

    #[test]
    fn count_in_clicks_then_the_song_starts_after_it() {
        let (mut c, mut p) = song_pair(SR);
        let mut s = spec(vec![hit(0.0, DrumLane::Snare)]);
        s.click = false;
        c.set(&s).unwrap();
        c.play(1.0).unwrap();
        let out = render(&mut p, SR as usize * 3);
        // The count-in clicks at once (4 beats = 2 s) even with the click off.
        assert_eq!(first_sound(&out), Some(0));
        assert_eq!(c.position().output_frame, Some(SR as u64 * 2));
        // Between the last count-in click's tail and beat 0, silence; then the snare.
        assert!(out[(SR as usize * 2 - 2_000)..(SR as usize * 2)].iter().all(|v| v.abs() < 1e-3));
        assert!(out[SR as usize * 2..SR as usize * 2 + 200].iter().any(|v| v.abs() > 1e-3));
    }

    #[test]
    fn skip_pass_silences_one_pass_only() {
        let (mut c, mut p) = song_pair(SR);
        let mut e = hit(0.5, DrumLane::Hat);
        e.skip_pass = Some(0);
        c.set(&spec(vec![e])).unwrap();
        c.play(0.0).unwrap();
        let out = render(&mut p, SR as usize * 3);
        assert_eq!(first_sound(&out), Some(SR as usize * 2 + SR as usize / 4));
    }

    #[test]
    fn replacing_the_song_keeps_position_and_tails() {
        let (mut c, mut p) = song_pair(SR);
        c.set(&spec(vec![hit(0.0, DrumLane::Kick)])).unwrap();
        c.play(0.0).unwrap();
        render(&mut p, SR as usize / 2);
        let before = c.position().beat;
        c.set(&spec(vec![hit(0.0, DrumLane::Kick), hit(2.0, DrumLane::Snare)])).unwrap();
        render(&mut p, 256);
        let after = c.position().beat;
        assert!((after - before - 256.0 / 24_000.0).abs() < 0.01, "{before} → {after}");
        c.collect();
    }

    #[test]
    fn every_kit_lane_and_preset_makes_finite_sound() {
        for kit in DRUM_KITS {
            for (fx, _, _) in FX_PRESETS {
                let (mut c, mut p) = song_pair(SR);
                let mut s = spec(vec![]);
                s.parts[0].sound = PartSound::Drums { kit: kit.id.into() };
                s.parts[0].fx = fx.into();
                c.set(&s).unwrap();
                for lane in [DrumLane::Kick, DrumLane::Snare, DrumLane::Hat, DrumLane::Clap, DrumLane::OpenHat] {
                    c.trigger(&SongTrigger { part_id: 7, lane: Some(lane), midi: None, velocity: 1.0, length_beats: 0.0, hold: false }).unwrap();
                }
                let out = render(&mut p, SR as usize / 2);
                assert!(out.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{} {fx}", kit.id);
                assert!(out.iter().any(|v| v.abs() > 0.01), "{} {fx} silent", kit.id);
            }
        }
        for voice in MELODY_VOICES {
            let (mut c, mut p) = song_pair(SR);
            let mut s = spec(vec![]);
            s.parts[0].sound = PartSound::Melody { instrument: voice.id.into() };
            c.set(&s).unwrap();
            c.trigger(&SongTrigger { part_id: 7, lane: None, midi: Some(60), velocity: 0.8, length_beats: 1.0, hold: false }).unwrap();
            let out = render(&mut p, SR as usize);
            assert!(out.iter().any(|v| v.abs() > 0.01), "{} silent", voice.id);
            assert!(out[SR as usize - 100..].iter().all(|v| v.abs() < 1e-3), "{} did not release", voice.id);
        }
    }

    #[test]
    fn held_notes_sustain_until_released() {
        let (mut c, mut p) = song_pair(SR);
        let mut s = spec(vec![]);
        s.parts[0].sound = PartSound::Melody { instrument: "softSynth".into() };
        c.set(&s).unwrap();
        c.trigger(&SongTrigger { part_id: 7, lane: None, midi: Some(64), velocity: 0.8, length_beats: 0.0, hold: true }).unwrap();
        let held = render(&mut p, SR as usize * 3);
        let tail = &held[held.len() - 2_000..];
        assert!(tail.iter().any(|v| v.abs() > 0.01), "a held note keeps sounding after 3 s");
        c.release(&SongRelease { part_id: 7, midi: Some(64) }).unwrap();
        let out = render(&mut p, SR as usize);
        assert!(out[out.len() - 100..].iter().all(|v| v.abs() < 1e-3), "released note fades out");
        assert!(c.release(&SongRelease { part_id: 99, midi: None }).is_err());
    }

    #[test]
    fn fx_stack_chains_presets_in_order_with_unique_ids() {
        let chain = fx_stack("room+punch");
        let ids: Vec<&str> = chain.effects().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["0rv", "1cp", "1st"]);
        assert_eq!(fx_stack("room+room+room+room").effects().len(), MAX_FX_PER_PART);
        assert!(fx_stack("").effects().is_empty());
        assert_eq!(fx_stack("punch").effects().len(), fx_preset("punch").effects().len());
    }

    #[test]
    fn locate_maps_input_and_now_to_heard_beats() {
        let (mut c, mut p) = song_pair(SR);
        c.set(&spec(vec![])).unwrap();
        c.play(0.0).unwrap();
        render(&mut p, SR as usize); // 2 beats in; origin = output frame 0
        let anchor = |frame: u64, host_ns: u64| Anchor { host_ns, frame, ..Default::default() };
        // Output: frame 48 000 at host 1 s. Input: frame 10 000 at host 1 s (it started later).
        let clocks = LocateClocks {
            output: Some(anchor(48_000, 1_000_000_000)),
            output_rate: SR,
            output_latency_frames: 480.0, // 10 ms
            input: Some(anchor(10_000, 1_000_000_000)),
            input_rate: SR,
            input_latency_frames: 480.0, // 10 ms
            extra_ms: 0.0,
            now_ns: 1_000_000_000,
        };
        // Now: produced frame 48 000, heard 47 520 → beat 1.98.
        let now = c.locate(None, &clocks).unwrap();
        assert!((now.absolute_beat - 47_520.0 / 24_000.0).abs() < 1e-6, "{now:?}");
        // Input frame 10 000 was received at 1 s, performed 10 ms earlier, when output
        // 47 520 was produced and 47 040 heard.
        let mic = c.locate(Some(10_000), &clocks).unwrap();
        assert!((mic.absolute_beat - 47_040.0 / 24_000.0).abs() < 1e-6, "{mic:?}");
        assert!((mic.latency_ms - 20.0).abs() < 1e-9);
        c.stop().unwrap();
        render(&mut p, 256);
        assert!(c.locate(None, &clocks).is_none());
    }

    #[test]
    fn rejects_bad_songs() {
        let (mut c, _p) = song_pair(SR);
        let mut s = spec(vec![]);
        s.bpm = 0.0;
        assert!(c.set(&s).is_err());
        let mut s = spec(vec![]);
        s.parts = (0..9).map(|i| SongPart { id: i, sound: PartSound::Drums { kit: "808".into() }, gain: 1.0, fx: String::new(), events: vec![] }).collect();
        assert!(c.set(&s).is_err());
        assert!(c.play(1.0).is_err());
    }
}
