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

## Chord analysis (exists: voiceloop-analysis, not yet wired to the UI)

Fields of `ChordConfig` in `crates/voiceloop-analysis/src/chord.rs`. See
[polyphonic-analysis](../polyphonic-analysis/README.md). Most are advanced
tuning; only the first few are likely user-facing.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `analysis.chord.minConfidence` | Chord confidence | How closely the sound must match a chord before a name is shown (0–1). Higher shows fewer, surer chords. | 0.8 | app | (none yet) | exists |
| `analysis.chord.holdFrames` | Chord hold | How many analysis frames in a row a new chord must win before the display changes. Higher is steadier but slower. | 3 | app | (none yet) | exists |
| `analysis.chord.tuningA4Hz` | Chord reference A4 | Pitch of A4 used to map frequencies to note names. Should follow `tuner.referenceA4`. | 440 Hz | app | (none yet) | exists |
| `analysis.chord.minRmsDb` | Chord silence level | Below this level (dBFS) no chord is reported. | -50 dB | app | (none yet) | exists |
| `analysis.chord.minHz` / `maxHz` | Chord frequency range | Frequencies considered when reading chords and notes. | 55 / 2000 Hz | app | (none yet) | exists |
| `analysis.chord.minFftSize` | Analysis resolution | Each block is padded to at least this many samples before the FFT. | 8192 | app | (none yet) | exists |
| `analysis.chord.peakFloorDb` | Peak floor | Spectral peaks this many dB below the strongest are ignored. | -40 dB | app | (none yet) | exists |
| `analysis.chord.significantLevel` | Significant peak level | Fraction of the strongest peak a peak needs to count as the bass or as a fundamental of overtones. | 0.1 | app | (none yet) | exists |
| `analysis.chord.harmonicWeight` / `maxHarmonic` / `harmonicToleranceCents` | Overtone attenuation | Overtones (up to harmonic 8, within 25 cents) count at this fraction of their strength. | 0.5 / 8 / 25 | app | (none yet) | exists |
| `analysis.chord.bassRootBonus` | Bass root preference | Score bonus for chords whose root is the lowest note; settles sus2/sus4 ambiguity. | 0.03 | app | (none yet) | exists |
| `analysis.chord.alternatives` | Alternative chords | How many runner-up chords are reported. | 3 | app | (none yet) | exists |
| `analysis.chord.maxNotes` | Max sounding notes | Most notes reported by the sounding-notes estimate. | 4 | app | (none yet) | exists |
| `analysis.chord.noteMinSalience` / `noteHarmonics` / `noteHarmonicDecay` | Note detection tuning | Relative strength a note needs, harmonics summed per note, and their weight decay. | 0.25 / 6 / 0.8 | app | (none yet) | exists |

## Practice scoring (exists: `ScoringConfig`, `src/practice/types.ts`)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `practice.difficulty` | Difficulty | Super Easy, Easy, Medium or Hard set how close pitch and timing must be. Super Easy accepts a note a sharp or flat away (125¢ / 50¢ / 150 ms). Editing a tolerance by hand switches to Custom. | Medium | app | Practice Lab Difficulty control (presets in `src/practice/difficulty.ts`); tolerance band on the overlay (proposed) | exists |
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
| `practice.stopStudioOnEnter` | Stop Studio in Practice | Stop loop playback when you open Practice Lab. | on | app | Workspace tabs (sends the transport Stop; a Studio take in progress is finished and kept) | exists |
| `practice.scale.octave` | Scale octave | Which octave scale exercises start in. 4 starts on middle C's octave. | 4 | app | Scale exercise card › Scale options | exists |
| `practice.scale.direction` | Scale direction | Sing scale exercises up only, or up and back down. | Up and down | app | Scale exercise card › Scale options | exists |
| `practice.scale.noteBeats` | Scale note length | How long each scale note lasts, in beats (1 = quarter notes). | 1 beat | app | Scale exercise card › Scale options | exists |
| `practice.scale.defaultBpm` | Scale tempo without looper | Tempo for scale exercises when there is no looper tempo to use (audio output off). Otherwise the looper tempo is used. | 80 BPM | app | Scale exercise card › Scale options | exists |
| `practice.scale.noteGate` | Scale note gate | How much of each note's beat the exercise holds the note; the rest is a short breath. Internal (not shown). | 0.9 | app | (none; affects the "notes short/long" feedback) | exists |
| `notation.view` | Note display | Show takes as a piano roll, a staff, or both. | Piano roll | app | Practice result view | exists |

## Voice range and games (exists: `src/games/`, Games workspace)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `voice.rangeLowMidi` | Lowest note | The lowest note you can sing comfortably (MIDI number). Set by the Range Finder; reusable by other features (for example the Practice scale octave, PENDING). | 48 (C3) | app | Games › Range Finder (capture, ±1 nudge); Pitch Glider "My range" | exists |
| `voice.rangeHighMidi` | Highest note | The highest note you can sing comfortably (MIDI number). | 72 (C5) | app | Games › Range Finder; Pitch Glider "My range" | exists |
| `voice.rangeFinder.minClarity` | Range Finder clarity | How clear your voice must be before the Range Finder counts a note. Internal. | 0.85 | app | Range Finder capture | exists |
| `voice.rangeFinder.maxStabilityCents` | Range Finder steadiness | How much your pitch may waver (std dev, cents) while a held note is counted. Internal. | 30 ¢ | app | Range Finder capture | exists |
| `voice.rangeFinder.holdMs` | Range Finder hold time | How long a note must be held on the same semitone to be captured. Internal. | 800 ms | app | Range Finder progress ring | exists |
| `games.difficulty` | Game difficulty | Pitch Glider opening height: Easy/Medium/Hard = 0.42/0.32/0.22 of the play field (`GAME_OPENINGS` in the registry). | Medium | app | Games › Pitch Glider controls | exists |
| `games.range` | Game range | Which notes span the play field: My range (`voice.range*`) or Custom (`games.range*Midi`). | My range | app | Games › Pitch Glider controls | exists |
| `games.rangeLowMidi` | Custom low note | Bottom of the play field when Game range is Custom. | 48 (C3) | app | Pitch Glider custom range nudges | exists |
| `games.rangeHighMidi` | Custom high note | Top of the play field when Game range is Custom. | 72 (C5) | app | Pitch Glider custom range nudges | exists |
| `games.speed` | Game speed | How fast gates scroll: Slow/Normal/Fast = 0.22/0.32/0.45 play-field heights per second (`GAME_SPEEDS`). Gates are `GAME_GATE_SPACING` = 0.9 heights apart. | Normal | app | Games › Pitch Glider controls | exists |
| `games.pitchSmoothingMs` | Glider smoothing | Time constant of the exponential smoothing from sung pitch to glider height. | 120 ms | app | Pitch Glider; Preferences › Games | exists |
| `games.gravity` | Glider sink rate | How fast the glider sinks without a confident pitch (play-field heights per second). | 0.25 | app | Pitch Glider; Preferences › Games | exists |
| `games.minClarity` | Glider clarity | Clarity a pitch frame needs for the glider to follow it. Internal. | 0.8 | app | Pitch Glider | exists |
| `games.bestScores` | Best scores | Best Pitch Glider score per difficulty. Stored value, not a control. | 0 / 0 / 0 | app | Pitch Glider game-over screen | exists |

## Visuals and theme (proposed; see [visualization](../visualization/README.md))

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `theme.mode` | Theme | Light, dark, or follow the system. | System | app | Whole app | exists |
| `theme.pitchReactive` | Pitch-reactive colors | Tint the app's accent colors with the note you're singing. | off | app | App accents/background | exists |
| `theme.pitchReactiveStrength` | Reactive color strength | How strongly pitch-reactive colors tint the app. | 0.35 | app | App accents | exists |
| `notes.colorMap` | Note colors | The color for each of the 12 note names. | Hue wheel, C = red (PENDING: tuned palette) | app | Pitch curve, piano roll, staff, loop rings, tuner | exists |
| `notes.accuracyEncoding` | Accuracy shading | How accuracy and stability show on note colors: brightness, saturation or opacity. | Saturation | app | Same as note colors | exists |
| `view.loopRingLayers` | Layer rings | Show each loop layer's loudness as its own ring around the track circle. | on | app | Track card ring | exists |
| `view.loopRingBins` | Layer ring resolution | Number of equal loop slices in each compact per-layer peak summary. Fixed as part of the engine/frontend wire contract. | 128 | app | Track card ring | exists |
| `view.loopRingTicks` | Beat and bar ticks | Show beat and bar marks around the loop circle. | on | app | Track card ring | exists |
| `view.playheadEmphasis` | Playhead highlight | Brighten and enlarge the part of the loop being played. | on | app | Track card ring | exists |
| `view.timingReadout` | Timing readout | After each take, show how far your hits landed from the beat. | on | app | Track card | proposed |
| `notation.gridSubdivision` | Notation grid | How finely sung timing is rounded for the staff: steps per beat. 4 rounds to sixteenth notes in 4/4; 2 to eighths (cleaner but less exact). | 4 | app | Staff view | exists |

## Looper limits (exists: `LooperConfig`, engine; set via `set_looper_config`, applied at next audio start)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `looper.poolLayers` | Layer memory | How many loop layers all tracks can hold together. Each layer reserves memory for the longest loop. | 32 | app | "Loop memory" in diagnostics; layer-limit notice | exists (not in UI) |
| `looper.maxLayersPerTrack` | Layers per track | The most layers (base take plus overdubs) one track can hold. | 16 | app | Track layer count | exists (not in UI) |
| `looper.maxLoopSeconds` | Longest loop | The longest loop you can record. Longer loops use more memory per layer. | 30 s | app | (none) | exists (not in UI) |
| `looper.memoryLimitBytes` | Memory guard | Refuses layer settings that would reserve more than this. | 512 MB | app | Start-audio error | exists (not in UI) |
| `view.loopRingFloorDb` | Layer ring floor | Quietest level drawn on the layer rings; louder parts reach further out. | −48 dBFS | app | Track card layer rings | exists (constant `LAYER_RING_FLOOR_DB`, not in UI) |
| `view.livePitchPanel` | Live pitch panel | Show the tuner and pitch curve beside the tracks in Studio. Pitch analysis only runs while something needs it (this panel, Practice Lab, or pitch-reactive colors). | off | app | Studio "Live pitch" toggle; tuner, pitch curve | exists |
| `widgets.hidden` | Hidden widgets | Widgets you have hidden (right-click › Hide widget, or the View menu). Show them again from View › Show all. | none | app | Practice dashboard widgets; View menu | exists |
| `practice.scale.instrument` | Scale sound | The instrument used when you play a scale or reference to hear it. More instruments are coming. | Piano | app | Scale card "Play"; reference "Play" | exists |
| `audio.previewVolume` | Preview volume | Volume of scales and references played to hear them (not recorded into loops). | 0.5 | app | Scale/reference playback | exists |
| `audio.previewTuningA4` | Preview tuning (A4) | Pitch of A4 for played scales and references. PENDING: follow the tuner's reference pitch. | 440 Hz | app | Scale/reference playback | exists (internal) |
| `practice.startNote.minClarity` | Start note clarity | How clear a sung note must be for "Start from my note". | 0.85 | app | Scale card | exists (internal) |
| `practice.startNote.maxDriftCents` | Start note steadiness | How far (cents) the sung note may wander while it is being captured. | 35 | app | Scale card | exists (internal) |
| `practice.startNote.holdMs` | Start note hold | How long (ms) to hold the note for "Start from my note". | 600 ms | app | Scale card | exists (internal) |
| `view.livePitchPanel` | Live pitch panel | Show the tuner and pitch curve beside the tracks in Studio. Pitch analysis only runs while something needs it (this panel, Practice Lab, Games, or pitch-reactive colors). | off | app | Studio "Live pitch" toggle; tuner, pitch curve | exists |
| `practice.scale.followRange` | Fit scales to my range | When you pick a key, start the scale in the octave that fits the range from the Range Finder (Games tab). | on | app | Scale card octave; "Fit my range" buttons | exists |
