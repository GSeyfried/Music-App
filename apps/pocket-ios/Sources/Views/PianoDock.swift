// Melody performance area: one chromatic octave (7 white + 5 black keys).
//
// Touch piano: keys sound while held and release on lift (several fingers
// at once); octave −/+ moves the keyboard.
//
// Voice → piano: you sing and the nearest enabled key plays. The keyboard
// shows the key sounding now and doubles as the enabled-keys editor ("Edit
// keys": tap a key to turn it on or off; quick sets All / C major / A minor
// pentatonic). Octave −/+ transposes the sung note in steps of 12.

import SwiftUI

struct PianoDock: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    let part: Part
    @State private var editing: Bool

    init(part: Part, startEditing: Bool = false) {
        self.part = part
        _editing = State(initialValue: startEditing && part.input == .voice)
    }

    private var tint: Color { settings.partColor(.melody) }
    private var voice: Bool { part.input == .voice }

    var body: some View {
        VStack(spacing: 8) {
            if voice {
                voiceHeader
                if editing { presetRow } else { MicSensitivityControl(); SpeakerMonitorHint() }
                transposeRow
            } else {
                touchHeader
            }
            keyboard
        }
        .animation(.spring(response: 0.3, dampingFraction: 0.85), value: editing)
    }

    // MARK: Keyboard

    /// Touch: the chosen octave. Voice: the octave of the key sounding now (else the chosen one).
    private var base: Int {
        if voice, let k = store.voiceKey { return (k / 12) * 12 }
        return (settings.keyboardOctave.clamped(1, 7) + 1) * 12
    }

    private var keyboard: some View {
        PianoKeyboard(base: base, labels: settings.noteLabels, used: Set(part.notes.map(\.midi)), tint: tint,
                      enabled: voice ? part.enabledKeys : nil,
                      sounding: voice ? store.voiceKey.map { (($0 % 12) + 12) % 12 } : nil,
                      editing: editing,
                      down: { midi in Haptics.key(settings); store.keyDown(midi) },
                      up: { midi in store.keyUp(midi) },
                      toggle: { pc in
                          Haptics.tick(settings)
                          store.setKeyEnabled(pc, !part.enabledKeys[pc])
                      })
            .frame(height: voice ? 150 : 168)
    }

    // MARK: Touch piano

    private var touchHeader: some View {
        let octave = settings.keyboardOctave.clamped(1, 7)
        return HStack(spacing: 6) {
            CircleIconButton(systemImage: "chevron.left", label: "Octave down", size: 36) {
                Haptics.tick(settings)
                settings.keyboardOctave = octave - 1
            }
            .disabled(octave <= 1)
            Text("\(NoteName.of(base))–\(NoteName.of(base + 11))")
                .font(.system(.headline, design: .rounded).weight(.bold))
                .monospacedDigit()
                .frame(minWidth: 72)
                .accessibilityLabel("Range \(NoteName.spokenOf(base)) to \(NoteName.spokenOf(base + 11))")
            CircleIconButton(systemImage: "chevron.right", label: "Octave up", size: 36) {
                Haptics.tick(settings)
                settings.keyboardOctave = octave + 1
            }
            .disabled(octave >= 7)
            Spacer(minLength: 0)
            Text("Hold to sustain")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
        }
    }

    // MARK: Voice → piano

    private var voiceHeader: some View {
        HStack(spacing: 8) {
            CapsuleToggle(label: "Voice", systemImage: settings.voiceListen ? "mic.fill" : "mic.slash.fill",
                          isOn: Binding(get: { settings.voiceListen }, set: { store.setListen($0) }), tint: .green)
                .accessibilityHint("Sing to play the nearest enabled key")
            VStack(alignment: .leading, spacing: 1) {
                MicStatusLine()
                singingReadout
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 0)
            Button {
                Haptics.tick(settings)
                editing.toggle()
            } label: {
                ChipLabel(systemImage: editing ? "checkmark" : "slider.horizontal.below.square.and.square.filled",
                          text: editing ? "Done" : "Edit keys", tint: editing ? tint : .primary, active: editing)
            }
            .buttonStyle(PressScale())
            .accessibilityLabel(editing ? "Done editing keys" : "Edit keys")
            .accessibilityHint("Choose which keys your voice can play")
        }
    }

    private var singingReadout: some View {
        Group {
            if editing {
                let n = part.enabledKeys.filter { $0 }.count
                Text(n == 0 ? "None on: every key plays" : "Tap keys · \(n) of 12 on")
                    .foregroundStyle(.secondary)
            } else if let sung = store.sungMidi {
                let snapped = store.voiceKey.map { " → \(NoteName.of($0))" } ?? ""
                Text("Singing: \(NoteName.of(Int(sung.rounded())))\(snapped)")
                    .foregroundStyle(tint)
                    .accessibilityLabel("Singing \(NoteName.spokenOf(Int(sung.rounded())))\(store.voiceKey.map { ", playing \(NoteName.spokenOf($0))" } ?? "")")
            } else {
                Text("Sing a note").foregroundStyle(.secondary)
            }
        }
        .font(.caption2.weight(.semibold))
        .lineLimit(1)
        .monospacedDigit()
    }

    private var presetRow: some View {
        HStack(spacing: 8) {
            ForEach(PocketSettings.voicePianoKeyPresets, id: \.id) { preset in
                let keys = (0..<12).map { preset.keys.contains($0) }
                let on = keys == part.enabledKeys
                Button {
                    Haptics.tick(settings)
                    store.edit { $0.update(part.id) { $0.enabledKeys = keys } }
                } label: {
                    ChipLabel(text: preset.label, tint: on ? tint : .primary, active: on)
                }
                .buttonStyle(PressScale())
                .accessibilityLabel("Keys: \(preset.label)")
                .accessibilityAddTraits(on ? .isSelected : [])
            }
            Spacer(minLength: 0)
        }
    }

    private var transposeRow: some View {
        let limit = max(12, settings.voicePianoMaxTranspose)
        let octaves = part.transpose / 12
        let label = part.transpose == 0 ? "As sung" : "\(octaves > 0 ? "+" : "−")\(abs(octaves)) oct"
        return HStack(spacing: 6) {
            CircleIconButton(systemImage: "minus", label: "Octave down", size: 36) { shift(-12) }
                .disabled(part.transpose - 12 < -limit)
            VStack(spacing: 0) {
                Text(label)
                    .font(.system(.subheadline, design: .rounded).weight(.bold))
                    .monospacedDigit()
                Text("Octave").font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
            }
            .frame(minWidth: 70)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("Octave shift")
            .accessibilityValue(part.transpose == 0 ? "None" : "\(octaves) octaves")
            CircleIconButton(systemImage: "plus", label: "Octave up", size: 36) { shift(12) }
                .disabled(part.transpose + 12 > limit)
            Spacer(minLength: 0)
            PocketMenu {
                Picker("Sound", selection: Binding(get: { part.instrument }, set: { v in store.edit { $0.update(part.id) { $0.instrument = v } } })) {
                    ForEach(MelodyInstrument.allCases) { Text($0.label).tag($0) }
                }
            } label: {
                ChipLabel(systemImage: "music.note", text: part.instrument.label, tint: tint, chevron: true)
            }
            .accessibilityLabel("Sound, \(part.instrument.label)")
        }
    }

    private func shift(_ semis: Int) {
        Haptics.tick(settings)
        store.edit { $0.update(part.id) { $0.transpose += semis } }
    }
}

/// Mic dot + state, shared by the drum and voice-piano docks.
struct MicStatusLine: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings

    var body: some View {
        HStack(spacing: 5) {
            Circle().fill(dot).frame(width: 7, height: 7)
            Text(text).font(.caption.weight(.semibold)).lineLimit(1)
        }
    }

    private var text: String {
        guard settings.voiceListen else { return "Voice off · tap to play" }
        switch store.micState {
        case "on": return "Listening"
        case "opening": return "Opening mic…"
        default: return store.engineReady ? "Mic off" : "Mic waiting for audio"
        }
    }

    private var dot: Color {
        guard settings.voiceListen else { return .secondary.opacity(0.5) }
        switch store.micState {
        case "on": return .green
        case "opening": return .yellow
        default: return .secondary.opacity(0.5)
        }
    }
}

struct PianoKeyboard: View {
    let base: Int
    let labels: Bool
    let used: Set<Int>
    let tint: Color
    /// Voice → piano: which pitch classes may sound (nil = touch piano, all keys normal).
    var enabled: [Bool]? = nil
    /// Voice → piano: pitch class sounding now.
    var sounding: Int? = nil
    var editing = false
    let down: (Int) -> Void
    let up: (Int) -> Void
    var toggle: (Int) -> Void = { _ in }

    private let whites = [0, 2, 4, 5, 7, 9, 11]
    /// Black key semitone and the white-key boundary it sits on.
    private let blacks: [(semi: Int, boundary: Int)] = [(1, 1), (3, 2), (6, 4), (8, 5), (10, 6)]

    var body: some View {
        GeometryReader { g in
            let w = g.size.width / 7
            let h = g.size.height
            ZStack(alignment: .topLeading) {
                ForEach(Array(whites.enumerated()), id: \.offset) { i, semi in
                    key(semi, black: false, label: NoteName.of(base + semi))
                        .frame(width: w - 3, height: h)
                        .offset(x: CGFloat(i) * w + 1.5)
                }
                ForEach(blacks, id: \.semi) { b in
                    key(b.semi, black: true, label: NoteName.names[b.semi])
                        .frame(width: w * 0.62, height: h * 0.6)
                        .offset(x: CGFloat(b.boundary) * w - w * 0.31)
                }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(editing ? "Keys your voice can play" : "Piano keys")
    }

    private func key(_ semi: Int, black: Bool, label: String) -> PianoKey {
        PianoKey(midi: base + semi, black: black, label: labels || editing ? label : nil,
                 used: used.contains(base + semi), tint: tint,
                 off: enabled.map { !$0[semi] } ?? false, sounding: sounding == semi, editing: editing,
                 down: down, up: up, toggle: { toggle(semi) })
    }
}

struct PianoKey: View {
    @Environment(\.colorScheme) private var scheme
    let midi: Int
    let black: Bool
    let label: String?
    let used: Bool
    let tint: Color
    /// Voice → piano: this key is turned off (dimmed and struck through).
    var off = false
    /// Voice → piano: the key your voice is playing now.
    var sounding = false
    var editing = false
    let down: (Int) -> Void
    let up: (Int) -> Void
    let toggle: () -> Void

    /// True while a finger is on the key; resets on lift *and* on a cancelled touch, so the note always releases.
    @GestureState private var held = false
    /// The note sent on touch-down (released on lift even if the keyboard's octave moved meanwhile).
    @State private var sent: Int?

    var body: some View {
        let shape = UnevenRoundedRectangle(topLeadingRadius: 4, bottomLeadingRadius: black ? 8 : 12,
                                           bottomTrailingRadius: black ? 8 : 12, topTrailingRadius: 4, style: .continuous)
        let lit = held || sounding
        let baseColor: Color = black ? Color(hex: scheme == .dark ? "#26242E" : "#1C1C22") : Color(hex: scheme == .dark ? "#ECEBF2" : "#FFFFFF")
        let face = ZStack(alignment: .bottom) {
            shape.fill(lit ? tint : baseColor)
            if !black && !lit {
                shape.fill(LinearGradient(colors: [.black.opacity(0.06), .clear], startPoint: .top, endPoint: .init(x: 0.5, y: 0.15)))
            }
            if off {
                // Off: dimmed, hatched and struck through (never colour alone).
                shape.fill(black ? Color.black.opacity(0.45) : Color.black.opacity(scheme == .dark ? 0.5 : 0.32))
                GeometryReader { g in
                    Path { p in
                        p.move(to: CGPoint(x: g.size.width * 0.18, y: g.size.height * 0.14))
                        p.addLine(to: CGPoint(x: g.size.width * 0.82, y: g.size.height * 0.86))
                    }
                    .stroke(black ? Color.white.opacity(0.45) : Color.black.opacity(0.45), style: StrokeStyle(lineWidth: 2.5, lineCap: .round))
                }
            }
            shape.strokeBorder(black ? Color.white.opacity(0.08) : Color.black.opacity(0.12), lineWidth: 1)
            if sounding {
                shape.strokeBorder(Color.white, lineWidth: 3).padding(1)
            }
            VStack(spacing: 5) {
                if sounding {
                    Image(systemName: "waveform").font(.system(size: black ? 11 : 14, weight: .heavy)).foregroundStyle(.white)
                } else if used && !editing {
                    Circle().fill(lit ? Color.white : tint).frame(width: 7, height: 7)
                }
                if editing {
                    Image(systemName: off ? "xmark" : "checkmark")
                        .font(.system(size: black ? 10 : 12, weight: .heavy))
                        .foregroundStyle(black ? Color.white.opacity(off ? 0.6 : 0.95) : Color.black.opacity(off ? 0.5 : 0.75))
                }
                if let label {
                    Text(label)
                        .font(.system(size: black ? 10 : 13, weight: .bold, design: .rounded))
                        .foregroundStyle(black || lit ? Color.white.opacity(off ? 0.5 : 0.9) : Color.black.opacity(off ? 0.4 : 0.55))
                        .minimumScaleFactor(0.7)
                        .lineLimit(1)
                }
            }
            .padding(.bottom, black ? 8 : 12)
        }
        .shadow(color: .black.opacity(black ? 0.35 : 0.12), radius: black ? 3 : 1, y: black ? 2 : 1)
        .scaleEffect(y: held ? 0.985 : 1, anchor: .top)
        .contentShape(shape)

        Group {
            if editing {
                face.onTapGesture(perform: toggle)
            } else {
                face.gesture(
                    DragGesture(minimumDistance: 0)
                        .updating($held) { _, state, _ in state = true }
                )
            }
        }
        .onChange(of: held) { _, isHeld in
            if isHeld, sent == nil {
                sent = midi
                down(midi)
            } else if !isHeld, let m = sent {
                sent = nil
                up(m)
            }
        }
        .onDisappear { if let m = sent { sent = nil; up(m) } }
        .animation(.easeOut(duration: 0.12), value: held)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(NoteName.spokenOf(midi))
        .accessibilityValue(accessibilityState)
        .accessibilityAddTraits(editing ? [.isButton] : [.isButton, .playsSound])
        .accessibilityHint(editing ? (off ? "Turns this key on" : "Turns this key off") : "")
        .accessibilityAction {
            if editing { toggle() } else { down(midi); up(midi) }
        }
    }

    private var accessibilityState: String {
        var parts: [String] = []
        if editing || off { parts.append(off ? "Off" : "On") }
        if sounding { parts.append("Playing now") }
        if used { parts.append("In this part") }
        return parts.joined(separator: ", ")
    }
}
