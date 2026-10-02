import base64
import io
import os
import shutil
from unittest.mock import MagicMock, patch

from agy_tui.clipboard import ClipboardManager


def test_osc52_encoding_integrity():
    text = (
        "def example():\n"
        "    indent_4 = True\n"
        "\tindent_tab = True\n"
        "    special_chars = \"'<>;&|`$\"\n"
        "    return 42\n"
    )
    buffer = io.StringIO()
    manager = ClipboardManager(enable_osc52=True, output_stream=buffer)
    success = manager.copy_osc52(text)
    assert success is True

    seq = buffer.getvalue()
    assert seq.startswith("\x1b]52;c;")
    assert seq.endswith("\x07")

    payload_b64 = seq[len("\x1b]52;c;"):-1]
    decoded = base64.b64decode(payload_b64).decode("utf-8")
    assert decoded == text


def test_clipboard_programmatic_byte_exact():
    test_code = (
        "class DataPipeline:\n"
        "    def __init__(self, name: str) -> None:\n"
        "        self.name = name\n"
        "        self.records: list[dict] = []\n"
        "\n"
        "    def process(self, batch: list[dict]) -> int:\n"
        "        valid = [r for r in batch if r.get('active')]\n"
        "        self.records.extend(valid)\n"
        "        return len(valid)\n"
    )
    manager = ClipboardManager()
    copied = manager.copy(test_code)
    assert copied is True

    powershell_bin = shutil.which("powershell.exe") or "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"
    if os.path.exists(powershell_bin) and manager.clip_exe_available:
        retrieved = manager.read_clipboard(normalize_newlines=True)
        assert retrieved == test_code
    else:
        assert manager.read_clipboard() == test_code


def test_clipboard_fallback():
    manager = ClipboardManager(clip_exe_path="/nonexistent/path/clip.exe", enable_osc52=False)
    sample = "fallback test content with indentation:\n    val = 123"
    assert manager.copy(sample) is True
    assert manager.read_clipboard() == sample


def test_clipboard_empty_string():
    manager = ClipboardManager()
    assert manager.copy("") is True
    assert manager.encode_osc52("") == "\x1b]52;c;\x07"


def test_clipboard_special_unicode_integrity():
    special = "def calcular_preço(taxa: float) -> str:\n    resultado = f'R$ {taxa * 1.5:.2f}'\n    return resultado\n"
    manager = ClipboardManager()
    assert manager.copy(special) is True
    assert manager.read_clipboard(normalize_newlines=True) == special


def test_clipboard_empty_string_pure_fallback():
    manager = ClipboardManager(clip_exe_path="/nonexistent/clip.exe", enable_osc52=False)
    manager._clip_exe = None
    assert manager.copy("") is True
    assert manager.read_clipboard() == ""


def test_clipboard_read_powershell_external_exception_fallback():
    manager = ClipboardManager()
    manager._clip_exe = "/mock/clip.exe"
    manager._fallback_buffer = "fallback_code = 42\n"
    with patch("os.path.exists", return_value=True), patch("shutil.which", return_value="/mock/powershell.exe"):
        mock_res = MagicMock()
        mock_res.returncode = 1
        mock_res.stdout = b""
        with patch("subprocess.run", return_value=mock_res):
            val = manager.read_clipboard(normalize_newlines=True)
            assert val == "fallback_code = 42\n"


def test_clipboard_read_powershell_empty_stdout_fallback():
    manager = ClipboardManager()
    manager._clip_exe = "/mock/clip.exe"
    manager._fallback_buffer = "def sample():\n    pass\n"
    with patch("os.path.exists", return_value=True), patch("shutil.which", return_value="/mock/powershell.exe"):
        mock_res = MagicMock()
        mock_res.returncode = 0
        mock_res.stdout = b""
        with patch("subprocess.run", return_value=mock_res):
            val = manager.read_clipboard(normalize_newlines=True)
            assert val == "def sample():\n    pass\n"


def test_clipboard_read_retry_backoff_success():
    manager = ClipboardManager()
    manager._clip_exe = "/mock/clip.exe"
    manager._fallback_buffer = "def sample():\n    pass\n"
    with patch("os.path.exists", return_value=True), patch("shutil.which", return_value="/mock/powershell.exe"):
        fail_res = MagicMock(returncode=1, stdout=b"")
        ok_res = MagicMock(returncode=0, stdout=b"def sample():\r\n    pass\r\n")
        with patch("subprocess.run", side_effect=[fail_res, ok_res]):
            val = manager.read_clipboard(normalize_newlines=True)
            assert val == "def sample():\n    pass\n"

