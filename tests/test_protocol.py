import json

from agy_tui.protocol import format_user_message, parse_stream_line


def test_format_user_message():
    content = "Hello, write a script"
    formatted = format_user_message(content)
    assert formatted.endswith("\n")
    data = json.loads(formatted)
    assert data["event"] == "user"
    assert data["message"]["content"] == content


def test_parse_init_event():
    line = json.dumps({
        "event": "init",
        "conversation_id": "test-uuid-1234",
        "init": {
            "cwd": "/workspace",
            "tools": ["read_file", "write_file"],
            "permission_mode": "always-proceed",
        },
    })
    event = parse_stream_line(line)
    assert event is not None
    assert event.event_type == "init"
    assert event.conversation_id == "test-uuid-1234"
    assert not event.is_done
    assert not event.is_error


def test_parse_step_update_user_input():
    line = json.dumps({
        "event": "step_update",
        "step_update": {
            "conversation_id": "conv-555",
            "step_index": 0,
            "state": "DONE",
            "step_type": "user_input",
        },
    })
    event = parse_stream_line(line)
    assert event is not None
    assert event.event_type == "step_update"
    assert event.conversation_id == "conv-555"
    assert event.step_index == 0
    assert event.step_type == "user_input"
    assert event.state == "DONE"


def test_parse_step_update_agent_response():
    line = json.dumps({
        "event": "step_update",
        "step_update": {
            "conversation_id": "conv-555",
            "step_index": 1,
            "state": "ACTIVE",
            "step_type": "agent_response",
            "text_delta": "print('hello')",
        },
    })
    event = parse_stream_line(line)
    assert event is not None
    assert event.text_delta == "print('hello')"
    assert event.step_type == "agent_response"
    assert event.state == "ACTIVE"


def test_parse_thought_event():
    standalone_line = json.dumps({
        "event": "thought",
        "thought": "Thinking about the solution...",
    })
    event1 = parse_stream_line(standalone_line)
    assert event1 is not None
    assert event1.thought_text == "Thinking about the solution..."

    step_thought_line = json.dumps({
        "event": "step_update",
        "step_update": {
            "step_type": "thought",
            "text_delta": "Evaluating trade-offs",
        },
    })
    event2 = parse_stream_line(step_thought_line)
    assert event2 is not None
    assert event2.thought_text == "Evaluating trade-offs"


def test_parse_result_success():
    line = json.dumps({
        "event": "result",
        "result": {
            "conversation_id": "conv-999",
            "status": "SUCCESS",
            "response": "Completed task.",
            "duration_seconds": 1.25,
            "usage": {"total_tokens": 120},
        },
    })
    event = parse_stream_line(line)
    assert event is not None
    assert event.is_done
    assert not event.is_error
    assert event.result_status == "SUCCESS"
    assert event.result_response == "Completed task."
    assert event.conversation_id == "conv-999"


def test_parse_result_error():
    line = json.dumps({
        "event": "result",
        "result": {
            "conversation_id": "conv-err",
            "status": "ERROR",
            "error": "Timeout contacting model",
        },
    })
    event = parse_stream_line(line)
    assert event is not None
    assert event.is_done
    assert event.is_error
    assert event.result_error == "Timeout contacting model"


def test_parse_raw_error_line():
    line = "error: stream input message is missing the 'event' field"
    event = parse_stream_line(line)
    assert event is not None
    assert event.is_error
    assert "stream input" in event.raw.get("error", "")


def test_parse_empty_and_whitespace():
    assert parse_stream_line("") is None
    assert parse_stream_line("   \n\t  ") is None
    assert parse_stream_line(b"") is None


def test_parse_bytes_input():
    line = b'{"event": "message", "message": {"content": "ok"}}'
    event = parse_stream_line(line)
    assert event is not None
    assert event.text_delta == "ok"
