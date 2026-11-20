// Voice drums: which drum a mouth sound means (port of the tested
// src/songbuilder/classify.ts). The engine describes each attack
// (HitTimbre); "rules" works untrained, "trained" uses the user's own
// sounds, "auto" switches once each sound has enough examples.

import Foundation

struct RuleThresholds {
    var kickMinLow: Double
    var kickMaxCentroidHz: Double
    var hatMinHigh: Double
    var hatMinCentroidHz: Double
}

struct LaneProfile: Codable, Equatable {
    var count: Int
    var mean: [Double]
}

typealias VoiceProfile = [DrumLane: LaneProfile]

enum VoiceDrums {
    /// Feature vector, each dimension roughly 0…1 for voice sounds.
    static func features(_ h: HitTimbre) -> [Double] {
        let centroid = log2(max(50, h.centroidHz) / 50) / 8
        return [h.low, h.mid, h.high, centroid, min(1, h.zeroCrossingRate * 2), min(1, max(0, -h.decayDb / 30))]
    }

    static func byRules(_ h: HitTimbre, _ t: RuleThresholds) -> DrumLane {
        if h.low >= t.kickMinLow || h.centroidHz < t.kickMaxCentroidHz { return .kick }
        if h.high >= t.hatMinHigh || h.centroidHz > t.hatMinCentroidHz { return .hat }
        return .snare
    }

    static func byProfile(_ h: HitTimbre, _ profile: VoiceProfile) -> DrumLane? {
        let f = features(h)
        var best: (DrumLane, Double)?
        for lane in DrumLane.voiceLanes {
            guard let p = profile[lane], p.count > 0 else { continue }
            let d = sqrt(zip(f, p.mean).map { ($0 - $1) * ($0 - $1) }.reduce(0, +))
            if best == nil || d < best!.1 { best = (lane, d) }
        }
        return best?.0
    }

    static func ready(_ profile: VoiceProfile, minExamples: Int) -> Bool {
        DrumLane.voiceLanes.allSatisfy { (profile[$0]?.count ?? 0) >= minExamples }
    }

    static func classify(_ h: HitTimbre, mode: ClassifierMode, profile: VoiceProfile, rules: RuleThresholds, minExamples: Int) -> DrumLane {
        let trained = mode == .trained || (mode == .auto && ready(profile, minExamples: minExamples))
        if trained, let lane = byProfile(h, profile) { return lane }
        return byRules(h, rules)
    }

    static func train(_ profile: VoiceProfile, lane: DrumLane, _ h: HitTimbre) -> VoiceProfile {
        var p = profile
        let f = features(h)
        if let prev = p[lane], prev.count > 0 {
            let count = prev.count + 1
            p[lane] = LaneProfile(count: count, mean: zip(prev.mean, f).map { $0 + ($1 - $0) / Double(count) })
        } else {
            p[lane] = LaneProfile(count: 1, mean: f)
        }
        return p
    }

    static func accepts(_ h: HitTimbre, minLevelDb: Double, minRiseDb: Double) -> Bool {
        h.levelDb >= minLevelDb && h.riseDb >= minRiseDb
    }

    static func velocity(levelDb: Double, minLevelDb: Double) -> Double {
        let span = max(1, -6 - minLevelDb)
        return min(1, max(0.35, 0.35 + 0.65 * (levelDb - minLevelDb) / span))
    }
}
