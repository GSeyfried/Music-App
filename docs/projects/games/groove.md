# GROOVE rhythm mini-game family

Status: **Echo first slice implemented 2026-11-18.** Other family members remain
exploratory/PENDING.

## Implemented: Echo

Echo plays a generated two-drum phrase after a four-click count-in, then gives
another four-click count-in before a player-controlled performance window. The
answer starts on its first hit and ends only when the player presses Done, so a
slow answer is not failed. Scoring fits the answer's tempo and evaluates its
rhythmic proportions. The player can answer on two touch pads / F and J keys,
with threshold-gated acoustic attacks, or with configurable low/high MIDI notes
from an external drum or controller.

The performance view shows accepted system beats as they occur and accepted
player beats as they arrive, using distinct low/high shapes; it never reveals a
future beat. After the player finishes, results show matched, missed and extra
hits, tempo-fitted timing error, answer tempo, drum-choice accuracy where
applicable, a post-performance timeline, and a saved best score per difficulty.
Scores of 80+ automatically advance difficulty; Hard success raises tempo.

The mode dropdown also includes **Free drums**, an unscored input visualizer for
testing pads, F/J, a microphone, or external MIDI before starting Echo.

PENDING real-hardware tuning: microphone event-delivery latency, the −24 dBFS / 18 dB
noise thresholds for bongos versus hand claps, the 140 ms proportional timing
window, phrase density, synthesized drum sound, external drum note defaults,
and whether count-ins should vary by difficulty.

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
