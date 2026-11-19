# iOS / iPadOS platform: findings and plan

Owner: iOS platform agent (workstation build-out). Date: 2026-11-17.
Related: [COMPATIBILITY_MATRIX.md](COMPATIBILITY_MATRIX.md),
[BUILD_AND_INSTALL.md](BUILD_AND_INSTALL.md), [TOUCH_UI.md](TOUCH_UI.md),
[PRODUCT_APPS.md](PRODUCT_APPS.md),
`scripts/ios-setup-check.sh`, `src-tauri/Info.ios.plist`.

**Nothing in this document has run on an iPhone, iPad, or simulator.** The
toolchain snapshot changed on 2026-11-19: Xcode 27.2 is selected, iPhoneOS and
Simulator SDK/runtime 27.2 are installed, both Rust ARM iOS targets are
installed, and about 62 GB is free. CocoaPods and XcodeGen remain missing,
`src-tauri/gen/apple` has not been generated, and a development team is not
configured. Labels used throughout:

- **compiles**: `cargo check --target aarch64-apple-ios[-sim]` succeeded here.
- **documented**: a primary source (Apple, Tauri, or CPAL source/changelog) says so.
- **unverified**: plausible but not demonstrated; needs a device.

**Current-worktree delta:** the recommended CPAL 0.18.2 upgrade and
park/restore support for preserving loops across stream restarts are now
implemented and pass host-side checks. Sections that compare CPAL 0.16 with
0.18.2 preserve the investigation that led to the upgrade; they are not a
claim that the current dependency is still 0.16. The AVAudioSession platform
layer, Tauri iOS build, interruption/route behavior and physical-device audio
remain unimplemented or unverified.

## 1. Summary

| Question | Answer |
| --- | --- |
| Can the engine target iOS? | **Yes, at the earlier type-check level.** `voiceloop-analysis` and `voiceloop-engine` passed `cargo check` for `aarch64-apple-ios` and `aarch64-apple-ios-sim`. The current CPAL 0.18.2 working tree needs the same check rerun once the iOS SDK/tooling environment is available. Linking needs the iOS SDK. |
| Can the Tauri shell target iOS? | The former Xcode/SDK blocker is resolved. Full shell generation/build is still pending CocoaPods/XcodeGen and signing configuration. |
| Is the CPAL base current? | **Partly.** CPAL 0.18.2 is now adopted. It improves interruption, route/device-loss, fixed-buffer and latency support, but it still does not replace VoiceLoop's required AVAudioSession policy layer. |
| What remains? | Add the **thin AVAudioSession layer in Rust (objc2-avf-audio)** for category/mode, preferred rate and buffer, activation, input selection, permission and app lifecycle. A prototype compiles for iOS (§5), but it is not integrated. |
| Biggest architectural gap | Physical-device proof: interruption/route recovery, background/screen-lock behavior, Bluetooth/automotive routes, latency and offline save/reopen. Looper park/restore now addresses the earlier state-loss design gap but still needs app/device verification. |
| Can the user test on their iPad/iPhone today? | Not yet. Xcode/SDK/disk are ready; CocoaPods/XcodeGen, project generation, signing, AVAudioSession integration and a device build remain. See [BUILD_AND_INSTALL.md](BUILD_AND_INSTALL.md). |

## 2. Compile-level proof (precise results)

Environment: macOS 27.0.1 arm64, rustc 1.100.0, `xcode-select -p` =
`/Library/Developer/CommandLineTools`, `xcodebuild` unavailable. Targets added:
`rustup target add aarch64-apple-ios aarch64-apple-ios-sim`. iOS artefacts
used about 330 MB under `target/`.

| Command | Result |
| --- | --- |
| `cargo check -p voiceloop-analysis --target aarch64-apple-ios` | **pass** (11.4 s) |
| `cargo check -p voiceloop-analysis --target aarch64-apple-ios-sim` | **pass** |
| `cargo check -p voiceloop-engine --target aarch64-apple-ios` | **pass**, 0 warnings, 2026-11-16 (cpal 0.16.0 → coreaudio-rs 0.13 → objc2-audio-toolbox/core-audio 0.3.2) |
| `cargo check -p voiceloop-engine --target aarch64-apple-ios-sim` | **pass**, 2026-11-16 |
| `cargo check -p voiceloop-engine -p voiceloop-analysis --tests --target aarch64-apple-ios` | **pass** (tests compile; they cannot run without a simulator) |
| `cargo build -p voiceloop-engine --lib --target aarch64-apple-ios` | **pass** (rlib only, no link) |
| `cargo build -p voiceloop-engine --example meter --target aarch64-apple-ios` | **fail at link**: `ld: building for 'iOS', but linking in dylib (.../MacOSX14.4.sdk/usr/lib/libiconv.2.tbd) built for 'macOS'`, and rustc says "when compiling for iOS ... you need a full installation of Xcode". Expected. |
| `cargo check -p voiceloop --target aarch64-apple-ios` (Tauri) | **fail**: build script of `objc2-exception-helper v0.1.1`: `cc-rs: "xcrun" "--show-sdk-path" "--sdk" "iphoneos"` → `SDK "iphoneos" cannot be located` |
| `cargo check -p voiceloop --target aarch64-apple-ios-sim --keep-going` | **fail**: same `objc2-exception-helper` error, plus `tauri v2.12.6` build script panics in `swift-rs-1.0.9/src-rs/build.rs:280`: `Failed to get SDK path with 'xcrun --sdk iphonesimulator --show-sdk-path'` |
| 2026-11-17 re-run, first attempt | Failed on the host and iOS alike (`unresolved import crate::clock` / `crate::routing`) while another agent was mid-edit. Not iOS-specific. |
| 2026-11-17 re-run, after their edit | `cargo check -p voiceloop-engine -p voiceloop-analysis --tests` **pass** on both iOS targets, 0 warnings. `voiceloop-project` is not yet a workspace member, so it is not checked. Re-run anytime with `bash scripts/ios-setup-check.sh --compile`. |

Correction to the brief's hypothesis: CPAL 0.16's CoreAudio backend **no
longer uses `coreaudio-sys`/bindgen**. It uses hand-written `objc2-*` bindings,
so the engine type-checks for iOS without any SDK. The SDK is needed only to
link, and by Tauri's build scripts.

### Scratch probe (outside the repo, in the session scratchpad)

A throwaway crate checked the dependencies we expect to add:

| Dependency | iOS `cargo check` | Notes |
| --- | --- | --- |
| `cpal = "=0.18.2"` (Apache-2.0) | **pass** (ios, ios-sim) | Pulls `objc2-avf-audio 0.3.2` on iOS. |
| `midir = "0.11.1"` (MIT) → `coremidi 0.9.2` (MIT) | **pass** | — |
| `hound = "3.5.1"` (Apache-2.0) | **pass** | Pure Rust. |
| `objc2-avf-audio 0.3.2` (Zlib/Apache-2.0/MIT) + `block2 0.6` + `objc2-foundation 0.3` | **pass** | Used by our session prototype (§5), which is also clippy-clean on iOS. |
| `midir = "0.10"` **together with** `cpal 0.18.2` | **dependency resolution fails on every target**: `alsa-sys` `links = "alsa"` conflict (midir 0.10.0 → alsa 0.9 → alsa-sys 0.3; cpal 0.18 → alsa 0.11 → alsa-sys 0.4) | **The MIDI agent must use midir ≥ 0.11.1** (its range `alsa >=0.9, <0.13` resolves). |

## 3. Dependency and API inventory (iOS status)

| Item | Where | iOS status |
| --- | --- | --- |
| `cpal 0.16` | engine | Compiles. Functionally inadequate (see §4b). **Upgrade to 0.18.2** (cross-platform breaking changes listed in §6). |
| `rtrb 0.3`, `serde`, `thiserror` | engine | Pure Rust, fine. |
| `voiceloop-analysis` | crate | Pure Rust, compiles. |
| `voiceloop-project` (new, persistence agent) | crate | Pure Rust (serde, thiserror). Should compile; needs a **storage root passed in** (no `$HOME` or `dirs` assumptions). |
| `std::thread::Builder` (control, analysis threads) | engine | Fine on iOS. Threads keep running in the background only while the audio session is active with background audio. |
| `clock.rs` `Instant`-based host clock | engine | Fine. **Caution for MIDI on all Apple platforms:** CoreMIDI `MIDITimeStamp` is in *mach ticks*. On Apple Silicon and iOS, 1 tick ≠ 1 ns (timebase 125/3), so convert with `mach_timebase_info` before comparing with `Instant`/`host_now_ns`. The comment in `clock.rs` ("the same clock CoreAudio and CoreMIDI stamp with") holds for the clock domain, not the units. |
| `devices.rs` name-based identity (D-008) | engine | On iOS CPAL returns a single "Default Device" (0.16) or "default" (0.18). Real input choice is `AVAudioSession` ports (`availableInputs`, `setPreferredInput`). The device list must come from the platform layer on iOS. |
| `routing.rs` `InputRoute`/`OutputRoute` | engine | Portable. On iOS the channel count comes from the session (`inputNumberOfChannels`, `setPreferredInputNumberOfChannels`). |
| `MissingDevicePolicy` | engine | Applies on iOS as a route change; see §4c. |
| `midir` (planned) | engine | midir ≥ 0.11.1 compiles for iOS (CoreMIDI). USB-MIDI and network sessions should appear as CoreMIDI endpoints. **Bluetooth LE MIDI needs a pairing UI** (`CABTMIDICentralViewController`, CoreAudioKit), which is a small Swift plugin. |
| `hound` (planned WAV) | project | Pure Rust, compiles. |
| `tauri 2.12.6` (wry 0.57.2 / tao 0.37.2 / swift-rs) | shell | Documented iOS support; needs Xcode to build. `#[cfg_attr(mobile, tauri::mobile_entry_point)]` is already on `run()`. |
| `RunEvent::Exit` → `Engine::shutdown()` | shell | **iOS does not deliver a clean exit** (apps are suspended, then killed). Takes must be crash-safe (persistence recovery) and the engine should stop recording when the app leaves the foreground without background audio. |
| `tauri.conf.json` window `minWidth 720 / minHeight 520` | shell | Ignored on iOS. iPhone portrait is about 375-440 pt wide and iPad Split View can be about 320 pt. See TOUCH_UI.md. |
| CSP `connect-src ipc: http://ipc.localhost` | shell | iOS uses the `ipc:` custom scheme like macOS, so this should be fine (unverified). |
| `vite.config.ts` `TAURI_DEV_HOST` handling | frontend | Already correct for `tauri ios dev` on a physical device. |
| Tauri plugins (fs, dialog, haptics, ...) | none yet | Not installed. fs: iOS "restricted to Application folder by default". dialog: iOS "does not support folder picker" and returns `file://` URIs ([fs](https://v2.tauri.app/plugin/file-system/), [dialog](https://v2.tauri.app/plugin/dialog/)). |
| Frontend `src/lib/shortcuts.ts` (Space/R/T/M/...) | frontend | Works with hardware keyboards on iPad (WebKit `keydown`). Irrelevant without one, so every action needs a touch control. |
| Frontend `src/lib/useFrame.ts` (shared rAF loop) | frontend | Good design. WKWebView rAF is capped at **60 Hz** on 120 Hz devices and **30 Hz in Low Power Mode**, and pauses in the background ([WebKit explainer](https://webkit.github.io/explainers/animation-frame-rate), [WebKit bug 215745](https://bugs.webkit.org/show_bug.cgi?id=215745), [bug 294338](https://bugs.webkit.org/show_bug.cgi?id=294338)). Visuals must interpolate by time, not by frame count. |

## 4. Research findings

### a. Tauri 2 on iOS (status: **supported with work**)

Evidence:
- Prerequisites: "requires Xcode and is only available on macOS"; install
  "Xcode and not Xcode Command Line Tools"; `rustup target add aarch64-apple-ios x86_64-apple-ios aarch64-apple-ios-sim`;
  `brew install cocoapods` ([prerequisites](https://v2.tauri.app/start/prerequisites/)).
- Workflow: `tauri ios init` (generates `src-tauri/gen/apple` with XcodeGen;
  `--reinstall-deps` shows it installs tools itself), `tauri ios dev [DEVICE]`
  (connected device first, else simulator prompt; `--open` for Xcode; `--host`
  for physical devices), `tauri ios build` (IPA), and `tauri ios run`. Verified
  from local `tauri-cli 2.12.5 --help` and the [develop guide](https://v2.tauri.app/develop/).
- Config: `bundle.iOS.{developmentTeam, minimumSystemVersion (default "15.0"), bundleVersion, frameworks, infoPlist, template}`;
  the `APPLE_DEVELOPMENT_TEAM` env var overrides the team. Tauri looks for
  `Info.ios.plist` next to `tauri.conf.json` and merges `tauri.ios.conf.json`
  ([config reference](https://v2.tauri.app/reference/config/)).
- The generated XcodeGen template sets `UISupportedInterfaceOrientations~ipad`
  and a "Build Rust Code" pre-build phase that calls the Tauri CLI
  ([template](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-cli/templates/mobile/ios/project.yml)).
  Expect iPhone and iPad in one universal app (XcodeGen default device family);
  confirm in Xcode › General › Supported Destinations.
- Physical-device dev server: the device loads the Vite server over the LAN
  (`TAURI_DEV_HOST`). The first run asks for Local Network permission; allow it
  and restart. Debug the webview with Safari › Develop (enable Web Inspector on
  the device).

Risks: version-manager Node (this Mac uses **nvm**) may be invisible to Xcode's
build phase. Tauri mobile is younger than desktop, so expect plugin gaps
(folder picker, BLE MIDI pairing, share sheet), filled with small Swift plugins.

WKWebView constraints for our UI: 60 Hz rAF cap, 30 Hz in Low Power Mode, no
rAF in the background, canvas memory limits at DPR 2-3, and safe-area or
keyboard inset quirks on iOS 26 ([Apple forum 802159](https://developer.apple.com/forums/thread/802159)).
Engine events at ~30 Hz are a good fit. See TOUCH_UI.md.

`tauri ios init` was **not** run. It needs Xcode, it would install Homebrew
packages (xcodegen, cocoapods) as a side effect, and with no way to build, the
generated project would be untested churn.

### b. CPAL on iOS (status: **supported with work**; 0.16 inadequate, 0.18.2 good base)

CPAL 0.16.0 iOS backend (`src/host/coreaudio/ios/mod.rs`, read from the registry copy):
- Top-of-file TODO: "Use AVAudioSession to enumerate buffer size / sample rate /
  number of channels and set buffer size."
- One `Device` named "Default Device". Input configs offer exactly the current
  rate. `BufferSize::Fixed` returns `StreamConfigNotSupported`.
- No `AVAudioSession` calls at all: no category, no activation, no
  interruption or route handling. Each stream gets its own `RemoteIO` unit
  (the input unit has output disabled).

CPAL 0.18.2 (2026-08-16), from the [changelog](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md) and the
[v0.18.2 source](https://github.com/RustAudio/cpal/tree/v0.18.2/src/host/coreaudio/ios):
- 0.17.0: "AVAudioSession integration for device enumeration and buffer size control".
- 0.18.0: timestamps include device latency. The error callback reports **route
  changes** on iOS, **device loss and headphone unplugging**, and `BufferSize::Fixed`
  is validated. Adds `StreamTrait::buffer_size()`.
- 0.18.2: **"iOS streams resume automatically after an audio session interruption ends"**,
  xruns are reported as `ErrorKind::Xrun`, and timestamps update on route change.
- `session_event_manager.rs` observes `AVAudioSessionInterruptionNotification`,
  `RouteChangeNotification`, `MediaServicesWereLost/Reset`. Route reasons map to
  `DeviceChanged` (old device unavailable), `StreamInvalidated` (category change,
  override, route-configuration change, media reset) and `DeviceNotAvailable`.
  **Interruptions emit no error**: the stream just stops and resumes.
- It **still does not set the category or mode, or activate the session**. That
  is left to the app (its iOS example configures the session before calling
  Rust). `BufferSize::Fixed` is limited to **256-4096 frames** and applied with
  `setPreferredIOBufferDuration`; there is no `setPreferredSampleRate`. Still one
  device, separate `RemoteIO` units per stream, and no duplex unit.

Duplex vs two streams (D-010): on iOS both RemoteIO units run on the **same
hardware clock** (one session), so the drift D-010's `InputBridge` guards
against should be about zero and priming plus underrun handling still applies.
CPAL's own `ios-feedback` example uses exactly this input-stream plus
output-stream pattern. A single duplex RemoteIO unit (render callback pulls
input with `AudioUnitRender`) is Apple's classic low-latency design. It gives
sample-aligned input and removes the ring and priming latency (about 10 ms
today), but CPAL cannot express it. **Recommendation:** keep the two-stream
design for parity with macOS and measure on device. Build our own duplex
RemoteIO backend only if the measurements fail the criteria in §7.

Thin platform layer: **yes, write one, in Rust with objc2** (not a Swift
plugin). The session must be configured before the engine's control thread
opens streams, and must be reconfigured on its schedule. Keeping it in the
engine crate keeps it Tauri-free (AGENTS.md), headless-testable on macOS
(stubs), and in one language. objc2 crates are already in the dependency graph
via CPAL.

### c. AVAudioSession configuration (status: **documented; unverified on device**)

Recommended defaults (sources: Apple docs via `developer.apple.com/tutorials/data/documentation/...json`):

| Setting | Value | Why / evidence |
| --- | --- | --- |
| Category | `playAndRecord` | Only category that records and plays. CPAL input fails without it on iOS 17+ ([cpal#842](https://github.com/RustAudio/cpal/issues/842)). |
| Mode | `measurement` for Practice Lab pitch work; offer `default` as a user option | Measurement "minimize[s] the amount of system-supplied signal processing", uses the primary mic, and "results in a lower-output playback level" ([Apple](https://developer.apple.com/documentation/avfaudio/avaudiosession/mode-swift.struct/measurement)). AGC off is right for analysis; the quieter playback is a tradeoff to expose. |
| `.defaultToSpeaker` | **on** | Otherwise iPhone routes output to the **receiver** (earpiece). Caveat: with this option "plugging in a headset doesn't cause the route to change" ([Apple](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/defaulttospeaker)). Verify on device; if it pins the speaker with headphones present, use `overrideOutputAudioPort(.speaker)` only when the route is the receiver. iPad has no receiver, so this matters on iPhone. |
| `.allowBluetoothA2DP` | on | Bluetooth headphones as output only, with good quality and **high latency** (≈150-250 ms, unverified). The looper must warn when the output is Bluetooth. |
| `.allowBluetoothHFP` | **off** | Makes BT headsets the input: 8-16 kHz narrowband and it forces the whole session into HFP. Unacceptable for music ([Apple](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/allowbluetoothhfp)). iOS 26 `.bluetoothHighQualityRecording` is default-mode only, not in the EU, and "may increase input latency" ([Apple](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/bluetoothhighqualityrecording)). Later, opt-in only. |
| `.mixWithOthers` | off | We want exclusive, uninterrupted I/O. |
| Preferred sample rate | 48 000 | `setPreferredSampleRate`; read back `sampleRate`. External USB devices may force 44 100. |
| Preferred IO buffer | 128 frames (≈2.7 ms) default; 64-512 user setting | "minimum ... at least 0.005 seconds (256 frames) but might be lower depending on the hardware" ([Apple](https://developer.apple.com/documentation/avfaudio/avaudiosession/setpreferrediobufferduration(_:))). **Set it ourselves and pass `BufferSize::Default` to CPAL**, because CPAL 0.18 rejects `Fixed` below 256. Always read back `IOBufferDuration`. |
| `setPrefersNoInterruptionsFromSystemAlerts(true)` | on | With banner-style calls, the session is interrupted only if the call is answered (iOS 14.5+). |
| Background | `UIBackgroundModes: audio` (added in `src-tauri/Info.ios.plist`) | Loops keep playing and takes keep recording when the app is in the background. Otherwise iOS suspends the app and audio stops. |

Event handling to surface through our engine event model (proposed; the engine
owner implements it):

| iOS event | Source | Engine action | Event to UI |
| --- | --- | --- | --- |
| Interruption began (call, Siri, alarm, other app's exclusive session, iPad cover closed) | Our `AVAudioSessionInterruptionNotification` observer (CPAL 0.18 stops the unit silently) | Control thread: close open takes at the current frame (fade-out), pause the transport, keep loops | `audioSession { state: "interrupted", reason }` |
| Interruption ended (`shouldResume`) | Observer (CPAL auto-resumes the unit) | Re-prime the input bridge, leave the transport paused until the user resumes | `audioSession { state: "active" }` |
| Route change: new device available (headphones, USB interface) | CPAL `StreamInvalidated`/`DeviceChanged`, or the observer | Restart streams **without dropping looper state** (§5), re-read the session snapshot, re-apply the input route | `audioRoute { inputs, outputs, sampleRate, ioBufferFrames, inputLatency, outputLatency }` |
| Route change: old device unavailable (headphones unplugged) | CPAL `DeviceChanged` | Same as above. Per Apple guidance, pause playback (privacy) | `audioRoute` + `warning { kind: "routeLost" }` |
| Sample rate changed (USB device at 44.1 kHz) | Snapshot after restart | Loops are in frames at the old rate. Either reject recording until the rate matches, or resample (later). | `warning { kind: "sampleRateChanged", from, to }` |
| Media services reset | CPAL `StreamInvalidated` | Reconfigure the session from scratch, restart streams; do not auto-play (Apple) | `audioSession { state: "reset" }` |
| Microphone permission denied | Platform permission API | Do not start input; metering shows a permission state | `error { kind: "permissionOrUnavailable" }` (exists) + `micPermission: "denied"` in status |

### d. Microphone permission (status: **documented**)

- `NSMicrophoneUsageDescription` is mandatory: "If an application attempts to
  access any of the device's microphones without a corresponding purpose string,
  the app exits" ([Apple](https://developer.apple.com/documentation/avfaudio/avaudioapplication/requestrecordpermission(completionhandler:))).
  It is now in `src-tauri/Info.ios.plist`.
- API: `AVAudioApplication.shared.recordPermission` (`undetermined | denied | granted`)
  and `AVAudioApplication.requestRecordPermission(completionHandler:)` (iOS 17+,
  which also exists on macOS 14+, so one code path serves both). The prompt
  appears once.
- **Denial looks like silence, not an error**: "it captures only silence (zeroed
  out audio samples)". Our `silentInput` warning would fire, but the UI should
  check permission first and show "Microphone access is off: Settings ›
  Privacy & Security › Microphone › VoiceLoop".
- Flow: on the first Start, `undetermined` → request (completion on any thread
  → post to the control thread) → start streams on `granted`. On `denied`,
  emit an error and do not start.
- Minimum iOS: `AVAudioApplication` needs iOS 17. **Recommend
  `bundle.iOS.minimumSystemVersion = "17.0"`** (Tauri's default is 15.0), which
  also avoids the deprecated `AVAudioSession.recordPermission` path.

### e. External audio interfaces and MIDI (status: **platform-limited / unverified**)

USB audio: iOS and iPadOS support USB Audio Class (class-compliant) devices
through USB-C, or Lightning plus the Lightning-to-USB 3 Camera Adapter (which
has power pass-through). A connected interface becomes an input and/or output
port in `AVAudioSession.availableInputs` and `currentRoute`. Multichannel
input: `maximumInputNumberOfChannels` / `setPreferredInputNumberOfChannels`
([Apple](https://developer.apple.com/documentation/avfaudio/avaudiosession/setpreferredinputnumberofchannels(_:)))
("only after setting the category and mode and activating the session"), then
our `InputRoute` picks channels.

**Zoom H4n Pro specifically: not confirmed by Zoom for iOS.**
- Zoom's product page says "2-input/2-output USB audio interface **for PC or Mac**"
  and "USB functions can be operated by USB bus power". iOS and class
  compliance are not mentioned ([zoomcorp.com H4n Pro](https://zoomcorp.com/en/us/handheld-recorders/handheld-recorders/h4n-pro/)).
- Zoom's iOS/iPadOS 26 compatibility list (Jan 2026) does **not** list the H4n
  or H4n Pro, only the H4essential ([Zoom iOS 26 compatibility](https://zoomcorp.com/en/ca/news/ios26compatibility)).
- Community reports (not authoritative): works with an M2 iPad Pro "if a little
  finicky". You "must connect the zoom to the iPad and turn on audio interface
  mode before launching" the app, "only ... 44.1kHz" worked, and "external DC
  power is a must" ([Loopy Pro gear DB](https://gear.loopypro.com/audio-interfaces/zoom-h4n-pro-handy-recorder)).
  An older thread reports mono-only input on iOS 14.6 ([Loopy Pro forum](https://forum.loopypro.com/discussion/45577/problem-with-h4-audio-interface-on-ios-14-6-only-mono-anyone-with-a-zoom-h4)).
- Implications: plan for **44.1 kHz sessions**, so the engine must not assume
  48 kHz. Power the H4n Pro from its AC adapter (an iPad or iPhone may refuse a
  bus-powered device that draws too much power). Expect 2 input channels at most. Test with the
  H4n set to audio interface mode *before* launching VoiceLoop, and also
  hot-plugged (a route change).

MIDI: CoreMIDI is on iOS. `midir 0.11.1` → `coremidi 0.9.2` **compiles** for
iOS. Class-compliant USB MIDI keyboards and controllers should enumerate like
on macOS (unverified). **Bluetooth LE MIDI** devices must be paired in-app with
`CABTMIDICentralViewController` (CoreAudioKit) before they appear as CoreMIDI
endpoints. That needs a small Swift Tauri plugin (iOS) or an objc2-core-audio-kit
call presenting a view controller. Network MIDI (RTP) sessions also appear as
endpoints. Background MIDI input works only while the audio session keeps the
app alive.

Hardware keyboards: WKWebView receives `keydown` for physical keys on iPad
(our `shortcuts.ts`). ⌘-chords are partly intercepted by iPadOS. The
hold-⌘ shortcut overlay only lists native `UIKeyCommand`s, which is a
nice-to-have via a Swift plugin later.

### f. Latency and clock expectations (status: **unverified; measure**)

Model (per direction): `IOBufferDuration` + `inputLatency`/`outputLatency`
(converter and hardware latency reported by the session) + our input-bridge
cushion (~10 ms prime today). Expected round trip on a modern iPad with the
built-in mic and speaker at 48 kHz and 128 frames: **≈ 8-15 ms** (unverified;
Apple only guarantees ≥ 5 ms buffers are possible). USB interfaces add
their own buffering and often under-report latency. Bluetooth A2DP output adds
≈150-250 ms (unverified), which is unusable for overdubbing.

Clock: one session means one hardware clock for input and output, so drift is
about zero. CPAL 0.18 timestamps include hardware latency and update on route
change. Our `clock.rs` anchors still apply.

What to measure on device (see BUILD_AND_INSTALL.md §6):
1. Granted vs requested `sampleRate` and `IOBufferDuration` for 64/128/256/512.
2. Reported `inputLatency + outputLatency + 2 × IOBuffer` vs **loopback measured**
   round trip (D-015 calibration: click out, detect in). Built-in speaker to mic
   works acoustically; use a cable for USB interfaces.
3. Callback period jitter (from `StreamStats`), xrun count over 10 min with UI
   active, with Low Power Mode on, and with the app backgrounded.
4. `InputBridge` underrun and drift counters (expect drift ≈ 0).
5. CPU headroom: render time / callback period with analysis on.
6. Loop length stability: after 30 min of looping, boundary error must be 0 frames (it is an exact integer clock).

### g. Project storage (status: **supported with work**)

Format (ARCHITECTURE.md; `docs/projects/persistence/` was empty at time of
writing, so it is **PENDING** alignment with the persistence agent):
`<Name>.voiceloop/` folder with `project.json` and `audio/*.wav`.

| Concern | Approach |
| --- | --- |
| Where projects live | App sandbox `Documents/Projects/<Name>.voiceloop/`. The Rust side writes with `std::fs` directly (no plugin needed). The shell resolves the root with Tauri's path resolver (`app.path().document_dir()`; verify on iOS) and passes it to the engine and project crates. **No crate may compute its own root.** |
| Visibility in Files | `UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace` (added to `Info.ios.plist`) make `Documents` appear under "On My iPad › VoiceLoop". The `.voiceloop` folder shows as a plain folder unless we export a UTType conforming to `com.apple.package` (`UTExportedTypeDeclarations`), which would make Files and macOS Finder treat it as one document. Recommended later; **PENDING**. |
| Import / export | Tauri dialog on iOS has **no folder picker** and returns `file://` URIs. Importing a `.voiceloop` folder or package and exporting to other apps needs `UIDocumentPickerViewController` (folder or package UTType) and `UIActivityViewController` (share sheet). Plan a small Swift plugin. For AirDrop and Mail, export a **single-file archive** (`.voiceloop.zip`, or a flat package) because folders do not travel well. Format choice is **PENDING** with the persistence agent. |
| Security-scoped URLs | Anything picked outside the sandbox must be accessed between `startAccessingSecurityScopedResource` and `stop…`, and a **bookmark** must be stored to reopen it later. Recommend *copying* imports into `Documents` rather than editing in place, at least for v1. The Tauri fs plugin exposes start/stop security-scoped permissions. |
| iCloud Drive | Files may be placeholders that are not downloaded, and writes need `NSFileCoordinator`. Live recording into iCloud is risky. v1: projects stay local and iCloud is only an export target. Note that a free Apple ID team cannot use the iCloud capability (§h). |
| Crash and kill safety | iOS can kill a backgrounded app without `RunEvent::Exit`. Takes must be streamed to disk with recoverable headers (the persistence crate's "recovery") and `project.json` written atomically (temp file plus rename). |
| Privacy manifest | The Tauri fs plugin requires `PrivacyInfo.xcprivacy` with `NSPrivacyAccessedAPICategoryFileTimestamp` / reason `C617.1` in `src-tauri/gen/apple` ([fs plugin](https://v2.tauri.app/plugin/file-system/)). Needed for App Store, not for dev installs. |

### h. Signing and device testing (status: **documented**)

| | Free Apple ID ("Personal Team") | Paid Apple Developer Program (US$99/yr) |
| --- | --- | --- |
| Install on own iPad and iPhone | Yes, via Xcode | Yes |
| Profile lifetime | **7 days**, then rebuild and reinstall ([Apple](https://developer.apple.com/support/compare-memberships)) | About 1 year |
| Limits | **3 apps per device**, 10 App IDs per 7 days | 100 devices per type per year |
| Capabilities | Basic only. Background audio is an Info.plist key and works. **No iCloud, no App Groups, no push** | All |
| TestFlight / sharing with others | No | Yes |

Steps (details in BUILD_AND_INSTALL.md): add the Apple ID in Xcode › Settings ›
Accounts. Set `bundle.iOS.developmentTeam` (or `APPLE_DEVELOPMENT_TEAM`, or pick
the team in Xcode › Signing & Capabilities; automatic signing is the Tauri
default). The bundle id `dev.gseyfried.voiceloop` must be globally unique;
a Personal Team may need a suffix. Enable **Developer Mode** on each device
(iOS/iPadOS 16+: Settings › Privacy & Security › Developer Mode, then restart).
With a free team, trust the developer certificate (Settings › General › VPN &
Device Management). The installed Xcode must support the iOS version on the
devices.

## 5. Architecture recommendations

### Platform boundary

Put platform differences behind one module in the engine,
`crates/voiceloop-engine/src/platform/` (`mod.rs`, `ios.rs`, `desktop.rs`),
selected by `cfg(target_os = "ios")`. The engine API stays identical, and the
Tauri shell does not change except for the storage root.

| Concern | iOS implementation | macOS implementation |
| --- | --- | --- |
| `prepare_audio(&SessionRequest) -> SessionInfo` | AVAudioSession category, mode, options, rate, buffer, activate. Returns the granted values. | No-op; values come from CPAL. Optionally set the device buffer later. |
| `release_audio()` | `setActive(false)` | No-op |
| `list_inputs()` / `select_input(id)` | `availableInputs` / `setPreferredInput` (port UID = id) | CPAL devices (name or `DeviceId` = id) |
| `mic_permission()` / `request_mic_permission(cb)` | `AVAudioApplication` | `AVAudioApplication` on macOS 14+ (else "unknown"; rely on `silentInput`) |
| Session events → control thread | NSNotificationCenter observers → `mpsc` command (`Interrupted`, `Resumed`, `RouteChanged`, `MediaReset`) | CPAL error callback (`DeviceChanged`, etc.) only |
| Storage root | Passed in by the shell (Documents) | Passed in by the shell (`~/Music/VoiceLoop` or app data) |
| MIDI | midir (CoreMIDI) + Swift plugin for BLE pairing | midir (CoreMIDI) |

Dependencies for `ios.rs` (target-gated so desktop builds are unaffected):

```toml
[target.'cfg(target_os = "ios")'.dependencies]
objc2 = "0.6"
block2 = "0.6"
objc2-foundation = { version = "0.3", features = ["NSString", "NSArray", "NSError"] }
objc2-avf-audio = { version = "0.3", features = ["AVAudioSession", "AVAudioSessionTypes", "AVAudioSessionRoute", "AVAudioApplication", "block2"] }
```

All are Zlib/Apache-2.0/MIT, maintained by the objc2 project, and already
transitively present through CPAL. A **compile-checked prototype** of `ios.rs`
(configure and activate, snapshot, available inputs, select input, input
channels, permission query and request) is kept at
`docs/projects/ios/prototype/ios_session.rs`, with the probe manifest
`prototype/Cargo.probe.toml`. It is not compiled by the workspace. It passed
`cargo check` and `cargo clippy` for `aarch64-apple-ios` and `-sim`, and a
sentinel `compile_error!` proved the iOS module was really compiled. Its core is:

```rust
let s = AVAudioSession::sharedInstance();
s.setCategory_mode_options_error(AVAudioSessionCategoryPlayAndRecord.unwrap(),
    AVAudioSessionModeMeasurement.unwrap(),
    AVAudioSessionCategoryOptions::DefaultToSpeaker | AVAudioSessionCategoryOptions::AllowBluetoothA2DP)?;
s.setPreferredSampleRate_error(48_000.0)?;
s.setPreferredIOBufferDuration_error(128.0 / 48_000.0)?;
let _ = s.setPrefersNoInterruptionsFromSystemAlerts_error(true);
s.setActive_error(true)?;
// then read back s.sampleRate(), s.IOBufferDuration(), s.inputLatency(), s.outputLatency(), s.currentRoute()
```

**Status (hardware-io round 3):** landed as
`crates/voiceloop-engine/src/platform/{mod.rs,ios.rs,desktop.rs}` on CPAL
0.18.2, with interruption, route-change, and media-services-reset observers
and the permission request; route changes and resets use the engine's
stream-restart path (loops parked and restored). See
`docs/projects/hardware-io/README.md` (Platform module).

### Changes the current engine needs to be iOS-ready (owners: engine/hardware-io agents)

1. **Looper state must survive stream restarts.** Today `LooperCore` is moved
   into the output closure and dropped with the stream (`stop_input` "releases
   devices and buffers"). Proposal: a `restart_streams` path that hands the
   core back. On stop, the output closure pushes `Box<LooperCore>` into a
   one-slot rtrb "return" ring after a `Park` command is acknowledged, then the
   new stream takes it. Alternatively, own the core in an `Arc` with a
   park/unpark handshake. Sample-rate changes must reject or resample. This is
   required on macOS too (USB interface unplug, AirPods connect).
2. **Upgrade CPAL 0.16 → 0.18.2** (cross-platform). **Done (hardware-io round 3).** Breaking changes that touch our code:
   `DeviceTrait::name()` removed (use `Display` or `description()`; D-008 can move to the new
   **stable `DeviceId`** and `device_by_id`). Per-operation errors are merged into `cpal::Error`
   with an `ErrorKind`, so `error.rs`'s `From` impls and `clock::stream_error_code`
   need rewriting. `SampleRate` is now a `u32` alias (`.0` accesses go). `build_*_stream`
   takes `StreamConfig` by value. Streams no longer start on build (we already call `play()`).
   `StreamInstant` was reworked. MSRV is 1.85.
3. Call `platform::prepare_audio` **before** any CPAL device query or stream
   build, from the control thread. Re-run it after media-services reset.
4. On iOS, pass `BufferSize::Default` and set the buffer through the session.
5. Never hard-code 48 kHz (`InputBridge`, metronome, analysis must take the granted rate).
6. Add the engine events from the table in §4c (`audioSession`, `audioRoute`,
   new warnings) and a `micPermission` field in `EngineStatus`, mirrored in
   `src/models/domain.ts`.
7. Interruption handling: close open takes with a fade, then pause the transport. Never leave a
   half-written layer that is still marked recording.

### What other agents must avoid

- No `cpal::default_host()` device-name logic outside `devices.rs` and the
  platform module. No assumptions that device names are meaningful on iOS.
- No `std::env::home_dir`, `dirs`, `~/…` or absolute paths. Take a root `&Path` from the caller.
- No `midir 0.10` (conflicts with CPAL 0.18). Use ≥ 0.11.1.
- No desktop-only Tauri plugins without checking the iOS column of the plugin
  page (global-shortcut, window-state, tray, updater, shell are desktop-only).
- No reliance on `RunEvent::Exit` for persistence.
- Frontend: no hover-only affordances, no right-click-only menus, no
  per-frame logic that assumes 60 Hz or more, no `@tauri-apps/api` outside
  `bridge.ts` (unchanged rule). See TOUCH_UI.md.
- No C/C++ dependencies that need a special SDK setup without an iOS build plan (Signalsmith
  Stretch FFI is the known one; `cc` builds for iOS need Xcode's SDK).

### Staged plan

| Stage | Goal and exit criteria | Effort | Risk |
| --- | --- | --- | --- |
| 0. Toolchain | User frees disk and installs Xcode. `scripts/ios-setup-check.sh` passes. `tauri ios init` run. `cargo check -p voiceloop --target aarch64-apple-ios-sim` passes. Commit decision on `src-tauri/gen/apple` (Tauri expects it committed; it carries its own `.gitignore` for build output). | 0.5-1 day (mostly downloads) | Low (disk) |
| 1. PoC | App launches in Simulator, UI renders, `list_input_devices` returns. On the iPad, the mic prompt appears and the meter moves (with the `ios.rs` session layer). | 2-4 days | Medium: first-build Tauri iOS issues, nvm/Xcode PATH |
| 2. Audio session | CPAL 0.18.2 upgrade, platform module, permission flow, interruption and route events, **looper survives restarts**, background audio. Exit: plug and unplug headphones and the H4n mid-loop, take a phone call, and Siri; loops intact. | 1.5-2.5 weeks | **High**: correctness under route churn |
| 3. Looper parity | Record, overdub, undo, and metronome on iPad and iPhone. Loopback latency calibration. Measurements from §4f recorded. | 1 week | Medium |
| 4. Files and MIDI | Documents storage visible in Files, import and export (Swift document picker / share sheet plugin), midir USB MIDI, BLE MIDI pairing plugin. | 1.5-2 weeks | Medium (Swift plugins, security scope) |
| 5. Touch UI | TOUCH_UI.md implemented: adaptive layout, multitouch transport, safe areas, Split View, keyboard and MIDI mapping. | 2-3 weeks (can overlap 2-4) | Medium |

### Fallbacks and D-001 revisit criteria

If CPAL-on-iOS proves inadequate (stage 2/3 measurements), replace only the
**I/O backend** behind an engine-internal `AudioBackend` interface (start
input/output with a config, then deliver input blocks and pull output blocks
into the existing `LooperCore`/meter/analysis code):

1. **Own RemoteIO duplex backend** via `objc2-audio-toolbox` (already a
   dependency). One unit, input enabled, render callback calls `AudioUnitRender`
   on bus 1 and then `LooperCore::process`. Sample-aligned input with no bridge.
   Effort 1-2 weeks. Also usable on macOS through the HAL output unit.
2. **AVAudioEngine** with `AVAudioSinkNode` and `AVAudioSourceNode` realtime blocks. Simpler
   session integration, less control, slightly higher latency. Effort about 1 week.

Trigger criteria for leaving CPAL on iOS (any of these, on device, after stage 2):
round trip more than 2 × (reported latency + 2 × buffer) and not fixable by calibration;
more than 1 xrun per 10 min at 256 frames with the UI idle; loss of loops or
crashes on route changes that cannot be fixed above CPAL; inability to select
USB input channels.

**D-001 (c) "iOS via Tauri proves impractical"** should only be triggered by
*shell/UI* problems, because the audio problems above are solved inside the Rust
engine regardless of the shell. Criteria: WKWebView touch-to-action latency
above ~50 ms for transport buttons; sustained rAF below 30 fps on a current iPad
with our visualisers; Tauri iOS lifecycle or IPC bugs blocking background
audio or event delivery; or Swift plugin needs (picker, share, BLE MIDI)
that cannot be met. The fallback is then a native SwiftUI shell over the same
Rust engine (C ABI or UniFFI). Engine and analysis are reused, and the React UI is not.

## 6. Files changed by this agent

- `docs/projects/ios/README.md`, `COMPATIBILITY_MATRIX.md`, `BUILD_AND_INSTALL.md`, `TOUCH_UI.md` (new)
- `scripts/ios-setup-check.sh` (new, read-only checker; `--compile` adds the iOS cargo checks)
- `src-tauri/Info.ios.plist` (new, iOS-only: mic usage string, background audio, Files sharing). Unverified until the first `tauri ios build`.
- `docs/projects/ios/prototype/ios_session.rs` + `Cargo.probe.toml` (compile-checked AVAudioSession layer prototype; not built by the workspace)
- No hub files edited. No engine, frontend, or config edits. Rust targets
  `aarch64-apple-ios` and `aarch64-apple-ios-sim` added via rustup (user toolchain).

## 7. PENDING

- PENDING: everything marked *unverified*, which needs Xcode plus a device.
- PENDING: H4n Pro behaviour on the user's iPad/iPhone (class compliance, 44.1 vs 48 kHz, channel count, power).
- PENDING: `defaultToSpeaker` vs headphone routing behaviour on iPhone with our options.
- PENDING: persistence agent's final package format, export archive choice, and UTType declaration.
- PENDING: whether `src-tauri/gen/apple` is committed (lead decision; Tauri's convention is yes, minus build output).
- PENDING: `bundle.iOS.minimumSystemVersion` "17.0" and `developmentTeam` in `tauri.conf.json` (lead/user; proposed diff in BUILD_AND_INSTALL.md).
- PENDING: decision record for "AVAudioSession layer in Rust via objc2" and "CPAL 0.18.2 upgrade" (proposed D-0xx for the lead).

## 8. Sources

Tauri 2 (tauri-cli 2.12.5, tauri 2.12.6):
[prerequisites](https://v2.tauri.app/start/prerequisites/) ·
[develop / iOS dev](https://v2.tauri.app/develop/) ·
[config reference (`bundle.iOS`)](https://v2.tauri.app/reference/config/) ·
[iOS signing](https://v2.tauri.app/distribute/sign/ios/) ·
[App Store distribution](https://v2.tauri.app/distribute/app-store/) ·
[fs plugin](https://v2.tauri.app/plugin/file-system/) ·
[dialog plugin](https://v2.tauri.app/plugin/dialog/) ·
[iOS XcodeGen template](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-cli/templates/mobile/ios/project.yml)

CPAL:
0.16.0 iOS backend (local registry source `cpal-0.16.0/src/host/coreaudio/ios/`) ·
[CHANGELOG (0.17.0 to 0.18.2)](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md) ·
[v0.18.2 iOS `mod.rs`](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/coreaudio/ios/mod.rs) ·
[v0.18.2 `session_event_manager.rs`](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/coreaudio/ios/session_event_manager.rs) ·
[ios-feedback example](https://github.com/RustAudio/cpal/tree/v0.18.2/examples/ios-feedback) ·
[issue #842: no iOS input devices without playAndRecord session](https://github.com/RustAudio/cpal/issues/842)

Apple (AVFAudio and others):
[measurement mode](https://developer.apple.com/documentation/avfaudio/avaudiosession/mode-swift.struct/measurement) ·
[defaultToSpeaker](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/defaulttospeaker) ·
[allowBluetoothHFP](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/allowbluetoothhfp) ·
[allowBluetoothA2DP](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/allowbluetootha2dp) ·
[bluetoothHighQualityRecording (iOS 26)](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/bluetoothhighqualityrecording) ·
[setPreferredIOBufferDuration](https://developer.apple.com/documentation/avfaudio/avaudiosession/setpreferrediobufferduration(_:)) ·
[setPreferredSampleRate](https://developer.apple.com/documentation/avfaudio/avaudiosession/setpreferredsamplerate(_:)) ·
[setPreferredInput](https://developer.apple.com/documentation/avfaudio/avaudiosession/setpreferredinput(_:)) ·
[setPreferredInputNumberOfChannels](https://developer.apple.com/documentation/avfaudio/avaudiosession/setpreferredinputnumberofchannels(_:)) ·
[setPrefersNoInterruptionsFromSystemAlerts](https://developer.apple.com/documentation/avfaudio/avaudiosession/setprefersnointerruptionsfromsystemalerts(_:)) ·
[Handling audio interruptions](https://developer.apple.com/documentation/avfaudio/handling-audio-interruptions) ·
[Responding to route changes](https://developer.apple.com/documentation/avfaudio/responding-to-audio-route-changes) ·
[mediaServicesWereResetNotification](https://developer.apple.com/documentation/avfaudio/avaudiosession/mediaserviceswereresetnotification) ·
[AVAudioApplication.requestRecordPermission](https://developer.apple.com/documentation/avfaudio/avaudioapplication/requestrecordpermission(completionhandler:)) ·
[UIBackgroundModes](https://developer.apple.com/documentation/bundleresources/information-property-list/uibackgroundmodes) ·
[Choosing a membership (free vs paid limits)](https://developer.apple.com/support/compare-memberships) ·
[Enabling Developer Mode](https://developer.apple.com/documentation/xcode/enabling-developer-mode-on-a-device)

WebKit: [animation frame rate explainer](https://webkit.github.io/explainers/animation-frame-rate) ·
[bug 215745 (30 fps Low Power Mode)](https://bugs.webkit.org/show_bug.cgi?id=215745) ·
[bug 294338 (WKWebView 120 Hz)](https://bugs.webkit.org/show_bug.cgi?id=294338) ·
[Apple forum 802159 (iOS 26 safe-area/keyboard)](https://developer.apple.com/forums/thread/802159)

Hardware and crates:
[Zoom H4n Pro product page](https://zoomcorp.com/en/us/handheld-recorders/handheld-recorders/h4n-pro/) ·
[Zoom iOS/iPadOS 26 compatibility](https://zoomcorp.com/en/ca/news/ios26compatibility) ·
[Loopy Pro gear DB: H4n Pro (community)](https://gear.loopypro.com/audio-interfaces/zoom-h4n-pro-handy-recorder) ·
[Loopy Pro forum: H4 mono on iOS 14.6 (community)](https://forum.loopypro.com/discussion/45577/problem-with-h4-audio-interface-on-ios-14-6-only-mono-anyone-with-a-zoom-h4) ·
[midir](https://github.com/Boddlnagg/midir) (0.11.1, MIT) ·
crates.io metadata for cpal 0.18.2 (Apache-2.0), coremidi 0.9.2 (MIT), hound 3.5.1 (Apache-2.0), objc2-avf-audio 0.3.2 (Zlib/Apache-2.0/MIT).
