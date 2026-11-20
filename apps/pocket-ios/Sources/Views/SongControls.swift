// Compact song row: tempo, bars, grid, click, and Tightness
// (Natural → On the grid). Tightness previews live while dragging and is
// committed as one undo step on release.

import SwiftUI

struct SongControls: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @Binding var draftTightness: Double?
    @State private var showTempo = false
    @State private var showBars = false
    @State private var showGrid = false

    private var song: Song { store.song }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Button { showTempo = true } label: {
                    ChipLabel(text: "\(Int(song.bpm)) BPM", chevron: true)
                        .monospacedDigit()
                        .fixedSize()
                }
                .buttonStyle(PressScale())
                .accessibilityLabel("Tempo, \(Int(song.bpm)) beats per minute")
                .popover(isPresented: $showTempo) {
                    TempoPopover().presentationCompactAdaptation(.popover)
                }

                // Plain buttons + dialogs, not Menus: this row is inside the vertical scroll view.
                Button { showBars = true } label: {
                    ChipLabel(text: "\(song.bars) \(song.bars == 1 ? "bar" : "bars")", chevron: true).fixedSize()
                }
                .buttonStyle(PressScale())
                .accessibilityLabel("Loop length, \(song.bars) bars")
                .confirmationDialog("Loop length", isPresented: $showBars, titleVisibility: .visible) {
                    ForEach([1, 2, 4, 8], id: \.self) { n in
                        Button("\(n) \(n == 1 ? "bar" : "bars")\(n == song.bars ? " ✓" : "")") { store.edit { $0.setBars(n) } }
                    }
                } message: {
                    Text("Longer repeats what's there; shorter drops what falls outside (Undo restores).")
                }

                Button { showGrid = true } label: {
                    ChipLabel(text: "Grid \(song.grid.label)", chevron: true).fixedSize()
                }
                .buttonStyle(PressScale())
                .accessibilityLabel("Grid, \(song.grid.label)")
                .confirmationDialog("Grid", isPresented: $showGrid, titleVisibility: .visible) {
                    ForEach(Grid.allCases) { g in
                        Button("\(g.label)\(g == song.grid ? " ✓" : "")") { store.edit { $0.grid = g } }
                    }
                }

                Spacer(minLength: 0)

                CapsuleToggle(label: "Click", systemImage: "metronome.fill", iconOnly: true, isOn: Binding(
                    get: { settings.clickMode != .off },
                    set: { on in
                        settings.clickMode = on ? .always : .off
                        store.settingsChanged()
                    }
                ), tint: .indigo)
            }

            TightnessRow(draft: $draftTightness)
        }
        .pocketCard(padding: 12)
    }
}

struct TightnessRow: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @Binding var draft: Double?

    var body: some View {
        let value = draft ?? store.song.tightness
        HStack(spacing: 10) {
            VStack(alignment: .leading, spacing: 0) {
                Text("Tightness")
                    .font(.subheadline.weight(.semibold))
                Text(Self.describe(value))
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
                    .contentTransition(.numericText())
            }
            .frame(width: 84, alignment: .leading)
            .accessibilityHidden(true)
            PocketSlider(value: store.song.tightness, range: 0...1, tint: .indigo, step: 0.05,
                         onDrag: { draft = $0 }) { v in
                store.edit { $0.tightness = v }
            }
            .accessibilityLabel("Tightness, natural to on the grid")
            .accessibilityValue(Self.describe(store.song.tightness))
        }
        .padding(.horizontal, 4)
    }

    static func describe(_ v: Double) -> String {
        if v <= 0.001 { return "Natural" }
        if v >= 0.999 { return "On the grid" }
        return "\(Int((v * 100).rounded()))%"
    }
}

struct TempoPopover: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @State private var drag: Double?

    var body: some View {
        let bpm = drag ?? store.song.bpm
        VStack(spacing: 10) {
            Text("Tempo").font(.subheadline.weight(.semibold)).foregroundStyle(.secondary)
            HStack(spacing: 18) {
                CircleIconButton(systemImage: "minus", label: "Slower", size: 44) { nudge(-1) }
                VStack(spacing: 0) {
                    Text("\(Int(bpm.rounded()))")
                        .font(.system(size: 48, weight: .heavy, design: .rounded))
                        .monospacedDigit()
                        .contentTransition(.numericText())
                    Text("BPM").font(.caption.weight(.bold)).foregroundStyle(.secondary)
                }
                .frame(minWidth: 96)
                CircleIconButton(systemImage: "plus", label: "Faster", size: 44) { nudge(1) }
            }
            PocketSlider(value: store.song.bpm, range: 40...240, tint: .indigo, step: 1, onDrag: { drag = $0 }) { v in
                store.edit { $0.bpm = v }
            }
            .accessibilityLabel("Tempo")
            .accessibilityValue("\(Int(store.song.bpm)) BPM")
        }
        .padding(18)
        .frame(width: 280)
    }

    private func nudge(_ d: Double) {
        Haptics.tick(settings)
        store.edit { $0.bpm = ($0.bpm + d).clamped(40, 240) }
    }
}
