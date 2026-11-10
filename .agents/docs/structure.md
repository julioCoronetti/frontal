# Frontal Repository Structure

Reference documentation for on-demand inspection of repository modules.

## Core & IPC (`src/`)
- `src/main.rs`: Application entry point.
- `src/lib.rs`: Public library module declarations (`cli`, `config`, `engine`, `protocol`, `session`, `transport`, `ui`).
- `src/config.rs`: Persistent user configuration (`~/.frontal/config.toml`, model, mode, and default effort).
- `src/cli.rs`: Command-line argument parser (`CliArgs`), headless orchestration, and TUI bootstrap.
- `src/protocol.rs`: Strict typing and NDJSON deserialization for the `stream-json` protocol (`EngineEvent`, `StreamEvent`, `ToolInfo`, etc.).
- `src/transport.rs`: `EngineTransport` implementations (`SubprocessEngine` with 16 MB `LinesCodec` buffer and `FixtureReplayEngine`).
- `src/engine.rs`: High-level agentic engine abstraction and streaming channels.
- `src/session.rs`: Session multiplexing and management via `SessionId` and `SessionManager`.
- `src/security.rs`: Deterministic containment security guard (`DangerAssessment`, `RiskCategory`) for inspecting destructive tool calls.

## Terminal Interface (`src/ui/`)
- `src/ui/mod.rs`: Asynchronous 60 FPS TUI event loop (crossterm `EventStream`, cancellation, render ticks).
- `src/ui/app/`: Central application module partitioned into modular files (500-line ceiling):
  - `src/ui/app/mod.rs`: `App` struct, `AppAction` enum, constructors (`new`, `new_isolated`), and active metadata getters.
  - `src/ui/app/tabs.rs`: Multi-session management and swap-cycle visual isolation (`std::mem::swap`).
  - `src/ui/app/events.rs`: Non-blocking terminal event routing (keyboard, mouse, paste, autocompletion).
  - `src/ui/app/commands.rs`: Local command dispatch (`/quit`, `/clear`, `/interrupt`, etc.), modal actions, and git diff extraction.
  - `src/ui/app/stream.rs`: Asynchronous IPC engine envelope handling (`handle_engine_envelope`).
  - `src/ui/app/render.rs`: 60 FPS drawing methods, layout calculation, and TUI widgets.
- `src/ui/tab.rs`: Tab state management (`TabState`), memory swap, and background processing.
- `src/ui/cache/`: Partitioned 2-level render cache (`mod.rs`, `format.rs`, `slice.rs`).
- `src/ui/viewport.rs`: Conversation viewport with state machine for sticky scroll and clean text selection.
- `src/ui/markdown/`: Partitioned incremental `pulldown-cmark` parser (`mod.rs`, `normalize.rs`, `parse.rs`, `render.rs`).
- `src/ui/highlight.rs`: Incremental line-by-line syntax highlighting via `syntect` with state checkpoints.
- `src/ui/editor/`: Multi-line prompt editor backed by B-Tree (`ropey`), atomic extmarks (`@`, `/`), and paste pills (`mod.rs`, `mutations.rs`, `navigation.rs`).
- `src/ui/autocomplete.rs`: Floating popover autocompletion for commands and files (`FileSearchWorker`).
- `src/ui/history.rs`: Command history persistence in SQLite WAL (`PromptHistory`).
- `src/ui/modal/`: Partitioned floating modal stack (`mod.rs`, `danger.rs`, `diff.rs`, `model_effort.rs`, `palette.rs`, `respawn.rs`, `select.rs`, `usage/`).
- `src/ui/diff.rs`: Integrated unified diff viewer.
- `src/ui/clipboard.rs`: System clipboard integration via `arboard` with isolated in-memory buffer and OSC 52 fallback.
- `src/ui/notify.rs`: Terminal sensory notifications (audio bell and visual flash).
- `src/ui/terminal.rs`: Raw mode bootstrap, bracketed paste, and TTY recovery panic hook.
- `src/ui/theme.rs`: Visual theme definitions and color palette inspired by OpenCode.

## Integration Test Suite (`tests/`)
- `tests/suite.rs`: Canonical entry point for the single integration test crate.
- `tests/common/mod.rs`: Hermetic test utilities (`test_app`, `press_key`, RAII guards `TempDirGuard`/`TempFileGuard`, `PGID_TEST_LOCK`).
- `tests/domains/`: Directory housing the 8 partitioned integration test domains.
