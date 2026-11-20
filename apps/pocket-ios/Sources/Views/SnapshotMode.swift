// Debug-only snapshot mode. Launch with `-pocket.snapshotDir <path>`: instead
// of running, the app renders the Song Builder in a few representative states
// with ImageRenderer into PNGs in that folder, then exits. It never starts the
// engine, never opens the mic and never touches the saved song or settings
// (preview store + in-memory settings).

#if DEBUG
import SwiftUI

@MainActor
enum SnapshotMode {
    static var directory: String? { UserDefaults.standard.string(forKey: "pocket.snapshotDir") }

    /// In-memory defaults so snapshots ignore launch arguments and saved settings.
    private final class MemoryDefaults: UserDefaults {
        var values: [String: Any] = [:]
        override func object(forKey key: String) -> Any? { values[key] }
        override func set(_ value: Any?, forKey key: String) { values[key] = value }
    }

    private struct Shot {
        let name: String
        var size = CGSize(width: 402, height: 874)
        var schemes: [ColorScheme] = [.dark, .light]
        var sheet: Sheet? = nil
        /// View-local state to start in (EnvironmentValues.snapshotPose).
        var viewPose: String? = nil
        let pose: @MainActor (SongStore) -> Void
    }

    private enum Sheet { case settings, fx }

    /// Adds a voice → piano part (C major, an octave down) and selects it.
    private static func addVoicePiano(_ st: SongStore) {
        st.addPart(.voicePiano)
        guard let id = st.selectedPartId else { return }
        st.edit { s in
            s.update(id) { p in
                p.enabledKeys = (0..<12).map { [0, 2, 4, 5, 7, 9, 11].contains($0) }
                p.transpose = -12
            }
            for (midi, beat, len) in [(48, 0.0, 1.5), (52, 2.0, 1.0), (55, 3.0, 1.0), (53, 4.0, 2.0)] {
                s.addNote(id, midi: midi, beat: beat + 0.05, length: len, velocity: 0.8)
            }
        }
    }

    static func sampleSong() -> Song {
        var s = Song(bpm: 96, beatsPerBar: 4, bars: 2, grid: .eighth, tightness: 0.65)
        s.name = "Late night loop"
        let d = s.addPart(.voiceDrums)
        s.applyStarter(StarterBeat.all[0], to: d)
        s.addHit(d, lane: .snare, beat: 3.62, velocity: 0.7, source: .voice)
        s.addHit(d, lane: .kick, beat: 6.41, velocity: 0.8, source: .voice)
        s.addHit(d, lane: .clap, beat: 7.02, velocity: 0.9, source: .pad)
        s.addHit(d, lane: .openHat, beat: 7.48, velocity: 0.6, source: .pad)
        s.update(d) { $0.fx = "room" }
        let m = s.addPart(.piano)
        for (midi, beat, len) in [(60, 0.0, 1.0), (64, 1.0, 0.5), (67, 1.5, 1.0), (69, 2.5, 0.5), (67, 3.0, 1.0),
                                  (64, 4.0, 1.0), (62, 5.0, 0.5), (64, 5.5, 0.5), (60, 6.0, 1.5)] {
            s.addNote(m, midi: midi, beat: beat + 0.04, length: len, velocity: 0.85)
        }
        s.update(m) { $0.instrument = .softSynth; $0.fx = "echo" }
        let d2 = s.addPart(.drumPads)
        s.applyStarter(StarterBeat.all[3], to: d2)
        s.update(d2) { $0.muted = true; $0.kit = "808"; $0.name = "808 groove" }
        return s
    }

    private static let shots: [Shot] = [
        Shot(name: "1-drums-recording") { st in
            st.posePreview(playing: true, recording: true, beat: 5.3, pass: 1, micState: "on", lastHeard: (.kick, 212))
        },
        Shot(name: "2-count-in", schemes: [.dark]) { st in
            st.posePreview(playing: true, recording: true, beat: -2.6, pass: -1, micState: "on")
        },
        Shot(name: "3-melody") { st in
            st.edit { $0.parts.swapAt(0, 1) }
            st.select(st.song.parts[0].id)
            st.posePreview(playing: true, beat: 2.7, pass: 3)
        },
        Shot(name: "4-training", schemes: [.dark]) { st in
            st.posePreview(micState: "on", training: (.snare, 2))
        },
        Shot(name: "5-empty-error", schemes: [.dark]) { st in
            st.edit { $0.update($0.parts[0].id) { $0.hits.removeAll() } }
            st.posePreview(micState: "opening", error: "Audio output could not start (device busy).")
        },
        Shot(name: "6-settings", sheet: .settings) { st in st.posePreview() },
        Shot(name: "6b-settings-voice", schemes: [.dark], sheet: .settings, viewPose: "scrollBottom") { st in
            st.posePreview(listening: true, inputLevelDb: -44)
        },
        Shot(name: "7-ipad", size: CGSize(width: 820, height: 1180), schemes: [.dark]) { st in
            st.posePreview(playing: true, beat: 1.2, pass: 0, micState: "on")
        },
        Shot(name: "8-voice-piano-singing") { st in
            addVoicePiano(st)
            st.posePreview(playing: true, beat: 2.2, pass: 1, micState: "on", listening: true, voiceKey: 50, sungMidi: 61.3, inputLevelDb: -28)
        },
        Shot(name: "9-edit-keys", schemes: [.dark], viewPose: "editKeys") { st in
            addVoicePiano(st)
            st.posePreview(micState: "on", listening: true, inputLevelDb: -62)
        },
        Shot(name: "10-add-track", schemes: [.dark, .light], viewPose: "scrollBottom") { st in
            // Scrolled to the end: the last part's controls (FX chain, starter, more) and the add-track grid.
            let id = st.song.parts[2].id
            st.setFx(id, ["punch", "lofi"])
            st.select(id)
            st.posePreview()
        },
        Shot(name: "11-fx-sheet", schemes: [.dark, .light], sheet: .fx) { st in
            st.setFx(st.song.parts[0].id, ["room", "punch"])
            st.posePreview()
        },
        Shot(name: "12-drum-pads", schemes: [.dark]) { st in
            st.select(st.song.parts[2].id)
            st.posePreview(playing: true, beat: 3.1, pass: 2)
        },
        Shot(name: "13-mic-recording", schemes: [.dark]) { st in
            st.posePreview(playing: true, recording: true, beat: 1.6, pass: 2, micState: "on", lastHeard: (.clap, 1400),
                           listening: true, inputLevelDb: -35)
        },
    ]

    @MainActor
    static func run(dir: String) {
        let folder = URL(fileURLWithPath: dir, isDirectory: true)
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        for shot in shots {
            for scheme in shot.schemes {
                let settings = PocketSettings(store: MemoryDefaults(suiteName: "pocket.snapshot.memory")!)
                let store = SongStore(settings: settings, previewSong: sampleSong())
                shot.pose(store)
                let screen = ZStack(alignment: .bottom) {
                    PocketBackground()
                    Color.pocketCard.frame(height: 40)
                    Group {
                        switch shot.sheet {
                        case .settings: SongSettingsSheet()
                        case .fx: FxSheet(partId: store.song.parts[0].id)
                        case nil: SongBuilderView()
                        }
                    }
                    .padding(.top, 54)
                    .padding(.bottom, 30)
                }
                .frame(width: shot.size.width, height: shot.size.height)
                .environmentObject(store)
                .environmentObject(settings)
                .environment(\.colorScheme, scheme)
                .environment(\.isSnapshot, true)
                .environment(\.snapshotPose, shot.viewPose)

                let renderer = ImageRenderer(content: screen)
                renderer.scale = 3
                renderer.proposedSize = ProposedViewSize(shot.size)
                let file = folder.appendingPathComponent("\(shot.name)-\(scheme == .dark ? "dark" : "light").png")
                if let data = renderer.uiImage?.pngData() {
                    try? data.write(to: file)
                    NSLog("[pocket] snapshot %@", file.path)
                } else {
                    NSLog("[pocket] snapshot failed: %@", shot.name)
                }
            }
        }
        exit(0)
    }
}

struct SnapshotRunnerView: View {
    let dir: String
    var body: some View {
        Color.black.task { SnapshotMode.run(dir: dir) }
    }
}
#endif
