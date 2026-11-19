//! VoiceLoop track effects (docs/projects/effects/README.md).
//!
//! - [`spec`]: the contract types ([`ChainSpec`], [`EffectSpec`], the
//!   parameter table) and their JSON form.
//! - [`fx`]: the mono processors for every [`EffectKind`].
//! - [`chain`]: [`ChainDsp`], a chain's processors with smoothing and bypass.
//!
//! Used by the engine (looper and arrangement player, realtime) and by
//! `voiceloop-project` (offline mixdown, stems, bounce), so a render sounds
//! like playback.

pub mod chain;
pub mod fx;
pub mod spec;
#[cfg(test)]
mod tests;

pub use chain::{render, ChainDsp, BYPASS_RAMP_MS, PARAM_SMOOTH_MS};
pub use spec::{
    lenient_chain, params_of, ChainSpec, EffectId, EffectKind, EffectSpec, LayerScope, ParamDef, ParamRange,
    MAX_EFFECTS_PER_TRACK, MAX_PARAMS,
};
