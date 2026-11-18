# GROOVE rhythm mini-game family

Status: Exploratory/PENDING. This is future planning only; no game contracts,
implementation, tuning values, or settings are approved.

GROOVE is a family of short timing exercises for voice, acoustic instruments
(including bongos and piano), MIDI controllers, and touch. The shared design principle is that visuals reward
rhythmic performance without revealing upcoming beats. The player should train
accuracy, consistency, memory, internal pulse, subdivision, and recovery—not reaction to a chart that
pre-announces the answer. Any cue language, accessibility aid, or difficulty
model remains PENDING until tested with real users and hardware.

## Candidate games

- **Echo** — repeat a heard rhythmic phrase from memory after it finishes; vary voice,
  instrument, MIDI, and touch input as separate input modes.
- **Pulse** — sustain a steady pulse while the metronome fades away and score
  consistency through the silent interval.
- **Crossfire** — practice syncopation and polyrhythms across two rhythmic roles
  while preserving the shared meter.
- **Missing Musician** — perform the absent percussion part of an arrangement while
  listening for the surrounding groove.
- **Groove Builder** — create or import rhythms, then practice reproducing and
  layering them, adding complexity only after timing remains stable.
- **Rhythm Reactor** — accurate playing powers an immersive visual environment;
  the effects respond to performance without displaying the next event.
- **Tempo Survivor** — maintain timing through changing tempos or controlled
  interruptions, with recovery valued over perfection.
- **Ghost Beat** — maintain a rhythm through periods of silence, testing
  internal time rather than visual tracking.
- **Drum Duel** — exchange rhythmic phrases with a virtual musician; evaluate
  placement, feel, memory, and recovery rather than speed alone.

## Shared constraints and open questions

- Keep a visible Play action, with an optional performance-to-start prompt.
  Rhythm games may eventually begin after the player copies a short bongo beat;
  pitched tools may use a note, chord, scale, or interval prompt.
- Input adapters should reduce voice, instrument, MIDI, and touch to timing
  events/summaries; raw audio must not cross the Tauri bridge.
- Visual feedback may show what was played, current pulse health, and earned
  effects, but must not expose future beats or hidden answer timing.
- Difficulty should be phrased in musical terms (subdivision, density,
  steadiness, tempo change, and recovery). Exact thresholds, scoring, colors,
  latency compensation, and accessibility modes are PENDING and belong in the
  settings registry only when implementation begins.
- PENDING: decide whether games share a transport/metronome contract, how
  latency is calibrated per input mode, and how free timing is distinguished
  from intentionally expressive timing.
- PENDING: test whether touch can express the same timing exercises without
  turning the experience into a reaction game.
