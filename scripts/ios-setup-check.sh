#!/usr/bin/env bash
# VoiceLoop iOS/iPadOS prerequisite check. Read-only: it installs nothing and
# changes nothing. Prints what is present, what is missing, and the command to
# fix each item. See docs/projects/ios/BUILD_AND_INSTALL.md.
#
#   bash scripts/ios-setup-check.sh            # checks only
#   bash scripts/ios-setup-check.sh --compile  # also `cargo check`s the pure-Rust
#                                              # crates for the iOS targets (~1 min)
#
# Exit status: 0 if everything required for `tauri ios dev` is present, 1 otherwise.

set -u
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/.." || exit 1

missing=0
ok()   { printf '  \033[32mok\033[0m    %s\n' "$1"; }
miss() { printf '  \033[31mMISS\033[0m  %s\n        fix: %s\n' "$1" "$2"; missing=$((missing + 1)); }
warn() { printf '  \033[33mwarn\033[0m  %s\n        %s\n' "$1" "$2"; }
info() { printf '        %s\n' "$1"; }

echo "== Host"
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "  iOS builds need macOS. This is $(uname -s)."
  exit 1
fi
ok "macOS $(sw_vers -productVersion) ($(uname -m))"

free_gb=$(df -g / | awk 'NR==2 {print $4}')
if [[ "${free_gb:-0}" -lt 45 ]]; then
  warn "Free disk: ${free_gb} GB" "Xcode needs roughly 40+ GB free to download, expand, and add the iOS platform/simulator runtime. Free space first."
else
  ok "Free disk: ${free_gb} GB"
fi

echo "== Xcode"
dev_dir=$(xcode-select -p 2>/dev/null || true)
if [[ -z "$dev_dir" ]]; then
  miss "No active developer directory" "install Xcode from the App Store, then: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
elif [[ "$dev_dir" == *CommandLineTools* ]]; then
  if [[ -d /Applications/Xcode.app ]]; then
    miss "Xcode is installed but not selected ($dev_dir)" "sudo xcode-select -s /Applications/Xcode.app/Contents/Developer"
  else
    miss "Only Command Line Tools are active ($dev_dir); full Xcode is required for iOS" "install Xcode from the App Store (Tauri: 'Xcode and not Xcode Command Line Tools')"
  fi
else
  ok "Developer dir: $dev_dir"
fi

if xcodebuild -version >/dev/null 2>&1; then
  ok "$(xcodebuild -version | tr '\n' ' ')"
  if xcodebuild -checkFirstLaunchStatus >/dev/null 2>&1; then ok "Xcode first-launch setup done"; else
    miss "Xcode first-launch setup not done" "open Xcode once, or: sudo xcodebuild -runFirstLaunch"; fi
  if xcodebuild -license check >/dev/null 2>&1; then ok "Xcode licence accepted"; else
    miss "Xcode licence not accepted" "sudo xcodebuild -license accept"; fi
else
  miss "xcodebuild unavailable" "see the Xcode item above"
fi

for sdk in iphoneos iphonesimulator; do
  if p=$(xcrun --sdk "$sdk" --show-sdk-path 2>/dev/null); then ok "SDK $sdk: $p"; else
    miss "SDK $sdk not found (Tauri build scripts call 'xcrun --sdk $sdk')" "install Xcode and add the iOS platform in Xcode › Settings › Components"; fi
done

if xcrun simctl list runtimes 2>/dev/null | grep -q "iOS"; then
  ok "Simulator runtime: $(xcrun simctl list runtimes 2>/dev/null | grep -m1 iOS | sed 's/ - .*//')"
else
  miss "No iOS simulator runtime" "Xcode › Settings › Components › iOS (or: xcodebuild -downloadPlatform iOS)"
fi

echo "== Rust"
if command -v rustup >/dev/null 2>&1; then
  ok "$(rustc --version)"
  installed=$(rustup target list --installed 2>/dev/null)
  for t in aarch64-apple-ios aarch64-apple-ios-sim; do
    if grep -qx "$t" <<<"$installed"; then ok "target $t"; else miss "target $t" "rustup target add $t"; fi
  done
  if grep -qx x86_64-apple-ios <<<"$installed"; then ok "target x86_64-apple-ios (Intel simulator; optional on Apple Silicon)"; else
    info "x86_64-apple-ios not installed (only needed for Intel-Mac simulators; Tauri's prerequisites list it)"; fi
else
  miss "rustup" "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

echo "== Tauri and Node"
if command -v node >/dev/null 2>&1; then ok "node $(node --version) at $(command -v node)"; else miss "node" "brew install node@22"; fi
if [[ -x node_modules/.bin/tauri ]]; then ok "tauri-cli $(node_modules/.bin/tauri --version 2>/dev/null | awk '{print $2}') (local)"; else
  miss "local tauri-cli" "npm ci"; fi
case "$(command -v node 2>/dev/null)" in
  *nvm*|*fnm*|*volta*|*asdf*)
    warn "node comes from a version manager" "Xcode's 'Build Rust Code' phase may not see it (Tauri docs). Prefer a Homebrew/system node or expose it on Xcode's PATH." ;;
esac

echo "== Homebrew-installed tools used by 'tauri ios init' (it installs missing ones itself)"
if command -v brew >/dev/null 2>&1; then ok "brew"; else miss "Homebrew" "see https://brew.sh"; fi
if command -v pod >/dev/null 2>&1; then ok "cocoapods $(pod --version 2>/dev/null)"; else
  miss "cocoapods (Tauri iOS prerequisite)" "brew install cocoapods"; fi
if command -v xcodegen >/dev/null 2>&1; then ok "xcodegen $(xcodegen --version 2>/dev/null | awk '{print $NF}')"; else
  warn "xcodegen not installed" "'tauri ios init' generates the Xcode project with XcodeGen and installs it if missing (brew install xcodegen)"; fi
if command -v ios-deploy >/dev/null 2>&1; then ok "ios-deploy"; else
  info "ios-deploy not installed (optional; current Xcode installs via devicectl)"; fi

echo "== Project state"
if [[ -d src-tauri/gen/apple ]]; then ok "src-tauri/gen/apple exists (iOS project generated)"; else
  info "src-tauri/gen/apple absent: run 'npm run tauri ios init' once Xcode is ready"; fi
if [[ -f src-tauri/Info.ios.plist ]] && grep -q NSMicrophoneUsageDescription src-tauri/Info.ios.plist; then
  ok "src-tauri/Info.ios.plist has NSMicrophoneUsageDescription"; else
  miss "NSMicrophoneUsageDescription for iOS" "add src-tauri/Info.ios.plist (see docs/projects/ios/BUILD_AND_INSTALL.md); without it the app is killed on first mic access"; fi
if grep -q '"developmentTeam"' src-tauri/tauri.conf.json 2>/dev/null || [[ -n "${APPLE_DEVELOPMENT_TEAM:-}" ]]; then
  ok "Apple development team configured"; else
  info "No bundle.iOS.developmentTeam / APPLE_DEVELOPMENT_TEAM yet (needed for device builds; pick your team in Xcode › Signing instead if you prefer)"; fi

if [[ "${1:-}" == "--compile" ]]; then
  echo "== Compile probe (cargo check, no SDK needed for these crates)"
  for t in aarch64-apple-ios aarch64-apple-ios-sim; do
    for p in voiceloop-analysis voiceloop-engine; do
      if cargo check -q -p "$p" --target "$t" >/dev/null 2>&1; then ok "cargo check -p $p --target $t"; else
        warn "cargo check -p $p --target $t failed" "re-run it by hand to see the error (another agent may be mid-edit)"; fi
    done
  done
  if cargo check -q -p voiceloop --target aarch64-apple-ios-sim >/dev/null 2>&1; then ok "cargo check -p voiceloop (Tauri) --target aarch64-apple-ios-sim"; else
    info "Tauri crate check fails without the iOS SDK (expected until Xcode is installed)"; fi
fi

echo
if [[ $missing -eq 0 ]]; then
  echo "All required iOS prerequisites are present. Next: npm run tauri ios init"
  exit 0
fi
echo "$missing required item(s) missing. Nothing was installed or changed."
exit 1
