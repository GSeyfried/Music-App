# VoiceLoop product roadmap

Audit date: 2026-11-17

This roadmap describes direction, not fixed commitments. The repository is the
source of truth: a capability is user-complete only when its native processing,
Rust service/state, Tauri contract, React integration, accessible controls and
feedback, and relevant functional or hardware verification are all complete.

The current interfaces are generally satisfactory. Milestones should extend
the existing components and layouts unless user testing or a technical
constraint demonstrates that structural change is necessary.

## Product direction

VoiceLoop is growing into a modular, local-first music workstation whose
workspaces share audio, MIDI, musical analysis, and project infrastructure:

| Workspace | Direction | Current reality |
| --- | --- | --- |
| **Studio** | Multitrack recording, live looping, mixing, effects, automation and editing | Current primary workflow: looping, overdubbing, transport, basic track mixing, hardware settings and live pitch are UI-accessible. Advanced editing/effects are absent. |
| **Practice Lab** | Pitch, ear and rhythm training with real-time feedback and history | Live tuner/curve and reference, attempt, scoring, history and library workflows are accessible; real-voice validation remains. |
| **Composition** | MIDI editing, notation, harmonic analysis, transcription and arrangement | Analysis, theory, MIDI-file and project-data foundations exist. There is no composition workspace, MIDI playback/editor, notation or MusicXML. |
| **Live Performance** | Instrument routing, monitoring, looping and digital instruments | Audio routing, monitoring, looping and MIDI input management exist. Instruments, mappings and performance-safe hardware validation do not. |
| **Music Library** | Projects, recordings, samples, scores, presets and practice material | Project save/open, recovery, recent-project access and interchange are exposed in the current Project panel; a broader searchable library is absent. |
| **VoiceLoop Pocket** | Simplified offline iPhone/iPad capture, layering and practice | Feasibility and touch guidance exist. No on-device Tauri/audio proof has run because full Xcode and device validation are still required. |

The accepted Rust engine, Tauri 2 shell and React/TypeScript UI remain the
baseline (D-001). Revisit that decision only at its documented technical
triggers, not merely because later features are ambitious.

## End-to-end implementation audit

Legend: **I** implemented, **P** partial, **—** absent, **A** automated
verification, **M** manual/hardware verification still required.

| Vertical capability | Native/DSP | Rust service/state | Tauri | React | User controls/feedback | Verification |
| --- | --- | --- | --- | --- | --- | --- |
| Device capture and level meter | I | I | I | I | I | Prior Mac capture verification; retest after CPAL 0.18.2 migration |
| Free/synced looping, count-in, transport, record/overdub | I | I | I | I | I | A; M for timing, playback and real recording |
| Track gain, play, mute, solo, undo/redo and clear | I | I | I | I | I | A; native UI launch only in latest handoff |
| Hardware routes, monitoring and calibration | I | I | I | I | I | A; partial built-in Mac run; H4n and cable loopback M |
| MIDI input and port management | I | I | I | I | I | A including virtual CoreMIDI; physical keyboard M |
| Live pitch, notes and onsets | I | I | I | I | I | A on synthetic signals; recorded voices/noisy rooms M |
| Practice reference/attempt/scoring/history | I | I | I | I | I | A; real voice/timing M |
| Project save/open/metadata/autosave/recovery | I | I | I | I | I | A and packaged UI/dialog launch; real save/recovery M |
| WAV import, mixdown, track/per-layer stems | I | I | I | I | I | A; live and external-DAW flow M |
| MIDI file import/export | I | I | I | I | I | A for files/UI; imported MIDI playback absent |
| REAPER project export | I | I | I | I | I | A for generation/UI; REAPER opening M |
| Looper track/layer resource configuration | I, fixed pool | I | I | I | I | A; fixed 16-track capacity |
| Dynamic Add Track and 10+ tracks | I | I | I | I | I | A up to 16; real-session stress M |
| Metronome gain/downbeat accent | I | I | I | I model | — | A below UI |
| Theory/key/scale/chord/rhythm utilities | I library | P, some Practice use | — standalone | — | — | A on known notes; not mixed-audio chord recognition |
| Internal instruments and percussion | Metronome click only | — | — | — | — | Not implemented |
| Arrangement, MIDI editing and notation | P data foundations | P project data | P file commands | — | — | Not implemented as a workflow |
| iOS/iPadOS application | P portability research | P compile-level engine | — on device | P responsive/touch foundation | — | Blocked on Xcode/device proof |
| Hands-free mobile commands | — | — | — | — | — | Exploratory |

Detailed evidence and integration gaps are maintained in
[the workstation integration audit](projects/workstation-buildout/UNWIRED_FEATURES.md).

## Architecture and product requirements

### Live audio and looping — highest priority

- Preserve independent input-recording and output-playback paths. Recorded
  audio must come from the selected input route, never the application output
  mix. Live monitoring remains opt-in and defaults off; normal loop playback
  remains on when transport is playing.
- Distinguish **software loopback** from **acoustic microphone bleed**. The
  existing routing prevents accidental software loopback. Speaker sound can
  still re-enter a microphone acoustically; headphones and placement are the
  reliable controls. Acoustic echo cancellation may be investigated for
  convenience, but must not be presented as perfectly removing musical bleed.
- Keep the output device frame clock authoritative, absolute-grid timing, and
  preallocated callback design. Quantized recording, count-in, BPM, meter and
  free-time looping already share this engine.
- Complete hardware validation for sample accuracy, two-device clock drift,
  automatic latency calibration, xruns, hot-plug, and loop preservation across
  stream restarts before expanding the engine broadly.
- Separate input ownership, transport, and playback lifecycle. Pause must not
  discard the session or return to the initial Start Input experience.
- Expose clear track/take states, including muted or armed states. Future track
  growth must preserve the existing track/layer distinction and nondestructive
  history described in
  [PRODUCT_DIRECTION.md](projects/workstation-buildout/PRODUCT_DIRECTION.md).

### Pitch and Practice Lab

- Preserve explicit confidence and unpitched states; never hold a stale note as
  if it were current. Tune smoothing and hysteresis against recorded voices,
  breathiness, vibrato and room noise rather than synthetic tones alone.
- Add vocal-range discovery, target-note matching, accuracy history and
  practice tracking incrementally through the existing analysis and Practice
  services.
- Custom note colours and optional pitch-reactive themes are presentation
  extensions. They must be user-controlled, smoothly interpolated and protected
  from rapid flicker. They do not outrank completing the Practice workflow.
- Chord identification currently operates on known note sets. Polyphonic chord
  recognition from mixed audio is a separate research problem and must not be
  inferred from the existing monophonic detector.

### Tracks, mixing and routing

- A consistent future track model needs dB volume, mute, solo, record arm,
  input/output route, monitoring, meters and master output. Current tracks have
  gain, mute, solo and loop controls, but routing is global and audio is mono.
- Default loop/project playback is audible; default live input monitoring is
  off. Enabling monitoring must continue to warn about headphones/feedback.
- True stereo layers, per-track inputs, pan, buses, sends, effect chains and
  automation are planned extensions, not current capabilities. Use the staged
  design in `hardware-io/STEREO_AND_PER_TRACK_INPUT.md` and avoid premature
  generalized routing graphs.
- The product target of at least 10 tracks is represented by live add/remove
  up to a preallocated 16-track capacity. Real-session CPU and memory stress
  still needs measurement before that ceiling is treated as production-safe.

### Instruments, percussion and composition

- Introduce instruments only after defining a realtime-safe instrument/track
  boundary that can serve computer input, physical MIDI and future sequencing.
  Initial synthesis may include polyphony, basic oscillators, ADSR, filters and
  a virtual keyboard; audio-to-MIDI, pitch shifting, correction and harmony are
  later research/processing features.
- Build percussion as sample-based instruments on the same instrument/MIDI
  infrastructure: pads, velocity, user samples and a step sequencer first;
  percussion transcription, notation and polyrhythm training later.
- Reuse current MIDI, theory, project and analysis foundations for composition.
  A first composition slice should prove MIDI playback and editing before a
  full piano roll or notation system.
- MusicXML, score following, PDF/image score conversion and polyphonic
  transcription are exploratory. AI-assisted conversion must remain optional
  and cannot compromise the local/offline core.
- Interoperability targets standard WAV, MIDI, MusicXML where implemented, and
  aligned stems. REAPER export may be refined, but lossless compatibility with
  proprietary GarageBand, Logic or other DAW project formats is not promised.

### VoiceLoop Pocket

- Reuse portable Rust analysis, looper and project code where feasible; isolate
  AVAudioSession, permissions, interruptions, route changes, storage and
  background behavior behind a platform boundary.
- Core mobile recording, looping, pitch analysis, playback, save and reopen
  must work in Airplane Mode. Optional desktop synchronization is a separate,
  explicit feature.
- Initial hands-free commands are: New song, Start recording, Finish loop, Add
  a layer, Play it back, Pause, Mute track, Undo, Stop everything and Save song.
  Recognition must be on-device or clearly unavailable; it must never silently
  depend on a network service.
- The intended driving workflow requires a safety review. No core action may
  require visual attention. Siri/Shortcuts, background audio, screen lock,
  Bluetooth/automotive routes and on-device speech recognition all require
  physical-device feasibility tests before commitment.
- Initial acceptance: on a physical iPhone or iPad in Airplane Mode, record,
  loop, overdub, play, undo, save, terminate, and reopen without data loss.

## Prioritized milestones

Each milestone is intentionally a small vertical slice and includes its UI and
verification. Starting unrelated backend systems does not satisfy completion.

### 1. Stabilize the existing Studio on real hardware

Scope: reconcile Rust/TypeScript contracts; verify capture/transport/playback
lifecycle; built-in devices and Zoom H4n Pro; physical MIDI keyboard; monitoring
off by default; loopback calibration; two-clock drift and restart preservation.

Acceptance:

- Rust events and fields currently missing from TypeScript are represented and
  regression-tested without changing the visible layout.
- Pause/resume retains the working session and microphone state; stopping or a
  device restart does not silently discard loops.
- Free and synced loops, overdubs, undo/redo, mute/solo and calibration pass the
  hardware procedures in the project docs.
- H4n routing/channel/rate behavior and keyboard timing are recorded with
  device/OS versions; limitations remain explicit.
- Required Mac checks pass and the native app is relaunched and exercised.

### 2. Complete project safety and interchange UI

Use the existing project service and current UI shell. Add the minimum React
contract, save/open/new flows, unsaved-work protection, recovery prompt and
import/export entry points; avoid building another persistence layer.

Acceptance: create → record → save → stop/restart audio → reopen → recover if
interrupted → export mix/stems/MIDI/REAPER completes from visible controls;
failure/progress is shown; exported files are manually checked in REAPER and at
least one Apple DAW.

### 3. Complete the Practice Lab vertical slice

Connect the existing 16 native commands and Practice events to the frontend
store and the reserved Practice workspace. Preserve the tuner and pitch curve.

Acceptance: record/rename a reference, capture/cancel an attempt, display pitch
and timing feedback, browse history/trends, rescore/delete, save/reopen the
library with the project, and validate with annotated real voices.

### 4. Expand tracks and layer editing deliberately

Design dynamic add/remove, raise the measured capacity to at least 10 tracks,
and expose layer/take detail without flattening every overdub into a track.
Then add nondestructive trim/arrangement primitives in independently testable
slices. Do not combine this milestone with an effects system.

### 5. Prove VoiceLoop Pocket offline on a physical device

Install full Xcode; build the smallest Tauri iOS shell; implement/verify the
AVAudioSession boundary; meter, record, loop, save and reopen locally in
Airplane Mode. Add hands-free controls only after the audio/storage proof.

### 6. Improve pitch personalization

Use real-voice evaluation to tune stability; add range detection, target-note
practice, colour customization and carefully smoothed optional reactive themes.

### 7. Establish MIDI playback and one internal instrument

Prove one polyphonic synth track from physical and virtual keyboards plus MIDI
clip playback. This becomes the common foundation for composition and live
performance rather than separate instrument systems.

### 8. Add percussion on the instrument foundation

Deliver a small licensed/local sample kit, velocity-sensitive pads, user sample
import and a minimal step sequencer before broad instrument libraries.

### 9. Grow composition and advanced production incrementally

Sequence MIDI editing, harmonic tools, notation/MusicXML, arrangement,
automation, effects, advanced pitch processing and transcription according to
validated dependencies. Prefer one complete workflow over several hidden
backends.

## Technical risks and open decisions

- **Realtime hardware evidence:** automated timing tests cannot establish
  CoreAudio latency, two-device drift, acoustic bleed or hot-plug behavior.
- **Contract drift:** Rust already emits project/Practice events and fields that
  the TypeScript parser omits. Mirrored types need stronger cross-language
  checks before the command surface grows.
- **Input discontinuity:** discontinuities are counted, but an active recording
  is not yet explicitly invalidated or segmented.
- **Mono engine limits:** global mono routing is not a hidden stereo mixer.
  Stereo/per-track input changes affect storage, rendering, UI and memory.
- **Fixed memory:** the preallocated pool is realtime-safe but conflicts with
  unbounded tracks/layers. Capacity, disk streaming and mobile memory need
  measured policy rather than arbitrary limits.
- **Pitch scope:** current detection is monophonic; polyphonic recognition and
  robust audio-to-MIDI are research risks.
- **iOS feasibility:** Tauri/CPAL portability is promising, but AVAudioSession,
  interruptions, routes, background behavior, signing and physical-device
  latency are not proven.
- **AEC:** speech-oriented echo cancellation may damage music and cannot promise
  perfect speaker-bleed removal. Decide only after representative recordings.
- **Effects architecture:** decide realtime versus offline processing, ordering,
  latency reporting and nondestructive derivatives before choosing libraries.
- **Mobile voice safety/privacy:** select an on-device recognition path and
  define behavior when unavailable before promising hands-free use.

## Verification rule

Every implementation milestone must record which of these six stages passed:
native/DSP, Rust service/state, Tauri, React, visible controls/feedback, and
functional/hardware verification. If a required stage is missing, document the
feature as partial rather than complete.
