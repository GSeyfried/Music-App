> Codex (read-only) research, 2026-11-19, for D-021. Verdict: keep the Rust engine; make iOS audio I/O and Apple features a thin native host. Local file links point at the author's checkout.

# VoiceLoop Pocket: Rust engine vs Swift audio engine

## Summary verdict

There is no fundamental iOS limitation that requires VoiceLoop’s DSP, looper, analysis, timing, or project engine to be rewritten in Swift. Rust produces native ARM64 code, can call Core Audio’s C APIs directly, and can use Accelerate/vDSP through its C ABI. A Swift rewrite would not inherently improve DSP speed or realtime safety; Apple explicitly warns that realtime callbacks must avoid allocations, locks, I/O, and interaction with the Swift/Objective-C runtimes. Apple’s own AUv3 sample uses Swift for the framework shell and C++ for its realtime DSP kernel. [Apple realtime guidance](https://developer.apple.com/library/archive/qa/qa1715/_index.html), [Apple AUv3 sample](https://developer.apple.com/documentation/avfaudio/creating-custom-audio-effects).

The real limit is narrower: CPAL 0.18.2’s iOS abstraction does not expose a single full-duplex RemoteIO/VoiceProcessingIO unit, its underlying AudioUnit, Audio Workgroups, or Apple’s audio graph/plugin ecosystem. VoiceLoop currently creates separate CPAL input and output RemoteIO units and repairs their relationship with a ring-buffer/drift bridge. That can work, but it adds latency and complexity and is materially less proven than the standard iOS single-I/O-unit design.

Recommendation: keep the shared Rust engine, DSP, analysis, transport, and storage. Make iOS audio I/O and Apple feature integration a thin native platform host. For the near-term looper, use one duplex RemoteIO Audio Unit whose render callback invokes a narrow, allocation-free Rust processing ABI. Use VoiceProcessingIO only as an optional voice/echo-cancelled mode. If AUv3 hosting becomes important, use AVAudioEngine/AUAudioUnit as the native graph and embed the Rust processor as a source/sink/custom-AU node. Do not move the core engine to Swift.

## Capability assessment

Legend: **Rust OK** means technically and practically appropriate in Rust; **native glue** means the Rust engine remains, but a Swift/Objective-C/C platform adapter is preferable; **Swift-only** means the feature depends on Swift declarations, compiler metadata, SwiftUI, or a Swift extension target.

| Capability | Status | Notes |
|---|---|---|
| Looper, transport, synthesis, effects, pitch/onset analysis | **Rust OK** | Already shared with macOS and future Windows/Android. Rust and Swift both compile through LLVM; algorithms, memory access, vectorization, and callback discipline matter more than language. |
| Accelerate/vDSP | **Rust OK** | vDSP has a standard C API and optimized FFT, convolution, biquads, and vector operations, so Rust can link it without moving DSP to Swift. [vDSP API](https://developer.apple.com/library/archive/documentation/Performance/Conceptual/vDSP_Programming_Guide/About_vDSP/About_vDSP.html) |
| Current CPAL output or capture separately | **Rust OK, device proof required** | CPAL 0.18.2 creates RemoteIO Audio Units and has substantially improved timestamps, route errors, xruns, latency, and interruption recovery. [CPAL 0.18 changelog](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md) |
| Low-latency full duplex | **Native glue recommended** | CPAL 0.18.2 builds a separate RemoteIO unit per stream; its input unit explicitly disables output. A single RemoteIO unit can pull input with `AudioUnitRender` and produce output in one hardware callback. CPAL’s unreleased duplex API does not yet list an iOS implementation. [CPAL iOS source](https://github.com/RustAudio/cpal/blob/v0.18.2/src/host/coreaudio/ios/mod.rs) |
| Voice processing/AEC | **Native glue** | `kAudioUnitSubType_VoiceProcessingIO` provides Apple voice processing. `setPrefersEchoCancelledInput` is music-friendlier but limited to certain 2024+ iPhones, default mode, and supported built-in routes. [VoiceProcessingIO](https://developer.apple.com/documentation/audiotoolbox/1584139-input_output_audio_unit_subtypes), [echo-cancelled input](https://developer.apple.com/documentation/avfaudio/avaudiosession/setprefersechocancelledinput%28_%3A%29?changes=latest_minor&language=objc) |
| AVAudioSession, permission, interruptions, routes | **Rust OK technically; native glue is easier to maintain** | The existing objc2 implementation is valid. Apple treats sample rate and buffer duration as preferences; actual values must be read after activation. [QA1631](https://developer.apple.com/library/archive/qa/qa1631/_index.html) |
| Background audio | **Native glue/configuration** | Requires an appropriate audio session plus `UIBackgroundModes=audio`; it is an app lifecycle/entitlement concern, not a DSP-language restriction. [Background execution modes](https://developer.apple.com/documentation/xcode/configuring-background-execution-modes) |
| Now Playing and lock-screen/remote commands | **Native glue** | `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter` should translate native commands into Rust control messages. [Now Playing](https://developer.apple.com/documentation/mediaplayer/mpnowplayinginfocenter), [remote commands](https://developer.apple.com/documentation/MediaPlayer/MPRemoteCommandCenter) |
| Built-in EQ, reverb, delay, distortion, time/pitch | **Native graph/glue** | Apple exposes these as AVAudioUnit nodes. Rust may control lower-level Audio Units, but AVAudioEngine is the practical host. Note that `AVAudioUnitTimePitch` is a non-realtime time effect. [Audio Unit effects](https://developer.apple.com/documentation/avfaudio/audio-units), [TimePitch](https://developer.apple.com/documentation/avfaudio/avaudiounittimepitch) |
| Host third-party AUv3 effects/instruments | **Native graph/glue** | Discovery, asynchronous instantiation, presets, and custom view controllers are Apple object/UI APIs. The Rust core can feed and consume the graph. iOS AUv3 runs out-of-process. [Apple AU host sample](https://developer.apple.com/documentation/audiotoolbox/incorporating-audio-effects-and-instruments) |
| Ship VoiceLoop as an AUv3 plug-in | **Native AUv3 shell + Rust DSP** | Requires an app-extension target, `AUAudioUnit` subclass/factory, bus and parameter definitions, and optional native UI. Rust can be the render kernel—the same pattern Apple demonstrates with C++. |
| Inter-App Audio | **Do not build** | Deprecated since iOS 13 in favor of AUv3. [Deprecated Audio Toolbox symbols](https://developer.apple.com/documentation/audiotoolbox/deprecated-symbols) |
| Audiobus/AUM interoperability | **AUv3 route** | Being a well-behaved AUv3 instrument/effect is the durable route. Supporting proprietary Audiobus SDK features would require its native SDK. |
| CoreMIDI/USB MIDI | **Rust OK** | `midir` uses CoreMIDI and already compiles in this repo. |
| BLE MIDI discovery/pairing | **Native glue** | Once connected, CoreMIDI endpoints are usable from Rust. Pairing/discovery needs Core Bluetooth/CoreMIDI or CoreAudioKit UI. Modern CoreMIDI can automatically reconnect paired BLE devices. [MIDI Bluetooth](https://developer.apple.com/documentation/coremidi/midi-bluetooth?language=objc) |
| Audio Workgroups | **Native I/O or direct Core Audio access** | To join auxiliary realtime threads, the app must obtain the I/O Audio Unit’s workgroup. CPAL does not expose its AudioUnit; a custom RemoteIO host can. [Audio Workgroups](https://developer.apple.com/documentation/audiotoolbox/adding-parallel-real-time-threads-to-audio-workgroups) |
| Spatial audio/head tracking | **Native glue** | AVAudioEnvironmentNode, PHASE, or the C Audio Toolbox spatial mixer can host Rust-generated audio. Head tracking/personalization also needs entitlements. [Spatial audio](https://developer.apple.com/documentation/phase/personalizing-spatial-audio-in-your-app) |
| App Intents/Siri/Shortcuts | **Swift-only declaration layer** | App Intent types are compiler-discovered Swift declarations. Their `perform()` implementations should call the Rust control API. [App Intents](https://developer.apple.com/documentation/appintents) |
| Live Activities | **Swift-only UI/extension layer** | ActivityKit plus WidgetKit/SwiftUI; the engine supplies state summaries only. [Live Activities](https://developer.apple.com/documentation/activitykit/displaying-live-data-with-live-activities) |
| CarPlay | **Native shell; poor fit for recording** | Requires a CarPlay entitlement and native templates. Apple says recording generally isn’t supported while CarPlay is active, so Pocket should disable mic workflows there. [CarPlay guide](https://developer.apple.com/download/files/CarPlay-Developer-Guide.pdf) |
| ShazamKit, SoundAnalysis, Core ML/Neural Engine, Speech | **Native glue** | Feed copied/downsampled buffers from the Rust capture path on a non-realtime thread. Core ML can select the Neural Engine; SoundAnalysis supports live/custom classifiers. [SoundAnalysis](https://developer.apple.com/documentation/soundanalysis), [Core ML compute units](https://developer.apple.com/documentation/coreml/mlcomputeunits?language=objc), [ShazamKit](https://developer.apple.com/documentation/ShazamKit?language=objc), [Speech](https://developer.apple.com/documentation/speech/) |

## Realtime I/O and latency

The current path is:

```text
CPAL input RemoteIO callback
    → mono rtrb
    → priming/drift-resampling bridge
    → independent CPAL output RemoteIO callback
    → Rust looper/DSP
```

The separate clocks and bridge are explicit in [looper/io.rs](../../../crates/voiceloop-engine/src/looper/io.rs:1), while output owns the musical clock and processes the Rust engine in its callback at [looper/io.rs](../../../crates/voiceloop-engine/src/looper/io.rs:394). Input is another CPAL stream at [capture.rs](../../../crates/voiceloop-engine/src/capture.rs:225).

The default iOS request is 256 frames at 48 kHz—about **5.33 ms per hardware quantum**, not 5.33 ms round trip—at [platform/mod.rs](../../../crates/voiceloop-engine/src/platform/mod.rs:55). Round-trip monitoring also includes input/output hardware latency, at least one callback in each direction, and VoiceLoop’s bridge cushion. Apple’s preferences may be refused or changed after route activation, and Bluetooth/AirPlay can be much slower.

A realistic first device matrix is 128 and 256 frames on built-in speaker, wired/USB, and headphones. CPAL advertises 256–4096 frames in its iOS backend even though mature native implementations such as JUCE probe as low as 64 on modern iOS. VoiceLoop bypasses CPAL’s fixed-buffer request and asks AVAudioSession directly, so lower sizes may work, but only measurements can establish that.

Bluetooth requires explicit policy:

- A2DP is high-quality stereo **output-only**.
- HFP supplies duplex input/output and automatically ties both directions to the headset route.
- VoiceLoop currently enables A2DP but not HFP, so a Bluetooth microphone is not enabled.
- Newer high-quality Bluetooth recording can increase input latency and is not recommended for realtime communication. [Apple Bluetooth routing](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/allowbluetootha2dp), [high-quality recording](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/bluetoothhighqualityrecording?changes=lat_7_5&language=objc)

## Risks in the current implementation

1. **No true duplex I/O unit.** [hardware.rs](../../../crates/voiceloop-engine/src/hardware.rs:249) opens an independent input stream and attaches it to the output through a ring. This is the largest latency/stability risk for Pocket.

2. **Bluetooth input is missing.** [ios.rs](../../../crates/voiceloop-engine/src/platform/ios.rs:63) enables only `AllowBluetoothA2DP`; HFP/high-quality-recording options and explicit route policy are absent.

3. **No input selection.** The snapshot exposes only the active route at [ios.rs](../../../crates/voiceloop-engine/src/platform/ios.rs:94). It does not enumerate `availableInputs`, call `setPreferredInput`, or expose the system input picker.

4. **Route/interruption handling may overlap CPAL behavior.** VoiceLoop restarts on route notifications at [hardware.rs](../../../crates/voiceloop-engine/src/hardware.rs:541), while CPAL 0.18.2 also updates latency, resumes after interruptions, and reports invalidation/device-change errors. Device testing must rule out double restart, duplicated events, and stale callbacks.

5. **Future interruption API migration.** [ios.rs](../../../crates/voiceloop-engine/src/platform/ios.rs:153) uses the traditional interruption notification. It is correct for iOS 17–26, but Apple recommends new lifecycle notifications beginning with iOS 27. [Apple interruption guidance](https://developer.apple.com/documentation/avfaudio/handling-audio-interruptions)

6. **No echo-cancellation mode.** The current measurement/default choice changes mic processing, but neither VoiceProcessingIO nor music-oriented echo-cancelled input is implemented.

7. **No background/Now Playing configuration.** [project.yml](../../../apps/pocket-ios/project.yml:35) has no audio background mode, MediaPlayer integration, widget extension, App Intents target, or AUv3 target.

8. **FFI is suitable for control, not rendering.** JSON allocation occurs only on the control thread, which is safe, but it should never be used inside an audio callback. The event bridge allocates/serializes at [voiceloop-ffi/src/lib.rs](../../../crates/voiceloop-ffi/src/lib.rs:37). A duplex host needs a separate raw-pointer render ABI.

9. **FFI panic containment is absent.** Exported C functions at [voiceloop-ffi/src/lib.rs](../../../crates/voiceloop-ffi/src/lib.rs:39) do not catch panics, and release configuration does not specify `panic=abort`. No Rust panic may unwind across C.

10. **Swift concurrency is only partially hardened.** [EngineBridge.swift](../../../apps/pocket-ios/Sources/Engine/EngineBridge.swift:74) uses `@unchecked Sendable`, while [project.yml](../../../apps/pocket-ios/project.yml:12) remains in Swift 5 mode. Swift 6’s `Sendable` checks help find control-plane races but do not make render code realtime-safe. Apple still requires no ARC/runtime interaction, allocations, or locks in the callback. [Swift realtime callback guidance](https://developer.apple.com/documentation/realitykit/audio/generatorrenderhandler)

## Recommended architecture boundary

```text
SwiftUI / App Intents / ActivityKit / MediaPlayer / document UI
                            │ typed control/events
                            ▼
                  Pocket native platform host
     AVAudioSession + routes + interruptions + background policy
          RemoteIO / VoiceProcessingIO or AVAudioEngine graph
                            │ raw preallocated audio buffers
                            ▼
         Rust realtime processor: looper, transport, DSP, synth
                            │ lock-free summaries/jobs
                            ▼
       Rust analysis, project model, persistence, offline render
```

Keep in Rust:

- all existing looper/transport/timing code;
- DSP, instruments, metering, pitch/onset/rhythm analysis;
- immutable project model and offline rendering;
- lock-free command/event structures;
- desktop CPAL/Core Audio and future WASAPI/Android backends.

Put in the native iOS layer:

- AVAudioSession policy, preferred input, route picker, interruptions, media reset;
- one RemoteIO/VoiceProcessingIO instance, or an AVAudioEngine graph;
- background mode, Now Playing, remote commands, App Intents, Live Activities;
- AUv3 hosting/plugin shell and plugin view controllers;
- Core ML/SoundAnalysis/Shazam/Speech adapters;
- entitlements, CarPlay, spatial-audio and BLE-MIDI UI.

Swift itself should not become the realtime DSP layer. Apple’s own architecture and AudioKit’s shipping ecosystem use Swift APIs over C/C++ realtime cores; AudioKit explicitly separates its Swift layer from C/C++ DSP. [AudioKit architecture](https://github.com/AudioKit/AudioKit/blob/main/Sources/AudioKit/AudioKit.docc/AudioKit.md), [AudioKit C-backed DSP](https://github.com/AudioKit/AudioKitEX). Rust/Swift bridging is also established outside audio through production-quality generated C bindings such as Mozilla UniFFI. [UniFFI Swift bindings](https://mozilla.github.io/uniffi-rs/next/swift/overview.html)

There is no App Store rule against Rust or statically linked native code. Rust’s ARM64 iOS target is officially distributed as a Tier 2 target and supports iOS 10+, while this app already targets iOS 17. [Rust iOS platform support](https://doc.rust-lang.org/stable/rustc/platform-support/apple-ios.html). Bitcode is no longer a concern; Xcode 14 deprecated it and Xcode 15 removed support. [Xcode 14 release notes](https://developer.apple.com/documentation/xcode-release-notes/xcode-14-release-notes), [Xcode 15 release notes](https://developer.apple.com/documentation/xcode-release-notes/xcode-15-release-notes)

The practical costs are longer dual-toolchain builds, weaker source-level Xcode navigation, and extra symbol packaging. Instruments can profile native Rust frames when DWARF symbols are preserved, but archives must retain the matching Rust symbols/dSYM for crash symbolication. App size should be measured after release LTO/dead stripping; the current release profile has no size/LTO/panic policy, so debug-library size is not meaningful.

## Concrete next steps

1. Add an on-device audio characterization harness—not a feature rewrite—and record actual sample rate, granted quantum, callback sizes, input/output latency, bridge cushion, xruns, and round-trip latency for built-in, wired/USB, HFP, and A2DP routes.

2. Test 128 and 256-frame requests at 48 kHz. Set acceptance thresholds before judging CPAL: no dropouts over a long run, reliable interruption/route recovery, stable event placement, and acceptable monitored round trip.

3. Prototype a single duplex RemoteIO adapter with a minimal C render ABI into the existing Rust processor. Compare it directly with CPAL’s two-stream path. This isolates whether CPAL—not Rust—is the limiting factor.

4. Add explicit session policies: music capture, speaker monitoring with supported echo cancellation, VoiceProcessingIO, Bluetooth HFP, A2DP output, and high-quality Bluetooth recording where available. These should be settings because they change sound and latency.

5. Add preferred-input enumeration/selection, route details, and privacy-safe headphone-disconnect behavior.

6. Harden the C boundary: panic containment/abort policy, opaque handle lifetime tests, typed version negotiation, and a non-JSON realtime ABI. Keep JSON for low-frequency commands until profiling justifies replacing it.

7. Enable Swift 6 strict-concurrency checking for the shell and remove avoidable `@unchecked Sendable` usage. Keep all Swift/Foundation work outside render callbacks.

8. Add background audio and Now Playing only after foreground interruption/route tests pass. Add App Intents and Live Activities as Swift adapters calling the existing Rust commands.

9. Defer AUv3 hosting until it is a product requirement. If adopted, host the graph natively and reuse Rust DSP; do not use AUv3 as a reason to rewrite the engine.

10. Preserve D-021. Revisit only if a measured single-RemoteIO Rust processor cannot meet latency/stability targets or if the product becomes primarily an Apple-only AU host. Neither condition is currently established.

The evidence therefore supports a **hybrid native-host/shared-Rust-core architecture**, not an all-Swift audio rewrite.