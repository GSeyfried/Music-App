# Voice progress: range training, profiles, milestones

Status: **proposed (2026-11-17), not started.** Builds on the Range Finder
(Games tab) and Practice Lab.

## Goal

The user wants to learn their real upper and lower range and push both ends
over time, in **head voice** and **chest voice**, with long-term milestones.

## Profiles (local only)

- Pick or create a user by name on launch (or from the menu bar). No password
  and no server: profiles exist only to keep each singer's history separate.
- All data stays on this machine (app data directory), per profile.
- If a server is ever added, real accounts would replace this. The user would
  prefer to avoid a server.

## What to record per profile

- **Range sessions:** each Range Finder run, with date, lowest and highest
  steady note, and how long each was held. Kept as history, not just the
  latest value (today `voice.rangeLowMidi` / `voice.rangeHighMidi` are a single
  app-wide value).
- **Register:** whether a note was sung in head, chest or mixed voice. The first
  version asks the singer to tag it (Head / Chest / Mixed) when a note is
  captured.
  - PENDING: automatic register detection. Possible cues are spectral tilt,
    harmonic balance (H1–H2) and a register break in a siren. These are
    research items, and accuracy on real voices is unknown.
- **Practice results:** scores per reference and scale, already stored in the
  practice library; link them to the profile.
- **Game scores:** Pitch Glider bests per difficulty and range.

## Views and milestones

- Range over time: low and high edges per register, as a chart. Show the
  comfortable range (steady, clear) separately from the reachable range
  (brief, less steady).
- Milestones, for example:
  - extend the top or bottom by one semitone
  - hold the edge note steadily for 2 s
  - in tune within ±25¢ at the edge
  - first clean head-voice note at a given pitch
- Exercises aimed at the edges: sirens, five-note scales stepping up or down by
  semitones, sustained notes at the edge. Each exercise starts from the singer's
  current range.

## Settings (to add to the registry when built)

- `voice.profile` (the current profile)
- Range-session thresholds: these already exist as `voice.rangeFinder.*`.
- Milestone targets (semitone step, hold seconds, cents tolerance)

## PENDING

- Storage format and location per profile (JSON in the app data directory?),
  and whether project files record the profile that made them.
- Whether practice references are shared between profiles or kept per profile.
- Register detection approach (manual tags first).
- Vocal health guardrails: rest reminders, and no pushing past strain. Wording
  and limits to be decided with the user.
