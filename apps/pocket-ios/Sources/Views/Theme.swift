// Song Builder look: colours (from PocketSettings, `songBuilder.color.*`),
// card surfaces, haptics and small custom controls. The controls are drawn in
// SwiftUI (so they also render in snapshot mode) and expose the native
// control to VoiceOver through `accessibilityRepresentation`.

import SwiftUI
import UIKit

// MARK: Colours

extension Color {
    init(hex: String) {
        let s = hex.trimmingCharacters(in: CharacterSet(charactersIn: "# "))
        var v: UInt64 = 0
        Scanner(string: s).scanHexInt64(&v)
        self.init(red: Double((v >> 16) & 0xFF) / 255, green: Double((v >> 8) & 0xFF) / 255, blue: Double(v & 0xFF) / 255)
    }

    /// Light/dark pair resolved from the SwiftUI environment.
    static func dynamic(light: UIColor, dark: UIColor) -> Color {
        Color(UIColor { $0.userInterfaceStyle == .dark ? dark : light })
    }

    static let pocketCard = dynamic(light: .white, dark: UIColor(red: 0.098, green: 0.094, blue: 0.13, alpha: 1))
    static let pocketRaised = dynamic(light: UIColor(red: 0.95, green: 0.95, blue: 0.97, alpha: 1), dark: UIColor(red: 0.15, green: 0.145, blue: 0.19, alpha: 1))
    static let pocketStroke = dynamic(light: UIColor.black.withAlphaComponent(0.07), dark: UIColor.white.withAlphaComponent(0.08))
    static let pocketTrack = dynamic(light: UIColor.black.withAlphaComponent(0.08), dark: UIColor.white.withAlphaComponent(0.1))
}

extension PocketSettings {
    func color(_ key: String) -> Color { Color(hex: colorHex(key)) }
    func laneColor(_ lane: DrumLane) -> Color { color(lane.rawValue) }
    func partColor(_ kind: PartKind) -> Color { color(kind.rawValue) }
    var recordColor: Color { color("record") }
}

extension AppearanceMode {
    var colorScheme: ColorScheme? {
        switch self {
        case .dark: .dark
        case .light: .light
        case .system: nil
        }
    }
}

extension DrumLane {
    var shortLabel: String {
        switch self {
        case .kick: "Kick"
        case .snare: "Snare"
        case .hat: "Hat"
        case .clap: "Clap"
        case .openHat: "Open"
        }
    }
}

enum NoteName {
    static let names = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"]
    static let spoken = ["C", "C sharp", "D", "D sharp", "E", "F", "F sharp", "G", "G sharp", "A", "A sharp", "B"]
    static func of(_ midi: Int) -> String { "\(names[((midi % 12) + 12) % 12])\(midi / 12 - 1)" }
    static func spokenOf(_ midi: Int) -> String { "\(spoken[((midi % 12) + 12) % 12]) \(midi / 12 - 1)" }
}

// MARK: Snapshot flag

private struct SnapshotKey: EnvironmentKey { static let defaultValue = false }

extension EnvironmentValues {
    /// True while rendering debug snapshots: scroll views become plain stacks.
    var isSnapshot: Bool {
        get { self[SnapshotKey.self] }
        set { self[SnapshotKey.self] = newValue }
    }
}

/// A vertical scroll view, or a plain stack in snapshot mode (ImageRenderer cannot draw UIScrollView).
struct LiveScroll<Content: View>: View {
    @Environment(\.isSnapshot) private var isSnapshot
    var axis: Axis.Set = .vertical
    @ViewBuilder var content: Content
    var body: some View {
        if isSnapshot {
            VStack(spacing: 0) { content }.frame(maxWidth: .infinity, minHeight: 0, maxHeight: .infinity, alignment: .top).clipped()
        } else {
            ScrollView(axis, showsIndicators: false) { content }
        }
    }
}

// MARK: Surfaces

struct PocketBackground: View {
    @Environment(\.colorScheme) private var scheme
    var body: some View {
        let top = scheme == .dark ? Color(hex: "#0C0B12") : Color(hex: "#F4F3F8")
        let bottom = scheme == .dark ? Color(hex: "#13101D") : Color(hex: "#ECEAF3")
        LinearGradient(colors: [top, bottom], startPoint: .top, endPoint: .bottom)
    }
}

struct CardStyle: ViewModifier {
    @Environment(\.colorScheme) private var scheme
    var accent: Color? = nil
    var padding: CGFloat = 14
    func body(content: Content) -> some View {
        content
            .padding(padding)
            .background(RoundedRectangle(cornerRadius: 24, style: .continuous).fill(Color.pocketCard))
            .overlay(
                RoundedRectangle(cornerRadius: 24, style: .continuous)
                    .strokeBorder(accent.map { $0.opacity(0.75) } ?? Color.pocketStroke, lineWidth: accent == nil ? 1 : 1.5)
            )
            .shadow(color: .black.opacity(scheme == .dark ? 0 : 0.06), radius: 12, y: 4)
    }
}

extension View {
    func pocketCard(accent: Color? = nil, padding: CGFloat = 14) -> some View {
        modifier(CardStyle(accent: accent, padding: padding))
    }
}

// MARK: Haptics

@MainActor
enum Haptics {
    private static let soft = UIImpactFeedbackGenerator(style: .soft)
    private static let medium = UIImpactFeedbackGenerator(style: .medium)
    private static let rigid = UIImpactFeedbackGenerator(style: .rigid)
    private static let selection = UISelectionFeedbackGenerator()

    static func pad(_ s: PocketSettings) { if s.haptics { medium.impactOccurred(intensity: 0.75) } }
    static func key(_ s: PocketSettings) { if s.haptics { soft.impactOccurred(intensity: 0.7) } }
    static func record(_ s: PocketSettings) { if s.haptics { rigid.impactOccurred() } }
    static func tick(_ s: PocketSettings) { if s.haptics { selection.selectionChanged() } }
}

// MARK: Controls

/// Round icon button with a 44 pt target.
struct CircleIconButton: View {
    let systemImage: String
    let label: String
    var size: CGFloat = 40
    var tint: Color = .primary
    var filled = false
    let action: () -> Void
    @Environment(\.isEnabled) private var enabled

    var body: some View {
        Button(action: action) {
            Image(systemName: systemImage)
                .font(.system(size: size * 0.4, weight: .semibold))
                .foregroundStyle(filled ? Color.white : tint)
                .frame(width: size, height: size)
                .background(Circle().fill(filled ? tint : Color.pocketRaised))
                .opacity(enabled ? 1 : 0.35)
                .frame(width: max(44, size), height: max(44, size))
                .contentShape(Rectangle())
        }
        .buttonStyle(PressScale())
        .accessibilityLabel(label)
    }
}

/// Springy press feedback for custom buttons.
struct PressScale: ButtonStyle {
    var scale: CGFloat = 0.92
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .scaleEffect(configuration.isPressed ? scale : 1)
            .animation(.spring(response: 0.25, dampingFraction: 0.6), value: configuration.isPressed)
    }
}

/// Capsule label used for chips and menu triggers.
struct ChipLabel: View {
    var systemImage: String? = nil
    let text: String
    var tint: Color = .primary
    var active = false
    var chevron = false

    var body: some View {
        HStack(spacing: 6) {
            if let systemImage { Image(systemName: systemImage).font(.system(size: 13, weight: .semibold)) }
            Text(text).font(.subheadline.weight(.semibold)).lineLimit(1)
            if chevron { Image(systemName: "chevron.down").font(.system(size: 10, weight: .bold)).opacity(0.55) }
        }
        .foregroundStyle(active ? Color.white : tint)
        .padding(.horizontal, 12)
        .frame(minHeight: 36)
        .background(Capsule().fill(active ? AnyShapeStyle(tint) : AnyShapeStyle(Color.pocketRaised)))
        .frame(minHeight: 44)
        .contentShape(Rectangle())
    }
}

/// A slider drawn in SwiftUI. `onDrag` reports the live value; `onCommit`
/// fires once on release (one undo step per gesture).
struct PocketSlider: View {
    let value: Double
    let range: ClosedRange<Double>
    var tint: Color = .accentColor
    var step: Double? = nil
    var onDrag: ((Double?) -> Void)? = nil
    let onCommit: (Double) -> Void
    @State private var drag: Double?

    private let thumb: CGFloat = 26

    var body: some View {
        GeometryReader { g in
            let w = g.size.width
            let span = range.upperBound - range.lowerBound
            let frac = CGFloat(((drag ?? value) - range.lowerBound) / max(1e-9, span)).clamped(0, 1)
            let x = thumb / 2 + frac * (w - thumb)
            ZStack(alignment: .leading) {
                Capsule().fill(Color.pocketTrack).frame(height: 6)
                Capsule().fill(tint).frame(width: x, height: 6)
                Circle()
                    .fill(.white)
                    .shadow(color: .black.opacity(0.25), radius: 3, y: 1)
                    .overlay(Circle().stroke(tint.opacity(0.5), lineWidth: drag == nil ? 0 : 3))
                    .frame(width: thumb, height: thumb)
                    .scaleEffect(drag == nil ? 1 : 1.12)
                    .offset(x: x - thumb / 2)
            }
            .frame(height: g.size.height)
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { v in
                        let f = Double(((v.location.x - thumb / 2) / max(1, w - thumb)).clamped(0, 1))
                        var nv = range.lowerBound + f * span
                        if let step { nv = (nv / step).rounded() * step }
                        if drag == nil { withAnimation(.spring(response: 0.2)) { drag = nv } } else { drag = nv }
                        onDrag?(nv)
                    }
                    .onEnded { _ in
                        if let d = drag { onCommit(d) }
                        withAnimation(.spring(response: 0.25)) { drag = nil }
                        onDrag?(nil)
                    }
            )
        }
        .frame(height: 44)
        .accessibilityRepresentation {
            Slider(value: Binding(get: { value }, set: { onCommit($0) }), in: range)
        }
    }
}

/// Capsule on/off switch with icon; reads as a toggle to VoiceOver.
struct CapsuleToggle: View {
    let label: String
    let systemImage: String
    var iconOnly = false
    @Binding var isOn: Bool
    var tint: Color = .green

    var body: some View {
        Button { isOn.toggle() } label: {
            if iconOnly {
                Image(systemName: systemImage)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(isOn ? Color.white : .secondary)
                    .frame(width: 44, height: 36)
                    .background(Capsule().fill(isOn ? AnyShapeStyle(tint) : AnyShapeStyle(Color.pocketRaised)))
                    .overlay { if !isOn { Image(systemName: "line.diagonal").font(.system(size: 22, weight: .bold)).foregroundStyle(.secondary) } }
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
            } else {
                ChipLabel(systemImage: systemImage, text: label, tint: isOn ? tint : .secondary, active: isOn)
            }
        }
        .buttonStyle(PressScale())
        .accessibilityRepresentation { Toggle(label, isOn: $isOn) }
    }
}

extension Comparable {
    func clamped(_ lo: Self, _ hi: Self) -> Self { min(max(self, lo), hi) }
}

/// A Menu, or just its label in snapshot mode (ImageRenderer cannot draw UIKit menus).
struct PocketMenu<Content: View, MenuLabel: View>: View {
    @Environment(\.isSnapshot) private var isSnapshot
    @ViewBuilder var content: Content
    @ViewBuilder var label: MenuLabel
    var body: some View {
        if isSnapshot { label } else { Menu { content } label: { label } }
    }
}
