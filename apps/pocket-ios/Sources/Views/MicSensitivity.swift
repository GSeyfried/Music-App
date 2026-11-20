// "Mic sensitivity": the one slider that sets every loudness gate
// (songBuilder.micSensitivity → micGateDb), with a live input meter under it
// and a marker at the gate, so you can see whether your voice clears it.
// Shown in the dock for voice parts and in the settings sheet.

import SwiftUI

struct MicSensitivityControl: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    /// Dock: one tight row. Sheet: title and explanation.
    var compact = true
    /// Sensitivity while the slider is dragged (the gate marker follows live).
    @State private var draft: Double?

    private var sensitivity: Double { draft ?? settings.micSensitivity }
    private var gateDb: Double { draft.map(PocketSettings.gateDb(sensitivity:)) ?? settings.micGateDb }

    var body: some View {
        VStack(alignment: .leading, spacing: compact ? 2 : 6) {
            if !compact {
                Text("Mic sensitivity").font(.subheadline.weight(.semibold))
            }
            HStack(spacing: 10) {
                if compact {
                    VStack(alignment: .leading, spacing: 0) {
                        Text("Mic").font(.caption.weight(.bold))
                        Text("sensitivity").font(.caption2.weight(.semibold)).foregroundStyle(.secondary)
                    }
                    .frame(width: 66, alignment: .leading)
                    .accessibilityHidden(true)
                }
                VStack(spacing: 0) {
                    PocketSlider(value: settings.micSensitivity, range: 0...1, tint: .green, step: 0.05, onDrag: { draft = $0 }) { v in
                        settings.micSensitivity = v
                        store.settingsChanged()
                    }
                    .frame(height: compact ? 30 : 44)
                    .accessibilityLabel("Mic sensitivity")
                    .accessibilityValue("\(Int((sensitivity * 100).rounded())) percent, hears sounds above \(Int(gateDb.rounded())) decibels")
                    LevelMeter(levelDb: store.inputLevelDb, gateDb: gateDb, floorDb: settings.micMeterFloorDb)
                        .padding(.horizontal, 13) // line up with the slider's thumb travel
                }
                Text("\(Int((sensitivity * 100).rounded()))%")
                    .font(.caption.weight(.semibold))
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .frame(width: 38, alignment: .trailing)
                    .accessibilityHidden(true)
            }
            if !compact {
                Text("Higher hears quieter sounds (and more room noise). The bar shows the mic; sounds past the white mark count. Now: \(Int(gateDb.rounded())) dB.")
                    .font(.footnote).foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// Horizontal input meter, floorDb…0 dBFS, with a marker at the gate.
/// Above the gate the bar is green; below it, grey.
/// The text state ("Heard"/"Too quiet"/"Mic off") is for VoiceOver, never colour-only.
struct LevelMeter: View {
    let levelDb: Double?
    let gateDb: Double
    let floorDb: Double

    private func frac(_ db: Double) -> CGFloat { CGFloat((db - floorDb) / max(1, -floorDb)).clamped(0, 1) }

    var body: some View {
        let loud = (levelDb ?? -200) >= gateDb
        GeometryReader { g in
            let w = g.size.width
            ZStack(alignment: .leading) {
                Capsule().fill(Color.pocketTrack)
                if let levelDb {
                    Capsule()
                        .fill(loud ? Color.green : Color.secondary.opacity(0.55))
                        .frame(width: max(4, w * frac(levelDb)))
                        .animation(.linear(duration: 0.06), value: levelDb)
                }
                // Gate marker: a tall white tick with a dark edge (visible on both fills).
                RoundedRectangle(cornerRadius: 1)
                    .fill(Color.white)
                    .overlay(RoundedRectangle(cornerRadius: 1).stroke(Color.black.opacity(0.35), lineWidth: 0.5))
                    .frame(width: 3, height: 12)
                    .offset(x: w * frac(gateDb) - 1.5)
            }
            .frame(height: 6)
            .frame(maxHeight: .infinity)
        }
        .frame(height: 12)
        .accessibilityElement()
        .accessibilityLabel("Mic level")
        .accessibilityValue(levelDb == nil ? "Mic off" : (loud ? "Heard" : "Too quiet"))
    }
}

/// Voice docks on the phone speaker: live voice sounds are off (they'd leak into
/// the mic). Pads and keys still light up on every recognised sound.
struct SpeakerMonitorHint: View {
    @EnvironmentObject private var store: SongStore

    var body: some View {
        if !store.monitorVoice && store.outputIsSpeaker {
            Label("Speaker: your voice sounds are silent so they don't leak into the mic. Use headphones to hear them.",
                  systemImage: "speaker.slash")
                .font(.caption2.weight(.medium))
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
