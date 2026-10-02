import difflib
import time
from dataclasses import dataclass
from typing import Any

from rich.text import Text


@dataclass
class DiffRecord:
    file_path: str
    tool_name: str
    diff_text: str
    timestamp: str


def format_diff_from_tool(
    tool_name: str,
    args: dict[str, Any] | None = None,
    raw_diff: str | None = None,
) -> DiffRecord | None:
    now_str = time.strftime("%H:%M:%S")
    if raw_diff and raw_diff.strip():
        file_path = "modified_file"
        if args and isinstance(args, dict):
            file_path = args.get("TargetFile") or args.get("file_path") or args.get("path") or args.get("file") or args.get("filename") or args.get("filepath") or file_path
        return DiffRecord(
            file_path=str(file_path),
            tool_name=tool_name,
            diff_text=raw_diff.strip(),
            timestamp=now_str,
        )

    if not args or not isinstance(args, dict):
        return None

    file_path = args.get("TargetFile") or args.get("file_path") or args.get("path") or args.get("file") or args.get("filename") or args.get("filepath") or "file"

    if "TargetContent" in args and "ReplacementContent" in args:
        target = str(args.get("TargetContent", ""))
        replacement = str(args.get("ReplacementContent", ""))
        target_lines = target.splitlines(keepends=True)
        replacement_lines = replacement.splitlines(keepends=True)
        diff_lines = list(
            difflib.unified_diff(
                target_lines,
                replacement_lines,
                fromfile=f"a/{file_path}",
                tofile=f"b/{file_path}",
                n=3,
            )
        )
        if not diff_lines:
            diff_lines = [
                f"--- a/{file_path}\n",
                f"+++ b/{file_path}\n",
                "@@ -1 +1 @@\n",
            ]
            for line in target_lines:
                diff_lines.append(f"-{line}")
            for line in replacement_lines:
                diff_lines.append(f"+{line}")
        diff_text = "".join(diff_lines)
        return DiffRecord(
            file_path=str(file_path),
            tool_name=tool_name,
            diff_text=diff_text,
            timestamp=now_str,
        )

    code_content = args.get("CodeContent") or args.get("content") or args.get("text")
    if code_content is not None:
        code = str(code_content)
        code_lines = code.splitlines(keepends=True)
        diff_lines = [
            "--- /dev/null\n",
            f"+++ b/{file_path}\n",
            f"@@ -0,0 +1,{len(code_lines)} @@\n",
        ]
        for line in code_lines:
            diff_lines.append(f"+{line}")
        diff_text = "".join(diff_lines)
        return DiffRecord(
            file_path=str(file_path),
            tool_name=tool_name,
            diff_text=diff_text,
            timestamp=now_str,
        )

    return None


def format_diff_text(diff_str: str) -> Text:
    result = Text()
    lines = diff_str.splitlines()
    for idx, line in enumerate(lines):
        if line.startswith("---") or line.startswith("+++"):
            result.append(line, style="bold cyan")
        elif line.startswith("@@"):
            result.append(line, style="bold magenta")
        elif line.startswith("+"):
            result.append(line, style="bold green")
        elif line.startswith("-"):
            result.append(line, style="bold red")
        else:
            result.append(line, style="dim")
        if idx < len(lines) - 1:
            result.append("\n")
    return result


class DiffRegistry:
    def __init__(self) -> None:
        self._diffs: list[DiffRecord] = []

    def add(self, record: DiffRecord) -> bool:
        if self._diffs:
            last = self._diffs[-1]
            if last.file_path == record.file_path and last.diff_text == record.diff_text:
                return False
        self._diffs.append(record)
        return True

    def get_latest(self) -> DiffRecord | None:
        if self._diffs:
            return self._diffs[-1]
        return None

    def get_all(self) -> list[DiffRecord]:
        return list(self._diffs)

    def clear(self) -> None:
        self._diffs.clear()
