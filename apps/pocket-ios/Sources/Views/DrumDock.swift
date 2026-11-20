// Drum performance area: three big voice pads (Kick, Snare, Hi-hat) and two
// smaller ones (Clap, Open hat). Pads fire on touch-down. Pads flash on every
// hit (touch, voice or training). Above them: mic status, "Use voice", the
// last heard sound, and voice training.

import SwiftUI

struct DrumDock: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    /// Per-lane flash token; the pad is lit while its token is set.
    @State private var lit: [DrumLane: Int] = [:]
    @State private var confirmForget = false

    var body: some View {
        VStack(spacing: 10) {
            if let t = store.training {
                TrainingBanner(lane: t.lane, count: t.count)
                    .transition(.move(edge: .top).combined(with: .opacity))
            } else {
                voiceStatus
            }

            HStack(spacing: 10) {
                ForEach(DrumLane.voiceLanes) { lane in
                    DrumPad(lane: lane, big: true, lit: lit[lane] != nil, target: store.training?.lane == lane) { hit(lane) }
                }
            }
            HStack(spacing: 10) {
                ForEach([DrumLane.clap, .openHat]) { lane in
                    DrumPad(lane: lane, big: false, lit: lit[lane] != nil, target: false) { hit(lane) }
                }
            }
        }
        .animation(.spring(response: 0.35, dampingFraction: 0.85), value: store.training?.lane)
        .onChange(of: store.flash.count) { _, _ in light(store.flash.lane) }
        .confirmationDialog("Forget your trained sounds?", isPresented: $confirmForget, titleVisibility: .visible) {
            Button("Forget my sounds", role: .destructive) { store.forgetTraining() }
        } message: {
            Text("Recognition goes back to the built-in rules until you train again.")
        }
    }

    private func hit(_ lane: DrumLane) {
        Haptics.pad(settings)
        store.tapPad(lane)
    }

    private func light(_ lane: DrumLane) {
        let token = (lit[lane] ?? 0) + 1
        withAnimation(.easeOut(duration: 0.04)) { lit[lane] = token }
        DispatchQueue.main.asyncAfter(deadline: .now() + settings.padFlashMs / 1000) {
            guard lit[lane] == token else { return }
            withAnimation(.easeOut(duration: 0.28)) { lit[lane] = nil }
        }
    }

    // MARK: Voice status

    private var voiceStatus: some View {
        HStack(spacing: 8) {
            CapsuleToggle(label: "Voice", systemImage: settings.voiceListen ? "mic.fill" : "mic.slash.fill",
                          isOn: Binding(get: { settings.voiceListen }, set: { store.setListen($0) }), tint: .green)
                .accessibilityHint("Turns your beatboxing into drum hits")

            VStack(alignment: .leading, spacing: 1) {
                HStack(spacing: 5) {
                    Circle().fill(micDot).frame(width: 7, height: 7)
                    Text(micText).font(.caption.weight(.semibold))
                }
                Group {
                    if let heard = store.lastHeard {
                        Text("Heard: \(heard.lane.label) · \(Int(heard.centroidHz.rounded())) Hz")
                            .foregroundStyle(settings.laneColor(heard.lane))
                    } else {
                        Text(store.recognitionUsesTraining ? "Using your sounds" : "Built-in recognition")
                            .foregroundStyle(.secondary)
                    }
                }
                .font(.caption2.weight(.semibold))
                .lineLimit(1)
                .monospacedDigit()
            }
            .accessibilityElement(children: .combine)

            Spacer(minLength: 0)

            PocketMenu {
                Button { store.startTraining() } label: {
                    Label(store.voiceProfileReady ? "Train again" : "Train my sounds", systemImage: "waveform.badge.plus")
                }
                Button(role: .destructive) { confirmForget = true } label: {
                    Label("Forget my sounds", systemImage: "trash")
                }
                .disabled(store.profile.isEmpty)
            } label: {
                ChipLabel(systemImage: "waveform.badge.plus", text: "Train", tint: .primary)
            }
            .accessibilityLabel("Train voice sounds")
        }
    }

    private var micText: String {
        guard settings.voiceListen else { return "Voice off · tap the pads" }
        switch store.micState {
        case "on": return "Listening"
        case "opening": return "Opening mic…"
        default: return store.engineReady ? "Mic off" : "Mic waiting for audio"
        }
    }

    private var micDot: Color {
        guard settings.voiceListen else { return .secondary.opacity(0.5) }
        switch store.micState {
        case "on": return .green
        case "opening": return .yellow
        default: return .secondary.opacity(0.5)
        }
    }
}

/// One drum pad. Fires on touch-down; `lit` is the hit flash.
struct DrumPad: View {
    @EnvironmentObject private var settings: PocketSettings
    @Environment(\.colorScheme) private var scheme
    let lane: DrumLane
    let big: Bool
    let lit: Bool
    let target: Bool
    let action: () -> Void
    @State private var pressed = false

    var body: some View {
        let color = settings.laneColor(lane)
        let shape = RoundedRectangle(cornerRadius: big ? 24 : 18, style: .continuous)
        ZStack {
            shape.fill(color)
            shape.fill(LinearGradient(colors: [.white.opacity(0.12), .black.opacity(0.16)], startPoint: .top, endPoint: .bottom))
            shape.fill(Color.white.opacity(lit ? 0.42 : 0))
            shape.strokeBorder(Color.white.opacity(lit ? 0.9 : 0.18), lineWidth: lit ? 2.5 : 1)
            if target {
                shape.strokeBorder(Color.primary, style: StrokeStyle(lineWidth: 3, dash: [7, 5]))
                    .padding(-5)
            }
            if big {
                VStack(spacing: 4) {
                    Text(lane.label)
                        .font(.system(size: 21, weight: .heavy, design: .rounded))
                    if let hint = lane.voiceHint {
                        Text(hint)
                            .font(.caption.weight(.semibold))
                            .opacity(0.85)
                    }
                }
                .foregroundStyle(.white)
                .shadow(color: .black.opacity(0.2), radius: 2, y: 1)
            } else {
                Text(lane.label)
                    .font(.system(size: 16, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
            }
        }
        .frame(height: big ? 104 : 50)
        .frame(maxWidth: .infinity)
        .shadow(color: color.opacity(lit ? 0.75 : (scheme == .dark ? 0.25 : 0.3)), radius: lit ? 18 : 8, y: lit ? 0 : 4)
        .scaleEffect(pressed ? 0.94 : (lit ? 1.02 : 1))
        .animation(.spring(response: 0.22, dampingFraction: 0.55), value: pressed)
        .contentShape(shape)
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { _ in
                    guard !pressed else { return }
                    pressed = true
                    action()
                }
                .onEnded { _ in pressed = false }
        )
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(lane.label) pad")
        .accessibilityHint(lane.voiceHint.map { "Plays \(lane.label.lowercased()). Voice: \($0)" } ?? "Plays \(lane.label.lowercased())")
        .accessibilityAddTraits([.isButton, .playsSound])
        .accessibilityAction { action() }
    }
}

/// Shown while training: which sound to make and how many examples are in.
struct TrainingBanner: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    let lane: DrumLane
    let count: Int

    var body: some View {
        let total = max(1, settings.trainingHits)
        let color = settings.laneColor(lane)
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                Text("Train: say \(lane.voiceHint ?? lane.label) for \(lane.label)")
                    .font(.subheadline.weight(.bold))
                    .lineLimit(2)
                    .minimumScaleFactor(0.85)
                HStack(spacing: 10) {
                    HStack(spacing: 4) {
                        ForEach(0..<total, id: \.self) { i in
                            Capsule()
                                .fill(i < count ? color : Color.pocketTrack)
                                .frame(width: 16, height: 6)
                        }
                    }
                    Text("\(count) of \(total)")
                        .font(.caption.weight(.semibold))
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                    Text(stepText)
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.tertiary)
                }
            }
            .accessibilityElement(children: .combine)
            Spacer(minLength: 0)
            Button { store.stopTraining() } label: {
                ChipLabel(text: "Done", tint: .primary)
            }
            .buttonStyle(PressScale())
            .accessibilityLabel("Stop training")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(color.opacity(0.14)))
        .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous).strokeBorder(color.opacity(0.5), lineWidth: 1))
    }

    private var stepText: String {
        let i = (DrumLane.voiceLanes.firstIndex(of: lane) ?? 0) + 1
        return "Sound \(i)/\(DrumLane.voiceLanes.count)"
    }
}
