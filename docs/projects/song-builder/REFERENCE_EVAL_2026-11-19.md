> Codex (read-only) research and evaluation after the first on-device test, 2026-11-19. Findings 1–4 and the first fixes it recommends (session mode, single relaxed gate, voiced-kick rule, card gesture, loudness) were applied the same day; the rest are PENDING in [README.md](README.md). Local file links point at the author's checkout.

# Song Builder voice-drum evaluation

## Summary

The first device test is consistent with four identifiable implementation issues:

1. **The timid kick is primarily a voicing problem.** The default Studio kick is a single decaying sine sweep ending at 52 Hz—poorly reproduced by an iPhone speaker—with modest drive and no dedicated upper-bass “body” layer.
2. **The low overall volume is very likely amplified by iOS Measurement mode.** Apple explicitly documents that this mode lowers playback output. Song Builder also applies fixed kit gain and an invisible 0.85 master gain before a limiter that prevents clipping but adds no loudness.
3. **Detection is gated twice and classification defaults to snare.** The engine first demands −45 dBFS plus a 9 dB rise; Swift then demands −42 dBFS plus 10 dB. Anything not clearly low-frequency enough for kick or bright enough for hat is unconditionally called snare.
4. **The menus sit in a hostile gesture hierarchy.** They are inside nested vertical/horizontal `ScrollView`s and a card-wide `onTapGesture`. The nominal 44-point targets are adequate, so gesture competition and SwiftUI `Menu` behavior are more plausible than target size.

The highest-value sequence is: correct the audio-session mode, relax/consolidate hit gating with an adaptive detector, strengthen user-trained recognition, then repair menu interaction and redesign the default kick.

This was a read-only review. No files were changed, and no builds, app launches, microphones, Cargo, or Xcode commands were used.

## References

| Comparable system | Detection/classification and training | Latency handling | Reported limitations |
|---|---|---|---|
| **Vochlea DubBox** | Every trigger is explicitly trained from the user’s vocal sound. It supports up to eight triggers, flags inconsistent training takes, maps them to samples, and offers gain/tone/pitch controls. This is the closest current workflow benchmark. [Product](https://vochlea.com/products/dubbox), [quick start](https://vochlea.com/learn/dubbox-quick-start) | Live triggering, overdub layers, smart quantization, and “voice warping” that preserves the recorded performance while moving it to the grid. | Too new for a meaningful recurring-complaint record. Its requirements emphasize a dynamic microphone, suggesting uncontrolled microphones remain a material variable. |
| **Vochlea Dubler 2** | Users teach each trigger with several short, consistent examples—up to 12—and map it to a MIDI note. [Training guide](https://vochlea.com/learn/how-to-add-triggers-dubler-2) | Recommends buffers of 128 samples or less; clearer percussive attacks improve response. [Latency guide](https://vochlea.com/learn/latency-2) | A hands-on review found third-party microphone calibration produced messier MIDI and noticeable drum latency. Similar syllables such as “ti,” “ts,” and “ta” were confused, and the reviewer recommended only three or four highly distinct triggers. [Engadget review](https://www.engadget.com/dubler-2-can-turn-any-microphone-into-a-midi-controller-090002585.html) |
| **Ableton Convert Drums to MIDI** | Offline transient extraction and kick/snare/hat classification. Users may edit transient markers before conversion; other percussion goes to a silent lane for correction. Ableton explicitly presents beatboxing as supported input. [Live manual](https://cdn-resources.ableton.com/resources/47/af/47af85ea-1e0f-45c4-9938-d90b21969d31/l10manual_en.pdf) | Offline, so it avoids live-monitor latency but cannot provide immediate feedback. | Users describe mixed accuracy, substantial cleanup, bleed-generated false notes, and the result as better for sketching than dependable drum replacement. [Ableton discussion](https://www.reddit.com/r/ableton/comments/1b3oh82/ableton_convert_drums_to_midi_vs_triggers_on/) |
| **Koala Sampler** | No voice classification; it establishes the mobile benchmark for direct sampling, large pads, sequencing, resampling, and immediate editing. There is no calibration step. | Direct pad performance and quantization; Bluetooth and plug-in chains can add substantial latency. Koala’s own help lists Bluetooth delay and Android latency as common problems. [Koala help](https://koalasampler.com/help/) | Reports include AUv3 effect latency, Bluetooth timing that quantization cannot repair, and apparent duplicate monitored hits during sequence recording. [AUv3 report](https://www.reddit.com/r/KoalaSampler/comments/1tm942g/anyone_else_dealing_with_auv3_induced_latency/), [Bluetooth report](https://www.reddit.com/r/KoalaSampler/comments/1qdsuz2/koala_and_a_cheap_bluetooth_controller_super_fun/), [duplicate-hit report](https://www.reddit.com/r/KoalaSampler/comments/nrvhm2/while_recording_in_sequence_mode_with_metronome/) |
| **GarageBand Live Loops / Touch Drums** | No beatbox classification. It supplies templates, synchronized cells, touch instruments, drum kits, merge recording, and editable recorded regions—useful benchmarks for “instant result” UX. [GarageBand iPhone guide](https://help.apple.com/pdf/garagebandiphone/en_US/garageband-iphone-user-guide.pdf) | Cells launch in sync and performances are recorded at their musical start positions. | An iPhone report describes roughly two-second touch-response delays in Live Loops, including delayed record response. This is an isolated report rather than evidence of a universal defect. [User report](https://www.reddit.com/r/GarageBand/comments/1ayaumk/touch_response_delay/) |
| **LVT and AVP research** | LVT uses onset segmentation, MFCCs, centroid/spread/slope/rolloff, zero crossings, per-user feature selection, and k-nearest-neighbor classification. Its five-per-class training flow closely resembles Pocket’s. In its evaluation, user-specific LVT substantially outperformed Ableton for vocalized kick, snare, and hat. [LVT paper](https://arxiv.org/abs/1811.02406) The AVP dataset contains 9,780 labeled utterances from 28 amateur participants. [AVP paper](https://arxiv.org/abs/2009.11737) | Research found that delaying classification analysis roughly 10–23 ms after onset improves beatbox class separation. [Stowell thesis](https://qmro.qmul.ac.uk/xmlui/bitstream/handle/123456789/412/STOWELLMakingMusic2010.pdf?isAllowed=y&sequence=1) | Cross-user generalization remains difficult because amateurs choose different, speech-like syllables. User-specific models are consistently better; later work favors MFCC/spectrogram embeddings and syllable-aware training. [Deep-embedding study](https://arxiv.org/abs/2204.04646) |

The LVT research uses Aubio for onset detection. Aubio is GPL-licensed, so it must remain **behavior-only reference material**; do not copy or translate its source. The same repository restriction applies to any GPL/AGPL reference implementation.

## Techniques

### Robust onsets across varied loudness

- Maintain a slowly adapting room-noise estimate and trigger from `noiseFloor + margin`, with an absolute safety floor. A fixed −45 dBFS cutoff cannot provide a genuinely wide hearing range across microphones, distances, rooms, and iOS processing modes.
- Combine energy rise with positive spectral flux or high-frequency content. Flux detects timbral attacks that do not create a 9–10 dB RMS jump. A causal spectral-flux detector is suitable for realtime use; SuperFlux demonstrates that adaptive spectral comparison can reduce false onsets. [DAFx paper](https://www.dafx.de/paper-archive/details/0oee-99Z88WL7pSo749gcA)
- Use separate attack and release thresholds plus the existing refractory interval. This allows a low attack threshold without repeated triggers from one vowel tail.
- Normalize each detected hit for classification, preferably against its local pre-onset noise and its own peak/RMS. Do not make classification depend strongly on absolute microphone gain.
- Treat AGC carefully: system or DSP AGC can improve reach, but time-varying gain changes the rise envelope used by the detector. Local per-hit normalization is more predictable than aggressive continuous AGC.

### Separating “bah/boots” from “K” and “ts”

A voiced kick such as “bah” or “boots” has a short plosive followed by a voiced vowel. Its first few milliseconds may look broadband like a snare, while the lower-frequency/periodic body develops later.

Useful additions are:

- MFCCs or a compact log-mel vector.
- Spectral centroid, rolloff, low-energy percentile, and flux over multiple short frames rather than one aggregate.
- Pitch/voicing confidence or autocorrelation strength during the 20–100 ms after onset.
- Attack-to-body energy ratio, low-band growth, decay, and duration.
- A classification window beginning about 10–20 ms after the detected onset, or separate transient and body windows. Research specifically found this delay improves beatbox classification.
- Feature standardization before distance calculation, plus per-class variance. Raw Euclidean distance gives every current feature equal nominal authority even when their real variance differs.

### Few-shot calibration

The existing five examples per class are a credible starting point, but calibration should also:

- Capture the room noise and normal speaking distance first.
- Ask for both softer and louder versions of each syllable.
- Reject or repeat inconsistent examples rather than averaging them into the class.
- Show the live confusion result during training.
- Retain individual examples or class mean plus variance, not only one running mean.
- Allow a distance/confidence rejection result—“uncertain”—instead of forcing every sound into a drum class.

### iOS duplex audio

Pocket correctly uses `playAndRecord` and requests `defaultToSpeaker`, so receiver/earpiece routing is not the leading code-level explanation for low volume.

The more important issue is Measurement mode. Apple says it minimizes system processing but also disables dynamics processing in a way that produces **lower playback output**. [Apple Measurement mode](https://developer.apple.com/documentation/avfaudio/avaudiosession/mode-swift.struct/measurement)

Recommended session strategies:

- **Headphones/music capture:** Measurement mode, no system AGC, clean input.
- **Built-in speaker:** Default mode plus `setPrefersEchoCancelledInput(true)` where supported. Apple describes this newer option as wide-band echo cancellation for simultaneous built-in mic/speaker use. [Apple echo-cancelled input](https://developer.apple.com/documentation/avfaudio/avaudiosession/setprefersechocancelledinput(_:)?changes=__6)
- **Fallback:** Voice-processing I/O can provide AEC/AGC, but it is speech-tuned and may alter kick/hat timbre. It should be an explicit alternative mode and tested against classification accuracy.

The current `defaultToSpeaker` option is appropriate for avoiding receiver routing. [Apple routing option](https://developer.apple.com/documentation/avfaudio/avaudiosession/categoryoptions-swift.struct/defaulttospeaker)

### Kick weight and loudness

For an iPhone speaker, “weight” cannot rely mainly on 45–55 Hz sub energy. A convincing small-speaker kick needs:

- A fast downward pitch envelope for impact.
- A body component around roughly 80–140 Hz.
- Harmonic saturation so the fundamental is implied on small speakers.
- A controlled transient/click.
- Compression or transient shaping followed by a true output limiter and makeup gain.
- Loudness matching between kits, preferably measured over representative patterns rather than normalized by single-sample peak.

### SwiftUI menus

`Menu` already has internal gesture machinery, and controls inside `ScrollView` have known responsiveness regressions and gesture competition. Reports exist for buttons requiring long or repeated presses inside scroll views and for unstable `Menu` interaction on recent iOS versions. [ScrollView report](https://www.reddit.com/r/SwiftUI/comments/1hjgvd4/button_not_as_responsive_when_inside_scrollview/), [Menu report](https://www.reddit.com/r/SwiftUI/comments/1ouo26q/are_menus_just_broken_in_ios_26/)

Prefer ordinary `Button`, `Picker`, `popover`, or `confirmationDialog` controls for frequent actions. Keep menu labels free of ancestor tap/drag gestures.

## Diagnosis of finding 1: timid kick

**High-confidence contributors:**

- The default kit is Studio: [PocketSettings.swift:53](../../../apps/pocket-ios/Sources/SongBuilder/PocketSettings.swift:53).
- Its kick is a single sweep from 140 Hz to 52 Hz, 320 ms decay, click 0.35, and only 0.15 drive: [song.rs:243](../../../crates/voiceloop-engine/src/song.rs:243).
- The 808 reaches 45 Hz and lasts longer, but that deep fundamental is also poorly served by a phone speaker: [song.rs:255](../../../crates/voiceloop-engine/src/song.rs:255).
- Synthesis is one sine oscillator plus filtered noise click; there is no separate upper-bass body, sub layer, transient shaper, EQ, or kick-specific compressor: [song.rs:512](../../../crates/voiceloop-engine/src/song.rs:512), [song.rs:585](../../../crates/voiceloop-engine/src/song.rs:585).
- The drive stage adds harmonics but normalizes against its saturated peak, so it changes tone more than sustained loudness: [song.rs:589](../../../crates/voiceloop-engine/src/song.rs:589).

**Conclusion:** the default kick is synthetically valid but optimized around low fundamental pitch rather than perceived weight on a small speaker. The Studio kit’s relatively short decay and light drive reinforce the timid result.

## Diagnosis of finding 2: all sounds very quiet

**Primary cause:**

- Every engine start requests Measurement mode: [platform/mod.rs:55](../../../crates/voiceloop-engine/src/platform/mod.rs:55).
- iOS applies it to the `playAndRecord` session: [ios.rs:56](../../../crates/voiceloop-engine/src/platform/ios.rs:56).
- Apple explicitly warns that Measurement mode lowers output playback level.

**Additional gain reductions:**

- Song master defaults to 0.85: [PocketSettings.swift:59](../../../apps/pocket-ios/Sources/SongBuilder/PocketSettings.swift:59).
- That master value is sent to the engine independently of the part slider: [SongStore.swift:210](../../../apps/pocket-ios/Sources/SongBuilder/SongStore.swift:210).
- Drum kits multiply velocity by another 0.85–0.95: [song.rs:243](../../../crates/voiceloop-engine/src/song.rs:243).
- The final stage is only a soft peak limiter; it supplies no makeup gain or loudness normalization: [song.rs:935](../../../crates/voiceloop-engine/src/song.rs:935), [processor.rs:754](../../../crates/voiceloop-engine/src/looper/processor.rs:754).
- The settings sheet exposes neither master nor click volume, despite both existing in the registry: [SongSettingsSheet.swift:45](../../../apps/pocket-ios/Sources/Views/SongSettingsSheet.swift:45).

**Not the leading explanation:** Pocket does request `DefaultToSpeaker`: [ios.rs:63](../../../crates/voiceloop-engine/src/platform/ios.rs:63). Route verification is still worthwhile, but the code does not simply omit speaker routing.

## Diagnosis of finding 3: loud input required, missed hits, mostly snare

### Missed hits

A hit must pass two successive gates:

1. Engine onset detector: −45 dBFS, 9 dB rise, with RMS measured over up to 20 ms: [onset.rs:37](../../../crates/voiceloop-analysis/src/onset.rs:37), [onset.rs:51](../../../crates/voiceloop-analysis/src/onset.rs:51), [onset.rs:129](../../../crates/voiceloop-analysis/src/onset.rs:129).
2. Swift gate: −42 dBFS and 10 dB rise: [PocketSettings.swift:74](../../../apps/pocket-ios/Sources/SongBuilder/PocketSettings.swift:74), [VoiceDrums.swift:68](../../../apps/pocket-ios/Sources/SongBuilder/VoiceDrums.swift:68).

`SongStore` only sends `enabled`; therefore the engine uses its generic onset defaults rather than Song Builder-specific settings: [SongStore.swift:328](../../../apps/pocket-ios/Sources/SongBuilder/SongStore.swift:328), [analysis.rs:55](../../../crates/voiceloop-engine/src/analysis.rs:55).

A hit rejected by the engine can never be recovered by Swift. The second stricter gate then rejects some events the engine did emit. This directly explains “must be very loud.”

The detector itself acknowledges that spectral flux is the planned path for softer onsets: [onset.rs:18](../../../crates/voiceloop-analysis/src/onset.rs:18).

### Snare bias

The rule tree is:

```text
low ≥ 0.50 OR centroid < 450 Hz  → kick
high ≥ 0.45 OR centroid > 3200 Hz → hat
everything else                   → snare
```

See [VoiceDrums.swift:29](../../../apps/pocket-ios/Sources/SongBuilder/VoiceDrums.swift:29) and [PocketSettings.swift:91](../../../apps/pocket-ios/Sources/SongBuilder/PocketSettings.swift:91).

“Bah” and “boots” include plosive/fricative energy followed by a voiced vowel. They can easily have less than 50% energy below 250 Hz and a centroid above 450 Hz. Unless they also cross the very high hat threshold, they fall into the unconditional snare branch.

The current 40 ms descriptor has only band shares, one centroid, zero crossings, and half-window decay: [percussion.rs:35](../../../crates/voiceloop-analysis/src/percussion.rs:35), [percussion.rs:188](../../../crates/voiceloop-analysis/src/percussion.rs:188). It lacks MFCCs, voicing/pitch, and transient-versus-vowel evolution.

Training is better aligned with the problem but stores only one unweighted mean vector per class and uses raw Euclidean distance with no variance, standardization, confidence, or rejection: [VoiceDrums.swift:23](../../../apps/pocket-ios/Sources/SongBuilder/VoiceDrums.swift:23), [VoiceDrums.swift:35](../../../apps/pocket-ios/Sources/SongBuilder/VoiceDrums.swift:35).

## Diagnosis of finding 4: dropdown menus require repeated taps

The labels already request 44-point minimum height, so the basic hit target is reasonable: [Theme.swift:178](../../../apps/pocket-ios/Sources/Views/Theme.swift:178).

The likely conflicts are structural:

- The whole part card has a rounded `contentShape` and `onTapGesture`, including the selected card containing its menus: [PartCard.swift:41](../../../apps/pocket-ios/Sources/Views/PartCard.swift:41).
- Menus are embedded in a horizontal `ScrollView`: [PartCard.swift:110](../../../apps/pocket-ios/Sources/Views/PartCard.swift:110), [PartCard.swift:239](../../../apps/pocket-ios/Sources/Views/PartCard.swift:239).
- That horizontal scroll is itself inside the screen’s vertical `ScrollView`: [SongBuilderView.swift:36](../../../apps/pocket-ios/Sources/Views/SongBuilderView.swift:36), [Theme.swift:78](../../../apps/pocket-ios/Sources/Views/Theme.swift:78).
- `PocketMenu` is only a thin wrapper over native SwiftUI `Menu`; it adds no explicit gesture isolation: [Theme.swift:288](../../../apps/pocket-ios/Sources/Views/Theme.swift:288).

The card tap guard prevents selection work after recognition, but it does not remove the ancestor recognizer. A small finger movement can also be interpreted by either scroll view rather than the menu.

## Ranked fixes

1. **Change the built-in-speaker audio-session policy.**  
   Use Default mode for speaker playback and request echo-cancelled input where supported; retain Measurement mode for headphones or an explicit clean-capture mode. Record the selected policy as a global setting.  
   **Expected effect:** major output-volume improvement; lower speaker-bleed retrigger risk on supported phones.  
   **Size:** small-to-medium. Validate route, output level, and classifier behavior on multiple iPhones.

2. **Remove the double fixed gate and introduce a Song Builder onset configuration.**  
   As an immediate tuning pass, use one gate around −55 dBFS/6 dB rise and keep the 60 ms refractory interval. Preferably make the threshold relative to a rolling noise floor, then add spectral-flux confirmation. Pass the entire onset configuration instead of only `enabled`.  
   **Expected effect:** much wider usable distance and substantially fewer missed soft `ts`/`bah` hits.  
   **Risk:** speaker bleed and room noise; test concurrently with the audio-session change.

3. **Make user training the primary first-run path.**  
   Train five to eight examples of the owner’s actual “Bah,” “K,” and “ts,” including quiet and loud versions. Preserve examples or mean plus variance, standardize features, reject outliers, and show confusion feedback. Avoid silently falling back to rules when a trained class is incomplete.  
   **Expected effect:** largest near-term reduction in “everything is snare.”  
   **Size:** small for UX/default changes; medium for variance/confidence handling.

4. **Improve the handcrafted classifier for voiced kicks.**  
   Add transient/body features, pitch or voicing confidence, MFCC/log-mel descriptors, and a delayed 10–20 ms analysis frame. At minimum, add an “uncertain” result instead of making snare the universal fallback.  
   **Expected effect:** better `bah/boots` recognition even before training and clearer debugging.  
   **Size:** medium. Any new window, threshold, or mode belongs in `PocketSettings` and `SETTINGS_INDEX`.

5. **Repair menu interaction before styling changes.**  
   Remove the card-wide tap recognizer when selected; make only the collapsed header select the card. Replace frequently used kit/FX/grid/bars menus with explicit popovers, segmented pickers, or sheets. Avoid placing menu triggers in a nested horizontal scroll where possible.  
   **Expected effect:** one-tap controls and fewer OS-version-specific failures.  
   **Size:** small.

6. **Raise perceived output loudness with measurement, not arbitrary gain.**  
   Expose master volume, default it closer to unity, loudness-match kits, and add a song-bus compressor/limiter with makeup gain. Test representative loops for peak and integrated loudness.  
   **Expected effect:** consistently louder playback without uncontrolled clipping.  
   **Size:** small-to-medium.

7. **Redesign the default Studio kick for phone translation.**  
   Add a short 80–140 Hz body/resonance, slightly longer body decay, stronger controlled saturation, and a separately shaped transient. Keep the sub tail but do not depend on it for weight. Consider a “Phone” or “Punch” kit variant rather than replacing every existing behavior.  
   **Expected effect:** heavier perceived kick on the iPhone speaker.  
   **Size:** medium.

8. **PENDING—build a recorded evaluation corpus and confusion harness.**  
   Capture consenting, labeled examples across users, syllables, distances, speaker/headphone modes, and iPhone models; score onset precision/recall, class confusion, latency, and bleed retriggers. The public AVP/LVT data are useful research baselines but do not represent Pocket’s exact microphone/session path.  
   **Size:** larger, but necessary before claiming robust recognition.

## Gaps vs comparable tools

- Pocket has training, but lacks DubBox’s inconsistent-take detection, multiple profiles, confidence feedback, and sample shaping.
- It has no user-visible input sensitivity, noise-floor calibration, detected-onset indicator, or “too quiet / too similar” coaching.
- It forces every accepted hit into a class; stronger tools and research workflows support correction, rejection, or retraining.
- It stores only hit events. DubBox can retain/export the original voice, drums, or both, which makes recognition mistakes recoverable.
- It has synthesized kits only and no per-hit gain, pitch, tone, filter, choke group, or imported sample.
- The reversible Tightness model and engine-clock placement are strong relative to the references, but there is no recorded-audio fallback if transcription is wrong.
- Speaker/no-headphone operation is not yet treated as a distinct acoustic mode despite the materially different output level, AGC, and echo-cancellation tradeoffs.
- Current verification is synthetic and snapshot-based. No real-voice confusion matrix, device loudness target, speaker-bleed test, or native touch-interaction matrix exists yet.