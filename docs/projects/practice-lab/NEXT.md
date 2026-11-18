# Practice Lab: next ideas (2026-11-17)

Status: the three "practice quick wins" (ROADMAP step 2) are **built**:
stop Studio on entering Practice, difficulty modes, preset scale exercises.
Frontend only, covered by vitest (`src/practice/quickwins.test.tsx`) against the
simulated engine; not yet tried in a native launch or on real voice takes.
Notation display: the staff is built (see below). Settings are listed in
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
  - Is the reference played as a synthesized tone, or only shown? Answered:
    both. "Play" uses the preview piano (see "Preview playback" below).
  - Melodic minor: offer the jazz form (same up and down)?
  - Generated references carry `formatVersion: 1` and an id `scale-…`; if the
    library format changes, update `PracticeStore.addReference`.
  - A dedicated engine command (e.g. `practice_add_reference` from a note list)
    would avoid building a synthetic take in the frontend; not needed today.

## Notation display

1. **Done:** the piano roll, with note names and note colors (see
   [visualization](../visualization/README.md)).
2. **Built (engraving pass, 2026-11-17):** a single-voice staff rendered with
   VexFlow 5.0.0 (MIT), `src/notation/`. Checked visually in a browser harness
   (dark and light themes, narrow width, colored notes) with synthetic sung
   input; not yet on real voice takes in a native launch.
   - `quantize.ts`: one voice; jitter notes shorter than `notation.minNote`
     merge into a neighbour (a blip inside a held note rejoins it); gaps
     shorter than `notation.minRest` are held over; early releases are held to
     the beat; notes snap to the `notation.gridSubdivision` grid and are spelled
     by metric position (`spellInBar`): simplest value, dots where exact,
     off-beat notes stay inside their beat, the middle of a 4/4 bar stays
     visible, compound beats (6/8) stay whole, rests are per beat and undotted
     in simple meters, an empty bar is a centered whole-bar rest, ties across
     bar lines.
   - `meter.ts`: with no beat grid, `estimateTempo` (inter-onset ratios, prior
     near 100 BPM, least-squares refinement) and `inferMeter(notes, bpm)`
     (accent on downbeats minus notes held across bar lines; 4/4 vs 3/4, 6/8
     when off-beats fall on thirds of the pulse) also find the downbeat, so a
     pickup starts after rests. A take's grid is used as-is.
   - `key.ts`: key from a duration-weighted pitch-class histogram against the
     Krumhansl–Kessler profiles; spelling by key (minor leading tone raised,
     other chromatic notes closest to the key on the line of fifths);
     accidentals once per bar and position.
   - `engrave.ts`: picks the clef with the fewest ledger lines (treble, bass,
     or treble-8 for tenor range) and puts it all together (`planNotation`).
   - `Staff.tsx`: beams by beat (`Beam.generateBeams` with meter groups), stems
     by VexFlow; measure widths follow content, lines wrap and are justified;
     theme ink from `--text` (notes) and `--text-dim` (staff lines); redraws on
     theme or width change; waits for the Bravura font before drawing (drawing
     earlier misplaced stems and spacing).
   - Settings: `notation.meter`, `notation.keySignature`, `notation.clef`,
     `notation.colorNotes` (off: black-ink engraving), `notation.minNote`,
     `notation.minRest`, `notation.scale` (none shown in the Practice UI yet).
   - PENDING: expose the notation settings next to the Note display picker.
   - PENDING: tempo drift over long gridless takes (a single tempo is fitted);
     takes whose tempo estimate is off by a 3:2 or 2:1 ratio still notate
     oddly. The engine's `TakeSummary.tempo` could replace the frontend
     estimate if it is passed to `takeToStaff`.
   - PENDING: triplets (subdivision 3/6 is rounded to a power of two), 2/4 and
     12/8 are never inferred (2/4 can be chosen), no courtesy accidentals.
   - PENDING: internal layout/inference constants (`MIN_MEASURE_CONTENT`,
     `PER_NOTE_PAD`, `CROSS_PENALTY`, `KEY_MIN_CORRELATION`, ...) are module
     constants, not registry entries; move them if they need tuning by users.
3. **Later:** score import/export as MusicXML, rendered with
   OpenSheetMusicDisplay (BSD-3). MIDI import already exists (`smf.rs`).

Not feasible: reading scores from PDFs or images (OMR). The main open-source
engine, Audiveris, is AGPL.

## Live colours vs. the end-of-take report (2026-11-18)

**Pitch: unified.** The live line, the results roll, the note table and both
legends use one set of bands (`pitchBands` in `src/practice/text.ts`), taken
from the difficulty's scoring values:

- green: within `notePitchThresholdCents`
- yellow: up to `wrongNoteCents`
- red: beyond that, which is exactly what the engine scores as "Wrong note"

Before this, the report used a fixed 10¢ / 25¢ scale, and the live line used
the threshold and twice the threshold. A note could be green live, yellow in
the report, and still count as right. The legends now show the current values
(Medium: "Within 25¢ / Within 50¢ / Wrong note"). The tuner's pitch-curve
legend (±5¢ / ±15¢ from the nearest semitone) is a separate thing and is
unchanged.

**Timing: deliberately different.**

- **Live:** the reference is placed once, from the engine's first alignment
  (the first sung note, or the bar grid with the click), and then stays put.
- **Report:** `melody::compare` fits your overall offset (and tempo, with
  Ignore tempo) across all the notes.

So a take that drifts late can show boxes sliding away from the line while you
sing, yet the report can still score the notes on time against your own
average. That is the more useful score, so the live view is not forced onto
it. With the click (both takes on the bar grid) the two agree.

PENDING:
- With **Ignore key** on, the report allows a transposition, but the live
  colours still compare against the untransposed reference.
- Should the live view re-fit its placement after a few notes (a single
  correction rather than continuous movement)?

## Preview playback (2026-11-18)

**Built.** Scales, references and the games' note previews play with the
**Salamander Grand Piano** (CC-BY 3.0, credits in
`src-tauri/resources/piano/CREDITS.md`). The audio runs inside the engine's
output callback, so it plays on the output device and channels you picked,
like the loops and the click.

- Engine: `crates/voiceloop-engine/src/preview.rs`.
  - A fixed 16-voice sampler, mixed after the looper and the monitor.
  - Never recorded: the looper records from the input before this mix.
  - Schedules and sample banks are built on the control thread, then handed
    over and back through `rtrb` rings, so the audio thread never allocates
    or frees.
  - 30 samples, every minor third from A0 to C8: mono, 32 kHz, 2–4 s each,
    levels matched, 5 MB in total.
  - Each note plays the nearest sample, resampled to its pitch.
- Tauri: `preview_play` and `preview_stop`. The samples are bundled as app
  resources and loaded at startup (`Engine::use_preview_samples`).
- Frontend: `src/audio/instruments.ts` keeps the same
  `playPreview`/`stopPreview` API.
  - It tries the engine first.
  - If audio isn't running, outside the desktop app, or with
    `audio.previewThroughEngine` off, it uses the old synthesized Web Audio
    piano on the system's default output.
- Verified: engine unit tests (pitch, timing, release, stop fade, voice
  stealing, bank swap and return, loading the bundled set) and vitest for the
  engine-first and fallback paths. **Not yet heard in the app on real
  hardware.**

PENDING:
- Only one velocity layer, so loudness doesn't change with velocity.
  Salamander has 16 layers if dynamics are wanted later.
- Should previews be heard while the microphone records an attempt? Speakers
  bleed into the mic acoustically. Headphones avoid it; nothing ducks the
  preview yet.
- With "mic only when needed" (hardware-io), the output stream should run
  without the input, so previews still use the engine with the mic off.
