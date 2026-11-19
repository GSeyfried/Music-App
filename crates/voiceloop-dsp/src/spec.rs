//! The effect-chain contract (docs/projects/effects/README.md, "Contract").
//!
//! [`ChainSpec`] is a fixed-size, `Copy` description of one track's chain, so
//! it can travel inside looper commands and snapshots without allocating. Its
//! JSON form is the camelCase contract shared with the UI:
//!
//! ```json
//! { "enabled": true, "layers": "all",
//!   "effects": [{ "id": "fx1", "kind": "filter", "enabled": true, "mix": 1,
//!                 "params": { "type": "lowpass", "cutoffHz": 1200 } }] }
//! ```
//!
//! Parsing validates and clamps: numeric params are clamped to their range,
//! unknown params are ignored, missing params take the defaults of
//! [`params_of`], choice params accept their option strings. Commands use the
//! strict parser (unknown kinds, too many effects or over-long ids are errors);
//! project files use [`lenient_chain`], which drops what it cannot read.

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

/// Most effects in one chain.
pub const MAX_EFFECTS_PER_TRACK: usize = 8;
/// Most parameters of any kind (autoFilter has 9).
pub const MAX_PARAMS: usize = 9;
/// Longest effect id, in UTF-8 bytes.
pub const MAX_EFFECT_ID_BYTES: usize = 64;
/// Layer indices a scope can name (`0..MAX_SCOPE_LAYERS`).
pub const MAX_SCOPE_LAYERS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectKind {
    Gain,
    Filter,
    Eq3,
    Gate,
    Compressor,
    Delay,
    Reverb,
    Saturation,
    Bitcrusher,
    AutoFilter,
    Chorus,
    Flanger,
    Phaser,
    Tremolo,
    RingMod,
}

impl EffectKind {
    pub const ALL: [EffectKind; 15] = [
        EffectKind::Gain,
        EffectKind::Filter,
        EffectKind::Eq3,
        EffectKind::Gate,
        EffectKind::Compressor,
        EffectKind::Delay,
        EffectKind::Reverb,
        EffectKind::Saturation,
        EffectKind::Bitcrusher,
        EffectKind::AutoFilter,
        EffectKind::Chorus,
        EffectKind::Flanger,
        EffectKind::Phaser,
        EffectKind::Tremolo,
        EffectKind::RingMod,
    ];

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            EffectKind::Gain => "gain",
            EffectKind::Filter => "filter",
            EffectKind::Eq3 => "eq3",
            EffectKind::Gate => "gate",
            EffectKind::Compressor => "compressor",
            EffectKind::Delay => "delay",
            EffectKind::Reverb => "reverb",
            EffectKind::Saturation => "saturation",
            EffectKind::Bitcrusher => "bitcrusher",
            EffectKind::AutoFilter => "autoFilter",
            EffectKind::Chorus => "chorus",
            EffectKind::Flanger => "flanger",
            EffectKind::Phaser => "phaser",
            EffectKind::Tremolo => "tremolo",
            EffectKind::RingMod => "ringMod",
        }
    }
}

/// The values a parameter can take.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamRange {
    /// Any number in `min..=max` (smoothed when it changes).
    Continuous { min: f32, max: f32 },
    /// One of these strings; stored as the option index. Changing it rebuilds the effect.
    Choice(&'static [&'static str]),
    /// One of these numbers (nearest wins); stored as the value. Changing it rebuilds the effect.
    Steps(&'static [f32]),
}

/// One parameter of an effect kind. `default` is a value for continuous and
/// step params and an option index for choices. These are the engine's copy
/// of the `effects.<kind>.<param>` defaults in the settings registry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDef {
    pub name: &'static str,
    pub range: ParamRange,
    pub default: f32,
}

impl ParamDef {
    pub fn is_discrete(&self) -> bool {
        !matches!(self.range, ParamRange::Continuous { .. })
    }

    /// Clamp/snap a number into this param's domain (non-finite → default).
    pub fn sanitize(&self, v: f32) -> f32 {
        if !v.is_finite() {
            return self.default;
        }
        match self.range {
            ParamRange::Continuous { min, max } => v.clamp(min, max),
            ParamRange::Choice(options) => (v.round().max(0.0) as usize).min(options.len() - 1) as f32,
            ParamRange::Steps(values) => {
                values
                    .iter()
                    .copied()
                    .fold(values[0], |best, s| if (s - v).abs() < (best - v).abs() { s } else { best })
            }
        }
    }

    /// Read a JSON value; `None` if it does not fit this param (the default is then kept).
    fn read(&self, value: &Value) -> Option<f32> {
        let number = match value {
            Value::Number(n) => n.as_f64().map(|f| f as f32),
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Value::String(s) => match self.range {
                ParamRange::Choice(options) => return options.iter().position(|o| o == s).map(|i| i as f32),
                _ => s.trim().parse::<f32>().ok(),
            },
            _ => None,
        }?;
        match self.range {
            // A choice given as a number is read as its index.
            _ if !number.is_finite() => None,
            _ => Some(self.sanitize(number)),
        }
    }
}

macro_rules! cont {
    ($name:literal, $min:expr, $max:expr, $default:expr) => {
        ParamDef { name: $name, range: ParamRange::Continuous { min: $min, max: $max }, default: $default }
    };
}
macro_rules! choice {
    ($name:literal, $options:expr, $default:expr) => {
        ParamDef { name: $name, range: ParamRange::Choice($options), default: $default }
    };
}

pub const FILTER_TYPES: &[&str] = &["lowpass", "highpass", "bandpass", "notch", "lowshelf", "highshelf", "peak"];
pub const DELAY_SYNC: &[&str] = &["off", "1/4", "1/8", "1/8d", "1/16", "1/4t"];
pub const SATURATION_MODES: &[&str] = &["soft", "hard", "tube", "fold"];
pub const AUTO_FILTER_TYPES: &[&str] = &["lowpass", "bandpass", "highpass"];
pub const AUTO_FILTER_SOURCES: &[&str] = &["lfo", "envelope"];
pub const AUTO_FILTER_SYNC: &[&str] = &["off", "1/1", "1/2", "1/4", "1/8"];
pub const PHASER_STAGES: &[f32] = &[2.0, 4.0, 6.0, 8.0, 12.0];
pub const TREMOLO_SHAPES: &[&str] = &["sine", "triangle", "square"];
pub const TREMOLO_SYNC: &[&str] = &["off", "1/4", "1/8", "1/16"];

/// Length in quarter notes of a sync option ("off" → `None`).
pub fn sync_beats(option: &str) -> Option<f64> {
    Some(match option {
        "1/1" => 4.0,
        "1/2" => 2.0,
        "1/4" => 1.0,
        "1/8" => 0.5,
        "1/8d" => 0.75,
        "1/16" => 0.25,
        "1/4t" => 2.0 / 3.0,
        _ => return None,
    })
}

const GAIN: &[ParamDef] = &[cont!("gainDb", -36.0, 24.0, 0.0)];
const FILTER: &[ParamDef] = &[
    choice!("type", FILTER_TYPES, 0.0),
    cont!("cutoffHz", 20.0, 20_000.0, 1_200.0),
    cont!("q", 0.1, 18.0, 0.707),
    cont!("gainDb", -24.0, 24.0, 0.0),
];
const EQ3: &[ParamDef] = &[
    cont!("lowDb", -24.0, 24.0, 0.0),
    cont!("lowHz", 40.0, 800.0, 200.0),
    cont!("midDb", -24.0, 24.0, 0.0),
    cont!("midHz", 200.0, 8_000.0, 1_000.0),
    cont!("midQ", 0.2, 8.0, 0.9),
    cont!("highDb", -24.0, 24.0, 0.0),
    cont!("highHz", 1_000.0, 16_000.0, 4_000.0),
];
const GATE: &[ParamDef] = &[
    cont!("thresholdDb", -90.0, 0.0, -50.0),
    cont!("rangeDb", -90.0, 0.0, -60.0),
    cont!("attackMs", 0.1, 50.0, 1.0),
    cont!("holdMs", 0.0, 500.0, 30.0),
    cont!("releaseMs", 5.0, 2_000.0, 120.0),
];
const COMPRESSOR: &[ParamDef] = &[
    cont!("thresholdDb", -60.0, 0.0, -18.0),
    cont!("ratio", 1.0, 20.0, 3.0),
    cont!("kneeDb", 0.0, 24.0, 6.0),
    cont!("attackMs", 0.1, 200.0, 10.0),
    cont!("releaseMs", 5.0, 2_000.0, 120.0),
    cont!("makeupDb", 0.0, 24.0, 0.0),
];
const DELAY: &[ParamDef] = &[
    cont!("timeMs", 1.0, 2_000.0, 375.0),
    choice!("sync", DELAY_SYNC, 0.0),
    cont!("feedback", 0.0, 0.95, 0.35),
    cont!("toneHz", 500.0, 16_000.0, 6_000.0),
];
const REVERB: &[ParamDef] = &[
    cont!("size", 0.0, 1.0, 0.5),
    cont!("decaySec", 0.1, 12.0, 1.8),
    cont!("damping", 0.0, 1.0, 0.5),
    cont!("preDelayMs", 0.0, 200.0, 20.0),
];
const SATURATION: &[ParamDef] = &[
    cont!("driveDb", 0.0, 36.0, 12.0),
    choice!("mode", SATURATION_MODES, 0.0),
    cont!("toneHz", 500.0, 16_000.0, 8_000.0),
    cont!("outputDb", -24.0, 12.0, -3.0),
];
const BITCRUSHER: &[ParamDef] = &[cont!("bits", 1.0, 16.0, 8.0), cont!("downsample", 1.0, 32.0, 4.0)];
const AUTO_FILTER: &[ParamDef] = &[
    choice!("type", AUTO_FILTER_TYPES, 0.0),
    cont!("baseHz", 40.0, 8_000.0, 400.0),
    cont!("depthOct", 0.0, 6.0, 3.0),
    choice!("source", AUTO_FILTER_SOURCES, 0.0),
    cont!("rateHz", 0.05, 20.0, 1.0),
    choice!("sync", AUTO_FILTER_SYNC, 0.0),
    cont!("q", 0.5, 18.0, 4.0),
    cont!("envAttackMs", 1.0, 200.0, 10.0),
    cont!("envReleaseMs", 10.0, 2_000.0, 200.0),
];
const CHORUS: &[ParamDef] = &[
    cont!("rateHz", 0.05, 5.0, 0.8),
    cont!("depthMs", 0.0, 15.0, 4.0),
    cont!("delayMs", 5.0, 40.0, 15.0),
    cont!("feedback", 0.0, 0.9, 0.0),
];
const FLANGER: &[ParamDef] = &[
    cont!("rateHz", 0.05, 5.0, 0.3),
    cont!("depthMs", 0.0, 8.0, 2.0),
    cont!("delayMs", 0.1, 10.0, 1.0),
    cont!("feedback", -0.95, 0.95, 0.6),
];
const PHASER: &[ParamDef] = &[
    cont!("rateHz", 0.05, 5.0, 0.5),
    ParamDef { name: "stages", range: ParamRange::Steps(PHASER_STAGES), default: 6.0 },
    cont!("depth", 0.0, 1.0, 0.8),
    cont!("feedback", 0.0, 0.9, 0.5),
    cont!("centerHz", 100.0, 4_000.0, 800.0),
];
const TREMOLO: &[ParamDef] = &[
    cont!("rateHz", 0.1, 20.0, 5.0),
    cont!("depth", 0.0, 1.0, 0.6),
    choice!("shape", TREMOLO_SHAPES, 0.0),
    choice!("sync", TREMOLO_SYNC, 0.0),
];
const RING_MOD: &[ParamDef] = &[cont!("freqHz", 1.0, 4_000.0, 220.0)];

/// Dry/wet a new effect of `kind` starts with, and what a missing `mix`
/// means (`effects.<kind>.mix` in the settings registry; mirrors the UI table
/// in `src/effects/types.ts`).
pub fn default_mix(kind: EffectKind) -> f32 {
    match kind {
        EffectKind::Delay => 0.35,
        EffectKind::Reverb => 0.3,
        EffectKind::Chorus | EffectKind::Flanger | EffectKind::Phaser => 0.5,
        _ => 1.0,
    }
}

/// The parameter table of `kind`, in storage order.
pub fn params_of(kind: EffectKind) -> &'static [ParamDef] {
    match kind {
        EffectKind::Gain => GAIN,
        EffectKind::Filter => FILTER,
        EffectKind::Eq3 => EQ3,
        EffectKind::Gate => GATE,
        EffectKind::Compressor => COMPRESSOR,
        EffectKind::Delay => DELAY,
        EffectKind::Reverb => REVERB,
        EffectKind::Saturation => SATURATION,
        EffectKind::Bitcrusher => BITCRUSHER,
        EffectKind::AutoFilter => AUTO_FILTER,
        EffectKind::Chorus => CHORUS,
        EffectKind::Flanger => FLANGER,
        EffectKind::Phaser => PHASER,
        EffectKind::Tremolo => TREMOLO,
        EffectKind::RingMod => RING_MOD,
    }
}

/// A short id stored inline (no allocation).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EffectId {
    len: u8,
    bytes: [u8; MAX_EFFECT_ID_BYTES],
}

impl EffectId {
    pub const EMPTY: Self = Self { len: 0, bytes: [0; MAX_EFFECT_ID_BYTES] };

    /// `None` when `id` is longer than [`MAX_EFFECT_ID_BYTES`].
    pub fn new(id: &str) -> Option<Self> {
        let b = id.as_bytes();
        if b.len() > MAX_EFFECT_ID_BYTES {
            return None;
        }
        let mut out = Self::EMPTY;
        out.bytes[..b.len()].copy_from_slice(b);
        out.len = b.len() as u8;
        Some(out)
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

impl std::fmt::Debug for EffectId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

/// One effect: kind, bypass, dry/wet and its params (storage order of [`params_of`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectSpec {
    pub id: EffectId,
    pub kind: EffectKind,
    pub enabled: bool,
    pub mix: f32,
    pub params: [f32; MAX_PARAMS],
}

impl EffectSpec {
    /// An enabled effect with default params and the kind's [`default_mix`].
    pub fn new(id: &str, kind: EffectKind) -> Self {
        let mut params = [0.0; MAX_PARAMS];
        for (p, d) in params.iter_mut().zip(params_of(kind)) {
            *p = d.default;
        }
        Self { id: EffectId::new(id).unwrap_or(EffectId::EMPTY), kind, enabled: true, mix: default_mix(kind), params }
    }

    pub fn defs(&self) -> &'static [ParamDef] {
        params_of(self.kind)
    }

    /// Value of `name` (choices: the option index).
    pub fn param(&self, name: &str) -> Option<f32> {
        self.defs().iter().position(|d| d.name == name).map(|i| self.params[i])
    }

    /// The selected option of a choice param.
    pub fn choice(&self, name: &str) -> Option<&'static str> {
        let i = self.defs().iter().position(|d| d.name == name)?;
        match self.defs()[i].range {
            ParamRange::Choice(options) => options.get(self.params[i] as usize).copied(),
            _ => None,
        }
    }

    /// Set a param by name (clamped). Unknown names are ignored. Choices take
    /// the option string through [`Self::set_choice`] or the index here.
    pub fn set(&mut self, name: &str, value: f32) -> &mut Self {
        if let Some(i) = self.defs().iter().position(|d| d.name == name) {
            self.params[i] = self.defs()[i].sanitize(value);
        }
        self
    }

    pub fn set_choice(&mut self, name: &str, option: &str) -> &mut Self {
        if let Some(i) = self.defs().iter().position(|d| d.name == name) {
            if let Some(v) = self.defs()[i].read(&Value::String(option.into())) {
                self.params[i] = v;
            }
        }
        self
    }

    /// Same id, kind and discrete params: `other` can be reached by smoothing.
    pub fn same_structure(&self, other: &Self) -> bool {
        self.id == other.id
            && self.kind == other.kind
            && self.defs().iter().enumerate().all(|(i, d)| !d.is_discrete() || self.params[i] == other.params[i])
    }
}

/// Which layers go through the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerScope {
    All,
    /// Bit `i` set: active layer `i` goes through the chain; the rest play dry.
    Layers(u32),
}

impl LayerScope {
    #[inline]
    pub fn includes(&self, layer: usize) -> bool {
        match *self {
            LayerScope::All => true,
            LayerScope::Layers(mask) => layer < MAX_SCOPE_LAYERS && mask & (1 << layer) != 0,
        }
    }
}

/// One track's effect chain. `Copy`, fixed size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainSpec {
    pub enabled: bool,
    pub layers: LayerScope,
    len: u8,
    effects: [EffectSpec; MAX_EFFECTS_PER_TRACK],
}

impl Default for ChainSpec {
    fn default() -> Self {
        Self::EMPTY
    }
}

const NO_EFFECT: EffectSpec =
    EffectSpec { id: EffectId::EMPTY, kind: EffectKind::Gain, enabled: false, mix: 0.0, params: [0.0; MAX_PARAMS] };

impl ChainSpec {
    /// Enabled, all layers, no effects.
    pub const EMPTY: Self =
        Self { enabled: true, layers: LayerScope::All, len: 0, effects: [NO_EFFECT; MAX_EFFECTS_PER_TRACK] };

    pub fn effects(&self) -> &[EffectSpec] {
        &self.effects[..self.len as usize]
    }

    pub fn effects_mut(&mut self) -> &mut [EffectSpec] {
        &mut self.effects[..self.len as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Append an effect; `false` when the chain is full.
    pub fn push(&mut self, effect: EffectSpec) -> bool {
        if self.len as usize >= MAX_EFFECTS_PER_TRACK {
            return false;
        }
        self.effects[self.len as usize] = effect;
        self.len += 1;
        true
    }

    pub fn clear(&mut self) {
        *self = Self::EMPTY;
    }

    /// A chain with these effects (extra ones are dropped).
    pub fn with(effects: &[EffectSpec]) -> Self {
        let mut c = Self::EMPTY;
        for e in effects {
            c.push(*e);
        }
        c
    }

    /// Going from `self` to `next` needs new DSP objects (and a crossfade):
    /// an effect was added, removed, reordered, or changed kind or a discrete
    /// param. Everything else (continuous params, mix, bypass, chain bypass,
    /// scope) is smoothed in place.
    pub fn needs_rebuild(&self, next: &ChainSpec) -> bool {
        self.len != next.len || self.effects().iter().zip(next.effects()).any(|(a, b)| !a.same_structure(b))
    }

    /// The same chain over all layers (used where layers cannot be told apart).
    pub fn all_layers(mut self) -> Self {
        self.layers = LayerScope::All;
        self
    }
}

// ---- JSON ----------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EffectIn {
    id: String,
    kind: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    mix: Value,
    #[serde(default)]
    params: Map<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChainIn {
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    layers: Value,
    #[serde(default)]
    effects: Vec<Value>,
}

fn yes() -> bool {
    true
}

fn read_effect(v: Value) -> Result<EffectSpec, String> {
    let e: EffectIn = serde_json::from_value(v).map_err(|e| e.to_string())?;
    let kind = EffectKind::from_name(&e.kind).ok_or_else(|| format!("unknown effect kind {:?}", e.kind))?;
    let id = EffectId::new(&e.id).ok_or_else(|| format!("effect id longer than {MAX_EFFECT_ID_BYTES} bytes"))?;
    let mut spec = EffectSpec::new("", kind);
    spec.id = id;
    spec.enabled = e.enabled;
    spec.mix = match e.mix.as_f64() {
        Some(m) if m.is_finite() => (m as f32).clamp(0.0, 1.0),
        _ => default_mix(kind),
    };
    for (i, d) in params_of(kind).iter().enumerate() {
        if let Some(v) = e.params.get(d.name).and_then(|v| d.read(v)) {
            spec.params[i] = v;
        }
    }
    Ok(spec)
}

fn read_scope(v: &Value) -> LayerScope {
    match v {
        Value::Array(items) => LayerScope::Layers(
            items
                .iter()
                .filter_map(Value::as_u64)
                .filter(|&i| (i as usize) < MAX_SCOPE_LAYERS)
                .fold(0u32, |m, i| m | (1 << i)),
        ),
        _ => LayerScope::All,
    }
}

impl ChainSpec {
    /// Strict parse (commands): errors on unknown kinds, too many effects, long ids.
    pub fn from_json(v: Value) -> Result<Self, String> {
        let c: ChainIn = serde_json::from_value(v).map_err(|e| e.to_string())?;
        if c.effects.len() > MAX_EFFECTS_PER_TRACK {
            return Err(format!("at most {MAX_EFFECTS_PER_TRACK} effects per track"));
        }
        let mut out = Self { enabled: c.enabled, layers: read_scope(&c.layers), ..Self::EMPTY };
        for e in c.effects {
            out.push(read_effect(e)?);
        }
        Ok(out)
    }

    /// Lenient parse (project files): effects that cannot be read are dropped,
    /// garbage becomes an empty chain.
    pub fn from_json_lenient(v: Value) -> Self {
        let Ok(c) = serde_json::from_value::<ChainIn>(v) else { return Self::EMPTY };
        let mut out = Self { enabled: c.enabled, layers: read_scope(&c.layers), ..Self::EMPTY };
        for e in c.effects {
            if let Ok(spec) = read_effect(e) {
                out.push(spec);
            }
        }
        out
    }
}

impl<'de> Deserialize<'de> for ChainSpec {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::from_json(Value::deserialize(d)?).map_err(D::Error::custom)
    }
}

/// `deserialize_with` for documents: never fails (see [`ChainSpec::from_json_lenient`]).
pub fn lenient_chain<'de, D: Deserializer<'de>>(d: D) -> Result<ChainSpec, D::Error> {
    Ok(ChainSpec::from_json_lenient(Value::deserialize(d)?))
}

#[derive(Serialize)]
#[serde(untagged)]
enum ParamOut {
    Num(f32),
    Str(&'static str),
}

struct ParamsOut<'a>(&'a EffectSpec);

impl Serialize for ParamsOut<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let defs = self.0.defs();
        let mut m = s.serialize_map(Some(defs.len()))?;
        for (i, d) in defs.iter().enumerate() {
            let v = self.0.params[i];
            match d.range {
                ParamRange::Choice(options) => m.serialize_entry(d.name, &ParamOut::Str(options[v as usize]))?,
                _ => m.serialize_entry(d.name, &ParamOut::Num(v))?,
            }
        }
        m.end()
    }
}

impl Serialize for EffectSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("Effect", 5)?;
        st.serialize_field("id", self.id.as_str())?;
        st.serialize_field("kind", &self.kind)?;
        st.serialize_field("enabled", &self.enabled)?;
        st.serialize_field("mix", &self.mix)?;
        st.serialize_field("params", &ParamsOut(self))?;
        st.end()
    }
}

impl Serialize for LayerScope {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match *self {
            LayerScope::All => s.serialize_str("all"),
            LayerScope::Layers(mask) => {
                let v: Vec<u32> = (0..MAX_SCOPE_LAYERS as u32).filter(|i| mask & (1 << i) != 0).collect();
                v.serialize(s)
            }
        }
    }
}

impl Serialize for ChainSpec {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("EffectChain", 3)?;
        st.serialize_field("enabled", &self.enabled)?;
        st.serialize_field("layers", &self.layers)?;
        st.serialize_field("effects", self.effects())?;
        st.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_round_trip_clamps_defaults_and_ignores_unknown_params() {
        let v = json!({
            "enabled": true, "layers": [0, 2, 99],
            "effects": [
                { "id": "a", "kind": "filter", "enabled": false, "mix": 0.5,
                  "params": { "type": "highpass", "cutoffHz": 99999, "bogus": 3 } },
                { "id": "b", "kind": "phaser", "params": { "stages": 7 } },
                { "id": "c", "kind": "delay", "params": { "sync": "1/8d", "feedback": "0.5" } }
            ]
        });
        let c: ChainSpec = serde_json::from_value(v).unwrap();
        assert_eq!(c.layers, LayerScope::Layers(0b101));
        let f = c.effects()[0];
        assert!(!f.enabled && f.mix == 0.5);
        assert_eq!(f.choice("type"), Some("highpass"));
        assert_eq!(f.param("cutoffHz"), Some(20_000.0));
        assert_eq!(f.param("q"), Some(0.707), "missing → default");
        assert_eq!(c.effects()[1].param("stages"), Some(6.0), "nearest step");
        assert_eq!(c.effects()[1].mix, 0.5, "missing mix → the kind's default");
        assert_eq!(c.effects()[2].mix, 0.35);
        assert_eq!(c.effects()[2].choice("sync"), Some("1/8d"));
        assert_eq!(c.effects()[2].param("feedback"), Some(0.5));
        let out = serde_json::to_value(c).unwrap();
        assert_eq!(out["layers"], json!([0, 2]));
        assert_eq!(out["effects"][0]["params"]["type"], json!("highpass"));
        assert!(serde_json::to_string(&c).unwrap().contains(r#""q":0.707"#), "f32 params print short");
        let back: ChainSpec = serde_json::from_value(out).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn strict_and_lenient_parsing() {
        let bad = json!({ "effects": [{ "id": "x", "kind": "warp" }, { "id": "y", "kind": "gain" }] });
        assert!(ChainSpec::from_json(bad.clone()).is_err());
        let lenient = ChainSpec::from_json_lenient(bad);
        assert_eq!(lenient.effects().len(), 1);
        assert_eq!(ChainSpec::from_json_lenient(json!(42)), ChainSpec::EMPTY);
        let nine: Vec<_> = (0..9).map(|i| json!({ "id": format!("e{i}"), "kind": "gain" })).collect();
        assert!(ChainSpec::from_json(json!({ "effects": nine })).is_err());
        assert!(ChainSpec::from_json(json!({ "effects": [{ "id": "x".repeat(65), "kind": "gain" }] })).is_err());
        assert_eq!(
            serde_json::to_value(ChainSpec::EMPTY).unwrap(),
            json!({"enabled": true, "layers": "all", "effects": []})
        );
    }

    #[test]
    fn rebuild_only_for_structure() {
        let a = ChainSpec::with(&[EffectSpec::new("f", EffectKind::Filter)]);
        let mut b = a;
        b.effects_mut()[0].set("cutoffHz", 300.0);
        b.effects_mut()[0].enabled = false;
        b.enabled = false;
        b.layers = LayerScope::Layers(1);
        assert!(!a.needs_rebuild(&b));
        b.effects_mut()[0].set_choice("type", "notch");
        assert!(a.needs_rebuild(&b));
        assert!(a.needs_rebuild(&ChainSpec::EMPTY));
        assert!(a.needs_rebuild(&ChainSpec::with(&[EffectSpec::new("g", EffectKind::Filter)])));
    }
}
