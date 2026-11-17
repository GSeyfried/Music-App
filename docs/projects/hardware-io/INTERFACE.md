# Hardware I/O: frontend interface

Exact contract for `src/engine/bridge.ts`. All JSON is camelCase. Tauri maps
camelCase argument keys to the Rust snake_case parameters (`portId` → `port_id`).
Every command rejects with an `EngineError` (`{ kind, message? }`).

## Commands

| Command | Arguments | Returns | Notes |
| --- | --- | --- | --- |
| `list_audio_devices` | none | `DeviceList` | Inputs **and** outputs with supported configs. Also the explicit re-scan. Slow-ish (tens of ms per device on CoreAudio); call on demand, not on a timer. |
| `start_audio` | `{ config: AudioConfig }` | `EngineStatus` | Start or restart audio. The config becomes the default for `start_input` and is saved to the settings file. |
| `start_input` | `{ deviceId?: string \| null }` | `EngineStatus` | **Unchanged.** Uses the current/saved `AudioConfig` with `inputDevice` replaced by `deviceId`. |
| `stop_input` | none | `EngineStatus` | Unchanged. |
| `set_input_route` | `{ route: InputRoute }` | `EngineStatus` | Live while running (no restart, no gap); remembered and saved. |
| `set_output_route` | `{ route: OutputRoute }` | `EngineStatus` | Live while running; remembered and saved. |
| `set_monitor` | `{ monitor: MonitorConfig }` | `EngineStatus` | Live (gain slews over ~11 ms); remembered and saved. Also emits `status`. |
| `list_midi_inputs` | none | `MidiPortInfo[]` | |
| `open_midi_input` | `{ portId: string }` | `MidiPortInfo` | `portId` may also be a port name. Opening an open port is a no-op. Remembered: reopens after unplug/replug and at next app start. |
| `close_midi_input` | `{ portId: string }` | `null` | Error `invalid` if it is not open. Forgets the port. |
| `get_audio_settings` | none | `AudioSettings` | What is in effect (and on disk). |
| `save_audio_settings` | none | `null` | Settings are auto-saved after every change; this forces a write. |
| `looper_command` | `{ command: { type: "setInputLatency", frames } }` | `null` | **Changed:** now accepted while audio is stopped; the value is remembered, re-applied on every start, and saved. |

Settings are loaded automatically at app start from
`<app config dir>/audio-settings.json`: saved MIDI ports open immediately,
and the next `start_input`/`start_audio` uses the saved devices (falling back
to system defaults with a `deviceFallback` warning when one is missing).
Audio is **not** started automatically (microphone permission prompt).

## Types

```ts
export type DeviceDirection = "input" | "output";

export interface ConfigRange {
  channels: number;
  minSampleRate: number;
  maxSampleRate: number;
  sampleFormat: string;            // "f32", "i16", ...
  minBufferSize: number | null;    // frames, null when the backend does not say
  maxBufferSize: number | null;
}

export interface StreamFormat { sampleRate: number; channels: number; sampleFormat: string; }

export interface DeviceInfo {
  id: string;                      // pass back as inputDevice/outputDevice (currently = name, D-008)
  name: string;
  host: string;                    // "CoreAudio"
  direction: DeviceDirection;
  isDefault: boolean;
  defaultFormat: StreamFormat | null;
  maxChannels: number;
  sampleRates: number[];           // common rates the device supports, ascending
  configs: ConfigRange[];
}

/** A device with both directions (e.g. Zoom H4n Pro) appears in both lists. */
export interface DeviceList { host: string; inputs: DeviceInfo[]; outputs: DeviceInfo[]; }

/** Channels are zero-based (H4n "input 3" is channel 2). */
export type InputRoute =
  | { type: "mixAll" }                                   // average of all channels (default; right for mono mics)
  | { type: "mono"; channel: number }                    // one channel, unchanged
  | { type: "stereoSum"; left: number; right: number };  // (L + R) / 2

export type OutputRoute =
  | { type: "pair"; left: number; right: number }        // default { left: 0, right: 1 }
  | { type: "mono"; channel: number }
  | { type: "all" };

export interface MonitorConfig { enabled: boolean; gain: number; }   // gain linear 0..2, default { false, 1 }

export type MissingDevicePolicy = "fail" | "useDefault";

/** Every field optional on input; omitted = default. */
export interface AudioConfig {
  inputDevice: string | null;      // null = system default
  outputDevice: string | null;     // null = system default
  missingDevice: MissingDevicePolicy;   // default "fail"
  sampleRate: number | null;       // null = negotiate (see below)
  bufferSize: number | null;       // frames, null = device default; clamped to 16..4096 and the device range
  inputRoute: InputRoute;          // default mixAll
  outputRoute: OutputRoute;        // default pair 0/1
  monitor: MonitorConfig;          // default off
}

export interface AudioSettings {
  version: 1;
  audio: AudioConfig;
  midiInputs: string[];            // port names
  inputLatencyFrames: number | null;
}

export interface CaptureInfo {
  deviceId: string;
  sampleRate: number;
  channels: number;                // all are metered; the route picks what the engine uses
  sampleFormat: string;
  route: InputRoute;               // as applied (may differ from requested; see configAdjusted)
  requestedBufferSize: number | null;
  blockFrames: number | null;      // actual frames per callback (null until the first)
  reportedLatencyFrames: number | null;  // backend estimate; CPAL 0.16/CoreAudio = one buffer
  discontinuities: number;         // xruns detected from callback timestamps
  lostFrames: number;
  nonFiniteSamples: number;        // NaN/Inf samples replaced by silence
}

export interface OutputInfo {
  deviceId: string;
  sampleRate: number;
  channels: number;
  sampleFormat: string;
  route: OutputRoute;
  monitor: MonitorConfig;
  requestedBufferSize: number | null;
  blockFrames: number | null;
  reportedLatencyFrames: number | null;
  discontinuities: number;
  lostFrames: number;
}

export interface EngineStatus {
  capturing: boolean;
  monitoring: boolean;             // monitor enabled and the output stream is running
  capture: CaptureInfo | null;
  output: OutputInfo | null;
  looper: boolean;
  analysis: boolean;
  audioConfig: AudioConfig;        // what start_input will use
}

export interface ChannelLevel { rmsDb: number; peakDb: number; clipped: boolean; }

/** `level` payload. rms/peak are now the ROUTED signal (what is recorded). */
export interface LevelReading {
  rms: number; peak: number; rmsDb: number; peakDb: number;
  channels: ChannelLevel[];        // every device input channel (max 32), index = channel
  frame: number; dropped: number;
}

export interface MidiPortInfo { id: string; name: string; open: boolean; }

/** channel is 0..15 (MIDI channel 1 = 0). */
export type MidiMessage =
  | { kind: "noteOn"; channel: number; note: number; velocity: number }
  | { kind: "noteOff"; channel: number; note: number; velocity: number }   // also note-on velocity 0
  | { kind: "controlChange"; channel: number; controller: number; value: number }  // never 64
  | { kind: "sustain"; channel: number; on: boolean; value: number }       // CC 64
  | { kind: "pitchBend"; channel: number; value: number }                  // -8192..8191
  | { kind: "programChange"; channel: number; program: number }
  | { kind: "channelPressure"; channel: number; pressure: number }
  | { kind: "polyPressure"; channel: number; note: number; pressure: number };

export type MidiEvent = MidiMessage & {
  portId: string;
  hostTimeNs: number;              // engine host clock (ns since engine start)
  outputFrame: number | null;      // output-stream frame being processed then (audio running)
  transportFrame: number | null;   // looper transport frame then (frozen while stopped)
  inputFrame: number | null;       // input clock (same clock as level/pitch/onset frames)
};

export interface MidiBatch { events: MidiEvent[]; dropped: number; }

export interface DevicesChanged { names: string[]; added: string[]; removed: string[]; }

export interface AudioDiagnostics {
  capture: CaptureInfo | null;
  output: OutputInfo | null;
  driftPpm: number | null;         // input clock vs output clock; null until ~12 s or without looper
  driftWindowSeconds: number | null;
}
```

## Events (`engine://event`, `{ type, payload }`)

| `type` | `payload` | When |
| --- | --- | --- |
| `level` | `LevelReading` | ~30 Hz while capturing. **Changed:** `rms/peak` = routed signal; new `channels`. |
| `status` | `EngineStatus` | **Changed:** new fields above. |
| `devicesChanged` | `DevicesChanged` | Audio device set changed (polled every 2 s). Re-call `list_audio_devices` if needed. |
| `midiPorts` | `MidiPortInfo[]` | MIDI port list changed (polled every 2 s). |
| `midi` | `MidiBatch` | MIDI received; ≤ 100 Hz batches while a port is open. |
| `audioDiagnostics` | `AudioDiagnostics` | 1 Hz while audio runs. |
| `error` | `EngineError` | New kind `deviceLost` (active device unplugged; audio has been stopped and a `status` follows). |
| `warning` | `{ kind, message }` | New kinds below. |

New `EngineError.kind` values: `deviceLost`, `midi`, `io` (settings file).
`deviceNotFound` now also covers output devices and MIDI ports.

New `warning` kinds: `deviceFallback` (saved/requested device or MIDI port
missing, default used), `configAdjusted` (route or buffer size did not fit the
device), `midiPortLost` (open port unplugged; reopens automatically),
`settingsProblem` (settings file unreadable/unwritable).

## Sample-rate negotiation (`sampleRate: null`)

Output default rate if the input supports it → else the input default if the
output supports it → else 48 000, 44 100, 96 000, 88 200 (first both support)
→ else no common rate: metering only, `looperUnavailable` warning. A requested
rate the **output** cannot run is an `unsupportedFormat` error; one only the
input cannot run gives metering-only. Never assume 48 kHz in the UI: read
`status.capture.sampleRate` / `status.output.sampleRate`.

## UI guidance

- Input picker: list `inputs`; for `maxChannels > 1` offer a route picker
  (Mix all / Channel N / Pair L+R) and show `level.channels` as a strip of
  per-channel meters so the user sees which input carries signal.
- Output picker: list `outputs`; for `maxChannels > 2` offer the pair picker.
- Monitoring toggle: off by default; warn "use headphones" before enabling
  (speaker monitoring with a mic feeds back).
- Show `audioDiagnostics.output.discontinuities`/`capture.discontinuities`
  growing and `|driftPpm| > 20` as a "timing not sample-tight" hint.
