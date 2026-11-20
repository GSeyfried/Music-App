// FX stack for one part: up to three presets (Catalog.fx) run in order.
// The chain is shown as numbered, removable chips; the list below adds to
// the end. A sheet, not a Menu, so it is easy to tap (owner report).

import SwiftUI

struct FxSheet: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @Environment(\.dismiss) private var dismiss
    let partId: Int

    /// The engine stacks at most this many (crates/voiceloop-engine/src/song.rs; SongStore.setFx).
    static let maxFx = 3

    private var part: Part? { store.song.parts.first { $0.id == partId } }

    var body: some View {
        let chain = part?.fxList ?? []
        let tint = settings.partColor(part?.kind ?? .drums)
        VStack(alignment: .leading, spacing: 14) {
            HStack {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Effects").font(.system(.title2, design: .rounded).weight(.bold))
                    Text(part?.name ?? "").font(.subheadline.weight(.medium)).foregroundStyle(.secondary)
                }
                Spacer()
                Button { dismiss() } label: { ChipLabel(text: "Done", tint: .indigo, active: true) }
                    .buttonStyle(PressScale())
            }

            VStack(alignment: .leading, spacing: 8) {
                Text("In order · \(chain.count) of \(Self.maxFx)")
                    .font(.footnote.weight(.bold)).textCase(.uppercase).foregroundStyle(.secondary)
                if chain.isEmpty {
                    Text("Clean: no effects. Add up to \(Self.maxFx) below.")
                        .font(.subheadline).foregroundStyle(.secondary)
                        .frame(minHeight: 44)
                } else {
                    FlowRow {
                        ForEach(Array(chain.enumerated()), id: \.offset) { i, id in
                            Button {
                                Haptics.tick(settings)
                                var next = chain
                                next.remove(at: i)
                                store.setFx(partId, next)
                            } label: {
                                HStack(spacing: 6) {
                                    Text("\(i + 1)").font(.caption.weight(.heavy)).monospacedDigit()
                                        .frame(width: 20, height: 20)
                                        .background(Circle().fill(Color.white.opacity(0.25)))
                                    Text(label(id)).font(.subheadline.weight(.semibold))
                                    Image(systemName: "xmark").font(.system(size: 11, weight: .heavy)).opacity(0.8)
                                }
                                .foregroundStyle(.white)
                                .padding(.horizontal, 10)
                                .frame(minHeight: 36)
                                .background(Capsule().fill(tint))
                                .frame(minHeight: 44)
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(PressScale())
                            .accessibilityLabel("\(i + 1): \(label(id))")
                            .accessibilityHint("Removes this effect")
                        }
                        if chain.count > 1 {
                            Button {
                                store.setFx(partId, [])
                            } label: { ChipLabel(systemImage: "trash", text: "Clear", tint: .red) }
                            .buttonStyle(PressScale())
                            .accessibilityLabel("Remove all effects")
                        }
                    }
                }
            }
            .padding(14)
            .background(RoundedRectangle(cornerRadius: 20, style: .continuous).fill(Color.pocketCard))

            Text(chain.count >= Self.maxFx ? "Full: remove one to add another." : "Add")
                .font(.footnote.weight(.bold)).textCase(.uppercase).foregroundStyle(.secondary)

            LiveScroll {
                VStack(spacing: 8) {
                    ForEach(Catalog.fx.filter { $0.id != "clean" }, id: \.id) { fx in
                        let inChain = chain.contains(fx.id)
                        let full = chain.count >= Self.maxFx
                        Button {
                            Haptics.tick(settings)
                            store.setFx(partId, chain + [fx.id])
                        } label: {
                            HStack(spacing: 12) {
                                Image(systemName: inChain ? "checkmark.circle.fill" : "plus.circle.fill")
                                    .font(.system(size: 22, weight: .semibold))
                                    .foregroundStyle(inChain ? Color.secondary : tint)
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(fx.label).font(.subheadline.weight(.bold)).foregroundStyle(.primary)
                                    Text(fx.help).font(.caption).foregroundStyle(.secondary)
                                        .multilineTextAlignment(.leading)
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                                Spacer(minLength: 0)
                                if inChain {
                                    Text("Added").font(.caption.weight(.bold)).foregroundStyle(.secondary)
                                }
                            }
                            .padding(.horizontal, 14)
                            .frame(maxWidth: .infinity, minHeight: 56)
                            .background(RoundedRectangle(cornerRadius: 16, style: .continuous).fill(Color.pocketCard))
                            .contentShape(Rectangle())
                            .opacity(inChain || full ? 0.5 : 1)
                        }
                        .buttonStyle(PressScale(scale: 0.97))
                        .disabled(inChain || full)
                        .accessibilityLabel("Add \(fx.label)")
                        .accessibilityValue(inChain ? "Added" : "")
                        .accessibilityHint(fx.help)
                    }
                }
                .padding(.bottom, 20)
            }
        }
        .padding(.horizontal, 18)
        .padding(.top, 20)
        .background(PocketBackground().ignoresSafeArea())
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    private func label(_ id: String) -> String { Catalog.fx.first { $0.id == id }?.label ?? id }
}
