//! Tauri shell: exposes engine commands to the frontend and forwards engine
//! events as a single `engine://event` stream. No audio processing happens
//! here, and no raw samples cross the bridge: only ~30 Hz level readings and
//! looper state, status changes, notices, and errors.

use tauri::{Emitter, Manager, RunEvent, State};
use voiceloop_engine::devices::DeviceList;
use voiceloop_engine::midi_in::MidiPortInfo;
use voiceloop_engine::model::AudioDevice;
use voiceloop_engine::{
    AnalysisConfig, AudioConfig, AudioSettings, Engine, EngineError, EngineStatus, EngineWarning, InputRoute,
    LooperCommand, MonitorConfig, OutputRoute,
};

pub const ENGINE_EVENT: &str = "engine://event";

#[tauri::command]
fn list_input_devices(engine: State<'_, Engine>) -> Result<Vec<AudioDevice>, EngineError> {
    engine.list_input_devices()
}

#[tauri::command]
fn start_input(engine: State<'_, Engine>, device_id: Option<String>) -> Result<EngineStatus, EngineError> {
    engine.start_input(device_id)
}

#[tauri::command]
fn stop_input(engine: State<'_, Engine>) -> Result<EngineStatus, EngineError> {
    engine.stop_input()
}

/// All looper/transport actions go through one command carrying a tagged enum,
/// e.g. `{ "type": "record", "track": 0 }`. Results arrive as `looper` events.
#[tauri::command]
fn looper_command(engine: State<'_, Engine>, command: LooperCommand) -> Result<(), EngineError> {
    engine.looper(command)
}

/// Pitch / note / onset analysis settings; any omitted field keeps its default.
/// Results arrive as `pitch`, `note`, and `onset` events.
#[tauri::command]
fn set_analysis_config(engine: State<'_, Engine>, config: AnalysisConfig) -> Result<(), EngineError> {
    engine.set_analysis_config(config)
}

#[tauri::command]
fn engine_status(engine: State<'_, Engine>) -> Result<EngineStatus, EngineError> {
    engine.status()
}

// --- Hardware I/O (docs/projects/hardware-io/INTERFACE.md) ---

/// Every input and output device with supported configurations. Also a re-scan.
#[tauri::command]
fn list_audio_devices(engine: State<'_, Engine>) -> Result<DeviceList, EngineError> {
    engine.list_audio_devices()
}

/// Start (or restart) audio with explicit devices, rate, buffer, routes, monitoring.
#[tauri::command]
fn start_audio(engine: State<'_, Engine>, config: AudioConfig) -> Result<EngineStatus, EngineError> {
    engine.start_audio(config)
}

#[tauri::command]
fn set_input_route(engine: State<'_, Engine>, route: InputRoute) -> Result<EngineStatus, EngineError> {
    engine.set_input_route(route)
}

#[tauri::command]
fn set_output_route(engine: State<'_, Engine>, route: OutputRoute) -> Result<EngineStatus, EngineError> {
    engine.set_output_route(route)
}

#[tauri::command]
fn set_monitor(engine: State<'_, Engine>, monitor: MonitorConfig) -> Result<EngineStatus, EngineError> {
    engine.set_monitor(monitor)
}

#[tauri::command]
fn list_midi_inputs(engine: State<'_, Engine>) -> Result<Vec<MidiPortInfo>, EngineError> {
    engine.list_midi_inputs()
}

#[tauri::command]
fn open_midi_input(engine: State<'_, Engine>, port_id: String) -> Result<MidiPortInfo, EngineError> {
    engine.open_midi_input(port_id)
}

#[tauri::command]
fn close_midi_input(engine: State<'_, Engine>, port_id: String) -> Result<(), EngineError> {
    engine.close_midi_input(port_id)
}

#[tauri::command]
fn get_audio_settings(engine: State<'_, Engine>) -> Result<AudioSettings, EngineError> {
    engine.audio_settings()
}

#[tauri::command]
fn save_audio_settings(engine: State<'_, Engine>) -> Result<(), EngineError> {
    engine.save_audio_settings()
}

/// Loopback latency calibration; the result arrives as a `calibration` event.
#[tauri::command]
fn start_latency_calibration(engine: State<'_, Engine>, apply: bool) -> Result<(), EngineError> {
    engine.start_latency_calibration(apply)
}

/// Warnings raised while loading settings at startup (emitted before the webview listened).
#[tauri::command]
fn startup_warnings(engine: State<'_, Engine>) -> Result<Vec<EngineWarning>, EngineError> {
    engine.startup_warnings()
}

// --- Projects and file interchange (docs/projects/persistence/INTERFACE.md) ---

/// New / save / open / export / import / recovery settings. Returns at once;
/// progress and results arrive as `project` events.
#[tauri::command]
fn project_command(
    engine: State<'_, Engine>,
    request: voiceloop_engine::ProjectRequest,
) -> Result<voiceloop_engine::ProjectReply, EngineError> {
    engine.project(request)
}

/// The crash-recovery session in `dir`, if any.
#[tauri::command]
fn find_recovery(dir: String) -> Result<Option<voiceloop_engine::project::RecoveryInfo>, EngineError> {
    voiceloop_engine::project::find_recovery(std::path::Path::new(&dir))
}

#[tauri::command]
fn discard_recovery(dir: String) -> Result<(), EngineError> {
    voiceloop_engine::project::discard_recovery(std::path::Path::new(&dir))
}

// ---- Practice Lab (docs/projects/practice-lab/INTERFACE.md) ----------------------

use voiceloop_engine::practice::{
    AttemptRecord, CaptureOptions, PracticeConfig, PracticeLibrary, PracticeScoring, PracticeStatus, ReferenceDetail,
    ReferenceSummary,
};

/// Record a reference. Ends with `practice_stop` (or by itself); the result
/// arrives as a `practiceTake` event of kind `reference`.
#[tauri::command]
fn practice_start_reference(
    engine: State<'_, Engine>,
    options: Option<CaptureOptions>,
) -> Result<PracticeStatus, EngineError> {
    engine.practice_start_reference(options.unwrap_or_default())
}

/// Record an attempt against a reference; the scored result arrives as a
/// `practiceTake` event of kind `attempt`.
#[tauri::command]
fn practice_start_attempt(
    engine: State<'_, Engine>,
    reference_id: String,
    options: Option<CaptureOptions>,
) -> Result<PracticeStatus, EngineError> {
    engine.practice_start_attempt(reference_id, options.unwrap_or_default())
}

#[tauri::command]
fn practice_stop(engine: State<'_, Engine>) -> Result<PracticeStatus, EngineError> {
    engine.practice_stop()
}

#[tauri::command]
fn practice_cancel(engine: State<'_, Engine>) -> Result<PracticeStatus, EngineError> {
    engine.practice_cancel()
}

#[tauri::command]
fn practice_status(engine: State<'_, Engine>) -> Result<PracticeStatus, EngineError> {
    engine.practice_status()
}

#[tauri::command]
fn practice_list_references(engine: State<'_, Engine>) -> Result<Vec<ReferenceSummary>, EngineError> {
    engine.practice_list_references()
}

#[tauri::command]
fn practice_get_reference(engine: State<'_, Engine>, reference_id: String) -> Result<ReferenceDetail, EngineError> {
    engine.practice_get_reference(reference_id)
}

#[tauri::command]
fn practice_rename_reference(engine: State<'_, Engine>, reference_id: String, name: String) -> Result<(), EngineError> {
    engine.practice_rename_reference(reference_id, name)
}

#[tauri::command]
fn practice_delete_reference(engine: State<'_, Engine>, reference_id: String) -> Result<(), EngineError> {
    engine.practice_delete_reference(reference_id)
}

#[tauri::command]
fn practice_get_attempt(
    engine: State<'_, Engine>,
    reference_id: String,
    attempt_id: String,
) -> Result<AttemptRecord, EngineError> {
    engine.practice_get_attempt(reference_id, attempt_id)
}

#[tauri::command]
fn practice_delete_attempt(engine: State<'_, Engine>, reference_id: String, attempt_id: String) -> Result<(), EngineError> {
    engine.practice_delete_attempt(reference_id, attempt_id)
}

/// Score a stored attempt again; `scoring` omitted = the current configuration.
#[tauri::command]
fn practice_rescore_attempt(
    engine: State<'_, Engine>,
    reference_id: String,
    attempt_id: String,
    scoring: Option<PracticeScoring>,
) -> Result<AttemptRecord, EngineError> {
    engine.practice_rescore_attempt(reference_id, attempt_id, scoring)
}

#[tauri::command]
fn practice_get_config(engine: State<'_, Engine>) -> Result<PracticeConfig, EngineError> {
    engine.practice_get_config()
}

#[tauri::command]
fn practice_set_config(engine: State<'_, Engine>, config: PracticeConfig) -> Result<PracticeConfig, EngineError> {
    engine.practice_set_config(config)
}

#[tauri::command]
fn practice_export_library(engine: State<'_, Engine>) -> Result<PracticeLibrary, EngineError> {
    engine.practice_export_library()
}

#[tauri::command]
fn practice_import_library(engine: State<'_, Engine>, library: PracticeLibrary, replace: bool) -> Result<(), EngineError> {
    engine.practice_import_library(library, replace)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let engine = Engine::start(Box::new(move |event| {
                // Runs on the engine control thread (never the audio callback).
                let _ = handle.emit(ENGINE_EVENT, event);
            }));
            // Hardware settings live in the app config dir; the engine only sees a path.
            // Applied now (MIDI ports reopen; the next start uses the saved devices).
            if let Ok(dir) = app.path().app_config_dir() {
                let _ = engine.use_settings_file(dir.join(voiceloop_engine::settings::SETTINGS_FILE_NAME));
            }
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_input_devices,
            start_input,
            stop_input,
            engine_status,
            looper_command,
            set_analysis_config,
            list_audio_devices,
            start_audio,
            set_input_route,
            set_output_route,
            set_monitor,
            list_midi_inputs,
            open_midi_input,
            close_midi_input,
            get_audio_settings,
            save_audio_settings,
            start_latency_calibration,
            startup_warnings,
            project_command,
            find_recovery,
            discard_recovery,
            practice_start_reference,
            practice_start_attempt,
            practice_stop,
            practice_cancel,
            practice_status,
            practice_list_references,
            practice_get_reference,
            practice_rename_reference,
            practice_delete_reference,
            practice_get_attempt,
            practice_delete_attempt,
            practice_rescore_attempt,
            practice_get_config,
            practice_set_config,
            practice_export_library,
            practice_import_library
        ])
        .build(tauri::generate_context!())
        .expect("error while building VoiceLoop");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            // Release audio devices and join the control thread.
            app.state::<Engine>().shutdown();
        }
    });
}
