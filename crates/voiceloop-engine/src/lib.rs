//! VoiceLoop native audio engine.
//!
//! This crate has no dependency on Tauri or any UI. The desktop shell talks to
//! it through [`Engine`], a cheap, cloneable, thread-safe handle that sends
//! commands to a dedicated control thread. The control thread owns the CPAL
//! streams (which are not `Send` on every platform) and forwards meter
//! readings to a caller-supplied [`EventSink`] at UI rate.
//!
//! Realtime rule: code that runs inside an audio callback lives in
//! [`capture`] and [`meter`] only. It never allocates, locks, blocks, or does
//! I/O; it communicates with other threads through `rtrb` ring buffers.
//!
//! Module map (see docs/ARCHITECTURE.md):
//! - [`devices`]   audio device enumeration
//! - [`capture`]   input stream + realtime-side metering
//! - [`meter`]     RMS / peak accumulation (pure, realtime-safe)
//! - [`engine`]    control thread, command handling, event delivery
//! - [`transport`] authoritative sample clock and musical-time conversion
//! - [`model`]     shared domain types (serialisable)

pub mod capture;
pub mod devices;
pub mod engine;
pub mod error;
pub mod meter;
pub mod model;
pub mod transport;

pub use engine::{Engine, EngineEvent, EngineStatus, EngineWarning, EventSink};
pub use error::EngineError;
