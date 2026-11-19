# Build and install on iPad / iPhone

From the current Mac to VoiceLoop running in the iOS Simulator and on your own
iPad and iPhone. As of 2026-11-19, Xcode 27.2 is selected, iOS device and
Simulator SDK/runtime 27.2 are installed, both ARM Rust targets are installed,
and about 62 GB is free. CocoaPods/XcodeGen, the generated Apple project,
signing and device execution remain unfinished.

> **Simulator success is not evidence that realtime audio works.** The
> Simulator runs x86/arm code on the Mac and routes audio through the Mac's
> CoreAudio hardware and buffer sizes. It has no USB interfaces, no Bluetooth
> MIDI, no receiver/speaker routing, no interruptions from real calls, and no
> iPad timing or thermal behaviour. Latency, xrun, routing, and permission
> behaviour must be verified on a physical device.

## 0. Check what's missing

```sh
bash scripts/ios-setup-check.sh            # read-only; installs nothing
bash scripts/ios-setup-check.sh --compile  # + cargo check of the Rust crates for iOS
```

On 2026-11-19 it reported that Xcode, SDKs, runtime, disk and Rust targets are
ready; CocoaPods is missing, XcodeGen is not on PATH, Node comes from nvm,
`src-tauri/gen/apple` is absent, and no Apple development team is configured.

## 1. Free disk space (user)

Xcode and the iOS runtime are installed and the Mac currently has about 62 GB
free, above the recommended 40 GB working margin. Keep monitoring free space
as generated Xcode builds and device symbols accumulate.

## 2. Install Xcode (user)

1. Install **Xcode** from the Mac App Store (or developer.apple.com/download).
   Use a version that supports the iOS/iPadOS version on your devices.
   An older Xcode cannot deploy to a newer iOS.
2. Launch Xcode once and accept the licence. In **Settings › Components**,
   install the **iOS** platform (includes the Simulator runtime).
3. Select it as the active developer directory:
   ```sh
   sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
   sudo xcodebuild -runFirstLaunch
   xcodebuild -version
   xcrun --sdk iphoneos --show-sdk-path          # must print a path
   xcrun --sdk iphonesimulator --show-sdk-path   # must print a path
   ```
   Desktop builds keep working with full Xcode selected.
4. **Settings › Accounts › +** → add your Apple ID. This creates a free
   "Personal Team". A paid Developer Program team also appears here.

## 3. Remaining prerequisites

```sh
rustup target add aarch64-apple-ios aarch64-apple-ios-sim   # already done on this Mac
rustup target add x86_64-apple-ios                          # only for Intel-Mac simulators
brew install cocoapods                                      # Tauri iOS prerequisite
```

Node: this Mac uses **nvm**. Xcode's "Build Rust Code" phase may not see
nvm's `node` (Tauri troubleshooting note). If the Xcode build fails with
`node: command not found` or `tauri: command not found`, either
`brew install node@22` and put it first on `PATH`, or run builds through
`npm run tauri ios dev` from a terminal (the CLI drives Xcode).

Re-run `bash scripts/ios-setup-check.sh --compile`. Everything should be
`ok`, and `cargo check -p voiceloop --target aarch64-apple-ios-sim` should now pass.

## 4. Configure the project (one time)

### 4.1 Info.plist keys

`src-tauri/Info.ios.plist` already exists. It holds `NSMicrophoneUsageDescription`
(**mandatory**, or iOS kills the app on mic access), `UIBackgroundModes: audio`,
`UIFileSharingEnabled`, and `LSSupportsOpeningDocumentsInPlace`. After step 4.3,
confirm these keys appear in `src-tauri/gen/apple/voiceloop_iOS/Info.plist`
(name may differ) or in Xcode › target › Info. If the CLI does not merge
`Info.ios.plist`, set `"bundle": { "iOS": { "infoPlist": "Info.ios.plist" } }`.

### 4.2 `tauri.conf.json` (proposed; lead/user to apply)

```jsonc
"bundle": {
  // ...existing keys...
  "iOS": {
    "minimumSystemVersion": "17.0",       // AVAudioApplication permission API (default is 15.0)
    "developmentTeam": "ABCDE12345"        // your Team ID; or export APPLE_DEVELOPMENT_TEAM instead
  }
}
```

Find the Team ID in Xcode › Settings › Accounts › (team) or at
developer.apple.com › Membership. With a free Personal Team you can also leave
it unset and choose the team in Xcode › Signing & Capabilities (step 7.2).

Bundle identifier: `dev.gseyfried.voiceloop`. It must be globally unique
across Apple. If Xcode reports it is unavailable for a Personal Team, use a
suffix such as `dev.gseyfried.voiceloop.dev` in a `src-tauri/tauri.ios.conf.json`
(`{ "identifier": "dev.gseyfried.voiceloop.dev" }`, merged automatically on iOS).

### 4.3 Generate the Xcode project

```sh
npm run tauri ios init
```

Expected: creates `src-tauri/gen/apple/` (XcodeGen `project.yml`, the
`.xcodeproj`, `Sources/`, `Assets.xcassets`, `LaunchScreen`, entitlements, and its
own `.gitignore`). It may install `xcodegen`, and cocoapods if missing, through
Homebrew. Tauri's convention is to **commit `src-tauri/gen/apple`** (minus its
ignored build outputs). Ask the lead before committing (the repo ignores only
`src-tauri/gen/schemas/`).

## 5. Simulator test procedure

```sh
xcrun simctl list devices available | grep -E "iPad|iPhone"   # pick names
npm run tauri ios dev "iPad Air 13-inch (M3)"                 # example name; use one from the list
npm run tauri ios dev "iPhone 17"                             # example
```

The first run compiles the whole Rust dependency tree for `aarch64-apple-ios-sim`
(several minutes; about 1-2 GB in `target/`).

| # | Step | Expected |
| --- | --- | --- |
| S1 | App launches | VoiceLoop window fills the screen. Studio workspace renders. No white screen (if white: Safari › Develop › Simulator › localhost for console errors). |
| S2 | Rotate (⌘←/→) | Layout adapts. No content under the status bar or home indicator (after TOUCH_UI work). |
| S3 | Input devices load | One input (CPAL "Default Device"/"default", or the session ports once the platform layer exists). No error banner. |
| S4 | Start input | **Without the session layer: expect failure or silence** (no `playAndRecord` category). With it: microphone prompt showing our usage string. Allow it, and the meter moves with the **Mac's** mic. |
| S5 | Looper record/play | Loops play through the Mac speakers. Functional check only. Timing numbers here are meaningless. |
| S6 | Background (⇧⌘H) and return | With background audio: loop keeps playing. UI resumes without an event backlog flood. |
| S7 | Hardware keyboard | With I/O › Keyboard › Connect Hardware Keyboard, Space/R/T shortcuts work. |
| S8 | Files app | Simulator Files app › On My iPad/iPhone › VoiceLoop shows the Documents folder (after storage work). |

Simulator-only artefacts to ignore: odd sample rates (whatever the Mac's
device runs), large buffers, and no route-change events for headphones.

## 6. Physical device: one-time setup (user)

1. Connect the iPad or iPhone by USB-C/Lightning and unlock it. Tap **Trust This Computer**.
2. In Xcode › Window › Devices and Simulators, wait for "Preparing device for
   development" to finish (it copies debug symbols, which takes minutes).
3. **Enable Developer Mode** (iOS/iPadOS 16+): Settings › Privacy & Security ›
   Developer Mode › On → restart → confirm. The switch appears only after the
   device has been connected to Xcode (or an app install was attempted).
4. Optional, for wireless runs later: in Devices and Simulators, check "Connect via network".
5. For webview debugging: on the device, Settings › Apps › Safari › Advanced ›
   **Web Inspector** on. On the Mac, Safari › Settings › Advanced › "Show features for web developers".

## 7. Physical device: build and run

### 7.1 Dev mode (hot reload over LAN)

```sh
npm run tauri ios dev                     # picks the connected device first
# or, if Xcode should do the install:
npm run tauri ios dev --open --host       # keep the CLI running while Xcode is open
```

The device loads the Vite dev server from the Mac's LAN address
(`TAURI_DEV_HOST`; `vite.config.ts` already handles this). On first launch iOS
asks for **Local Network** access. Allow it, then relaunch. Mac and device must
be on the same network, with no client isolation (some guest Wi-Fi blocks it).
`--force-ip-prompt` lets you choose the address.

### 7.2 Signing

Automatic signing is the default. If the build stops with a signing error,
run `npm run tauri ios dev --open`, then in Xcode select the app target ›
**Signing & Capabilities** › Team = your team, "Automatically manage signing" on.
Xcode creates the App ID and provisioning profile.

First launch with a **free** team: iOS refuses to open the app ("Untrusted
Developer"). Settings › General › **VPN & Device Management** › your Apple ID ›
**Trust**.

### 7.3 Standalone build (no dev server)

```sh
npm run tauri ios build -- --debug                    # debug IPA; output under src-tauri/gen/apple/build/
npm run tauri ios build -- --export-method debugging  # if the default export method needs a paid account (verify)
```

Or in Xcode: select the device, then Product › Run (Debug), or set the scheme
to Release for realistic DSP performance (`opt-level` for the engine is already
2 in dev, so dev builds are close).

### 7.4 Free vs paid account

| | Free Personal Team | Paid Developer Program |
| --- | --- | --- |
| Profile valid | **7 days**. After that the app will not launch, so re-run step 7.1/7.3 to reinstall | About 1 year |
| Apps per device | 3 | Unlimited for development |
| App IDs | 10 per 7 days | No practical limit |
| iCloud / App Groups / push | No | Yes |
| TestFlight, other people's devices | No | Yes |

The same account and steps work for the iPad and the iPhone. Each device needs
Developer Mode and (free team) Trust. Both count toward their own 3-app limit.

## 8. Physical device test procedure (iPad and iPhone)

Record results in `docs/projects/ios/DEVICE_RESULTS.md` (create it on the first run): device model, iOS
version, Xcode version, commit, and for every row pass/fail plus numbers.

| # | Step | Expected (with stage 1/2 work in place) |
| --- | --- | --- |
| D1 | Launch | UI renders and stays within safe areas. Portrait and landscape both work. On iPad, also Split View ⅓ and ½. |
| D2 | First Start input | Mic prompt shows our purpose string. On **Deny**: no crash, clear "Microphone access is off" message, meter not moving. Settings › Privacy & Security › Microphone › VoiceLoop on → relaunch → works. |
| D3 | Session readback | Diagnostics shows granted sample rate (48000 expected on built-in), IO buffer frames (128 requested), input and output latency, current input and output ports. |
| D4 | Meter, built-in mic | Moves with voice. With `measurement` mode, no AGC pumping on loud or quiet passages. |
| D5 | Free loop and metronome loop | Record, overdub, undo, mute, clear. Click audible. On iPhone, output from the **speaker** (not the earpiece). |
| D6 | Latency calibration | Loopback (speaker → mic, or a cable on an interface) measured round trip within about 3 ms of reported `in + out + 2×buffer`, or record the difference. Overdub lands in time after calibration. |
| D7 | Buffer sweep | 64/128/256/512 requested: record granted values, xruns over 5 min each, CPU headroom. |
| D8 | Headphones in and out mid-loop (wired/USB-C, then AirPods) | Streams restart, **loops intact**, route shown, playback paused on unplug. AirPods: high-latency warning. Recording while on Bluetooth output is discouraged. |
| D9 | Phone call / FaceTime / Siri during a loop (iPhone; iPad via FaceTime) | Take closes cleanly and the transport pauses. After the call: audio session active again, loops intact, user resumes. Banner-style call ignored → no interruption. |
| D10 | Background | Home gesture: loops keep playing (background audio). Lock screen: keeps playing. Return: UI catches up without freezing. |
| D11 | Low Power Mode | UI animations at 30 fps are acceptable. **Audio unaffected** (no xruns). |
| D12 | Media services reset | Settings › Developer › Reset Media Services while running: engine recovers (reconfigure session, restart) or reports clearly. No crash. |
| D13 | USB interface (Zoom H4n Pro) | H4n on AC power, set to **audio interface mode first**, then connect (USB-C, or Lightning + USB 3 Camera Adapter) and launch: input port "H4n…" listed and selectable, channels 2. Record its sample rate (44.1 kHz is likely). Hot-plug while running → D8 behaviour. If it does not appear, note it and test another class-compliant interface. |
| D14 | MIDI (after stage 4) | USB MIDI keyboard appears and notes trigger. BLE MIDI pairs through the in-app sheet. Timestamps map to frames (check jitter). |
| D15 | Files | Projects appear in Files › On My iPad › VoiceLoop. Export via share sheet. Import of an exported project round-trips. |
| D16 | Long run | 30 min loop: 0 frames boundary error, xrun count, battery % and thermal state noted. |

## 9. Troubleshooting

| Symptom | Likely cause / fix |
| --- | --- |
| `xcrun: error: SDK "iphoneos" cannot be located` | Xcode not selected: `sudo xcode-select -s /Applications/Xcode.app/Contents/Developer`. |
| `Failed to get SDK path with xcrun --sdk iphonesimulator` (swift-rs) | Same as above, or the iOS platform component is not installed. |
| Build phase "Build Rust Code" can't find node/tauri | nvm PATH not visible to Xcode; see §3. |
| White screen on device in dev | Local Network permission denied or a different network. Settings › Privacy & Security › Local Network › VoiceLoop. |
| App crashes immediately on Start input | `NSMicrophoneUsageDescription` missing from the built Info.plist (§4.1). |
| Meter flat at -100 dBFS on device | Permission denied (silence, not an error), or the session category is not `playAndRecord`. |
| "Untrusted Developer" | §7.2 Trust. |
| App stops launching after a week | Free-team profile expired; reinstall. |
| "Unable to install … maximum number of apps" | Free team limit (3 per device). Delete another sideloaded app. |
