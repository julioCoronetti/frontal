## Project Context
- Project: Frontal — asynchronous, decoupled Terminal User Interface (TUI) harness in Rust for Antigravity CLI (`agy`) and `stream-json` compatible engines.
- Status: Production architecture at version `0.11.0`, verified by 193 tests across 8 hermetic domains on branch `main`, evolving via GitHub Project Board #8.
- Stack: `tokio`, `tokio-util`, `bytes`, `futures-util`, `serde`, `serde_json`, `libc`, `ratatui` (0.29), `crossterm` (0.28), `pulldown-cmark` (0.12), `syntect` (5.2), `ropey` (1.6), `rusqlite` (0.32), `arboard` (3.4).
- Branches: `main` (active) and `origin/legacy` (Python/Textual prototype).
- Do not add external dependencies to this single-crate repository without explicit operator authorization.

## Engineering Standards
- Never write comments in source files; place all documentation in git commit messages, GitHub Wiki, or `.agents/docs/`.
- Enforce a strict ceiling of 500 lines per `.rs` file; partition larger files into cohesive submodules with public re-exports.
- Pass `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` with zero warnings, and all 193 tests cleanly.

## Architectural Invariants
- Store immutable AST (`MessageBlock` with styled spans) in Level 1 of the 2-level render cache and formatted visual lines indexed by `(width, revision)` in Level 2; invalidate only Level 2 on resizes or block toggles and emit streaming deltas at 60 FPS without re-parsing history.
- Never execute synchronous disk I/O, persistence, or process scans inside the async `tokio::select!` event loop in `src/ui/mod.rs`.
- Encapsulate every session's state in `TabState`, reference the active tab in `App` via O(1) indexing and a `Deref`/`DerefMut` bridge without memory swaps, and process all stream events uniformly via `TabState::apply_stream_event`.
- Spawn every subprocess (`agy`) in a dedicated process group (`setpgid(0, 0)`) with `PR_SET_PDEATHSIG(libc::SIGTERM)`, and terminate via ordered `killpg` escalation (`SIGINT` -> `SIGTERM` -> `SIGKILL`).
- Isolate test side effects using `PromptHistory::open_in_memory()` for database fixtures and `SystemClipboard::new_isolated()` for clipboard tests.

## Integration Test Suite
- `tests/suite.rs`: Canonical single-crate test harness validating all 8 architectural domains.
- `tests/domains/architecture.rs`: Architectural mechanical invariants (4 tests).
- `tests/domains/protocol_and_cli.rs`: Protocol `stream-json`, NDJSON, and CLI (21 tests).
- `tests/domains/engine_and_session/`: Subprocess `agy`, fixtures, POSIX isolation, and `OpenCodeEngine` turns (25 tests).
- `tests/domains/cache_and_markdown.rs`: RenderCache L1/L2 and Markdown (16 tests).
- `tests/domains/viewport_and_clipboard.rs`: Sticky scroll, clean selection, and clipboard (16 tests).
- `tests/domains/editor_and_history.rs`: Editor `ropey` and SQLite WAL history (16 tests).
- `tests/domains/tabs_and_modals/`: Multi-session and modal dialogs (75 tests).
- `tests/domains/tui_integration/`: End-to-end TUI integration (20 tests).

## On-Demand Resources
- `.agents/skills/frontal-quality-gate`: Runs the canonical Cargo triad (`fmt`, `clippy`, and 193 tests) and verifies architectural invariants.
- `.agents/skills/frontal-card-lifecycle`: Manages card closure workflows, SemVer bumping, release tags, and Project Board #8 transitions.
- `.agents/skills/frontal-wiki-sync`: Audits invariant tokens, prevents extinct legacy terms, and commits to the `.wiki/` mirror.
- `.agents/docs/multi_harness_architecture.md`: Multi-harness architecture specification and implementation plan (`Agy` and `OpenCode`).
- `.agents/docs/structure.md`: Comprehensive module layout for core and UI crates.
- `.agents/docs/sources.md`: GitHub, Wiki, and technical dossier references (`agy`, `OpenCode`).
- `.agents/docs/subagents.md`: Guidelines, profiles, and anti-patterns for subagent delegation.

## Quality Gate
- Run `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh` before every commit to enforce `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --quiet` (193 tests passing).
- Limit each commit to 1 indivisible logical change touching at most 5 modified files (unless `ATOMIC_OVERRIDE=1`).

## Commit Governance
- Validate commit messages and trailers via `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh validate-commit`.
- Reject `Card-Status: In Review` when `Reviewed-By: Argon-Advisor <mode:none>` is used; prior Argon Advisor audit is mandatory.
- Ensure the `.wiki/` documentation mirror is clean before committing `Card-Status: In Review` or `Done`.
- Allowed types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- Allowed scopes: `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.
- Format every commit message strictly as:
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <technical justification or root cause>
- What: <concise summary of structural or behavioral changes>
- Invariants: <preserved invariants: L1/L2 cache, O(1) TabState Deref, POSIX pgid, etc.>

Refs: #<card_or_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive|refine>
```

## SemVer Policy
- Maintain SemVer `0.y.z`; version `0.11.0` represents production consolidation.
- Never modify `version` in `Cargo.toml` during intermediate commits (`Card-Status: In Progress`).
- Bump `version` exclusively in the closing commit (`Card-Status: Done`) after explicit authorization from the Senhor: patch (`0.11.x`) for `fix(...)` or pure `refactor(...)`, minor (`0.x.0`) for structural feature cards.
- Tag every closing commit immediately with an annotated Git tag (`git tag -a vX.Y.Z -m "Release vX.Y.Z"`).
- Keep CLI (`frontal --version` / `frontal -V`) and TUI (`/help`) versions identical.

## Card Workflow
- On card kickoff (`Todo` -> `In Progress`), run `~/.gemini/config/scripts/advisor.sh -m design -f <file>... "<objective and context>"`.
- Implement in minimal test-driven slices, running `.agents/skills/frontal-quality-gate/scripts/quality_gate.sh` and committing each slice with `Card-Status: In Progress`.
- On completion of all slices, run adversarial diff review via `~/.gemini/config/scripts/advisor.sh -m review -d -f <file>... "<objective and context>"` before committing with `Card-Status: In Review`.
- After explicit operator authorization, run `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh` to bump SemVer, commit `Card-Status: Done`, create tag `vX.Y.Z`, and transition the Project Board #8 card to `Done`.
