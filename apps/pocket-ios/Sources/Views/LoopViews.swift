// Loop drawings: the drum step grid (tap a cell to add/remove a hit), the
// melody note strip, and the small preview on collapsed part cards. All are
// drawn from the *tightened* positions (Song.tightened), so the dots move as
// Tightness changes; raw beats are never altered.

import SwiftUI

// MARK: Drum step grid

struct StepGrid: View {
    @EnvironmentObject private var settings: PocketSettings
    @Environment(\.isSnapshot) private var isSnapshot
    let song: Song
    let part: Part
    /// Playhead beat (nil when stopped or counting in).
    let playhead: Double?
    let onToggle: (DrumLane, Int) -> Void

    private let lanes = DrumLane.allCases
    private let rowH: CGFloat = 28
    private let gap: CGFloat = 4
    private let header: CGFloat = 16
    private let labelW: CGFloat = 50

    private var perBeat: Int { song.grid.stepsPerBeat }
    private var steps: Int { max(1, Int((song.loopBeats * Double(perBeat)).rounded())) }
    private var gridHeight: CGFloat { header + CGFloat(lanes.count) * rowH + CGFloat(lanes.count - 1) * gap }

    var body: some View {
        HStack(alignment: .top, spacing: 6) {
            VStack(alignment: .leading, spacing: gap) {
                Color.clear.frame(height: header - gap)
                ForEach(lanes) { lane in
                    HStack(spacing: 5) {
                        Circle().fill(settings.laneColor(lane)).frame(width: 7, height: 7)
                        Text(lane.shortLabel)
                            .font(.caption.weight(.semibold))
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                            .minimumScaleFactor(0.8)
                    }
                    .frame(height: rowH)
                }
            }
            .frame(width: labelW, alignment: .leading)
            .accessibilityHidden(true)

            GeometryReader { g in
                let cell = max(CGFloat(settings.gridMinStepPt), g.size.width / CGFloat(steps))
                let total = cell * CGFloat(steps)
                if total <= g.size.width + 0.5 || isSnapshot {
                    canvas(cell: cell).frame(width: total, height: gridHeight, alignment: .leading)
                } else {
                    ScrollViewReader { proxy in
                        ScrollView(.horizontal, showsIndicators: false) {
                            ZStack(alignment: .topLeading) {
                                HStack(spacing: 0) {
                                    ForEach(0..<song.bars, id: \.self) { bar in
                                        Color.clear.frame(width: cell * CGFloat(song.beatsPerBar * perBeat), height: 1).id(bar)
                                    }
                                }
                                canvas(cell: cell)
                            }
                            .frame(width: total, height: gridHeight)
                        }
                        .onChange(of: currentBar) { _, bar in
                            guard let bar else { return }
                            withAnimation(.easeInOut(duration: 0.3)) { proxy.scrollTo(bar, anchor: .leading) }
                        }
                    }
                }
            }
            .frame(height: gridHeight)
            .clipped()
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Step grid, \(part.hits.count) hits")
    }

    private var currentBar: Int? {
        guard let playhead else { return nil }
        return Int(playhead) / max(1, song.beatsPerBar)
    }

    /// Strongest velocity per (lane, step), using tightened positions (same rule as SongStore.toggleStep).
    private var occupancy: [DrumLane: [Int: Double]] {
        var occ: [DrumLane: [Int: Double]] = [:]
        for h in part.hits {
            let s = Int((song.tightened(h.beat) * Double(perBeat)).rounded()) % steps
            occ[h.lane, default: [:]][s] = max(occ[h.lane]?[s] ?? 0, h.velocity)
        }
        return occ
    }

    private func canvas(cell: CGFloat) -> some View {
        let occ = occupancy
        let playStep: Double? = playhead.map { $0 * Double(perBeat) }
        return Canvas { ctx, size in
            let stepsPerBar = song.beatsPerBar * perBeat
            // Bar numbers.
            for bar in 0..<song.bars {
                let x = CGFloat(bar * stepsPerBar) * cell + 4
                ctx.draw(Text("\(bar + 1)").font(.caption2.weight(.bold)).foregroundColor(.secondary), at: CGPoint(x: x, y: 6), anchor: .leading)
            }
            for (r, lane) in lanes.enumerated() {
                let y = header + CGFloat(r) * (rowH + gap)
                let color = settings.laneColor(lane)
                for s in 0..<steps {
                    let rect = CGRect(x: CGFloat(s) * cell + 1.5, y: y, width: cell - 3, height: rowH)
                    let path = Path(roundedRect: rect, cornerRadius: min(7, cell / 3), style: .continuous)
                    if let v = occ[lane]?[s] {
                        ctx.fill(path, with: .color(color.opacity(0.45 + 0.55 * v)))
                        if let p = playStep, abs(p - Double(s)) < 0.5 {
                            ctx.fill(path, with: .color(.white.opacity(0.45)))
                        }
                    } else {
                        let onBeat = s % perBeat == 0
                        ctx.fill(path, with: .color(.primary.opacity(onBeat ? 0.1 : 0.05)))
                    }
                }
                // Where each hit actually plays (tightened), as a dot.
                for h in part.hits where h.lane == lane {
                    var t = song.tightened(h.beat) * Double(perBeat)
                    if t > Double(steps) - 0.5 { t -= Double(steps) }
                    let c = CGPoint(x: (CGFloat(t) + 0.5) * cell, y: y + rowH / 2)
                    ctx.fill(Path(ellipseIn: CGRect(x: c.x - 3, y: c.y - 3, width: 6, height: 6)), with: .color(.white.opacity(0.92)))
                }
            }
            // Bar lines.
            for bar in 1..<max(1, song.bars) {
                let x = CGFloat(bar * stepsPerBar) * cell
                ctx.fill(Path(CGRect(x: x - 0.75, y: header - 2, width: 1.5, height: size.height - header + 2)), with: .color(.primary.opacity(0.35)))
            }
            // Playhead.
            if let p = playStep {
                let x = (CGFloat(p) + 0.5) * cell
                let line = Path(roundedRect: CGRect(x: x - 1.25, y: header - 4, width: 2.5, height: size.height - header + 4), cornerRadius: 1.25)
                ctx.fill(line, with: .color(.primary))
            }
        }
        .contentShape(Rectangle())
        .onTapGesture { loc in
            let r = Int((loc.y - header) / (rowH + gap))
            let s = Int(loc.x / cell)
            guard loc.y >= header, lanes.indices.contains(r), (0..<steps).contains(s) else { return }
            onToggle(lanes[r], s)
        }
        .accessibilityChildren {
            ForEach(lanes) { lane in
                ForEach(0..<steps, id: \.self) { s in
                    let on = occ[lane]?[s] != nil
                    Button("\(lane.label), \(stepName(s))") { onToggle(lane, s) }
                        .accessibilityValue(on ? "On" : "Off")
                }
            }
        }
    }

    private func stepName(_ s: Int) -> String {
        let beat = s / perBeat
        let sub = s % perBeat
        let name = "bar \(beat / song.beatsPerBar + 1), beat \(beat % song.beatsPerBar + 1)"
        return sub == 0 ? name : "\(name), step \(sub + 1)"
    }
}

// MARK: Melody note strip

struct MelodyStrip: View {
    @EnvironmentObject private var settings: PocketSettings
    let song: Song
    let part: Part
    let playhead: Double?

    var body: some View {
        let tint = settings.partColor(.melody)
        let pitches = part.notes.map(\.midi)
        let lo0 = pitches.min() ?? 60, hi0 = pitches.max() ?? 71
        let pad = max(0, 11 - (hi0 - lo0))
        let lo = lo0 - pad / 2, hi = hi0 + (pad - pad / 2)
        ZStack {
            Canvas { ctx, size in
                let L = song.loopBeats
                let rows = CGFloat(hi - lo + 1)
                let rowH = size.height / rows
                // Octave C rows and beat/bar lines.
                for m in lo...hi where m % 12 == 0 {
                    let y = size.height - CGFloat(m - lo + 1) * rowH
                    ctx.fill(Path(CGRect(x: 0, y: y, width: size.width, height: rowH)), with: .color(.primary.opacity(0.05)))
                }
                for b in 0...Int(L) {
                    let x = CGFloat(Double(b) / L) * size.width
                    let isBar = b % song.beatsPerBar == 0
                    ctx.fill(Path(CGRect(x: x - 0.5, y: 0, width: isBar ? 1.5 : 1, height: size.height)),
                             with: .color(.primary.opacity(isBar ? 0.28 : 0.08)))
                }
                for n in part.notes {
                    let x = CGFloat(song.tightened(n.beat) / L) * size.width
                    let w = max(4, CGFloat(n.length / L) * size.width - 2)
                    let y = size.height - CGFloat(n.midi - lo + 1) * rowH
                    let rect = CGRect(x: x + 1, y: y + 0.5, width: w, height: max(3, rowH - 1))
                    ctx.fill(Path(roundedRect: rect, cornerRadius: min(4, rowH / 2), style: .continuous), with: .color(tint.opacity(0.55 + 0.45 * n.velocity)))
                }
                if let p = playhead {
                    let x = CGFloat(p / L) * size.width
                    ctx.fill(Path(roundedRect: CGRect(x: x - 1.25, y: 0, width: 2.5, height: size.height), cornerRadius: 1.25), with: .color(.primary))
                }
            }
            if part.notes.isEmpty {
                Text("No notes yet. Press Record and play the keys.")
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
                    .background(Capsule().fill(Color.pocketCard.opacity(0.9)))
            }
        }
        .frame(height: 96)
        .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(Color.primary.opacity(0.03)))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(part.notes.isEmpty ? "No notes yet" :
            "\(part.notes.count) notes, from \(NoteName.spokenOf(lo0)) to \(NoteName.spokenOf(hi0))")
    }
}

// MARK: Collapsed-card preview

struct MiniPreview: View {
    @EnvironmentObject private var settings: PocketSettings
    let song: Song
    let part: Part
    let playhead: Double?

    var body: some View {
        Canvas { ctx, size in
            let L = song.loopBeats
            for bar in 1..<max(1, song.bars) {
                let x = CGFloat(Double(bar * song.beatsPerBar) / L) * size.width
                ctx.fill(Path(CGRect(x: x - 0.5, y: 0, width: 1, height: size.height)), with: .color(.primary.opacity(0.18)))
            }
            if part.kind == .drums {
                let lanes = DrumLane.allCases
                let rowH = size.height / CGFloat(lanes.count)
                for h in part.hits {
                    let r = CGFloat(lanes.firstIndex(of: h.lane) ?? 0)
                    let x = CGFloat(song.tightened(h.beat) / L) * size.width
                    ctx.fill(Path(roundedRect: CGRect(x: x, y: r * rowH + 0.5, width: 3, height: rowH - 1), cornerRadius: 1),
                             with: .color(settings.laneColor(h.lane).opacity(part.muted ? 0.35 : 1)))
                }
            } else if !part.notes.isEmpty {
                let lo = part.notes.map(\.midi).min()!, hi = max(lo + 6, part.notes.map(\.midi).max()!)
                for n in part.notes {
                    let x = CGFloat(song.tightened(n.beat) / L) * size.width
                    let y = size.height - CGFloat(n.midi - lo + 1) / CGFloat(hi - lo + 1) * size.height
                    let w = max(3, CGFloat(n.length / L) * size.width)
                    ctx.fill(Path(roundedRect: CGRect(x: x, y: y, width: w, height: 3), cornerRadius: 1.5),
                             with: .color(settings.partColor(.melody).opacity(part.muted ? 0.35 : 1)))
                }
            }
            if let p = playhead {
                let x = CGFloat(p / L) * size.width
                ctx.fill(Path(CGRect(x: x - 0.75, y: 0, width: 1.5, height: size.height)), with: .color(.primary.opacity(0.7)))
            }
        }
        .frame(height: 26)
        .accessibilityHidden(true)
    }
}
