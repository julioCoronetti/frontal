class InputHistory:
    def __init__(self, max_size: int = 1000) -> None:
        self.max_size = max_size
        self._entries: list[str] = []
        self._cursor: int = 0
        self._draft: str = ""

    def add(self, text: str) -> None:
        text = text.strip()
        if not text:
            return
        if self._entries and self._entries[-1] == text:
            self._cursor = len(self._entries)
            return
        self._entries.append(text)
        if len(self._entries) > self.max_size:
            self._entries.pop(0)
        self._cursor = len(self._entries)

    def previous(self, current_buffer: str) -> str:
        if not self._entries:
            return current_buffer
        if self._cursor == len(self._entries):
            self._draft = current_buffer
        if self._cursor > 0:
            self._cursor -= 1
            return self._entries[self._cursor]
        return self._entries[0]

    def next(self) -> str:
        if not self._entries:
            return ""
        if self._cursor < len(self._entries) - 1:
            self._cursor += 1
            return self._entries[self._cursor]
        self._cursor = len(self._entries)
        return self._draft

    def reset_cursor(self) -> None:
        self._cursor = len(self._entries)
        self._draft = ""

    def get_all(self) -> list[str]:
        return list(self._entries)
