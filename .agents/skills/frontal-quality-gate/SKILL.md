---
name: frontal-quality-gate
description: Run the canonical quality gate for the Frontal repository (cargo fmt, clippy, and full integration test suite including architectural invariants).
---

# Frontal Quality Gate Skill

Executes the canonical validation pipeline required before every commit in the Frontal repository:
1. Canonical formatting verification (`cargo fmt --check`)
2. Strict linting without warnings (`cargo clippy --all-targets -- -D warnings`)
3. Hermetic integration test suite execution (`cargo test --quiet`), validating:
   - Zero comments across all Rust source files
   - Modularity ceiling of 500 lines per `.rs` file
   - Multi-session swap invariants (`App` <-> `TabState`)
   - 60 FPS non-blocking render loop constraints
   - POSIX process group isolation (`setpgid` / `killpg`)
   - Metadata and test count synchronization

## Usage

Run the bundled runner script from the repository root:

```bash
.agents/skills/frontal-quality-gate/scripts/quality_gate.sh
```

## Failure Diagnosis

- **Formatting failure**: Run `cargo fmt` to apply canonical formatting automatically.
- **Clippy failure**: Resolve all reported compiler warnings; compiler flags enforce `-D warnings`.
- **Architecture test failure**: Inspect `tests/domains/architecture.rs` output to identify offending files (comment violations, files exceeding 500 lines, or metadata desynchronization).
