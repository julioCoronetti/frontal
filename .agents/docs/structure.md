# Frontal Repository Structure

Reference documentation for on-demand inspection of repository modules.

## Core Domain (`src/core/`)
- `src/core/mod.rs`: Domain module declarations and public exports.
- `src/core/media.rs`: Media URI/path normalization, shell-escape unescaping, and image MIME detection (`extract_pasted_image_path`, `image_mime_for_path`).
- `src/core/protocol/`: Strict typing and NDJSON/ACP deserialization (`EngineEvent`, `StreamEvent`, `ToolInfo`, `PromptPayload`, `MediaItem`) partitioned into `mod.rs`, `events.rs`, `agy.rs`, `opencode.rs`, and `acp.rs`.
- `src/core/security/`: Deterministic containment security guard (`DangerAssessment`, `RiskCategory`) partitioned into `mod.rs`, `shell.rs`, and `sql.rs`.
- `src/core/model_catalog.rs`: Runtime catalog mapping for discovered OpenCode/Antigravity models and providers.
- `src/core/model_spec.rs`: Context window, semaphore threshold, compact token formatting, and model specification resolution (`ModelSpec`, `ContextSemaphore`).
- `src/core/config.rs`: Persistent user configuration (`~/.frontal/config.toml`, model, mode, and default effort).

## Infrastructure & IPC (`src/infra/`)
- `src/infra/mod.rs`: Infrastructure module declarations.
- `src/infra/engine/`: High-level agentic engines (`AcpEngine` over `agy acp --stdio`, and `OpenCodeEngine` per-turn supervisor) partitioned into `mod.rs`, `opencode.rs`, `acp/{mod.rs,coordinator.rs,handshake.rs,setup.rs}`, and `process.rs`, with shared POSIX pgid registry and streaming channels.
- `src/infra/transport/`: `EngineTransport` implementations (`EngineConfig`, `EngineError`, `OpenCode` helpers, and `FixtureReplayEngine`) partitioned into `mod.rs`, `opencode.rs`, and `replay.rs`.
- `src/infra/session/`: Session multiplexing and management (`mod.rs`, `config.rs`, `lifecycle.rs`) via `SessionId`, `SessionConfig`, and `SessionManager`.
- `src/infra/clipboard/`: System clipboard integration via `arboard`, zero-dep PNG encoding (`image_codec.rs`), platform media fallbacks (`platform.rs`), isolated in-memory buffer, and OSC 52 fallback.
- `src/infra/history/`: Command history and unified session persistence in SQLite WAL (`PromptHistory`) with dedicated background worker (`worker.rs`), partitioned session types & operations (`session_types.rs`, `sessions.rs`), local sync & external Agy/OpenCode discovery (`sync.rs`, `external.rs`), RAII `Drop` flush, non-blocking external `conversation_summaries.db` cache (`agent.rs`), JSONL transcript block & telemetry parser (`transcript.rs`), OpenCode SQLite discovery/hydration/message reader (`opencode/{mod.rs,messages.rs}`), and per-conversation message/telemetry storage (`messages.rs`, `telemetry.rs`) for cross-engine resume.

## Terminal Interface (`src/ui/`)
- `src/ui/mod.rs`: Asynchronous 60 FPS TUI event loop (crossterm `EventStream`, cancellation, render ticks).
- `src/ui/app/`: Central application module partitioned into modular submodules (500-line ceiling):
  - `src/ui/app/mod.rs`: `App` struct, `AppAction` enum, `apply_sync_action`, and constructors.
  - `src/ui/app/tabs.rs`: Multi-session tab CRUD/reset with O(1) tab switching and `TabState` single source of truth.
  - `src/ui/app/respawn.rs`: Active session respawn, history hydration bridge, and backend/model compatibility resolution.
  - `src/ui/app/events/`: Non-blocking terminal event routing partitioned into `mod.rs`, `keyboard.rs`, and `mouse.rs`.
  - `src/ui/app/commands/`: Command dispatch, modal actions, and session resume partitioned into `mod.rs`, `modals.rs`, and `resume.rs`.
  - `src/ui/app/stream.rs`: Streamlined asynchronous IPC engine envelope dispatch and per-turn conversation persistence.
  - `src/ui/app/render/`: 60 FPS drawing methods, layout calculation, and TUI widgets partitioned into `mod.rs`, `chrome.rs`, `input.rs`, `panes.rs`, and `welcome.rs`.
- `src/ui/tab.rs`: Tab state management (`TabState`) and unified stream event handling (`apply_stream_event`).
- `src/ui/history.rs`: Visual cache hydration and telemetry/model resolution bridge (`hydrate_session_blocking`, `resolve_session_telemetry`, `hydrate_conversation`) with Agy transcript, OpenCode SQLite, and Frontal store persistence.
- `src/ui/cache/`: Partitioned 2-level render cache (`mod.rs`, `format.rs`, `slice.rs`, `user_format.rs`).
- `src/ui/viewport.rs`: Conversation viewport with state machine for sticky scroll and clean text selection.
- `src/ui/markdown/`: Partitioned incremental `pulldown-cmark` parser (`mod.rs`, `normalize.rs`, `parse.rs`, `render.rs`, `wrap.rs`).
- `src/ui/highlight.rs`: Incremental line-by-line syntax highlighting via `syntect` with state checkpoints.
- `src/ui/editor/`: Multi-line prompt editor backed by B-Tree (`ropey`), atomic extmarks (`@`, `/`), and paste pills (`mod.rs`, `mutations.rs`, `navigation.rs`).
- `src/ui/autocomplete.rs`: Floating popover autocompletion for commands and files (`FileSearchWorker`).
- `src/ui/modal/`: Partitioned floating modal stack (`mod.rs`, `commands.rs`, `danger.rs`, `model_effort/{mod.rs,render.rs}`, `select/{mod.rs,render.rs}`, `usage/{mod.rs,cli.rs,fetch.rs,input.rs,opencode_api.rs,quota_parse.rs,render.rs,stats.rs,stats_render.rs,types.rs}`).
- `src/ui/notify.rs`: Terminal sensory notifications (audio bell and visual flash).
- `src/ui/terminal.rs`: Raw mode bootstrap, bracketed paste, and TTY recovery panic hook.
- `src/ui/theme.rs`: Visual theme definitions and color palette inspired by OpenCode.
- `src/ui/labels.rs`: Display-name formatting for providers and models (slug to human label), keeping engine identifiers untouched.

## Entrypoints & Facade (`src/`)
- `src/main.rs`: Application entry point (`tokio::main`).
- `src/lib.rs`: Public library module declarations and backward-compatible re-exports.
- `src/cli.rs`: Command-line argument parser (`CliArgs`), headless orchestration, and TUI bootstrap.

## Integration Test Suite (`tests/`)
- `tests/suite.rs`: Canonical entry point for the single integration test crate.
- `tests/common/mod.rs`: Hermetic test utilities (`test_app`, `press_key`, RAII guards `TempDirGuard`/`TempFileGuard`, `PGID_TEST_LOCK`).
- `tests/domains/`: Partitioned 8 integration test domains (500-line ceiling per file):
  - `tests/domains/architecture/`: Repository architecture & module boundary invariants (`mod.rs`, `invariants.rs`, `lexer.rs`, `boundaries.rs`).
  - `tests/domains/protocol_and_cli.rs`: NDJSON/ACP protocol parsing, CLI flags, and model catalog decoding.
  - `tests/domains/viewport_and_clipboard.rs`: Viewport sticky scroll, text selection, and clipboard integration.
  - `tests/domains/cache_and_markdown/`: `mod.rs`, `cache_l1_l2.rs`, `markdown_ast.rs`, `table_wrap.rs`, `tool_clamping.rs`.
  - `tests/domains/editor_and_history/`: `mod.rs`, `agent_async.rs`, `core.rs`, `media.rs`, `opencode_sync.rs`.
  - `tests/domains/engine_and_session/`: `mod.rs`, `acp.rs`, `lifecycle.rs`, `media.rs`, `model_identity.rs`, `opencode.rs`, `turn_meta.rs`.
  - `tests/domains/tabs_and_modals/`: `mod.rs`, `history_resume.rs`, `modals.rs`, `model_effort.rs`, `resume.rs`, `resume_delete.rs`, `select.rs`, `tab_bar_mouse.rs`, `tabs.rs`, `unified_resume.rs`, `usage.rs`, `usage_stats.rs`, `usage_telemetry.rs`.
  - `tests/domains/tui_integration/`: `mod.rs`, `context_gauge.rs`, `input_and_events.rs`, `interrupt.rs`, `media.rs`, `rendering.rs`, `tab_bar_rendering.rs`, `welcome.rs`.
