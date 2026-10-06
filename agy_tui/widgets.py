import os
import sys

from rich.panel import Panel
from rich.syntax import Syntax
from rich.text import Text
from textual import events
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.reactive import reactive
from textual.screen import ModalScreen
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
        css_class = "message-user" if self.role == "user" else "message-assistant"
        self.add_class(css_class)
        yield Static(self.content_text, classes="message-body")


class ThoughtCard(Static):
    collapsed: reactive[bool] = reactive(False)

    def __init__(self, thought_text: str, is_collapsed: bool = False, **kwargs) -> None:
        super().__init__(**kwargs)
        self.thought_text = thought_text
        self.collapsed = is_collapsed

    def compose(self):
        body_text = "[dim #cba6f7]▣ Thought (collapsed)[/dim #cba6f7]" if self.collapsed else f"[dim #cba6f7]▣ Thought[/dim #cba6f7]\n[dim italic]{self.thought_text}[/dim italic]"
        yield Static(body_text, id="thought-body", classes="thought-content")

    def toggle(self) -> None:
        self.collapsed = not self.collapsed
        body = self.query_one("#thought-body", Static)
        if self.collapsed:
            body.update("[dim #cba6f7]▣ Thought (collapsed)[/dim #cba6f7]")
        else:
            body.update(f"[dim #cba6f7]▣ Thought[/dim #cba6f7]\n[dim italic]{self.thought_text}[/dim italic]")


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


def format_workspace_path(target_path: str = "") -> str:
    target = target_path or os.getcwd()
    home = os.path.expanduser("~")
    if target == home:
        return "~"
    if target.startswith(home + "/"):
        return "~/" + os.path.relpath(target, home)
    return target


def format_compact_tokens(tokens: int, limit: int = 200000) -> str:
    pct = int((tokens / max(1, limit)) * 100)
    if tokens >= 1_000_000:
        val = tokens / 1_000_000
        tok_str = f"{val:.1f}M".replace(".0M", "M")
    elif tokens >= 1_000:
        val = tokens / 1_000
        tok_str = f"{val:.1f}K".replace(".0K", "K")
    else:
        tok_str = str(tokens)
    return f"{tok_str} ({pct}%)"


class SidebarInspector(Vertical):
    session_title: reactive[str] = reactive("Nova sessão")
    tokens_used: reactive[int] = reactive(0)
    context_limit: reactive[int] = reactive(200000)
    workspace_path: reactive[str] = reactive("")

    agent_status: reactive[str] = reactive("Idle")
    model_name: reactive[str] = reactive("gemini-3.8-flash-high")
    mode_name: reactive[str] = reactive("code")
    effort_level: reactive[str] = reactive("medium")
    tool_steps_count: reactive[int] = reactive(0)
    subagents_count: reactive[int] = reactive(0)
    conversation_id: reactive[str] = reactive("")

    def compose(self):
        with Vertical(id="sidebar-container"):
            with Vertical(id="sidebar-main"):
                yield Static(self.session_title, id="sidebar-session-title", classes="sidebar-session-title")
                with Vertical(id="sidebar-context-group"):
                    yield Static("Context", classes="sidebar-context-title")
                    yield Static(self._format_token_count(), id="sidebar-tokens-count", classes="sidebar-context-tokens")
                    yield Static(self._format_token_percent(), id="sidebar-tokens-pct", classes="sidebar-context-percent")
            with Vertical(id="sidebar-footer"):
                yield Static(self._get_workspace_path(), id="sidebar-workspace-path", classes="sidebar-workspace-text")

        with Vertical(id="sidebar-compat", classes="sidebar-compat-hidden"):
            yield Static("● Idle", id="status-indicator")
            yield Static(self.model_name, id="model-indicator")
            yield Static(f"Mode: {self.mode_name} | Effort: {self.effort_level}", id="profile-indicator")
            yield Static(self._build_token_meter(), id="token-meter")
            yield Static("Steps: 0", id="steps-indicator")
            yield Static("None recorded yet", id="steps-list")
            yield Static("Active: 0", id="subagents-indicator")
            yield Static("None", id="subagents-list")
            disp = (self.conversation_id[:18] + "...") if len(self.conversation_id) > 18 else (self.conversation_id or "default")
            yield Static(disp, id="session-indicator")

    def _format_token_count(self) -> str:
        return f"{self.tokens_used:,} tokens"

    def _format_token_percent(self) -> str:
        pct = (self.tokens_used / max(1, self.context_limit)) * 100
        return f"{int(pct)}% used"

    def _get_workspace_path(self) -> str:
        return format_workspace_path(self.workspace_path)

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

    def watch_tokens_used(self, new_val: int) -> None:
        try:
            self.query_one("#sidebar-tokens-count", Static).update(self._format_token_count())
            self.query_one("#sidebar-tokens-pct", Static).update(self._format_token_percent())
            self.query_one("#token-meter", Static).update(self._build_token_meter())
        except Exception:
            pass

    def watch_context_limit(self, new_val: int) -> None:
        try:
            self.query_one("#sidebar-tokens-pct", Static).update(self._format_token_percent())
            self.query_one("#token-meter", Static).update(self._build_token_meter())
        except Exception:
            pass

    def watch_session_title(self, new_val: str) -> None:
        try:
            self.query_one("#sidebar-session-title", Static).update(new_val)
        except Exception:
            pass

    def watch_workspace_path(self, new_val: str) -> None:
        try:
            self.query_one("#sidebar-workspace-path", Static).update(self._get_workspace_path())
        except Exception:
            pass

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
        session_title: str | None = None,
        workspace_path: str | None = None,
    ) -> None:
        if session_title:
            self.session_title = session_title

        if workspace_path:
            self.workspace_path = workspace_path

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


class ChatInputSubBar(Horizontal):
    mode_name: reactive[str] = reactive("code")
    model_name: reactive[str] = reactive("gemini-3.8-flash-high")
    effort_level: reactive[str] = reactive("medium")

    def compose(self) -> ComposeResult:
        yield Static(self._build_text(), id="subbar-text")

    def _build_text(self) -> str:
        mode = (self.mode_name or "code").capitalize()
        model = self.model_name or "gemini-3.8-flash-high"
        effort = (self.effort_level or "medium").capitalize()
        return f"[bold #89b4fa]{mode}[/] [dim #6c7086]·[/] [#cdd6f4]{model}[/] [dim #a6adc8]{effort}[/]"

    def get_plain_text(self) -> str:
        mode = (self.mode_name or "code").capitalize()
        model = self.model_name or "gemini-3.8-flash-high"
        effort = (self.effort_level or "medium").capitalize()
        return f"{mode} · {model} {effort}"

    def watch_mode_name(self, new_val: str) -> None:
        try:
            self.query_one("#subbar-text", Static).update(self._build_text())
        except Exception:
            pass

    def watch_model_name(self, new_val: str) -> None:
        try:
            self.query_one("#subbar-text", Static).update(self._build_text())
        except Exception:
            pass

    def watch_effort_level(self, new_val: str) -> None:
        try:
            self.query_one("#subbar-text", Static).update(self._build_text())
        except Exception:
            pass

    def update_info(
        self,
        mode: str | None = None,
        model: str | None = None,
        effort: str | None = None,
    ) -> None:
        if mode:
            self.mode_name = mode
        if model:
            self.model_name = model
        if effort:
            self.effort_level = effort


class ChatFooterBar(Horizontal):
    workspace_path: reactive[str] = reactive("")
    tokens_used: reactive[int] = reactive(0)
    context_limit: reactive[int] = reactive(200000)
    shortcuts_text: reactive[str] = reactive("tab /help · ctrl+b sidebar")

    def compose(self) -> ComposeResult:
        yield Static(self._get_workspace_path(), id="footer-workspace-path", classes="footer-path")
        yield Static(self._build_context_and_shortcuts(), id="footer-context-shortcuts", classes="footer-info")

    def _get_workspace_path(self) -> str:
        return format_workspace_path(self.workspace_path)

    def _format_compact_tokens(self) -> str:
        return format_compact_tokens(self.tokens_used, self.context_limit)

    def _build_context_and_shortcuts(self) -> str:
        tok_summary = self._format_compact_tokens()
        return f"[dim #888888]{tok_summary}[/]  [dim #585b70]{self.shortcuts_text}[/]"

    def get_plain_context(self) -> str:
        return self._format_compact_tokens()

    def watch_workspace_path(self, new_val: str) -> None:
        try:
            self.query_one("#footer-workspace-path", Static).update(self._get_workspace_path())
        except Exception:
            pass

    def watch_tokens_used(self, new_val: int) -> None:
        try:
            self.query_one("#footer-context-shortcuts", Static).update(self._build_context_and_shortcuts())
        except Exception:
            pass

    def watch_context_limit(self, new_val: int) -> None:
        try:
            self.query_one("#footer-context-shortcuts", Static).update(self._build_context_and_shortcuts())
        except Exception:
            pass

    def watch_shortcuts_text(self, new_val: str) -> None:
        try:
            self.query_one("#footer-context-shortcuts", Static).update(self._build_context_and_shortcuts())
        except Exception:
            pass

    def update_telemetry(
        self,
        tokens: int | None = None,
        limit: int | None = None,
        workspace: str | None = None,
    ) -> None:
        if tokens is not None:
            self.tokens_used = tokens
        if limit is not None:
            self.context_limit = limit
        if workspace:
            self.workspace_path = workspace


class ChatInputContainer(Vertical):
    mode_name: reactive[str] = reactive("code")
    model_name: reactive[str] = reactive("gemini-3.8-flash-high")
    effort_level: reactive[str] = reactive("medium")

    def compose(self) -> ComposeResult:
        yield ChatInput(id="chat-input")
        subbar = ChatInputSubBar(id="chat-input-subbar")
        subbar.mode_name = self.mode_name
        subbar.model_name = self.model_name
        subbar.effort_level = self.effort_level
        yield subbar

    def watch_mode_name(self, new_val: str) -> None:
        try:
            self.query_one("#chat-input-subbar", ChatInputSubBar).mode_name = new_val
        except Exception:
            pass

    def watch_model_name(self, new_val: str) -> None:
        try:
            self.query_one("#chat-input-subbar", ChatInputSubBar).model_name = new_val
        except Exception:
            pass

    def watch_effort_level(self, new_val: str) -> None:
        try:
            self.query_one("#chat-input-subbar", ChatInputSubBar).effort_level = new_val
        except Exception:
            pass


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


class ModelSelectModal(ModalScreen[tuple[str, str] | None]):
    BINDINGS = [
        Binding("escape", "cancel", "Cancel"),
    ]

    MODELS = [
        ("gemini-3.8-flash", "Gemini 3.8 Flash"),
        ("gemini-3.7-flash", "Gemini 3.7 Flash"),
        ("gemini-3.6-flash", "Gemini 3.6 Flash"),
        ("gemini-3.1-pro", "Gemini 3.1 Pro"),
        ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
        ("claude-opus-5-5", "Claude Opus 5.5"),
        ("gpt-oss-120b-medium", "GPT-OSS 120B"),
    ]

    EFFORTS = [
        ("high", "High", "Recommended"),
        ("medium", "Medium", "Balanced"),
        ("low", "Low", "Fast response"),
    ]

    def __init__(
        self,
        current_model: str | None = None,
        current_effort: str | None = None,
        **kwargs,
    ) -> None:
        super().__init__(**kwargs)
        self.current_model = current_model or "gemini-3.8-flash"
        self.current_effort = current_effort or "medium"
        self._selected_model: str | None = None
        self._step: int = 1

    def compose(self) -> ComposeResult:
        with Vertical(id="model-modal-container"):
            yield Static("Select model", id="model-modal-title")
            yield Static("Step 1/2", id="model-modal-step")
            yield OptionList(id="model-modal-options")
            yield Static("esc cancel · enter select", id="model-modal-footer")

    def on_mount(self) -> None:
        self._populate_models()

    def _populate_models(self) -> None:
        self._step = 1
        self._selected_model = None
        try:
            self.query_one("#model-modal-step", Static).update("Step 1/2 · Model")
            opt_list = self.query_one("#model-modal-options", OptionList)
            opt_list.clear_options()
            initial_idx = 0
            for idx, (model_id, name) in enumerate(self.MODELS):
                is_active = model_id in self.current_model.lower()
                prefix = "● " if is_active else "  "
                opt_list.add_option(f"{prefix}{name:<20} [dim]{model_id}[/]")
                if is_active:
                    initial_idx = idx
            opt_list.highlighted = initial_idx
            opt_list.focus()
        except Exception:
            pass

    def _populate_efforts(self, chosen_name: str) -> None:
        self._step = 2
        try:
            self.query_one("#model-modal-step", Static).update(f"Step 2/2 · Effort for {chosen_name}")
            opt_list = self.query_one("#model-modal-options", OptionList)
            opt_list.clear_options()
            initial_idx = 0
            for idx, (eff_id, name, hint) in enumerate(self.EFFORTS):
                is_active = eff_id == self.current_effort.lower()
                prefix = "● " if is_active else "  "
                opt_list.add_option(f"{prefix}{name:<10} [dim]({hint})[/]")
                if is_active:
                    initial_idx = idx
            opt_list.highlighted = initial_idx
            opt_list.focus()
        except Exception:
            pass

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        if self._step == 1:
            idx = event.option_index
            if 0 <= idx < len(self.MODELS):
                model_id, display_name = self.MODELS[idx]
                self._selected_model = model_id
                self._populate_efforts(display_name)
        elif self._step == 2:
            idx = event.option_index
            if 0 <= idx < len(self.EFFORTS):
                eff_id, _, _ = self.EFFORTS[idx]
                model = self._selected_model or self.current_model
                self.dismiss((model, eff_id))

    def action_cancel(self) -> None:
        if self._step == 2:
            self._populate_models()
        else:
            self.dismiss(None)

