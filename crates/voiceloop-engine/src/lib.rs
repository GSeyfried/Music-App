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
//! - [`engine`]    control thread, command handling, event delivery
//! - [`transport`] musical-time arithmetic (bars/beats/subdivisions ↔ frames)
//! - [`model`]     shared domain types (serialisable)
//! - [`routing`]   audio config, input/output channel routes, monitoring
//! - [`clock`]     stream clock anchors, xrun detection, drift estimation
//! - [`midi_in`]   USB-MIDI input (midir), timestamp mapping
//! - [`settings`]  persisted hardware settings
//! - [`practice`]  Practice Lab: reference/attempt takes, input→transport clock mapping, scoring
//! - [`hardware`]  control-thread side of devices, MIDI, diagnostics
//! - [`mic`]       "mic only when needed": input reasons, hold timer, requests waiting for input
//! - [`noise`]     background noise reduction: learning profiles, per-layer spectral gating, derived buffers
//! - [`platform`]  audio session (iOS AVAudioSession; desktop no-ops)
//! - [`calibration`] loopback latency test signal and detector

pub mod analysis;
pub mod arrangement;
pub mod calibration;
pub mod capture;
pub mod clock;
pub mod devices;
pub mod engine;
pub mod error;
pub mod hardware;
pub mod looper;
pub mod meter;
pub mod mic;
pub mod midi_in;
pub mod model;
pub mod noise;
pub mod platform;
pub mod practice;
pub mod preview;
pub mod project;
pub mod routing;
pub mod settings;
pub mod song;
pub mod transport;

pub use analysis::AnalysisConfig;
pub use engine::{Engine, EngineEvent, EngineStatus, EngineWarning, EventSink, PitchUpdate};
pub use error::EngineError;
pub use looper::{LooperCommand, LooperConfig, LooperState};
pub use project::{ProjectEvent, ProjectReply, ProjectRequest};
pub use routing::{AudioConfig, InputRoute, MonitorConfig, OutputRoute};
pub use settings::AudioSettings;
