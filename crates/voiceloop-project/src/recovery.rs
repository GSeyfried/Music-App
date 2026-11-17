//! Session recovery: autosaves of the current session into a folder the host
//! app supplies (e.g. `<app data>/recovery`), restorable after a crash.
//!
//! One slot: `<recovery>/session.voiceloop/` (a normal package, so the same
//! crash-safe save applies and unchanged layers are not rewritten) plus
//! `<recovery>/recovery.json` describing it. A clean shutdown, or the user
//! saving to a real location, calls [`discard`]. If the marker exists at
//! start-up, the previous run ended without that and the UI can offer it.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::doc::ProjectDoc;
use crate::error::{ProjectError, Result};
use crate::package::{self, Progress, TrackAudio};
use crate::util;

pub const MARKER_FILE: &str = "recovery.json";
pub const SESSION_DIR: &str = "session.voiceloop";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryInfo {
    /// Where to restore from (inside the recovery folder).
    pub package_path: String,
    /// The project the session belonged to, if it had been saved.
    pub original_path: Option<String>,
    pub project_name: String,
    /// RFC 3339 UTC.
    pub saved_at: String,
    pub app_version: String,
}

/// Write (or refresh) the recovery copy.
pub fn autosave(
    recovery_dir: &Path,
    doc: &ProjectDoc,
    audio: &[TrackAudio],
    practice: Option<&[u8]>,
    original_path: Option<&Path>,
    progress: Progress,
) -> Result<RecoveryInfo> {
    fs::create_dir_all(recovery_dir).map_err(|e| ProjectError::io("create", recovery_dir, e))?;
    let pkg = recovery_dir.join(SESSION_DIR);
    package::save_package(&pkg, doc, audio, practice, progress)?;
    let info = RecoveryInfo {
        package_path: pkg.to_string_lossy().into_owned(),
        original_path: original_path.map(|p| p.to_string_lossy().into_owned()),
        project_name: doc.name.clone(),
        saved_at: util::now_rfc3339(),
        app_version: env!("CARGO_PKG_VERSION").into(),
    };
    let bytes = serde_json::to_vec_pretty(&info).map_err(|e| ProjectError::Invalid(e.to_string()))?;
    util::atomic_write(&recovery_dir.join(MARKER_FILE), &bytes)?;
    Ok(info)
}

/// The recoverable session, if any (and if it can actually be opened).
pub fn find(recovery_dir: &Path) -> Result<Option<RecoveryInfo>> {
    let marker = recovery_dir.join(MARKER_FILE);
    let bytes = match fs::read(&marker) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(ProjectError::io("read", &marker, e)),
    };
    let Ok(info) = serde_json::from_slice::<RecoveryInfo>(&bytes) else { return Ok(None) };
    match package::open_package(&recovery_dir.join(SESSION_DIR)) {
        Ok(_) => Ok(Some(info)),
        Err(_) => Ok(None),
    }
}

/// Path of the recovery package to open with the normal project loader.
pub fn package_dir(recovery_dir: &Path) -> PathBuf {
    recovery_dir.join(SESSION_DIR)
}

/// Delete the recovery copy (after a clean exit, a real save, or on request).
pub fn discard(recovery_dir: &Path) -> Result<()> {
    let marker = recovery_dir.join(MARKER_FILE);
    match fs::remove_file(&marker) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(ProjectError::io("remove", &marker, e)),
    }
    let pkg = recovery_dir.join(SESSION_DIR);
    if pkg.exists() {
        fs::remove_dir_all(&pkg).map_err(|e| ProjectError::io("remove", &pkg, e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::LoopDoc;

    #[test]
    fn autosave_find_restore_discard() {
        let rec = crate::test_dir("recovery");
        assert_eq!(find(&rec).unwrap(), None);
        let mut doc = ProjectDoc::new("Jam", 48_000, 1);
        doc.loop_ = Some(LoopDoc { period_frames: 100.0, extra: Default::default() });
        let audio = vec![TrackAudio { layers: vec![vec![0.25f32; 100].into()], redo: vec![] }];
        autosave(&rec, &doc, &audio, None, None, &mut |_| {}).unwrap();
        let info = find(&rec).unwrap().expect("recoverable");
        assert_eq!(info.project_name, "Jam");
        let opened = package::open_package(&package_dir(&rec)).unwrap();
        assert_eq!(package::load_audio(&opened.path, &opened.doc, &mut |_| {}).unwrap(), audio);

        // A second autosave refreshes it.
        let audio2 = vec![TrackAudio { layers: vec![vec![0.5f32; 100].into()], redo: vec![] }];
        autosave(&rec, &opened.doc, &audio2, None, Some(Path::new("/x/Jam.voiceloop")), &mut |_| {}).unwrap();
        assert_eq!(find(&rec).unwrap().unwrap().original_path.as_deref(), Some("/x/Jam.voiceloop"));

        discard(&rec).unwrap();
        assert_eq!(find(&rec).unwrap(), None);
        discard(&rec).unwrap(); // idempotent
    }

    #[test]
    fn a_damaged_recovery_copy_is_not_offered() {
        let rec = crate::test_dir("recovery-bad");
        let doc = ProjectDoc::new("Jam", 48_000, 1);
        autosave(&rec, &doc, &[], None, None, &mut |_| {}).unwrap();
        fs::write(package_dir(&rec).join(package::PROJECT_FILE), b"{").unwrap();
        fs::remove_file(package_dir(&rec).join(package::PREVIOUS_FILE)).ok();
        assert_eq!(find(&rec).unwrap(), None);
    }
}
