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
| `voiceloop-engine` compiles | supported | supported (compiles) | supported (compiles) | Passes with CPAL 0.16, tests included (re-checked 2026-11-17). Linking needs the iOS SDK (Xcode). |
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
| Device enumeration | supported (CPAL names) | supported (work) | supported (work) | CPAL exposes a single "default" device. Use `AVAudioSession.availableInputs`. |
| Input selection (built-in vs USB vs headset) | supported | supported (work) | supported (work) | `setPreferredInput(port)`. |
| Output device selection | supported | platform-limited | platform-limited | iOS routes output itself (the user picks via Control Center / AirPlay). Apps can only override to the speaker. |
| Sample rate control | supported (device) | supported (work) | supported (work) | `setPreferredSampleRate` is a request; external devices may force 44.1 kHz. |
| Buffer size control | platform-limited (CPAL default) | supported (work) | supported (work) | `setPreferredIOBufferDuration`. Avoid CPAL `Fixed` (minimum 256 in 0.18). |
| Small buffers (64-128 frames) | unverified | unverified | unverified | Apple: ≥ 5 ms guaranteed, lower "depending on hardware". |
| Reported latency | platform-limited | supported (work) | supported (work) | `inputLatency`/`outputLatency` and CPAL 0.18 timestamps. Still calibrate (D-015). |
| Interruptions (calls, Siri, alarms) | n/a | supported (work) | supported (work) | CPAL 0.18.2 auto-resumes. We must stop takes and pause the transport. |
| Route changes (headphones, USB) | supported (work) | supported (work) | supported (work) | Engine must restart streams **without losing loops** (README §5, item 1). |
| Media services reset | n/a | supported (work) | supported (work) | Reconfigure the session and restart. Test via Settings › Developer › Reset Media Services. |
| Mic permission prompt and status | supported | supported (work) | supported (work) | `NSMicrophoneUsageDescription` present (Info.plist / Info.ios.plist). Denied means silence, not an error. |
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
