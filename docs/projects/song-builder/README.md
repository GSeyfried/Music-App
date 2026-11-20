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

## Track types (2026-11-19)

One tap adds a track (`TrackTemplate`). Reference check:
[REFERENCE_CHECK_VOICE_TRACKS.md](REFERENCE_CHECK_VOICE_TRACKS.md).

| Track | Input | State |
| --- | --- | --- |
| Voice drums | Mic: Ba (kick), Da (clap), K (snare), Ts (hat). Da is recognised once trained | Built |
| Drum pads | Touch | Built |
| Voice → piano | Mic: sung pitch → nearest **enabled** key (12 on/off keys per track), held while you sing, recorded with its length and loudness, instrument release as the fade | Built; not yet sung on a device |
| Piano | Touch; keys sound while held and record their held length | Built |
| Voice (raw) | Mic audio recorded into the loop, with FX | **PENDING**: engine clip recording/playback |
| Voice + tune | Raw voice pitch-corrected toward the enabled keys | **PENDING**: needs Voice (raw) plus a TD-PSOLA corrector (reference check: ~50–75 ms live latency expected) |

- **Snapping:** the nearest enabled key by pitch distance. Example: with C and
  C♯ off, a sung C♯ goes to D (1 semitone away) rather than B (2 away). Only
  an exact halfway pitch uses the "Between two keys" setting (default lower).
  The sounding key is kept until another is nearer by 15 cents.
- **Mic auto-mute:** while the mic records into the song, the other parts go
  silent (`songBuilder.voiceRecord.mute`), so the phone speaker doesn't leak
  into the mic. Touch pads and keys never mute anything.
- **FX:** up to 3 presets stacked per track, run in order.
- **Mic sensitivity:** one slider with a live meter sets every loudness gate.

## First device test (2026-11-19, iPhone speaker, no headphones)

Findings: timid kick; everything quiet; had to be loud and hits were missed;
most hits read as snare; dropdowns needed several taps. Codex's research and
evaluation: [REFERENCE_EVAL_2026-11-19.md](REFERENCE_EVAL_2026-11-19.md).
Fixed the same day (not yet re-tested on the device):

- iOS session mode `default` instead of Measurement (Apple: Measurement lowers
  playback; it also turns off mic gain) — `audio.ios.sessionMode`.
- One relaxed hit gate: the engine onset detector gets Song Builder's own
  rise/floor (6 dB, −58 dB) and the app gate no longer out-strict it (5 dB,
  −55 dB).
- Voiced-kick rule ("bah", "boots": low zero-crossing rate, little hiss) and a
  darker-centroid limit of 900 Hz; hat is checked first.
- Heavier kicks (higher beater start, more drive for harmonics a phone speaker
  can play, more click); master 1.0 and +6 dB make-up into the limiter; soft
  voice hits play at ≥ 0.65 velocity.
- Part-card tap gesture masked off on the selected card so it no longer
  competes with its menus.

## PENDING

- Voice (raw) and Voice + tune tracks (see Track types).
- Voice → piano: a faster provisional pitch path (YIN window ≈ 35 ms today),
  per-note envelope velocity, octave-leap handling, tap-to-correct notes
  ([reference check](REFERENCE_CHECK_VOICE_TRACKS.md)).
- Mic auto-mute ramps exactly at the first captured frame; today it switches
  when recording starts (after the count-in), with the engine's gain ramp.

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
- From the Codex evaluation: echo-cancelled input for speaker use
  (`setPrefersEchoCancelledInput`); adaptive noise-floor onsets plus spectral
  flux; richer hit features (MFCC/log-mel, voicing, a transient window and a
  body window 10–20 ms later); training that keeps examples or variance,
  standardises features, rejects outliers and can answer "uncertain"; a
  distinct kick body layer (80–140 Hz) and loudness-matched kits; master
  volume in the settings sheet; menus out of nested scroll views; a labelled
  real-voice test corpus with a confusion harness.
