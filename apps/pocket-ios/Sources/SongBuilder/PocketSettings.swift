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
    var masterVolume: Double { get { value("songBuilder.masterVolume", 0.85) } set { set("songBuilder.masterVolume", newValue) } }
    var undoDepth: Int { get { value("songBuilder.undoDepth", 50) } set { set("songBuilder.undoDepth", newValue) } }
    var earlyHitBeats: Double { get { value("songBuilder.earlyHitBeats", 0.5) } set { set("songBuilder.earlyHitBeats", newValue) } }

    // Touch.
    var padVelocity: Double { get { value("songBuilder.padVelocity", 0.85) } set { set("songBuilder.padVelocity", newValue) } }
    var melodyNoteBeats: Double { get { value("songBuilder.melodyNoteBeats", 0.5) } set { set("songBuilder.melodyNoteBeats", newValue) } }
    var touchLatencyMs: Double { get { value("songBuilder.touchLatencyMs", 0.0) } set { set("songBuilder.touchLatencyMs", newValue) } }
    var keyboardOctave: Int { get { value("keyboard.octave", 4) } set { set("keyboard.octave", newValue) } }
    var noteLabels: Bool { get { value("keyboard.noteLabels", true) } set { set("keyboard.noteLabels", newValue) } }

    // Voice drums.
    var voiceListen: Bool { get { value("songBuilder.voiceDrums.listen", true) } set { set("songBuilder.voiceDrums.listen", newValue) } }
    var voiceMonitor: Bool { get { value("songBuilder.voiceDrums.monitor", true) } set { set("songBuilder.voiceDrums.monitor", newValue) } }
    var classifier: ClassifierMode { get { ClassifierMode(rawValue: value("songBuilder.voiceDrums.classifier", "auto")) ?? .auto } set { set("songBuilder.voiceDrums.classifier", newValue.rawValue) } }
    var minLevelDb: Double { get { value("songBuilder.voiceDrums.minLevelDb", -42.0) } set { set("songBuilder.voiceDrums.minLevelDb", newValue) } }
    var minRiseDb: Double { get { value("songBuilder.voiceDrums.minRiseDb", 10.0) } set { set("songBuilder.voiceDrums.minRiseDb", newValue) } }
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
            kickMinLow: value("songBuilder.voiceDrums.kickMinLow", 0.5),
            kickMaxCentroidHz: value("songBuilder.voiceDrums.kickMaxCentroidHz", 450.0),
            hatMinHigh: value("songBuilder.voiceDrums.hatMinHigh", 0.45),
            hatMinCentroidHz: value("songBuilder.voiceDrums.hatMinCentroidHz", 3200.0)
        )
    }
}
