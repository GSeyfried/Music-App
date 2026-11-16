# Decisions

Format: context → decision → consequences. Status is **Accepted**, **Proposed**, or **Open**.

## D-001 Rust engine + Tauri 2 shell + React UI (Accepted, 2026-11-16)

**Context.** We need low-latency recording and looping, native pitch and rhythm
analysis, offline operation, macOS first with iOS later, and as much reuse of
established components as possible. Alternatives considered: a JUCE app (C++
UI and engine), a Tracktion Engine + JUCE app, and a Tracktion/JUCE engine
behind a webview UI.

**Decision.** Keep Rust (CPAL, rtrb) for the engine and Tauri 2 + React for the shell and UI.

**Rationale.**

| Factor | Rust + Tauri | JUCE / Tracktion |
| --- | --- | --- |
| Already-built DAW features (clips, tempo maps, launcher, plugins, rendering) | Must build | **Large head start** |
| Looper core (record, overdub, sync, undo) | Must build (moderate) | Partial: loop recording and retrospective buffer exist; looper UX still custom |
| Practice Lab analysis (pitch, onset, comparison) | Must build | Must build (only Beat/TempoDetect exist) |
| Licensing for a closed or App Store app | Permissive crates (MIT/Apache) | GPL-3 (Tracktion) / AGPL-3 (JUCE) or paid licences |
| UI iteration speed | React | JUCE GUI (slower) or a hybrid with a webview bridge |
| Memory safety in the realtime engine | Rust | C++ |
| iOS | Tauri 2 mobile + CPAL iOS (less proven) | JUCE iOS (mature) |
| Plugin hosting (AU/VST3) | Not available in mature form | **Mature** |

The product's distinguishing half (Practice Lab) gets little from Tracktion,
and the looper is a much smaller problem than a full DAW. The licensing cost
or copyleft obligation of JUCE/Tracktion is significant for a personal project
that may ship on the App Store. Rust also gives us safety in exactly the code
that is hardest to debug.

**Consequences.** We build the looper, latency compensation, and analysis
ourselves, using Tracktion and mloop as design references (not code sources).
We have no third-party plugin hosting for now.

**Revisit if:** (a) AU/VST3 hosting becomes a must-have; (b) CPAL on macOS
cannot deliver stable latency or duplex alignment after M1 measurement;
(c) iOS via Tauri proves impractical in M5. In case (a), a Tracktion-based
sidecar engine under a commercial licence is the fallback.

## D-002 Engine independent of Tauri (Accepted)

The engine is its own crate with a `Box<dyn Fn(EngineEvent)>` sink and a
command handle. Tauri is one consumer and the headless example is another.

## D-003 Control thread owns streams; rtrb for realtime communication (Accepted)

`cpal::Stream` is not `Send` on every platform, and UI threads must never touch
audio state. One control thread owns streams and serialises commands. The
realtime callback communicates only through preallocated wait-free SPSC rings
and drops data rather than blocking.

## D-004 ~30 Hz summarised events to the UI; no raw audio over IPC (Accepted)

The meter window is 10 ms on the audio thread. The control thread emits the
latest RMS and the max peak every 33 ms.

## D-005 Frames on the device clock are the time base (Accepted)

All positions are `u64` frames. Bars and beats are derived. A tempo map is
deferred; constant tempo per session for now.

## D-006 Reference sources are git-ignored and pinned (Accepted)

`reference-packages/*/` is ignored. `manifest.json` (tracked) pins URL, SHA,
and licence. `scripts/reference-packages.mjs` restores exactly those commits.

## D-007 No code from GPL/AGPL references (Accepted)

Tracktion (GPL-3), JUCE (AGPL-3), mloop (AGPL-3), and Essentia(.js)
(AGPL-3) are read for design only. Translating their code into Rust is still
a derivative work, so we write independent implementations from public
algorithms (papers) or permissive sources.

## D-008 Device identity = device name (Open)

CPAL 0.16 offers no stable device id, so names are used. Duplicate names or
renames are possible. Revisit when CPAL exposes ids.

## D-009 Model types mirrored by hand in Rust and TS (Open)

Revisit with `ts-rs` or `specta` once the models stabilise (M1).

## Open tradeoffs

- **Duplex alignment:** separate CPAL input and output streams have independent
  callbacks. Loop alignment needs measured round-trip latency (planned loopback
  calibration in M1) and possibly drift correction (rubato).
- **Time stretch / pitch correction backend:** Signalsmith Stretch (MIT, C++) is
  the lead candidate. FFI build complexity on iOS is untested.
- **Noise suppression:** RNNoise is speech-tuned and stays optional and off by default.
