# Song Builder (VoiceLoop Pocket)

Status: **built 2026-11-19 as a native SwiftUI app over the Rust engine
([D-021](../../DECISIONS.md)); engine tested; SwiftUI screen built and checked
through offscreen snapshots; not yet run with a real voice or on a device.** Product definition:
[ios/PRODUCT_APPS.md](../ios/PRODUCT_APPS.md) "Voice Song Builder". App layout
and build: [ios/SWIFT_APP.md](../ios/SWIFT_APP.md). Settings:
[SETTINGS_INDEX.md](../settings/SETTINGS_INDEX.md) "Song Builder".

A loop of 1–8 bars made of stacked parts: voice drums (beatboxing) and touch
melody, with quantization, a few kits, starter beats and small FX presets.

## How it works

```text
mic ─▶ engine analysis ─ onset ─▶ HitDescriber (percussion.rs) ─ `hit` event (timbre summary, no audio)
                                                                     │
SwiftUI SongStore ◀──────────────────────────────────────────────────┘
  classify kick/snare/hat (VoiceDrums.swift: rules | trained | auto)
  song_trigger  → engine plays the drum now (monitor)
  song_locate {inputFrame} → beat the performer heard → add hit at its raw beat
  song_set {song} (events at tightened beats) → engine song player (song.rs) on the output clock
```

- **Voice drums.** For each attack the engine measures band energy (low / mid
  / high), centroid, zero-crossing rate and decay over 40 ms
  (`voiceloop-analysis/src/percussion.rs`). Pocket gates the hit by level and
  rise, then classifies it. Recognition has three modes: **Rules** (works
  untrained), **Trained** (nearest average of the user's own sounds) and
  **Auto** (the default: trained once each sound has enough examples). The mic
  opens only while a drum part is selected with "Use voice" on, or during
  training (input reason `songBuilder`).
- **Timing.** The song plays inside the engine's output callback, so playback
  is sample-accurate. A voice hit's input frame is mapped through the input and
  output stream anchors to the output frame that was *heard* when it was
  performed, with the reported input and output latency removed. A touch is
  placed at the beat heard "now". The setting offsets
  (`voiceDrums.latencyMs`, `touchLatencyMs`) add to that.
- **Quantization (Tightness).** Hits keep their raw beat. Tightness (Natural →
  On the grid; grid 1/4, 1/8, 1/8 triplet, 1/16) is applied when the song is
  sent to the engine and drawn, so it's always reversible. A hit just recorded
  is marked so it doesn't play a second time in the same pass.
- **Sounds.** Three synthesized kits (Studio, 808, Lo-fi) with kick, snare,
  hi-hat, clap and open hat. Melody voices: Piano, Soft synth, Bass. FX presets
  per part: Clean, Room, Big hall, Echo (tempo-synced), Punch, Lo-fi, Radio.
  The presets are chains of the desktop effect blocks (`voiceloop-dsp`), on up
  to 8 part buses whose tails continue across edits.
- **Editing.** Four starter beats, a tappable step grid, mute/solo/level,
  duplicate/delete, Undo/Redo, and local autosave of the song and voice
  training.

## Verification

| Stage | State |
| --- | --- |
| Native/DSP | `percussion.rs` tests (kick/snare/hat shapes distinct on synthetic signals); `song.rs` tests: sample-accurate placement, looping, count-in, skip-pass, position kept across edits, every kit/lane/preset/voice finite and audible, input→heard-beat clock maths |
| Rust service/state | All engine tests pass; `voiceloop-ffi` test exercises commands, errors and events over the C interface |
| Bridge | Swift `EngineBridge` builds and links for the iOS simulator |
| UI | SwiftUI Song Builder built (transport, song controls, part cards with step grid / note strip, voice pads with training, one-octave piano, settings sheet, dark/light, iPad column); verified via `-pocket.snapshotDir` renders, not by hand |
| Functional/hardware | **Not done:** simulator screens can't be captured on this Mac (see SWIFT_APP.md); no real-voice, device or latency test yet |

## Reference check

Lightweight desk check from known behaviour of comparable tools (no forum
skim yet). GPL/AGPL tools were not consulted.

| Tool | Relevant behaviour |
| --- | --- |
| Ableton Live "Convert Drums to MIDI" | Offline kick/snare/hat split by spectrum. Works on clean beatbox, but often confuses snare and hat on breathy sounds. |
| Koala Sampler / pad samplers | Big pads, a few one-tap FX, loop-and-overdub. Recording is aligned to the heard click. |
| GarageBand Live Loops / Drummer | Starter patterns and kits for instant results. Quantize is a reversible region setting. |
| Beatbox-to-MIDI trainers (various) | Per-user training clearly beats fixed rules. Users want a "what did it hear" readout. |

Gaps and known problems:

- Snare/hat confusion on breathy sounds: **handled** by training and the
  "Heard" readout. Thresholds still need tuning on real voices (**PENDING**).
- Live monitoring latency: **handled**. Hits are recorded at the performed
  time, so playback is on time; monitoring can be turned off.
- Speaker bleed retriggering drums without headphones: **PENDING** (gate
  thresholds only).
- Reversible quantize, starter patterns, kits: **handled**.

## PENDING

- Real beatboxing on a device: recognition thresholds and gate defaults.
- Keep the raw voice recording beside the hits (only hit events are stored
  today).
- Voice Melody (sing → notes, Tune); melody parts are touch-only.
- Sampled piano and sample-based kits from the
  [sound library](../sound-library/README.md) (synthesized for now).
- Several songs, project packages, M4A/WAV export and the share sheet.
- A per-part Tightness override; song rename UI.
- Engine events for melody/hit playback flashes (the grid lights as the playhead passes; piano keys light only on touch).
- Background audio; idle CPU (part effect chains keep running while stopped,
  to finish tails).
- Forum and issue-tracker skim for the reference check.
