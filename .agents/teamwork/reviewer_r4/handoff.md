> [!WARNING] **Skepticism Disclaimer**
> Moderate-to-high confidence across the 81-test automated suite; Windows host Win32 clipboard locking is mitigated through PowerShell-level try/catch sleep loops and memory fallback, but external third-party clipboard managers on Windows could still induce momentary delays.

## 1. What the prior attempt got wrong

### Issue 1: Flaky Windows clipboard retrieval under Win32 lock (Ledger Item 11)
- **Input:** Rapid successive clipboard write and read operations across test runs (`test_clipboard_programmatic_byte_exact`, `test_clipboard_special_unicode_integrity`, `test_code_block_interactive_copy_button`).
- **Expected:** `ClipboardManager.read_clipboard()` successfully returns the clipboard contents or falls back to `_fallback_buffer` if the clipboard is temporarily locked.
- **Actual:** `powershell.exe Get-Clipboard` intermittently threw `ExternalException` ("A operação de Área de Transferência solicitada não foi bem-sucedida") due to Win32 clipboard locking, producing empty output `""` with exit code 0. `read_clipboard` returned `""` unconditionally without retrying or falling back, failing assertions.
- **Root cause:** `ClipboardManager.read_clipboard` executed a single unhandled PowerShell expression `[Console]::Out.Write((Get-Clipboard -Raw))` lacking retry backoff loops, lacking `stdin=subprocess.DEVNULL` (which risks hanging in WSL when stdin is unredirected), and returning empty strings immediately rather than falling back to `_fallback_buffer`.

### Issue 2: Potential clip.exe transient write lock drop
- **Input:** `ClipboardManager.copy_clip_exe` invoked while another process momentarily holds the Win32 clipboard open.
- **Expected:** Retry with short backoff before returning failure.
- **Actual:** Single write attempt failed immediately without retry.
- **Root cause:** `copy_clip_exe` lacked an attempt loop with sleep backoff.

## 2. What I changed

- `agy_tui/clipboard.py`:
  - Imported `time`.
  - Updated `copy_clip_exe` to retry up to 2 times with 50ms backoff on lock failure.
  - Set `stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL` in `wl-copy` and `xclip` fallbacks in `copy()`.
  - Re-implemented `read_clipboard()`:
    - Added PowerShell internal retry loop (up to 5 attempts with 60ms sleep backoff) catching `ExternalException` on `Get-Clipboard -Raw -ErrorAction Stop`.
    - Added `stdin=subprocess.DEVNULL` to all subprocess calls to prevent WSL stdin blocking.
    - Added Python-level retry loop (2 attempts) for PowerShell execution.
    - Implemented guarded return: if output is empty string `""` while `_fallback_buffer` is non-empty, falls back to `wl-paste`, `xclip`, or `_fallback_buffer`.
    - Maintained exact newline normalization (`replace("\r\n", "\n")`).
    - Verified 0 source code comments.
- `tests/test_clipboard.py`:
  - Added 3 regression tests:
    - `test_clipboard_read_powershell_external_exception_fallback`: verifies fallback to `_fallback_buffer` when PowerShell exits with code 1.
    - `test_clipboard_read_powershell_empty_stdout_fallback`: verifies fallback to `_fallback_buffer` when PowerShell returns empty stdout on exit code 0 while buffer is populated.
    - `test_clipboard_read_retry_backoff_success`: verifies retry recovery when first PowerShell call fails and second succeeds.
  - Verified 0 source code comments.

## 3. Verification Record
- **Deep Verification (ran actual tests):**
  - Full test suite: `pytest tests/` -> 81 passed in 111.02s, 0 failures, 0 warnings.
  - Fullscreen suite: `pytest -v tests/test_fullscreen_tui.py` -> 27 passed in 106.82s.
  - Unit/integration suite: `pytest -v tests/test_app.py tests/test_clipboard.py tests/test_engine.py tests/test_integration_cli.py tests/test_protocol.py tests/test_renderer.py` -> 54 passed in 2.26s.
  - Linter: `ruff check agy_tui tests` -> All checks passed, 0 violations.
  - Comment scan: Tokenizer verified 0 comments across all `.py` files in `agy_tui` and `tests`.
  - Containment: Verified 0 files created or modified outside `/home/julio_regino/workspace/github/agy-tui`.
- **Shallow Verification (manual only):**
  - Simulated `ExternalException` thrown in PowerShell with backoff sleep recovery.
- **Unverified aspects:**
  - Physical non-WSL native Linux without X11 or Wayland display servers (relies on memory fallback buffer).
  - External clipboard synchronizers with multi-second lock durations exceeding retry timeouts.

## 4. Known Issues
- `Minor Robustness Risk`: Terminal viewports under 40 columns width require collapsing sidebar (`Ctrl+B`) for code readability.
- `Shallow Verification`: Visual rendering nuances on specialized hardware terminals without truecolor support.

## 5. Remaining risk & next step
All open ledger items, including Ledger Item 11, are resolved and verified. All 81 automated tests pass cleanly in headless mode with 0 lint violations, 0 source code comments, and strict workspace containment. The task is complete.
