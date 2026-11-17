//! Typed Practice Lab methods on [`Engine`]. Each sends one [`Request`] to the
//! control thread; the Tauri commands in `src-tauri` map one-to-one onto these.
//! The contract is documented in `docs/projects/practice-lab/INTERFACE.md`.

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
}
