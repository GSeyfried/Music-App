> Codex (read-only) reference check, 2026-11-19, before building Voice → piano, Voice (raw), Voice + tune, 4-sound percussion and mic auto-mute. Its defaults were applied to Voice → piano the same day; see [README.md](README.md).

# VoiceLoop Pocket Song Builder — next track types reference check

**Date:** 2026-11-19  
**Scope:** pre-build, read-only reference check. No files changed; no app, microphone, Cargo, or Xcode commands run. No GPL/AGPL code was consulted or reused.

## Summary

The planned tracks fit VoiceLoop’s existing architecture, but three shared foundations are still missing:

1. Parallel retention of raw microphone audio, measured pitch/envelope data, and nondestructive interpreted output.
2. A low-latency provisional note path. The existing `NoteSegmenter` is suitable for committed notes, but emits only after note completion.
3. A reusable 12-bit pitch-class target mask with deterministic snapping, hysteresis, and explicit octave handling.

Existing analysis defaults are a sound committed-note baseline: 10 ms hops, 3-frame median, 80 ms minimum note, 0.7-semitone switch threshold, three-frame switch hold, four-frame gap tolerance, and octave-slip correction. The default YIN frame is 1,700 samples at 48 kHz—35.4 ms before median, switching, audio I/O, and rendering—so it cannot match specialized 1–12 ms commercial live trackers without a faster provisional path.

Recommended shared data model:

```text
raw mic audio (immutable)
+ measured pitch/envelope frames
+ provisional live note/target
+ committed note events
+ render settings: target mask, Tune, instrument, FX preset
```

Changing allowed keys, Tune, instrument, or FX should rerender from preserved data rather than rewrite the raw take.

---

## Reference check — 1. Percussion: “Da, Ba, K, Ts” → drums

Compared with:

- [Vochlea Dubler 2 trigger training](https://vochlea.com/learn/how-to-add-triggers): up to 12 examples per trigger, optional input-derived velocity, and explicit advice to use distinct sounds.
- [Vochlea trigger guidance](https://vochlea.com/learn/tips-for-triggers): short attacks improve responsiveness; sensitivity trades nuanced velocity against stable triggering.
- [LVT user-specific vocal-percussion study](https://arxiv.org/abs/1811.02406): onset → features → kNN classification; user-specific training substantially outperformed generic Ableton conversion.
- [Ableton Convert Drums to MIDI](https://www.ableton.com/en/manual/converting-audio-to-midi/): offline kick/snare/hat extraction; clear attacks and isolated audio are recommended, with editable transient markers.

| Gap or known problem | Source | Us | Action |
| --- | --- | --- | --- |
| Similar syllables are confused | Dubler advises distinct triggers; review testing found “ti/ts/ta” confusion and cleaner results with only 3–4 sounds ([opened review](https://www.engadget.com/dubler-2-can-turn-any-microphone-into-a-midi-controller-090002585.html)) | Partly handled: per-user training and “Heard” feedback | **PENDING:** confidence/rejection result, retained examples/variance, confusion test specifically for Da/Ba/K/Ts |
| Generic classification performs poorly across speakers | LVT reports large cross-user variation and much better user-specific results | Handled in design: Auto switches to trained classification | Keep Rules only as onboarding/fallback |
| Missed soft attacks versus false triggers | Dubler exposes sensitivity; Ableton recommends clear attacks | Existing adaptive-onset improvements remain incomplete | **PENDING:** adaptive floor, attack/release hysteresis, spectral flux, real-device precision/recall corpus |
| Velocity changes unpredictably with mic distance or training loudness | Dubler warns that training loudness affects velocity | Current events carry velocity, but robust normalization is absent | **PENDING:** noise-relative normalized envelope mapping; optional fixed velocity per trigger |
| Speaker bleed creates false hits | Ableton users report bleed-generated notes and substantial cleanup ([discussion](https://www.reddit.com/r/ableton/comments/1b3oh82/ableton_convert_drums_to_midi_vs_triggers_on/)) | Not handled by classification alone | Handled by proposed mic auto-mute; still test count-in/tail retriggers |
| Offline conversion requires cleanup | Ableton users report extra notes from sustained material and manual transient correction ([discussion](https://www.reddit.com/r/ableton/comments/x41v9q/how_do_you_clean_up_midi_after_audio_to_midi/)) | Live user-specific path is deliberately different | Preserve raw hits and make class/timing editable |
| Raw vocal performance is unavailable | DubBox/Ableton-style workflows can preserve or export the source | Missing | **PENDING:** capture raw mic alongside drum events |
| Touch pads should not interrupt playback | Mobile pad samplers treat touch as direct performance input | Existing `song_trigger` plays over the running song | **Handled** |

Deliberately different: begin with exactly four owner-defined sounds, but allow training to report that two are insufficiently separable rather than silently selecting the wrong drum.

---

## Reference check — 2. Voice Raw

Compared with:

- [GarageBand iPhone Audio Recorder](https://support.apple.com/en-gw/guide/garageband-iphone/chs39283a21/ios): raw recording, input level, noise gate, optional monitoring, and simple character presets.
- [Koala Sampler](https://manual.koalasampler.com/mobile/4-sample/): direct mic capture, eight input effects, auto-trim/normalize, bar-aligned loop capture, and explicit headphone warning for monitoring.
- [BandLab monitoring](https://help.bandlab.com/hc/en-us/articles/115002945233-Monitoring-your-Recording): monitoring is user-controlled and headphones are required to avoid bleed.

| Gap or known problem | Source | Us | Action |
| --- | --- | --- | --- |
| Raw voice is not retained in Song Builder | All three tools retain audio | Missing | **PENDING, must-have:** immutable raw take with latency-aligned loop window |
| Quiet takes and clipped takes | GarageBand exposes level and clipping feedback; Koala offers normalize | Metering exists, Song Builder raw workflow does not | **PENDING:** clipping warning and nondestructive playback gain; avoid destructive normalize |
| Noise gates cut breaths or word endings | Common gate behavior; GarageBand makes the threshold user-controlled | DSP gate exists elsewhere, not defined for this track | Default gate off; preset may enable a gentle expander. Preserve raw input |
| Monitoring feedback and bleed | Koala warns that mic monitoring requires headphones | Monitoring is already off by default | **Handled:** keep dry monitoring off on speaker route |
| FX alter or obscure the source | GarageBand applies presets while keeping an editable recording | Existing realtime effect chains are nondestructive | **Handled architecturally; PENDING integration** |
| Effect tails or delay contaminate subsequent mic capture | Phone speaker recording | Not addressed by ordinary mute state | Mic auto-mute must also ramp sends/tails or suspend their output |
| Excessive preset choice | Mobile comparables present a small preset surface | Seven Song Builder presets already exist | Start with Clean, Warm, Room, Radio; expose chain editing later |

Recommended raw-voice fades:

- 5 ms fade-in.
- 20 ms fade-out.
- 10 ms crossfade at a loop seam when source padding permits.
- Preserve at least 100 ms hidden source padding on both sides for later trim/fade changes.

---

## Reference check — 3. Voice to Piano

Compared with:

- [Dubler 2 pitch behavior](https://vochlea.com/learn/using-pitch): stickiness controls note-change difficulty; IntelliBend anchors sustained notes while allowing bends into/out of them.
- [Dubler 2 key restrictions](https://vochlea.com/learn/restricting-the-key): 12 pitch classes, scale/custom restrictions, individual-note disabling, nearest permitted-note output.
- GarageBand’s scale instruments reduce the playable surface to scale notes; its touch keyboard also exposes octave, sustain, velocity sensitivity, glissando, and portamento behavior.
- [Ableton Convert Melody to MIDI](https://www.ableton.com/en/manual/converting-audio-to-midi/): singing/whistling → monophonic notes, but clear attacks and isolated audio are necessary.

| Gap or known problem | Source | Us | Action |
| --- | --- | --- | --- |
| Pitch flickers between neighboring notes | Dubler exposes stickiness; reviewers report extra MIDI from unsteady voices | Existing 0.7-semitone/3-hop hysteresis handles committed notes | **PENDING:** provisional live target state using allowed-set boundaries |
| Disabled-note snapping is underspecified | Dubler says “nearest in-key note” but does not document midpoint ties | Missing | Implement deterministic rule below |
| Glide or vibrato becomes many short notes | Dubler separates IntelliBend from precise TruBend; Ableton users report sustained audio becoming extra notes | Existing soft-exit logic protects vibrato; glides can still create short notes | Preserve raw contour; commit discrete notes only after 80 ms |
| Real octave leaps versus octave errors | Low notes and octave mistakes recur in audio-to-MIDI reports; Logic users report sustained notes split into octave/fifth notes ([report](https://www.reddit.com/r/Logic_Studio/comments/l2yt5m/editing_flex_pitchs_note_detection/)) | Eight-hop octave hold rejects slips, but delays real octave leaps by ~80 ms | **PENDING:** provisional visual target immediately; commit leap after 60–80 ms or a clear onset |
| Note-on arrives only after note-off | Existing `NoteSegmenter` emits completed events | Missing for live playing | **PENDING, must-have:** provisional NoteOn/TargetChange/NoteOff stream |
| Dynamics are unavailable in `NoteEvent` | Song voices already accept velocity | Missing analysis field | **PENDING:** per-note envelope/peak and normalized velocity |
| Duration/release can click or feel detached | Song voices support duration and release | Partly handled | Use last voiced frame as duration; renderer-specific release and de-click |
| Automatic key changes during performance are surprising | Dubler defaults to auto detection | Pocket product doc says Auto must only suggest | **Handled by plan:** never silently change target mask while recording |
| Conversion requires cleanup | Ableton community reports wrong/missing notes and manual editing | No note editor yet | **PENDING:** tap-to-correct pitch, onset, duration, and velocity |

### Snapping rule

Represent enabled notes as a 12-bit pitch-class mask; reject an empty mask.

For input pitch `p` in fractional MIDI:

1. Search allowed notes across neighboring octaves.
2. Choose minimum absolute cents distance.
3. Retain the current target until `p` crosses the Voronoi midpoint toward the challenger by an additional **15 cents** for **30 ms**.
4. At an exact unresolved tie: keep the current target; otherwise follow the recent pitch direction; if direction is neutral, choose the lower note.
5. Store measured `p` separately from the chosen target.

This avoids biasing ordinary transitions downward while remaining deterministic.

---

## Reference check — 4. Voice with auto-tune

Compared with:

- [Antares AutoTune guidance](https://help.antarestech.com/hc/en-us/articles/42858099043092-AutoTune-Best-Practices): 10–30 ms for live correction, 30–50 ms for natural correction, Humanize for sustained notes, and Flex Tune for gestures.
- [Waves Tune Real-Time manual](https://assets.wavescdn.com/pdf/plugins/tune-real-time.pdf): nearest legal note, 0.1–800 ms speed and transition controls, cents/time tolerance, vibrato preservation, formant correction, and stated 0–4 ms pitch-dependent processing delay.
- [BandLab AutoPitch](https://help.bandlab.com/hc/en-us/articles/29099155628953-AutoPitch-FAQ): Original, Low-Latency, and Formant-Preserving modes plus custom scales.
- GarageBand Extreme Tuning: song key/major-minor targeting with deliberately simple mobile controls.

| Gap or known problem | Source | Us | Action |
| --- | --- | --- | --- |
| Wrong key produces undesirable note jumps | GarageBand explicitly warns about incorrect key/scale | Custom 12-key mask is planned | Show input and corrected notes simultaneously; never hide active target mask |
| Fast correction sounds robotic | Antares and Waves expose speed, tolerance, vibrato, and humanization; BandLab users report robotic results even at low displayed amounts ([report](https://www.reddit.com/r/Bandlab/comments/1l86d74/the_app_updated_for_me_and_now_it_almost/)) | No corrector exists | Use Tune plus an Advanced sheet; preserve raw take |
| Loud or unstable input causes correction to gate/glitch | BandLab user reports correction switching on/off with louder input ([report](https://www.reddit.com/r/Bandlab/comments/1xd133q/autopitch_broken/)) | Confidence gate exists for analysis only | Crossfade to aligned dry audio during unvoiced/low-confidence spans; never hold stale pitch |
| Large shifts change perceived voice identity | Antares recommends formant compensation; BandLab exposes a formant-preserving mode | Missing | Formant preservation required; warn/soft-limit live correction beyond ±4 semitones |
| Fast target transitions flatten scoops and vibrato | Antares Flex Tune/Humanize; Waves separate Speed, Transition, Tolerance, Vibrato | Existing pitch stability is analysis-only | Preserve vibrato around the settled center and correct the center, not every instantaneous deviation |
| Pitch-correction latency disrupts singing | Antares quotes sub-1 to 2.3 ms specialized modes; Waves quotes 0–4 ms; BandLab documents additional effects latency | Current YIN alone needs 35.4 ms | Set honest latency expectation; disable speaker monitoring by default |
| Formant correction is better in high-quality/offline modes | Antares distinguishes low-latency and HQ | One realtime path planned | **PENDING:** later offline/HQ render; do not block first realtime implementation |

### Algorithm-family assessment

| Family | Strength | Typical artifacts | Fit |
| --- | --- | --- | --- |
| TD-PSOLA / pitch-synchronous overlap-add | Low latency; well suited to monophonic voiced material; can preserve vocal envelope for moderate shifts | Pitch-mark jitter, buzz/roughness at irregular phonation, trouble on breath/unvoiced consonants, conspicuous artifacts on large shifts | **Recommended first realtime path** |
| Phase vocoder | Robust for larger shifts and more general signals | Phasiness, loss of presence, transient smearing; documented even near unity ratios ([Laroche/Dolson paper](https://seniordesign.cs.tcu.edu/1516Ribbit/resources/ImprovedPhaseVocoder.pdf)) | Not the first live vocal path |
| PSOLA plus LPC/spectral-envelope processing | Better formant control and voice identity | More CPU and potential filter instability; still dependent on reliable F0/pitch marks | Add after baseline PSOLA |
| Neural vocoder | Potentially higher quality on extreme transformations | Model size, CPU/battery, training-domain failures, harder deterministic realtime behavior | Not applicable to first offline-first release |

A singing-voice study describes YIN-guided pitch marks, two-period Hann windows, PSOLA synthesis, and spectral-envelope/formant filtering; it also notes that pitch-mark errors degrade quality ([study](https://www.mdpi.com/2076-3417/6/11/368)). Signalsmith’s MIT implementation is a useful benchmark for latency/formant behavior but not a required dependency; its documentation notes that general spectral formant correction is less sharp than monophonic PSOLA ([documentation](https://git.signalsmith-audio.co.uk/Signalsmith-Audio/signalsmith-stretch/src/commit/c5987267e6ad42ff9b620b2802c4600472e85818/README.md?display=source)).

### Recommended realtime-safe Rust design

```text
input callback
  → preallocated raw-audio ring
dedicated vocal DSP worker
  → YIN/provisional F0
  → target mask + hysteresis + correction contour
  → fixed-size pitch-mark circular buffer
  → TD-PSOLA overlap/add
  → aligned voiced/corrected ↔ dry/unvoiced crossfade
  → preallocated corrected-audio ring
output callback
  → drain corrected ring
  → track FX chain
  → mix
```

Requirements:

- Allocate all rings, windows, pitch-mark storage, overlap buffers, LPC state, and scratch arrays before starting.
- No allocation, locks, logging, I/O, destruction, or large clears in either callback.
- Pass target/settings changes through bounded `rtrb` messages.
- Drop or bypass safely on ring starvation; never block the output callback.
- Keep raw audio on the capture path before correction and FX.
- Run ordinary EQ/compression/reverb after correction so the tracker sees clean audio, matching Antares/Waves guidance.

Expected first-version latency at 48 kHz:

- Pitch evidence: **32–36 ms** with the present range/window.
- Median/provisional confirmation: **10 ms**.
- Worker/ring/audio buffers: approximately **8–25 ms**, device-dependent.
- Expected algorithmic-plus-buffer monitoring: **50–75 ms**, before Bluetooth.
- Bluetooth monitoring: unsupported for live correction; offer record-without-monitoring.

This is adequate for recording and later playback, but marginal for comfortable live self-monitoring. A dedicated adaptive short-window tracker is **PENDING** if device tests exceed 60 ms median mouth-to-ear latency.

---

## Reference check — microphone auto-mute

Compared with:

- [BandLab monitoring guidance](https://help.bandlab.com/hc/en-us/articles/115002945233-Monitoring-your-Recording): headphones prevent playback bleed into the microphone.
- [Koala monitoring guidance](https://manual.koalasampler.com/mobile/4-sample/): headphone warning because speaker monitoring can create feedback.
- GarageBand iPhone: monitoring becomes available for an external mic/headset rather than encouraging built-in-mic-to-speaker monitoring.
- [BandLab latency guidance](https://help.bandlab.com/hc/en-us/articles/58874563630361-Latency-Issues): wired monitoring is recommended; effects/pitch correction add delay, and recording placement compensation does not remove heard monitoring latency.

| Gap or known problem | Source | Us | Action |
| --- | --- | --- | --- |
| Backing tracks leak into the built-in mic | All mobile references recommend headphones; Ableton users report bleed-created events | Owner requires auto-mute | **Handled by plan**, with exact behavior below |
| Abrupt mute clicks | General audio behavior | No Song Builder policy yet | 10 ms equal-power mute/restore ramps |
| Muting overwrites the user’s mix | Potential implementation trap | No policy yet | Use transient “mic capture suppression,” never modify persisted mute/solo |
| Reverb/delay tails continue after tracks mute | Stacked FX | Not handled by gain alone | Suppress complete part-bus output, including returns/tails |
| User loses pitch/timing reference | Consequence of speaker-safe recording | Inherent tradeoff | Play count-in, then mute at capture start; make behavior explicit |
| Touch instruments unnecessarily stop the song | Owner says they must continue | Existing touch triggering can overlay playback | **Handled:** auto-mute applies only to microphone capture |

Recommended behavior:

- On microphone capture start, ramp every other part bus and click to silence over **10 ms**.
- The count-in may play; suppression reaches silence exactly at the first captured frame.
- Do not persist or visually impersonate track mute state. Show one banner: “Other parts muted while the mic records.”
- Restore the prior audible mix over **20 ms** after capture stops.
- Suppress complete bus output, not merely event generation, so effect tails cannot bleed.
- Touch piano/drum-pad recording bypasses suppression and leaves the song playing.
- With wired headphones, provide a future setting for “Play backing tracks while mic records”; default remains auto-mute.
- If capture is interrupted or errors, restore the prior mix through the same cleanup path.

---

## Recommended defaults

| Area | Default |
| --- | --- |
| Allowed pitches | Chromatic mask initially; require at least one enabled pitch class |
| Pitch snapping | Nearest allowed absolute pitch; 15-cent hysteresis beyond boundary; 30 ms target-change hold |
| Exact tie | Current target → recent movement direction → lower note |
| Pitch confidence | Current YIN clarity ≥ 0.75; do not hold stale pitch through unvoiced frames |
| Provisional NoteOn | Two consecutive confident frames, approximately 10 ms confirmation after first estimate |
| Committed minimum note | **80 ms** |
| Hard note switch | **0.7 semitone**, **30 ms** |
| Soft/vibrato switch | **100 ms** |
| Note-off gap | **40 ms**; end duration at last confident pitched frame |
| Octave-slip acceptance | **80 ms**, or **60 ms** when accompanied by a fresh amplitude onset |
| Velocity analysis | Noise-relative RMS/peak over the first **40 ms** |
| Velocity curve | Normalize from `noise floor + 6 dB` to `noise floor + 36 dB`; exponent **0.65**; output **0.20–1.00** |
| Initial live velocity | 0.65, ramp to measured velocity within 20 ms; recorded event receives final measured value |
| Instrument release | Piano **250 ms**, soft synth **350 ms**, bass **120 ms**; generic default **120 ms** |
| Raw clip fades | 5 ms in, 20 ms out; 10 ms loop crossfade |
| Tune default | **50%**, approximately **25 ms** retune |
| Natural | 0% correction; retain segmentation hysteresis |
| Locked | **5 ms** retune, maximum target pull |
| Humanize | After **300 ms** sustained, relax effective retune toward **60 ms** |
| Vibrato | Preserve deviations within approximately **±35 cents** around the settled note center at default Tune |
| Voiced/unvoiced DSP crossfade | **10 ms** |
| Mic auto-mute | 10 ms down, 20 ms restore |

All of these sound/interface-affecting values need registry entries and `SETTINGS_INDEX.md` rows when implementation begins.

## Risks

- The existing analysis path implies noticeably more live latency than Dubler, Antares, or Waves.
- Built-in speaker recording without backing tracks prevents bleed but also removes musical reference after the count-in.
- iOS AGC, voice processing, echo cancellation, route changes, and microphone distance can change both classification and velocity.
- Sparse pitch masks can require shifts of several semitones; TD-PSOLA artifacts and voice-identity changes become much more obvious beyond roughly ±4 semitones.
- Breath, fry, growl, consonants, rapid melisma, and very low fundamentals can cause pitch gaps, octave errors, or unstable pitch marks.
- Switching dry/corrected audio without identical delay creates combing or discontinuities.
- Per-track FX multiply CPU and battery use; convolution/lookahead effects should not appear in low-latency presets.
- Raw audio plus interpreted derivatives increases mobile storage; use immutable shared sources rather than duplicate audio.
- Current synthetic tests cannot establish perceived latency, syllable confusion, formant quality, or speaker-bleed behavior. A labeled real-voice/device corpus remains required.