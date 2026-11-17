#!/usr/bin/env bash

set -uo pipefail

export PATH="$HOME/.cargo/bin:$PATH"

repo_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)" || exit 1
cd "$repo_root" || exit 1

quick=false
case "${1:-}" in
  "")
    ;;
  --quick)
    quick=true
    ;;
  -h|--help)
    printf 'Usage: %s [--quick]\n' "$0"
    printf '  --quick  Skip the Tauri compile step.\n'
    exit 0
    ;;
  *)
    printf 'Unknown option: %s\n' "$1" >&2
    printf 'Usage: %s [--quick]\n' "$0" >&2
    exit 2
    ;;
esac

failures=0

run_step() {
  local label="$1"
  shift

  printf '\n==> %s\n' "$label"
  if "$@"; then
    printf 'PASS: %s\n' "$label"
  else
    local status=$?
    printf 'FAIL: %s (exit %d)\n' "$label" "$status" >&2
    failures=$((failures + 1))
  fi
}

run_step "Rust formatting" cargo fmt --all -- --check
run_step "Rust Clippy" cargo clippy --workspace --all-targets -- -D warnings
run_step "Rust workspace tests (no audio devices)" cargo test --workspace
run_step "Frontend dependency install" npm ci
run_step "Frontend type-check" npm run typecheck
run_step "Frontend tests (if present)" npm test --if-present
run_step "Frontend build" npm run build

if [[ "$quick" == true ]]; then
  printf '\nSKIP: Tauri compile (--quick)\n'
else
  run_step "Tauri compile" cargo build -p voiceloop
fi

run_step "Reference manifest JSON" node -e \
  "JSON.parse(require('fs').readFileSync('reference-packages/manifest.json', 'utf8'))"

printf '\n'
if ((failures > 0)); then
  printf '%d check(s) failed.\n' "$failures" >&2
  exit 1
fi

printf 'All checks passed.\n'
