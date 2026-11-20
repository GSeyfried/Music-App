// Melody performance area: one chromatic octave (7 white + 5 black keys),
// octave −/+ with the range label, optional note names, and a dot on keys the
// part already uses. Keys fire on touch-down.

import SwiftUI

struct PianoDock: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    let part: Part

    var body: some View {
        let octave = settings.keyboardOctave.clamped(1, 7)
        let base = (octave + 1) * 12
        VStack(spacing: 10) {
            HStack(spacing: 6) {
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
                PocketMenu {
                    Picker("Sound", selection: Binding(get: { part.instrument }, set: { v in store.edit { $0.update(part.id) { $0.instrument = v } } })) {
                        ForEach(MelodyInstrument.allCases) { Text($0.label).tag($0) }
                    }
                } label: {
                    ChipLabel(systemImage: "music.note", text: part.instrument.label, tint: settings.partColor(.melody), chevron: true)
                }
                .accessibilityLabel("Sound, \(part.instrument.label)")
            }
            PianoKeyboard(base: base, labels: settings.noteLabels, used: Set(part.notes.map(\.midi)), tint: settings.partColor(.melody)) { midi in
                Haptics.key(settings)
                store.tapKey(midi)
            }
        }
    }
}

struct PianoKeyboard: View {
    let base: Int
    let labels: Bool
    let used: Set<Int>
    let tint: Color
    let play: (Int) -> Void

    private let whites = [0, 2, 4, 5, 7, 9, 11]
    /// Black key semitone and the white-key boundary it sits on.
    private let blacks: [(semi: Int, boundary: Int)] = [(1, 1), (3, 2), (6, 4), (8, 5), (10, 6)]

    var body: some View {
        GeometryReader { g in
            let w = g.size.width / 7
            let h = g.size.height
            ZStack(alignment: .topLeading) {
                ForEach(Array(whites.enumerated()), id: \.offset) { i, semi in
                    PianoKey(midi: base + semi, black: false, label: labels ? NoteName.of(base + semi) : nil,
                             used: used.contains(base + semi), tint: tint, play: play)
                        .frame(width: w - 3, height: h)
                        .offset(x: CGFloat(i) * w + 1.5)
                }
                ForEach(blacks, id: \.semi) { b in
                    PianoKey(midi: base + b.semi, black: true, label: labels ? NoteName.names[b.semi] : nil,
                             used: used.contains(base + b.semi), tint: tint, play: play)
                        .frame(width: w * 0.62, height: h * 0.6)
                        .offset(x: CGFloat(b.boundary) * w - w * 0.31)
                }
            }
        }
        .frame(height: 168)
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Piano keys")
    }
}

struct PianoKey: View {
    @Environment(\.colorScheme) private var scheme
    let midi: Int
    let black: Bool
    let label: String?
    let used: Bool
    let tint: Color
    let play: (Int) -> Void
    @State private var pressed = false

    var body: some View {
        let shape = UnevenRoundedRectangle(topLeadingRadius: 4, bottomLeadingRadius: black ? 8 : 12,
                                           bottomTrailingRadius: black ? 8 : 12, topTrailingRadius: 4, style: .continuous)
        let base: Color = black ? Color(hex: scheme == .dark ? "#26242E" : "#1C1C22") : Color(hex: scheme == .dark ? "#ECEBF2" : "#FFFFFF")
        ZStack(alignment: .bottom) {
            shape.fill(pressed ? tint : base)
            if !black && !pressed {
                shape.fill(LinearGradient(colors: [.black.opacity(0.06), .clear], startPoint: .top, endPoint: .init(x: 0.5, y: 0.15)))
            }
            shape.strokeBorder(black ? Color.white.opacity(0.08) : Color.black.opacity(0.12), lineWidth: 1)
            VStack(spacing: 5) {
                if used {
                    Circle().fill(pressed ? Color.white : tint).frame(width: 7, height: 7)
                }
                if let label {
                    Text(label)
                        .font(.system(size: black ? 10 : 13, weight: .bold, design: .rounded))
                        .foregroundStyle(black || pressed ? Color.white.opacity(0.85) : Color.black.opacity(0.55))
                        .minimumScaleFactor(0.7)
                        .lineLimit(1)
                }
            }
            .padding(.bottom, black ? 8 : 12)
        }
        .shadow(color: .black.opacity(black ? 0.35 : 0.12), radius: black ? 3 : 1, y: black ? 2 : 1)
        .scaleEffect(y: pressed ? 0.985 : 1, anchor: .top)
        .contentShape(shape)
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in
                    guard !pressed else { return }
                    pressed = true
                    play(midi)
                }
                .onEnded { _ in withAnimation(.easeOut(duration: 0.15)) { pressed = false } }
        )
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(NoteName.spokenOf(midi))
        .accessibilityValue(used ? "In this part" : "")
        .accessibilityAddTraits([.isButton, .playsSound])
        .accessibilityAction { play(midi) }
    }
}
