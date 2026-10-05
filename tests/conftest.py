import os

import pytest


@pytest.fixture(autouse=True)
def isolate_test_environment(monkeypatch, tmp_path):
    mock_engine = os.path.join(os.path.dirname(__file__), "mock_engine.py")
    test_sessions = str(tmp_path / ".test_sessions.json")
    monkeypatch.setenv("AGY_BIN", mock_engine)
    monkeypatch.setenv("AGY_SESSIONS_FILE", test_sessions)
    yield
    for filename in (".test_sessions.json", ".test_sessions_effort.json"):
        path = os.path.join(os.path.dirname(__file__), filename)
        if os.path.exists(path):
            try:
                os.remove(path)
            except Exception:
                pass
