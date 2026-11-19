# Music App agent instructions

Personal project workspace for the Music App. Remote:
`https://github.com/GSeyfried/Music-App` (account `GSeyfried`,
`griffin.seyfried@gmail.com`).

## First reads

1. `AGENTS.md` (this file)
2. `README.md`, then `docs/ARCHITECTURE.md` and `docs/DECISIONS.md`
3. `docs/projects/README.md`, then the folder for whichever project is in scope

## Code boundaries

- `crates/voiceloop-engine` must not depend on Tauri. The Tauri crate (`src-tauri`) stays a thin adapter.
- Realtime callback code (`capture.rs`, `meter.rs`) must not allocate, lock, block, log, or do I/O.
  Use preallocated `rtrb` rings.
- Never send raw audio over the Tauri bridge. Send summaries and events only.
- `src/engine/bridge.ts` is the only frontend file that imports `@tauri-apps/api`.
- Domain types are mirrored by hand: change `model.rs`/`transport.rs` and `src/models/domain.ts` together.
- `reference-packages/*/` is git-ignored reference source. Never import or compile it, and never copy
  or translate code from GPL/AGPL references (Tracktion, JUCE, mloop, Essentia).

## Settings principle

Any new variable that affects the interface or the sound (thresholds,
defaults, timings, colors, modes) is defined once in the global settings
registry, with a row in `docs/projects/settings/SETTINGS_INDEX.md`. Do this even
if it is never exposed as a user setting. Skip it only for values that truly
don't matter. Whether and where it becomes a visible setting is decided
separately. See `docs/projects/settings/README.md`.

When there's a question about how something should work (two reasonable
behaviours), build both behind a setting with a sensible default instead of
picking one or asking. The exception is when doing both would roughly triple
the work; then build one, record the other as `PENDING`, and say so.

## Checks

On the Mac: `npm run engine:test`, `npm run typecheck`, `cargo build -p voiceloop`, `npm run refs:verify`.
On the Windows PC: only `npm run typecheck` and `npm run refs:verify`.

## Workspace boundary

- All work stays inside this directory (`Music APP/`) and this repo.
- Do not create, change, move, or delete files outside this directory unless
  the user specifically authorizes it for that task. Reading outside files for
  reference is fine.
- Approval for one outside change does not carry over to the next.

## Build machines

- **macOS (personal Mac) is the build and runtime target.** See `docs/MAC_SETUP.md`.
- **Windows work PC: no native compilation.** Never run `cargo build/check/test/run`,
  `tauri dev`, or `tauri build` there, and don't cross-compile. Lightweight checks are fine
  (`npm run typecheck`, `node --check`, JSON validation, `npm run refs:verify`).
  Ask before anything that may use substantial CPU, RAM, or disk.

## Session setup

- Run `npm start` at the start of a session. It switches git, GitHub CLI,
  Firebase CLI, and gcloud to the personal account and points `origin` at the
  Music-App repo. `npm run start:check` verifies without changing anything.
- Script: `scripts/start-project-session.mjs`. Set the `projectId` fields there
  once a Firebase/GCP project exists.

## Safety

- Do not commit, push, deploy, or create/modify cloud resources unless the user
  asks.
- Never commit credentials, tokens, or `.env` files.
- Confirm the active GitHub account is `GSeyfried` before any push.

## Integration checkpoints

The user has given standing authorization for **local integration** after a
feature build-out passes its required checks and its relevant functionality is
confirmed in a native app launch. At that checkpoint, the coordinating agent
should:

1. Make sure no agent is still editing and preserve all completed work in
   local commits on the appropriate feature branches.
2. Merge every completed feature branch into local `main`, resolve integration
   issues, then run the required checks and launch the integrated app again.
3. Remove merged local worktrees and delete merged local feature branches only
   after verifying they have no commits unique from `main` (for example with
   `git branch --merged main`). Never delete an unmerged branch or active
   worktree.

This standing authorization covers local commits, local merges, and cleanup of
merged local branches/worktrees at a verified integration checkpoint. It does
**not** authorize pushing, deleting remote branches, deploying, or changing
cloud resources; those still require a direct request. If hardware-dependent
functionality was not exercised, record that limitation rather than treating
the launch as hardware verification.

## Documentation

- General docs live in `docs/`.
- Each project or feature area we build out gets its own folder under
  `docs/projects/<project-name>/` (see `docs/projects/README.md`).
- Record open questions as `PENDING` rather than guessing.
