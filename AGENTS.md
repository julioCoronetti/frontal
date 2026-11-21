## Guardrails
- Never add external crate dependencies to `Cargo.toml` (package version `0.20.0`) without explicit authorization.
- Keep `Cargo.toml` version `0.20.0` immutable during `Card-Status: In Progress` commits; bump patch (`0.20.x`) or minor (`0.x.0`) only in an authorized `Card-Status: Done` commit with an annotated `vX.Y.Z` tag and identical CLI (`-V`) and TUI (`/help`) versions.
- Enforce a 500-line ceiling per `.rs` file (partition larger modules with public re-exports) and zero comments, `#[doc]`/`#![doc]` attributes, `#[rustfmt::skip]`, or `include!`/`include_str!`/`include_bytes!` macros in Rust source files.
- Limit every commit to 1 logical change touching at most 5 modified files (unless `ATOMIC_OVERRIDE=1`) and ensure `.wiki/` has a clean working tree before committing `Card-Status: In Review` or `Done`.

## Commands
- Full quality gate (atomic check, `fmt`, `clippy -D warnings`, 276 tests): `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh`
- Full integration test harness (276 tests across 8 domains in `tests/suite.rs`): `cargo test --quiet`
- Architecture invariants (`tests/domains/architecture.rs`, 4 tests): `cargo test --test suite domains::architecture`
- Protocol & CLI (`tests/domains/protocol_and_cli.rs`, 21 tests): `cargo test --test suite domains::protocol_and_cli`
- Engine & session (`tests/domains/engine_and_session/`, 35 tests): `cargo test --test suite domains::engine_and_session`
- Cache & markdown (`tests/domains/cache_and_markdown/`, 30 tests): `cargo test --test suite domains::cache_and_markdown`
- Viewport & clipboard (`tests/domains/viewport_and_clipboard.rs`, 16 tests): `cargo test --test suite domains::viewport_and_clipboard`
- Editor & SQLite history (`tests/domains/editor_and_history/`, 36 tests): `cargo test --test suite domains::editor_and_history`
- Tabs & modals (`tests/domains/tabs_and_modals/`, 94 tests): `cargo test --test suite domains::tabs_and_modals`
- End-to-end TUI (`tests/domains/tui_integration/`, 40 tests): `cargo test --test suite domains::tui_integration`

## Invariants & Traps
- Store immutable AST (`MessageBlock` with styled spans) in RenderCache L1 and width-wrapped visual lines indexed by `(width, revision)` in L2; invalidate only L2 on resize or block toggle to sustain 60 FPS streaming without re-parsing history.
- Never execute synchronous disk I/O, SQLite queries, or subprocess scans inside the `tokio::select!` event loop in `src/ui/mod.rs`.
- Encapsulate per-session state in `TabState`, access the active tab in `App` via O(1) index and `Deref`/`DerefMut` without memory swaps, and route all stream events through `TabState::apply_stream_event`.
- Spawn every child engine subprocess in a dedicated POSIX process group (`setpgid(0, 0)`) with `PR_SET_PDEATHSIG(libc::SIGTERM)` and terminate via ordered `killpg` escalation (`SIGINT` -> `SIGTERM` -> `SIGKILL`).
- Isolate test fixtures with `PromptHistory::open_in_memory()` and `SystemClipboard::new_isolated()`.

## Documentation & Knowledge Index
- Pre-commit verification or test/linter failures: follow `.agents/skills/frontal-quality-gate/SKILL.md`.
- Commit message formatting, trailer validation, card lifecycle, SemVer bumps, release tagging, or Project Board #8 transitions: follow `.agents/skills/frontal-card-lifecycle/SKILL.md`.
- Auditing or committing `.wiki/` documentation mirror updates: follow `.agents/skills/frontal-wiki-sync/SKILL.md`.
- Multi-harness engine lifecycle, `AgyEngine`/`OpenCodeEngine` contracts, or `/model` and `/quota` modals: read `.agents/docs/multi_harness_architecture.md` before modifying `src/core/protocol/` or `src/infra/engine/`.
- Module layout across `src/core/`, `src/infra/`, `src/ui/`, and `tests/`: read `.agents/docs/structure.md` when locating subsystem boundaries.
- Subagent delegation criteria and context-asymmetry rules: read `.agents/docs/subagents.md` before invoking subagents.
- External GitHub Wiki pages (`Home`, `Frontal-Architecture`) and Project Board #8 URLs: read `.agents/docs/sources.md`.
