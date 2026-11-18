# Practice Lab: next ideas (2026-11-17)

Status: **proposed, not started.** Settings are listed in
[settings/SETTINGS_INDEX.md](../settings/SETTINGS_INDEX.md).

## Stop Studio when entering Practice

- Opening Practice Lab stops loop playback (`practice.stopStudioOnEnter`,
  default on).
- Running both at once can come later if a reason emerges.
- PENDING:
  - Stop or pause (pausing keeps the playhead position)?
  - What happens to a Studio recording in progress when switching tabs: finish
    it, discard it, or ask?

## Difficulty modes

Easy, Medium and Hard are presets over the existing `ScoringConfig` tolerances.
Editing any tolerance by hand switches to "Custom".

| Mode | wrongNoteCents | notePitchThresholdCents | noteTimingThresholdMs |
| --- | --- | --- | --- |
| Easy | 50 | 35 | 100 |
| Medium (current defaults) | 50 | 25 | 60 |
| Hard | 35 | 12 | 35 |

The Easy and Hard values are starting guesses. PENDING: tune them on real voice
takes.

## Preset scale exercises

- Generate references from `ScaleKind` plus a tonic, instead of entering notes by
  hand:
  - first: major, natural minor, harmonic minor, melodic minor
  - eventually: every `ScaleKind`, in any key
- Rust theory code is in `crates/voiceloop-analysis/src/theory.rs`.
- PENDING:
  - Up only, or up and down?
  - Note length and tempo.
  - Octave/range, perhaps chosen from the user's measured vocal range.
  - Is the reference played as a synthesized tone, or only shown?

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
