// Song Builder data model (docs/projects/song-builder/README.md).
//
// A song is one loop. Parts keep events at their *raw* beat: where the
// performer played them. Tightness is applied only when the song is sent to
// the engine or drawn, so it can change at any time and always starts again
// from the untouched performance. Nothing here touches audio.

import Foundation

enum DrumLane: String, Codable, CaseIterable, Identifiable {
    case kick, snare, hat, clap, openHat
    var id: String { rawValue }
    var label: String {
        switch self {
        case .kick: "Kick"
        case .snare: "Snare"
        case .hat: "Hi-hat"
        case .clap: "Clap"
        case .openHat: "Open hat"
        }
    }
    /// What to say for voice drums.
    var voiceHint: String? {
        switch self {
        case .kick: "“b” / “boom”"
        case .snare: "“psh” / “k”"
        case .hat: "“ts” / “t”"
        default: nil
        }
    }
    static let voiceLanes: [DrumLane] = [.kick, .snare, .hat]
}

enum Grid: String, Codable, CaseIterable, Identifiable {
    case quarter = "1/4", eighth = "1/8", triplet = "1/8T", sixteenth = "1/16"
    var id: String { rawValue }
    var stepsPerBeat: Int {
        switch self {
        case .quarter: 1
        case .eighth: 2
        case .triplet: 3
        case .sixteenth: 4
        }
    }
    var label: String { self == .triplet ? "1/8 trip" : rawValue }
}

enum HitSource: String, Codable { case pad, voice, starter }

struct DrumHit: Codable, Identifiable, Equatable {
    var id: Int
    var lane: DrumLane
    /// Raw beat in the loop, 0 ≤ beat < loop length. Never overwritten by Tightness.
    var beat: Double
    var velocity: Double
    var source: HitSource
}

struct MelodyNote: Codable, Identifiable, Equatable {
    var id: Int
    var midi: Int
    var beat: Double
    var length: Double
    var velocity: Double
}

enum MelodyInstrument: String, Codable, CaseIterable, Identifiable {
    case piano, softSynth, bass
    var id: String { rawValue }
    var label: String {
        switch self {
        case .piano: "Piano"
        case .softSynth: "Soft synth"
        case .bass: "Bass"
        }
    }
}

enum PartKind: String, Codable { case drums, melody }

struct Part: Codable, Identifiable, Equatable {
    var id: Int
    var kind: PartKind
    var name: String
    var level: Double = 1
    var muted = false
    var solo = false
    var fx = "clean"
    var kit = "studio"
    var instrument: MelodyInstrument = .piano
    var hits: [DrumHit] = []
    var notes: [MelodyNote] = []

    var eventCount: Int { kind == .drums ? hits.count : notes.count }
}

struct Song: Codable, Equatable {
    var version = 1
    var name = "New song"
    var bpm: Double
    var beatsPerBar: Int
    var bars: Int
    var grid: Grid
    /// 0 = natural timing, 1 = fully on the grid.
    var tightness: Double
    var parts: [Part] = []
    var nextId = 1

    var loopBeats: Double { Double(bars * beatsPerBar) }
    var secondsPerBeat: Double { 60 / max(1, bpm) }

    static func new(settings: PocketSettings) -> Song {
        var s = Song(bpm: settings.songBpm, beatsPerBar: settings.beatsPerBar, bars: settings.loopBars, grid: settings.grid, tightness: settings.tightness)
        _ = s.addPart(.drums, kit: settings.defaultKit)
        return s
    }

    // MARK: Timing

    static func wrap(_ beat: Double, _ length: Double) -> Double {
        guard length > 0 else { return 0 }
        let w = beat.truncatingRemainder(dividingBy: length)
        return w < 0 ? w + length : w
    }

    /// Where a raw beat plays after Tightness: blended toward the nearest grid line, wrapped into the loop.
    func tightened(_ raw: Double) -> Double {
        let step = 1 / Double(grid.stepsPerBeat)
        let snapped = (raw / step).rounded() * step
        let amount = min(1, max(0, tightness))
        return Song.wrap(raw + (snapped - raw) * amount, loopBeats)
    }

    /// The tightened position of a raw beat as an absolute beat near `raw` (for loop-pass bookkeeping).
    func tightenedAbsolute(_ raw: Double) -> Double {
        let length = loopBeats
        var d = tightened(raw) - Song.wrap(raw, length)
        if d > length / 2 { d -= length }
        if d < -length / 2 { d += length }
        return raw + d
    }

    var audibleParts: [Part] {
        let soloed = parts.contains { $0.solo }
        return parts.filter { soloed ? $0.solo : !$0.muted }
    }

    // MARK: Editing

    @discardableResult
    mutating func addPart(_ kind: PartKind, kit: String = "studio") -> Int {
        let id = nextId
        nextId += 1
        let count = parts.filter { $0.kind == kind }.count + 1
        let name = kind == .drums ? (count == 1 ? "Voice drums" : "Voice drums \(count)") : (count == 1 ? "Melody" : "Melody \(count)")
        parts.append(Part(id: id, kind: kind, name: name, kit: kit))
        return id
    }

    mutating func update(_ id: Int, _ change: (inout Part) -> Void) {
        guard let i = parts.firstIndex(where: { $0.id == id }) else { return }
        change(&parts[i])
    }

    @discardableResult
    mutating func addHit(_ partId: Int, lane: DrumLane, beat: Double, velocity: Double, source: HitSource) -> Int {
        let id = nextId
        nextId += 1
        let b = Song.wrap(beat, loopBeats)
        update(partId) { $0.hits.append(DrumHit(id: id, lane: lane, beat: b, velocity: velocity, source: source)) }
        return id
    }

    @discardableResult
    mutating func addNote(_ partId: Int, midi: Int, beat: Double, length: Double, velocity: Double) -> Int {
        let id = nextId
        nextId += 1
        let b = Song.wrap(beat, loopBeats)
        update(partId) { $0.notes.append(MelodyNote(id: id, midi: midi, beat: b, length: length, velocity: velocity)) }
        return id
    }

    mutating func duplicatePart(_ id: Int) {
        guard let i = parts.firstIndex(where: { $0.id == id }) else { return }
        var copy = parts[i]
        copy.id = nextId
        nextId += 1
        copy.name += " copy"
        copy.solo = false
        copy.hits = copy.hits.map { var h = $0; h.id = nextId; nextId += 1; return h }
        copy.notes = copy.notes.map { var n = $0; n.id = nextId; nextId += 1; return n }
        parts.insert(copy, at: i + 1)
    }

    /// Change the loop length. Growing repeats the existing material; shrinking drops what falls outside.
    mutating func setBars(_ newBars: Int) {
        let oldLength = loopBeats
        bars = newBars
        let newLength = loopBeats
        guard newLength != oldLength else { return }
        for i in parts.indices {
            if newLength < oldLength {
                parts[i].hits.removeAll { $0.beat >= newLength }
                parts[i].notes.removeAll { $0.beat >= newLength }
            } else {
                let hits = parts[i].hits, notes = parts[i].notes
                var offset = oldLength
                while offset < newLength {
                    for h in hits where h.beat + offset < newLength {
                        var c = h; c.id = nextId; nextId += 1; c.beat += offset; parts[i].hits.append(c)
                    }
                    for n in notes where n.beat + offset < newLength {
                        var c = n; c.id = nextId; nextId += 1; c.beat += offset; parts[i].notes.append(c)
                    }
                    offset += oldLength
                }
            }
        }
    }

    mutating func applyStarter(_ beat: StarterBeat, to partId: Int) {
        var hits: [DrumHit] = []
        for bar in 0..<bars {
            for (lane, steps) in beat.steps {
                for step in steps {
                    let inBar = Double(step) / 4
                    guard inBar < Double(beatsPerBar) else { continue }
                    let b = Double(bar * beatsPerBar) + inBar
                    let soft = beat.soft.contains(step) || (lane == .hat && step % 4 != 0)
                    hits.append(DrumHit(id: nextId, lane: lane, beat: b, velocity: soft ? 0.6 : 0.9, source: .starter))
                    nextId += 1
                }
            }
        }
        update(partId) { $0.hits = hits }
    }
}

struct StarterBeat: Identifiable {
    let id: String
    let label: String
    /// One 4/4 bar in sixteenth steps (0…15) per lane.
    let steps: [(DrumLane, [Int])]
    var soft: [Int] = []

    static let all: [StarterBeat] = [
        StarterBeat(id: "basic", label: "Basic rock", steps: [(.kick, [0, 8]), (.snare, [4, 12]), (.hat, [0, 2, 4, 6, 8, 10, 12, 14])]),
        StarterBeat(id: "boomBap", label: "Boom bap", steps: [(.kick, [0, 7, 10]), (.snare, [4, 12]), (.hat, [0, 2, 4, 6, 8, 10, 12, 14]), (.openHat, [14])], soft: [7]),
        StarterBeat(id: "fourFloor", label: "Four on the floor", steps: [(.kick, [0, 4, 8, 12]), (.clap, [4, 12]), (.openHat, [2, 6, 10, 14]), (.hat, [0, 4, 8, 12])]),
        StarterBeat(id: "halfTime", label: "Half time", steps: [(.kick, [0, 3]), (.snare, [8]), (.hat, [0, 2, 4, 6, 8, 10, 12, 14])]),
    ]
}

/// Kits and FX presets offered in the app. The sounds themselves are defined
/// once in the engine (crates/voiceloop-engine/src/song.rs: DRUM_KITS, FX_PRESETS).
enum Catalog {
    static let kits: [(id: String, label: String)] = [("studio", "Studio"), ("808", "808"), ("lofi", "Lo-fi")]
    static let fx: [(id: String, label: String, help: String)] = [
        ("clean", "Clean", "No processing."),
        ("room", "Room", "A small room around the sound."),
        ("hall", "Big hall", "Long, wide reverb."),
        ("echo", "Echo", "Dotted-eighth echoes that follow the tempo."),
        ("punch", "Punch", "Compressed and a little driven: louder, tighter hits."),
        ("lofi", "Lo-fi", "Crunchy, dull and narrow, like an old sampler."),
        ("radio", "Radio", "Thin telephone/radio band."),
    ]
}
