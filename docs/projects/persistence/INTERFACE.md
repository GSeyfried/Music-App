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
  pendingLoad: boolean;         // opened while audio was stopped; loads into the looper when audio starts
  recoveryDir: string | null;
}

export interface ProjectReply {
  opId: number | null;          // set when the request started a background operation
  project: ProjectInfo;
}

export type OpKind =
  | "new" | "save" | "open" | "exportMixdown" | "exportStems"
  | "importAudio" | "exportMidi" | "importMidi" | "autosave";

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
  | { type: "autosaved"; savedAt: string };

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
- **Loops live in the running looper.** `exportMixdown`, `exportStems` and
  `importAudio` need audio started. `save` with audio stopped writes the
  project that was opened (if any) or metadata only. `stop_input` discards
  the looper's audio, so prompt "Save first?" before stopping audio or
  quitting when there are loops.
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

## Dialogs (integration step, not done here)

No Tauri plugins were added. The UI integration needs:

- `tauri-plugin-dialog` (MIT/Apache-2.0) for open/save pickers, with the
  capability permissions `dialog:allow-open` and `dialog:allow-save` in
  `src-tauri/capabilities/default.json`. Packages are folders: use
  `open({ directory: true })` for Open, `save({ defaultPath: "Song.voiceloop" })`
  for Save As. Stems use `open({ directory: true })`.
- The recovery folder: `appDataDir()` from `@tauri-apps/api/path` (core
  API, permission `core:path:default`, already part of `core:default`)
  joined with `recovery`.
- No `fs` plugin is needed: the engine does all file I/O itself.
- Optional macOS polish: declare `.voiceloop` as a document package type
  (`LSTypeIsPackage`) in `Info.plist` so Finder shows it as one file.
