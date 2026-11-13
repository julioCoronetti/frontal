## Project Overview & Core Constraints
- Project: Frontal.
- Purpose: High-performance, asynchronous, decoupled Terminal User Interface (TUI) harness for Antigravity CLI (`agy`) and `stream-json` compatible engines.
- Status: Production architecture in Rust (version `0.10.0`), verified by 182 tests across 8 hermetic domains on branch `main`. Continuous flow driven by GitHub Project Board #8.
- Stack: `tokio`, `tokio-util`, `bytes`, `futures-util`, `serde`, `serde_json`, `libc`, `ratatui` (0.29), `crossterm` (0.28), `pulldown-cmark` (0.12), `syntect` (5.2), `ropey` (1.6), `rusqlite` (0.32), `arboard` (3.4). Single-crate repository; no external dependencies allowed without explicit operator authorization.
- Branches: `main` (active) and `origin/legacy` (Python/Textual prototype).

---

## Engineering Standards & Architectural Invariants

### Mechanical Rules
- Zero comments in code: Strictly forbidden across all source files. Documentation belongs in git commit messages, GitHub Wiki, or `.agents/docs/`.
- File modularity ceiling: Strict limit of 500 lines per `.rs` file. Files exceeding this ceiling must be partitioned into cohesive submodules with public re-exports.
- Quality enforcement: Strict compilation without warnings (`cargo clippy --all-targets -- -D warnings`), canonical formatting (`cargo fmt --check`), and 182 tests clean pass.

### Architectural Invariants
1. 2-Level Render Cache ($O(1)$ during active turns): Level 1 stores immutable AST (`MessageBlock` with styled spans). Level 2 caches formatted visual lines indexed by `(width, revision)`. Resizes or block toggles invalidate Level 2 only. Active streaming emits deltas at 60 FPS without re-parsing history.
2. Non-blocking TUI Event Loop (60 FPS): Async `tokio::select!` in `src/ui/mod.rs` must never execute synchronous disk I/O, persistence, or process scans on the render thread.
3. Multi-Session Single Source of Truth (`TabState` with O(1) Switching): Every session encapsulates its own state in `TabState`. `App` references the active tab via O(1) indexing and a `Deref`/`DerefMut` bridge without memory swaps. Stream events are processed uniformly via `TabState::apply_stream_event`.
4. POSIX Process Isolation: Subprocesses (`agy`) must run in a dedicated process group (`setpgid(0, 0)`) with `PR_SET_PDEATHSIG(libc::SIGTERM)`. Teardown must issue ordered `killpg` escalation (`SIGINT` -> `SIGTERM` -> `SIGKILL`).
5. Side-Effect Isolation in Tests: Use `PromptHistory::open_in_memory()` for database fixtures and `SystemClipboard::new_isolated()` for clipboard tests.

---

## Unified Integration Test Suite (tests/)
Canonical test harness in `tests/suite.rs` validates all 8 architectural domains:
- `tests/domains/architecture.rs`: Architectural mechanical invariants (4 tests).
- `tests/domains/protocol_and_cli.rs`: Protocol `stream-json`, NDJSON, and CLI (21 tests).
- `tests/domains/engine_and_session/`: Subprocess `agy`, fixtures, POSIX isolation, and `OpenCodeEngine` per-turn turns (15 tests).
- `tests/domains/cache_and_markdown.rs`: RenderCache L1/L2 and Markdown (16 tests).
- `tests/domains/viewport_and_clipboard.rs`: Sticky scroll, clean selection, and clipboard (16 tests).
- `tests/domains/editor_and_history.rs`: Editor `ropey` and SQLite WAL history (16 tests).
- `tests/domains/tabs_and_modals/`: Multi-session and modal dialogs (74 tests).
- `tests/domains/tui_integration/`: End-to-end TUI integration (20 tests).

Detailed module mapping available on demand in `.agents/docs/structure.md`.

---

## On-Demand Resources & Skills (.agents/)
Operational procedures and reference documentation are decoupled from the base prompt:
- Skills (`.agents/skills/`):
  - `frontal-quality-gate`: Runs the canonical Cargo triad (`fmt`, `clippy`, and 182 tests) and verifies architectural invariants.
  - `frontal-card-lifecycle`: Manages card closure workflows, SemVer bumping, release tags, and Project Board #8 transitions.
  - `frontal-wiki-sync`: Audits invariant tokens, prevents extinct legacy terms, and commits to the `.wiki/` mirror.
- Documentation (`.agents/docs/`):
  - `structure.md`: Comprehensive module layout for core and UI crates.
  - `sources.md`: GitHub, Wiki, and technical dossier references (`agy`, `OpenCode`).
  - `subagents.md`: Guidelines, profiles, and anti-patterns for subagent delegation.

---

## Git Delivery Flow & Atomic Commits

### Canonical Quality Gate
Every change must pass the automated gate (`.agents/skills/frontal-quality-gate/scripts/quality_gate.sh`):
1. Atomic threshold: Maximum 5 modified files (unless `ATOMIC_OVERRIDE=1`).
2. Canonical formatting: `cargo fmt --check`.
3. Strict linting: `cargo clippy --all-targets -- -D warnings` (zero warnings).
4. Test suite pass: `cargo test --quiet` (unified integration suite passing 182 tests).

### Commit Message Governance
Commit messages and trailers are validated via `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh validate-commit`:
- Must match canonical header regex and include mandatory sections (`- Why:`, `- What:`, `- Invariants:`).
- `Card-Status: In Review` strictly requires prior Argon Advisor audit (rejects `Reviewed-By: Argon-Advisor <mode:none>`).
- `Card-Status: In Review` and `Done` require a clean `.wiki/` documentation mirror.

### SemVer Policy
- Version Series `0.y.z`: Maturing software following SemVer. Version `0.10.0` represents production consolidation.
- Golden Rule: Never modify `version` in `Cargo.toml` during intermediate commits (`Card-Status: In Progress`).
- Done Commit Bump: Version bump occurs exclusively in the closing commit (`Card-Status: Done`) authorized by the Senhor:
  - Patch (`0.10.x`): Bug fixes (`fix(...)`) or pure refactorings (`refactor(...)`).
  - Minor (`0.x.0`): Structural features closing Project Board cards.
  - Every closing commit requires an annotated Git tag (`git tag -a vX.Y.Z -m "Release vX.Y.Z"`).
- Symmetry: CLI (`frontal --version` / `frontal -V`) and TUI (`/help`) versions must match identically.

### Atomic Commit Discipline
- 1 Commit = 1 Indivisible Change: Maximum 3 to 5 modified files per commit (unless `ATOMIC_OVERRIDE=1`).
- Commit Template:
```text
<type>(<scope>): <imperative summary in english, max 72 chars>

- Why: <technical justification or root cause>
- What: <concise summary of structural or behavioral changes>
- Invariants: <preserved invariants: L1/L2 cache, swap App/TabState, POSIX pgid, etc.>

Refs: #<card_or_issue>
Card-Status: <In Progress | In Review | Done>
Reviewed-By: Argon-Advisor <mode:none|review|design|troubleshoot|deep-dive|refine>
```
- Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `perf`.
- Scopes: `config`, `protocol`, `transport`, `engine`, `session`, `cli`, `ui/app`, `ui/cache`, `ui/editor`, `ui/markdown`, `ui/modal`, `ui/tab`, `ui/viewport`, `tests`, `agents`, `wiki`.

---

## Card Workflow & Advisor Governance
1. Stage 1 (Kickoff): Invoke Argon Advisor (`advisor.sh --mode design -f <files> "..."`) when moving card to `In Progress`.
2. Stage 2 (Atomic TDD): Implement via minimal test-driven slices (`quality_gate.sh` -> Commit with `Card-Status: In Progress`).
3. Stage 3 (Adversarial Review): Run diff audit (`advisor.sh --mode review`). Favorable verdict unlocks `Card-Status: In Review`.
4. Stage 4 (Card Closure & Release): With operator authorization, run `.agents/skills/frontal-card-lifecycle/scripts/card_lifecycle.sh` to bump SemVer, commit `Card-Status: Done`, tag `vX.Y.Z`, and transition Project Board #8 card to `Done`.
