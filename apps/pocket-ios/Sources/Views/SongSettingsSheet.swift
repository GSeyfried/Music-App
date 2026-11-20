// Song settings sheet: the song's tempo/bars/grid/Tightness plus app
// settings (click, count-in, voice drums, keyboard, feel). Closing it keeps
// the main screen as it was.

import SwiftUI

struct SongSettingsSheet: View {
    @EnvironmentObject private var store: SongStore
    @EnvironmentObject private var settings: PocketSettings
    @Environment(\.dismiss) private var dismiss
    @State private var draftTightness: Double?
    @State private var confirmForget = false

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Song settings").font(.system(.title2, design: .rounded).weight(.bold))
                Spacer()
                Button { dismiss() } label: { ChipLabel(text: "Done", tint: .indigo, active: true) }
                    .buttonStyle(PressScale())
            }
            .padding(.horizontal, 20)
            .padding(.top, 20)
            .padding(.bottom, 8)

            LiveScroll {
                VStack(spacing: 14) {
                    section("Song", "music.quarternote.3") {
                        row("Tempo") {
                            Stepperish(value: "\(Int(store.song.bpm)) BPM",
                                       minus: { store.edit { $0.bpm = max(40, $0.bpm - 1) } },
                                       plus: { store.edit { $0.bpm = min(240, $0.bpm + 1) } }, label: "Tempo")
                        }
                        PocketSlider(value: store.song.bpm, range: 40...240, tint: .indigo, step: 1) { v in store.edit { $0.bpm = v } }
                            .accessibilityLabel("Tempo")
                        row("Loop length") {
                            Segments(options: [1, 2, 4, 8], selection: store.song.bars, label: { "\($0)" }) { n in store.edit { $0.setBars(n) } }
                        }
                        row("Grid") {
                            Segments(options: Grid.allCases, selection: store.song.grid, label: { $0 == .triplet ? "1/8T" : $0.rawValue }) { g in store.edit { $0.grid = g } }
                        }
                        TightnessRow(draft: $draftTightness)
                    }

                    section("Click & count-in", "metronome") {
                        row("Click") {
                            Segments(options: ClickMode.allCases, selection: settings.clickMode, label: clickLabel) { m in
                                settings.clickMode = m
                                store.settingsChanged()
                            }
                        }
                        row("Count-in") {
                            Stepperish(value: settings.countInBars == 0 ? "None" : "\(settings.countInBars) \(settings.countInBars == 1 ? "bar" : "bars")",
                                       minus: { settings.countInBars = max(0, settings.countInBars - 1) },
                                       plus: { settings.countInBars = min(2, settings.countInBars + 1) }, label: "Count-in")
                        }
                    }

                    section("Voice drums", "mic") {
                        toggleRow("Use voice", "Beatbox into the selected drum part.",
                                  Binding(get: { settings.voiceListen }, set: { store.setListen($0) }))
                        toggleRow("Hear voice hits", "Play the drum as soon as a sound is recognised.",
                                  Binding(get: { settings.voiceMonitor }, set: { settings.voiceMonitor = $0 }))
                        row("Recognition") {
                            Segments(options: ClassifierMode.allCases, selection: settings.classifier, label: { $0.rawValue.capitalized }) { settings.classifier = $0 }
                        }
                        Text(store.voiceProfileReady ? "Your trained sounds are ready." : "Train your own sounds for better recognition. Auto switches to them once each sound has \(settings.trainingHits) examples.")
                            .font(.footnote).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        HStack(spacing: 8) {
                            Button { store.startTraining(); dismiss() } label: {
                                ChipLabel(systemImage: "waveform.badge.plus", text: store.voiceProfileReady ? "Train again" : "Train my sounds", tint: .green, active: true)
                            }
                            .buttonStyle(PressScale())
                            Button { confirmForget = true } label: {
                                ChipLabel(systemImage: "trash", text: "Forget", tint: .red)
                            }
                            .buttonStyle(PressScale())
                            .disabled(store.profile.isEmpty)
                            .opacity(store.profile.isEmpty ? 0.4 : 1)
                            Spacer(minLength: 0)
                        }
                        Label("Use headphones: sound from the speaker can trigger drums.", systemImage: "headphones")
                            .font(.footnote).foregroundStyle(.secondary)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }

                    section("Keys & feel", "hand.tap") {
                        toggleRow("Note names on keys", nil, Binding(get: { settings.noteLabels }, set: { settings.noteLabels = $0 }))
                        toggleRow("Haptics", "A light tap on pads, keys and Record.", Binding(get: { settings.haptics }, set: { settings.haptics = $0 }))
                        row("Appearance") {
                            Segments(options: AppearanceMode.allCases, selection: settings.appearance, label: { $0.label }) { settings.appearance = $0 }
                        }
                    }
                }
                .padding(.horizontal, 16)
                .padding(.bottom, 30)
            }
        }
        .background(PocketBackground().ignoresSafeArea())
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
        .confirmationDialog("Forget your trained sounds?", isPresented: $confirmForget, titleVisibility: .visible) {
            Button("Forget my sounds", role: .destructive) { store.forgetTraining() }
        }
    }

    private func clickLabel(_ m: ClickMode) -> String {
        switch m {
        case .always: "Always"
        case .recording: "Recording"
        case .off: "Off"
        }
    }

    private func section<C: View>(_ title: String, _ icon: String, @ViewBuilder _ content: () -> C) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Label(title, systemImage: icon)
                .font(.footnote.weight(.bold))
                .textCase(.uppercase)
                .foregroundStyle(.secondary)
            content()
        }
        .pocketCard()
    }

    private func row<C: View>(_ title: String, @ViewBuilder _ content: () -> C) -> some View {
        HStack(spacing: 10) {
            Text(title).font(.subheadline.weight(.semibold)).lineLimit(1).layoutPriority(-1)
            Spacer(minLength: 6)
            content()
        }
        .frame(minHeight: 44)
    }

    private func toggleRow(_ title: String, _ detail: String?, _ isOn: Binding<Bool>) -> some View {
        Button { isOn.wrappedValue.toggle() } label: {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.subheadline.weight(.semibold)).foregroundStyle(.primary)
                    if let detail { Text(detail).font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.leading).fixedSize(horizontal: false, vertical: true) }
                }
                Spacer(minLength: 6)
                SwitchShape(isOn: isOn.wrappedValue)
            }
            .frame(minHeight: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityRepresentation { Toggle(title, isOn: isOn) }
    }
}

/// Switch drawn in SwiftUI (on = filled + knob right).
struct SwitchShape: View {
    let isOn: Bool
    var body: some View {
        Capsule()
            .fill(isOn ? Color.green : Color.pocketTrack)
            .frame(width: 50, height: 30)
            .overlay(alignment: isOn ? .trailing : .leading) {
                Circle().fill(.white).shadow(color: .black.opacity(0.2), radius: 2, y: 1).padding(2.5)
            }
            .animation(.spring(response: 0.25, dampingFraction: 0.8), value: isOn)
    }
}

/// Small segmented control.
struct Segments<T: Hashable>: View {
    let options: [T]
    let selection: T
    let label: (T) -> String
    let choose: (T) -> Void

    var body: some View {
        HStack(spacing: 2) {
            ForEach(options, id: \.self) { o in
                let on = o == selection
                Button { choose(o) } label: {
                    Text(label(o))
                        .font(.footnote.weight(.semibold))
                        .lineLimit(1)
                        .fixedSize()
                        .foregroundStyle(on ? Color.primary : .secondary)
                        .padding(.horizontal, 9)
                        .frame(minWidth: 34, minHeight: 32)
                        .background(RoundedRectangle(cornerRadius: 9, style: .continuous).fill(on ? Color.pocketCard : .clear)
                            .shadow(color: .black.opacity(on ? 0.15 : 0), radius: 2, y: 1))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(label(o))
                .accessibilityAddTraits(on ? .isSelected : [])
            }
        }
        .padding(3)
        .background(RoundedRectangle(cornerRadius: 11, style: .continuous).fill(Color.pocketRaised))
        .frame(minHeight: 44)
    }
}

/// − value + control.
struct Stepperish: View {
    let value: String
    let minus: () -> Void
    let plus: () -> Void
    let label: String

    var body: some View {
        HStack(spacing: 0) {
            CircleIconButton(systemImage: "minus", label: "Decrease \(label)", size: 32, action: minus)
            Text(value)
                .font(.subheadline.weight(.semibold))
                .monospacedDigit()
                .frame(minWidth: 70)
            CircleIconButton(systemImage: "plus", label: "Increase \(label)", size: 32, action: plus)
        }
        .accessibilityElement(children: .contain)
    }
}
