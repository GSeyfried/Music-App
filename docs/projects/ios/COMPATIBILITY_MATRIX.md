# Compatibility matrix: macOS × iPadOS × iPhone (iOS)

Status as of 2026-11-17. **Nothing in the iPadOS or iPhone columns has run on a device or simulator.**

Legend:
- **supported**: works today, or is documented by Apple/Tauri/CPAL with no extra work beyond normal integration.
- **supported (work)**: documented and feasible, but code we haven't written is needed (see README §5).
- **unverified**: expected to work; needs hardware to confirm.
- **platform-limited**: the platform restricts or degrades it; design around it.
- **n/a**: does not apply.

macOS status means "written for macOS". Per ARCHITECTURE.md, Mac hardware
verification is still in progress by other agents.

## Build and shell

| Feature | macOS | iPadOS | iPhone | Notes |
| --- | --- | --- | --- | --- |
| `voiceloop-analysis` compiles | supported | supported (compiles) | supported (compiles) | `cargo check` passes for `aarch64-apple-ios` and `-sim`. |
| `voiceloop-engine` compiles | supported | supported (compiles) | supported (compiles) | CPAL 0.18.2 + midir 0.11.1 + `platform/ios.rs`: `cargo check` and `cargo clippy` pass for `aarch64-apple-ios`, `cargo check` for `-sim` (2026-11-17, hardware-io round 3). Linking needs the iOS SDK (Xcode). |
| Tauri shell builds | supported | supported (work) | supported (work) | Needs full Xcode. Build scripts call `xcrun --sdk iphoneos`. |
| Simulator run | n/a | unverified | unverified | Simulator audio goes through the Mac's hardware and is **not** evidence for device audio. |
| Install on own device | n/a | supported (work) | supported (work) | Free Apple ID: 7-day profiles, 3 apps per device. Developer Mode required. |
| Universal app (one build for iPad and iPhone) | n/a | unverified | unverified | XcodeGen default; check Supported Destinations in Xcode. |
| Background operation | supported | supported (work) | supported (work) | `UIBackgroundModes: audio` (in `Info.ios.plist`). Only while the session is active. |
| Clean shutdown hook (`RunEvent::Exit`) | supported | platform-limited | platform-limited | iOS suspends and then kills apps. Persistence must be crash-safe. |

## Audio I/O

| Feature | macOS | iPadOS | iPhone | Notes |
| --- | --- | --- | --- | --- |
| Input capture (built-in mic) | supported | supported (work) | supported (work) | Needs a `playAndRecord` session, activated before CPAL (README §4b/c). |
| Output (looper, click) | supported | supported (work) | supported (work) | iPhone: `defaultToSpeaker` or audio goes to the earpiece. |
| Separate input and output streams (D-010) | supported | unverified | unverified | Same hardware clock on iOS, so drift ≈ 0. CPAL's iOS example uses this pattern. |
| True duplex single I/O unit | n/a | platform-limited | platform-limited | Not expressible in CPAL. Fallback: our own RemoteIO backend. |
| Device enumeration | supported (CPAL 0.18 stable `DeviceId` = CoreAudio UID, name fallback) | supported (work) | supported (work) | CPAL exposes a single "default" device. `platform::SessionInfo` lists route ports; `availableInputs` selection is not wired yet. |
| Input selection (built-in vs USB vs headset) | supported | supported (work) | supported (work) | `setPreferredInput(port)`. |
| Output device selection | supported | platform-limited | platform-limited | iOS routes output itself (the user picks via Control Center / AirPlay). Apps can only override to the speaker. |
| Sample rate control | supported (device) | supported (work) | supported (work) | `setPreferredSampleRate` is a request; external devices may force 44.1 kHz. |
| Buffer size control | supported (CPAL `Fixed`, clamped to the device range) | supported (work) | supported (work) | Engine requests it through `setPreferredIOBufferDuration` (`platform::prepare_audio`) and passes `BufferSize::Default` to CPAL on iOS. Granted value in the `audioSessionInfo` event. |
| Small buffers (64-128 frames) | unverified | unverified | unverified | Apple: ≥ 5 ms guaranteed, lower "depending on hardware". |
| Reported latency | supported (CPAL 0.18 includes device latency) | supported (work) | supported (work) | `inputLatency`/`outputLatency` in `audioSessionInfo`; CPAL 0.18 timestamps. Still calibrate (`start_latency_calibration`). |
| Interruptions (calls, Siri, alarms) | n/a | supported (written, unverified) | supported (written, unverified) | `platform/ios.rs` observes `AVAudioSessionInterruptionNotification`: began stops the streams (loops parked); ended with shouldResume restarts them. `audioSession` events. |
| Route changes (headphones, USB) | supported (written): default-device change and CPAL `DeviceChanged` restart the streams | supported (written, unverified) | supported (written, unverified) | `AVAudioSessionRouteChangeNotification` (new/old device, override, configuration change) triggers `restart_audio`, which parks and restores the loops. |
| Media services reset | n/a | supported (written, unverified) | supported (written, unverified) | Observed; restarts audio, which re-runs `prepare_audio`. Test via Settings › Developer › Reset Media Services. |
| Mic permission prompt and status | supported (status `unknown`; denial shows as `silentInput`) | supported (written, unverified) | supported (written, unverified) | `EngineStatus.micPermission`; `request_mic_permission` command answers with an `audioSession` `micPermission` event (`AVAudioApplication`). |
| Raw input (no AGC or voice processing) | supported | supported (work) | supported (work) | `measurement` mode. Playback is quieter. |
| Bluetooth headphones (output) | supported | platform-limited | platform-limited | A2DP output only, high latency. Warn in the looper. |
| Bluetooth mic (HFP) | supported | platform-limited | platform-limited | Narrowband; disabled by default. iOS 26 high-quality BT recording is default-mode only and not available in the EU. |
| Input monitoring (`routing.rs MonitorConfig`) | supported | unverified | unverified | Speaker-plus-mic feedback risk on iPhone and iPad. Allow only with headphones (check the route). |

## External hardware

| Feature | macOS | iPadOS | iPhone | Notes |
| --- | --- | --- | --- | --- |
| Class-compliant USB audio interface | supported | supported (work) | supported (work) | USB-C, or Lightning + USB 3 Camera Adapter (powered). |
| Zoom H4n Pro as interface | supported (Zoom: PC/Mac) | unverified | unverified | Not on Zoom's iOS 26 list. Community: interface mode before launching the app, 44.1 kHz, external power. |
| Multichannel input selection | supported | supported (work) | supported (work) | `setPreferredInputNumberOfChannels` + our `InputRoute`. |
| USB MIDI (class-compliant) | supported (work) | supported (work) | supported (work) | midir 0.11.1 compiles for iOS (CoreMIDI). |
| Bluetooth LE MIDI | supported (work) (Audio MIDI Setup pairing) | supported (work) | supported (work) | iOS needs in-app pairing UI (`CABTMIDICentralViewController`) via a Swift plugin. |
| MIDI timestamps to audio frames | supported (work) | supported (work) | supported (work) | Convert mach ticks with `mach_timebase_info` (ticks ≠ ns on Apple Silicon and iOS). |
| Hardware keyboard shortcuts | supported | supported | supported (rare) | WebKit `keydown`. Some ⌘-chords are reserved by iPadOS. |
| Foot pedals / MIDI controllers for transport | supported (work) | supported (work) | supported (work) | Same MIDI path as above. |

## Files and projects

| Feature | macOS | iPadOS | iPhone | Notes |
| --- | --- | --- | --- | --- |
| Project folder `<Name>.voiceloop/` | supported (work) | supported (work) | supported (work) | iOS: under the app's `Documents`. Root passed in by the shell. |
| Visible in Finder / Files | supported | supported (work) | supported (work) | `UIFileSharingEnabled` + `LSSupportsOpeningDocumentsInPlace`. |
| Shown as a single package document | supported (work) | supported (work) | supported (work) | Exported UTType conforming to `com.apple.package`. PENDING. |
| Open/save dialog for files | supported | supported | supported | Tauri dialog. iOS returns `file://` URIs. |
| Folder picker | supported | platform-limited | platform-limited | Tauri dialog has no iOS folder picker. Needs a Swift `UIDocumentPickerViewController`. |
| Share sheet / AirDrop export | supported (work) | supported (work) | supported (work) | Swift plugin. Prefer a single-file archive. |
| iCloud Drive projects | supported (work) | platform-limited | platform-limited | Placeholders and file coordination. Free team: no iCloud capability. Export target only in v1. |
| Edit in place outside the sandbox | supported | platform-limited | platform-limited | Security-scoped URLs and bookmarks. v1 copies imports in. |

## UI (WKWebView)

| Feature | macOS | iPadOS | iPhone | Notes |
| --- | --- | --- | --- | --- |
| rAF-driven visualisers | supported (60 Hz cap) | platform-limited | platform-limited | 60 Hz max, 30 Hz in Low Power Mode, paused in background. |
| Multitouch (two controls at once) | n/a | supported (work) | supported (work) | Pointer Events per `pointerId`. See TOUCH_UI.md. |
| Pinch-zoom / double-tap-zoom disabled | n/a | supported (work) | supported (work) | Viewport meta + `touch-action`. |
| Split View / Stage Manager sizes | n/a | supported (work) | n/a | Layout must work from about 320 pt wide. |
| Safe areas (notch, home indicator) | n/a | supported (work) | supported (work) | `viewport-fit=cover` + `env(safe-area-inset-*)`. |
| Haptics | n/a | n/a (most iPads lack them) | supported (work) | `tauri-plugin-haptics` (official, mobile). |
