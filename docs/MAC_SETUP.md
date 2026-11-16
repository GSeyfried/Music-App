# Building and running on macOS

macOS is the primary build and runtime target. The Windows work PC is used for
editing only: **no native compilation happens there.**

## 1. One-time prerequisites

| Tool | Install | Check |
| --- | --- | --- |
| Xcode Command Line Tools | `xcode-select --install` | `xcode-select -p` |
| Rust (rustup, stable) | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` | `rustc --version` |
| Node.js ≥ 20 (22 recommended) | `brew install node@22` or nodejs.org | `node --version` |
| GitHub CLI (optional, for `npm start`) | `brew install gh` | `gh --version` |

Full Xcode is **not** required for the desktop app (only for iOS later).
`rust-toolchain.toml` pins the stable channel and adds `rustfmt` and `clippy`
automatically on first use.

`bash scripts/mac-setup.sh` checks all of this, and with `--install` it offers to
install what is missing (it asks before each item).

## 2. Clone and build

```sh
git clone https://github.com/GSeyfried/Music-App.git
cd Music-App
bash scripts/mac-setup.sh            # checks prereqs, runs npm ci

npm run typecheck                 # TypeScript, seconds
npm run engine:test               # first Rust compile of the engine (1–3 min) + 51 unit tests
npm run engine:meter              # headless mic test: lists devices, meters default input for 3 s
npm run app:dev                   # first Tauri compile (3–10 min), opens the VoiceLoop window
```

Optional:

```sh
npm run refs:fetch                # restore the 14 reference repos (~1 GB, shallow); not needed to build
npm run app:build                 # release .app + .dmg in target/release/bundle/
```

## 3. Microphone permission (read this first)

macOS attributes microphone access to the **responsible app**:

| How you run it | Who macOS asks about | Where to allow it |
| --- | --- | --- |
| `npm run engine:meter`, `npm run app:dev` | **Your terminal** (Terminal, iTerm, VS Code, …) | System Settings › Privacy & Security › Microphone › *terminal app* |
| `VoiceLoop.app` from `app:build` | **VoiceLoop** (uses the text in `src-tauri/Info.plist`) | same panel › VoiceLoop |

If access is denied, CoreAudio usually delivers **silence rather than an
error**. The engine detects 2 s of exact digital silence and shows a
warning in the input panel. In `engine:meter` this appears as a `Warning(SilentInput …)`
line and constant `-100.0 dBFS`. After changing the permission, quit and
relaunch the terminal.

Reset a stuck decision: `tccutil reset Microphone` (resets mic permission for all apps).

## 4. What a successful run looks like

`npm run engine:meter`:

```
2 input device(s):
  * MacBook Pro Microphone [CoreAudio] Some(48000) Hz Some(1) ch Some("f32")
event  Status(EngineStatus { capturing: true, ... })
level  rms   -52.3 dBFS  peak   -38.1 dBFS  frame 4800  dropped 0
...
engine shut down cleanly
```

`npm run app:dev`: the VoiceLoop window opens on **Studio**. The device list
is filled in; **Start input** makes the meter move when you speak, and
**Stop input** releases the mic (the orange mic indicator in the menu bar
turns off). Switching to **Practice Lab** shows the same live input state.

## 5. Notes

- **Apple Silicon and Intel:** builds natively for the host. Universal binary:
  `rustup target add aarch64-apple-darwin x86_64-apple-darwin && npm run tauri build -- --target universal-apple-darwin`.
- **Unsigned local builds:** the first launch of the `.app` may need right-click › Open.
  `Entitlements.plist` (`com.apple.security.device.audio-input`) takes effect when
  the app is signed with the hardened runtime (distribution, later).
- **CI:** `.github/workflows/macos.yml` runs the same checks on a GitHub macOS
  runner. It is **manual only** (Actions › macOS build › Run workflow) to avoid
  spending runner minutes unexpectedly.
- If the frontend dev server port is busy: Vite uses 1420 (`strictPort`). Free
  it, or change both `vite.config.ts` and `tauri.conf.json > build.devUrl`.
