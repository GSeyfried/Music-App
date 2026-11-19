# VoiceLoop (working title)

A modular, local-first music workstation. The current app has two active
workspaces sharing one native audio engine:

- **Studio:** live looping, overdubbing, transport, metronome, routing, metering,
  and live pitch feedback.
- **Practice Lab:** live tuner and pitch history plus reference recording,
  scored attempts, note-by-note feedback, and practice history.

The longer product direction adds Composition, Live Performance, Music Library,
and an offline-first iPhone/iPad experience called VoiceLoop Pocket. These are
directional workspaces, not claims of current implementation. See
[the roadmap](docs/ROADMAP.md) and
[product direction](docs/projects/workstation-buildout/PRODUCT_DIRECTION.md).

Rust audio engine (CPAL) · Tauri 2 desktop shell · React + TypeScript UI. Offline; no server.

> **Status (2026-11-17): active workstation build-out.** Studio looping,
> transport, pitch analysis, hardware settings, MIDI input management, and
> latency calibration are implemented through the UI, with hardware validation
> still incomplete. Project persistence/import/export and the full Practice Lab
> workflow are connected to React and covered by automated tests; real audio,
> recovery, external-DAW, and hardware procedures remain. The roadmap uses
> an end-to-end integration matrix so backend completion is not mistaken for a
> finished user feature.

## Repository layout

```
crates/voiceloop-engine/   Rust audio engine (no Tauri dependency)
src-tauri/                 Tauri 2 shell: commands + event bridge
src/                       React UI (workspaces, components, engine bridge, domain models)
scripts/                   start-project-session (credentials), reference-packages (refs)
reference-packages/        git-ignored reference sources + tracked manifest.json
docs/                      architecture, decisions, roadmap, assessments
```

## Build machine

**macOS is the primary build and runtime target.** Full setup, microphone
permission notes, and expected output are in **[docs/MAC_SETUP.md](docs/MAC_SETUP.md)**.

> The Windows work PC is used for editing only. Do not run `cargo` or `tauri`
> builds there (a Tauri compile overwhelmed it).

## Getting started (macOS)

Run these on the Mac, in order:

```sh
git clone https://github.com/GSeyfried/Music-App.git
cd Music-App
bash scripts/mac-setup.sh     # checks Xcode CLT, Rust, Node; runs npm ci (add --install to install missing tools)
npm run typecheck             # TypeScript, seconds
npm run engine:test           # first Rust compile of the engine (1–3 min) + unit tests
npm run engine:meter          # headless mic test, 3 s; allow microphone access for your terminal app
npm run app:dev               # first Tauri compile (3–10 min); opens VoiceLoop
```

Already cloned? `git pull && npm ci`, then continue from `npm run typecheck`.

In the app: **Studio → Start input**. The meter should move when you speak,
and **Stop input** should turn off the orange mic indicator in the menu bar.
If the meter sits at −100 dB with a warning, grant microphone access (see
[docs/MAC_SETUP.md](docs/MAC_SETUP.md) §3) and restart the terminal.

In the app: **Studio → Audio input → Start input**. Speak or play and the meter
moves. On macOS the first start triggers the microphone permission prompt.
Monitoring (playing input through the speakers) is off.

## Other commands

| Command | What it does |
| --- | --- |
| `npm run engine:test` | Rust engine unit tests (no hardware needed) |
| `npm run engine:meter` | Headless hardware check: list devices, meter the default mic for 3 s |
| `npm run typecheck` | TypeScript check |
| `npm run build` | Type-check and build the frontend into `dist/` |
| `npm run dev` | Frontend only in a browser (engine shows "unavailable") |
| `npm run app:build` | Release build and installers (`target/release/bundle/`) |
| `npm run refs:fetch` / `refs:verify` | Restore and verify reference sources |
| `npm start` | Switch git/GitHub/gcloud/Firebase credentials to the personal account |

## Docs

- [macOS setup](docs/MAC_SETUP.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Decisions](docs/DECISIONS.md), including Rust/Tauri vs JUCE/Tracktion
- [Roadmap](docs/ROADMAP.md)
- [Current end-to-end integration audit](docs/projects/workstation-buildout/UNWIRED_FEATURES.md)
- [Expanded product direction](docs/projects/workstation-buildout/PRODUCT_DIRECTION.md)
- [Track system, tool modes, templates, and presets](docs/projects/track-system/README.md)
- [Sound library, sampler, and instrument articulation](docs/projects/sound-library/README.md)
- [VoiceLoop Pocket focused mobile products](docs/projects/ios/PRODUCT_APPS.md)
- [Reference package assessment](docs/REFERENCE_PACKAGE_ASSESSMENT.md)
- [Reference packages: how to restore](reference-packages/README.md)
