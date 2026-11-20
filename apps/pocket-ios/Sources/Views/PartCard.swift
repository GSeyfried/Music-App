// One stacked part card. Collapsed: colour, name, count, mini preview, M/S.
// Selected: the step grid or note strip plus sound, FX, starter beat, level
// and the clear/duplicate/delete menu.

import SwiftUI

struct PartCard: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    let part: Part
    /// The song as drawn (with a Tightness drag preview applied).
    let song: Song
    let selected: Bool
    let playhead: Double?
    @State private var showFx = false
    @State private var showStarter = false
    @State private var showMore = false

    private var tint: Color { settings.partColor(part.kind) }
    private var anySolo: Bool { song.parts.contains { $0.solo } }
    private var silent: Bool { anySolo ? !part.solo : part.muted }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            header
            if selected {
                if part.kind == .drums {
                    StepGrid(song: song, part: part, playhead: playhead) { lane, step in
                        Haptics.tick(settings)
                        store.toggleStep(partId: part.id, lane: lane, step: step)
                    }
                    if part.hits.isEmpty {
                        hint(part.input == .voice ? "Tap the pads or beatbox, then press Record. Or start from a beat below."
                                                  : "Tap the pads or the grid, then press Record. Or start from a beat below.")
                    }
                } else {
                    MelodyStrip(song: song, part: part, playhead: playhead)
                    if part.notes.isEmpty {
                        hint(part.input == .voice ? "Sing a note to hear it on the nearest enabled key. Press Record to keep it."
                                                  : "Hold a key to play it. Press Record to keep what you play.")
                    }
                }
                controls
            } else {
                MiniPreview(song: song, part: part, playhead: playhead)
                    .opacity(silent ? 0.45 : 1)
            }
        }
        .pocketCard(accent: selected ? tint : nil)
        .contentShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        // Selecting by tapping the card only while it is collapsed: on the selected card the
        // gesture is masked off (`.subviews`) so it can't swallow taps meant for its menus.
        .gesture(TapGesture().onEnded {
            Haptics.tick(settings)
            withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { store.select(part.id) }
        }, including: selected ? .subviews : .all)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(tint.opacity(silent ? 0.25 : 1))
                Image(systemName: part.systemImage)
                    .font(.system(size: 18, weight: .bold))
                    .foregroundStyle(.white)
            }
            .frame(width: 42, height: 42)

            VStack(alignment: .leading, spacing: 2) {
                Text(part.name)
                    .font(.system(.headline, design: .rounded))
                    .lineLimit(1)
                Text(subtitle)
                    .font(.caption.weight(.medium))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isButton)
            .accessibilityHint(selected ? "" : "Selects this part")
            .accessibilityAction { store.select(part.id) }

            Spacer(minLength: 4)

            MSButton(letter: "M", label: "Mute", on: part.muted, tint: .primary) {
                store.edit { $0.update(part.id) { $0.muted.toggle() } }
            }
            MSButton(letter: "S", label: "Solo", on: part.solo, tint: Color(hex: "#FFD60A")) {
                store.edit { $0.update(part.id) { $0.solo.toggle() } }
            }
        }
    }

    private var subtitle: String {
        let count = part.kind == .drums
            ? "\(part.hits.count) \(part.hits.count == 1 ? "hit" : "hits")"
            : "\(part.notes.count) \(part.notes.count == 1 ? "note" : "notes")"
        let sound = part.kind == .drums ? (Catalog.kits.first { $0.id == part.kit }?.label ?? part.kit) : part.instrument.label
        var s = "\(sound) · \(count)"
        if let fx = part.fxLabel { s += " · \(fx)" }
        if silent { s += anySolo && !part.solo ? " · not soloed" : " · muted" }
        return s
    }

    // MARK: Selected controls

    // Frequent controls sit in a wrapping row of plain buttons (no Menu inside a
    // horizontal ScrollView: owner report + REFERENCE_EVAL_2026-11-19.md finding 4).
    private var controls: some View {
        VStack(alignment: .leading, spacing: 8) {
            soundPicker
            FlowRow {
                fxButton
                if part.kind == .drums { starterButton }
                moreButton
            }
            if let fx = part.fxLabel {
                hint("FX: \(fx). \(part.fxList.count == 1 ? (Catalog.fx.first { $0.id == part.fxList[0] }?.help ?? "") : "Runs left to right.")")
            }
            HStack(spacing: 10) {
                Image(systemName: "speaker.fill").font(.footnote).foregroundStyle(.secondary)
                PocketSlider(value: part.level, range: 0...1.5, tint: tint) { v in
                    store.edit { $0.update(part.id) { $0.level = v } }
                }
                .accessibilityLabel("\(part.name) level")
                .accessibilityValue("\(Int((part.level * 100).rounded())) percent")
                Text("\(Int((part.level * 100).rounded()))%")
                    .font(.caption.weight(.semibold))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .frame(width: 42, alignment: .trailing)
                    .accessibilityHidden(true)
            }
        }
        .sheet(isPresented: $showFx) {
            FxSheet(partId: part.id).preferredColorScheme(settings.appearance.colorScheme)
        }
    }

    /// Kit (drums) or sound (melody): every option visible, one tap.
    private var soundPicker: some View {
        HStack(spacing: 10) {
            Text(part.kind == .drums ? "Kit" : "Sound")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)
                .frame(width: 48, alignment: .leading)
                .accessibilityHidden(true)
            if part.kind == .drums {
                Segments(options: Catalog.kits.map(\.id), selection: part.kit, label: { id in Catalog.kits.first { $0.id == id }?.label ?? id },
                         height: 38, fill: true) { id in
                    Haptics.tick(settings)
                    store.edit { $0.update(part.id) { $0.kit = id } }
                }
                .accessibilityLabel("Kit")
            } else {
                Segments(options: MelodyInstrument.allCases, selection: part.instrument, label: { $0.label }, height: 38, fill: true) { i in
                    Haptics.tick(settings)
                    store.edit { $0.update(part.id) { $0.instrument = i } }
                }
                .accessibilityLabel("Sound")
            }
        }
    }

    private var fxButton: some View {
        Button { showFx = true } label: {
            ChipLabel(systemImage: "sparkles", text: part.fxLabel ?? "FX: clean", chevron: true)
        }
        .buttonStyle(PressScale())
        .accessibilityLabel("Effects")
        .accessibilityValue(part.fxLabel ?? "Clean")
    }

    private var starterButton: some View {
        Button { showStarter = true } label: {
            ChipLabel(systemImage: "wand.and.stars", text: "Starter beat", chevron: true)
        }
        .buttonStyle(PressScale())
        .accessibilityLabel("Starter beat")
        .confirmationDialog("Replace this part's hits with", isPresented: $showStarter, titleVisibility: .visible) {
            ForEach(StarterBeat.all) { beat in
                Button(beat.label) { store.edit { $0.applyStarter(beat, to: part.id) } }
            }
        } message: {
            Text("You can bring the old hits back with Undo.")
        }
    }

    private var moreButton: some View {
        Button { showMore = true } label: {
            ChipLabel(systemImage: "ellipsis", text: "More")
        }
        .buttonStyle(PressScale())
        .accessibilityLabel("More actions for \(part.name)")
        .confirmationDialog(part.name, isPresented: $showMore, titleVisibility: .visible) {
            if part.eventCount > 0 {
                Button("Clear \(part.kind == .drums ? "hits" : "notes")") {
                    store.edit { $0.update(part.id) { $0.hits.removeAll(); $0.notes.removeAll() } }
                }
            }
            Button("Duplicate") { store.edit { $0.duplicatePart(part.id) } }
            Button("Delete part", role: .destructive) {
                withAnimation(.spring(response: 0.4)) { store.deletePart(part.id) }
            }
        } message: {
            Text("Undo brings back a cleared or deleted part.")
        }
    }

    private func binding<T>(_ key: WritableKeyPath<Part, T>) -> Binding<T> {
        Binding(get: { part[keyPath: key] }, set: { v in store.edit { $0.update(part.id) { $0[keyPath: key] = v } } })
    }

    private func hint(_ text: String) -> some View {
        Text(text)
            .font(.footnote)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// Mute / Solo square: letter always visible, filled when on (works without colour).
struct MSButton: View {
    let letter: String
    let label: String
    let on: Bool
    let tint: Color
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(letter)
                .font(.system(size: 15, weight: .heavy, design: .rounded))
                .foregroundStyle(on ? (tint == .primary ? Color.pocketCard : .black) : .secondary)
                .frame(width: 36, height: 36)
                .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(on ? AnyShapeStyle(tint) : AnyShapeStyle(Color.pocketRaised)))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(PressScale())
        .accessibilityLabel(label)
        .accessibilityValue(on ? "On" : "Off")
    }
}
