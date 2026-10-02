import os
import sys

from rich.panel import Panel
from rich.syntax import Syntax
from rich.text import Text
from textual import events
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.reactive import reactive
from textual.widgets import Button, OptionList, Static, TextArea

from agy_tui.diff import DiffRecord, format_diff_text


class CodeBlockWidget(Static):
    def __init__(self, block_id: int, language: str, code: str, **kwargs) -> None:
        super().__init__(**kwargs)
        self.block_id = block_id
        self.language = language or "text"
        self.code = code
        self.copy_btn: Button | None = None

    def compose(self):
        with Horizontal(classes="code-header"):
            yield Static(f"Code Block #{self.block_id} ({self.language})", classes="code-title")
            btn = Button("📋 Copy", id=f"copy-btn-{self.block_id}", classes="copy-btn")
            self.copy_btn = btn
            yield btn
        syntax = Syntax(
            self.code,
            self.language,
            theme="monokai",
            line_numbers=True,
            word_wrap=True,
            padding=(0, 1),
        )
        yield Static(syntax, classes="code-content")


class DiffViewWidget(Static):
    def __init__(self, record: DiffRecord, **kwargs) -> None:
        super().__init__(**kwargs)
        self.record = record

    def compose(self):
        with Horizontal(classes="diff-header"):
            yield Static(f"Diff: {self.record.file_path} ({self.record.tool_name})", classes="diff-title")
            yield Static(self.record.timestamp, classes="diff-time")
        yield Static(format_diff_text(self.record.diff_text), classes="diff-content")


class MessageCard(Static):
    def __init__(self, role: str, content: str, **kwargs) -> None:
        super().__init__(**kwargs)
        self.role = role
        self.content_text = content

    def compose(self):
        role_label = "USER ❯" if self.role == "user" else "ASSISTANT ❯"
        role_class = "role-user" if self.role == "user" else "role-assistant"
        yield Static(role_label, classes=f"message-role {role_class}")
        yield Static(self.content_text, classes="message-body")


class ThoughtCard(Static):
    collapsed: reactive[bool] = reactive(False)

    def __init__(self, thought_text: str, is_collapsed: bool = False, **kwargs) -> None:
        super().__init__(**kwargs)
        self.thought_text = thought_text
        self.collapsed = is_collapsed

    def compose(self):
        with Horizontal(classes="thought-header"):
            yield Static("Thinking / Reasoning", classes="thought-title")
            yield Button("Collapse" if not self.collapsed else "Expand", id="toggle-thought-btn", classes="thought-btn")
        body_text = "[thought collapsed]" if self.collapsed else self.thought_text
        yield Static(body_text, id="thought-body", classes="thought-content")

    def toggle(self) -> None:
        self.collapsed = not self.collapsed
        body = self.query_one("#thought-body", Static)
        btn = self.query_one("#toggle-thought-btn", Button)
        if self.collapsed:
            body.update("[thought collapsed]")
            btn.label = "Expand"
        else:
            body.update(self.thought_text)
            btn.label = "Collapse"


class StepCard(Static):
    def __init__(
        self,
        step_type: str,
        state: str,
        step_index: int | None = None,
        duration: float | None = None,
        **kwargs,
    ) -> None:
        super().__init__(**kwargs)
        self.step_type = step_type
        self.state = state
        self.step_index = step_index
        self.duration = duration

    def compose(self):
        idx_str = f" #{self.step_index}" if self.step_index is not None else ""
        dur_str = f" ({self.duration:.2f}s)" if self.duration is not None else ""
        state_color = "green" if self.state == "DONE" else "yellow"
        text = Text.assemble(
            ("Tool Step", "bold yellow"),
            (f"{idx_str}: ", "yellow"),
            (self.step_type, "bold"),
            (" -> ", "dim"),
            (f"[{self.state}]", f"bold {state_color}"),
            (dur_str, "dim"),
        )
        yield Static(Panel(text, border_style="yellow", expand=False), classes="step-panel")


class SidebarInspector(Vertical):
    agent_status: reactive[str] = reactive("Idle")
    model_name: reactive[str] = reactive("gemini-3.8-flash-high")
    mode_name: reactive[str] = reactive("code")
    effort_level: reactive[str] = reactive("medium")
    tokens_used: reactive[int] = reactive(0)
    context_limit: reactive[int] = reactive(200000)
    tool_steps_count: reactive[int] = reactive(0)
    subagents_count: reactive[int] = reactive(0)
    conversation_id: reactive[str] = reactive("")

    def compose(self):
        yield Static("SIDEBAR INSPECTOR", classes="sidebar-header")

        with Vertical(classes="sidebar-section"):
            yield Static("Agent Status", classes="sidebar-label")
            yield Static("● Idle", id="status-indicator", classes="sidebar-status-idle")

        with Vertical(classes="sidebar-section"):
            yield Static("Active Model", classes="sidebar-label")
            yield Static(self.model_name, id="model-indicator", classes="sidebar-value")

        with Vertical(classes="sidebar-section"):
            yield Static("Execution Profile", classes="sidebar-label")
            yield Static(f"Mode: {self.mode_name} | Effort: {self.effort_level}", id="profile-indicator", classes="sidebar-value")

        with Vertical(classes="sidebar-section"):
            yield Static("Context & Tokens", classes="sidebar-label")
            yield Static(self._build_token_meter(), id="token-meter", classes="sidebar-meter")

        with Vertical(classes="sidebar-section"):
            yield Static("Tool Steps", classes="sidebar-label")
            yield Static("Steps: 0", id="steps-indicator", classes="sidebar-value")
            yield Static("None recorded yet", id="steps-list", classes="sidebar-subtext")

        with Vertical(classes="sidebar-section"):
            yield Static("Subagents", classes="sidebar-label")
            yield Static("Active: 0", id="subagents-indicator", classes="sidebar-value")
            yield Static("None", id="subagents-list", classes="sidebar-subtext")

        with Vertical(classes="sidebar-section"):
            yield Static("Session ID", classes="sidebar-label")
            disp = (self.conversation_id[:18] + "...") if len(self.conversation_id) > 18 else (self.conversation_id or "default")
            yield Static(disp, id="session-indicator", classes="sidebar-session-text")

    def _build_token_meter(self, ascii_fallback: bool = False) -> str:
        pct = min(100, int((self.tokens_used / max(1, self.context_limit)) * 100))
        bars = int(pct / 10)
        use_ascii = ascii_fallback or os.environ.get("AGY_ASCII", "").lower() in ("1", "true", "yes")
        if not use_ascii:
            try:
                "█░".encode(sys.stdout.encoding or "utf-8")
            except Exception:
                use_ascii = True
        fill_char = "#" if use_ascii else "█"
        empty_char = "-" if use_ascii else "░"
        meter = fill_char * bars + empty_char * (10 - bars)
        return f"[{meter}] {pct}%\n{self.tokens_used:,} / {self.context_limit:,} tokens"

    def update_telemetry(
        self,
        status: str | None = None,
        model: str | None = None,
        mode: str | None = None,
        effort: str | None = None,
        tokens: int | None = None,
        tool_step: str | None = None,
        subagents: list[str] | int | None = None,
        conversation_id: str | None = None,
        context_limit: int | None = None,
    ) -> None:
        if status:
            self.agent_status = status
            try:
                ind = self.query_one("#status-indicator", Static)
                style_class = "sidebar-status-active" if "Running" in status or "Thinking" in status else "sidebar-status-idle"
                ind.remove_class("sidebar-status-idle", "sidebar-status-active")
                ind.add_class(style_class)
                prefix = "● " if not status.startswith("●") else ""
                ind.update(f"{prefix}{status}")
            except Exception:
                pass

        if model:
            self.model_name = model
            if "gemini" in model.lower():
                self.context_limit = 1000000
            elif "claude" in model.lower():
                self.context_limit = 200000
            elif "gpt" in model.lower():
                self.context_limit = 128000
            try:
                self.query_one("#model-indicator", Static).update(model)
                self.query_one("#token-meter", Static).update(self._build_token_meter())
            except Exception:
                pass

        if context_limit is not None:
            self.context_limit = context_limit
            try:
                self.query_one("#token-meter", Static).update(self._build_token_meter())
            except Exception:
                pass

        if mode or effort:
            if mode:
                self.mode_name = mode
            if effort:
                self.effort_level = effort
            try:
                self.query_one("#profile-indicator", Static).update(f"Mode: {self.mode_name} | Effort: {self.effort_level}")
            except Exception:
                pass

        if tokens is not None:
            self.tokens_used = tokens
            try:
                self.query_one("#token-meter", Static).update(self._build_token_meter())
            except Exception:
                pass

        if tool_step:
            self.tool_steps_count += 1
            try:
                self.query_one("#steps-indicator", Static).update(f"Steps: {self.tool_steps_count}")
                self.query_one("#steps-list", Static).update(tool_step[:26])
            except Exception:
                pass

        if subagents is not None:
            if isinstance(subagents, list):
                self.subagents_count = len(subagents)
                info = f"Active: {len(subagents)}"
                sub_detail = ", ".join(subagents)[:26] if subagents else "None"
            else:
                self.subagents_count = int(subagents)
                info = f"Active: {subagents}"
                sub_detail = "None" if self.subagents_count == 0 else f"{subagents} workers"
            try:
                self.query_one("#subagents-indicator", Static).update(info)
                self.query_one("#subagents-list", Static).update(sub_detail)
            except Exception:
                pass

        if conversation_id:
            self.conversation_id = conversation_id
            try:
                disp = conversation_id[:18] + "..." if len(conversation_id) > 18 else conversation_id
                self.query_one("#session-indicator", Static).update(disp)
            except Exception:
                pass


class ChatInput(TextArea):
    class Submitted(Message):
        def __init__(self, value: str) -> None:
            super().__init__()
            self.value = value

    class AutocompleteRequested(Message):
        def __init__(self, prefix: str) -> None:
            super().__init__()
            self.prefix = prefix

    class HistoryNavigation(Message):
        def __init__(self, direction: str) -> None:
            super().__init__()
            self.direction = direction

    def __init__(self, **kwargs) -> None:
        super().__init__(**kwargs)
        self.multiline_submit_ctrl_enter: bool = False

    def _on_key(self, event: events.Key) -> None:
        try:
            popup = self.screen.query_one("#autocomplete-popup", AutocompletePopup)
            if popup and not popup.has_class("hidden") and popup.option_count > 0:
                if event.key == "down":
                    popup.highlighted = 0 if popup.highlighted is None else min(popup.highlighted + 1, popup.option_count - 1)
                    event.prevent_default()
                    event.stop()
                    return
                if event.key == "up":
                    popup.highlighted = (popup.option_count - 1) if (popup.highlighted is None or popup.highlighted == 0) else max(popup.highlighted - 1, 0)
                    event.prevent_default()
                    event.stop()
                    return
                if event.key in ("tab", "enter") and (popup.highlighted is not None or event.key == "tab"):
                    if popup.highlighted is None:
                        popup.highlighted = 0
                    selected = popup.get_selected_command()
                    if selected:
                        self.text = selected + " "
                        self.cursor_location = (0, len(self.text))
                        popup.add_class("hidden")
                        popup.highlighted = None
                        event.prevent_default()
                        event.stop()
                        return
                if event.key == "escape":
                    popup.add_class("hidden")
                    popup.highlighted = None
                    event.prevent_default()
                    event.stop()
                    return
        except Exception:
            pass

        if event.key in ("pageup",):
            event.prevent_default()
            event.stop()
            try:
                self.app.action_scroll_page_up()
            except Exception:
                pass
            return

        if event.key in ("pagedown",):
            event.prevent_default()
            event.stop()
            try:
                self.app.action_scroll_page_down()
            except Exception:
                pass
            return

        if event.key in ("ctrl+enter",) or (event.key in ("ctrl+j", "newline") and self.multiline_submit_ctrl_enter):
            event.prevent_default()
            event.stop()
            text = self.text.strip()
            if text:
                self.post_message(self.Submitted(text))
                self.text = ""
                self.styles.height = 3
            return

        if event.key in ("shift+enter", "ctrl+j", "newline"):
            self.insert("\n")
            event.prevent_default()
            event.stop()
            return

        if event.key == "enter":
            if self.multiline_submit_ctrl_enter:
                self.insert("\n")
                event.prevent_default()
                event.stop()
                return
            event.prevent_default()
            event.stop()
            text = self.text.strip()
            if text:
                self.post_message(self.Submitted(text))
                self.text = ""
                self.styles.height = 3
            return

        if event.key == "up" and self.cursor_location[0] == 0:
            event.prevent_default()
            event.stop()
            self.post_message(self.HistoryNavigation("up"))
            return

        if event.key == "down" and self.cursor_location[0] >= self.document.line_count - 1:
            event.prevent_default()
            event.stop()
            self.post_message(self.HistoryNavigation("down"))
            return


class AutocompletePopup(OptionList):
    COMMANDS = [
        ("/model", "Switch or view active model"),
        ("/mode", "Set execution mode (code, plan, architect, ask)"),
        ("/effort", "Set reasoning effort (low, medium, high)"),
        ("/diff", "Inspect recent tool diffs and file modifications"),
        ("/resume", "List or resume previous sessions"),
        ("/c", "Copy code block by ID to clipboard (/c <id>)"),
        ("/copy", "Copy code block by ID to clipboard"),
        ("/ca", "Copy entire assistant response to clipboard"),
        ("/copyall", "Copy entire assistant response to clipboard"),
        ("/multi", "Toggle multiline input mode"),
        ("/send", "Submit multiline input buffer"),
        ("/thoughts", "Toggle reasoning thoughts display"),
        ("/expand", "Expand thoughts display"),
        ("/sidebar", "Toggle sidebar inspector (Ctrl+B)"),
        ("/history", "Display recent command history"),
        ("/clear", "Clear chat history viewport"),
        ("/help", "Show command assistance and keybindings"),
        ("/quit", "Gracefully terminate application"),
    ]

    def populate(self, query: str = "") -> int:
        self.clear_options()
        count = 0
        q = query.lower().strip()
        for cmd, desc in self.COMMANDS:
            if not q or cmd.startswith(q) or q in cmd:
                self.add_option(f"{cmd:<10}  {desc}")
                count += 1
        return count

    def get_selected_command(self, index: int | None = None) -> str | None:
        idx = index if index is not None else self.highlighted
        if idx is not None and 0 <= idx < self.option_count:
            opt = self.get_option_at_index(idx)
            raw = str(opt.prompt)
            return raw.split()[0]
        return None
