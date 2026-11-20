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
    /// What to say for voice drums (Ba, Da, K, Ts).
    var voiceHint: String? {
        switch self {
        case .kick: "“ba” / “boots”"
        case .clap: "“da”"
        case .snare: "“k” / “psh”"
        case .hat: "“ts”"
        default: nil
        }
    }
    /// Voice sounds in training order: Ba, Da, K, Ts.
    static let voiceLanes: [DrumLane] = [.kick, .clap, .snare, .hat]
    /// Sounds the untrained rules can tell apart; "Da" is recognised only once trained.
    static let coreVoiceLanes: [DrumLane] = [.kick, .snare, .hat]
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

/// Where a part's events come from. Touch parts keep the song playing while
/// you add to them; voice parts use the microphone (the others can auto-mute).
enum PartInput: String, Codable { case touch, voice }

/// One-tap "add a track" templates (docs/projects/track-system/README.md).
enum TrackTemplate: String, CaseIterable, Identifiable {
    case voiceDrums, voicePiano, drumPads, piano, voiceRaw, voiceTune
    var id: String { rawValue }
    var label: String {
        switch self {
        case .voiceDrums: "Voice drums"
        case .voicePiano: "Voice → piano"
        case .drumPads: "Drum pads"
        case .piano: "Piano"
        case .voiceRaw: "Voice"
        case .voiceTune: "Voice + tune"
        }
    }
    var detail: String {
        switch self {
        case .voiceDrums: "Beatbox Ba, Da, K, Ts into drums"
        case .voicePiano: "Sing; plays the nearest enabled key"
        case .drumPads: "Tap drums"
        case .piano: "Tap keys"
        case .voiceRaw: "Record your voice with filters"
        case .voiceTune: "Your voice, pitch-corrected"
        }
    }
    var systemImage: String {
        switch self {
        case .voiceDrums: "waveform"
        case .voicePiano: "music.mic"
        case .drumPads: "square.grid.2x2.fill"
        case .piano: "pianokeys"
        case .voiceRaw: "mic.fill"
        case .voiceTune: "wand.and.stars"
        }
    }
    var kind: PartKind { (self == .voiceDrums || self == .drumPads) ? .drums : .melody }
    var input: PartInput { (self == .drumPads || self == .piano) ? .touch : .voice }
    /// Voice and Voice + tune need recorded audio in the engine (PENDING).
    var available: Bool { self != .voiceRaw && self != .voiceTune }
}

/// How a sung pitch between two enabled keys is resolved (`songBuilder.voicePiano.tieBreak`).
enum SnapTie: String, Codable, CaseIterable { case down, up }

enum KeySnap {
    static let names = ["C", "C♯", "D", "E♭", "E", "F", "F♯", "G", "A♭", "A", "B♭", "B"]

    /// The enabled key nearest to a fractional MIDI pitch. With no key enabled, every key counts.
    /// `current` (the note already sounding) is kept until another enabled key is nearer by more than
    /// `hysteresis` semitones, so the note doesn't flicker at the boundary. Exact ties go to `tie`.
    static func snap(_ midi: Double, enabled: [Bool], tie: SnapTie, current: Int? = nil, hysteresis: Double = 0.15) -> Int {
        let on = enabled.count == 12 && enabled.contains(true) ? enabled : Array(repeating: true, count: 12)
        let nearest = nearestKey(midi, on, tie)
        if let c = current, on[((c % 12) + 12) % 12], abs(midi - Double(c)) <= abs(midi - Double(nearest)) + hysteresis { return c }
        return nearest
    }

    private static func nearestKey(_ midi: Double, _ on: [Bool], _ tie: SnapTie) -> Int {
        let base = Int(midi.rounded(.down))
        var best: (note: Int, dist: Double)?
        for n in (base - 12)...(base + 13) where on[((n % 12) + 12) % 12] {
            let d = abs(Double(n) - midi)
            if let b = best {
                if d < b.dist - 1e-6 || (abs(d - b.dist) <= 1e-6 && (tie == .up ? n > b.note : n < b.note)) { best = (n, d) }
            } else {
                best = (n, d)
            }
        }
        return best?.note ?? Int(midi.rounded())
    }
}

struct Part: Codable, Identifiable, Equatable {
    var id: Int
    var kind: PartKind
    var name: String
    var level: Double = 1
    var muted = false
    var solo = false
    /// FX presets run in order, joined with "+" (e.g. "room+punch"); the engine stacks up to 3.
    var fx = "clean"
    var kit = "studio"
    var instrument: MelodyInstrument = .piano
    var hits: [DrumHit] = []
    var notes: [MelodyNote] = []
    var input: PartInput = .voice
    /// Voice → piano: which of the 12 pitch classes (C…B) may sound.
    var enabledKeys: [Bool] = Array(repeating: true, count: 12)
    /// Voice → piano: semitones added to the sung note (e.g. -12 = an octave down).
    var transpose = 0

    var eventCount: Int { kind == .drums ? hits.count : notes.count }
    var fxList: [String] { fx.split(separator: "+").map(String.init).filter { $0 != "clean" } }

    init(id: Int, kind: PartKind, name: String, kit: String = "studio", input: PartInput = .voice) {
        self.id = id
        self.kind = kind
        self.name = name
        self.kit = kit
        self.input = input
    }

    // Lenient decoding: songs saved before a field existed still load.
    private enum CodingKeys: String, CodingKey {
        case id, kind, name, level, muted, solo, fx, kit, instrument, hits, notes, input, enabledKeys, transpose
    }

    init(from d: Decoder) throws {
        let c = try d.container(keyedBy: CodingKeys.self)
        id = try c.decode(Int.self, forKey: .id)
        kind = try c.decode(PartKind.self, forKey: .kind)
        name = try c.decode(String.self, forKey: .name)
        level = try c.decodeIfPresent(Double.self, forKey: .level) ?? 1
        muted = try c.decodeIfPresent(Bool.self, forKey: .muted) ?? false
        solo = try c.decodeIfPresent(Bool.self, forKey: .solo) ?? false
        fx = try c.decodeIfPresent(String.self, forKey: .fx) ?? "clean"
        kit = try c.decodeIfPresent(String.self, forKey: .kit) ?? "studio"
        instrument = try c.decodeIfPresent(MelodyInstrument.self, forKey: .instrument) ?? .piano
        hits = try c.decodeIfPresent([DrumHit].self, forKey: .hits) ?? []
        notes = try c.decodeIfPresent([MelodyNote].self, forKey: .notes) ?? []
        // Before inputs existed, drum parts were voice drums and melody parts were touch piano.
        input = try c.decodeIfPresent(PartInput.self, forKey: .input) ?? (kind == .drums ? .voice : .touch)
        let keys = try c.decodeIfPresent([Bool].self, forKey: .enabledKeys) ?? []
        enabledKeys = keys.count == 12 ? keys : Array(repeating: true, count: 12)
        transpose = try c.decodeIfPresent(Int.self, forKey: .transpose) ?? 0
    }
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
        _ = s.addPart(.voiceDrums, kit: settings.defaultKit)
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
    mutating func addPart(_ template: TrackTemplate, kit: String = "studio") -> Int {
        let id = nextId
        nextId += 1
        let same = parts.filter { $0.kind == template.kind && $0.input == template.input }.count + 1
        let name = same == 1 ? template.label : "\(template.label) \(same)"
        var part = Part(id: id, kind: template.kind, name: name, kit: kit, input: template.input)
        if template == .voicePiano { part.instrument = .softSynth }
        parts.append(part)
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
