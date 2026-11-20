// VoiceLoop Pocket: SwiftUI app over the Rust engine (crates/voiceloop-ffi).
import SwiftUI

@main
struct PocketApp: App {
    @StateObject private var store = SongStore()
    @Environment(\.scenePhase) private var phase

    var body: some Scene {
        WindowGroup {
            #if DEBUG
            // Debug switch: `-pocket.snapshotDir <path>` renders snapshots and exits (no engine).
            if let dir = SnapshotMode.directory {
                SnapshotRunnerView(dir: dir)
            } else {
                main
            }
            #else
            main
            #endif
        }
        .onChange(of: phase) { _, newPhase in
            // Stop playback when leaving the app (background audio is PENDING).
            if newPhase == .background { store.stop() }
        }
    }

    private var main: some View {
        SongBuilderView()
            .environmentObject(store)
            .environmentObject(store.settings)
            .onAppear {
                NSLog("[pocket] onAppear")
                // Debug switch: `-pocket.skipEngine YES` launches without the audio engine.
                if !UserDefaults.standard.bool(forKey: "pocket.skipEngine") { store.start() }
            }
    }
}
