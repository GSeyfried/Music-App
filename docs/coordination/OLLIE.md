# Ollie: TestFlight handoff protocol

Ollie is the computer agent on Griffin's brother's Mac. It builds VoiceLoop
Pocket (`apps/pocket-ios/`) under the brother's Apple Developer team and
uploads it to TestFlight for Griffin. Griffin's agent and Ollie communicate
**only through this file on the `pocket/testflight` branch**.

Read `AGENTS.md`, then this file, at the start of every session. On anything
about builds and uploads, this file wins.

## Rules

- Work only on `pocket/testflight`. Never push to `main`, force-push, or
  delete branches.
- Commit only: log lines below, build-number bumps, and minimal build fixes
  (each its own commit, explained in a log line).
- Never commit `apps/pocket-ios/Local.xcconfig`, certificates, provisioning
  profiles, API keys or App Store Connect credentials.
- Don't change app behaviour (`crates/`, `src/`, `apps/pocket-ios/Sources/`);
  report problems in the log instead.
- Don't run the desktop app or leave anything holding the microphone.
- Upload only a commit Griffin's side has logged as `ready for TestFlight`.
- Commit messages from Ollie start with `ollie:`.

## One-time setup

1. Xcode (open once, accept the licence); Rust via rustup (the repo's
   `rust-toolchain.toml` picks the version);
   `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`;
   `brew install xcodegen`.
2. `cd apps/pocket-ios && xcodegen generate`.
3. Create `apps/pocket-ios/Local.xcconfig`:

   ```
   DEVELOPMENT_TEAM = <brother's team ID>
   POCKET_BUNDLE_ID = <bundle ID, if not dev.gseyfried.voiceloop.pocket>
   ```

4. Smoke build for a simulator (command in `docs/projects/ios/SWIFT_APP.md`).

## Each upload

1. `git pull` on `pocket/testflight`; find the newest `ready for TestFlight`
   line below and check out that commit (or the branch tip if it matches).
2. Raise `CURRENT_PROJECT_VERSION` above the last uploaded build.
3. `xcodegen generate`, then in Xcode pick **Any iOS Device (arm64)** ›
   Product › Archive › Distribute App › App Store Connect › Upload.
4. After processing, add a log line (`uploaded build N`) and push.
5. On any failure: add a `blocked:` log line with the exact error and push.

## Open questions for Ollie (answer in your first log line)

- Ollie's GitHub account name (for collaborator access).
- Developer Program type (individual/organization) and team ID.
- Bundle ID used.
- Xcode and macOS versions on the brother's Mac.

## Log (newest first)

| Date | Who | Commit | Note |
| --- | --- | --- | --- |
| 2026-11-19 | Griffin's agent | (this commit) | Protocol created. Not yet ready for TestFlight: app icon, encryption key, configurable bundle ID and a device Release build check still to do. |
