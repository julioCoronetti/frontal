# Frontal

Decoupled, asynchronous, high-performance Terminal User Interface (TUI) harness developed in **Rust** (version `0.9.0`) for the Antigravity CLI (`agy`) and agentic engines compatible with the `stream-json` protocol.

---

## 1. Overview

Frontal provides a high-performance terminal presentation layer designed for the stable, responsive rendering of continuous LLM token streams. The harness consumes structured events (NDJSON) over process pipes (`stdin`/`stdout`), fully decoupling the AI engine lifecycle from the visual interface. Architectural stability is verified by a unified suite of **150 tests across 8 domains**.

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
| **Serialization** | `serde` + `serde_json` | Strict, typed deserialization of `stream-json` events and commands. |
| **Prompt Editor** | `ropey` + Token Registry | B-Tree (Rope) buffer for fast multiline navigation, atomic mention extmarks (`@`, `/`), and large paste collapse (`[Pasted ~N lines]`). |
| **Markdown Parsing** | `pulldown-cmark` | High-performance CommonMark event parser building an internal AST of modular blocks (`MessageBlock`). |
| **Syntax Highlighting** | `syntect` | Line-incremental highlighting with 150+ bundled grammars without native C compilation. |
| **Local Persistence** | `rusqlite` (WAL mode) | Embedded lightweight SQLite for session metadata, command history, and cache. |
| **System Clipboard** | `arboard` + OSC 52 | Native system clipboard access with fallback to OSC 52 escape sequences for remote SSH sessions. |
| **Process Isolation & POSIX** | `libc` | POSIX process groups (`setpgid` / `killpg`) and orphan prevention (`PR_SET_PDEATHSIG`). |
| **Buffers & Async Utils** | `bytes` + `futures-util` | Network buffer management and async stream combinators with minimal allocations. |

---

## 3. Architectural Invariants

1. **Two-Level Render Cache ($O(1)$ during active streaming)**:
   - Consolidated turn messages are preserved as immutable semantic ASTs (`MessageBlock` with style spans) in Level 1.
   - Level 2 maintains pre-formatted visual lines indexed by `(width, revision)`, lazily invalidated on terminal resizes (`SIGWINCH`) or block expand/collapse.
   - Only the active streaming block computes deltas, keeping frame generation at $O(1)$ and locking display refresh to 60 FPS regardless of conversation history size.
2. **Deterministic Sticky Scroll**:
   - Viewport remains locked to the bottom edge during token streaming, immediately unanchors on user scroll-up, and re-anchors upon returning to the footer.
3. **Large Paste Folding**:
   - Clipboard insertions exceeding 5 lines collapse into interactive visual pills in the prompt, preventing layout disruption.
4. **POSIX Process Isolation & Cleanup**:
   - Subprocesses are spawned in dedicated process groups via `setpgid(0, 0)` with `PR_SET_PDEATHSIG(libc::SIGTERM)` to guarantee that terminating Frontal never leaves orphaned `agy` processes running.

---

## 4. Consolidated Capabilities and Continuous Evolution

Frontal is operational in production (version `0.9.0`), fully verified by **150 tests across 8 domains**. Development progresses continuously via GitHub Project Board #8 cards across these consolidated domains:

- **Foundation, `stream-json` Protocol, and Headless IPC:**
  - Single Cargo workspace, session multiplexing via `SessionId`, typed `stream-json` protocol, dual `EngineTransport` implementations (`SubprocessEngine` and `FixtureReplayEngine`), async line decoding via `LinesCodec` (16 MB buffer), and POSIX process isolation (`setpgid`/`killpg`/`PR_SET_PDEATHSIG`).
- **Core TUI (60 FPS), Sticky Scroll Viewport, and 2-Level Cache:**
  - Non-blocking Ratatui event loop at 60 FPS, bracketed paste, TTY restore panic hook, deterministic sticky scroll, and 2-level render cache (L1 immutable AST, L2 width/revision indexed visual lines).
- **Incremental Markdown Streaming, `syntect` Highlighting, and Tool Cards:**
  - Resilient `pulldown-cmark` incremental parser handling open code blocks, line-by-line syntax highlighting with state checkpoints, terminal output sanitization (`\r`/ANSI), clean collapsible thinking blocks (`+ Thought`/`- Thought`), command indicators (`●`), and interactive tool execution cards.
- **`ropey` Editor with Atomic Extmarks, Autocomplete, and SQLite WAL History:**
  - Multiline B-Tree buffer, priority popover overlay, atomic mention tags (`@`, `/`), collapsed paste pills, async file searching worker, and persistent SQLite WAL command history.
- **Multi-Session (`TabState`), Modal Stack (`DialogSelect`), Safety Guardian, and Hot-Respawn:**
  - Multi-tab management (`Alt+1..9`) via `SessionManager`, staged prompt barrier, local command dispatch, floating modal dialog stack with backdrop dimming, deterministic Safety Guardian (`DangerAssessment`) with confirmation containment modal for destructive actions, unified `DialogSelect` component (keyboard and mouse navigation), hot-respawn model switching, command palette (`Ctrl+P`), and integrated git diff viewer.
- **OpenCode Visual Ergonomics, Atlas Crimson Theme, and Clipboard Fidelity:**
  - Centered welcome state, fluid transition to 2-column layout (conversation + Context Sidebar), 2-column aligned autocomplete popover, scroll indicator, **Atlas Crimson** theme, normalized `/model` selector with reasoning effort slider (`◀ ●──◎──● ▶`), clean column 0 margin alignment, granular text selection in viewport, and faithful clipboard copying.

---

## 5. Build, Test, and Run

### Prerequisites
- Stable Rust toolchain (`cargo`, `rustc` 2021 edition).
- `agy` executable or compatible engine available in `PATH` (or configured via `--agy-bin`).

### Build & Verification Commands
```bash
# Optimized release build
cargo build --release

# Execute full 150 tests across 8 domains
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

## 6. Technical Documentation and Specifications

Detailed technical documentation is available on the [GitHub Wiki](https://github.com/julioCoronetti/frontal/wiki):

- **[Home](https://github.com/julioCoronetti/frontal/wiki/Home)**: Central documentation index.
- **[Frontal-Architecture](https://github.com/julioCoronetti/frontal/wiki/Frontal-Architecture)**: Official Rust implementation architecture (v0.9.0), modules, Atlas Crimson theme, and 150 tests suite.
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
