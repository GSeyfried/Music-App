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
| `audio.micMode` | Microphone | Only when needed: the microphone opens for recording, the tuner, live pitch, Practice Lab and games, and closes again afterwards, so the system mic indicator stays off while you only listen. Always on: it stays open while audio runs. | Only when needed | app | Preferences › Audio input; top-bar mic indicator (`MicIndicator`); `src/engine/micNeeds.ts` | wired |
| `audio.micIdleCloseSeconds` | Close the mic after | Seconds the microphone stays open after the last thing that needed it ends (ready again at once). 0 closes it immediately. Sent to the engine as `holdMs` (`set_input_needs`); engine default `mic::DEFAULT_HOLD_MS`. | 10 s | app | Preferences › Audio input (advanced) | wired |
| (engine) `mic::START_TIMEOUT` | Mic start timeout | A record, overdub or calibration waiting for the microphone is cancelled (with an error) if input is not flowing after this long. Not user-facing. | 5 s | app | `crates/voiceloop-engine/src/mic.rs` | internal |
| (engine) `mic::RETRY_AFTER` | Mic retry delay | After the microphone failed to open, wait this long before trying again. Not user-facing. | 3 s | app | `crates/voiceloop-engine/src/mic.rs` | internal |

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

## Per-track loops (exists: `TrackLoop` in `transport.rs` / `domain.ts`; see [PER_TRACK_LOOPS.md](../looper-01/PER_TRACK_LOOPS.md))

Each track's own values live in the engine (`SetTrackLoop`, saved with the project). The
`looper.trackDefaults.*` settings are what a new track starts with; when they differ from the
engine default (Same as song), the UI sends `setTrackLoop` right after adding the track.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `track.loop.mode` | Track loop | How this track loops: Same as song, Bars, Multiple (of the base loop) or Free. Changes only while the track is empty. | Same as song | track | Transport bar "Track N loop" group; track card summary; Track details "Loops" | exists |
| `track.loop.bars` / `beats` | Track bars / + beats | Bars mode: this track's loop length on the song's grid. Changes while empty, or while stopped (layers are kept). | 1 bar + 0 | track | Transport bar "Track N loop" group; track ring ticks | exists |
| `track.loop.ratio` | Track multiple | Multiple mode: ×¼, ×½, ×1, ×1.5, ×2, ×3, ×4, ×8 or ×16 of the base loop. Changes while empty, or while stopped. | ×1 | track | Transport bar "Track N loop" group | exists |
| `track.loop.countInBars` | Track count-in | Bars of click before recording on this track starts from stopped. | 1 | track | Transport bar "Track N loop" group | exists |
| `track.loop.overdubStart` | Track overdub start | When an overdub on this track begins. | Loop start | track | Transport bar "Track N loop" group | exists |
| `track.loop.loopFit` | Track fit first take | How this track's first take is fitted (Free, or the first Multiple take). | Nearest | track | Transport bar "Track N loop" group | exists |
| `looper.trackDefaults.mode` | New track loops | How a new track loops. Same as song follows the song's loop; Bars uses its own number of bars on the song's grid; Multiple is a multiple of the first Multiple loop; Free is as long as its first take. | Same as song | app | Settings › New tracks; Add track | exists |
| `looper.trackDefaults.bars` | New track bars | Loop length in whole bars for a new track in Bars mode (plus the extra beats). | 1 | app | Settings › New tracks | exists |
| `looper.trackDefaults.beats` | New track extra beats | Beats added after the whole bars for a new track in Bars mode. | 0 | app | Settings › New tracks | exists |
| `looper.trackDefaults.ratio` | New track multiple | For a new track in Multiple mode: how many times the base loop it lasts. | ×1 | app | Settings › New tracks | exists |
| `looper.trackDefaults.countInBars` | New track count-in | Bars of click before a new track starts recording from stopped. | 1 | app | Settings › New tracks | exists |
| `looper.trackDefaults.overdubStart` | New track overdub start | When an overdub on a new track begins. | Loop start | app | Settings › New tracks | exists |
| `looper.trackDefaults.loopFit` | New track fit | How a new Free track's first take (or the first Multiple take) is fitted when you stop. | Nearest | app | Settings › New tracks | exists |

## Tuner and pitch analysis (exists: `SettingsSheet.tsx` › TunerSettings)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `tuner.referenceA4` | Reference A4 | The pitch of A above middle C. Most music uses 440 Hz. | 440 Hz (PENDING: confirm) | app (PENDING: confirm) | Tuner; pitch curve | exists |
| `tuner.range` | Pitch range | The range of notes the detector listens for: Voice 65–1400 Hz, Low 40–700 Hz, High 100–2000 Hz. | Voice | app (PENDING: confirm) | Tuner; pitch curve | exists |
| `analysis.enabled` | Pitch analysis | Turn live pitch detection on or off. | PENDING: confirm | app | Tuner; diagnostics "Analysis" | exists |

## Chord analysis (wired to live analysis and UI readout)

Fields of `ChordConfig` in `crates/voiceloop-analysis/src/chord.rs`. See
[polyphonic-analysis](../polyphonic-analysis/README.md). Most are advanced
tuning; only the first few are likely user-facing.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `analysis.chord.minConfidence` | Chord confidence | How closely the sound must match a chord before a name is shown (0–1). Higher shows fewer, surer chords. | 0.8 | app | Live chord readout | wired |
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
| `practice.legatoGlideMs` / `legatoGapMs` | Legato slides | When you slide smoothly between notes, sung pieces up to this long (ms) that run straight into a note (gap up to `legatoGapMs`) count as part of that note, not as notes of their own. 0 turns it off. | 160 / 50 | app | (none; scoring) | exists (engine ScoringConfig) |
| `practice.maxTakeSeconds` | Maximum take length | Longest reference or attempt recording. | 60 s | app | Record button | exists |
| `practice.preRollMs` | Pre-roll | Audio kept from just before you press record. | 250 ms | app | (none) | exists |
| `practice.latencyOverrideMs` | Latency override | Overrides the measured latency for practice scoring only. | None | app | (none) | exists |
| `practice.stopStudioOnEnter` | Stop Studio in Practice | Stop loop playback when you open Practice Lab. | on | app | Workspace tabs (sends the transport Stop; a Studio take in progress is finished and kept) | exists |
| `practice.scale.octave` | Scale octave | Which octave scale exercises start in. 4 starts on middle C's octave. | 4 | app | Scale exercise card › Scale options | exists |
| `practice.scale.direction` | Scale direction | Sing scale exercises up only, or up and back down. | Up and down | app | Scale exercise card › Scale options | exists |
| `practice.scale.noteBeats` | Scale note length | How long each scale note lasts, in beats (1 = quarter notes). | 1 beat | app | Scale exercise card › Scale options | exists |
| `practice.scale.defaultBpm` | Scale tempo without looper | Tempo for scale exercises when there is no looper tempo to use (audio output off). Otherwise the looper tempo is used. | 80 BPM | app | Scale exercise card › Scale options | exists |
| `practice.scale.noteGate` | Scale note gate | How much of each note's beat the exercise holds the note; the rest is a short breath. Internal (not shown). | 0.9 | app | (none; affects the "notes short/long" feedback) | exists |
| `notation.view` | Note display | Show the reference, the live attempt and results as a piano roll, a staff (clef mode; the note to sing now is highlighted live), or both. | Piano roll | app | Practice reference card, live attempt, result view | exists |

## Voice range and games (exists: `src/games/`, Games workspace)

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `voice.rangeLowMidi` | Lowest note | The lowest note you can sing comfortably (MIDI number). Set by the Range Finder; reusable by other features (for example the Practice scale octave, PENDING). | 48 (C3) | app | Games › Range Finder (capture, ±1 nudge); Pitch Glider "My range" | exists |
| `voice.rangeHighMidi` | Highest note | The highest note you can sing comfortably (MIDI number). | 72 (C5) | app | Games › Range Finder; Pitch Glider "My range" | exists |
| `voice.rangeFinder.minClarity` | Range Finder clarity | How clear your voice must be before the Range Finder counts a note. Internal. | 0.85 | app | Range Finder capture | exists |
| `voice.rangeFinder.maxStabilityCents` | Range Finder steadiness | How much your pitch may waver (std dev, cents) while a held note is counted. Internal. | 30 ¢ | app | Range Finder capture | exists |
| `voice.rangeFinder.holdMs` | Range Finder hold time | How long a note must be held on the same semitone to be captured. Internal. | 800 ms | app | Range Finder progress ring | exists |
| `games.difficulty` | Game difficulty | Super Easy, Easy, Medium or Hard sets the cents accuracy required as the gate reaches the glider. | Medium | app | Games › Pitch Glider controls | exists |
| `games.pitchToleranceCents` | Glider pitch tolerance | Cents of visible craft-centre clearance around a gate's target note: Super Easy / Easy / Medium / Hard = ±100 / ±60 / ±40 / ±25¢. The first three values are PENDING real-voice tuning; Hard is fixed. | by difficulty | app | Pitch Glider pipe opening | exists (constant `GAME_DIFFICULTY_TOLERANCE_CENTS`, not shown) |
| `games.stabilityToleranceCents` | Glider wobble threshold | Live `stabilityCents` at which the clearly drawn wobble envelope reaches a pipe edge: Super Easy / Easy / Medium / Hard = 100 / 60 / 40 / 25¢. Greater wobble can collide only while the envelope overlaps a pipe. The first three values are PENDING real-voice tuning; Hard is fixed. | by difficulty | app | Pitch Glider wobble envelope | exists (constant `GAME_DIFFICULTY_STABILITY_CENTS`, not shown) |
| `games.pipeWidth` | Glider pipe width | Width of a pipe, which determines how long the craft must remain visibly within the opening: Super Easy / Easy / Medium / Hard = 0.10 / 0.13 / 0.16 / 0.20 field widths. At extreme course speed factors it contracts below the inter-note spacing so adjacent solid walls cannot overlap. Values are PENDING real-voice tuning. | by difficulty | app | Pitch Glider pipe | exists (constant `GAME_GATE_WIDTHS`, not shown) |
| `games.craftRadius` | Glider size | Radius of the drawn glider and its collision envelope as a fraction of the visible pitch viewport height. The pipe opening expands by the same rendered size, so the craft centre retains exactly the selected cents clearance at every zoom level. | 0.035 viewport heights | app | Pitch Glider craft and pipe opening | exists (constant `GAME_CRAFT_RADIUS`, not shown) |
| `games.gateEdgeMargin` | Glider pipe edge margin | Minimum empty space above and below a pipe opening where the selected range permits. | 0.06 field heights | app | Pitch Glider pipe | exists (constant `GAME_GATE_EDGE_MARGIN`, not shown) |
| `games.firstGateDelay` | First glider pipe delay | How far beyond the right edge the first pipe starts. | 0.6 field widths | app | Pitch Glider play field | exists (constant `GAME_FIRST_GATE_DELAY`, not shown) |
| `games.pitchWarpExponent` | Glider pitch magnification | Internal nonlinear curve that magnifies small cents drift near each semitone while preserving pitch order and the selected range endpoints. | 0.55 | app | Pitch Glider position | exists (constant `GAME_PITCH_WARP_EXPONENT`, not shown; PENDING real-voice tuning) |
| `games.range` | Game range | Which notes span the play field: My range (`voice.range*`) or Custom (`games.range*Midi`). | My range | app | Games › Pitch Glider controls | exists |
| `games.rangeLowMidi` | Custom low note | Bottom of the play field when Game range is Custom. | 48 (C3) | app | Pitch Glider custom range nudges | exists |
| `games.rangeHighMidi` | Custom high note | Top of the play field when Game range is Custom. | 72 (C5) | app | Pitch Glider custom range nudges | exists |
| `games.speed` | Scroll speed | How fast gates approach: Slow/Normal/Fast = 0.22/0.32/0.45 play-field heights per second (`GAME_SPEEDS`). In courses, written BPM and the speed factor control rhythm. | Normal | app | Games › Pitch Glider controls | exists |
| `games.courseId` | Glider course | Free flight generates targets forever. A bundled scale/melody or saved Practice reference supplies an exact, finite note order. | Free flight | app | Pitch Glider course picker | exists |
| `games.courseSpeedFactor` | Course speed factor | Directly editable multiplier for the written course tempo: the pipes scroll faster or slower and keep their rhythmic spacing. 1× is real written speed, 0.5× is half speed, and the range extends from 0.05× to 12× in 0.01× steps. | 1× | app | Pitch Glider course controls | exists |
| `games.pipeDistanceSemitones` | Pipe pitch distance | Requested melodic interval between consecutive pipe targets. The game chooses the nearest achievable exact semitone inside the selected range; larger values demand larger pitch changes in the same pipe travel time. | 5 semitones | app | Games › Pitch Glider controls | exists |
| `games.voiceStartHoldMs` | Hands-free start hold | Continuous time on the randomized prompt note before a game starts. The Play button and Space remain available. | 250 ms | app | Pitch Glider start/game-over overlay | exists (internal; capped at 250 ms) |
| `games.voiceStartToleranceCents` | Hands-free start tolerance | Maximum distance from the prompted semitone while holding it. Any octave currently qualifies; octave-specific starts are PENDING. | 45 cents | app | Pitch Glider start/game-over overlay | exists (internal) |
| `games.viewportSemitones` | Pitch viewport | Number of semitones shown at once. The camera smoothly follows the next pipe target while the full selected range remains eligible. Default is PENDING real-voice/display tuning. | 12 semitones | app | Pitch Glider; Advanced Games | exists |
| `games.pitchSmoothingMs` | Glider response | Time constant of the exponential move toward a clear sung pitch. The 30 ms default is intentionally very fast; PENDING real-voice tuning. | 30 ms | app | Pitch Glider; Advanced Games | exists |
| `games.gravity` | Glider sink rate | How fast the glider visibly sinks without a confident pitch (play-field heights per second). PENDING real-voice tuning. | 0.25 | app | Pitch Glider; Advanced Games | exists |
| `games.minClarity` | Glider clarity | Clarity a pitch frame needs for the glider to follow it. Internal. | 0.8 | app | Pitch Glider | exists |
| `games.bestScores` | Best scores | Best Pitch Glider score per difficulty. Stored value, not a control. | 0 / 0 / 0 | app | Pitch Glider game-over screen | exists |
| `pitchCatcher.choiceCount` | Note choices | Number of possible notes shown in Pitch Catcher. | 2 | app | Pitch Catcher controls | exists |
| `pitchCatcher.autoAdvance` | Automatically get harder | Adds another note choice after a correct streak, up to four. | on | app | Pitch Catcher controls | exists |
| `pitchCatcher.advanceStreak` | Catches to advance | Consecutive correct answers needed to add a choice. | 3 | app | internal Pitch Catcher progression | exists |
| `pitchCatcher.fallSeconds` | Fall time | Seconds colored choices take to reach the catch line; they wait there rather than failing. | 6 s | app | Pitch Catcher controls | exists |
| `pitchCatcher.notePitchClasses` | Notes to include | Colored pitch classes allowed as heard targets and falling choices; at least two remain selected. | all 12 | app | Pitch Catcher note picker | exists |
| `pitchCatcher.octaves` | Octaves | Keep each round in one octave or allow different octaves across the saved voice range. | One octave per round | app | Pitch Catcher controls | exists |
| `groove.difficulty` | Echo difficulty | Controls remembered phrase length, density, and syncopation without displaying future hits. | Easy | app | Games › GROOVE Echo | exists |
| `groove.mode` | GROOVE mode | Echo plays a phrase for a tempo-flexible answer; Free drums gives unscored visual input feedback. | Echo | app | GROOVE mode dropdown | exists |
| `groove.inputMode` | Echo input | Play the two pads with touch/F/J, use gated microphone onset detection, or map two notes from an external MIDI drum/controller. | Pads + F/J | app | GROOVE input dropdown, pads and visual trails | exists |
| `groove.bpm` | Echo tempo | Tempo of the heard phrase and both four-click count-ins. | 92 BPM | app | GROOVE Echo controls | exists |
| `groove.timingToleranceMs` | Echo timing window | Maximum proportional timing error after the answer is rebased to its first hit and fitted to the player's tempo. | 140 ms | app | GROOVE Echo scoring | exists |
| `groove.micMinLevelDb` | Microphone noise gate | Reject acoustic attacks quieter than this level. Raising toward −6 dB filters more background noise. | −24 dBFS | app | GROOVE microphone input | exists |
| `groove.micMinRiseDb` | Attack contrast | Reject acoustic sounds that do not rise sharply enough over the recent background. | 18 dB | app | GROOVE microphone input | exists |
| `groove.midiLowNote` | External low-drum note | MIDI note mapped to the low drum. | 36 (GM kick) | app | GROOVE external MIDI input | exists |
| `groove.midiHighNote` | External high-drum note | MIDI note mapped to the high drum. | 38 (GM snare) | app | GROOVE external MIDI input | exists |
| `groove.autoAdvance` | Adapt challenge | Scores of 80+ get harder, scores below 55 get easier, and the middle band stays unchanged. | on | app | GROOVE Echo progression/result | exists |
| `groove.continuous` | Keep going | Automatically starts another Echo phrase after each result. | on | app | GROOVE Echo controls | exists |
| `groove.failureThreshold` | Easier below | Scores below this value make the next round easier. | 55 | app | internal GROOVE progression | exists |
| `groove.successThreshold` | Harder at | Scores at or above this value make the next round harder. | 80 | app | internal GROOVE progression | exists |
| `groove.nextRoundDelayMs` | Next-round pause | Time the result remains visible before continuous Echo starts again. | 1800 ms | app | internal GROOVE timing | exists |
| `groove.bestScores` | Echo best scores | Best GROOVE Echo score for Easy, Medium, and Hard. Stored value, not a control. | 0 / 0 / 0 | app | GROOVE Echo result | exists |

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
| `notation.meter` | Time signature | Time signature for the staff. Automatic uses the take's beat grid when it has one; otherwise it guesses 4/4, 3/4 or 6/8 from where the long notes fall. | Automatic | app | Staff view | exists |
| `notation.keySignature` | Key signature | Key signature for the staff. Automatic estimates the key from the notes and spells sharps or flats to match. | Automatic | app | Staff view | exists |
| `notation.clef` | Clef | Clef for the staff. Automatic picks the one that needs the fewest ledger lines; tenor-range voices get the treble clef with an 8 below. | Automatic | app | Staff view | exists |
| `notation.colorNotes` | Colored staff notes | Color notes on the staff with the note colors, shaded by accuracy. Off shows plain black-and-white notation. | off | app | Staff view | exists |
| `notation.minNote` | Shortest staff note | Sung notes shorter than this are treated as pitch wobble and merged into the note next to them. | 32nd | app | Staff view | exists |
| `notation.minRest` | Shortest staff rest | Gaps between sung notes shorter than this are held over by the earlier note instead of showing as rests. | Eighth | app | Staff view | exists |
| `notation.scale` | Staff size | How large staff notation is drawn. | 1.25 | app | Staff view | exists |

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
| `audio.previewThroughEngine` | Play previews on the output device | Scales and references play with the sampled piano (Salamander Grand) on the chosen output device, mixed after the loops and never recorded. Off, or while audio is stopped: the synthesized Web Audio piano on the system default output. | on | app | Scale/reference playback; games' note previews | exists |
| `audio.previewTuningA4` | Preview tuning (A4) | Pitch of A4 for played scales and references. PENDING: follow the tuner's reference pitch. | 440 Hz | app | Scale/reference playback | exists (internal) |
| `practice.startNote.minClarity` | Start note clarity | How clear a sung note must be for "Start from my note". | 0.85 | app | Scale card | exists (internal) |
| `practice.startNote.maxDriftCents` | Start note steadiness | How far (cents) the sung note may wander while it is being captured. | 35 | app | Scale card | exists (internal) |
| `practice.startNote.holdMs` | Start note hold | How long (ms) to hold the note for "Start from my note". | 600 ms | app | Scale card | exists (internal) |
| `view.livePitchPanel` | Live pitch panel | Show the tuner and pitch curve beside the tracks in Studio. Pitch analysis only runs while something needs it (this panel, Practice Lab, Games, or pitch-reactive colors). | off | app | Studio "Live pitch" toggle; tuner, pitch curve | exists |
| `practice.scale.followRange` | Fit scales to my range | When you pick a key, start the scale in the octave that fits the range from the Range Finder (Games tab). | on | app | Scale card octave; "Fit my range" buttons | exists |
| `looper.timing.windowFrames` | Timing energy window | Frames in each RMS window while scanning a completed layer for attacks. | 64 frames | app | Track Details timing readout | exists (engine internal) |
| `looper.timing.minimumRms` | Timing minimum energy | Minimum RMS required for a completed-layer attack. | 0.02 | app | Track Details timing readout | exists (engine internal) |
| `looper.timing.riseRatio` / `releaseRatio` | Timing onset shape | Required RMS rise to mark an attack and fall before another can arm. | 2.0 / 0.6 | app | Track Details timing readout | exists (engine internal) |
| `looper.timing.minimumSpacingMs` | Timing hit spacing | Minimum distance between detected attacks. | 80 ms | app | Track Details timing readout | exists (engine internal) |
| `looper.timing.maxHits` | Timing hit limit | Maximum attack offsets carried in one layer-timing event. | 32 | app | Track Details timing readout | exists (engine internal) |
| `looper.timing.wholeBeatToleranceFrames` | Whole-beat tolerance | Maximum period mismatch allowed before beat-relative timing is withheld. | 1 frame | app | Track Details timing readout | exists (engine internal) |

## PRODUCE (exists: `src/produce/`; see [produce](../produce/README.md))

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `produce.snap` | Snap | Where moved, trimmed and split edits land: whole bars, beats, eighth or sixteenth notes, or anywhere (Off). | Beat | app | PRODUCE toolbar › Snap; region drag/trim/split, ruler seek, arrow-key nudge | exists |
| `produce.zoom` | Timeline zoom | How wide one second of the arrangement is, in pixels. | 80 px/s | app | PRODUCE toolbar › zoom in/out/fit; `=` / `-` / `Z` | exists |
| `produce.zoomStep` | Zoom step | Zoom in and out multiply or divide the zoom by this. | 1.5 | app | Zoom buttons and keys | internal |
| `produce.trackHeight` | Lane height | Height of each arrangement lane, in pixels. | 88 px | app | Lane headers and lanes | exists |
| `produce.followPlayhead` | Follow playhead | Scroll the timeline to keep the playhead in view while playing. | on | app | PRODUCE toolbar › Follow; `F` | exists |
| `produce.defaultRepeats` | Bring-in repeats | How many loop cycles a looper track fills when you bring it in from PLAY. | 4 | app | Browser › PLAY tracks › Repeats | exists |
| `produce.bringInPerLayer` | Bring in each layer | Bring each overdub layer in as its own lane instead of one mixed lane per track. | off | app | Browser › PLAY tracks › Mixed / Per layer | exists |
| `produce.bringInAtPlayhead` | Bring in at playhead | Place brought-in loops at the playhead (off: at the start of the song). | on | app | Browser › PLAY tracks › At playhead | exists |
| `produce.regionColorMode` | Region colors | Color regions by their lane, by the PLAY track they came from, or plainly. | Lane | app | Regions | exists |
| `produce.waveformStyle` | Waveform style | How region waveforms are drawn (filled, outline, or off). | Filled | app | Regions | exists |
| `produce.mixerVisible` | Show mixer | Show the mixer (a channel strip per lane) under the timeline. | off | app | PRODUCE toolbar › Mixer | exists |
| `produce.browserVisible` | Show browser | Show the browser (PLAY tracks and project audio) beside the timeline. | on | app | PRODUCE toolbar › Browser | exists |
| `produce.inspectorVisible` | Show inspector | Show the inspector for the selected region or lane. | on | app | PRODUCE toolbar › Inspector | exists |
| `produce.bounceLoopRange` | Bounce the loop range | Bounce only the loop range when one is set (off: the whole arrangement). | off | app | PRODUCE toolbar › Bounce | exists |
| `produce.syncDebounceMs` | Arrangement sync delay | Edits are sent to the engine (`arrangement_set`) after this many ms without another edit. | 150 ms | app | `src/produce/arrangementStore.ts` | internal |
| `produce.undoDepth` | Undo steps | How many arrangement edits Undo remembers. | 200 | app | PRODUCE undo/redo | internal |
| `produce.minRegionMs` | Shortest region | Trimming never makes a region shorter than this many ms. | 10 ms | app | Region trim, split | internal |
| `produce.nudgeMs` | Nudge step | Arrow keys move a selected region by this many ms when Snap is Off. | 10 ms | app | Region keyboard nudge | internal |
| `produce.edgeHandlePx` | Edge handle width | Width of the trim zone at a region's edges, in pixels. | 8 px | app | Region trim handles | internal |
| `produce.tailBars` | Space after the end | Empty bars shown after the last region. | 8 | app | Timeline width | internal |
| `produce.headerWidth` | Lane header width | Width of the lane headers, in pixels. | 232 px | app | Lane headers | internal |
| (const) `LANE_PALETTE` | Lane colors | Colors offered by the lane color picker for a lane with no color of its own (mirrors the `--track-1..8` tokens; a lane with `color: null` follows the theme). | Track palette | app | Lane header color, regions, mixer | internal |
