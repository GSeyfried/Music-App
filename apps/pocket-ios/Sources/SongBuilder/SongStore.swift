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
                try timed("start_input") { _ = try bridge.call("start_input", ["deviceId": NSNull()]) }
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

    func addPart(_ kind: PartKind) {
        var id = 0
        edit { id = $0.addPart(kind, kit: settings.defaultKit) }
        select(id)
    }

    func deletePart(_ id: Int) {
        edit { s in s.parts.removeAll { $0.id == id } }
    }

    // MARK: Engine sync

    /// Skip-pass marks for events just heard live while recording (not played again this pass).
    private var heardPass: [Int: Int64] = [:]

    private func syncEngine() {
        guard let engine, engineReady else { return }
        let s = song
        let parts: [[String: Any]] = s.audibleParts.map { part in
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
        let click = settings.clickMode == .always || (settings.clickMode == .recording && recording)
        let spec: [String: Any] = [
            "bpm": s.bpm, "beatsPerBar": s.beatsPerBar, "loopBeats": s.loopBeats,
            "click": click, "clickGain": settings.clickVolume, "masterGain": settings.masterVolume, "parts": parts,
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
        playing = false
        recording = false
        beat = 0
        pass = -1
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
        guard let part = selectedPart, part.kind == .drums else { return }
        trigger(part.id, lane: lane, velocity: settings.padVelocity)
        flashLane(lane)
        let v = settings.padVelocity
        record(inputFrame: nil, extraMs: settings.touchLatencyMs) { s, beat in
            s.addHit(part.id, lane: lane, beat: beat, velocity: v, source: .pad)
        }
    }

    func tapKey(_ midi: Int) {
        guard let part = selectedPart, part.kind == .melody, let engine else { return }
        let length = settings.melodyNoteBeats
        try? engine.call("song_trigger", ["trigger": ["partId": part.id, "midi": midi, "velocity": 0.85, "lengthBeats": length]])
        record(inputFrame: nil, extraMs: settings.touchLatencyMs) { s, beat in
            s.addNote(part.id, midi: midi, beat: beat, length: length, velocity: 0.85)
        }
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
        let want = engineReady && settings.voiceListen && (selectedPart?.kind == .drums || training != nil)
        guard want != listening, let engine else { return }
        listening = want
        let reasons = want ? ["songBuilder"] : [String]()
        _ = try? engine.call("set_input_needs", ["needs": ["reasons": reasons]])
        _ = try? engine.call("set_analysis_config", ["config": ["enabled": want]])
        if want { _ = try? engine.call("request_mic_permission") }
    }

    private func onHit(_ h: HitTimbre) {
        guard listening, VoiceDrums.accepts(h, minLevelDb: settings.minLevelDb, minRiseDb: settings.minRiseDb) else { return }
        if let t = training {
            profile = VoiceDrums.train(profile, lane: t.lane, h)
            SongStore.saveProfile(profile)
            flashLane(t.lane)
            if let part = selectedPart, part.kind == .drums { trigger(part.id, lane: t.lane, velocity: 0.8) }
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
        guard let part = selectedPart, part.kind == .drums else { return }
        let lane = VoiceDrums.classify(h, mode: settings.classifier, profile: profile, rules: settings.rules, minExamples: settings.trainingHits)
        lastHeard = (lane, h.centroidHz)
        flashLane(lane)
        let velocity = VoiceDrums.velocity(levelDb: h.levelDb, minLevelDb: settings.minLevelDb)
        if settings.voiceMonitor { trigger(part.id, lane: lane, velocity: velocity) }
        record(inputFrame: h.frame, extraMs: settings.voiceLatencyMs) { s, beat in
            s.addHit(part.id, lane: lane, beat: beat, velocity: velocity, source: .voice)
        }
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
                     training: (lane: DrumLane, count: Int)? = nil, profile: VoiceProfile? = nil, error: String? = nil) {
        guard !persists else { return }
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
