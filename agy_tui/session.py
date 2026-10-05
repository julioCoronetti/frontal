import json
import os
import time
from dataclasses import asdict, dataclass


@dataclass
class SessionRecord:
    conversation_id: str
    timestamp: str
    model: str
    mode: str
    effort: str
    last_prompt: str
    turns: int = 1
    updated_at: float = 0.0


class SessionManager:
    DEFAULT_STORAGE = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), ".sessions.json")

    def __init__(self, storage_path: str | None = None) -> None:
        self.storage_path = storage_path or os.environ.get("AGY_SESSIONS_FILE") or self.DEFAULT_STORAGE
        self._sessions: dict[str, SessionRecord] = {}
        self._load()

    def _load(self) -> None:
        if not os.path.exists(self.storage_path):
            return
        try:
            with open(self.storage_path, encoding="utf-8") as f:
                data = json.load(f)
            if isinstance(data, list):
                for item in data:
                    if isinstance(item, dict) and "conversation_id" in item:
                        rec = SessionRecord(
                            conversation_id=str(item["conversation_id"]),
                            timestamp=str(item.get("timestamp", "")),
                            model=str(item.get("model", "default")),
                            mode=str(item.get("mode", "code")),
                            effort=str(item.get("effort", "medium")),
                            last_prompt=str(item.get("last_prompt", "")),
                            turns=int(item.get("turns", 1)),
                            updated_at=float(item.get("updated_at", 0.0)),
                        )
                        self._sessions[rec.conversation_id] = rec
        except Exception:
            pass

    def _save(self) -> None:
        try:
            records = [asdict(r) for r in self._sessions.values()]
            records.sort(key=lambda x: (x.get("updated_at", 0.0), x.get("timestamp", "")), reverse=True)
            with open(self.storage_path, "w", encoding="utf-8") as f:
                json.dump(records, f, indent=2, ensure_ascii=False)
        except Exception:
            pass

    def record(
        self,
        conversation_id: str,
        model: str | None = None,
        mode: str | None = None,
        effort: str | None = None,
        prompt: str = "",
    ) -> SessionRecord:
        now_ts = time.time()
        now_str = time.strftime("%Y-%m-%d %H:%M:%S")
        prompt_snippet = (prompt[:60] + "...") if len(prompt) > 60 else prompt
        prompt_snippet = prompt_snippet.replace("\n", " ").strip()

        if conversation_id in self._sessions:
            rec = self._sessions[conversation_id]
            rec.timestamp = now_str
            rec.updated_at = now_ts
            rec.turns += 1
            if model:
                rec.model = model
            if mode:
                rec.mode = mode
            if effort:
                rec.effort = effort
            if prompt_snippet:
                rec.last_prompt = prompt_snippet
        else:
            rec = SessionRecord(
                conversation_id=conversation_id,
                timestamp=now_str,
                model=model or "default",
                mode=mode or "code",
                effort=effort or "medium",
                last_prompt=prompt_snippet,
                turns=1,
                updated_at=now_ts,
            )
            self._sessions[conversation_id] = rec

        self._save()
        return rec

    def list_sessions(self, limit: int = 20) -> list[SessionRecord]:
        records = list(self._sessions.values())
        records.sort(key=lambda x: (x.updated_at, x.timestamp), reverse=True)
        return records[:limit]

    def get_session(self, query: str) -> SessionRecord | None:
        query = query.strip()
        if not query:
            return None
        if query in self._sessions:
            return self._sessions[query]
        if query.isdigit():
            idx = int(query) - 1
            sorted_list = self.list_sessions()
            if 0 <= idx < len(sorted_list):
                return sorted_list[idx]
        for cid, rec in self._sessions.items():
            if cid.startswith(query):
                return rec
        return None

    def get(self, conversation_id: str) -> SessionRecord | None:
        return self._sessions.get(conversation_id)

    def clear(self) -> None:
        self._sessions.clear()
        if os.path.exists(self.storage_path):
            try:
                os.remove(self.storage_path)
            except Exception:
                pass
