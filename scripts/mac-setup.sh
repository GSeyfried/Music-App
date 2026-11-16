#!/usr/bin/env bash
# macOS developer setup check for VoiceLoop.
#
#   bash scripts/mac-setup.sh           check prerequisites, print what is missing
#   bash scripts/mac-setup.sh --install also install missing pieces (asks before each one)
#
# Installs nothing system-wide without asking. Never runs sudo itself; Apple's
# Command Line Tools installer shows its own dialog.

set -euo pipefail

install=false
[[ "${1:-}" == "--install" ]] && install=true

ok()   { printf '  \033[32mok\033[0m   %s\n' "$1"; }
miss() { printf '  \033[31mmiss\033[0m %s\n' "$1"; missing=$((missing + 1)); }
ask()  { $install && { read -r -p "  Install $1? [y/N] " a; [[ "$a" =~ ^[Yy]$ ]]; }; }

missing=0
[[ "$(uname -s)" == "Darwin" ]] || { echo "This script is for macOS."; exit 1; }
echo "macOS $(sw_vers -productVersion) on $(uname -m)"

echo "Prerequisites:"
if xcode-select -p >/dev/null 2>&1; then
  ok "Xcode Command Line Tools ($(xcode-select -p))"
else
  miss "Xcode Command Line Tools  ->  xcode-select --install"
  if ask "Xcode Command Line Tools"; then xcode-select --install || true; fi
fi

if command -v rustup >/dev/null 2>&1 || [[ -x "$HOME/.cargo/bin/rustup" ]]; then
  export PATH="$HOME/.cargo/bin:$PATH"
  ok "rustup / $(rustc --version 2>/dev/null || echo 'rustc pending first use')"
else
  miss "Rust (rustup)  ->  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y"
  if ask "Rust via rustup (user-level, ~/.cargo and ~/.rustup)"; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    export PATH="$HOME/.cargo/bin:$PATH"
  fi
fi

if command -v node >/dev/null 2>&1; then
  major="$(node -p 'process.versions.node.split(".")[0]')"
  if (( major >= 20 )); then ok "Node $(node --version)"; else miss "Node >= 20 (found $(node --version))  ->  brew install node@22"; fi
else
  miss "Node.js >= 20  ->  brew install node@22  (or https://nodejs.org)"
fi

echo
if (( missing > 0 )); then
  echo "$missing prerequisite(s) missing. Install them, open a new terminal, and rerun this script."
  exit 1
fi

cd "$(dirname "$0")/.."
if [[ ! -d node_modules ]]; then
  echo "Installing npm dependencies (npm ci)..."
  npm ci
fi

cat <<'EOF'
Prerequisites OK. Next, in order:

  npm run typecheck        # TypeScript (seconds)
  npm run engine:test      # first Rust compile of the engine (~1-3 min), unit tests
  npm run engine:meter     # headless: list devices, meter default mic 3 s
  npm run app:dev          # first Tauri compile (~3-10 min), opens VoiceLoop

See docs/MAC_SETUP.md for microphone-permission notes.
EOF
