# Frontal Repository Structure

Reference documentation for on-demand inspection of repository modules.

## Core Domain (`src/core/`)
- `src/core/mod.rs`: Domain module declarations and public exports.
- `src/core/protocol/`: Strict typing and NDJSON deserialization for the `stream-json` protocol (`EngineEvent`, `StreamEvent`, `ToolInfo`, etc.) partitioned into `mod.rs` (canonical types), `agy.rs` (`AgyStreamParser`), and `opencode.rs` (`OpenCodeStreamParser`).
- `src/core/security/`: Deterministic containment security guard (`DangerAssessment`, `RiskCategory`) partitioned into `mod.rs`, `shell.rs`, and `sql.rs`.
- `src/core/config.rs`: Persistent user configuration (`~/.frontal/config.toml`, model, mode, and default effort).

## Infrastructure & IPC (`src/infra/`)
- `src/infra/mod.rs`: Infrastructure module declarations.
- `src/infra/engine/`: High-level agentic engines (`AgyEngine` persistent pipe daemon and `OpenCodeEngine` per-turn supervisor) partitioned into `mod.rs` and `opencode.rs`, with shared POSIX pgid registry and streaming channels.
- `src/infra/transport/`: `EngineTransport` implementations (`AgyEngine` config with 16 MB `LinesCodec` buffer and `FixtureReplayEngine`) partitioned into `mod.rs` and `opencode.rs`.
- `src/infra/session.rs`: Session multiplexing and management via `SessionId` and `SessionManager`.
- `src/infra/clipboard.rs`: System clipboard integration via `arboard` with isolated in-memory buffer and OSC 52 fallback.
- `src/infra/history/`: Command history persistence in SQLite WAL (`PromptHistory`) with RAII `Drop` flush, JSONL transcript parser (`TranscriptBlock`), and per-conversation message storage (`messages.rs`) for cross-engine resume.

## Terminal Interface (`src/ui/`)
- `src/ui/mod.rs`: Asynchronous 60 FPS TUI event loop (crossterm `EventStream`, cancellation, render ticks).
- `src/ui/app/`: Central application module partitioned into modular submodules (500-line ceiling):
  - `src/ui/app/mod.rs`: `App` struct with `Deref`/`DerefMut` to active `TabState`, `AppAction` enum, and constructors.
  - `src/ui/app/tabs.rs`: Multi-session management with O(1) tab switching and `TabState` single source of truth.
  - `src/ui/app/events/`: Non-blocking terminal event routing partitioned into `mod.rs`, `keyboard.rs`, and `mouse.rs`.
  - `src/ui/app/commands/`: Command dispatch, modal actions, session resume, and git diff partitioned into `mod.rs`, `modals.rs`, `resume.rs`, and `git_diff.rs`.
  - `src/ui/app/stream.rs`: Streamlined asynchronous IPC engine envelope dispatch and per-turn conversation persistence.
  - `src/ui/app/render/`: 60 FPS drawing methods, layout calculation, and TUI widgets partitioned into `mod.rs`, `chrome.rs`, and `panes.rs`.
- `src/ui/tab.rs`: Tab state management (`TabState`) and unified stream event handling (`apply_stream_event`).
- `src/ui/history.rs`: Visual cache hydration bridge (`hydrate_cache_from_reader`, `hydrate_conversation`) with Agy transcript preference and Frontal store fallback.
- `src/ui/cache/`: Partitioned 2-level render cache (`mod.rs`, `format.rs`, `slice.rs`).
- `src/ui/viewport.rs`: Conversation viewport with state machine for sticky scroll and clean text selection.
- `src/ui/markdown/`: Partitioned incremental `pulldown-cmark` parser (`mod.rs`, `normalize.rs`, `parse.rs`, `render.rs`).
- `src/ui/highlight.rs`: Incremental line-by-line syntax highlighting via `syntect` with state checkpoints.
- `src/ui/editor/`: Multi-line prompt editor backed by B-Tree (`ropey`), atomic extmarks (`@`, `/`), and paste pills (`mod.rs`, `mutations.rs`, `navigation.rs`).
- `src/ui/autocomplete.rs`: Floating popover autocompletion for commands and files (`FileSearchWorker`).
- `src/ui/modal/`: Partitioned floating modal stack (`mod.rs`, `danger.rs`, `diff.rs`, `model_effort/`, `palette.rs`, `respawn.rs`, `select.rs`, `usage/`).
- `src/ui/diff.rs`: Integrated unified diff viewer.
- `src/ui/notify.rs`: Terminal sensory notifications (audio bell and visual flash).
- `src/ui/terminal.rs`: Raw mode bootstrap, bracketed paste, and TTY recovery panic hook.
- `src/ui/theme.rs`: Visual theme definitions and color palette inspired by OpenCode.

## Entrypoints & Facade (`src/`)
- `src/main.rs`: Application entry point (`tokio::main`).
- `src/lib.rs`: Public library module declarations and backward-compatible re-exports.
- `src/cli.rs`: Command-line argument parser (`CliArgs`), headless orchestration, and TUI bootstrap.

## Integration Test Suite (`tests/`)
- `tests/suite.rs`: Canonical entry point for the single integration test crate.
- `tests/common/mod.rs`: Hermetic test utilities (`test_app`, `press_key`, RAII guards `TempDirGuard`/`TempFileGuard`, `PGID_TEST_LOCK`).
- `tests/domains/`: Directory housing the 8 partitioned integration test domains.
