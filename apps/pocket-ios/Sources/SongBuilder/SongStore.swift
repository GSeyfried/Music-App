// Song Builder state and engine glue. Views read and call this; it owns the
// song (with undo/redo), the transport, voice-drum listening and training,
// and keeps the Rust engine's song (crates/voiceloop-engine/src/song.rs) in
// step with the model.
//
// Mic only when needed: the microphone and analysis are requested only while
// voice drums are listening (a drum part is selected and "Use voice" is on)
// or while training, and released otherwise.

import Foundation
import SwiftUI

@MainActor
final class SongStore: ObservableObject {
    // MARK: Published state

    @Published private(set) var song: Song
    @Published var selectedPartId: Int?
    @Published private(set) var canUndo = false
    @Published private(set) var canRedo = false

    @Published private(set) var playing = false
    @Published private(set) var recording = false
    /// Loop position from the engine; `pass < 0` = count-in (beat is negative then).
    @Published private(set) var beat: Double = 0
    @Published private(set) var pass: Int64 = -1

    @Published private(set) var micState = "off"
    @Published private(set) var engineReady = false
    @Published private(set) var lastError: String?
    /// Most recent recognised voice hit, for the "Heard:" readout and pad flash.
    @Published private(set) var lastHeard: (lane: DrumLane, centroidHz: Double)?
    /// Increments on every pad/voice/training hit, with the lane (drives pad flashes).
    @Published private(set) var flash: (lane: DrumLane, count: Int) = (.kick, 0)

    @Published private(set) var profile: VoiceProfile = [:]
    /// Training in progress: which sound and how many examples so far.
    @Published private(set) var training: (lane: DrumLane, count: Int)?
    /// Voice → piano: the key sounding now (after snapping) and the sung pitch.
    @Published private(set) var voiceKey: Int?
    @Published private(set) var sungMidi: Double?
    /// Output is the phone speaker (or other open air), so playback leaks into the mic. True until the engine reports a route.
    @Published private(set) var outputIsSpeaker = true
    /// Live voice monitoring allowed now: never on the speaker unless the setting says so.
    var monitorVoice: Bool { settings.voiceMonitor && (!outputIsSpeaker || settings.monitorOnSpeaker) }

    /// Mic input level (dBFS RMS) while listening, for the sensitivity meter.
    @Published private(set) var inputLevelDb: Double?

    let settings: PocketSettings

    // MARK: Private

    private var past: [Song] = []
    private var future: [Song] = []
    private var engine: EngineBridge?
    private var saveTask: Task<Void, Never>?
    private var listening = false

    /// False for a preview/snapshot store: it never starts the engine or saves.
    private let persists: Bool

    /// `previewSong` makes a preview store (snapshots): that song, no saved
    /// profile, no engine, no saving.
    init(settings: PocketSettings = PocketSettings(), previewSong: Song? = nil) {
        self.settings = settings
        persists = previewSong == nil
        song = previewSong ?? SongStore.loadSong() ?? Song.new(settings: settings)
        profile = previewSong == nil ? SongStore.loadProfile() : [:]
        selectedPartId = song.parts.first?.id
    }

    /// Dismiss the error banner.
    func clearError() { lastError = nil }

    var selectedPart: Part? { song.parts.first { $0.id == selectedPartId } ?? song.parts.first }
    var voiceProfileReady: Bool { VoiceDrums.ready(profile, minExamples: settings.trainingHits) }
    var recognitionUsesTraining: Bool {
        settings.classifier == .trained || (settings.classifier == .auto && voiceProfileReady)
    }

    // MARK: Engine lifecycle

    /// Start the engine and audio output (the mic opens later, only when needed).
    /// Runs the engine calls off the main thread: device setup can take a while.
    func start() {
        guard engine == nil, persists else { return }
        let bridge = EngineBridge { [weak self] event, arrival in
            Task { @MainActor in self?.handle(event, arrival: arrival) }
        }
        engine = bridge
        let sessionMode = settings.sessionMode
        let settingsPath = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            .map { dir -> String in
                try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
                return dir.appendingPathComponent("audio-settings.json").path
            }
        Task.detached(priority: .userInitiated) { [weak self] in
            func timed(_ name: String, _ body: () throws -> Void) throws {
                let t0 = Date()
                NSLog("[pocket] %@ …", name)
                try body()
                NSLog("[pocket] %@ done in %.0f ms", name, Date().timeIntervalSince(t0) * 1000)
            }
            do {
                if let settingsPath { try? timed("use_settings_file") { _ = try bridge.call("use_settings_file", ["path": settingsPath]) } }
                try timed("set_input_needs") { _ = try bridge.call("set_input_needs", ["needs": ["reasons": [String](), "holdMs": 3000]]) }
                // Session mode "default" keeps iOS mic gain and full speaker level (see audio.ios.sessionMode).
                try timed("start_audio") { _ = try bridge.call("start_audio", ["config": ["sessionMode": sessionMode]]) }
                await MainActor.run {
                    guard let self else { return }
                    self.engineReady = true
                    self.syncEngine()
                    self.updateListening()
                }
            } catch {
                NSLog("[pocket] engine start failed: %@", "\(error)")
                await MainActor.run { self?.lastError = "\(error)" }
            }
        }
    }

    private func handle(_ event: EngineEvent, arrival: Double) {
        switch event {
        case .status(let s):
            micState = s.input?.mic ?? (s.capturing ? "on" : "off")
        case .songPosition(let p):
            beat = p.beat
            pass = p.pass
            if !p.playing && playing {
                playing = false
                recording = false
            }
        case .hit(let h):
            onHit(h)
        case .pitch(let p):
            onPitch(p)
        case .session(let info):
            outputIsSpeaker = info.outputIsOpenAir
            if !monitorVoice { endVoiceNote(at: nil) }
            syncEngine()
        case .level(let l):
            inputLevelDb = listening ? l.rmsDb : nil
        case .error(let text):
            lastError = text
        default:
            break
        }
    }

    // MARK: Undo / edits

    /// Apply an edit, record an undo step, and resync the engine.
    func edit(_ change: (inout Song) -> Void) {
        var next = song
        change(&next)
        guard next != song else { return }
        past.append(song)
        if past.count > settings.undoDepth { past.removeFirst(past.count - settings.undoDepth) }
        future.removeAll()
        song = next
        afterChange()
    }

    func undo() {
        guard let prev = past.popLast() else { return }
        future.insert(song, at: 0)
        song = prev
        afterChange()
    }

    func redo() {
        guard !future.isEmpty else { return }
        past.append(song)
        song = future.removeFirst()
        afterChange()
    }

    private func afterChange() {
        canUndo = !past.isEmpty
        canRedo = !future.isEmpty
        if selectedPartId == nil || !song.parts.contains(where: { $0.id == selectedPartId }) {
            selectedPartId = song.parts.first?.id
        }
        syncEngine()
        updateListening()
        scheduleSave()
    }

    func select(_ partId: Int) {
        selectedPartId = partId
        updateListening()
    }

    func addPart(_ template: TrackTemplate) {
        guard template.available else { return }
        var id = 0
        edit { id = $0.addPart(template, kit: settings.defaultKit) }
        select(id)
    }

    /// True while the mic is recording into the song: the other parts may be muted (no headphones → bleed).
    var micRecording: Bool { recording && listening && training == nil }

    func deletePart(_ id: Int) {
        edit { s in s.parts.removeAll { $0.id == id } }
    }

    // MARK: Engine sync

    /// Skip-pass marks for events just heard live while recording (not played again this pass).
    private var heardPass: [Int: Int64] = [:]

    private func syncEngine() {
        guard let engine, engineReady else { return }
        let s = song
        var audible = s.audibleParts
        if micRecording {
            switch settings.voiceRecordMute {
            case "all": audible = []
            case "others": audible = audible.filter { $0.id == selectedPartId }
            default: break
            }
        }
        let parts: [[String: Any]] = audible.map { part in
            let sound: [String: Any] = part.kind == .drums ? ["kind": "drums", "kit": part.kit] : ["kind": "melody", "instrument": part.instrument.rawValue]
            let events: [[String: Any]] = part.kind == .drums
                ? part.hits.map { h in
                    var e: [String: Any] = ["beat": s.tightened(h.beat), "velocity": h.velocity, "lane": h.lane.rawValue]
                    if let p = heardPass[h.id] { e["skipPass"] = p }
                    return e
                }
                : part.notes.map { n in
                    var e: [String: Any] = ["beat": s.tightened(n.beat), "velocity": n.velocity, "midi": n.midi, "lengthBeats": n.length]
                    if let p = heardPass[n.id] { e["skipPass"] = p }
                    return e
                }
            return ["id": part.id, "sound": sound, "gain": part.level, "fx": part.fx, "events": events]
        }
        var click = settings.clickMode == .always || (settings.clickMode == .recording && recording)
        // Through the speaker the click leaks into the mic and records as hits: drop it while the mic records.
        if micRecording && outputIsSpeaker && !settings.clickOnSpeakerWhileMic { click = false }
        let spec: [String: Any] = [
            "bpm": s.bpm, "beatsPerBar": s.beatsPerBar, "loopBeats": s.loopBeats,
            "click": click, "clickGain": settings.clickVolume, "masterGain": settings.masterVolume, "loudnessDb": settings.loudnessDb, "parts": parts,
        ]
        do { try engine.call("song_set", ["song": spec]) } catch { lastError = "\(error)" }
    }

    // MARK: Transport

    func togglePlay() {
        playing ? stop() : play(countIn: 0, record: false)
    }

    func toggleRecord() {
        if !playing {
            play(countIn: settings.countInBars, record: true)
        } else {
            recording.toggle()
            syncEngine() // click mode "while recording"
        }
    }

    private func play(countIn: Int, record: Bool) {
        guard let engine else { return }
        recording = record
        heardPass.removeAll()
        syncEngine()
        do {
            try engine.call("song_play", ["countInBars": Double(countIn)])
            playing = true
        } catch {
            lastError = "\(error)"
            recording = false
        }
    }

    func stop() {
        try? engine?.call("song_stop")
        endVoiceNote(at: nil)
        playing = false
        recording = false
        beat = 0
        pass = -1
        syncEngine() // un-mute parts muted while recording
    }

    /// Settings that change the engine song (click, volume) were edited.
    func settingsChanged() {
        syncEngine()
        updateListening()
    }

    // MARK: Playing and recording sounds

    private struct Located: Decodable { let absoluteBeat: Double; let loopBeat: Double; let pass: Int64 }

    /// Place an event played at `inputFrame` (mic) or now (touch) into the selected part, if recording.
    private func record(inputFrame: UInt64?, extraMs: Double, place: (inout Song, Double) -> Int) {
        guard recording, playing, let engine else { return }
        var args: [String: Any] = ["extraMs": extraMs]
        if let inputFrame { args["inputFrame"] = inputFrame }
        guard let loc = try? engine.call("song_locate", args, as: Located?.self) else { return }
        guard loc.absoluteBeat >= -settings.earlyHitBeats else { return }
        var id = -1
        var next = song
        id = place(&next, loc.absoluteBeat)
        // It already sounded live: skip it in the pass where it would play next.
        let tightAbs = next.tightenedAbsolute(loc.absoluteBeat)
        heardPass[id] = Int64((tightAbs / next.loopBeats + 1e-9).rounded(.down))
        edit { $0 = next }
    }

    func tapPad(_ lane: DrumLane) {
        // Touch input never mutes the song; only mic recording does.
        guard let part = selectedPart, part.kind == .drums else { return }
        trigger(part.id, lane: lane, velocity: settings.padVelocity)
        flashLane(lane)
        let v = settings.padVelocity
        record(inputFrame: nil, extraMs: settings.touchLatencyMs) { s, beat in
            s.addHit(part.id, lane: lane, beat: beat, velocity: v, source: .pad)
        }
    }

    /// Keys held down now: when each was pressed (for the recorded length).
    private var heldKeys: [Int: (partId: Int, pressed: Date)] = [:]

    /// Touch-down on a piano key: the note sounds until `keyUp`.
    func keyDown(_ midi: Int) {
        guard let part = selectedPart, part.kind == .melody, let engine, heldKeys[midi] == nil else { return }
        heldKeys[midi] = (part.id, Date())
        try? engine.call("song_trigger", ["trigger": ["partId": part.id, "midi": midi, "velocity": settings.padVelocity, "hold": true]])
    }

    /// Touch-up: release the note and, when recording, add it with the length it was held.
    func keyUp(_ midi: Int) {
        guard let held = heldKeys.removeValue(forKey: midi) else { return }
        try? engine?.call("song_release", ["release": ["partId": held.partId, "midi": midi]])
        let seconds = Date().timeIntervalSince(held.pressed)
        let length = max(settings.melodyNoteBeats * 0.25, seconds / song.secondsPerBeat)
        let v = settings.padVelocity
        // Placed where the press was heard: "now" minus how long it was held.
        record(inputFrame: nil, extraMs: settings.touchLatencyMs + seconds * 1000) { s, beat in
            s.addNote(held.partId, midi: midi, beat: beat, length: length, velocity: v)
        }
    }

    /// A quick tap (accessibility, tests): press and release.
    func tapKey(_ midi: Int) {
        keyDown(midi)
        keyUp(midi)
    }

    /// Add or remove a hit in one grid cell (step editing).
    func toggleStep(partId: Int, lane: DrumLane, step: Int) {
        let perBeat = Double(song.grid.stepsPerBeat)
        let steps = Int((song.loopBeats * perBeat).rounded())
        guard let part = song.parts.first(where: { $0.id == partId }) else { return }
        let inCell = part.hits.filter { $0.lane == lane && Int((song.tightened($0.beat) * perBeat).rounded()) % max(1, steps) == step }
        if inCell.isEmpty {
            edit { $0.addHit(partId, lane: lane, beat: Double(step) / perBeat, velocity: 0.85, source: .pad) }
            if !playing { trigger(partId, lane: lane, velocity: 0.85) }
        } else {
            let ids = Set(inCell.map(\.id))
            edit { $0.update(partId) { $0.hits.removeAll { ids.contains($0.id) } } }
        }
    }

    private func trigger(_ partId: Int, lane: DrumLane, velocity: Double) {
        try? engine?.call("song_trigger", ["trigger": ["partId": partId, "lane": lane.rawValue, "velocity": velocity]])
    }

    private func flashLane(_ lane: DrumLane) {
        flash = (lane, flash.count + 1)
    }

    // MARK: Voice drums

    func setListen(_ on: Bool) {
        settings.voiceListen = on
        updateListening()
    }

    private func updateListening() {
        let want = engineReady && settings.voiceListen && (selectedPart?.input == .voice || training != nil)
        guard want != listening, let engine else { return }
        listening = want
        if !want { endVoiceNote(at: nil) }
        defer { syncEngine() } // mic recording mutes or un-mutes parts
        let reasons = want ? ["songBuilder"] : [String]()
        _ = try? engine.call("set_input_needs", ["needs": ["reasons": reasons]])
        _ = try? engine.call("set_analysis_config", ["config": [
            "enabled": want,
            "onsets": ["riseDb": settings.onsetRiseDb, "floorDb": settings.onsetFloorDb],
        ]])
        if want { _ = try? engine.call("request_mic_permission") }
    }

    private func onHit(_ h: HitTimbre) {
        guard listening, VoiceDrums.accepts(h, minLevelDb: settings.minLevelDb, minRiseDb: settings.minRiseDb) else { return }
        if let t = training {
            profile = VoiceDrums.train(profile, lane: t.lane, h)
            SongStore.saveProfile(profile)
            flashLane(t.lane)
            if monitorVoice, let part = selectedPart, part.kind == .drums { trigger(part.id, lane: t.lane, velocity: 0.8) }
            let count = t.count + 1
            if count >= settings.trainingHits {
                let i = DrumLane.voiceLanes.firstIndex(of: t.lane) ?? 0
                training = i + 1 < DrumLane.voiceLanes.count ? (DrumLane.voiceLanes[i + 1], 0) : nil
                if training == nil { updateListening() }
            } else {
                training = (t.lane, count)
            }
            return
        }
        guard let part = selectedPart, part.kind == .drums, part.input == .voice else { return }
        let lane = VoiceDrums.classify(h, mode: settings.classifier, profile: profile, rules: settings.rules, minExamples: settings.trainingHits)
        lastHeard = (lane, h.centroidHz)
        flashLane(lane)
        let velocity = VoiceDrums.velocity(levelDb: h.levelDb, minLevelDb: settings.minLevelDb, floor: settings.minVelocity)
        if monitorVoice { trigger(part.id, lane: lane, velocity: velocity) }
        record(inputFrame: h.frame, extraMs: settings.voiceLatencyMs) { s, beat in
            s.addHit(part.id, lane: lane, beat: beat, velocity: velocity, source: .voice)
        }
    }

    // MARK: Voice → piano

    private struct VoiceNote { let partId: Int; let key: Int; let startFrame: UInt64; let velocity: Double }
    private var voiceNote: VoiceNote?
    /// A key waiting to become the note: how many frames in a row it has been heard.
    private var pendingKey: (key: Int, frames: Int, firstFrame: UInt64)?
    private var unpitchedFrames = 0
    private var pitchRate = 48_000

    /// Pitch frames (≈ every 10 ms) → held notes on the nearest enabled key.
    private func onPitch(_ update: PitchUpdate) {
        pitchRate = update.sampleRate
        guard listening, training == nil, let part = selectedPart, part.kind == .melody, part.input == .voice else { return }
        for f in update.frames {
            let loud = (f.rmsDb ?? -120) >= settings.voicePianoMinLevelDb
            let clear = (f.clarity ?? 0) >= settings.voicePianoMinClarity
            guard let midi = f.midi, loud, clear else {
                sungMidi = nil
                pendingKey = nil
                unpitchedFrames += 1
                if unpitchedFrames >= settings.voicePianoReleaseFrames { endVoiceNote(at: f.frame) }
                continue
            }
            unpitchedFrames = 0
            sungMidi = midi
            let key = KeySnap.snap(midi + Double(part.transpose), enabled: part.enabledKeys, tie: settings.voicePianoTie,
                                   current: voiceNote?.key, hysteresis: settings.voicePianoHysteresis)
            if key == voiceNote?.key { pendingKey = nil; continue }
            let p = pendingKey?.key == key ? (key, pendingKey!.frames + 1, pendingKey!.firstFrame) : (key, 1, f.frame)
            pendingKey = p
            guard p.1 >= settings.voicePianoStartFrames else { continue }
            pendingKey = nil
            endVoiceNote(at: p.2)
            let velocity = VoiceDrums.velocity(levelDb: f.rmsDb ?? -30, minLevelDb: settings.voicePianoMinLevelDb, floor: settings.minVelocity)
            voiceNote = VoiceNote(partId: part.id, key: key, startFrame: p.2, velocity: velocity)
            voiceKey = key
            if monitorVoice {
                try? engine?.call("song_trigger", ["trigger": ["partId": part.id, "midi": key, "velocity": velocity, "hold": true]])
            }
        }
    }

    /// Release the sounding voice note and, when recording, add it to the part.
    private func endVoiceNote(at endFrame: UInt64?) {
        guard let n = voiceNote else { return }
        voiceNote = nil
        voiceKey = nil
        try? engine?.call("song_release", ["release": ["partId": n.partId, "midi": n.key]])
        guard let endFrame, endFrame > n.startFrame else { return }
        let seconds = Double(endFrame - n.startFrame) / Double(max(1, pitchRate))
        guard seconds >= settings.voicePianoMinNoteSeconds else { return } // a blip, not a note
        let length = seconds / song.secondsPerBeat
        record(inputFrame: n.startFrame, extraMs: settings.voiceLatencyMs) { s, beat in
            s.addNote(n.partId, midi: n.key, beat: beat, length: length, velocity: n.velocity)
        }
    }

    func setKeyEnabled(_ pitchClass: Int, _ on: Bool) {
        guard let id = selectedPart?.id, (0..<12).contains(pitchClass) else { return }
        edit { $0.update(id) { $0.enabledKeys[pitchClass] = on } }
    }

    func setFx(_ partId: Int, _ ids: [String]) {
        let list = ids.filter { $0 != "clean" }.prefix(3)
        edit { $0.update(partId) { $0.fx = list.isEmpty ? "clean" : list.joined(separator: "+") } }
    }

    func startTraining() {
        training = (.kick, 0)
        updateListening()
    }

    func stopTraining() {
        training = nil
        updateListening()
    }

    func forgetTraining() {
        profile = [:]
        SongStore.saveProfile(profile)
    }

    // MARK: Persistence (local, offline)

    private static var folder: URL {
        let dir = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    private static func loadSong() -> Song? {
        guard let data = try? Data(contentsOf: folder.appendingPathComponent("song.json")) else { return nil }
        return try? JSONDecoder().decode(Song.self, from: data)
    }

    private func scheduleSave() {
        guard persists else { return }
        saveTask?.cancel()
        let s = song
        saveTask = Task {
            try? await Task.sleep(nanoseconds: 300_000_000)
            guard !Task.isCancelled, let data = try? JSONEncoder().encode(s) else { return }
            try? data.write(to: SongStore.folder.appendingPathComponent("song.json"), options: .atomic)
        }
    }

    private static func loadProfile() -> VoiceProfile {
        guard let data = try? Data(contentsOf: folder.appendingPathComponent("voice-profile.json")),
              let raw = try? JSONDecoder().decode([String: LaneProfile].self, from: data) else { return [:] }
        var p: VoiceProfile = [:]
        for (k, v) in raw { if let lane = DrumLane(rawValue: k) { p[lane] = v } }
        return p
    }

    private static func saveProfile(_ p: VoiceProfile) {
        let raw = Dictionary(uniqueKeysWithValues: p.map { ($0.key.rawValue, $0.value) })
        if let data = try? JSONEncoder().encode(raw) {
            try? data.write(to: folder.appendingPathComponent("voice-profile.json"), options: .atomic)
        }
    }

    #if DEBUG
    /// Snapshot mode only (Views/SnapshotMode.swift): pose a preview store in a
    /// transport/voice state without an engine. Ignored on a real store.
    func posePreview(playing: Bool = false, recording: Bool = false, beat: Double = 0, pass: Int64 = -1,
                     micState: String = "off", lastHeard: (lane: DrumLane, centroidHz: Double)? = nil,
                     training: (lane: DrumLane, count: Int)? = nil, profile: VoiceProfile? = nil, error: String? = nil,
                     listening: Bool = false, voiceKey: Int? = nil, sungMidi: Double? = nil, inputLevelDb: Double? = nil) {
        guard !persists else { return }
        self.listening = listening
        self.voiceKey = voiceKey
        self.sungMidi = sungMidi
        self.inputLevelDb = inputLevelDb
        self.playing = playing
        self.recording = recording
        self.beat = beat
        self.pass = pass
        self.micState = micState
        self.lastHeard = lastHeard
        self.training = training
        if let profile { self.profile = profile }
        lastError = error
        canUndo = true
    }
    #endif
}
