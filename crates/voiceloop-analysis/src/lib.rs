//! VoiceLoop musical analysis: pitch, notes, onsets, rhythm, melody comparison.
//!
//! Pure Rust with no audio I/O, no threads, and no UI. Studio and Practice
//! Lab both use it through `voiceloop-engine`, which feeds it audio from the
//! input stream on a dedicated analysis thread. Nothing here belongs in an
//! audio callback.
//!
//! ```text
//!  samples ─▶ PitchTracker ─▶ PitchFrame (100/s) ─▶ NoteSegmenter ─▶ NoteEvent
//!     │         └ PitchDetector (YIN)                                   │
//!     └─────▶ OnsetDetector ─▶ Onset ─▶ rhythm::analyze_timing          ├─▶ melody::compare
//!                                                                       ├─▶ theory (key, intervals, chords)
//!                                                                       └─▶ midi (events, .mid export)
//! ```
//!
//! Module map:
//! - [`pitch`]    single-frame fundamental-frequency estimation (YIN)
//! - [`tracker`]  streaming pitch tracking: smoothing, octave correction, stability
//! - [`segment`]  note onsets/offsets and per-note intonation
//! - [`onset`]    energy-based onset detection (percussive and plucked attacks)
//! - [`rhythm`]   timing of onsets against a tempo grid; tempo estimation
//! - [`melody`]   reference melodies and performed-versus-reference comparison
//! - [`practice`] Practice Lab takes, attempt scoring, feedback items, progress trend
//! - [`theory`]   intervals, scales, key estimation, chord identification
//! - [`midi`]     MIDI-compatible events and Standard MIDI File export
//! - [`note`]     frequency ↔ note arithmetic
//! - [`signal`]   deterministic synthetic signals for tests and benchmarks

pub mod melody;
pub mod midi;
pub mod note;
pub mod onset;
pub mod pitch;
pub mod practice;
pub mod rhythm;
pub mod segment;
pub mod signal;
pub mod theory;
pub mod tracker;

pub use note::{NoteReading, Tuning};
pub use onset::{Onset, OnsetConfig, OnsetDetector};
pub use pitch::{PitchConfig, PitchDetector, PitchEstimate, Unpitched};
pub use segment::{NoteConfig, NoteEvent, NoteSegmenter};
pub use tracker::{PitchFrame, PitchTracker, TrackerConfig};
