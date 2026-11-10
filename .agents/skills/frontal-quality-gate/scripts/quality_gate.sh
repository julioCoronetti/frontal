#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet

echo "Quality gate passed: fmt, clippy, and full test suite verified."
