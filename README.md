# Frontal

Decoupled, asynchronous, high-performance Terminal User Interface (TUI) harness developed in **Rust** (version `0.20.0`) for the Antigravity CLI (`agy`) and agentic engines compatible with the `stream-json` protocol.

---

## 1. Overview

Frontal provides a high-performance terminal presentation layer designed for the stable, responsive rendering of continuous LLM token streams. The harness consumes structured events (NDJSON) over process pipes (`stdin`/`stdout`), fully decoupling the AI engine lifecycle from the visual interface. Architectural stability is verified by a unified suite of **276 tests across 8 domains**.

The design is informed by comparative analysis of two reference harnesses:
- **Antigravity CLI (`agy`)**: Benchmark for operational simplicity, process stability, and a streamlined `stream-json` protocol.
- **OpenCode**: Benchmark for terminal ergonomics, message chunking, tool cards, and context management.

---

## 2. Official Technology Stack (Rust)

Frontal's architecture delivers deterministic 60 FPS execution with minimal resource utilization and zero external runtime dependencies:

| Layer | Crate / Library | Role and Technical Justification |
| :--- | :--- | :--- |
| **Presentation & TUI** | `ratatui` + `crossterm` | Immediate-mode rendering with double-buffering and microsecond cell diffing. |
| **Async Runtime & IPC** | `tokio` + `tokio-util` | Asynchronous subprocess orchestration (`tokio::process`), MPSC channels, and NDJSON decoding via `LinesCodec`. |
| **Serialization** | `serde` + `serde_json` | Strict, typed deserialization of `stream-json` events/commands and JSON-RPC 2.0 ACP (`agy acp --stdio`) multimodal payloads. |
| **Prompt Editor** | `ropey` + Token Registry | B-Tree (Rope) buffer for fast multiline navigation, atomic mention and media extmarks (`@`, `/`, `[Image #N]`), and large paste collapse (`[Pasted ~N lines]`). |
| **Markdown Parsing** | `pulldown-cmark` | High-performance CommonMark event parser building an internal AST of modular blocks (`MessageBlock`). |
| **Syntax Highlighting** | `syntect` | Line-incremental highlighting with 150+ bundled grammars without native C compilation. |
| **Local Persistence** | `rusqlite` (WAL mode) | Embedded lightweight SQLite for session metadata, command history, and cache. |
| **System Clipboard** | `arboard` + OSC 52 | Native text and RGBA image capture (`Ctrl+V` / `Alt+V`) with zero-dependency PNG encoding in `~/.frontal/cache/media/` and OSC 52 escape fallback for remote SSH sessions. |
| **Process Isolation & POSIX** | `libc` | POSIX process groups (`setpgid` / `killpg`) and orphan prevention (`PR_SET_PDEATHSIG`). |
| **Buffers & Async Utils** | `bytes` + `futures-util` | Network buffer management and async stream combinators with minimal allocations. |

---

## 3. Architectural Invariants

1. **Two-Level Render Cache ($O(1)$ during active streaming)**:
   - Consolidated turn messages are preserved as immutable semantic ASTs (`MessageBlock` with style spans) in Level 1.
   - Level 2 maintains pre-formatted visual lines indexed by `(width, revision)`, lazily invalidated on terminal resizes (`SIGWINCH`) or block expand/collapse.
   - Only the active streaming block computes deltas, keeping frame generation at $O(1)$ and locking display refresh to 60 FPS regardless of conversation history size.
2. **Non-blocking TUI Event Loop (60 FPS)**:
   - Asynchronous `tokio::select!` in `src/ui/mod.rs` never executes synchronous disk I/O, persistence, or process scans on the render thread.
3. **Multi-Session Single Source of Truth (`TabState` with $O(1)$ Switching)**:
   - Every session encapsulates its own state in `TabState`. `App` references the active tab via $O(1)$ indexing and a `Deref`/`DerefMut` bridge without memory swaps (`std::mem::swap`). Stream events are processed uniformly via `TabState::apply_stream_event`.
4. **POSIX Process Isolation & Cleanup**:
   - Subprocesses are spawned in dedicated process groups via `setpgid(0, 0)` with `PR_SET_PDEATHSIG(libc::SIGTERM)`. Teardown issues ordered `killpg` escalation (`SIGINT` -> `SIGTERM` -> `SIGKILL`) preventing orphaned child processes.
5. **Deterministic Sticky Scroll & Large Paste Folding**:
   - Viewport remains locked to the bottom edge during token streaming, immediately unanchors on user scroll-up, and re-anchors upon returning to the footer. Clipboard insertions exceeding 5 lines collapse into interactive visual pills in the prompt.

---

## 4. Modular Repository Architecture

The codebase enforces strict decoupling into three cohesive architectural layers with a strict 500-line ceiling per `.rs` file and zero comments:

- **Core (`src/core/`)**: Domain logic and protocols (`protocol/` partitioned into `mod.rs`, `events.rs`, `agy.rs`, `opencode.rs`, and `acp.rs`), deterministic risk evaluation matrix (`security/` partitioned into `mod.rs`, `shell.rs`, and `sql.rs`), model specifications (`model_spec.rs`, `model_catalog.rs`), and persistent user configuration (`config.rs`).
- **Infra (`src/infra/`)**: Asynchronous IPC transport (`transport/` partitioned into `mod.rs` and `opencode.rs`), subprocess lifecycle and POSIX groups (`engine/` partitioned into `mod.rs`, `agy.rs`, `opencode.rs`, `acp/`, and `process.rs`), session multiplexing (`session.rs`), text and PNG image clipboard driver (`clipboard/`), and SQLite WAL history persistence with RAII flush (`history/`).
- **UI (`src/ui/`)**: 60 FPS non-blocking render loop (`mod.rs`), modular application state and commands (`app/` partitioned into `mod.rs`, `tabs.rs`, `stream.rs`, `render/`, `commands/`, and `events/`), unified tab state (`tab.rs`), cache hydration bridge (`history.rs`), 2-level render cache (`cache/` including `user_format.rs`), viewport state machine (`viewport.rs`), incremental markdown parser (`markdown/` including `wrap.rs`), syntax highlighter (`highlight.rs`), rope editor (`editor/`), autocomplete (`autocomplete.rs`), modal stack (`modal/` partitioned into `commands.rs`, `danger.rs`, `model_effort/`, `select/`, and `usage/`), sensory notifications (`notify.rs`), TTY recovery (`terminal.rs`), Atlas Crimson theme (`theme.rs`), and model/provider label formatter (`labels.rs`).
- **Entrypoints (`src/`)**: Binary bootstrap (`main.rs`), library facade (`lib.rs`), and CLI argument parser (`cli.rs`).

---

## 5. Consolidated Capabilities and Continuous Evolution

Frontal is operational in production (version `0.20.0`), fully verified by **276 tests across 8 domains**. Development progresses continuously via GitHub Project Board #8 cards across these consolidated domains:

- **Foundation, `stream-json` & ACP Protocols, and Headless IPC:**
  - Single Cargo crate, session multiplexing via `SessionId`, typed `stream-json` and JSON-RPC 2.0 ACP (`agy acp --stdio`) protocols, `AcpEngine` with RFC 4648 Base64 `ImageContentBlock` and async permission handling, `AgyEngine`, `OpenCodeEngine`, `FixtureReplayEngine`, async line decoding via `LinesCodec` (16 MB buffer), structured `PromptPayload` (`text` + `media`), and POSIX process isolation (`setpgid`/`killpg`/`PR_SET_PDEATHSIG`).
- **Core TUI (60 FPS), Sticky Scroll Viewport, and 2-Level Cache:**
  - Non-blocking Ratatui event loop at 60 FPS, bracketed paste, TTY restore panic hook, deterministic sticky scroll, and 2-level render cache (L1 immutable AST, L2 width/revision indexed visual lines) with O(1)-memory defensive tool output clamping (`clamp_tool_output`).
- **Incremental Markdown Streaming, `syntect` Highlighting, and Tool Cards:**
  - Resilient `pulldown-cmark` incremental parser handling open code blocks with indentation-preserving wrapping, multi-row wrapped table cells (`wrap.rs`), line-by-line syntax highlighting with state checkpoints, terminal output sanitization (`\r`/ANSI), clean collapsible thinking blocks (`+ Thought`/`- Thought`), command indicators (`●`), and interactive tool execution cards.
- **`ropey` Editor, Multimodal Attachments, Autocomplete, and SQLite WAL History:**
  - Multiline B-Tree buffer, downward-anchored priority popover overlay, atomic mention and media extmarks (`@`, `/`, `[Image #N]` in `ACCENT_RUBY`), collapsed paste pills, `Ctrl+V` / `Alt+V` clipboard RGBA image capture with zero-dependency PNG encoding into `~/.frontal/cache/media/`, drag-and-drop image paths, chat transcript `[Image N]` highlighting with `ACCENT_RUBY` ` file ` badges, async file searching worker, and persistent SQLite WAL command history.
- **Multi-Session (`TabState`), Modal Stack (`DialogSelect`), Double-`Esc` Interruption, and Safety Guardian:**
  - OpenCode-style interactive tab bar (`Ctrl+T`, `Ctrl+W`, `Alt+1..9`, hover-only `×` close, `+` new tab) via `SessionManager`, staged prompt barrier, local command dispatch, double-`Esc` active-turn interruption (`1500 ms` confirmation window with `Thinking...  esc again to interrupt` hint in `TEXT_INTERRUPT_HINT` and muted `Prompt interrupted` block, keeping `Ctrl+C` strictly for selection copy, input clear, and quit), floating modal dialog stack with backdrop dimming and standardized header alignment, deterministic Safety Guardian (`DangerAssessment`) with confirmation containment modal for destructive actions, unified `DialogSelect` component (keyboard, mouse navigation, and two-step confirmed session deletion), hot-respawn model switching, and commands modal (`Ctrl+P`).
- **Multi-Backend Harness (`FrontalEngine`), OpenCode Integration, and Resume Parity:**
  - Decoupled `AgyEngine` (persistent pipe daemon, default), `AcpEngine`, and `OpenCodeEngine` (per-turn `opencode run` supervisor with `--file` attachments) behind `FrontalEngine`, runtime model discovery via `opencode models`, a unified `/model` provider selector (Antigravity plus each OpenCode provider) with global search that shows the provider on every result and human-readable provider/model labels while identifiers stay canonical, contextual `/quota` with asynchronous Go usage windows (`opencode_api.rs`) and an OpenCode `Global Usage & Tool Stats` subpage (`stats.rs`), cross-backend `conversation_id` isolation, unified `history.db` session indexing (`backend` + `TitleSource`) with `deleted_sessions` tombstones and non-blocking external Agy/OpenCode discovery, `/resume` engine badges (`[Agy]` / `[OpenCode]`) with two-step deletion cleanup (`opencode session delete`), automatic provisional-to-semantic tab title upgrades preserving `/rename`, and symmetric `/resume` hydration (`hydrate_session_blocking`, `transcript::load_usage`, `opencode::load_usage`, `opencode.db` `session_message` hydration) restoring history, model, effort, and token telemetry on both engines without mutating global `user_config`.
- **OpenCode Visual Ergonomics, Hero Welcome State, Context Telemetry, and Atlas Crimson Theme:**
  - Centered Hero welcome state featuring a 4-row (`35×4`) half-block `Frontal` wordmark with Atlas Crimson vertical gradient and 6×6 truecolor Atlas Gem (`< 50×16` compact fallback), width-aware visual input word-wrapping and cursor alignment (`input.rs`) across Welcome and chat, fluid transition to 2-column layout (conversation + Context Sidebar with `ModelSpec` resolution, token count, semaphore indicator `●` and compaction threshold, `Ctrl+B` toggle, narrow-screen footer semaphore gauge, and persisted turn metadata across `/resume`), 2-column aligned autocomplete popover, scroll indicator, **Atlas Crimson** theme, normalized `/model` selector with reasoning effort slider (`◀ ●──◎──● ▶`), clean column 0 margin alignment, granular text selection in viewport, and faithful clipboard copying.

---

## 6. Build, Test, and Run

### Prerequisites
- Stable Rust toolchain (`cargo`, `rustc` 2021 edition).
- `agy` executable or compatible engine available in `PATH` (or configured via `--agy-bin`).

### Build & Verification Commands
```bash
# Optimized release build
cargo build --release

# Execute full 276 tests across 8 domains
cargo test --quiet
```

### Launch and CLI Options
```bash
# Standard interactive TUI
cargo run --release

# Specify model, reasoning effort, and mode
cargo run --release -- --model gemini-3.8-flash-high --effort high --mode plan

# Resume existing conversation by UUID
cargo run --release -- --conversation <UUID>

# Automated headless execution with a single prompt
cargo run --release -- --headless -p "Describe the project architecture"
```

---

## 7. Technical Documentation and Specifications

Detailed technical documentation is available on the [GitHub Wiki](https://github.com/julioCoronetti/frontal/wiki):

- **[Home](https://github.com/julioCoronetti/frontal/wiki/Home)**: Central documentation index.
- **[Frontal-Architecture](https://github.com/julioCoronetti/frontal/wiki/Frontal-Architecture)**: Official Rust implementation architecture (v0.20.0), modules, Atlas Crimson theme, and 276 tests suite.
- **[Board-Roadmap](https://github.com/julioCoronetti/frontal/wiki/Board-Roadmap)**: Architecture specification, Rust tech stack, decision matrix, consolidated capabilities, and Project Board evolution.
- **Antigravity CLI (`agy`) Dossier**:
  - [Agy](https://github.com/julioCoronetti/frontal/wiki/Agy): Official CLI overview.
  - [Agy-Architecture](https://github.com/julioCoronetti/frontal/wiki/Agy-Architecture): Go binary, process orchestration, and `stream-json` protocol.
  - [Agy-Backend](https://github.com/julioCoronetti/frontal/wiki/Agy-Backend): Cortex engine, tools, security containment, and SQLite storage.
  - [Agy-UI](https://github.com/julioCoronetti/frontal/wiki/Agy-UI): Bubble Tea v2, 48 model catalog, EditArea, and viewports.
- **OpenCode Dossier**:
  - [OpenCode](https://github.com/julioCoronetti/frontal/wiki/OpenCode): Ergonomic reference overview.
  - [OpenCode-Architecture](https://github.com/julioCoronetti/frontal/wiki/OpenCode-Architecture): 34-package monorepo, Effect-TS runtime, dual-process RPC.
  - [OpenCode-Backend](https://github.com/julioCoronetti/frontal/wiki/OpenCode-Backend): Turn cycle, tool bounding, shadow Git, and session compaction.
  - [OpenCode-UI](https://github.com/julioCoronetti/frontal/wiki/OpenCode-UI): SolidJS + OpenTUI, Tree-sitter WASM, extmarks, keymaps, and theming.
- **[Legacy](https://github.com/julioCoronetti/frontal/wiki/Legacy)**: Initial v0.1 Python/Textual prototype dossier (code preserved on the [`legacy`](https://github.com/julioCoronetti/frontal/tree/legacy) branch).
