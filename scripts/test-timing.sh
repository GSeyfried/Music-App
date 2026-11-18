#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"

cargo test -p voiceloop-engine transport::tests
cargo test -p voiceloop-engine clock::tests
cargo test -p voiceloop-engine looper::tests
cargo test -p voiceloop-engine looper::bridge::tests
cargo test -p voiceloop-engine looper::io::tests
