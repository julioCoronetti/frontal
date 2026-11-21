---
name: frontal-quality-gate
description: Runs the canonical Frontal quality gate (5-file atomic threshold, cargo fmt, clippy with -D warnings, and the integration test suite). Use before every commit or after completing an implementation slice.
---

## Execution & Verification
1. Run the bundled gate script from the repository root:
   `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh`
   (Set `ATOMIC_OVERRIDE=1` only when an indivisible change legitimately exceeds 5 files.)
2. Verify the script exits `0`, confirming:
   - Atomic limit check (`<= 5` modified files, unless `ATOMIC_OVERRIDE=1`).
   - Canonical formatting (`cargo fmt --check`).
   - Zero linter warnings (`cargo clippy --all-targets -- -D warnings`).
   - Full integration suite (`cargo test --quiet`), including `tests/domains/architecture.rs` invariants (zero comments, 500-line ceiling per `.rs` file, version and test-count metadata sync).
3. On failure, remediate and re-run step 1:
   - Formatting failure: run `cargo fmt`.
   - Clippy failure: resolve all warnings (`-D warnings`).
   - Architecture test failure: inspect `tests/domains/architecture.rs` output for comment violations, files over 500 lines, or metadata mismatches.
