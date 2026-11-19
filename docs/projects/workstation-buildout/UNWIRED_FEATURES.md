# Workstation vertical-integration audit

Audit date: 2026-11-19

This inventory distinguishes capabilities that already exist in code from partial foundations and product ideas that have not been implemented. It reflects the current working tree, including the workstation-buildout changes that have not yet been integrated into `main`.

For roadmap decisions, evaluate every feature across six stages: native/DSP,
Rust service and state, Tauri commands/events, React protocol and state,
user-accessible controls and feedback, and functional/hardware verification.
Passing an early stage does not imply completion of the later stages.

## Vertical integration summary

| Capability | Native/DSP | Rust service | Tauri | React | UI | Verification |
| --- | --- | --- | --- | --- | --- | --- |
| Studio looping and current track controls | Implemented | Implemented | Implemented | Implemented | Implemented | Automated; real recording/timing pending |
| Hardware routing, monitoring, calibration and MIDI ports | Implemented | Implemented | Implemented | Implemented | Implemented | Automated/partial built-in Mac; external hardware pending |
| Live pitch/note/onset analysis | Implemented | Implemented | Implemented | Implemented | Implemented | Synthetic; recorded voices pending |
| Practice reference/attempt/score/history | Implemented | Implemented | Implemented | Implemented | Implemented | Automated end to end; real voice pending |
| Projects, recovery and interchange | Implemented | Implemented | Implemented | Implemented | Implemented | Automated plus packaged UI/dialog launch; real save/recovery/DAW flow pending |
| Dynamic Add Track / 10+ tracks | Implemented | Implemented | Implemented | Implemented | Implemented | Automated up to 16; real-session stress pending |
| Realtime filter chains, presets and freeze | Implemented | Implemented | Implemented | Implemented | Implemented in PLAY/PRODUCE | Automated; native listening/stereo pending |
| Offline per-layer noise reduction | Implemented | Implemented | Implemented | Implemented | Implemented in Track Details | Automated; real room/voice listening pending |
| Audio arrangement and bounce | Implemented | Implemented | Implemented | Implemented | Implemented in PRODUCE | Automated; native listening/export flow pending |
| Instruments and percussion | Metronome/preview only | Missing | Missing | Partial preview tools | Missing as tracks | Not implemented as a workflow |

## Contract mismatches resolved in the current working tree

- TypeScript now mirrors `EngineStatus.unsavedChanges` and
  `PitchUpdate.transportMap`.
- The event parser accepts `project`, `practiceState`, `practiceProgress`, and
  `practiceTake`; project and Practice stores consume them.
- `EngineCommands` includes the project and Practice command sets.
- Protocol tests enumerate every current event tag so new drift fails visibly.

## Newly wired through React

### Project lifecycle and persistence

The native command surface supports:

- new, open, save, and Save As;
- project metadata;
- autosave and recovery configuration;
- finding and discarding recovery data;
- progress, completion, and failure events for project operations.

The underlying project package is crash-safe and stores loop audio and session
state. `src/project/` now supplies the typed contract, external store, recovery
startup/quit handling, recent-project list, operation progress and Project
panel. Native filesystem choices stay in `src/engine/bridge.ts`.

### Audio, MIDI, stem, and DAW interchange

Native project commands support:

- audio import with offline sample-rate conversion;
- WAV mixdown export;
- track stems and per-layer stems;
- Standard MIDI File import and export;
- REAPER project export.

These flows are exposed in the Project panel. MIDI file import currently
provides imported clip/data infrastructure; MIDI clip playback is not
implemented and should not be presented as available.

### Practice Lab workflow

The backend has a substantial Practice Lab command surface:

- record and manage references;
- record, cancel, retrieve, delete, and rescore attempts;
- pitch, intonation, and timing scoring;
- feedback, history, trends, and configuration;
- library import/export.

The Practice Lab React workspace now provides the reference library,
reference/attempt capture, live expected-versus-sung overlay, scored results,
history, trends and rescoring controls. Real voice and timing validation remains.

## Implemented below the Tauri boundary, with no UI or command wiring

### Looper resource configuration

Rust can configure looper track and layer pools, loop duration, and memory
limits. Tauri exposes config changes, while the main Studio workflow uses
realtime-safe `addTrack`/`removeTrack` commands up to the preallocated 16-track
capacity. Pool size and duration remain advanced, pre-start settings without a
general UI.

### Analysis and music-theory utilities

The analysis crate includes reusable capabilities for intervals, scales, key estimation, chord identification, rhythm/timing analysis, tempo estimation, melody comparison, and note-to-MIDI conversion. Some analysis is used internally by tuning, Practice Lab, or persistence paths, but these capabilities do not have standalone engine commands or UI tools.

Treat these as useful foundations, not finished end-user features.

## Partially wired capabilities

### Metronome detail controls

The transport model supports metronome gain and downbeat accent behavior, but the current transport UI exposes only the main metronome state, subdivision, and count-in controls. Gain and accent controls still need UI.

### MIDI input beyond port management

Settings can list, open, and close MIDI input ports, and the engine can receive MIDI input. The app does not yet turn those messages into instrument performance, transport mappings, loop recording, or user-configurable control bindings.

### Tracks and layers

The Studio UI exposes live add/remove plus recording, overdubbing, undo and
redo. Track Details exposes per-layer timing/nudge and noise reduction, and
persistence/export preserve layers. Nondestructive trim, audition/mute,
reorder, promotion, comping and a full take browser remain missing.

### Hardware settings verification

Input/output selection, routes, monitoring, sample rate, buffer size, latency calibration, and MIDI port management now have Settings UI in the current working tree. Their presence should no longer be described as “no UI,” but physical-device behavior still needs hardware verification on the target Mac.

## Desired, but not implemented yet

Do not describe these as hidden features:

- nondestructive looper-layer trim, comping and warp editing;
- harmonization, native instruments, pitch correction, stereo effects and automation;
- per-layer mute, reorder, promotion, bounce, and deeper editing;
- gesture bindings and rebindable hotkeys;
- audible MIDI clip playback.
