# Engine interface changes on `backend/engine-dsp`

For the frontend developer. Everything here is additive: existing commands,
events, and fields are unchanged, so the current UI keeps working. Nothing
in `src/` was modified on this branch; the TypeScript below is what
`src/engine/bridge.ts` needs when the UI adopts each feature.

All JSON is camelCase. Frame numbers are sample frames.

## Looper

### New commands (`looper_command`)

```ts
| { type: "setSolo"; track: number; solo: boolean }  // any solo on: only soloed tracks sound; mute still wins
| { type: "redo"; track: number }                    // restore the layer removed by the last undo
| { type: "restart" }                                // jump to loop start, keep running, no count-in
```

### New fields

```ts
interface TrackSnapshot {
  solo: boolean;
  redoLayers: number;      // enable the Redo button when > 0
}
interface LooperState {
  inputCushionFrames: number;  // input buffered ahead of the looper; grows after underruns
}
type RejectReason = ... | "nothingToRedo";
```

### Behaviour changes to know about

- **Undo keeps the layer for Redo.** `freeLayers` does not go up after an
  undo. A new recording on that track discards its redo history. If the pool
  is empty, the first Record/Overdub is rejected with `poolExhausted` *and*
  reclaims all redo history; the same command succeeds a moment later.
- **Mute/solo/level/play-stop ramp over about 5 ms** instead of stepping.
- **The output is soft-limited** above 0.8 of full scale rather than clipped.
- **With input latency set**, a free first take stays in `recording` state
  for `latencyFrames` after Stop while its tail arrives (about 20 ms).

## Analysis (pitch, notes, onsets)

Runs automatically whenever audio input is running. `EngineStatus.analysis`
is true while it is active.

### New command

```ts
invoke("set_analysis_config", { config: AnalysisConfig }): Promise<void>
```

Every field is optional and defaults as shown. Applies immediately if audio
is running, otherwise at the next start. Invalid values reject with
`{ kind: "invalid" }` and change nothing.

```ts
interface AnalysisConfig {
  enabled?: boolean;              // true
  tracker?: {
    pitch?: {
      minHz?: number;             // 65   lowest detectable note (sets analysis latency)
      maxHz?: number;             // 1400
      threshold?: number;         // 0.15 YIN threshold; lower = stricter
      windowSeconds?: number;     // 0.02
      silenceDb?: number;         // -50  quieter than this is "silence"
      minClarity?: number;        // 0.75 below this is "aperiodic"
    };
    hopSeconds?: number;          // 0.01 one pitch frame per hop
    medianFrames?: number;        // 3    odd, 1..15
    octaveHoldFrames?: number;    // 8    0 disables octave-jump correction
    stabilityFrames?: number;     // 25
    tuning?: number;              // 440  A4 in Hz
  };
  notes?: { minNoteSeconds?: number; switchSemitones?: number; switchHoldFrames?: number; maxGapFrames?: number };
                                  // 0.08, 0.7, 3, 4
  onsets?: { hopSeconds?: number; riseDb?: number; floorDb?: number; lookbackSeconds?: number; minIntervalSeconds?: number };
                                  // 0.005, 9, -45, 0.02, 0.06
}
```

### New events on `engine://event`

```ts
| { type: "pitch"; payload: PitchUpdate }   // ~30 Hz
| { type: "note";  payload: NoteEvent }     // when a note ends
| { type: "onset"; payload: Onset }         // when an attack is detected

interface PitchUpdate {
  frames: PitchFrame[];          // every frame since the last update, oldest first (about 3)
  sampleRate: number;
  droppedInputFrames: number;    // > 0: analysis fell behind; timestamps have slipped
}
interface PitchFrame {
  frame: number;                 // input-clock frame at the centre of the analysed window
  hz: number | null;             // null when there is no confident pitch
  midi: number | null;           // fractional MIDI note (69 = A4)
  note: { note: number; cents: number } | null;   // nearest note; cents in [-50, 50), + = sharp
  clarity: number;               // 0..1 periodicity; the confidence of this frame
  rmsDb: number;
  stabilityCents: number | null; // std dev of recent pitch; null until 5 frames of history
  unpitched: "silence" | "aperiodic" | "outOfRange" | null;
  octaveCorrected: boolean;
}
interface NoteEvent {
  startFrame: number; endFrame: number;
  note: number;                  // MIDI note
  midi: number;                  // median pitch, fractional
  cents: number;                 // median deviation from `note`
  stabilityCents: number;        // std dev within the note (vibrato reads as its width)
  clarity: number;
  frames: number;
}
interface Onset { frame: number; levelDb: number; riseDb: number }
```

New warning kind: `"analysisUnavailable"` (audio runs, analysis does not).

### Displaying pitch honestly

- Show a note only when `hz !== null`. When it is null, `unpitched` says why;
  show a rest or a dimmed state, not the last value.
- `clarity` near `minClarity` means "probably"; consider dimming below 0.9.
- Cents are meaningful to about ±2 on clean tones (measured on synthetic
  signals). Do not show decimals.
- Analysis latency is about 45 ms: 35 ms of window plus the 10 ms hop. Note
  names lag the voice by that much.
- `frame` is on the **input clock** (frames since capture started), the same
  clock as `level.frame`. It is not the looper transport frame.

## Not exposed yet (engine library only)

Available in `voiceloop-analysis` with tests, with no engine command yet:
`rhythm::analyze_timing`, `rhythm::estimate_tempo`, `melody::compare`,
`theory::{interval_between, Scale, estimate_key, identify_chord}`,
`midi::{notes_to_midi, write_smf}`. Say which the UI needs first.
