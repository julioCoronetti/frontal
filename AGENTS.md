## Guardrails
- Never add crate deps to `Cargo.toml` (package version `0.20.0`) without explicit authorization.
- Keep `Cargo.toml` version `0.20.0` immutable during `In Progress`; bump patch (`0.20.x`) or minor (`0.x.0`) only in an authorized `Done` commit with annotated `vX.Y.Z` tag and identical CLI (`-V`)/TUI (`/help`) versions.
- Enforce <= 500 lines per `.rs` file and zero comments, `#[doc]`/`#![doc]`, `#[rustfmt::skip]`, or `include*!` macros in Rust code.
- Limit commits to 1 logical change (<= 5 files unless `ATOMIC_OVERRIDE=1`) and keep `.wiki/` clean before `In Review` or `Done`.
- Use `WaitMsBeforeAsync: 10000` on synchronous commands (no `manage_task` polling), read `.rs` files in full before editing, and batch edits before running the gate.

## Commands
- Snapshot (`--files`, `--board [id]`): `.agents/skills/frontal-context/scripts/bootstrap.sh`
- Atomic commit (`.git/FRONTAL_COMMIT_EDITMSG` or flags): `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh commit`
- Quality gate (pre-checks, `fmt`, `clippy -D warnings`, `tests/suite.rs`): `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh`
- Full suite (`tests/suite.rs`): `cargo test --quiet`
- Architecture (`tests/domains/architecture.rs`): `cargo test --test suite domains::architecture`
- Protocol & CLI (`tests/domains/protocol_and_cli.rs`): `cargo test --test suite domains::protocol_and_cli`
- Engine & session (`tests/domains/engine_and_session/`): `cargo test --test suite domains::engine_and_session`
- Cache & markdown (`tests/domains/cache_and_markdown/`): `cargo test --test suite domains::cache_and_markdown`
- Viewport & clipboard (`tests/domains/viewport_and_clipboard.rs`): `cargo test --test suite domains::viewport_and_clipboard`
- Editor & history (`tests/domains/editor_and_history/`): `cargo test --test suite domains::editor_and_history`
- Tabs & modals (`tests/domains/tabs_and_modals/`): `cargo test --test suite domains::tabs_and_modals`
- TUI E2E (`tests/domains/tui_integration/`): `cargo test --test suite domains::tui_integration`

## Invariants & Traps
- RenderCache L1 stores immutable `MessageBlock` AST; L2 caches width-wrapped lines by `(width, revision)` — invalidate only L2 on resize/toggle for 60 FPS streaming.
- Never run synchronous disk I/O, SQLite queries, or subprocess scans inside `tokio::select!` (`src/ui/mod.rs`).
- Access active `TabState` in `App` via O(1) index and `Deref`/`DerefMut` (no `std::mem::swap`); route stream events through `TabState::apply_stream_event`.
- Spawn child engines in dedicated POSIX process groups (`setpgid(0, 0)`, `PR_SET_PDEATHSIG(libc::SIGTERM)`) and terminate via `killpg` (`SIGINT` -> `SIGTERM` -> `SIGKILL`).
- Isolate test fixtures with `PromptHistory::open_in_memory()` and `SystemClipboard::new_isolated()`.

## Documentation & Knowledge Index
- Session snapshot & board lookup: `.agents/skills/frontal-context/SKILL.md`.
- Quality gate & failure remediation: `.agents/skills/frontal-quality-gate/SKILL.md`.
- Commit format, SemVer bump, tags, & Board #8: `.agents/skills/frontal-card-lifecycle/SKILL.md`.
- Wiki mirror audit & commit: `.agents/skills/frontal-wiki-sync/SKILL.md`.
- Multi-harness contracts (`AgyEngine`/`OpenCodeEngine`, `/model`, `/quota`): read `.agents/docs/multi_harness_architecture.md`.
- Module layout (`src/`, `tests/`): read `.agents/docs/structure.md`.
- Subagent delegation rules: read `.agents/docs/subagents.md`.
- GitHub Wiki & Project Board #8 URLs: read `.agents/docs/sources.md`.
