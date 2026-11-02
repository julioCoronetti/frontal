#!/usr/bin/env bash
set -euo pipefail

echo "=== [Gate 1/6] Formatting Check (cargo fmt) ==="
cargo fmt --check

echo "=== [Gate 2/6] 500-Line Ceiling Check ==="
oversized=$(find src tests -name "*.rs" -exec awk 'FNR > 500 { print FILENAME ":" FNR; nextfile }' {} +)
if [[ -n "$oversized" ]]; then
  echo "Error: 500-line ceiling violated in the following files:" >&2
  echo "$oversized" >&2
  exit 1
fi

echo "=== [Gate 3/6] Zero Comments Check ==="
rs_files=$(find src tests -name "*.rs")
comments=$(awk '{ line=$0; gsub(/"([^"\\]|\\.)*"/, "\"\"", line); if (line ~ /(\/\/|\/\*|#\[doc)/) print FILENAME ":" FNR ":" $0 }' $rs_files)
if [[ -n "$comments" ]]; then
  echo "Error: Comments detected in code:" >&2
  echo "$comments" >&2
  exit 1
fi

echo "=== [Gate 4/6] Compilation & Targets Check ==="
cargo check --all-targets

echo "=== [Gate 5/6] Strict Linting (cargo clippy) ==="
cargo clippy --all-targets -- -D warnings

echo "=== [Gate 6/6] Unified Test Suite (cargo test) ==="
cargo test --quiet

echo "=== All gates passed successfully! ==="
