// VoiceLoop Pocket settings registry (AGENTS.md settings principle). Every
// value that affects the interface or the sound is defined here once, with
// the same id as its row in docs/projects/settings/SETTINGS_INDEX.md
// ("Pocket keyboard and voice song building" / "Song Builder"). Stored in
// UserDefaults; defaults apply when nothing is stored.

import Foundation

enum ClickMode: String, CaseIterable, Identifiable {
    case always, recording, off
    var id: String { rawValue }
}

enum ClassifierMode: String, CaseIterable, Identifiable {
    case auto, rules, trained
    var id: String { rawValue }
}

enum AppearanceMode: String, CaseIterable, Identifiable {
    case dark, system, light
    var id: String { rawValue }
    var label: String {
        switch self {
        case .dark: "Dark"
        case .system: "System"
        case .light: "Light"
        }
    }
}

final class PocketSettings: ObservableObject {
    private let store: UserDefaults

    init(store: UserDefaults = .standard) {
        self.store = store
    }

    private func value<T>(_ id: String, _ fallback: T) -> T {
        (store.object(forKey: id) as? T) ?? fallback
    }

    private func set(_ id: String, _ v: Any) {
        objectWillChange.send()
        store.set(v, forKey: id)
    }

    // New-song defaults (each song then keeps its own).
    var songBpm: Double { get { value("pocket.songBpm", 100.0) } set { set("pocket.songBpm", newValue) } }
    var grid: Grid { get { Grid(rawValue: value("pocket.quantizeSubdivision", "1/8")) ?? .eighth } set { set("pocket.quantizeSubdivision", newValue.rawValue) } }
    var tightness: Double { get { value("pocket.quantizeStrength", 0.65) } set { set("pocket.quantizeStrength", newValue) } }
    var loopBars: Int { get { value("songBuilder.loopBars", 2) } set { set("songBuilder.loopBars", newValue) } }
    var beatsPerBar: Int { get { value("songBuilder.beatsPerBar", 4) } set { set("songBuilder.beatsPerBar", newValue) } }
    var defaultKit: String { get { value("songBuilder.defaultKit", "studio") } set { set("songBuilder.defaultKit", newValue) } }

    // Transport.
    var countInBars: Int { get { value("songBuilder.countInBars", 1) } set { set("songBuilder.countInBars", newValue) } }
    var clickMode: ClickMode { get { ClickMode(rawValue: value("songBuilder.clickMode", "always")) ?? .always } set { set("songBuilder.clickMode", newValue.rawValue) } }
    var clickVolume: Double { get { value("songBuilder.clickVolume", 0.45) } set { set("songBuilder.clickVolume", newValue) } }
    var masterVolume: Double { get { value("songBuilder.masterVolume", 1.0) } set { set("songBuilder.masterVolume", newValue) } }
    /// Make-up gain into the master limiter (dB): louder on the phone speaker.
    var loudnessDb: Double { get { value("songBuilder.loudnessDb", 6.0) } set { set("songBuilder.loudnessDb", newValue) } }
    var undoDepth: Int { get { value("songBuilder.undoDepth", 50) } set { set("songBuilder.undoDepth", newValue) } }
    var earlyHitBeats: Double { get { value("songBuilder.earlyHitBeats", 0.5) } set { set("songBuilder.earlyHitBeats", newValue) } }

    // Touch.
    var padVelocity: Double { get { value("songBuilder.padVelocity", 0.85) } set { set("songBuilder.padVelocity", newValue) } }
    var melodyNoteBeats: Double { get { value("songBuilder.melodyNoteBeats", 0.5) } set { set("songBuilder.melodyNoteBeats", newValue) } }
    var touchLatencyMs: Double { get { value("songBuilder.touchLatencyMs", 0.0) } set { set("songBuilder.touchLatencyMs", newValue) } }
    var keyboardOctave: Int { get { value("keyboard.octave", 4) } set { set("keyboard.octave", newValue) } }
    var noteLabels: Bool { get { value("keyboard.noteLabels", true) } set { set("keyboard.noteLabels", newValue) } }

    // Audio session: "default" (mic gain on, full speaker) or "measurement" (flat, quieter on iPhone).
    var sessionMode: String { get { value("audio.ios.sessionMode", "default") } set { set("audio.ios.sessionMode", newValue) } }

    // Recording from the mic: "all" mutes every part, "others" all but the one recording, "none" keeps them.
    var voiceRecordMute: String { get { value("songBuilder.voiceRecord.mute", "all") } set { set("songBuilder.voiceRecord.mute", newValue) } }

    /// Hear voice drums / sung notes live through the phone speaker (off: only with headphones; the speaker leaks into the mic).
    var monitorOnSpeaker: Bool { get { value("songBuilder.voice.monitorOnSpeaker", false) } set { set("songBuilder.voice.monitorOnSpeaker", newValue) } }
    /// Keep the click while the mic records through the speaker (off: the count-in still plays).
    var clickOnSpeakerWhileMic: Bool { get { value("songBuilder.voiceRecord.clickOnSpeaker", false) } set { set("songBuilder.voiceRecord.clickOnSpeaker", newValue) } }

    // Voice → piano.
    /// Singing needs a little more than a drum hit, so room noise doesn't hold notes on.
    var voicePianoMinLevelDb: Double { micGateDb + 5 }
    var voicePianoMinClarity: Double { get { value("songBuilder.voicePiano.minClarity", 0.75) } set { set("songBuilder.voicePiano.minClarity", newValue) } }
    var voicePianoStartFrames: Int { get { value("songBuilder.voicePiano.startFrames", 2) } set { set("songBuilder.voicePiano.startFrames", newValue) } }
    var voicePianoReleaseFrames: Int { get { value("songBuilder.voicePiano.releaseFrames", 4) } set { set("songBuilder.voicePiano.releaseFrames", newValue) } }
    var voicePianoHysteresis: Double { get { value("songBuilder.voicePiano.hysteresisSemitones", 0.15) } set { set("songBuilder.voicePiano.hysteresisSemitones", newValue) } }
    var voicePianoTie: SnapTie { get { SnapTie(rawValue: value("songBuilder.voicePiano.tieBreak", "down")) ?? .down } set { set("songBuilder.voicePiano.tieBreak", newValue.rawValue) } }
    var voicePianoMinNoteSeconds: Double { get { value("songBuilder.voicePiano.minNoteSeconds", 0.08) } set { set("songBuilder.voicePiano.minNoteSeconds", newValue) } }
    /// Furthest the part's octave −/+ can shift the sung note, in semitones (steps of 12).
    var voicePianoMaxTranspose: Int { get { value("songBuilder.voicePiano.maxTranspose", 24) } set { set("songBuilder.voicePiano.maxTranspose", newValue) } }
    /// Quick key sets for the "Edit keys" mode (`songBuilder.voicePiano.keyPresets`): pitch classes 0 = C … 11 = B.
    static let voicePianoKeyPresets: [(id: String, label: String, keys: [Int])] = [
        ("all", "All", Array(0..<12)),
        ("cMajor", "C major", [0, 2, 4, 5, 7, 9, 11]),
        ("aMinorPent", "A minor pent.", [9, 0, 2, 4, 7]),
    ]

    /// Mic level meter (sensitivity control): left edge in dBFS; the right edge is 0 dBFS.
    var micMeterFloorDb: Double { get { value("songBuilder.micMeter.floorDb", -70.0) } set { set("songBuilder.micMeter.floorDb", newValue) } }

    // Voice drums.
    var voiceListen: Bool { get { value("songBuilder.voiceDrums.listen", true) } set { set("songBuilder.voiceDrums.listen", newValue) } }
    var voiceMonitor: Bool { get { value("songBuilder.voiceDrums.monitor", true) } set { set("songBuilder.voiceDrums.monitor", newValue) } }
    var classifier: ClassifierMode { get { ClassifierMode(rawValue: value("songBuilder.voiceDrums.classifier", "auto")) ?? .auto } set { set("songBuilder.voiceDrums.classifier", newValue.rawValue) } }
    /// Mic sensitivity 0…1 (the one slider): sets every loudness gate below. 1 hears whispers, 0 only loud sounds.
    var micSensitivity: Double { get { value("songBuilder.micSensitivity", 0.75) } set { set("songBuilder.micSensitivity", newValue) } }
    /// Quietest sound (dBFS) that counts, from the sensitivity: 0 → -25 dB, 0.75 → -55 dB, 1 → -65 dB.
    var micGateDb: Double { PocketSettings.gateDb(sensitivity: micSensitivity) }
    /// The sensitivity → gate formula (also used to preview a slider drag).
    static func gateDb(sensitivity: Double) -> Double { -25 - 40 * min(1, max(0, sensitivity)) }
    var minLevelDb: Double { micGateDb }
    var minRiseDb: Double { get { value("songBuilder.voiceDrums.minRiseDb", 5.0) } set { set("songBuilder.voiceDrums.minRiseDb", newValue) } }
    /// Engine onset detector (analysis.onsets.*), sent while listening.
    var onsetRiseDb: Double { get { value("songBuilder.voiceDrums.onsetRiseDb", 6.0) } set { set("songBuilder.voiceDrums.onsetRiseDb", newValue) } }
    var onsetFloorDb: Double { micGateDb - 3 }
    /// Quietest voice hit still plays at this velocity (0…1).
    var minVelocity: Double { get { value("songBuilder.voiceDrums.minVelocity", 0.65) } set { set("songBuilder.voiceDrums.minVelocity", newValue) } }
    var voiceLatencyMs: Double { get { value("songBuilder.voiceDrums.latencyMs", 0.0) } set { set("songBuilder.voiceDrums.latencyMs", newValue) } }
    var trainingHits: Int { get { value("songBuilder.voiceDrums.trainingHits", 5) } set { set("songBuilder.voiceDrums.trainingHits", newValue) } }

    // Look and feel (Song Builder screen).
    var appearance: AppearanceMode { get { AppearanceMode(rawValue: value("songBuilder.appearance", "dark")) ?? .dark } set { set("songBuilder.appearance", newValue.rawValue) } }
    var haptics: Bool { get { value("songBuilder.haptics", true) } set { set("songBuilder.haptics", newValue) } }
    var padFlashMs: Double { get { value("songBuilder.padFlashMs", 160.0) } set { set("songBuilder.padFlashMs", newValue) } }
    var gridMinStepPt: Double { get { value("songBuilder.gridMinStepPt", 18.0) } set { set("songBuilder.gridMinStepPt", newValue) } }
    /// Colour ids: `songBuilder.color.<key>` with keys kick, snare, hat, clap, openHat, drums, melody, record.
    static let defaultColors: [String: String] = [
        "kick": "#FF8A3D", "snare": "#4C9DFF", "hat": "#34D399", "clap": "#B57CFF", "openHat": "#2CD3D3",
        "drums": "#FF5C7A", "melody": "#FFC145", "record": "#FF3B4E",
    ]
    func colorHex(_ key: String) -> String { value("songBuilder.color.\(key)", PocketSettings.defaultColors[key] ?? "#8E8E93") }

    var rules: RuleThresholds {
        RuleThresholds(
            kickMinLow: value("songBuilder.voiceDrums.kickMinLow", 0.3),
            kickMaxCentroidHz: value("songBuilder.voiceDrums.kickMaxCentroidHz", 900.0),
            kickMaxZcr: value("songBuilder.voiceDrums.kickMaxZcr", 0.06),
            kickMaxHigh: value("songBuilder.voiceDrums.kickMaxHigh", 0.15),
            hatMinHigh: value("songBuilder.voiceDrums.hatMinHigh", 0.45),
            hatMinCentroidHz: value("songBuilder.voiceDrums.hatMinCentroidHz", 3200.0)
        )
    }
}
