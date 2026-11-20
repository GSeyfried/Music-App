//! C interface to `voiceloop-engine` for native shells (VoiceLoop Pocket's
//! SwiftUI app). The same contract as the Tauri adapter (`src-tauri`):
//! commands by name with JSON arguments, results as JSON, and every engine
//! event as one JSON object `{ "type": ..., "payload": ... }` delivered to a
//! callback. No raw audio crosses this boundary.
//!
//! Header: `include/voiceloop.h`. Threading: `vl_engine_call` may be called
//! from any thread (the engine serialises commands on its control thread).
//! The event callback runs on the engine control thread, never the audio
//! callback; copy the string before returning.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};
use voiceloop_engine::mic::InputNeeds;
use voiceloop_engine::preview::PreviewRequest;
use voiceloop_engine::song::{SongRelease, SongSpec, SongTrigger};
use voiceloop_engine::{AnalysisConfig, AudioConfig, Engine, EngineError, LooperCommand};

/// Receives one engine event as a NUL-terminated UTF-8 JSON string.
pub type VlEventCallback = extern "C" fn(context: *mut c_void, event_json: *const c_char);

pub struct VlEngine {
    engine: Engine,
}

struct Context(*mut c_void);
// SAFETY: the context pointer is owned by the caller, who promises (header
// contract) that it stays valid until `vl_engine_destroy` returns and may be
// used from the engine control thread.
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

/// Start the engine. Events are delivered to `callback(context, json)`.
/// Returns null if `callback` is null or the engine failed to start (a panic is never
/// unwound across this boundary; `vl_engine_call` reports one as an `internal` error).
#[no_mangle]
pub extern "C" fn vl_engine_create(callback: Option<VlEventCallback>, context: *mut c_void) -> *mut VlEngine {
    let Some(callback) = callback else { return std::ptr::null_mut() };
    let context = Context(context);
    let started = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        Engine::start(Box::new(move |event| {
            let ctx = &context;
            if let Ok(text) = serde_json::to_string(&event) {
                if let Ok(c) = CString::new(text) {
                    callback(ctx.0, c.as_ptr());
                }
            }
        }))
    }));
    match started {
        Ok(engine) => Box::into_raw(Box::new(VlEngine { engine })),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Shut the engine down (releases audio devices) and free it.
///
/// # Safety
/// `engine` must come from `vl_engine_create` and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn vl_engine_destroy(engine: *mut VlEngine) {
    if engine.is_null() {
        return;
    }
    // SAFETY: per the contract above, the pointer is a live Box from vl_engine_create.
    let boxed = unsafe { Box::from_raw(engine) };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || boxed.engine.shutdown()));
}

/// Run a command. `args_json` may be null or `{}` for commands without
/// arguments. Returns a JSON string the caller frees with `vl_string_free`:
/// `{"ok": <result>}` or `{"error": <EngineError>}`.
///
/// # Safety
/// `engine` must be live; `command` and `args_json` (if non-null) must be
/// NUL-terminated UTF-8.
#[no_mangle]
pub unsafe extern "C" fn vl_engine_call(engine: *const VlEngine, command: *const c_char, args_json: *const c_char) -> *mut c_char {
    let reply = if engine.is_null() || command.is_null() {
        json!({ "error": { "kind": "invalid", "message": "null engine or command" } })
    } else {
        // SAFETY: checked non-null; validity is the caller's contract.
        let engine = unsafe { &(*engine).engine };
        let command = unsafe { CStr::from_ptr(command) }.to_string_lossy();
        let args: Value = if args_json.is_null() {
            Value::Object(Default::default())
        } else {
            let text = unsafe { CStr::from_ptr(args_json) }.to_string_lossy();
            serde_json::from_str(&text).unwrap_or(Value::Null)
        };
        // A Rust panic must never unwind into Swift/C: report it as an error instead.
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| dispatch(engine, &command, &args))) {
            Ok(Ok(v)) => json!({ "ok": v }),
            Ok(Err(e)) => json!({ "error": e }),
            Err(_) => json!({ "error": { "kind": "internal", "message": format!("engine panicked in `{command}`") } }),
        }
    };
    CString::new(reply.to_string()).map(CString::into_raw).unwrap_or(std::ptr::null_mut())
}

/// Free a string returned by `vl_engine_call`.
///
/// # Safety
/// `s` must come from `vl_engine_call` (or be null) and not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn vl_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: the pointer came from CString::into_raw in vl_engine_call.
        drop(unsafe { CString::from_raw(s) });
    }
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::Invalid(message.into())
}

/// A named argument, deserialized (same camelCase names as the Tauri commands).
fn arg<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, EngineError> {
    let v = args.get(name).cloned().unwrap_or(Value::Null);
    serde_json::from_value(v).map_err(|e| invalid(format!("argument `{name}`: {e}")))
}

fn ok<T: Serialize>(v: T) -> Result<Value, EngineError> {
    serde_json::to_value(v).map_err(|e| invalid(format!("result: {e}")))
}

/// The commands a native shell needs. Names match `src-tauri` (and
/// `src/engine/protocol.ts`), so both shells speak one contract.
fn dispatch(engine: &Engine, command: &str, args: &Value) -> Result<Value, EngineError> {
    match command {
        "engine_status" => ok(engine.status()?),
        "list_input_devices" => ok(engine.list_input_devices()?),
        "list_audio_devices" => ok(engine.list_audio_devices()?),
        "start_input" => ok(engine.start_input(arg::<Option<String>>(args, "deviceId")?)?),
        "stop_input" => ok(engine.stop_input()?),
        "start_audio" => ok(engine.start_audio(arg::<AudioConfig>(args, "config")?)?),
        "set_analysis_config" => ok(engine.set_analysis_config(arg::<AnalysisConfig>(args, "config")?)?),
        "set_input_needs" => ok(engine.set_input_needs(arg::<InputNeeds>(args, "needs")?)?),
        "request_mic_permission" => ok(engine.request_mic_permission()?),
        "startup_warnings" => ok(engine.startup_warnings()?),
        "looper_command" => ok(engine.looper(arg::<LooperCommand>(args, "command")?)?),
        "preview_play" => ok(engine.preview_play(arg::<PreviewRequest>(args, "request")?)?),
        "preview_stop" => ok(engine.preview_stop()?),
        // Pocket Song Builder (voiceloop_engine::song).
        "song_set" => ok(engine.song_set(arg::<SongSpec>(args, "song")?)?),
        "song_play" => ok(engine.song_play(arg::<Option<f64>>(args, "countInBars")?.unwrap_or(0.0))?),
        "song_stop" => ok(engine.song_stop()?),
        "song_trigger" => ok(engine.song_trigger(arg::<SongTrigger>(args, "trigger")?)?),
        "song_release" => ok(engine.song_release(arg::<SongRelease>(args, "release")?)?),
        "song_locate" => ok(engine.song_locate(
            arg::<Option<u64>>(args, "inputFrame")?,
            arg::<Option<f64>>(args, "extraMs")?.unwrap_or(0.0),
        )?),
        // Native shells pass their own paths (app support dir, bundle resources).
        "use_settings_file" => ok(engine.use_settings_file(PathBuf::from(arg::<String>(args, "path")?))?),
        "use_preview_samples" => ok(engine.use_preview_samples(PathBuf::from(arg::<String>(args, "path")?))?),
        other => Err(invalid(format!("unknown command `{other}`"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static EVENTS: AtomicUsize = AtomicUsize::new(0);
    extern "C" fn count(_ctx: *mut c_void, json: *const c_char) {
        let text = unsafe { CStr::from_ptr(json) }.to_string_lossy();
        assert!(text.starts_with("{\"type\":"));
        EVENTS.fetch_add(1, Ordering::Relaxed);
    }

    fn call(e: *const VlEngine, cmd: &str, args: Option<&str>) -> Value {
        let c = CString::new(cmd).unwrap();
        let a = args.map(|a| CString::new(a).unwrap());
        let raw = unsafe { vl_engine_call(e, c.as_ptr(), a.as_ref().map_or(std::ptr::null(), |a| a.as_ptr())) };
        let text = unsafe { CStr::from_ptr(raw) }.to_string_lossy().into_owned();
        unsafe { vl_string_free(raw) };
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn status_unknown_commands_and_bad_arguments() {
        let e = vl_engine_create(Some(count), std::ptr::null_mut());
        assert!(!e.is_null());
        let status = call(e, "engine_status", None);
        assert!(status["ok"]["capturing"].is_boolean(), "{status}");
        let unknown = call(e, "nope", None);
        assert!(unknown["error"].is_object(), "{unknown}");
        let bad = call(e, "set_input_needs", Some("{\"needs\": 5}"));
        assert!(bad["error"].is_object(), "{bad}");
        let needs = call(e, "set_input_needs", Some("{\"needs\": {\"reasons\": []}}"));
        assert!(needs["ok"].is_object(), "{needs}");
        unsafe { vl_engine_destroy(e) };
        assert!(vl_engine_create(None, std::ptr::null_mut()).is_null());
    }
}
