# Polyphonic analysis: chords, multiple notes, instrument separation

Status: **proposed (2026-11-17), not started.** Research notes only.

## Today

- `crates/voiceloop-analysis/src/pitch.rs` detects a single pitch at a time.
  Chords are classified as "not periodic enough".
- `theory.rs` can name a chord from a given set of notes (11 shapes: triads,
  sus, 7ths). It cannot detect which notes are sounding.

## Feasibility (without AI models)

| Goal | Approach | Feasibility |
| --- | --- | --- |
| Chord label | Chroma (pitch-class energy) from an FFT, matched against chord templates, smoothed over time | Good on clean guitar/piano |
| Which notes are sounding | Multi-pitch estimation (harmonic summation / spectral peak picking) | Moderate: 2–4 notes on clean sources; weaker on dense voicings, octave doublings and voice + instrument |
| Separate instruments by timbre | NMF with spectral templates | Rough, with artifacts. Not realistic in general without models |

Light AI models, for later (not the current goal):

- **Basic Pitch** (Spotify, Apache-2.0): small polyphonic note-transcription
  model.
- **Demucs-class models:** source separation. Heavier; probably offline only,
  not live.

## Licensing

Essentia is AGPL: it may be used as a reference only, never as code
(AGENTS.md).

## PENDING

- Real-time (live chord display) or after recording (on takes and loop layers)?
- Target sources first: guitar, piano, or voice-plus-instrument?
- Where results appear: Studio, Practice, or both?
