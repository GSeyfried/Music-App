// One-tap "Add a track": every TrackTemplate as a tile (icon, name, one-line
// detail). Tapping adds the part and selects it. Templates that aren't built
// yet show "Soon" and are disabled.

import SwiftUI

struct AddTrackGrid: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings

    var body: some View {
        let templates = TrackTemplate.allCases
        VStack(alignment: .leading, spacing: 8) {
            Label("Add a track", systemImage: "plus")
                .font(.footnote.weight(.bold))
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
                .padding(.leading, 4)
                .accessibilityAddTraits(.isHeader)
            // Two columns (plain stacks: they also render in snapshot mode).
            VStack(spacing: 8) {
                ForEach(0..<(templates.count + 1) / 2, id: \.self) { row in
                    HStack(spacing: 8) {
                        tile(templates[row * 2])
                        if row * 2 + 1 < templates.count { tile(templates[row * 2 + 1]) } else { Color.clear.frame(maxWidth: .infinity) }
                    }
                }
            }
        }
    }

    private func tile(_ t: TrackTemplate) -> some View {
        let tint = settings.partColor(t.kind)
        let on = t.available
        return Button {
            Haptics.tick(settings)
            store.addPart(t)
        } label: {
            HStack(alignment: .top, spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: 10, style: .continuous).fill(on ? tint : Color.pocketTrack)
                    Image(systemName: t.systemImage)
                        .font(.system(size: 15, weight: .bold))
                        .foregroundStyle(on ? Color.white : .secondary)
                }
                .frame(width: 34, height: 34)
                VStack(alignment: .leading, spacing: 2) {
                    Text(t.label)
                        .font(.subheadline.weight(.bold))
                        .foregroundStyle(on ? Color.primary : .secondary)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                    if !on {
                        Text("Soon")
                            .font(.caption2.weight(.heavy))
                            .textCase(.uppercase)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .overlay(Capsule().strokeBorder(Color.secondary.opacity(0.6), lineWidth: 1))
                    }
                    Text(t.detail)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(3)
                        .multilineTextAlignment(.leading)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 0)
            }
            .padding(10)
            .frame(maxWidth: .infinity, minHeight: 72, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(on ? tint.opacity(0.1) : Color.pocketCard.opacity(0.6)))
            .overlay(RoundedRectangle(cornerRadius: 16, style: .continuous)
                .strokeBorder(on ? tint.opacity(0.45) : Color.pocketStroke, style: StrokeStyle(lineWidth: 1.5, dash: on ? [] : [5, 4])))
            .contentShape(RoundedRectangle(cornerRadius: 16, style: .continuous))
        }
        .buttonStyle(PressScale(scale: 0.96))
        .disabled(!on)
        .accessibilityLabel(on ? "Add \(t.label) track" : "\(t.label), coming soon")
        .accessibilityHint(t.detail)
    }
}
