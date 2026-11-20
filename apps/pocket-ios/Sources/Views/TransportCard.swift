// Top card: song title, Undo/Redo, settings, the bar.beat readout, Play/Stop
// and the big Record button. Always on screen (recording Stop stays visible).

import SwiftUI

struct TransportCard: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @Binding var showSettings: Bool

    private var song: Song { store.song }
    private var countingIn: Bool { store.playing && store.pass < 0 }

    var body: some View {
        VStack(spacing: 10) {
            HStack(spacing: 2) {
                VStack(alignment: .leading, spacing: 1) {
                    Text(song.name)
                        .font(.system(.title3, design: .rounded).weight(.bold))
                        .lineLimit(1)
                    Text("\(Int(song.bpm)) BPM · \(song.bars) \(song.bars == 1 ? "bar" : "bars") · \(song.parts.count) \(song.parts.count == 1 ? "part" : "parts")")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(.secondary)
                        .monospacedDigit()
                }
                .accessibilityElement(children: .combine)
                Spacer(minLength: 4)
                CircleIconButton(systemImage: "arrow.uturn.backward", label: "Undo", size: 36) { store.undo() }
                    .disabled(!store.canUndo)
                CircleIconButton(systemImage: "arrow.uturn.forward", label: "Redo", size: 36) { store.redo() }
                    .disabled(!store.canRedo)
                CircleIconButton(systemImage: "slider.horizontal.3", label: "Song settings", size: 36) { showSettings = true }
            }

            HStack(alignment: .center, spacing: 12) {
                readout
                Spacer(minLength: 0)
                playButton
                RecordButton()
            }

            LoopProgress(beat: store.playing && store.pass >= 0 ? store.beat : nil,
                         loopBeats: song.loopBeats, beatsPerBar: song.beatsPerBar,
                         tint: store.recording ? settings.recordColor : .primary)
        }
        .pocketCard(padding: 14)
    }

    // MARK: Readout

    private var readout: some View {
        VStack(alignment: .leading, spacing: 2) {
            Group {
                if countingIn {
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(max(1, Int((-store.beat).rounded(.up))))")
                            .contentTransition(.numericText(countsDown: true))
                        Text("Count-in")
                            .font(.system(size: 20, weight: .bold, design: .rounded))
                            .foregroundStyle(settings.recordColor)
                    }
                } else {
                    Text(positionText).contentTransition(.numericText())
                }
            }
            .font(.system(size: 46, weight: .heavy, design: .rounded))
            .monospacedDigit()
            .foregroundStyle(store.playing ? Color.primary : Color.primary.opacity(0.45))
            .animation(.snappy(duration: 0.18), value: positionText)

            statusLine
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel(accessibilityPosition)
    }

    private var positionText: String {
        guard store.playing, store.pass >= 0 else { return "1.1" }
        let bpb = max(1, song.beatsPerBar)
        let b = Int(max(0, store.beat))
        return "\(b / bpb + 1).\(b % bpb + 1)"
    }

    private var statusLine: some View {
        HStack(spacing: 6) {
            if store.recording {
                Circle().fill(settings.recordColor).frame(width: 8, height: 8)
                Text(countingIn ? "Get ready" : "Recording · pass \(store.pass + 1)")
                    .foregroundStyle(settings.recordColor)
            } else if store.playing {
                Image(systemName: "play.fill").font(.system(size: 9))
                Text(countingIn ? "Get ready" : "Playing · pass \(store.pass + 1)")
            } else {
                Text("Stopped")
            }
        }
        .font(.footnote.weight(.semibold))
        .foregroundStyle(.secondary)
        .monospacedDigit()
    }

    private var accessibilityPosition: String {
        if countingIn { return "Count-in, \(max(1, Int((-store.beat).rounded(.up)))) beats left" }
        guard store.playing else { return "Stopped" }
        let parts = positionText.split(separator: ".")
        return "\(store.recording ? "Recording" : "Playing"), bar \(parts.first ?? "1"), beat \(parts.last ?? "1"), pass \(store.pass + 1)"
    }

    // MARK: Buttons

    private var playButton: some View {
        Button {
            Haptics.tick(settings)
            store.togglePlay()
        } label: {
            VStack(spacing: 4) {
                Image(systemName: store.playing ? "stop.fill" : "play.fill")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(store.playing ? Color.primary : Color.pocketCard)
                    .contentTransition(.symbolEffect(.replace))
                    .frame(width: 58, height: 58)
                    .background(Circle().fill(store.playing ? Color.pocketRaised : Color.primary))
                Text(store.playing ? "Stop" : "Play")
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.secondary)
            }
        }
        .buttonStyle(PressScale())
        .accessibilityLabel(store.playing ? "Stop" : "Play")
        .accessibilityHint(store.playing ? "Stops playback and recording" : "Plays the loop")
    }
}

/// The dominant action: Record (count-in, then every pass adds to the loop).
struct RecordButton: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @State private var pulse = false

    var body: some View {
        let red = settings.recordColor
        Button {
            Haptics.record(settings)
            store.toggleRecord()
        } label: {
            VStack(spacing: 4) {
                ZStack {
                    Circle()
                        .fill(red.opacity(store.recording ? 0.28 : 0))
                        .frame(width: 84, height: 84)
                        .scaleEffect(pulse && store.recording ? 1.12 : 0.92)
                    Circle()
                        .strokeBorder(red, lineWidth: 4)
                        .frame(width: 74, height: 74)
                    RoundedRectangle(cornerRadius: store.recording ? 8 : 30, style: .continuous)
                        .fill(red)
                        .frame(width: store.recording ? 28 : 58, height: store.recording ? 28 : 58)
                        .shadow(color: red.opacity(0.55), radius: store.recording ? 10 : 6)
                }
                .frame(width: 84, height: 84)
                .animation(.spring(response: 0.35, dampingFraction: 0.7), value: store.recording)
                Text(store.recording ? "Stop rec" : "Record")
                    .font(.caption2.weight(.bold))
                    .foregroundStyle(store.recording ? red : .secondary)
                    .padding(.top, -6)
            }
        }
        .buttonStyle(PressScale(scale: 0.9))
        .accessibilityLabel(store.recording ? "Stop recording" : "Record")
        .accessibilityHint(store.recording ? "Keeps playing; stops adding to the loop" : "Counts in, then each pass adds what you play to the loop")
        .onAppear { withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) { pulse = true } }
    }
}

/// Thin loop bar: bar segments that fill as the loop plays.
struct LoopProgress: View {
    let beat: Double?
    let loopBeats: Double
    let beatsPerBar: Int
    var tint: Color = .primary

    var body: some View {
        let bars = max(1, Int(loopBeats) / max(1, beatsPerBar))
        HStack(spacing: 4) {
            ForEach(0..<bars, id: \.self) { bar in
                GeometryReader { g in
                    let start = Double(bar * beatsPerBar)
                    let f = beat.map { (($0 - start) / Double(beatsPerBar)).clamped(0, 1) } ?? 0
                    ZStack(alignment: .leading) {
                        Capsule().fill(Color.pocketTrack)
                        Capsule().fill(tint.opacity(0.85)).frame(width: g.size.width * f)
                    }
                }
                .frame(height: 5)
            }
        }
        .accessibilityHidden(true)
    }
}
