# Settings index

The source of truth for every setting. See [README.md](README.md) for the rules.

Status values: **exists** or **wired** (in the current tree), **proposed** (not built).
Descriptions are user-facing drafts.

Scopes: app / project / track / session.

## Tool modes, track templates, and presets (proposed; see [track-system](../track-system/README.md))

These ids reserve product behavior; they are not present in the registry yet.
Mode controls disclosure and workflow policy. It must not discard hidden track
overrides. Templates create editable track configurations, while presets copy
values and profiles hold learned/calibrated data.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `tools.defaultMode` | Default tool mode | Mode first selected for a tool that supports it. Each tool remembers its own project mode; this preference does not switch the entire app at once. | Supportive | app | Preferences › Tools; new tool/workspace setup | proposed |
| `looper.experienceMode` | Looper mode | Supportive recommends reversible setup/cleanup, Hardware follows a controller-style state machine, and Advanced exposes the complete track configuration. Changing mode does not erase hidden values. | Supportive | project | PLAY header; Add Track; Track Details | proposed |
| `looper.supportivePreset` | Supportive setup | Starting behavior for a new Audio Looper, such as Vocal Loop, Beatbox, Acoustic Instrument, Ambient Texture, or Free Idea. The chosen preset copies inspectable values into the track. | Vocal Loop | track | Add Track template; compact track preset chip; Details › Overview | proposed |
| `looper.hardwareProfile` | Hardware controls | Press/hold/double-press transitions and controller feedback. Factory choices are Single switch, Two switch, and Multitrack tabletop; custom mappings are profiles. | Single switch | project | PLAY hardware surface; MIDI mapping editor | proposed |
| `tracks.defaultTemplate` | New track type | Template selected initially when Add Track opens; it can still be changed in the picker. | Audio Looper | app | Add Track picker | proposed |
| `presets.applyBehavior` | Applying presets | Show a value diff before a preset replaces meaningful track overrides, or apply immediately. Destructive data replacement is never allowed. | Show changes | app | Every preset menu | proposed |
| `help.detailLevel` | Help detail | Supportive explanations emphasize purpose and audible result; Technical also shows processing, timing, CPU/latency and value provenance. | Follow tool mode | app | Tooltips, info sheets and Settings help | proposed |

## Pocket keyboard and voice song building (see [mobile products](../ios/PRODUCT_APPS.md))

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `keyboard.octave` | Keyboard octave | Lowest C of the one-octave digital piano. Octave changes affect new touch notes, not recorded notes. | C4–B4 | project | Pocket one-octave keyboard; octave down/up | wired |
| `keyboard.noteLabels` | Piano note labels | Show note names on the digital piano. Accessibility names remain available when visual labels are hidden. | on | app | Pocket one-octave keyboard | wired |
| `pocket.songBpm` | Pocket song tempo | Established beat grid used to place Pocket notes and hits. | 100 BPM | project | Pocket tempo field | wired |
| `pocket.quantizeSubdivision` | Pocket beat grid | Nearest rhythmic division used by Pocket timing correction. | Eighth notes | project | Pocket beat-grid picker | wired |
| `pocket.quantizeStrength` | Pocket tightness | Blend note times toward the beat grid without replacing their original performance times. | 65% | project | Pocket Tightness control and adjusted preview | wired |
| `keyboard.scaleHighlight` | Highlight allowed notes | Mark notes allowed by the selected key/scale or custom note set. | on | app | Voice Melody and practice keyboards | proposed |
| `keyboard.touchVelocity` | Touch key loudness | Fixed plays every touch at one velocity; Position uses where the key is touched when the platform can report it reliably. | Fixed, 0.8 | app | Digital piano | proposed |
| `voiceMelody.tuneAmount` | Tune | How strongly interpreted instrument pitch moves toward the selected chromatic/key/custom note. Raw measured pitch remains unchanged. If continuous pitch bend is unavailable, expose this as Note lock instead. | 100% (Locked) | track | Voice Melody primary screen | proposed |
| `voiceMelody.noteSetMode` | Allowed notes | Use every chromatic note, a key/scale, or a custom set selected on the keyboard. | Key/scale | track | Voice Melody key chip and keyboard | proposed |
| `voiceMelody.noteChangeHoldMs` | Note-change steadiness | How long a new target must remain confident before the interpreted instrument changes note. Lower is more responsive; higher resists vibrato/noise. | 80 ms | track | Voice Melody Advanced | proposed |
| `voiceMelody.keepRawAudio` | Keep my voice recording | Save the microphone performance beside the interpreted notes. Raw pitch observations are always retained for nondestructive correction. | on | track | Voice Melody recording options | proposed |
| `voicePercussion.tightness` | Beat tightness | Blend recorded trigger times toward the selected grid while preserving original timing for undo/readjustment. | 50% | track | Voice Percussion primary screen | proposed |
| `pocket.resumeLastActivity` | Resume where I left off | Put the most recent unfinished song or practice activity at the top of the Pocket home screen. | on | app | Pocket home | proposed |

### Song Builder (wired: VoiceLoop Pocket SwiftUI app; see [song-builder](../song-builder/README.md))

Pocket's registry is `apps/pocket-ios/Sources/SongBuilder/PocketSettings.swift` (same ids as below; stored in UserDefaults). `pocket.songBpm`, `pocket.quantizeSubdivision` and `pocket.quantizeStrength` above are the values a **new** song starts with; each song stores its own tempo, grid and Tightness. Drum kits, melody voices and FX presets are tables in the engine, `crates/voiceloop-engine/src/song.rs` (`DRUM_KITS`, `MELODY_VOICES`, `FX_PRESETS` → `fx_preset`, built from the desktop effect blocks); starter beats are `StarterBeat.all` in `SongModel.swift`. The `scope` column below is where the value lives in Pocket.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `songBuilder.loopBars` | Loop length (bars) | Length of a new song's loop. Growing repeats existing material; shrinking drops what falls outside (Undo restores). | 2 | project | Song Builder Bars picker (per song) | wired |
| `songBuilder.beatsPerBar` | Beats per bar | Meter of new songs. Starter beats are 4/4 and are trimmed in shorter bars. | 4 | project | — (new songs) | wired |
| `songBuilder.countInBars` | Count-in | Bars of click before recording starts from a stopped song. | 1 | app | Record from stopped | wired |
| `songBuilder.clickMode` | Click | Always / While recording / Off. The count-in always clicks. | Always | app | Click checkbox (Always/Off) | wired |
| `songBuilder.clickVolume` | Click volume | Loudness of the click and count-in. | 0.45 | app | — | wired |
| `songBuilder.masterVolume` | Song volume | Overall Song Builder output level. | 0.85 | app | — | wired |
| `songBuilder.defaultKit` | Default drum kit | Kit a new drum part starts with (Studio, 808, Lo-fi). | Studio | app | Add part | wired |
| `songBuilder.padVelocity` | Pad strength | Velocity of touch-pad hits. | 0.85 | app | Drum pads | wired |
| `songBuilder.melodyNoteBeats` | Piano note length | Length in beats of a note recorded from the touch piano. | 0.5 | app | Melody part piano | wired |
| `songBuilder.touchLatencyMs` | Touch timing offset | Extra milliseconds to move recorded taps earlier, beyond the output latency the engine already removes. | 0 ms | app | Pads, piano | wired |
| `songBuilder.earlyHitBeats` | Early-hit window | Hits this many beats before the first downbeat (during the count-in) still count as the downbeat. | 0.5 beat | app | — (internal) | wired |
| `songBuilder.undoDepth` | Undo steps | How many Song Builder edits Undo can step back through. | 50 | app | Undo/Redo | wired |
| `songBuilder.voiceDrums.listen` | Voice drums | Listen to the mic while a drum part is selected and turn mouth sounds into hits. Opens the mic with reason `songBuilder`. | on | app | "Use voice" checkbox | wired |
| `songBuilder.voiceDrums.monitor` | Hear voice hits | Play the drum sound as soon as a mouth sound is recognised (a few tens of ms late). | on | app | — | wired |
| `songBuilder.voiceDrums.classifier` | Sound recognition | Auto (trained once every sound has enough examples, else rules), Rules, or Trained. | Auto | app | Recognition line under the pads | wired |
| `songBuilder.voiceDrums.minLevelDb` | Voice hit level | Quietest attack (dBFS) that counts as a hit. | -42 dB | app | — | wired |
| `songBuilder.voiceDrums.minRiseDb` | Voice hit sharpness | How suddenly (dB) a sound must jump to count. | 10 dB | app | — | wired |
| `songBuilder.voiceDrums.latencyMs` | Mic timing offset | Extra milliseconds to move recorded voice hits earlier, beyond the input + output latency the engine already removes (`song_locate`). | 0 ms | app | — | wired |
| `songBuilder.voiceDrums.trainingHits` | Training examples | Examples per sound asked for when training, and needed before Auto uses them. | 5 | app | Train my sounds | wired |
| `songBuilder.voiceDrums.kickMinLow` | Kick: low share | Rules: at least this share of energy below 250 Hz is a kick. | 0.5 | app | — (internal) | wired |
| `songBuilder.voiceDrums.kickMaxCentroidHz` | Kick: brightness limit | Rules: darker than this centroid is a kick. | 450 Hz | app | — (internal) | wired |
| `songBuilder.voiceDrums.hatMinHigh` | Hi-hat: high share | Rules: at least this share of energy above 3 kHz is a hi-hat. | 0.45 | app | — (internal) | wired |
| `songBuilder.voiceDrums.hatMinCentroidHz` | Hi-hat: brightness | Rules: brighter than this centroid is a hi-hat. | 3200 Hz | app | — (internal) | wired |
| `songBuilder.appearance` | Appearance (Pocket) | Dark (default), System or Light look for the iOS Song Builder. | Dark | app | iOS Song settings › Appearance | wired (iOS) |
| `songBuilder.haptics` | Haptics | Light taps on pads, piano keys, Record and step edits (iOS). | on | app | iOS Song settings › Haptics | wired (iOS) |
| `songBuilder.padFlashMs` | Pad flash time | How long a drum pad stays lit after a touch, voice or training hit. | 160 ms | app | — (internal; iOS drum pads) | wired (iOS) |
| `songBuilder.gridMinStepPt` | Step grid cell width | Narrowest step-grid cell (pt) before the grid scrolls sideways (iOS). | 18 pt | app | — (internal; iOS step grid) | wired (iOS) |
| `songBuilder.color.<key>` | Song Builder colours | Lane and part colours (hex). Keys and defaults: kick `#FF8A3D`, snare `#4C9DFF`, hat `#34D399`, clap `#B57CFF`, openHat `#2CD3D3`, drums part `#FF5C7A`, melody part `#FFC145`, record `#FF3B4E`. | see description | app | — (internal; iOS pads, grid, part cards, Record) | wired (iOS) |
| `analysis.hits.enabled` | Hit timbre | Engine: describe each onset's timbre (`hit` event). | on | app (engine `AnalysisConfig.hits`) | — | wired |
| `analysis.hits.windowSeconds` | Hit window | Engine: sound after the onset that is described. | 0.04 s | app (engine) | — | wired |
| `analysis.hits.lowHz` | Hit low band edge | Engine: upper edge of the low band. | 250 Hz | app (engine) | — | wired |
| `analysis.hits.highHz` | Hit high band edge | Engine: lower edge of the high band. | 3000 Hz | app (engine) | — | wired |

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

## Noise reduction (wired: `src/noise`, `crates/voiceloop-engine/src/noise.rs`, `voiceloop-analysis/src/denoise.rs`)

All `noise.*` values are sent to the engine together with `set_noise_config` (`useNoiseConfigSync`). Profiles and per-layer settings are project data (`noiseProfiles`, `LayerDoc.noiseReduction`), not settings.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `noise.learnMode` | Learn the room's noise | How the app learns background noise: the Learn room button, the quiet moment before each take (between record and the first sung note), or both. | Both | app | Advanced options › Noise reduction; Learn room button (Studio input area) | wired |
| `noise.learnSeconds` | Learn room length | Seconds Learn room records (click muted meanwhile). | 3 s | app | Advanced options › Noise reduction; Learn room countdown | wired |
| `noise.autoApply` | Auto-apply to new layers | Once a profile exists, new layers get noise reduction with the newest profile (raw kept). | On | app | Advanced options › Noise reduction; noise pop-up | wired |
| `noise.defaultAmountDb` | Default reduction | dB that noise is turned down when a layer's noise reduction is switched on. | 12 dB | app | Advanced options › Noise reduction; Track details layer list | wired |
| `noise.defaultSensitivity` | Default sensitivity | dB above the learned noise a sound must be to count as wanted. Higher removes more. | 6 dB | app | Advanced options › Noise reduction | wired |
| `noise.defaultSmoothing` | Default smoothing | 0..1 smoothing of the reduction over time and frequency (fewer artefacts). | 0.5 | app | Advanced options › Noise reduction | wired |
| `noise.clickBleed` | Before-take learning with the click | Skip clicks and knocks (frames far above the median) or learn before takes only while the metronome is off. | Skip clicks and knocks | app | Advanced options › Noise reduction | wired |
| `noise.minAutoMs` | Shortest before-take sample | Room sound before a take shorter than this is not used. | 500 ms | app | internal | internal |
| `noise.autoMaxSeconds` | Longest before-take sample | At most this much room sound before a take is used. | 8 s | app | internal | internal |
| `noise.onsetMarginMs` | Margin before the first note | Room sound this close to the first voiced frame is left out. | 60 ms | app | internal | internal |
| `noise.transientRejectDb` | Click rejection threshold | Frames this far above the median frame are skipped (before-take learning). | 6 dB | app | internal | internal |
| `noise.fftSize` | Analysis size | STFT size (hop 1/4, Hann) for learning; a layer is processed at its profile's size. | 2048 | app | internal | internal |
| `noise.layerIndicator` | Show noise reduction on layer chips | NR badge on layers that play a noise-reduced buffer. | On | app | Advanced options › Noise reduction; Track details layer chips | wired |
| (engine) `noise::PRETAKE_WAIT`, `MAX_ATTEMPTS` | Auto-apply wait, retries | How long auto-apply waits for a before-take profile (2 s); fetch/swap retries before a layer job fails (4). Not user-facing. | 2 s / 4 | app | `crates/voiceloop-engine/src/noise.rs` | internal |

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

## Sound library and instruments (proposed; see [sound-library](../sound-library/README.md))

Instrument patches store their resolved articulation values in the project.
These registry entries supply defaults and safety/resource policy for newly
created or imported sounds.

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `library.importStorage` | Imported sound storage | Copy imported audio into the local VoiceLoop library, or reference its current file location. Copying is safer if the original moves. | Copy into library | app | Library › Import | proposed |
| `library.projectEmbedPolicy` | Keep project sounds portable | Which used sounds are copied into a saved project: all, non-factory only, or references only. References only may create missing sounds on another device. | Embed non-factory sounds | app | Preferences › Library; Save/portable export summary | proposed |
| `library.autoAnalyze` | Analyze new sounds | Generate waveform peaks, duration, loudness and a pitch/root-note suggestion after recording or import. Suggestions retain confidence and never rewrite audio. | on | app | Record/Import sound progress; Library details | proposed |
| `instrument.ringDefault` | Ring / Linger | Friendly starting amount for how long a newly created instrument continues while held or after release. Each instrument preset maps this macro to its natural tail, sustain loop and envelope; reverb remains separate. | 50% | track | Compact instrument track; Details › Shape | proposed |
| `instrument.playbackModeDefault` | New sample behavior | Starting playback rule for an unmapped imported sample: One-shot, Gated, Natural, Sustain loop, or Latch. Drum-kit templates override this per pad. | One-shot | track | New Sample; Details › Shape | proposed |
| `instrument.attackMsDefault` | New instrument attack | Starting fade-in time for a newly created sampler instrument. Instrument presets may override it. | 2 ms | track | Details › Shape › Envelope | proposed |
| `instrument.decayMsDefault` | New instrument decay | Time from attack peak to the sustain level for a newly created sampler instrument. | 0 ms | track | Details › Shape › Envelope | proposed |
| `instrument.sustainDefault` | New instrument sustain | Level held after decay while a sustain-capable note remains on. A one-shot or naturally decaying sample may still become silent. | 100% | track | Details › Shape › Envelope | proposed |
| `instrument.releaseMsDefault` | New instrument release | Fade time after Note Off for a newly created sampler instrument. | 60 ms | track | Ring/Linger macro; Details › Shape › Envelope | proposed |
| `instrument.sustainLoopCrossfadeMs` | Sustain-loop smoothing | Crossfade at loop boundaries so a held sample repeats without clicking. | 20 ms | track | Sustain-loop editor | proposed |
| `instrument.maxVoices` | Instrument polyphony | Most sample/synth voices one instrument track may sound at once. The engine preallocates them and steals a voice at the limit. | 32 | app | Instrument diagnostics; Advanced › Voice behavior | proposed |
| `instrument.voiceStealPolicy` | At the voice limit | Which voice ends when polyphony is full. Prefer already-released/quiet voices before the oldest held voice. | Released, then quietest | app | Advanced › Voice behavior | proposed |
| `instrument.maxHeldSeconds` | Longest held note | Optional safety timeout for latch or sustain-loop notes. Off permits a note to continue until Note Off, unlatch, Stop/All Notes Off, or voice stealing. | Off (unlimited) | app | Advanced › Voice behavior; stuck-note warning | proposed |

## PRODUCE engine (exists: `crates/voiceloop-engine/src/arrangement.rs`, `voiceloop-project/src/arrangement.rs`; see [produce](../produce/README.md))

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| (engine) `arrangement::ARRANGEMENT_DECLICK_FRAMES` | Arrangement start/stop fade | Fade when arrangement playback starts or stops, so starting inside a region or stopping never clicks. Not user-facing. | 64 frames | app | PRODUCE play/stop | exists (engine internal) |
| (engine) `arrangement::MAX_SCHEDULED_REGIONS` | Audible region limit | Most audible regions one playback schedule holds. Not user-facing. | 4096 | project | PRODUCE playback | exists (engine internal) |
| (project) `arrangement::MAX_PEAK_BINS` | Asset waveform resolution | Peak bins stored per asset for drawing regions (part of the engine/UI contract). | 1024 | project | PRODUCE region waveforms | exists (engine internal) |
| (engine) bounce format | Bounce format | `arrangement_bounce` writes mono 24-bit WAV at the arrangement rate. PENDING: a user choice like the export options. | mono, 24-bit | app | PRODUCE Bounce | exists (engine internal) |
| (engine) capture repeats range | Bring-in repeats | Loop cycles one captured region spans (clamped). The UI default lives in its own `produce.*` setting. | 1..1024 | session | PRODUCE browser "Bring in from Play" | exists (engine internal) |

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

## Effects (exists: `src/effects/`; see [effects](../effects/README.md))

| ID | Label | Description | Default | Scope | Connected element | Status |
| --- | --- | --- | --- | --- | --- | --- |
| `effects.maxPerTrack` | Filters per track | Most filters one track (or lane) can hold; the engine reserves `MAX_EFFECTS_PER_TRACK` (8). | 8 | app | Add filter button (disabled at the limit), paste | internal |
| `effects.pasteMode` | Paste filters | Pasting adds the copied filters after the track's own (append) or replaces its chain. | Add to the end | app | Settings › Effects; effect context menu, ⌘V | exists |
| `effects.boxLayout` | Filter boxes | Filter boxes under a PLAY track card's controls, or beside them. | Below | app | Settings › Effects; PLAY track cards | exists |
| `effects.controlStyle` | Filter controls | Knobs (drag up/down, arrow keys) or sliders for filter parameters. | Knobs | app | Settings › Effects; every effect box | exists |
| `effects.stemsWet` | Stems include filters | Exported stems rendered with each track's chain (on) or dry; sent as `RenderOptions.wet`. Mixdown and bounce always include filters. | on | app | Settings › Effects; Project › Stems… | exists |
| `effects.presets` | Filter presets | Chains saved with "Save chain as preset" (`{ id, name, chain }[]`). Built-in presets live in `src/effects/presets.ts`. | none | app | Presets menu on every chain | exists |
| `effects.sendIntervalMs` | Filter drag update interval | While a control is dragged, `setTrackEffects` is sent at most this often (ms). | 30 ms | app | PLAY effect boxes | internal |
| `effects.echoTimeoutMs` | Filter edit confirm timeout | How long an edited chain is shown before falling back to the engine's copy when no snapshot confirms it (ms). | 1500 ms | app | PLAY effect boxes | internal |
| `effects.knobDragPixels` | Knob drag distance | Vertical drag (px) for a knob's whole range; Shift is 10x finer. | 160 px | app | Effect knobs | internal |
| (engine) `EFFECT_CROSSFADE_MS` | Filter change crossfade | When a track's chain changes structure (a filter added, removed, reordered, or a choice such as filter type changed), when its DSP is installed, and when a freeze swaps in the rendered layer, old and new sound crossfade over this long. Also how long a layer takes to move into or out of the chain's scope. | 10 ms | app | `crates/voiceloop-engine/src/looper/mod.rs` | internal |
| (engine) `PARAM_SMOOTH_MS` | Filter parameter glide | A changed continuous parameter glides to its new value over this long (no zipper noise). | 20 ms | app | `crates/voiceloop-dsp/src/chain.rs` | internal |
| (engine) `BYPASS_RAMP_MS` | Filter bypass ramp | Bypass, mix and chain-bypass changes ramp over this long; a fully bypassed filter is then skipped (bit-exact dry). | 10 ms | app | `crates/voiceloop-dsp/src/chain.rs` | internal |
| (engine) `FREEZE_FRAMES_PER_BLOCK` | Freeze render pace | Frames of a Freeze render done per audio callback (two passes around the loop). | 8192 | app | `crates/voiceloop-engine/src/looper/mod.rs` | internal |
| (engine) `DELAY_MAX_MS` | Longest delay | The delay buffer size; synced delays are clamped to it. | 2000 ms | app | `crates/voiceloop-dsp/src/fx.rs` | internal |
| (engine) `AUTO_FILTER_ENV_FULL_SCALE` | Auto-filter envelope sensitivity | Input level (linear peak) at which the envelope-driven auto filter reaches its full depth. | 0.25 (−12 dBFS) | app | `crates/voiceloop-dsp/src/fx.rs` | internal |
| (engine) `PHASER_SPAN_OCT` | Phaser sweep span | Octaves either side of the phaser centre at depth 1. | 2 | app | `crates/voiceloop-dsp/src/fx.rs` | internal |
| (engine) `REVERB_OUTPUT_GAIN` | Reverb level | Output gain of the reverb's wet signal. | 0.35 | app | `crates/voiceloop-dsp/src/fx.rs` | internal |
| (engine) `default_mix` | Default mix per kind | The engine's copy of `effects.<kind>.mix`: what a missing `mix` means. Delay 0.35, reverb 0.3, chorus/flanger/phaser 0.5, others 1. | per kind | app | `crates/voiceloop-dsp/src/spec.rs` | internal |

### Effect parameter defaults (`effects.<kind>.<param>`, generated from `EFFECT_DEFS`)

Every entry is an `internal` app setting: the value a new effect of that kind starts with. Ranges and defaults mirror the effects README parameter table; `mix` is the starting dry/wet.

| Kind | Settings (range, default) |
| --- | --- |
| Gain (Tone) | `effects.gain.mix` (0..1, 1); `effects.gain.gainDb` (-36..24 dB, 0) |
| Filter (Tone) | `effects.filter.mix` (0..1, 1); `effects.filter.type` (lowpass / highpass / bandpass / notch / lowshelf / highshelf / peak; lowpass); `effects.filter.cutoffHz` (20..20000 Hz, 1200); `effects.filter.q` (0.1..18, 0.707); `effects.filter.gainDb` (-24..24 dB, 0) |
| 3-band EQ (Tone) | `effects.eq3.mix` (0..1, 1); `effects.eq3.lowDb` (-24..24 dB, 0); `effects.eq3.lowHz` (40..800 Hz, 200); `effects.eq3.midDb` (-24..24 dB, 0); `effects.eq3.midHz` (200..8000 Hz, 1000); `effects.eq3.midQ` (0.2..8, 0.9); `effects.eq3.highDb` (-24..24 dB, 0); `effects.eq3.highHz` (1000..16000 Hz, 4000) |
| Noise gate (Dynamics) | `effects.gate.mix` (0..1, 1); `effects.gate.thresholdDb` (-90..0 dB, -50); `effects.gate.rangeDb` (-90..0 dB, -60); `effects.gate.attackMs` (0.1..50 ms, 1); `effects.gate.holdMs` (0..500 ms, 30); `effects.gate.releaseMs` (5..2000 ms, 120) |
| Compressor (Dynamics) | `effects.compressor.mix` (0..1, 1); `effects.compressor.thresholdDb` (-60..0 dB, -18); `effects.compressor.ratio` (1..20 ratio, 3); `effects.compressor.kneeDb` (0..24 dB, 6); `effects.compressor.attackMs` (0.1..200 ms, 10); `effects.compressor.releaseMs` (5..2000 ms, 120); `effects.compressor.makeupDb` (0..24 dB, 0) |
| Delay (Space) | `effects.delay.mix` (0..1, 0.35); `effects.delay.timeMs` (1..2000 ms, 375); `effects.delay.sync` (off / 1/4 / 1/8 / 1/8d / 1/16 / 1/4t; off); `effects.delay.feedback` (0..0.95, 0.35); `effects.delay.toneHz` (500..16000 Hz, 6000) |
| Reverb (Space) | `effects.reverb.mix` (0..1, 0.3); `effects.reverb.size` (0..1, 0.5); `effects.reverb.decaySec` (0.1..12 s, 1.8); `effects.reverb.damping` (0..1, 0.5); `effects.reverb.preDelayMs` (0..200 ms, 20) |
| Saturation (Character) | `effects.saturation.mix` (0..1, 1); `effects.saturation.driveDb` (0..36 dB, 12); `effects.saturation.mode` (soft / hard / tube / fold; soft); `effects.saturation.toneHz` (500..16000 Hz, 8000); `effects.saturation.outputDb` (-24..12 dB, -3) |
| Bitcrusher (Character) | `effects.bitcrusher.mix` (0..1, 1); `effects.bitcrusher.bits` (1..16 bits, 8); `effects.bitcrusher.downsample` (1..32, 4) |
| Auto filter (Movement) | `effects.autoFilter.mix` (0..1, 1); `effects.autoFilter.type` (lowpass / bandpass / highpass; lowpass); `effects.autoFilter.baseHz` (40..8000 Hz, 400); `effects.autoFilter.depthOct` (0..6 oct, 3); `effects.autoFilter.source` (lfo / envelope; lfo); `effects.autoFilter.rateHz` (0.05..20 Hz, 1); `effects.autoFilter.sync` (off / 1/1 / 1/2 / 1/4 / 1/8; off); `effects.autoFilter.q` (0.5..18, 4); `effects.autoFilter.envAttackMs` (1..200 ms, 10); `effects.autoFilter.envReleaseMs` (10..2000 ms, 200) |
| Chorus (Movement) | `effects.chorus.mix` (0..1, 0.5); `effects.chorus.rateHz` (0.05..5 Hz, 0.8); `effects.chorus.depthMs` (0..15 ms, 4); `effects.chorus.delayMs` (5..40 ms, 15); `effects.chorus.feedback` (0..0.9, 0) |
| Flanger (Movement) | `effects.flanger.mix` (0..1, 0.5); `effects.flanger.rateHz` (0.05..5 Hz, 0.3); `effects.flanger.depthMs` (0..8 ms, 2); `effects.flanger.delayMs` (0.1..10 ms, 1); `effects.flanger.feedback` (-0.95..0.95, 0.6) |
| Phaser (Movement) | `effects.phaser.mix` (0..1, 0.5); `effects.phaser.rateHz` (0.05..5 Hz, 0.5); `effects.phaser.stages` (2 / 4 / 6 / 8 / 12; 6); `effects.phaser.depth` (0..1, 0.8); `effects.phaser.feedback` (0..0.9, 0.5); `effects.phaser.centerHz` (100..4000 Hz, 800) |
| Tremolo (Movement) | `effects.tremolo.mix` (0..1, 1); `effects.tremolo.rateHz` (0.1..20 Hz, 5); `effects.tremolo.depth` (0..1, 0.6); `effects.tremolo.shape` (sine / triangle / square; sine); `effects.tremolo.sync` (off / 1/4 / 1/8 / 1/16; off) |
| Ring mod (Movement) | `effects.ringMod.mix` (0..1, 1); `effects.ringMod.freqHz` (1..4000 Hz, 220) |
