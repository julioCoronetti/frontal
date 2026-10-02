# agy-tui

Decoupled, high-performance TUI frontend for Antigravity CLI (`agy`) communicating via headless `stream-json`.

## Features

- **Headless Stream Protocol**: Async bidirectional streaming (`stdin`/`stdout`/`stderr`) using the native `stream-json` engine protocol without blocking the UI.
- **Session & Concurrency Isolation**: Every session automatically generates an isolated conversation ID (`--conversation <uuid>`) or resumes a specified session, preventing collision with parallel `agy` runs.
- **Markdown & Code Presentation**: Clean Markdown rendering with syntax highlighting and code block extraction without hard-wrap line injections.
- **Collapsible / Distinct Event Styling**: Visually separated panels for reasoning (`thought`), tool actions (`step_update`), and execution results.
- **Clipboard Integration**: Direct copy of code blocks and responses to OS clipboard via Windows/WSL `clip.exe` and terminal OSC 52 sequences, preserving exact indentation and newlines.
- **Interactive Input**: Full multiline input support, command history (Up/Down arrows), turn interruption via `Ctrl+C`, and slash commands.
- **Zero Zombie Processes**: Clean asynchronous subprocess lifecycle with graceful shutdown, signal management, and wait-reaping.

## Installation

```bash
pip install -e .
```

## Usage

```bash
# Launch agy-tui
python3 -m agy_tui

# Connect to a specific conversation or project
python3 -m agy_tui --conversation <id> --project <project_id>

# Run with custom execution mode or effort
python3 -m agy_tui --mode plan --effort high
```

## Commands & Shortcuts

- `/c <id>` or `/copy <id>`: Copy code block `<id>` to clipboard.
- `/ca` or `/copyall`: Copy full assistant response to clipboard.
- `/multi`: Toggle multiline input mode.
- `/send`: Submit multiline buffer.
- `/history`: Display recent commands.
- `/clear`: Clear terminal screen.
- `/help`: Show command assistance.
- `/quit` or `/exit`: Gracefully terminate session.
- `Ctrl+C`: Interrupt active turn or clear input line.
- `Ctrl+D`: Submit multiline buffer or exit.
- `Up / Down`: Browse command history.

## Testing

```bash
pytest -v
ruff check agy_tui tests
```
