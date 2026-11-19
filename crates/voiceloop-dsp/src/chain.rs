//! [`ChainDsp`]: the processors for one [`ChainSpec`].
//!
//! Built (allocated) off the realtime thread; afterwards `apply`, `set_tempo`,
//! `process` and `reset` never allocate, lock or block.
//!
//! - Continuous params glide to new values over [`PARAM_SMOOTH_MS`];
//!   coefficients follow every [`CONTROL_FRAMES`] frames while they move.
//! - Each effect's wet amount (`enabled ? mix : 0`) and the chain's own
//!   bypass ramp over [`BYPASS_RAMP_MS`]. Once a ramp reaches 0 the effect
//!   (or chain) is skipped, so a bypassed effect is bit-exact and costs
//!   nothing; it starts again from cleared state when re-enabled.
//! - A non-finite output resets that effect and yields its input.

use crate::fx::{Fx, Ramp, CONTROL_FRAMES};
use crate::spec::{ChainSpec, EffectSpec, MAX_PARAMS};

/// Glide time of continuous parameter changes.
pub const PARAM_SMOOTH_MS: f32 = 20.0;
/// Ramp time of per-effect bypass/mix and whole-chain bypass changes.
pub const BYPASS_RAMP_MS: f32 = 10.0;

#[derive(Debug, Clone)]
struct Slot {
    fx: Fx,
    params: [Ramp; MAX_PARAMS],
    cur: [f32; MAX_PARAMS],
    n: usize,
    wet: Ramp,
    /// Skipped while fully bypassed; cleared before it runs again.
    idle: bool,
    dirty: bool,
    counter: u32,
}

impl Slot {
    fn new(e: &EffectSpec, sr: f32, bpm: f64) -> Self {
        let n = e.defs().len();
        let glide = (PARAM_SMOOTH_MS * 0.001 * sr) as u32;
        let params = std::array::from_fn(|i| Ramp::new(e.params[i], glide));
        let wet0 = if e.enabled { e.mix } else { 0.0 };
        let mut s = Self {
            fx: Fx::new(e.kind, sr),
            params,
            cur: e.params,
            n,
            wet: Ramp::new(wet0, (BYPASS_RAMP_MS * 0.001 * sr) as u32),
            idle: wet0 == 0.0,
            dirty: false,
            counter: 0,
        };
        s.fx.update(&s.cur[..n], sr, bpm, true);
        s
    }

    fn apply(&mut self, e: &EffectSpec, smooth: bool) {
        for i in 0..self.n {
            if smooth {
                self.params[i].set(e.params[i]);
            } else {
                self.params[i].snap(e.params[i]);
                self.cur[i] = e.params[i];
            }
        }
        let wet = if e.enabled { e.mix } else { 0.0 };
        if smooth {
            self.wet.set(wet);
        } else {
            self.wet.snap(wet);
            self.idle = wet == 0.0;
        }
        self.dirty = true;
    }

    #[inline]
    fn process(&mut self, x: f32, sr: f32, bpm: f64) -> f32 {
        let w = self.wet.tick();
        if w == 0.0 && self.wet.settled() {
            self.idle = true;
            return x;
        }
        if self.idle {
            self.idle = false;
            self.fx.reset();
            self.dirty = true;
        }
        let mut moving = false;
        for i in 0..self.n {
            if !self.params[i].settled() {
                self.cur[i] = self.params[i].tick();
                moving = true;
            }
        }
        if moving {
            self.dirty = true;
        }
        if self.dirty && self.counter == 0 {
            self.fx.update(&self.cur[..self.n], sr, bpm, false);
            self.dirty = false;
        }
        self.counter = (self.counter + 1) % CONTROL_FRAMES;
        let y = self.fx.process(x, &self.cur[..self.n], sr);
        let y = if y.is_finite() {
            y
        } else {
            self.fx.reset();
            x
        };
        if w >= 1.0 {
            y
        } else {
            x + w * (y - x)
        }
    }
}

/// The processors of one chain.
#[derive(Debug, Clone)]
pub struct ChainDsp {
    spec: ChainSpec,
    sr: f32,
    bpm: f64,
    slots: Vec<Slot>,
    on: Ramp,
}

impl ChainDsp {
    /// Allocate everything `spec` needs at `sample_rate`. `bpm` drives synced times.
    pub fn new(spec: &ChainSpec, sample_rate: u32, bpm: f64) -> Self {
        let sr = sample_rate.max(1) as f32;
        let slots = spec.effects().iter().map(|e| Slot::new(e, sr, bpm)).collect();
        let on = Ramp::new(if spec.enabled { 1.0 } else { 0.0 }, (BYPASS_RAMP_MS * 0.001 * sr) as u32);
        Self { spec: *spec, sr, bpm, slots, on }
    }

    pub fn spec(&self) -> &ChainSpec {
        &self.spec
    }

    pub fn sample_rate(&self) -> u32 {
        self.sr as u32
    }

    /// `spec` can be applied in place (see [`ChainSpec::needs_rebuild`]).
    pub fn can_apply(&self, spec: &ChainSpec) -> bool {
        !self.spec.needs_rebuild(spec)
    }

    /// Move to `spec`'s params, bypasses and mixes (glide when `smooth`).
    /// Returns `false` (and changes nothing) when `spec` needs a rebuild.
    pub fn apply(&mut self, spec: &ChainSpec, smooth: bool) -> bool {
        if !self.can_apply(spec) {
            return false;
        }
        for (slot, e) in self.slots.iter_mut().zip(spec.effects()) {
            slot.apply(e, smooth);
        }
        let on = if spec.enabled { 1.0 } else { 0.0 };
        if smooth {
            self.on.set(on);
        } else {
            self.on.snap(on);
        }
        self.spec = *spec;
        true
    }

    /// Song tempo for synced delay times and LFO rates.
    pub fn set_tempo(&mut self, bpm: f64) {
        if bpm != self.bpm && bpm.is_finite() && bpm > 0.0 {
            self.bpm = bpm;
            for s in &mut self.slots {
                s.dirty = true;
            }
        }
    }

    /// Clear all audio state (tails, envelopes, filter memories).
    pub fn reset(&mut self) {
        for s in &mut self.slots {
            s.fx.reset();
        }
    }

    /// The chain is (and stays) a pass-through: no effects, or bypassed and settled.
    #[inline]
    pub fn is_transparent(&self) -> bool {
        self.slots.is_empty() || (self.on.cur == 0.0 && self.on.settled())
    }

    /// Process one sample.
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        if self.slots.is_empty() {
            return x;
        }
        let on = self.on.tick();
        if on == 0.0 && self.on.settled() {
            return x;
        }
        let (sr, bpm) = (self.sr, self.bpm);
        let mut y = x;
        for s in &mut self.slots {
            y = s.process(y, sr, bpm);
        }
        if on >= 1.0 {
            y
        } else {
            x + on * (y - x)
        }
    }

    /// Process a block in place.
    pub fn process_block(&mut self, buf: &mut [f32]) {
        if self.slots.is_empty() {
            return;
        }
        for v in buf.iter_mut() {
            *v = self.process(*v);
        }
    }
}

/// Run `input` through a fresh chain (offline helper).
pub fn render(spec: &ChainSpec, sample_rate: u32, bpm: f64, input: &[f32]) -> Vec<f32> {
    let mut c = ChainDsp::new(spec, sample_rate, bpm);
    let mut out = input.to_vec();
    c.process_block(&mut out);
    out
}
