//! Tauri shell: exposes engine commands to the frontend and forwards engine
//! events as a single `engine://event` stream. No audio processing happens
//! here, and no raw samples cross the bridge: only ~30 Hz level readings and
//! looper state, status changes, notices, and errors.

use tauri::{Emitter, Manager, RunEvent, State};
use voiceloop_engine::model::AudioDevice;
use voiceloop_engine::{AnalysisConfig, Engine, EngineError, EngineStatus, LooperCommand};

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let engine = Engine::start(Box::new(move |event| {
                // Runs on the engine control thread (never the audio callback).
                let _ = handle.emit(ENGINE_EVENT, event);
            }));
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_input_devices,
            start_input,
            stop_input,
            engine_status,
            looper_command,
            set_analysis_config
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
