//! VoiceLoop native audio engine.
//!
//! This crate has no dependency on Tauri or any UI. The desktop shell talks to
//! it through [`Engine`], a cheap, cloneable, thread-safe handle that sends
//! commands to a dedicated control thread. The control thread owns the CPAL
//! streams (which are not `Send` on every platform) and forwards meter
//! readings and looper state to a caller-supplied [`EventSink`] at UI rate.
//!
//! Realtime rule: code that runs inside an audio callback lives in
//! [`capture`], [`meter`], and [`looper`] (`processor`, `click`, the `io`
//! callback). It never allocates, locks, blocks, or does I/O; it communicates
//! with other threads through `rtrb` ring buffers and relaxed atomics.
//!
//! Module map (see docs/ARCHITECTURE.md):
//! - [`devices`]   audio device enumeration
//! - [`capture`]   input stream, realtime metering, mono feed to the looper
//! - [`meter`]     RMS / peak accumulation (pure, realtime-safe)
//! - [`looper`]    master clock, metronome, multitrack looper, output stream
//! - [`analysis`]  live pitch / note / onset analysis thread (DSP in `voiceloop-analysis`)
//! - [`practice`]  Practice Lab session: reference melody capture and take scoring
//! - [`engine`]    control thread, command handling, event delivery
//! - [`transport`] musical-time arithmetic (bars/beats/subdivisions ↔ frames)
//! - [`model`]     shared domain types (serialisable)

pub mod analysis;
pub mod capture;
pub mod devices;
pub mod engine;
pub mod error;
pub mod looper;
pub mod meter;
pub mod model;
pub mod practice;
pub mod transport;

pub use analysis::AnalysisConfig;
pub use engine::{Engine, EngineEvent, EngineStatus, EngineWarning, EventSink, PitchUpdate};
pub use error::EngineError;
pub use looper::{LooperCommand, LooperConfig, LooperState};
