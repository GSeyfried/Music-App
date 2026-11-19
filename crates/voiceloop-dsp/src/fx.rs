//! The effect processors (mono, per sample, realtime-safe).
//!
//! Every buffer is allocated in `new` and sized for the largest parameter
//! values at the given sample rate; `process`, `update` and `reset` never
//! allocate. Recursive state is flushed to zero below [`DENORMAL`] so long
//! silences never produce subnormal numbers.
//!
//! All algorithms are written here from textbook descriptions: RBJ "Audio EQ
//! Cookbook" biquad formulas, a topology-preserving state-variable filter, a
//! feedback-delay-network reverb with a Householder matrix, feed-forward
//! compressor/gate gain computers. No code is taken from other projects.

use std::f64::consts::PI;

use crate::spec::{sync_beats, EffectKind, AUTO_FILTER_SYNC, DELAY_SYNC, FILTER_TYPES, TREMOLO_SYNC};

/// Recursive state below this magnitude is set to zero (denormal protection).
pub const DENORMAL: f32 = 1.0e-20;
/// Largest delay time (ms) the delay effect can reach, synced or not.
pub const DELAY_MAX_MS: f32 = 2_000.0;
/// Envelope level (linear) that sweeps the auto-filter over its full depth.
pub const AUTO_FILTER_ENV_FULL_SCALE: f32 = 0.25;
/// Phaser sweep span at depth 1, in octaves each side of the center.
pub const PHASER_SPAN_OCT: f32 = 2.0;
/// Reverb wet output gain.
pub const REVERB_OUTPUT_GAIN: f32 = 0.35;

#[inline]
pub fn flush(x: f32) -> f32 {
    if x.abs() < DENORMAL {
        0.0
    } else {
        x
    }
}

#[inline]
fn flush64(x: f64) -> f64 {
    if x.abs() < DENORMAL as f64 {
        0.0
    } else {
        x
    }
}

#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

#[inline]
pub fn lin_to_db(x: f32) -> f32 {
    20.0 * x.max(1.0e-10).log10()
}

/// One-pole coefficient for a time constant of `ms` at `sr`.
#[inline]
fn coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms.max(0.001) * 0.001 * sr)).exp()
}

/// Linear ramp toward a target over a fixed number of frames.
#[derive(Debug, Clone, Copy)]
pub struct Ramp {
    pub cur: f32,
    pub target: f32,
    step: f32,
    frames: f32,
}

impl Ramp {
    pub fn new(value: f32, frames: u32) -> Self {
        Self { cur: value, target: value, step: 0.0, frames: frames.max(1) as f32 }
    }

    pub fn set(&mut self, target: f32) {
        self.target = target;
        self.step = (target - self.cur).abs() / self.frames;
    }

    pub fn snap(&mut self, value: f32) {
        self.cur = value;
        self.target = value;
        self.step = 0.0;
    }

    #[inline]
    pub fn settled(&self) -> bool {
        self.cur == self.target
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        if self.cur != self.target {
            let d = self.target - self.cur;
            if d.abs() <= self.step || self.step == 0.0 {
                self.cur = self.target;
            } else {
                self.cur += self.step.copysign(d);
            }
        }
        self.cur
    }
}

// ---- building blocks ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BiquadType {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
    LowShelf,
    HighShelf,
    Peak,
}

impl BiquadType {
    fn from_index(i: usize) -> Self {
        match FILTER_TYPES.get(i).copied().unwrap_or("lowpass") {
            "highpass" => Self::Highpass,
            "bandpass" => Self::Bandpass,
            "notch" => Self::Notch,
            "lowshelf" => Self::LowShelf,
            "highshelf" => Self::HighShelf,
            "peak" => Self::Peak,
            _ => Self::Lowpass,
        }
    }
}

/// Second-order section, transposed direct form II, f64 state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    /// RBJ cookbook design. `f0` is clamped below Nyquist; shelves use `q` as their slope Q.
    pub fn design(&mut self, t: BiquadType, f0: f32, q: f32, gain_db: f32, sr: f32) {
        let f0 = (f0 as f64).clamp(1.0, 0.49 * sr as f64);
        let w0 = 2.0 * PI * f0 / sr as f64;
        let (sin, cos) = w0.sin_cos();
        let q = (q as f64).max(0.01);
        let alpha = sin / (2.0 * q);
        let a = 10f64.powf(gain_db as f64 / 40.0);
        let sa = 2.0 * a.sqrt() * alpha;
        let (b0, b1, b2, a0, a1, a2) = match t {
            BiquadType::Lowpass => {
                ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            BiquadType::Highpass => {
                ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha)
            }
            BiquadType::Bandpass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadType::Notch => (1.0, -2.0 * cos, 1.0, 1.0 + alpha, -2.0 * cos, 1.0 - alpha),
            BiquadType::Peak => {
                (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a)
            }
            BiquadType::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cos + sa),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                a * ((a + 1.0) - (a - 1.0) * cos - sa),
                (a + 1.0) + (a - 1.0) * cos + sa,
                -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                (a + 1.0) + (a - 1.0) * cos - sa,
            ),
            BiquadType::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cos + sa),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                a * ((a + 1.0) + (a - 1.0) * cos - sa),
                (a + 1.0) - (a - 1.0) * cos + sa,
                2.0 * ((a - 1.0) - (a + 1.0) * cos),
                (a + 1.0) - (a - 1.0) * cos - sa,
            ),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let x = x as f64;
        let y = self.b0 * x + self.z1;
        self.z1 = flush64(self.b1 * x - self.a1 * y + self.z2);
        self.z2 = flush64(self.b2 * x - self.a2 * y);
        y as f32
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// One-pole lowpass `y += a (x − y)`.
#[derive(Debug, Clone, Copy, Default)]
struct OnePole {
    a: f32,
    y: f32,
}

impl OnePole {
    fn set_hz(&mut self, hz: f32, sr: f32) {
        let hz = hz.clamp(1.0, 0.49 * sr);
        self.a = 1.0 - (-2.0 * std::f32::consts::PI * hz / sr).exp();
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.y = flush(self.y + self.a * (x - self.y));
        self.y
    }
}

/// Circular delay line read with linear interpolation.
#[derive(Debug, Clone)]
struct Line {
    buf: Box<[f32]>,
    w: usize,
}

impl Line {
    fn new(frames: usize) -> Self {
        Self { buf: vec![0.0; frames.max(4)].into_boxed_slice(), w: 0 }
    }
    #[inline]
    fn at(&self, k: usize) -> f32 {
        let n = self.buf.len();
        self.buf[(self.w + n - (k % n)) % n]
    }
    /// The sample written `d` frames ago (`d` ≥ 1; clamped to the line).
    #[inline]
    fn read(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (self.buf.len() - 2) as f32);
        let i = d.floor();
        let frac = d - i;
        let a = self.at(i as usize);
        if frac == 0.0 {
            return a;
        }
        let b = self.at(i as usize + 1);
        a + (b - a) * frac
    }
    #[inline]
    fn push(&mut self, x: f32) {
        self.buf[self.w] = flush(x);
        self.w = (self.w + 1) % self.buf.len();
    }
    fn reset(&mut self) {
        self.buf.fill(0.0);
        self.w = 0;
    }
}

/// Sine/triangle/square LFO with a phase in cycles.
#[derive(Debug, Clone, Copy, Default)]
struct Lfo {
    phase: f64,
    inc: f64,
}

impl Lfo {
    fn set(&mut self, hz: f64, sr: f32) {
        self.inc = hz / sr as f64;
    }
    /// Advance and return the phase in `[0, 1)`.
    #[inline]
    fn tick(&mut self) -> f64 {
        let p = self.phase;
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        }
        p
    }
    #[inline]
    fn sine(&mut self) -> f32 {
        (self.tick() * 2.0 * PI).sin() as f32
    }
}

/// LFO rate: the sync option when set (one cycle per note value), else `hz`.
fn lfo_rate(options: &[&str], sync_index: f32, hz: f32, bpm: f64) -> f64 {
    match options.get(sync_index as usize).and_then(|o| sync_beats(o)) {
        Some(beats) if bpm > 0.0 => bpm / 60.0 / beats,
        _ => hz as f64,
    }
}

// ---- effects ---------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct GainFx {
    g: Ramp,
}

#[derive(Debug, Clone)]
pub struct FilterFx {
    bq: Biquad,
}

#[derive(Debug, Clone)]
pub struct Eq3Fx {
    low: Biquad,
    mid: Biquad,
    high: Biquad,
}

#[derive(Debug, Clone)]
pub struct GateFx {
    env: f32,
    det_rel: f32,
    gain: f32,
    hold_left: u32,
    hold: u32,
    threshold: f32,
    floor: f32,
    att: f32,
    rel: f32,
}

#[derive(Debug, Clone)]
pub struct CompressorFx {
    gr_db: f32,
    att: f32,
    rel: f32,
    makeup: Ramp,
}

#[derive(Debug, Clone)]
pub struct DelayFx {
    line: Line,
    time: Ramp,
    tone: OnePole,
}

const REVERB_LINES: usize = 8;
/// Base line lengths (ms) at size 0.5; mutually prime-ish so modes spread.
const REVERB_BASE_MS: [f32; REVERB_LINES] = [31.3, 37.9, 41.3, 45.7, 49.9, 53.3, 59.5, 66.7];
const REVERB_DIFFUSER_MS: [f32; 2] = [4.7, 3.6];
const REVERB_DIFFUSION: f32 = 0.6;

#[derive(Debug, Clone)]
pub struct ReverbFx {
    pre: Line,
    pre_ms: Ramp,
    diff: [Line; 2],
    diff_len: [f32; 2],
    lines: [Line; REVERB_LINES],
    len: [Ramp; REVERB_LINES],
    gain: [f32; REVERB_LINES],
    damp: [f32; REVERB_LINES],
    damping: f32,
}

#[derive(Debug, Clone)]
pub struct SaturationFx {
    drive: Ramp,
    out: Ramp,
    tone: OnePole,
    dc_x: f32,
    dc_y: f32,
    dc_r: f32,
}

#[derive(Debug, Clone)]
pub struct BitcrusherFx {
    held: f32,
    count: u32,
}

#[derive(Debug, Clone)]
pub struct AutoFilterFx {
    lfo: Lfo,
    env: f32,
    env_att: f32,
    env_rel: f32,
    ic1: f64,
    ic2: f64,
}

#[derive(Debug, Clone)]
pub struct ModDelayFx {
    line: Line,
    lfo: Lfo,
    last: f32,
}

#[derive(Debug, Clone)]
pub struct PhaserFx {
    lfo: Lfo,
    z: [f32; 12],
    last: f32,
}

#[derive(Debug, Clone)]
pub struct TremoloFx {
    lfo: Lfo,
    g: f32,
    slew: f32,
}

#[derive(Debug, Clone)]
pub struct RingModFx {
    osc: Lfo,
}

/// One effect's processor; the variant matches its [`EffectKind`]. Sizes
/// differ (the reverb holds its lines inline); each lives in a heap `Vec`.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Fx {
    Gain(GainFx),
    Filter(FilterFx),
    Eq3(Eq3Fx),
    Gate(GateFx),
    Compressor(CompressorFx),
    Delay(DelayFx),
    Reverb(ReverbFx),
    Saturation(SaturationFx),
    Bitcrusher(BitcrusherFx),
    AutoFilter(AutoFilterFx),
    Chorus(ModDelayFx),
    Flanger(ModDelayFx),
    Phaser(PhaserFx),
    Tremolo(TremoloFx),
    RingMod(RingModFx),
}

/// Frames between coefficient recalculations while params move.
pub const CONTROL_FRAMES: u32 = 16;

impl Fx {
    /// Allocate a processor for `kind` at `sr`, with every buffer at its maximum size.
    pub fn new(kind: EffectKind, sr: f32) -> Self {
        let ms = |ms: f32| (ms * 0.001 * sr).ceil() as usize + 4;
        match kind {
            EffectKind::Gain => Fx::Gain(GainFx { g: Ramp::new(1.0, CONTROL_FRAMES) }),
            EffectKind::Filter => Fx::Filter(FilterFx { bq: Biquad::default() }),
            EffectKind::Eq3 => {
                Fx::Eq3(Eq3Fx { low: Biquad::default(), mid: Biquad::default(), high: Biquad::default() })
            }
            EffectKind::Gate => Fx::Gate(GateFx {
                env: 0.0,
                det_rel: coef(5.0, sr),
                gain: 0.0,
                hold_left: 0,
                hold: 0,
                threshold: 0.0,
                floor: 0.0,
                att: 0.0,
                rel: 0.0,
            }),
            EffectKind::Compressor => {
                Fx::Compressor(CompressorFx { gr_db: 0.0, att: 0.0, rel: 0.0, makeup: Ramp::new(1.0, CONTROL_FRAMES) })
            }
            EffectKind::Delay => Fx::Delay(DelayFx {
                line: Line::new(ms(DELAY_MAX_MS)),
                time: Ramp::new(0.0, (0.02 * sr) as u32),
                tone: OnePole::default(),
            }),
            EffectKind::Reverb => {
                // Longest line: base × 1.5 (size 1), plus interpolation room.
                let lines = std::array::from_fn(|i| Line::new(ms(REVERB_BASE_MS[i] * 1.5)));
                Fx::Reverb(ReverbFx {
                    pre: Line::new(ms(200.0)),
                    pre_ms: Ramp::new(0.0, (0.05 * sr) as u32),
                    diff: [Line::new(ms(REVERB_DIFFUSER_MS[0])), Line::new(ms(REVERB_DIFFUSER_MS[1]))],
                    diff_len: [REVERB_DIFFUSER_MS[0] * 0.001 * sr, REVERB_DIFFUSER_MS[1] * 0.001 * sr],
                    lines,
                    len: [Ramp::new(1.0, (0.05 * sr) as u32); REVERB_LINES],
                    gain: [0.0; REVERB_LINES],
                    damp: [0.0; REVERB_LINES],
                    damping: 0.0,
                })
            }
            EffectKind::Saturation => Fx::Saturation(SaturationFx {
                drive: Ramp::new(1.0, CONTROL_FRAMES),
                out: Ramp::new(1.0, CONTROL_FRAMES),
                tone: OnePole::default(),
                dc_x: 0.0,
                dc_y: 0.0,
                dc_r: 1.0 - 2.0 * std::f32::consts::PI * 10.0 / sr,
            }),
            EffectKind::Bitcrusher => Fx::Bitcrusher(BitcrusherFx { held: 0.0, count: 0 }),
            EffectKind::AutoFilter => Fx::AutoFilter(AutoFilterFx {
                lfo: Lfo::default(),
                env: 0.0,
                env_att: 0.0,
                env_rel: 0.0,
                ic1: 0.0,
                ic2: 0.0,
            }),
            EffectKind::Chorus => Fx::Chorus(ModDelayFx { line: Line::new(ms(60.0)), lfo: Lfo::default(), last: 0.0 }),
            EffectKind::Flanger => {
                Fx::Flanger(ModDelayFx { line: Line::new(ms(20.0)), lfo: Lfo::default(), last: 0.0 })
            }
            EffectKind::Phaser => Fx::Phaser(PhaserFx { lfo: Lfo::default(), z: [0.0; 12], last: 0.0 }),
            EffectKind::Tremolo => {
                Fx::Tremolo(TremoloFx { lfo: Lfo::default(), g: 1.0, slew: 1.0 / (0.002 * sr).max(1.0) })
            }
            EffectKind::RingMod => Fx::RingMod(RingModFx { osc: Lfo::default() }),
        }
    }

    /// Recompute coefficients from (smoothed) params `p`. `first`: jump
    /// internal ramps instead of gliding (construction, reset).
    pub fn update(&mut self, p: &[f32], sr: f32, bpm: f64, first: bool) {
        let set = |r: &mut Ramp, v: f32| if first { r.snap(v) } else { r.set(v) };
        match self {
            Fx::Gain(f) => set(&mut f.g, db_to_lin(p[0])),
            Fx::Filter(f) => f.bq.design(BiquadType::from_index(p[0] as usize), p[1], p[2], p[3], sr),
            Fx::Eq3(f) => {
                f.low.design(BiquadType::LowShelf, p[1], 0.707, p[0], sr);
                f.mid.design(BiquadType::Peak, p[3], p[4], p[2], sr);
                f.high.design(BiquadType::HighShelf, p[6], 0.707, p[5], sr);
            }
            Fx::Gate(f) => {
                f.threshold = db_to_lin(p[0]);
                f.floor = db_to_lin(p[1]);
                f.att = coef(p[2], sr);
                f.hold = (p[3] * 0.001 * sr) as u32;
                f.rel = coef(p[4], sr);
                if first {
                    f.gain = f.floor;
                }
            }
            Fx::Compressor(f) => {
                f.att = coef(p[3], sr);
                f.rel = coef(p[4], sr);
                set(&mut f.makeup, db_to_lin(p[5]));
            }
            Fx::Delay(f) => {
                let ms = match DELAY_SYNC.get(p[1] as usize).and_then(|o| sync_beats(o)) {
                    Some(beats) if bpm > 0.0 => (beats * 60_000.0 / bpm) as f32,
                    _ => p[0],
                };
                // Whole frames when the time is (within rounding) a whole number of frames,
                // so a 1 ms echo at 48 kHz lands exactly 48 frames later.
                let frames = ms.clamp(1.0, DELAY_MAX_MS) as f64 * 0.001 * sr as f64;
                let frames = if (frames - frames.round()).abs() < 1.0e-3 { frames.round() } else { frames };
                set(&mut f.time, frames as f32);
                f.tone.set_hz(p[3], sr);
            }
            Fx::Reverb(f) => {
                let scale = 0.5 + p[0]; // 0.5..1.5
                let decay = p[1].max(0.05);
                for i in 0..REVERB_LINES {
                    let frames = REVERB_BASE_MS[i] * scale * 0.001 * sr;
                    set(&mut f.len[i], frames);
                    // −60 dB after `decay` seconds: g^(decay·sr / len) = 10^-3.
                    f.gain[i] = 10f32.powf(-3.0 * frames / (decay * sr));
                }
                f.damping = p[2] * 0.85;
                set(&mut f.pre_ms, p[3] * 0.001 * sr);
            }
            Fx::Saturation(f) => {
                set(&mut f.drive, db_to_lin(p[0]));
                f.tone.set_hz(p[2], sr);
                set(&mut f.out, db_to_lin(p[3]));
            }
            Fx::Bitcrusher(_) => {}
            Fx::AutoFilter(f) => {
                f.lfo.set(lfo_rate(AUTO_FILTER_SYNC, p[5], p[4], bpm), sr);
                f.env_att = coef(p[7], sr);
                f.env_rel = coef(p[8], sr);
            }
            Fx::Chorus(f) | Fx::Flanger(f) => f.lfo.set(p[0] as f64, sr),
            Fx::Phaser(f) => f.lfo.set(p[0] as f64, sr),
            Fx::Tremolo(f) => f.lfo.set(lfo_rate(TREMOLO_SYNC, p[3], p[0], bpm), sr),
            Fx::RingMod(f) => f.osc.set(p[0] as f64, sr),
        }
    }

    /// Clear all audio state (buffers, filter memories, envelopes).
    pub fn reset(&mut self) {
        match self {
            Fx::Gain(_) | Fx::Bitcrusher(_) | Fx::RingMod(_) => {
                if let Fx::Bitcrusher(f) = self {
                    f.held = 0.0;
                    f.count = 0;
                }
            }
            Fx::Filter(f) => f.bq.reset(),
            Fx::Eq3(f) => {
                f.low.reset();
                f.mid.reset();
                f.high.reset();
            }
            Fx::Gate(f) => {
                f.env = 0.0;
                f.gain = f.floor;
                f.hold_left = 0;
            }
            Fx::Compressor(f) => f.gr_db = 0.0,
            Fx::Delay(f) => {
                f.line.reset();
                f.tone.y = 0.0;
            }
            Fx::Reverb(f) => {
                f.pre.reset();
                for l in f.diff.iter_mut().chain(f.lines.iter_mut()) {
                    l.reset();
                }
                f.damp = [0.0; REVERB_LINES];
            }
            Fx::Saturation(f) => {
                f.tone.y = 0.0;
                f.dc_x = 0.0;
                f.dc_y = 0.0;
            }
            Fx::AutoFilter(f) => {
                f.env = 0.0;
                f.ic1 = 0.0;
                f.ic2 = 0.0;
            }
            Fx::Chorus(f) | Fx::Flanger(f) => {
                f.line.reset();
                f.last = 0.0;
            }
            Fx::Phaser(f) => {
                f.z = [0.0; 12];
                f.last = 0.0;
            }
            Fx::Tremolo(_) => {}
        }
    }

    /// Process one sample (wet signal only; the caller mixes dry/wet).
    #[inline]
    pub fn process(&mut self, x: f32, p: &[f32], sr: f32) -> f32 {
        match self {
            Fx::Gain(f) => x * f.g.tick(),
            Fx::Filter(f) => f.bq.process(x),
            Fx::Eq3(f) => f.high.process(f.mid.process(f.low.process(x))),
            Fx::Gate(f) => {
                let a = x.abs();
                f.env = flush(if a > f.env { a } else { f.env * f.det_rel });
                if f.env >= f.threshold {
                    f.hold_left = f.hold.max(1);
                }
                let target = if f.hold_left > 0 {
                    f.hold_left -= 1;
                    1.0
                } else {
                    f.floor
                };
                let c = if target > f.gain { f.att } else { f.rel };
                f.gain = target + c * (f.gain - target);
                x * f.gain
            }
            Fx::Compressor(f) => {
                let (t, r, w) = (p[0], p[1], p[2]);
                let l = lin_to_db(x.abs());
                let over = l - t;
                let g = if 2.0 * over < -w {
                    0.0
                } else if w > 0.0 && 2.0 * over.abs() <= w {
                    (1.0 / r - 1.0) * (over + w / 2.0).powi(2) / (2.0 * w)
                } else {
                    (1.0 / r - 1.0) * over
                };
                let c = if g < f.gr_db { f.att } else { f.rel };
                f.gr_db = g + c * (f.gr_db - g);
                x * db_to_lin(f.gr_db) * f.makeup.tick()
            }
            Fx::Delay(f) => {
                let d = f.time.tick();
                let y = f.line.read(d);
                let fb = f.tone.process(y) * p[2];
                f.line.push(x + fb);
                y
            }
            Fx::Reverb(f) => {
                let pre = f.pre_ms.tick();
                let mut u = if pre >= 1.0 { f.pre.read(pre) } else { x };
                f.pre.push(x);
                for k in 0..2 {
                    // Schroeder allpass: v = u + g·d; out = d − g·v.
                    let d = f.diff[k].read(f.diff_len[k]);
                    let v = u + REVERB_DIFFUSION * d;
                    f.diff[k].push(v);
                    u = d - REVERB_DIFFUSION * v;
                }
                let mut o = [0.0f32; REVERB_LINES];
                let mut sum = 0.0;
                for i in 0..REVERB_LINES {
                    let len = f.len[i].tick();
                    let r = f.lines[i].read(len);
                    f.damp[i] = flush(r + f.damping * (f.damp[i] - r));
                    o[i] = f.damp[i];
                    sum += o[i];
                }
                let h = sum * (2.0 / REVERB_LINES as f32);
                let mut out = 0.0;
                for i in 0..REVERB_LINES {
                    let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
                    f.lines[i].push(u * sign + f.gain[i] * (o[i] - h));
                    out += o[i] * sign;
                }
                out * REVERB_OUTPUT_GAIN
            }
            Fx::Saturation(f) => {
                let u = x * f.drive.tick();
                let y = match p[1] as usize {
                    1 => u.clamp(-1.0, 1.0),
                    2 => {
                        let s = (u + 0.3).tanh() - 0.3f32.tanh();
                        // DC blocker: the asymmetric curve adds an offset.
                        let o = s - f.dc_x + f.dc_r * f.dc_y;
                        f.dc_x = s;
                        f.dc_y = flush(o);
                        o
                    }
                    3 => (u * std::f32::consts::FRAC_PI_2).sin(),
                    _ => u.tanh(),
                };
                f.tone.process(y) * f.out.tick()
            }
            Fx::Bitcrusher(f) => {
                let n = p[1].round().max(1.0) as u32;
                if f.count == 0 {
                    let levels = 2f32.powf(p[0] - 1.0);
                    f.held = (x * levels).round() / levels;
                }
                f.count += 1;
                if f.count >= n {
                    f.count = 0;
                }
                f.held
            }
            Fx::AutoFilter(f) => {
                let a = x.abs();
                let c = if a > f.env { f.env_att } else { f.env_rel };
                f.env = flush(a + c * (f.env - a));
                let m = if p[3] as usize == 1 {
                    (f.env / AUTO_FILTER_ENV_FULL_SCALE).min(1.0)
                } else {
                    (0.5 - 0.5 * (f.lfo.tick() * 2.0 * PI).cos()) as f32
                };
                let fc = (p[1] * 2f32.powf(p[2] * m)).min(0.45 * sr) as f64;
                let g = (PI * fc / sr as f64).tan();
                let k = 1.0 / p[6].max(0.1) as f64;
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let x = x as f64;
                let v3 = x - f.ic2;
                let v1 = a1 * f.ic1 + a2 * v3;
                let v2 = f.ic2 + a2 * f.ic1 + a3 * v3;
                f.ic1 = flush64(2.0 * v1 - f.ic1);
                f.ic2 = flush64(2.0 * v2 - f.ic2);
                (match p[0] as usize {
                    1 => k * v1, // unity-peak bandpass
                    2 => x - k * v1 - v2,
                    _ => v2,
                }) as f32
            }
            Fx::Chorus(f) | Fx::Flanger(f) => {
                let s = 0.5 + 0.5 * f.lfo.sine();
                let d = (p[2] + p[1] * s) * 0.001 * sr;
                let y = f.line.read(d.max(1.0));
                f.line.push(x + p[3] * y);
                f.last = y;
                y
            }
            Fx::Phaser(f) => {
                let stages = (p[1] as usize).min(12);
                let m = f.lfo.sine();
                let fc = (p[4] * 2f32.powf(PHASER_SPAN_OCT * p[2] * m)).clamp(20.0, 0.45 * sr);
                let t = (std::f32::consts::PI * fc / sr).tan();
                let a = (t - 1.0) / (t + 1.0);
                let mut u = x + p[3] * f.last;
                for z in f.z.iter_mut().take(stages) {
                    let y = a * u + *z;
                    *z = flush(u - a * y);
                    u = y;
                }
                f.last = flush(u);
                u
            }
            Fx::Tremolo(f) => {
                let ph = f.lfo.tick();
                let shape = match p[2] as usize {
                    1 => (1.0 - 4.0 * (ph - 0.5).abs()) as f32,
                    2 => {
                        if ph < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                    _ => (ph * 2.0 * PI).sin() as f32,
                };
                let target = 1.0 - p[1] * (0.5 + 0.5 * shape);
                // A short slew keeps the square shape click-free.
                let d = target - f.g;
                f.g += d.clamp(-f.slew, f.slew);
                x * f.g
            }
            Fx::RingMod(f) => x * f.osc.sine(),
        }
    }
}
