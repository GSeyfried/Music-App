//! Typed Practice Lab methods on [`Engine`]. Each sends one [`Request`] to the
//! control thread; the Tauri commands in `src-tauri` map one-to-one onto these.
//! The contract is documented in `docs/projects/practice-lab/INTERFACE.md`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use voiceloop_analysis::practice::ScoringConfig;

use super::{
    AttemptRecord, CaptureOptions, PracticeConfig, PracticeLibrary, PracticeStatus, ReferenceDetail, ReferenceSummary,
    Request, Response,
};
use crate::engine::Engine;
use crate::error::EngineError;

fn unexpected() -> EngineError {
    EngineError::Backend("unexpected practice response".into())
}

macro_rules! expect {
    ($result:expr, $variant:ident) => {
        match $result? {
            Response::$variant(v) => Ok(v),
            _ => Err(unexpected()),
        }
    };
}

fn done(r: Result<Response, EngineError>) -> Result<(), EngineError> {
    match r? {
        Response::Done => Ok(()),
        _ => Err(unexpected()),
    }
}

impl Engine {
    /// Start recording a reference. Ends with `practice_stop` (or by itself
    /// after `options.bars` bars / `maxTakeSeconds`); the result arrives as a
    /// `practiceTake` event of kind `reference`.
    pub fn practice_start_reference(&self, options: CaptureOptions) -> Result<PracticeStatus, EngineError> {
        expect!(self.practice_request(Request::StartReference(options)), Status)
    }

    /// Start an attempt against a reference; the scored result arrives as a
    /// `practiceTake` event of kind `attempt`.
    pub fn practice_start_attempt(
        &self,
        reference_id: String,
        options: CaptureOptions,
    ) -> Result<PracticeStatus, EngineError> {
        expect!(self.practice_request(Request::StartAttempt { reference_id, options }), Status)
    }

    /// End the take in progress (it is analysed and saved shortly after).
    pub fn practice_stop(&self) -> Result<PracticeStatus, EngineError> {
        expect!(self.practice_request(Request::Stop), Status)
    }

    /// Abandon the take in progress without saving it.
    pub fn practice_cancel(&self) -> Result<PracticeStatus, EngineError> {
        expect!(self.practice_request(Request::Cancel), Status)
    }

    pub fn practice_status(&self) -> Result<PracticeStatus, EngineError> {
        expect!(self.practice_request(Request::Status), Status)
    }

    pub fn practice_list_references(&self) -> Result<Vec<ReferenceSummary>, EngineError> {
        expect!(self.practice_request(Request::ListReferences), References)
    }

    pub fn practice_get_reference(&self, reference_id: String) -> Result<ReferenceDetail, EngineError> {
        expect!(self.practice_request(Request::GetReference(reference_id)), Reference).map(|b| *b)
    }

    pub fn practice_rename_reference(&self, reference_id: String, name: String) -> Result<(), EngineError> {
        done(self.practice_request(Request::RenameReference { reference_id, name }))
    }

    pub fn practice_delete_reference(&self, reference_id: String) -> Result<(), EngineError> {
        done(self.practice_request(Request::DeleteReference(reference_id)))
    }

    pub fn practice_get_attempt(&self, reference_id: String, attempt_id: String) -> Result<AttemptRecord, EngineError> {
        expect!(self.practice_request(Request::GetAttempt { reference_id, attempt_id }), Attempt).map(|b| *b)
    }

    pub fn practice_delete_attempt(&self, reference_id: String, attempt_id: String) -> Result<(), EngineError> {
        done(self.practice_request(Request::DeleteAttempt { reference_id, attempt_id }))
    }

    /// Score a stored attempt again (e.g. with transposition or tempo
    /// tolerance switched on). `None` uses the current configuration. The
    /// stored report is replaced.
    pub fn practice_rescore_attempt(
        &self,
        reference_id: String,
        attempt_id: String,
        scoring: Option<ScoringConfig>,
    ) -> Result<AttemptRecord, EngineError> {
        expect!(self.practice_request(Request::Rescore { reference_id, attempt_id, scoring }), Attempt).map(|b| *b)
    }

    pub fn practice_get_config(&self) -> Result<PracticeConfig, EngineError> {
        expect!(self.practice_request(Request::GetConfig), Config)
    }

    pub fn practice_set_config(&self, config: PracticeConfig) -> Result<PracticeConfig, EngineError> {
        expect!(self.practice_request(Request::SetConfig(config)), Config)
    }

    /// The whole library as plain data (for the persistence layer).
    pub fn practice_export_library(&self) -> Result<PracticeLibrary, EngineError> {
        expect!(self.practice_request(Request::Export), Library).map(|b| *b)
    }

    /// Load references (and their attempts). `replace` drops the current
    /// library first; otherwise entries with the same id are replaced.
    pub fn practice_import_library(&self, library: PracticeLibrary, replace: bool) -> Result<(), EngineError> {
        done(self.practice_request(Request::Import { library, replace }))
    }

    /// Write the whole Practice library (every reference and attempt with its
    /// take), the Practice config and `extra` (app settings from the UI) to a
    /// JSON file for debugging or backup. Runs on the calling thread; the
    /// control thread only hands over the data.
    pub fn practice_export_file(
        &self,
        path: &Path,
        extra: serde_json::Value,
    ) -> Result<PracticeExportSummary, EngineError> {
        let library = self.practice_export_library()?;
        let config = self.practice_get_config()?;
        let summary = PracticeExportSummary {
            references: library.references.len() as u32,
            attempts: library.references.iter().map(|r| r.attempts.len() as u32).sum(),
        };
        let file = PracticeExportFile {
            kind: PRACTICE_EXPORT_KIND.into(),
            format_version: 1,
            app_version: env!("CARGO_PKG_VERSION").into(),
            exported_at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
            config,
            library,
            extra,
        };
        let json = serde_json::to_vec_pretty(&file).map_err(|e| EngineError::Backend(e.to_string()))?;
        std::fs::write(path, json)
            .map_err(|e| EngineError::Invalid(format!("cannot write {}: {e}", path.display())))?;
        Ok(summary)
    }

    /// Load references from a file written by [`Engine::practice_export_file`]
    /// (or a bare library JSON). `replace` drops the current library first;
    /// otherwise references with the same id are replaced and others added.
    pub fn practice_import_file(&self, path: &Path, replace: bool) -> Result<PracticeExportSummary, EngineError> {
        let bytes =
            std::fs::read(path).map_err(|e| EngineError::Invalid(format!("cannot read {}: {e}", path.display())))?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| EngineError::Invalid(format!("not JSON: {e}")))?;
        let library_value = match value.get("kind").and_then(|k| k.as_str()) {
            Some(PRACTICE_EXPORT_KIND) => value.get("library").cloned().unwrap_or_default(),
            _ => value,
        };
        if library_value.get("references").is_none_or(|r| !r.is_array()) {
            return Err(EngineError::Invalid("not a Practice library: no references".into()));
        }
        let library: PracticeLibrary = serde_json::from_value(library_value)
            .map_err(|e| EngineError::Invalid(format!("not a Practice library: {e}")))?;
        let summary = PracticeExportSummary {
            references: library.references.len() as u32,
            attempts: library.references.iter().map(|r| r.attempts.len() as u32).sum(),
        };
        self.practice_import_library(library, replace)?;
        Ok(summary)
    }
}

/// `kind` tag of a Practice export file.
pub const PRACTICE_EXPORT_KIND: &str = "voiceloopPracticeExport";

/// A Practice export file (JSON, camelCase).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeExportFile {
    pub kind: String,
    pub format_version: u32,
    pub app_version: String,
    pub exported_at_ms: u64,
    pub config: PracticeConfig,
    pub library: PracticeLibrary,
    /// Whatever the UI adds (app settings, user agent, ...).
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// What an export or import contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PracticeExportSummary {
    pub references: u32,
    pub attempts: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_file_round_trips_and_bare_libraries_import_too() {
        let engine = Engine::start(Box::new(|_| {}));
        let dir = std::env::temp_dir().join(format!("voiceloop-practice-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("practice.json");
        let extra = serde_json::json!({ "settings": { "practice.difficulty": "medium" } });
        let out = engine.practice_export_file(&path, extra.clone()).unwrap();
        assert_eq!(out, PracticeExportSummary { references: 0, attempts: 0 });
        let file: PracticeExportFile = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(file.kind, PRACTICE_EXPORT_KIND);
        assert_eq!(file.extra, extra);
        assert_eq!(engine.practice_import_file(&path, false).unwrap(), out);

        let bare = dir.join("library.json");
        std::fs::write(&bare, serde_json::to_vec(&file.library).unwrap()).unwrap();
        assert!(engine.practice_import_file(&bare, true).is_ok());
        std::fs::write(&bare, b"{\"nope\": 1}").unwrap();
        assert!(engine.practice_import_file(&bare, true).is_err());
        engine.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
