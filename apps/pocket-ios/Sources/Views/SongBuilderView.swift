// Song Builder main screen (docs/projects/song-builder/README.md).
//
//   ┌ Transport card (always visible: readout, Play/Stop, Record) ┐
//   │ scrolling: song controls, stacked part cards, add part      │
//   └ performance dock: voice pads or one-octave piano            ┘

import SwiftUI

struct SongBuilderView: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @State private var showSettings = false
    /// Tightness while its slider is being dragged (previewed, committed on release).
    @State private var draftTightness: Double?

    /// The song as drawn: the live Tightness drag applied.
    private var displaySong: Song {
        var s = store.song
        if let d = draftTightness { s.tightness = d }
        return s
    }

    private var playhead: Double? { store.playing && store.pass >= 0 ? store.beat : nil }

    var body: some View {
        VStack(spacing: 10) {
            TransportCard(showSettings: $showSettings)
                .padding(.horizontal, 12)

            if let error = store.lastError {
                ErrorBanner(text: error) { withAnimation { store.clearError() } }
                    .padding(.horizontal, 12)
                    .transition(.move(edge: .top).combined(with: .opacity))
            }

            ScrollViewReader { proxy in
            LiveScroll {
                VStack(spacing: 10) {
                    SongControls(draftTightness: $draftTightness)
                    let song = displaySong
                    ForEach(song.parts) { part in
                        PartCard(part: part, song: song, selected: part.id == store.selectedPart?.id, playhead: playhead)
                            .id(part.id)
                            .transition(.asymmetric(insertion: .scale(scale: 0.95).combined(with: .opacity), removal: .opacity))
                    }
                    addButtons
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 12)
            }
            .onChange(of: store.selectedPartId) { _, id in
                // Bring the newly selected (expanded) card into view.
                guard let id else { return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
                    withAnimation(.spring(response: 0.45, dampingFraction: 0.9)) { proxy.scrollTo(id, anchor: .top) }
                }
            }
            }
            .mask(LinearGradient(stops: [.init(color: .clear, location: 0), .init(color: .black, location: 0.025),
                                         .init(color: .black, location: 0.97), .init(color: .clear, location: 1)],
                                 startPoint: .top, endPoint: .bottom))
        }
        .padding(.top, 6)
        .frame(maxWidth: 680)
        .frame(maxWidth: .infinity)
        .safeAreaInset(edge: .bottom, spacing: 0) { dock }
        .background(PocketBackground().ignoresSafeArea())
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: store.song.parts.map(\.id))
        .animation(.spring(response: 0.4, dampingFraction: 0.85), value: store.selectedPartId)
        .animation(.easeInOut(duration: 0.25), value: store.lastError)
        .sheet(isPresented: $showSettings) {
            SongSettingsSheet()
                .preferredColorScheme(settings.appearance.colorScheme)
        }
        .preferredColorScheme(settings.appearance.colorScheme)
    }

    private var addButtons: some View {
        HStack(spacing: 10) {
            addButton("Voice drums", "waveform", .drums)
            addButton("Melody", "pianokeys", .melody)
        }
    }

    private func addButton(_ title: String, _ icon: String, _ kind: PartKind) -> some View {
        let tint = settings.partColor(kind)
        return Button {
            Haptics.tick(settings)
            store.addPart(kind)
        } label: {
            HStack(spacing: 8) {
                Image(systemName: "plus").font(.system(size: 14, weight: .heavy))
                Image(systemName: icon).font(.system(size: 14, weight: .semibold))
                Text(title).font(.subheadline.weight(.bold))
            }
            .foregroundStyle(tint)
            .frame(maxWidth: .infinity, minHeight: 50)
            .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(tint.opacity(0.1)))
            .overlay(RoundedRectangle(cornerRadius: 18, style: .continuous).strokeBorder(tint.opacity(0.45), style: StrokeStyle(lineWidth: 1.5, dash: [6, 4])))
        }
        .buttonStyle(PressScale(scale: 0.96))
        .accessibilityLabel("Add \(title.lowercased()) part")
    }

    // MARK: Performance dock

    @ViewBuilder
    private var dock: some View {
        let content = Group {
            if let part = store.selectedPart {
                if part.kind == .drums {
                    DrumDock()
                } else {
                    PianoDock(part: part)
                }
            } else {
                Text("Add a part to start your song.")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 80)
            }
        }
        content
            .padding(.horizontal, 14)
            .padding(.top, 12)
            .padding(.bottom, 8)
            .frame(maxWidth: 680)
            .frame(maxWidth: .infinity)
            .background(
                UnevenRoundedRectangle(topLeadingRadius: 28, topTrailingRadius: 28, style: .continuous)
                    .fill(Color.pocketCard)
                    .shadow(color: .black.opacity(0.18), radius: 16, y: -2)
                    .ignoresSafeArea(edges: .bottom)
            )
            .overlay(alignment: .top) {
                UnevenRoundedRectangle(topLeadingRadius: 28, topTrailingRadius: 28, style: .continuous)
                    .stroke(Color.pocketStroke, lineWidth: 1)
                    .ignoresSafeArea(edges: .bottom)
            }
    }
}

struct ErrorBanner: View {
    let text: String
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.triangle.fill").foregroundStyle(.yellow)
            Text(text)
                .font(.footnote.weight(.medium))
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(action: dismiss) {
                Image(systemName: "xmark").font(.footnote.weight(.bold)).foregroundStyle(.secondary)
                    .frame(width: 44, height: 36)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss error")
        }
        .padding(.leading, 12)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Color.yellow.opacity(0.14)))
        .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(Color.yellow.opacity(0.4), lineWidth: 1))
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Error: \(text)")
    }
}
