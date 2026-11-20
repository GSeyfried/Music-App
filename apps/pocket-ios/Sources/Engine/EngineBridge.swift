// Swift side of the Rust engine (crates/voiceloop-ffi). Commands go in as
// JSON by name (the same names as the desktop Tauri commands); events come
// back as JSON and are decoded into the few types Pocket uses. No audio
// crosses this boundary.

import Foundation

struct EngineError: Error, Decodable, CustomStringConvertible {
    let kind: String?
    let message: String?
    var description: String { message ?? kind ?? "engine error" }
}

/// Mirrors voiceloop-analysis `percussion::HitTimbre`.
struct HitTimbre: Decodable, Equatable {
    let frame: UInt64
    let sampleRate: Int
    let levelDb: Double
    let riseDb: Double
    let low: Double
    let mid: Double
    let high: Double
    let centroidHz: Double
    let zeroCrossingRate: Double
    let decayDb: Double
}

struct Onset: Decodable { let frame: UInt64; let levelDb: Double; let riseDb: Double }

struct InputStatus: Decodable { let mic: String; let reasons: [String] }

/// The parts of `EngineStatus` Pocket shows.
struct EngineStatus: Decodable {
    let capturing: Bool
    let looper: Bool
    let analysis: Bool
    let micPermission: String?
    let input: InputStatus?
}

struct PitchFrameLite: Decodable { let frame: UInt64 }
struct PitchUpdate: Decodable { let frames: [PitchFrameLite]; let sampleRate: Int }

struct LevelReading: Decodable { let rmsDb: Double?; let peakDb: Double? }

enum EngineEvent {
    case status(EngineStatus)
    case hit(HitTimbre)
    case onset(Onset)
    case pitch(PitchUpdate)
    case level(LevelReading)
    case songPosition(SongPosition)
    case warning(String)
    case error(String)
    case other(String)
}

/// Mirrors `voiceloop_engine::song::SongPosition`.
struct SongPosition: Decodable, Equatable {
    let playing: Bool
    /// Beat in the loop, 0..<loopBeats; negative during the count-in.
    let beat: Double
    let pass: Int64
    /// Output frame at which beat 0 of pass 0 sounded (heard-time anchor).
    let outputFrame: UInt64?
}

private struct Envelope: Decodable {
    let type: String
}

private struct Payload<T: Decodable>: Decodable { let payload: T }

final class EngineBridge: @unchecked Sendable {
    private var handle: OpaquePointer?
    private let onEvent: (EngineEvent, Double) -> Void

    /// `onEvent(event, arrivalSeconds)` is called on the main queue. `arrivalSeconds`
    /// is the host-clock time (ProcessInfo.systemUptime) the event reached Swift.
    init(onEvent: @escaping (EngineEvent, Double) -> Void) {
        self.onEvent = onEvent
        let context = Unmanaged.passUnretained(self).toOpaque()
        NSLog("[pocket] vl_engine_create …")
        handle = vl_engine_create({ context, json in
            guard let context, let json else { return }
            let bridge = Unmanaged<EngineBridge>.fromOpaque(context).takeUnretainedValue()
            let arrival = ProcessInfo.processInfo.systemUptime
            let data = Data(bytes: json, count: strlen(json))
            guard let event = EngineBridge.decode(data) else { return }
            DispatchQueue.main.async { bridge.onEvent(event, arrival) }
        }, context)
        NSLog("[pocket] vl_engine_create done")
    }

    deinit {
        if let handle { vl_engine_destroy(handle) }
    }

    private static func decode(_ data: Data) -> EngineEvent? {
        let decoder = JSONDecoder()
        guard let env = try? decoder.decode(Envelope.self, from: data) else { return nil }
        func p<T: Decodable>(_: T.Type) -> T? { try? decoder.decode(Payload<T>.self, from: data).payload }
        switch env.type {
        case "status": return p(EngineStatus.self).map(EngineEvent.status)
        case "hit": return p(HitTimbre.self).map(EngineEvent.hit)
        case "onset": return p(Onset.self).map(EngineEvent.onset)
        case "pitch": return p(PitchUpdate.self).map(EngineEvent.pitch)
        case "level": return p(LevelReading.self).map(EngineEvent.level)
        case "songPosition": return p(SongPosition.self).map(EngineEvent.songPosition)
        case "warning", "error":
            let text = String(data: data, encoding: .utf8) ?? env.type
            return env.type == "warning" ? .warning(text) : .error(text)
        default: return .other(env.type)
        }
    }

    /// Run a command; returns the raw JSON `ok` value.
    @discardableResult
    func call(_ command: String, _ args: [String: Any] = [:]) throws -> Any? {
        guard let handle else { throw EngineError(kind: "unavailable", message: "engine not running") }
        let argsData = try JSONSerialization.data(withJSONObject: args)
        let argsText = String(data: argsData, encoding: .utf8) ?? "{}"
        guard let raw = vl_engine_call(handle, command, argsText) else {
            throw EngineError(kind: "invalid", message: "no reply")
        }
        defer { vl_string_free(raw) }
        let data = Data(bytes: raw, count: strlen(raw))
        let obj = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]) as? [String: Any]
        if let err = obj?["error"] {
            let errData = try JSONSerialization.data(withJSONObject: err, options: [.fragmentsAllowed])
            throw (try? JSONDecoder().decode(EngineError.self, from: errData)) ?? EngineError(kind: nil, message: "\(err)")
        }
        return obj?["ok"] ?? nil
    }

    /// Decode a command's result.
    func call<T: Decodable>(_ command: String, _ args: [String: Any] = [:], as: T.Type) throws -> T {
        let value = try call(command, args) ?? NSNull()
        let data = try JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed])
        return try JSONDecoder().decode(T.self, from: data)
    }

    /// Encode a Codable argument into the dictionary form `call` takes.
    static func json<T: Encodable>(_ value: T) -> Any {
        let data = (try? JSONEncoder().encode(value)) ?? Data("null".utf8)
        return (try? JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])) ?? NSNull()
    }
}
