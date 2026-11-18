# Practice Lab: next ideas (2026-11-17)

Status: the three "practice quick wins" (ROADMAP step 2) are **built**:
stop Studio on entering Practice, difficulty modes, preset scale exercises.
Frontend only, covered by vitest (`src/practice/quickwins.test.tsx`) against the
simulated engine; not yet tried in a native launch or on real voice takes.
Notation display is still **proposed**. Settings are listed in
[settings/SETTINGS_INDEX.md](../settings/SETTINGS_INDEX.md).

## Stop Studio when entering Practice

**Done.** Switching to the Practice Lab tab sends the transport's Stop
(`{ type: "stop" }`) when `practice.stopStudioOnEnter` is on (default) and
the looper is running or a track is armed/recording
(`src/practice/studioHandoff.ts`, called from the tab switch in `App.tsx`).

- Chosen for now: **stop**, not pause, and a Studio recording in progress is
  **finished and kept**, exactly as the Stop button does (the engine's stop
  finishes takes; an armed track that hasn't started is dropped).
- Running both at once can come later if a reason emerges.
- PENDING:
  - Confirm stop vs. pause (pausing keeps the playhead position).
  - Confirm finishing (vs. discarding, or asking about) a Studio recording in
    progress when switching tabs.

## Difficulty modes

**Done.** Easy, Medium and Hard are presets over the existing `ScoringConfig`
tolerances (table: `DIFFICULTY_PRESETS` in `src/practice/difficulty.ts`). The
Difficulty control sits at the top of Practice Lab. Choosing a mode re-scores
the open result (when it still has its take) and saves the values as the
default for future attempts, through the same store path as the tolerance
toggles. Editing a tolerance by hand (the Ignore key / tempo / octave toggles)
switches to "Custom", which appears only then. On load, if the engine's saved
tolerances don't match the chosen preset, the control shows the mode they do
match (or Custom) instead of overwriting them.

| Mode | wrongNoteCents | notePitchThresholdCents | noteTimingThresholdMs |
| --- | --- | --- | --- |
| Super Easy | 125 | 50 | 150 |
| Easy | 50 | 35 | 100 |
| Medium (current defaults) | 50 | 25 | 60 |
| Hard | 35 | 12 | 35 |

The Easy and Hard values are starting guesses. PENDING:
- Tune them on real voice takes.
- Whether the Ignore key/tempo/octave toggles should count as "Custom" (they
  do now, as asked) or be independent of difficulty, since the presets don't
  set them.
- No numeric tolerance editor exists yet; Custom keeps whatever values are set.

## Preset scale exercises

**Done (no engine change).** The Library view has a "Scale exercise" card: key
(C..B) and scale (every `ScaleKind`; major, natural/harmonic/melodic minor
first). `src/practice/scales.ts` generates the notes in TypeScript (intervals
match `crates/voiceloop-analysis/src/theory.rs`) and builds a synthetic
`Take`: notes on a start-aligned metronome grid, an exact pitch curve at the
10 ms tracker hop (clarity 1), clock "mapped" with zero latency. It is added
with `practice_import_library` (`replace: false`) and opened like a recorded
reference, so attempts (free or "With click", which lines up bar by bar and
stops after the scale's length) are recorded and scored by the existing engine.

- Defaults (registry `practice.scale.*`, in the card's "Scale options"
  pop-up): tonic in octave 4, up and down, one beat per note, the looper tempo
  (or 80 BPM without a looper output), each note held for 90% of its beat.
- Melodic minor descends as natural minor (classical form).
- PENDING:
  - Octave/range from the user's measured vocal range.
  - Is the reference played as a synthesized tone, or only shown? (Only shown
    now: piano roll/pitch curve and the live overlay.)
  - Melodic minor: offer the jazz form (same up and down)?
  - Generated references carry `formatVersion: 1` and an id `scale-…`; if the
    library format changes, update `PracticeStore.addReference`.
  - A dedicated engine command (e.g. `practice_add_reference` from a note list)
    would avoid building a synthetic take in the frontend; not needed today.

## Notation display

1. **Now:** the existing piano roll, with note names and note colors (see
   [visualization](../visualization/README.md)).
2. **Next:** a single-voice staff, rendered with VexFlow (MIT).
   - Hard part: quantizing sung timing into note values. This builds on the
     existing grid settings (`gridSubdivision`, `gridSnapTolerance`) and the
     tempo estimate.
3. **Later:** score import/export as MusicXML, rendered with
   OpenSheetMusicDisplay (BSD-3). MIDI import already exists (`smf.rs`).

Not feasible: reading scores from PDFs or images (OMR). The main open-source
engine, Audiveris, is AGPL.

PENDING: clef choice (treble/bass/auto from range), key signature (from key
estimation or user-chosen).
