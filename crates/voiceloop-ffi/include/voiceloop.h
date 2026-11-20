// C interface to the VoiceLoop audio engine (crates/voiceloop-ffi).
// JSON commands in, JSON events out. See src/lib.rs for the contract.
#ifndef VOICELOOP_H
#define VOICELOOP_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct VlEngine VlEngine;

/// Receives one engine event: {"type": ..., "payload": ...} (UTF-8 JSON).
/// Runs on the engine control thread; copy `event_json` before returning.
typedef void (*VlEventCallback)(void *context, const char *event_json);

/// Start the engine. `context` must stay valid until vl_engine_destroy returns.
/// Returns NULL if the engine failed to start. Rust panics never unwind across
/// this interface; vl_engine_call reports one as {"error":{"kind":"internal",...}}.
VlEngine *vl_engine_create(VlEventCallback callback, void *context);

/// Shut down (releases audio devices) and free the engine.
void vl_engine_destroy(VlEngine *engine);

/// Run a command by name with JSON arguments (may be NULL). Returns
/// {"ok": result} or {"error": EngineError}; free with vl_string_free.
char *vl_engine_call(const VlEngine *engine, const char *command, const char *args_json);

void vl_string_free(char *s);

#ifdef __cplusplus
}
#endif

#endif
