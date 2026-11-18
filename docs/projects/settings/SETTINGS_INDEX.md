# Settings index

The source of truth for every setting. See [README.md](README.md) for the rules.

Status values: **exists** (in the code at `3157f27`), **proposed** (not built).
Descriptions are user-facing drafts.

Scopes: app / project / track / session.

## Audio devices and I/O (exists: `AudioConfig`, `routing.rs`; stored in `audio-settings.json`)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `audio.inputDevice` | Input device | The microphone or interface VoiceLoop records from. | System default | app | Settings › Input; device name display | exists |
| `audio.outputDevice` | Output device | Where VoiceLoop plays loops, the metronome and monitoring. | System default | app | Settings › Output & format | exists |
| `audio.missingDevice` | If a device is missing | What to do when the saved device isn't connected. | (see `MissingDevicePolicy`) | app | Settings › Input | exists |
| `audio.sampleRate` | Sample rate | Requested sample rate. Leave on automatic unless you know you need a specific rate. | Negotiated | app | Settings › Output & format | exists |
| `audio.bufferSize` | Buffer size | Smaller buffers lower latency but are more likely to click on a busy machine. | Device default | app | Settings › Output & format; diagnostics "Input block" | exists |
| `audio.inputRoute` | Input channels | Which input channel(s) to record. | (see `InputRoute`) | app | Settings › Input; channel meters | exists |
| `audio.outputRoute` | Output channels | Which output channels to play through. | (see `OutputRoute`) | app | Settings › Output & format | exists |
| `audio.monitor` | Monitoring | Whether, and how loudly, you hear your input live. | (see `MonitorConfig`) | app | Settings › Monitoring | exists |
| `audio.inputLatencyFrames` | Latency compensation | Shifts recordings to line up with what you heard. Set by calibration. | None | app | Settings › Latency & calibration | exists |
| `midi.inputs` | MIDI inputs | MIDI devices to open at startup. | None | app | Settings › MIDI input | exists |

## Transport and metronome (exists: `transport.rs`)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `transport.bpm` | Tempo | Beats per minute. | PENDING: confirm | project | Transport bar; loop-circle beat ticks (proposed) | exists |
| `transport.timeSignature` | Time signature | Beats per bar and beat unit. | PENDING: confirm | project | Transport bar; loop-circle measure ticks (proposed) | exists |
| `transport.loopMode` / `loopLength` / `loopFit` | Loop length and fit | How long new loops are, in bars/beats, and how recordings fit. | PENDING: confirm | project | Transport bar | exists |
| `transport.overdubStart` | Overdub start | When an overdub begins after you press Overdub. Loop start waits for the loop's 0 and counts down on the track; Next bar waits for the next bar line; Now starts at once (in Metronome mode, at the next bar). | Loop start | project | Transport bar; "in N" countdown on the track ring | exists |
| `metronome.enabled` | Metronome | Play a click. | PENDING: confirm | project | Transport bar | exists |
| `metronome.subdivision` | Click subdivision | Clicks per beat. | PENDING: confirm | project | Transport bar | exists |
| `metronome.accentDownbeat` | Accent downbeat | Make the first beat of each bar louder. | PENDING: confirm | project | Transport bar | exists |
| `metronome.gainDb` | Click volume | Metronome volume. | PENDING: confirm | project | Transport bar | exists |
| `metronome.countInBars` | Count-in | Bars of click before recording starts. | PENDING: confirm | project | Transport bar | exists |

## Tuner and pitch analysis (exists: `SettingsSheet.tsx` › TunerSettings)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `tuner.referenceA4` | Reference A4 | The pitch of A above middle C. Most music uses 440 Hz. | 440 Hz (PENDING: confirm) | app (PENDING: confirm) | Tuner; pitch curve | exists |
| `tuner.range` | Pitch range | The range of notes the detector listens for: Voice 65–1400 Hz, Low 40–700 Hz, High 100–2000 Hz. | Voice | app (PENDING: confirm) | Tuner; pitch curve | exists |
| `analysis.enabled` | Pitch analysis | Turn live pitch detection on or off. | PENDING: confirm | app | Tuner; diagnostics "Analysis" | exists |

## Practice scoring (exists: `ScoringConfig`, `src/practice/types.ts`)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `practice.difficulty` | Difficulty | Easy, Medium or Hard. Sets the pitch and timing tolerances below; editing any one of them switches to Custom. | Medium | app | Practice header; tolerance band on the overlay (proposed) | proposed |
| `practice.wrongNoteCents` | Wrong-note threshold | How far off a note can be, in cents, before it counts as a different note. | 50 | app | Practice overlay | exists |
| `practice.notePitchThresholdCents` | In-tune threshold | How close, in cents, a note must be to count as in tune. | 25 | app | Results; overlay colors | exists |
| `practice.noteTimingThresholdMs` | Timing threshold | How early or late, in ms, a note can be and still count as on time. | 60 | app | Results | exists |
| `practice.matchWindowMs` | Match window | How far, in ms, VoiceLoop looks to pair a sung note with a reference note. | 250 | app | (none) | exists |
| `practice.referencePitch` | Reference pitch | Score against the nearest semitone, or against the reference as it was sung. | nearestSemitone | app | Tolerance toggles | exists |
| `practice.transposition` | Allow transposition | Score the melody shape even when sung in a different key. | off | app | Tolerance toggles | exists |
| `practice.octaveInvariant` | Ignore octave | Count the right note in any octave as correct. | off | app | Tolerance toggles | exists |
| `practice.tempoTolerant` | Ignore tempo | Score rhythm relative to your own tempo. | off | app | Tolerance toggles | exists |
| `practice.minClarity` / `minPitchFrames` / `edgeTrimMs` / `gridSubdivision` / `gridSnapTolerance` | Advanced detection | Low-level note-detection tuning. | 0.8 / 4 / 40 / 4 / 0.25 | app | (none) | exists |
| `practice.maxTakeSeconds` | Maximum take length | Longest reference or attempt recording. | 60 s | app | Record button | exists |
| `practice.preRollMs` | Pre-roll | Audio kept from just before you press record. | 250 ms | app | (none) | exists |
| `practice.latencyOverrideMs` | Latency override | Overrides the measured latency for practice scoring only. | None | app | (none) | exists |
| `practice.stopStudioOnEnter` | Stop Studio in Practice | Stop loop playback when you open Practice Lab. | on | app | Workspace tabs | proposed |
| `practice.notationView` | Note display | Show takes as a piano roll, a staff, or both. | Piano roll | app | Practice result view | proposed |

## Visuals and theme (proposed; see [visualization](../visualization/README.md))

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `theme.mode` | Theme | Light, dark, or follow the system. | System | app | Whole app | proposed |
| `theme.pitchReactive` | Pitch-reactive colors | Tint the app's accent colors with the note you're singing. | off | app | App accents/background | proposed |
| `theme.pitchReactiveStrength` | Reactive color strength | How strongly pitch-reactive colors tint the app. | PENDING | app | App accents | proposed |
| `notes.colorMap` | Note colors | The color for each of the 12 note names. | PENDING: default palette | app | Pitch curve, piano roll, staff, loop rings, tuner | proposed |
| `notes.accuracyEncoding` | Accuracy shading | How accuracy and stability show on note colors: brightness, saturation or opacity. | PENDING | app | Same as note colors | proposed |
| `view.loopRingHistogram` | Loop waveform ring | Show each loop layer's loudness around the loop circle. | on | app | Track card ring | proposed |
| `view.loopRingTicks` | Beat and bar ticks | Show beat and bar marks around the loop circle. | on | app | Track card ring | proposed |
| `view.playheadEmphasis` | Playhead highlight | Brighten and enlarge the part of the loop being played. | on | app | Track card ring | proposed |
