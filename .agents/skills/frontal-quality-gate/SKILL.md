---
name: frontal-quality-gate
description: Runs the canonical Frontal quality gate (hook check, 5-file atomic threshold, 500-line check, cargo fmt, conditional wiki check, clippy with -D warnings, and the integration test suite). Use before every commit or after completing an implementation slice.
---

## Execution & Verification
1. Run the bundled gate script from the repository root with `WaitMsBeforeAsync: 10000`:
   `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh`
   (Set `ATOMIC_OVERRIDE=1` only when an indivisible change legitimately exceeds 5 files.)
2. Verify the script exits `0`, confirming:
   - Aggregated static pre-checks before compilation: `.githooks/commit-msg` active, atomic limit (`<= 5` staged or modified tracked files), 500-line ceiling per `.rs` file, `cargo fmt --check`, and `.agents/skills/frontal-wiki-sync/scripts/wiki_sync.sh check` when `.wiki/.git` is present.
   - Zero linter warnings (`cargo clippy --all-targets -- -D warnings`).
   - Full integration suite (`cargo test --test suite --quiet`), including `tests/domains/architecture.rs` invariants (zero comments, 500-line ceiling, version parity, domain path coverage, and extinct-token checks).
3. On failure, remediate all reported errors in a single batch and re-run step 1:
   - Formatting failure: run `cargo fmt`.
   - Clippy failure: resolve all warnings (`-D warnings`).
   - Architecture test failure: inspect `tests/domains/architecture.rs` output for comment violations, files over 500 lines, or metadata mismatches.
