# Project documents

Each project we build out gets its own folder here:

```
docs/projects/<project-name>/
  README.md       overview: goal, scope, status
  decisions.md    decisions made and why (optional)
  notes.md        working notes, open questions marked PENDING (optional)
```

Use lowercase-kebab-case folder names (for example `audio-player/`).

## Projects

| Project | Status |
| --- | --- |
| [scaffold-01](scaffold-01/README.md): native foundation and reference library | Historical milestone complete; macOS build subsequently verified |
| [looper-01](looper-01/README.md): Milestone 1A native looper engine | Implemented and automated-tested; current hardware checklist remains incomplete |
| [backend-dsp](backend-dsp/README.md): looper repairs, pitch/rhythm analysis crate, live analysis thread | Integrated in the workstation working tree; tuner/pitch UI wired; real-voice validation pending |
| [frontend](frontend/README.md): Studio, Practice, project flows, tuner, pitch visualisation, settings, and interaction direction | Current vertical workflows implemented; native audio and hardware verification incomplete |
| [practice-lab](practice-lab/README.md): reference/attempt capture and scoring | Rust service, Tauri contract, React workflow and simulator tests implemented; real voice/timing pending |
| [persistence](persistence/README.md): projects, recovery, WAV/MIDI/DAW interchange | Rust service, Tauri commands, native dialogs and React workflow implemented; real save/recovery/DAW validation pending |
| [hardware-io](hardware-io/README.md): devices, routing, monitoring, MIDI, calibration | UI wired; built-in-device testing partial; H4n and physical keyboard pending |
| [ios](ios/README.md): iPhone/iPad feasibility and touch guidance | Compile-level investigation complete; Xcode/device proof blocked |
| [workstation-buildout](workstation-buildout/PROGRESS.md): cross-project integration and handoff | Active working tree; not integrated into `main` |

Individual project files preserve detailed historical checkpoints. Use
[ROADMAP.md](../ROADMAP.md) and the
[current integration audit](workstation-buildout/UNWIRED_FEATURES.md) for the
current cross-project status.
