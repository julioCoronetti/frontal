import json
from dataclasses import dataclass
from typing import Any


@dataclass
class StreamEvent:
    event_type: str
    raw: dict[str, Any]

    @property
    def conversation_id(self) -> str | None:
        if "conversation_id" in self.raw:
            return str(self.raw["conversation_id"])
        for key in ("step_update", "result", "init"):
            nested = self.raw.get(key)
            if isinstance(nested, dict) and "conversation_id" in nested:
                return str(nested["conversation_id"])
        return None

    @property
    def text_delta(self) -> str | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict) and "text_delta" in step:
            return step["text_delta"]
        if self.event_type == "message":
            msg = self.raw.get("message")
            if isinstance(msg, dict):
                return msg.get("content")
            return self.raw.get("content")
        return None

    @property
    def step_type(self) -> str | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            return step.get("step_type")
        return None

    @property
    def state(self) -> str | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            return step.get("state")
        return None

    @property
    def step_index(self) -> int | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            return step.get("step_index")
        return None

    @property
    def thought_text(self) -> str | None:
        if self.event_type in ("thought", "thinking"):
            for key in ("thought", "content", "text"):
                val = self.raw.get(key)
                if isinstance(val, str) and val:
                    return val
                if isinstance(val, dict) and "text" in val:
                    return str(val["text"])
            msg = self.raw.get("message")
            if isinstance(msg, str) and msg:
                return msg
            if isinstance(msg, dict):
                for key in ("content", "thought", "text"):
                    if key in msg and msg[key]:
                        return str(msg[key])
        step = self.raw.get("step_update")
        if isinstance(step, dict) and (step.get("step_type") in ("thought", "thinking") or "thought" in step):
            val = step.get("text_delta") or step.get("content") or step.get("thought")
            if val:
                return str(val)
        return None

    @property
    def result_data(self) -> dict[str, Any] | None:
        res = self.raw.get("result")
        if isinstance(res, dict):
            return res
        if self.event_type == "result":
            return self.raw
        return None

    @property
    def result_status(self) -> str | None:
        data = self.result_data
        if data:
            return data.get("status")
        return None

    @property
    def result_response(self) -> str | None:
        data = self.result_data
        if data:
            return data.get("response")
        return None

    @property
    def result_error(self) -> str | None:
        data = self.result_data
        if data:
            return data.get("error")
        return self.raw.get("error")

    @property
    def usage(self) -> dict[str, Any] | None:
        data = self.result_data
        if data and isinstance(data.get("usage"), dict):
            return data["usage"]
        if "usage" in self.raw and isinstance(self.raw["usage"], dict):
            return self.raw["usage"]
        step = self.raw.get("step_update")
        if isinstance(step, dict) and isinstance(step.get("usage"), dict):
            return step["usage"]
        return None

    @property
    def tool_name(self) -> str | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            return step.get("tool_name") or step.get("tool") or step.get("name")
        return self.raw.get("tool_name") or self.raw.get("tool")

    @property
    def tool_args(self) -> dict[str, Any] | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            args = step.get("args") or step.get("parameters") or step.get("input")
            if isinstance(args, dict):
                return args
        return None

    @property
    def tool_diff(self) -> str | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            for k in ("diff", "patch", "unified_diff"):
                if k in step and step[k]:
                    return str(step[k])
            args = step.get("args") or step.get("parameters") or {}
            if isinstance(args, dict):
                for k in ("diff", "patch", "unified_diff"):
                    if k in args and args[k]:
                        return str(args[k])
        for k in ("diff", "patch", "unified_diff"):
            if k in self.raw and self.raw[k]:
                return str(self.raw[k])
        return None

    @property
    def subagents(self) -> list[str] | None:
        step = self.raw.get("step_update")
        if isinstance(step, dict):
            if "subagents" in step and isinstance(step["subagents"], list):
                return [str(s) for s in step["subagents"]]
            if "subagent" in step and step["subagent"]:
                return [str(step["subagent"])]
        if "subagents" in self.raw and isinstance(self.raw["subagents"], list):
            return [str(s) for s in self.raw["subagents"]]
        return None

    @property
    def is_done(self) -> bool:
        return self.event_type == "result"

    @property
    def is_error(self) -> bool:
        if self.event_type == "error":
            return True
        if self.result_status == "ERROR":
            return True
        return "error" in self.raw and self.event_type != "init"


def format_user_message(content: str) -> str:
    payload = {
        "event": "user",
        "message": {
            "content": content
        }
    }
    return json.dumps(payload, ensure_ascii=False) + "\n"


def parse_stream_line(line: str | bytes) -> StreamEvent | None:
    if isinstance(line, bytes):
        line = line.decode("utf-8", errors="replace")
    line = line.strip()
    if not line:
        return None
    try:
        data = json.loads(line)
    except json.JSONDecodeError:
        if line.startswith("error:"):
            return StreamEvent(event_type="error", raw={"error": line[6:].strip()})
        return StreamEvent(event_type="error", raw={"error": line})
    if not isinstance(data, dict):
        return None
    event_type = data.get("event") or data.get("type")
    if not event_type:
        if "error" in data:
            return StreamEvent(event_type="error", raw=data)
        return None
    return StreamEvent(event_type=str(event_type), raw=data)
