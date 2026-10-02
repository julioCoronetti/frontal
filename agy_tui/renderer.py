import re
from dataclasses import dataclass

from rich.console import Group, RenderableType
from rich.markdown import Markdown
from rich.panel import Panel
from rich.syntax import Syntax
from rich.text import Text


@dataclass
class CodeBlock:
    id: int
    language: str
    code: str


class CodeBlockRegistry:
    def __init__(self) -> None:
        self._blocks: list[CodeBlock] = []

    def clear(self) -> None:
        self._blocks.clear()

    def register(self, language: str, code: str) -> CodeBlock:
        lang = language.strip() if language else "text"
        for existing in self._blocks:
            if existing.code == code and existing.language == lang:
                return existing
        block_id = len(self._blocks) + 1
        block = CodeBlock(id=block_id, language=lang, code=code)
        self._blocks.append(block)
        return block

    def get(self, block_id: int) -> CodeBlock | None:
        if 1 <= block_id <= len(self._blocks):
            return self._blocks[block_id - 1]
        return None

    def all_blocks(self) -> list[CodeBlock]:
        return list(self._blocks)


class MarkdownRenderer:
    CODE_FENCE_REGEX = re.compile(
        r"(?:^|(?<=\n))[ \t]{0,3}(`{3,}|~{3,})[ \t]*([^\r\n`~]*)\r?\n(.*?)(?<=\n)[ \t]{0,3}\1[ \t]*(?=\r?\n|$)",
        re.DOTALL,
    )

    def __init__(self, code_registry: CodeBlockRegistry | None = None) -> None:
        self.code_registry = code_registry or CodeBlockRegistry()

    @staticmethod
    def _parse_lang(info_string: str) -> str:
        info = info_string.strip()
        if not info:
            return "text"
        token = info.split()[0]
        lang = token.split(":")[0].strip()
        return lang or "text"

    def extract_and_register_code_blocks(self, text: str) -> list[CodeBlock]:
        blocks = []
        for match in self.CODE_FENCE_REGEX.finditer(text):
            lang = self._parse_lang(match.group(2))
            code = match.group(3)
            block = self.code_registry.register(lang, code)
            blocks.append(block)
        return blocks

    def render_markdown_with_code_blocks(self, text: str) -> RenderableType:
        renderables: list[RenderableType] = []
        last_idx = 0

        for match in self.CODE_FENCE_REGEX.finditer(text):
            start, end = match.span()
            if start > last_idx:
                md_part = text[last_idx:start].strip()
                if md_part:
                    renderables.append(Markdown(md_part))

            lang = self._parse_lang(match.group(2))
            code = match.group(3)
            block = self.code_registry.register(lang, code)

            syntax = Syntax(
                code,
                lang,
                theme="monokai",
                line_numbers=True,
                word_wrap=True,
                padding=(0, 1),
            )
            title = f"[bold cyan]Code Block #{block.id}[/bold cyan] [dim]({lang})[/dim]"
            subtitle = f"[dim]Copy: /c {block.id}[/dim]"
            panel = Panel(
                syntax,
                title=title,
                subtitle=subtitle,
                border_style="cyan",
                expand=False,
            )
            renderables.append(panel)
            last_idx = end

        if last_idx < len(text):
            trailing_part = text[last_idx:].strip()
            if trailing_part:
                renderables.append(Markdown(trailing_part))

        if not renderables:
            return Text("")
        if len(renderables) == 1:
            return renderables[0]
        return Group(*renderables)

    def render_thought(self, thought_text: str, is_collapsed: bool = False) -> Panel:
        if is_collapsed:
            content = Text("[thought collapsed - type /thoughts or /expand to view]", style="dim italic")
        else:
            content = Text(thought_text, style="dim italic cyan")
        return Panel(
            content,
            title="[dim cyan]Thought / Reasoning[/dim cyan]",
            border_style="dim cyan",
            expand=False,
        )

    def render_step(
        self,
        step_type: str,
        state: str,
        step_index: int | None = None,
        duration: float | None = None,
    ) -> Panel:
        idx_str = f" #{step_index}" if step_index is not None else ""
        dur_str = f" ({duration:.2f}s)" if duration is not None else ""
        state_color = "green" if state == "DONE" else "yellow"
        text = Text.assemble(
            ("Step", "bold yellow"),
            (f"{idx_str}: ", "yellow"),
            (step_type, "bold"),
            (" -> ", "dim"),
            (f"[{state}]", f"bold {state_color}"),
            (dur_str, "dim"),
        )
        return Panel(
            text,
            title="[yellow]Tool / Step Update[/yellow]",
            border_style="yellow",
            expand=False,
        )

    def render_result(
        self,
        status: str,
        duration: float | None = None,
        error_message: str | None = None,
    ) -> Panel:
        dur_str = f" in {duration:.2f}s" if duration is not None else ""
        if status == "SUCCESS":
            text = Text.assemble(
                ("Completed successfully", "bold green"),
                (dur_str, "dim"),
            )
            return Panel(text, title="[green]Turn Finished[/green]", border_style="green", expand=False)
        else:
            msg = error_message or "Execution failed"
            text = Text.assemble(
                (f"Turn error: {msg}", "bold red"),
                (dur_str, "dim"),
            )
            return Panel(text, title="[red]Error[/red]", border_style="red", expand=False)

    def render_diff(self, file_path: str, diff_text: str, tool_name: str | None = None) -> Panel:
        from agy_tui.diff import format_diff_text
        tool_str = f" ({tool_name})" if tool_name else ""
        title = f"[bold cyan]File Diff: {file_path}[/bold cyan]{tool_str}"
        return Panel(
            format_diff_text(diff_text),
            title=title,
            border_style="cyan",
            expand=False,
        )
