#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(git rev-parse --show-toplevel)"
cd "$ROOT_DIR"

if [[ "${ATOMIC_OVERRIDE:-0}" != "1" ]]; then
  staged_count=$(git diff --cached --name-only | wc -l)
  if [[ "$staged_count" -gt 0 ]]; then
    file_count="$staged_count"
  else
    file_count=$(git status --porcelain | wc -l)
  fi

  if [[ "$file_count" -gt 5 ]]; then
    echo "Error: Atomic commit threshold exceeded ($file_count files modified, max 5 allowed)." >&2
    echo "Decompose changes into smaller atomic commits, or set ATOMIC_OVERRIDE=1." >&2
    exit 1
  fi
fi

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet

echo "Quality gate passed: atomic limit, fmt, clippy, and full test suite verified."
