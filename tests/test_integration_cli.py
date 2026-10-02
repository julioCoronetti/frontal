import os
import subprocess
import sys


def test_cli_integration_interactive():
    mock_engine_path = os.path.abspath(os.path.join(os.path.dirname(__file__), "mock_engine.py"))
    cmd = [
        sys.executable,
        "-m",
        "agy_tui",
        "--agy-bin",
        mock_engine_path,
        "--conversation",
        "test-integration-conv-1",
        "--no-osc52",
    ]

    proc = subprocess.Popen(
        cmd,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )

    inputs = [
        "/help\n",
        "echo test message\n",
        "/history\n",
        "/quit\n",
    ]
    stdout_data, stderr_data = proc.communicate(input="".join(inputs), timeout=10.0)

    assert proc.returncode == 0
    assert "Antigravity CLI TUI Frontend" in stdout_data or "Antigravity CLI" in stdout_data
    assert "Available Commands" in stdout_data
    assert "Response to: echo test message" in stdout_data
    assert "echo test message" in stdout_data
    assert "agy session ended cleanly." in stdout_data
