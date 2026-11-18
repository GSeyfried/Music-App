# Polyphonic analysis: chords, multiple notes, instrument separation

Status: **chord recognition core built (2026-11-17) in `voiceloop-analysis`;
not wired to the engine or UI.** Instrument separation not started.

## Today

- `crates/voiceloop-analysis/src/pitch.rs` detects a single pitch at a time.
  Chords are classified as "not periodic enough".
- `theory.rs` can name a chord from a given set of notes (11 shapes: triads,
  sus, 7ths).
- `chord.rs` + `spectrum.rs` (new) recognise chords and sounding notes from
  mixed audio, without models. See below.

## Chord recognition core (`chord.rs`, `spectrum.rs`)

Pipeline per block of mono samples:

1. **Spectrum** (`spectrum::Spectrum`): Hann window, zero-padded radix-2 FFT
   (written in-crate; no FFT crate was available offline and none is needed),
   local-maximum peaks with parabolic interpolation, `min_hz..max_hz`.
2. **Chroma** (`chroma_from_peaks`, `chroma`): each peak adds its magnitude to
   the nearest pitch class (A4 = `tuning_a4_hz`). Peaks at an integer multiple
   of a significant lower peak are treated as overtones and count at
   `harmonic_weight`. Normalised so the top bin is 1. The lowest significant
   non-overtone peak is the bass.
3. **Matching** (`match_chord`): cosine similarity against binary templates
   from `theory::CHORD_SHAPES` on 12 roots (132 templates). Root = bass gets
   `bass_root_bonus`, which resolves shared-note shapes (Fsus4 vs B♭sus2) and
   names inversions (`C/E`). Returns best + confidence + margin + alternatives,
   or `NoChord::Silent` / `NoChord::WeakMatch`.
4. **Sounding notes** (`estimate_notes`): greedy harmonic summation over the
   peaks, up to `max_notes`, as MIDI notes and pitch classes.
5. **Smoothing** (`ChordSmoother`): holds a chord until a different result
   (including "no chord") wins `hold_frames` frames in a row.

`ChordAnalyzer::analyze(&[f32]) -> ChordFrame` runs 1–4 with reused buffers.
All thresholds are fields of `ChordConfig`; see the "Chord analysis" section
of [SETTINGS_INDEX.md](../settings/SETTINGS_INDEX.md).

Tested on synthetic harmonic tones (44.1 kHz, 8192-sample blocks): C, Am, G7,
Fsus4, C/E; ±20 cents detuning; A4 = 415 Hz; single note → no chord; silence
and low noise → silent; 3- and 4-note chords → correct notes.

### Limits

- Only the 11 shapes in `theory` (no 6ths, 9ths, add chords, power chords).
  A bare fifth matches several triads about equally (reported, low margin).
- Nearest-pitch-class mapping: sources more than ~40 cents from the tuning
  reference smear. No automatic tuning estimation.
- Low notes need long blocks: below ~100 Hz adjacent semitones are closer than
  an 8192-sample FFT separates cleanly.
- Overtone attenuation can under-weight a real note that is exactly a
  harmonic of a lower one (e.g. G4 over a C3 bass).
- `estimate_notes` needs the fundamental present, and drops octave doublings
  (absorbed as overtones). 2–4 notes on clean sources.
- Not tested on real recordings yet; voice-plus-instrument, distortion and
  percussion are expected to be unreliable.
- Allocates when the block length changes: analysis thread only.

### Suggested wiring (later step)

- Engine: run a `ChordAnalyzer` on the existing analysis thread over a
  sliding block (e.g. 8192 samples, hop ~2048 ≈ 20 frames/s at 44.1/48 kHz),
  feed `best.map(|c| c.chord)` through a `ChordSmoother`, and emit only on
  change (plus a low-rate confidence update).
- Event (mirrored by hand in `model.rs` / `src/models/domain.ts`):

  ```ts
  type ChordEvent = {
    kind: "chord";
    frame: number;            // input frame of the block end
    chord: { root: number; quality: ChordQuality; bass: number; name: string } | null;
    confidence: number;       // 0..1
    alternatives: { name: string; confidence: number }[];
    notes?: { midi: number; pitchClass: number; cents: number }[];
  };
  ```

- Offline: the same analyzer over a take or loop layer, producing a chord
  timeline (`[{startFrame, endFrame, chord}]`) for display and MIDI export.

## Feasibility (without AI models)

| Goal | Approach | Feasibility |
| --- | --- | --- |
| Chord label | Chroma (pitch-class energy) from an FFT, matched against chord templates, smoothed over time | Good on clean guitar/piano (built) |
| Which notes are sounding | Multi-pitch estimation (harmonic summation / spectral peak picking) | Moderate: 2–4 notes on clean sources; weaker on dense voicings, octave doublings and voice + instrument (simple version built) |
| Separate instruments by timbre | NMF with spectral templates | Rough, with artifacts. Not realistic in general without models |

Light AI models, for later (not the current goal):

- **Basic Pitch** (Spotify, Apache-2.0): small polyphonic note-transcription
  model.
- **Demucs-class models:** source separation. Heavier; probably offline only,
  not live.

## Licensing

Essentia is AGPL: it may be used as a reference only, never as code
(AGENTS.md). The chord core is original code with no new dependencies.

## PENDING

- Real-time (live chord display) or after recording (on takes and loop layers)?
- Target sources first: guitar, piano, or voice-plus-instrument?
- Where results appear: Studio, Practice, or both?
- Engine wiring: block size, hop, and event rate (suggestion above).
- Should `analysis.chord.tuningA4Hz` follow `tuner.referenceA4` automatically?
- Evaluate on real recordings and tune `minConfidence` / `harmonicWeight`.
- More chord shapes (6, 9, add9, power chords) would need additions to
  `theory::CHORD_SHAPES` (shared with `identify_chord`).
- Automatic tuning-offset estimation for sources far from A4 = 440.
