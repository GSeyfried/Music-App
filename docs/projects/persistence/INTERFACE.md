# Persistence interface (UI ↔ engine)

Implement against this verbatim. All JSON is camelCase. Frames are sample
frames (`number` in TS; they stay below 2^53). Paths are absolute path
strings chosen by the user (see *Dialogs* below); the engine never invents
locations except inside a folder the UI gives it.

## Tauri commands

| Command | Args (invoke JSON) | Returns | Thread |
| --- | --- | --- | --- |
| `project_command` | `{ request: ProjectRequest }` | `ProjectReply` | engine control thread, returns at once |
| `find_recovery` | `{ dir: string }` | `RecoveryInfo \| null` | Tauri worker (small file reads) |
| `discard_recovery` | `{ dir: string }` | `null` | Tauri worker |
| `set_looper_config` | `{ config: Partial<LooperConfig> }` | `LooperConfig` (the result) | engine control thread |

Looper commands added (through the existing `looper_command { command }`):
`{ type: "addTrack" }` and `{ type: "removeTrack", track: number }`.

Errors from `project_command` are `EngineError` (`{ kind, message }`):
`busy` (another project operation runs), `invalid` (e.g. `save` before a
first `saveAs`, `start audio first`), `engineStopped`. Everything that
happens after the command returns is reported through events.

## Events

On the existing `engine://event` stream: `{ type: "project", payload: ProjectEvent }`.

- `progress`: at most ~10/s per operation, `fraction` 0..1 (worker stages only).
- `done`: exactly one per operation that did not fail, with the result and
  the refreshed `ProjectInfo`.
- `failed`: exactly one per failed operation, with a typed `FileError`.
- Autosaves produce `done` with `op: "autosave"` (show quietly, e.g. "Saved
  for recovery 12:03"); an autosave postponed because the user is
  recording produces no event and retries ~5 s later.

## TypeScript types

```ts
export type BitDepth = "int16" | "int24" | "float32";
export type FitMode = "trim" | "reject";

export interface RenderOptions {
  depth?: BitDepth;   // default "int24" (TPDF-dithered); "float32" is lossless
  loops?: number;     // loop repetitions to render, default 1
  stereo?: boolean;   // mixdown only: dual-mono 2-channel file, default false
}

export interface MidiExportOptions {
  sampleRate?: number;     // clock of the note frames: the `sampleRate` of pitch events. Default 48000
  originFrame?: number;    // frame that becomes bar 1 beat 1. Default 0
  ticksPerBeat?: number;   // default 480
  channel?: number;        // 0..15, default 0
  velocity?: number;       // default 96
  pitchBend?: boolean;     // per-note intonation as pitch bend (±2 semitones), default false
  trackName?: string;      // default "Voice"
}

/** NoteEvent exactly as received in `note` events (voiceloop-analysis). */
export type NoteEvent = import("./analysis").NoteEvent;

export type ProjectRequest =
  | { type: "new"; name: string }
  | { type: "save" }                                  // to ProjectInfo.path; `invalid` if never saved
  | { type: "saveAs"; path: string }                  // ".voiceloop" appended if missing
  | { type: "open"; path: string }                    // the package folder, or its project.json
  | { type: "exportMixdown"; path: string; options?: RenderOptions }
  | { type: "exportStems"; dir: string; options?: RenderOptions; perLayer?: boolean }
  | { type: "importAudio"; path: string; track: number; fit?: FitMode }
  | { type: "exportMidi"; path: string; notes: NoteEvent[]; options?: MidiExportOptions }
  | { type: "importMidi"; path: string; track: number }
  | { type: "exportReaper"; dir: string; options?: RenderOptions }  // <dir>/<name>.rpp + <dir>/Audio/*.wav
  | { type: "setRecovery"; dir: string | null; intervalSeconds: number | null } // min 5 s; null = no timer
  | { type: "autosave" }                              // now (e.g. on window blur); needs setRecovery
  | { type: "setMetadata"; name?: string | null; trackNames?: string[] | null }
  | { type: "info" };

export interface ProjectInfo {
  id: string;
  name: string;
  path: string | null;          // package folder once saved/opened; null for untitled or recovered sessions
  sampleRate: number;
  trackNames: string[];
  midiClips: MidiClip[][];      // per track
  pendingLoad: boolean;         // opened while audio was stopped, or loops parked by an audio restart
  unsavedChanges: boolean;      // loops, track list, names, MIDI or Practice Lab changed since save/open/new
  recoveryDir: string | null;
}

export interface ProjectReply {
  opId: number | null;          // set when the request started a background operation
  project: ProjectInfo;
}

export type OpKind =
  | "new" | "save" | "open" | "exportMixdown" | "exportStems"
  | "importAudio" | "exportMidi" | "importMidi" | "autosave"
  | "exportReaper"
  | "restore";        // loops parked at an audio restart are being put back

export interface FileError {
  kind:
    | "io" | "notFound" | "notRecognised" | "corrupt" | "unsupportedVersion"
    | "invalid" | "unsupported" | "tooLarge"
    | "busy"          // recording / transport running / content changed mid-copy: retry after stopping
    | "doesNotFit"    // more tracks/layers/length than the looper config allows
    | "audioStopped";
  message: string;
}

export interface TimeSignature { numerator: number; denominator: number }

export interface MidiNote {
  pitch: number; velocity: number;
  startFrame: number;      // relative to the clip start
  lengthFrames: number;
  channel: number;
}

export interface MidiClip {
  id: string;
  name: string;
  startFrame: number;
  lengthFrames: number;
  loopLengthFrames?: number;
  notes: MidiNote[];
  [unknown: string]: unknown;   // preserved fields
}

export type OpResult =
  | { type: "saved"; path: string }
  | { type: "opened"; path: string; warnings: string[]; loadedIntoLooper: boolean }
  | { type: "created" }
  | { type: "exported"; files: string[]; frames: number }
  | { type: "audioImported"; track: number; frames: number; trimmedFrames: number;
      paddedFrames: number; resampledFrom: number | null }
  | { type: "midiExported"; path: string; notes: number }
  | { type: "midiImported"; track: number; clip: MidiClip; bpm: number; timeSignature: TimeSignature }
  | { type: "autosaved"; savedAt: string }
  | { type: "restored"; resampledFrom: number | null; loadedIntoLooper: boolean };

export type ProjectEvent =
  | { kind: "progress"; opId: number; op: OpKind; fraction: number }
  | { kind: "done"; opId: number; op: OpKind; result: OpResult; project: ProjectInfo }
  | { kind: "failed"; opId: number; op: OpKind; error: FileError };

export interface RecoveryInfo {
  packagePath: string;          // open this with { type: "open", path }
  originalPath: string | null;  // the project it belonged to, if it had been saved
  projectName: string;
  savedAt: string;              // RFC 3339 UTC
  appVersion: string;
}
```

## Behaviour the UI must respect

- **One operation at a time.** While one runs, other requests get `busy`
  (except `info`, `setMetadata`, `setRecovery`). Disable file menu items
  between `opId` and its `done`/`failed`.
- **Transport must be stopped** for `new`, `open` (when audio runs),
  `importAudio`. Otherwise `failed` with `busy`. `save`, `saveAs`,
  exports and autosave work while loops play, but not while a track records
  or a stop is still collecting its latency tail (`busy`; retry).
- **Loops live in the running looper.** `exportMixdown`, `exportStems`,
  `exportReaper` and `importAudio` need audio started. With audio stopped,
  `save` writes the loops parked when audio stopped (see *Loops survive audio
  restarts*), else the project that was opened, else metadata only. Use
  `EngineStatus.unsavedChanges` to prompt before quitting.
- **Open while audio is stopped** completes with `loadedIntoLooper: false`
  and `pendingLoad: true`; the next `start_input`/`start_audio` loads it and
  emits a second `done` (op `open`, `loadedIntoLooper: true`) or `failed`.
- **Different sample rate:** a project saved at 48 kHz opened on a 44.1 kHz
  device is resampled on load; saving then writes 44.1 kHz layers.
- **Looper limits:** content longer than `maxLoopSeconds`, or more tracks or
  layers than the looper config, fails with `doesNotFit`. Raise the limits
  with `set_looper_config` (when exposed) and restart audio.
- **Import audio fit rules:** the file is averaged to mono and resampled to
  the engine rate. Empty project (no loop yet, Free mode): the file length
  becomes the loop length. Otherwise the loop length wins: shorter files are
  padded with silence (64-frame fade-out at the file end), longer files are
  trimmed with a 64-frame fade-out (`fit: "trim"`, default) or refused
  (`"reject"`). The file becomes a new layer on top of the track (any redo
  history on that track is discarded, as with recording).
- **MIDI export tempo:** written from the project transport; BPM is
  converted from the meter's beat unit to quarter notes (7/8 at 240 → 120).
  `notes` are the `note` events the UI collected; `originFrame` should be the
  loop start on the same clock if the UI wants bar alignment.
- **Recovery:** at start-up call `find_recovery({ dir })` with
  `<appDataDir>/recovery`. If it returns info, offer "Restore" (`open`
  `packagePath`, then prompt Save As, because `path` stays `null`) or
  "Discard" (`discard_recovery`). Then send `setRecovery` with the same dir and
  e.g. 60 s. Call `discard_recovery` after a clean quit and after a
  successful user save, so an old copy is never offered by mistake.

## Dialogs and folders

- Open/save pickers: `tauri-plugin-dialog` is installed (see *File dialogs*
  under Round 2 for the frontend calls).
- The recovery folder: `appDataDir()` from `@tauri-apps/api/path` (core
  API, permission `core:path:default`, already part of `core:default`)
  joined with `recovery`.
- No `fs` plugin is needed: the engine does all file I/O itself.
- Optional macOS polish: declare `.voiceloop` as a document package type
  (`LSTypeIsPackage`) in `Info.plist` so Finder shows it as one file.

## Round 2 additions

### Loops survive audio restarts

Changing device, sample rate or buffer, hot-plug, a stream error, `stop_input`,
and (on iOS) every route change tear the output stream down. The looper's
layers, redo history, gains, mute/solo/play state, transport, loop period
and latency are **parked** and put back into the next stream automatically:

- After the new stream starts, a `project` event `done` with `op: "restore"`,
  `result: { type: "restored", resampledFrom, loadedIntoLooper: true }`
  arrives (typically within tens of ms; seconds for long sessions at a new
  rate because of resampling).
- Until then `looper_command` returns `busy` ("restoring the loops after an
  audio restart"); `setInputLatency` is still accepted. Show a short
  "Restoring loops..." state, not an error.
- With audio stopped (`stop_input`), the parked loops stay in memory
  (`ProjectInfo.pendingLoad: true`) and `save` / `saveAs` write them.
- If the restore fails for a reason other than audio stopping again (for
  example the new looper config is smaller), a `failed` event (`op:
  "restore"`) arrives. The loops stay pending (not retried automatically)
  and `save` still writes them. Starting audio with a larger config loads them.
- A take that was recording when the stream died is closed at that point,
  exactly like Stop (input still in flight is lost with the stream).

### Unsaved changes and safe quit

- `EngineStatus.unsavedChanges: boolean` (also `ProjectInfo.unsavedChanges`).
  Set by recording, overdub, undo/redo, clear, add/remove track, renaming,
  MIDI or audio import, and Practice Lab changes. Cleared by `save`/`saveAs`
  (if nothing changed during the save), `open`, `new`.
- When the stream goes away with unsaved changes and a recovery folder is
  set (`setRecovery`), a recovery autosave runs automatically (`done`,
  `op: "autosave"`). On app exit the engine waits up to 10 s for it.
- Quit flow for the UI: on the window's close request (`onCloseRequested`
  from `@tauri-apps/api/window`, inside `bridge.ts`), if `unsavedChanges`,
  prevent the close and ask Save / Don't save / Cancel. "Don't save" may still
  leave a recovery copy; call `discard_recovery` if the user really wants none.

### Tracks: up to 16, added and removed live

- `LooperState.maxTracks: number`: the most tracks this looper can run
  (the `addTrack` ceiling, 16 by default). Matches the frontend's proposed name.
- `{ type: "addTrack" }` appends an empty track at once, with no stream restart.
  At the ceiling the looper answers `looperNotice { kind: "rejected", reason: "trackLimit" }`.
- `{ type: "removeTrack", track }` releases that track's layers and redo
  history, and later tracks move down one index. Rejected with `trackBusy`
  while it records, `trackLimit` for the last remaining track.
- New `RejectReason`: `"trackLimit"`.
- `set_looper_config({ config })` merges the given fields into the current
  config (others keep their values) and returns the result. `maxTracks` =
  tracks active at audio start; raising it while audio runs adds tracks
  immediately (same as `addTrack`). Lowering it applies at the next start,
  and existing loops are never dropped: a restore keeps the parked track count.
  New field `trackCapacity` (default 16, max 16) = preallocated tracks.
  **Recommendation:** use `looper_command { type: "addTrack" }` for the Add
  Track button. The frontend's current `setLooperConfig({ maxTracks: n + 1 })`
  also works.
- Project files store every track (including empty ones), and opening a
  project re-creates the same number of tracks.

### Timing-suspect takes

`TrackSnapshot.timingSuspect: boolean` is true when an input discontinuity
(bridge underrun, skip, or input overrun) happened while that track was
recording its newest take, so the take may be shifted in time. Suggest
re-recording (or re-calibrating latency). It clears when the next take on
that track starts, or on Clear.

### Practice Lab library in projects

Saved automatically with the project as `practice/library-<hash>.json`
(referenced from `project.json` `practice.library`). On `open` and `new`
the engine replaces the Practice Lab library itself (same as
`practice_import_library` with `replace: true`); the UI only needs to
refresh its reference list after the `done` event. A damaged library file
does not block opening: the project opens with a warning in
`result.warnings`.

### REAPER export

`{ type: "exportReaper", dir, options }` writes unity-gain stems to
`<dir>/Audio/<name> - <track>.wav` (same start and length, BWF time 0) and
`<dir>/<name>.rpp`. In the `.rpp` each track's item references its stem by
relative path. Track volume, mute and solo are on the REAPER tracks; tempo
(quarter-note BPM), time signature and a looping region over one loop are
set. `done` lists the `.rpp` first in `result.files`. Choose `dir` with
`open({ directory: true })`, preferably an empty or new folder.

### File dialogs (`tauri-plugin-dialog` 2.8, MIT OR Apache-2.0)

Rust side is done: crate added, plugin registered, capability permissions
`dialog:allow-open` and `dialog:allow-save` in `src-tauri/capabilities/default.json`.
Frontend: add the JS package **`@tauri-apps/plugin-dialog`** (v2) and import
it **only in `src/engine/bridge.ts`**:

```ts
import { open, save } from "@tauri-apps/plugin-dialog";

// Open a project (packages are folders on desktop)
const dir = await open({ directory: true, multiple: false, title: "Open project" });
if (typeof dir === "string") await projectCommand({ type: "open", path: dir });

// Save As
const path = await save({ title: "Save project", defaultPath: "Untitled.voiceloop" });
if (path) await projectCommand({ type: "saveAs", path });   // ".voiceloop" is appended if missing

// Exports
const mix = await save({ defaultPath: "Mixdown.wav", filters: [{ name: "WAV", extensions: ["wav"] }] });
const stemsDir = await open({ directory: true, title: "Folder for stems" });
const mid = await save({ defaultPath: "Melody.mid", filters: [{ name: "MIDI", extensions: ["mid"] }] });
const wav = await open({ multiple: false, filters: [{ name: "Audio", extensions: ["wav"] }] }); // importAudio
```

`save()` may return a path to a file that already exists (the user confirmed
replacing it). For Save As on desktop, a path ending in `.voiceloop` that
exists must be a VoiceLoop project, or the save fails with `invalid`.

**iOS limits (from the plugin's iOS source).** There is no folder picker.
`save()` creates an empty placeholder *file* in Documents and lets the user
export it, and the returned location may need security-scoped access. Neither
fits folder packages. On iOS: keep projects under the app's Documents folder
(the storage root the shell passes in) with an in-app project list, use
`open()` only to import single files (WAV, MIDI), and share exports via the
share sheet. Not tested on a device (PENDING).
