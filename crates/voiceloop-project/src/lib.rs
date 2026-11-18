//! VoiceLoop project persistence and file interchange.
//!
//! Pure Rust with no audio devices, no threads, and no UI framework, so it
//! is portable to iOS: every function takes caller-supplied paths and uses
//! plain `std::fs`.
//!
//! - [`wav`]      WAV read/write (16/24-bit PCM, 32-bit float, BWF `bext`, `acid`)
//! - [`resample`] offline sample-rate conversion for imports
//! - [`smf`]      Standard MIDI File write (tempo + meter) and read
//! - [`arrangement`] PRODUCE timeline: lanes, regions, assets, shared render rules
//! - [`doc`]      `project.json` schema, validation, versioning, migration hook
//! - [`package`]  `<Name>.voiceloop/` folders: crash-safe save, open, verify
//! - [`recovery`] autosave slot for crash recovery
//! - [`render`]   offline mixdown and timeline-aligned stems
//! - [`rpp`]      REAPER project export
//!
//! Format specification: `docs/projects/persistence/README.md`.

pub mod arrangement;
pub mod doc;
pub mod error;
pub mod package;
pub mod recovery;
pub mod render;
pub mod resample;
pub mod rpp;
pub mod smf;
mod util;
pub mod wav;

pub use error::{ProjectError, Result};
pub use util::{new_id, now_rfc3339};

/// A fresh, empty directory for one test, under the workspace `target/`
/// folder (git-ignored) so tests never write outside the repository.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/voiceloop-project").join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
