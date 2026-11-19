# Reference audit

**Status:** process agreed 2026-11-19. Audits of already-built tools are planned (table below); none has run yet.

## Why

Most tools we build (a looper, a pitch trainer, an EQ, noise reduction, a DAW timeline) already exist in other apps. Those apps' users have already found the common failures. A short look at a similar tool, and at what people complain about in it, catches the gaps we'd otherwise rediscover one bug report at a time.

This is deliberately **light**. It's a checklist pass, not research. Budget it at about 30–45 minutes of agent time per tool. The output is a short section of notes, not a report.

Related:
- [REFERENCE_PACKAGE_ASSESSMENT.md](../../REFERENCE_PACKAGE_ASSESSMENT.md): licenses and adopt/reference decisions for packages.
- [produce/REFERENCES.md](../produce/REFERENCES.md): an example of a fuller survey.

## The lightweight check (every new tool, and every major extension)

Run it twice: once **before** building (to shape the plan), and again briefly **after** (to check the result).

1. **Pick 2–4 comparable tools.** Aim for one well-known commercial product, one open-source project, and, if relevant, a hardware unit or a mobile app. Prefer what our users would actually have tried.
2. **Feature diff.** List what they do that we don't and sort each item:
   - **must have** (users will expect it): build it or record it as PENDING with a reason;
   - **nice to have:** add it to the project's PENDING list;
   - **deliberately different:** one line on why.
3. **Known-problems sweep.** Skim where users and developers report trouble:
   - the issue trackers of open-source equivalents;
   - product forums and FAQs, and support "known issues" pages;
   - KVR, Gearspace, Reddit (r/WeAreTheMusicMakers, r/edmproduction, r/singing, r/ableton, r/Logic_Studio), the Loopy Pro forum;
   - App Store reviews for mobile tools.

   Collect the **recurring** problems (seen several times, not one-offs).
4. **Check each problem against ours:** handled (say how, with a test if there is one), not handled (fix it or record PENDING), or not applicable.
5. **Write it down** as a `## Reference check` section in that tool's project doc, using the template below. Add any new settings to the registry, as usual.

**Rules:**
- Never copy or translate GPL/AGPL code (see AGENTS.md). Behaviour and documentation are fine to study; record which kind of reference each one was.
- Where two reasonable behaviours turn up, follow the AGENTS.md rule: build both behind a setting unless that roughly triples the work.
- Don't redo a check that's already recorded unless the tool changed substantially (see "No redundant audits").

### Template

```md
## Reference check (YYYY-MM-DD)

Compared with: <Tool A> (commercial), <Tool B> (open source, MIT, code readable), <Tool C> (GPL, behaviour only)

| Gap or known problem | Source | Us | Action |
| --- | --- | --- | --- |
| Loop seam clicks on long overdubs | Loopy forum, several threads | Handled: crossfade at the seam (test `seam_is_click_free`) | — |
| No "undo last overdub only" | Tool A feature | Missing | PENDING |
| Effects tails cut when the transport stops | KVR, DAW complaints | Missing | Fix now |

Deliberately different: <item> because <reason>.
```

## Planned audit of tools already built

Run these one or two per session, starting with the most user-facing, as small isolated agent tasks. The "seed problems" are starting points from general knowledge to confirm or reject. They are not findings.

| # | Tool (project doc) | Compare with | Seed problems to look for |
| --- | --- | --- | --- |
| 1 | **Looper core and per-track loops** (looper-01) | Loopy Pro, Boss RC-505/RC-600, Ableton Looper, mloop/SooperLooper (behaviour only) | Latency compensation drift; loop seam clicks; overdub decay/feedback options; first-loop length quantize; undo/redo depth; count-in edge cases; speaker bleed into overdubs; MIDI foot control; sync of odd ratios (×1.5) |
| 2 | **Effects** (effects) | Stock effects in GarageBand/Logic, Ableton and REAPER (ReaEQ, ReaComp), Valhalla Supermassive, Airwindows (MIT, code readable) | Zipper noise; tails cut on bypass/edit/stop; denormal CPU spikes; aliasing in distortion; compressor pumping and lack of RMS/lookahead; latency of lookahead effects; preset portability; CPU per track |
| 3 | **Noise reduction** (effects) | iZotope RX Voice De-noise, Audacity (behaviour only), Krisp, RNNoise (BSD) | "Musical noise" warbling; pumping at phrase ends; profile mismatch when the room changes; eating breaths and consonants; artifacts on sibilants; latency |
| 4 | **Practice: pitch scoring, legato, notation** (practice-lab) | Sing Sharp, Vanido, SingTrue, Yousician, Melodyne's note display, Erol Singer's Studio | Octave errors; vibrato judged as off-pitch; scoops and slides; breathy onsets; latency between the reference and the voice; unfair rhythm scoring; low-voice (bass) detection; mic auto-gain |
| 5 | **PRODUCE timeline** (produce) | GarageBand, Ableton arrangement, REAPER, BandLab, openDAW (behaviour only), waveform-playlist (MIT) | Fades and de-clicks at edits; clip gain staging; snap/grid frustrations; undo of multi-step edits; sample-rate mismatch on import; project portability; playhead/scroll follow; nondestructive edits |
| 6 | **Recording, devices, mic-only-when-needed** (hardware-io) | GarageBand, Ableton, Loopy Pro, AudioKit (MIT) | Bluetooth headset sample-rate switch and latency; device hot-plug; macOS mic permission flows; input monitoring feedback; iOS audio-session interruptions |
| 7 | **Piano preview and sound library** (sound-library) | GarageBand instruments, Pianoteq/Salamander users | Note-on latency; voice stealing clicks; level mismatch between samples; loop points; polyphony limits |
| 8 | **Games** (games) | Yousician, Sing! by Smule, pitch-game apps | Latency fairness; difficulty curves; mic noise triggering notes; motion sickness or readability of moving notes |
| 9 | **Settings and help** (settings) | macOS System Settings patterns, Ableton preferences, Logic's "Show Advanced Tools" | Too many visible options; settings that can't be reset; unclear units; inconsistent defaults across tracks |
| 10 | **Project files, recovery, export** (persistence) | Ableton .als, REAPER .rpp, DAWproject (MIT) | Lost work on crash; huge project folders; broken relative paths; version migration; stem export naming and alignment |

Track progress here:

| # | Ran on | By | Findings section | Open actions |
| --- | --- | --- | --- | --- |
| — | — | — | — | — |
