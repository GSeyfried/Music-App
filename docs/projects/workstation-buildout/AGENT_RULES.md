# Rules for parallel agents (workstation build-out)

Several agents work in this **one working tree at the same time** (disk is too
tight for separate worktrees). Read `AGENTS.md` first, then follow these rules.

## Environment

- Cargo is not on the default PATH: start shell commands with
  `export PATH="$HOME/.cargo/bin:$PATH"`.
- Branch: `dev/workstation-buildout`. **Do not commit, stash, checkout, reset,
  or switch branches.** The lead commits. Never run `git add`/`git commit`.
- Disk has ~15 GB free. Do not create git worktrees or extra `target` dirs. Do
  not run `tauri build` / release builds. `cargo build -p voiceloop` (debug) is fine.
- Full Xcode is not installed (Command Line Tools only).
- Codex CLI: `$HOME/.nvm/versions/node/v22.18.0/bin/codex` (used by the lead for separate tasks).

## Ownership

- Each agent owns the files listed in its brief. Create new files freely
  inside your area.
- **Hub files** (`crates/voiceloop-engine/src/engine.rs`, `lib.rs`,
  `src-tauri/src/lib.rs`, `Cargo.toml`, `src/engine/bridge.ts`) are shared:
  make small, additive, localised edits with the Edit tool only (never rewrite
  or reformat the whole file), and re-read the region right before editing.
- If you need a change in a file another agent owns, make the smallest additive
  edit and list it in your final report.
- `docs/ROADMAP.md`, `docs/DECISIONS.md`, `docs/ARCHITECTURE.md`, `README.md`
  and `docs/projects/workstation-buildout/` belong to the lead. Put proposed
  decisions in your own `docs/projects/<area>/README.md`.

## Broken builds caused by others

Another agent may be mid-edit. If a compile or test failure is in a file you
did not touch, do **not** fix it: wait a minute and retry (up to ~10 times),
and scope checks to your crate (`cargo test -p <crate>`) where possible.

## Engineering constraints

- Realtime callbacks: no allocation, locks, blocking, logging, or I/O.
- Permissive licences only (MIT/Apache/BSD/Zlib/ISC). Check the licence and
  maintenance state of every new dependency and record it in your README.
- Never copy or translate GPL/AGPL code. No raw audio over the Tauri bridge.
- Rust formatting: `cargo fmt --all` (config in `rustfmt.toml`). No new clippy
  warnings in files you own; fix existing ones there.
- Do not weaken or delete tests to make them pass.
- Serde JSON is camelCase. Frames are `u64` sample frames.

## Deliverables (every agent)

1. Working, tested code. Say exactly which checks you ran and their results.
2. `docs/projects/<area>/README.md`: what was built, decisions, dependencies
   and licences, what is verified by tests vs. needs hardware, open `PENDING` items.
3. For anything the UI must call: `docs/projects/<area>/INTERFACE.md` with the
   exact Tauri command names, argument JSON, events, and TypeScript types.
4. Final report (under 400 words): files changed, hub-file edits, check
   results, defects found, recommended next tasks. Distinguish clearly between
   "compiles", "passes automated tests", and "verified on hardware" (you cannot
   claim the last).
