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
    @State private var confirmDelete = false

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
                        hint("Tap the pads or beatbox, then press Record. Or start from a beat below.")
                    }
                } else {
                    MelodyStrip(song: song, part: part, playhead: playhead)
                }
                controls
            } else {
                MiniPreview(song: song, part: part, playhead: playhead)
                    .opacity(silent ? 0.45 : 1)
            }
        }
        .pocketCard(accent: selected ? tint : nil)
        .contentShape(RoundedRectangle(cornerRadius: 24, style: .continuous))
        .onTapGesture {
            guard !selected else { return }
            Haptics.tick(settings)
            withAnimation(.spring(response: 0.4, dampingFraction: 0.85)) { store.select(part.id) }
        }
        .accessibilityAddTraits(selected ? .isSelected : [])
        .confirmationDialog("Delete “\(part.name)”?", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("Delete part", role: .destructive) {
                withAnimation(.spring(response: 0.4)) { store.deletePart(part.id) }
            }
        } message: {
            Text("You can bring it back with Undo.")
        }
    }

    // MARK: Header

    private var header: some View {
        HStack(spacing: 12) {
            ZStack {
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(tint.opacity(silent ? 0.25 : 1))
                Image(systemName: part.kind == .drums ? "waveform" : "pianokeys")
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
        if part.fx != "clean", let fx = Catalog.fx.first(where: { $0.id == part.fx }) { s += " · \(fx.label)" }
        if silent { s += anySolo && !part.solo ? " · not soloed" : " · muted" }
        return s
    }

    // MARK: Selected controls

    private var controls: some View {
        VStack(alignment: .leading, spacing: 6) {
            ScrollViewIfNeeded {
                HStack(spacing: 8) {
                    soundMenu
                    fxMenu
                    if part.kind == .drums { starterMenu }
                    moreMenu
                }
            }
            if let fx = Catalog.fx.first(where: { $0.id == part.fx }), fx.id != "clean" {
                hint("\(fx.label): \(fx.help)")
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
    }

    private var soundMenu: some View {
        PocketMenu {
            if part.kind == .drums {
                Picker("Kit", selection: binding(\.kit)) {
                    ForEach(Catalog.kits, id: \.id) { Text($0.label).tag($0.id) }
                }
            } else {
                Picker("Sound", selection: binding(\.instrument)) {
                    ForEach(MelodyInstrument.allCases) { Text($0.label).tag($0) }
                }
            }
        } label: {
            ChipLabel(systemImage: part.kind == .drums ? "square.grid.2x2.fill" : "music.note",
                      text: part.kind == .drums ? (Catalog.kits.first { $0.id == part.kit }?.label ?? part.kit) : part.instrument.label,
                      chevron: true)
        }
        .accessibilityLabel(part.kind == .drums ? "Kit" : "Sound")
    }

    private var fxMenu: some View {
        PocketMenu {
            Picker("Effect", selection: binding(\.fx)) {
                ForEach(Catalog.fx, id: \.id) { fx in
                    Text(fx.label).tag(fx.id)
                }
            }
        } label: {
            ChipLabel(systemImage: "sparkles", text: Catalog.fx.first { $0.id == part.fx }?.label ?? "FX", chevron: true)
        }
        .accessibilityLabel("Effect")
    }

    private var starterMenu: some View {
        PocketMenu {
            Section("Replace this part's hits with") {
                ForEach(StarterBeat.all) { beat in
                    Button(beat.label) { store.edit { $0.applyStarter(beat, to: part.id) } }
                }
            }
        } label: {
            ChipLabel(systemImage: "wand.and.stars", text: "Starter", chevron: true)
        }
        .accessibilityLabel("Starter beat")
    }

    private var moreMenu: some View {
        PocketMenu {
            Button {
                store.edit { $0.update(part.id) { $0.hits.removeAll(); $0.notes.removeAll() } }
            } label: { Label("Clear", systemImage: "eraser") }
                .disabled(part.eventCount == 0)
            Button {
                store.edit { $0.duplicatePart(part.id) }
            } label: { Label("Duplicate", systemImage: "plus.square.on.square") }
            Button(role: .destructive) {
                confirmDelete = true
            } label: { Label("Delete…", systemImage: "trash") }
        } label: {
            ChipLabel(systemImage: "ellipsis", text: "More")
        }
        .accessibilityLabel("More actions for \(part.name)")
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

/// Horizontal scroll for a chip row (plain row in snapshot mode).
struct ScrollViewIfNeeded<Content: View>: View {
    @ViewBuilder var content: Content
    @Environment(\.isSnapshot) private var isSnapshot
    var body: some View {
        if isSnapshot { content } else { ScrollView(.horizontal, showsIndicators: false) { content } }
    }
}
