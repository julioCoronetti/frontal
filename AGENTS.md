## Guardrails
- Never add crate deps to `Cargo.toml` (package version `0.21.0`) without explicit authorization.
- Keep `Cargo.toml` version `0.21.0` immutable during `In Progress`; bump patch (`0.21.x`) or minor (`0.x.0`) only in an authorized `Done` commit with annotated `vX.Y.Z` tag and identical CLI (`-V`) and TUI (`/help`) versions.
- Keep commits to 1 logical change (<= 5 files unless `ATOMIC_OVERRIDE=1`) and commit `.wiki/` updates before `In Review` or `Done`.
- Delegate multi-file `.rs` exploration (`>= 8` reads) to `invoke_subagent(research)` or `run_workflow` before reading source files in the primary context.

## Commands
- Snapshot (`--files`, `--board [id]`): `.agents/skills/frontal-context/scripts/bootstrap.sh`
- Atomic commit (`.git/FRONTAL_COMMIT_EDITMSG` or flags): `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh commit`
- Quality gate (`--fast`, `--domain <name>`, `--full`): `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh`
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
- Session refresh, file line counts, or Project Board #8 lookup: follow `.agents/skills/frontal-context/SKILL.md`.
- Mid-edit checks, full gate, or parallel codebase audits: follow `.agents/skills/frontal-quality-gate/SKILL.md`.
- Commits, SemVer bumps, release tags, or closing Board #8 cards: follow `.agents/skills/frontal-card-lifecycle/SKILL.md`.
- Editing `.wiki/` pages or validating mirror parity before `In Review` or `Done`: follow `.agents/skills/frontal-wiki-sync/SKILL.md`.
- Multi-harness engine, `/model`, or `/quota` changes: read `.agents/docs/multi_harness_architecture.md` before modifying engine or modal code.
- Locating unfamiliar modules across `src/` or `tests/`: read `.agents/docs/structure.md` first.
- Subagent delegation boundaries and parallel audit workflow usage: read `.agents/docs/subagents.md` first.
- GitHub Wiki or Project Board #8 references: read `.agents/docs/sources.md`.
