# VoiceLoop Pocket: SwiftUI app

Decision: [D-021](../../DECISIONS.md). Product: [PRODUCT_APPS.md](PRODUCT_APPS.md).
Feature: [song-builder](../song-builder/README.md).

## Layout

```text
apps/pocket-ios/
  project.yml                 XcodeGen spec (the .xcodeproj is generated, git-ignored)
  Sources/App/                @main app
  Sources/Engine/             EngineBridge.swift (C interface → Swift, event decoding), bridging header
  Sources/SongBuilder/        SongModel, SongStore (state + engine glue), PocketSettings (registry), VoiceDrums
  Sources/Views/              SwiftUI screens
crates/voiceloop-ffi/         staticlib: vl_engine_create / vl_engine_call / vl_string_free / vl_engine_destroy
  include/voiceloop.h
scripts/build-pocket-engine.sh  cargo build for the platform Xcode is building (Xcode pre-build phase)
```

The engine is the same `voiceloop-engine` the desktop app uses. iOS audio
session setup (AVAudioSession, permission, interruptions, route changes) is
in Rust (`voiceloop-engine/src/platform/ios.rs`), so the app itself needs no
Swift audio code.

## Build and run

One-time tools: full Xcode with an iOS simulator runtime; Rust targets
`aarch64-apple-ios` and `aarch64-apple-ios-sim`; XcodeGen. XcodeGen 2.48.0 is
kept unzipped in the git-ignored `.tools/xcodegen/` (`brew install xcodegen`
also works).

```sh
npm run pocket:project      # generate apps/pocket-ios/VoiceLoopPocket.xcodeproj
open apps/pocket-ios/VoiceLoopPocket.xcodeproj   # ⌘R in Xcode builds Rust + Swift
```

Command line (simulator):

```sh
cd apps/pocket-ios && xcodebuild -project VoiceLoopPocket.xcodeproj -scheme VoiceLoopPocket \
  -sdk iphonesimulator -destination 'platform=iOS Simulator,name=iPhone 18 Pro' -derivedDataPath build/dd build
```

Debug launch arguments (UserDefaults argument domain):

- `-songBuilder.voiceDrums.listen NO`: never open the microphone. On the
  simulator the mic is the **Mac's** microphone.
- `-pocket.skipEngine YES`: launch without the audio engine.
- `-pocket.snapshotDir <dir>`: render the main screens to PNGs offscreen and
  exit. Use this for visual checks when simulator screenshots are blank (see
  below).

Physical iPhone (one-time):

1. Xcode › Settings › Accounts › **+** › Apple ID; sign in. A free personal
   team works for your own phone (apps then expire after 7 days and need a
   rebuild).
2. Put the team ID in `apps/pocket-ios/Local.xcconfig` (git-ignored, included
   by `Base.xcconfig`): `DEVELOPMENT_TEAM = ABCDE12345`. This survives
   `npm run pocket:project`; setting the team only in Xcode's Signing tab is
   lost on the next regenerate.
3. Plug in the phone, trust the Mac, and on the phone enable Settings ›
   Privacy & Security › Developer Mode. Pick the phone in Xcode and press ⌘R.
   On first launch, trust the developer under Settings › General › VPN &
   Device Management.

## Known environment issue (2026-11-19)

On the build Mac, `xcrun simctl io … screenshot` returns a blank white
screen for every app, Apple's Settings included, and Simulator.app is not
present in this Xcode 27 install. Rendering through a GUI simulator or on a
device is untested. The snapshot mode above is the workaround for visual
checks.

## Engine contract used by Pocket

Commands (`vl_engine_call`, JSON args by name): `engine_status`,
`start_input {deviceId}`, `stop_input`, `set_input_needs {needs}` (reason
`songBuilder`), `set_analysis_config {config}`, `request_mic_permission`,
`use_settings_file {path}`, `song_set {song}`, `song_play {countInBars}`,
`song_stop`, `song_trigger {trigger}`, `song_locate {inputFrame?, extraMs}`.

Events: `status`, `hit` (voice-drum timbre), `songPosition`, `pitch`,
`onset`, `level`, `warning`, `error` (others are ignored).

## PENDING

- On-device run (mic permission, audio session, latency, Airplane Mode).
- Background audio and lock-screen behaviour; App Intents for hands-free
  commands.
- Signing/team setup, app icon, final app name.
- A shared generated mirror of the domain types (Swift and TypeScript are
  hand-written today).
- From [RUST_VS_SWIFT_2026-11-19.md](RUST_VS_SWIFT_2026-11-19.md): an
  on-device latency/dropout harness (128 vs 256 frames, speaker, wired, HFP,
  A2DP); a single duplex RemoteIO host calling a raw (non-JSON) Rust render
  ABI, compared against today's two CPAL streams; echo-cancelled input and
  VoiceProcessingIO as session policies; Bluetooth HFP input and input
  selection; Swift 6 strict concurrency for the shell. Done 2026-11-19: Rust
  panics are caught at the C boundary.
