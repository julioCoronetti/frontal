import asyncio
import readline
import sys
import time
from typing import Any

from rich.console import Console
from rich.panel import Panel
from rich.text import Text
from textual import on
from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, OptionList, Static, TextArea

from agy_tui.clipboard import ClipboardManager
from agy_tui.diff import DiffRegistry, format_diff_from_tool
from agy_tui.engine import AgyEngine
from agy_tui.input import InputHistory
from agy_tui.protocol import StreamEvent
from agy_tui.renderer import MarkdownRenderer
from agy_tui.session import SessionManager
from agy_tui.widgets import (
    AutocompletePopup,
    ChatInput,
    CodeBlockWidget,
    DiffViewWidget,
    MessageCard,
    SidebarInspector,
    StepCard,
    ThoughtCard,
)


class TUIApp(App):
    CSS = """
    Screen {
        background: #181825;
        color: #cdd6f4;
    }

    #header-bar {
        dock: top;
        height: 3;
        background: #11111b;
        color: #89b4fa;
        border-bottom: solid #313244;
        padding: 0 1;
    }

    #header-title {
        width: auto;
        text-style: bold;
        color: #89dceb;
    }

    #header-session {
        width: 1fr;
        text-align: right;
        color: #6c7086;
    }

    #body-container {
        height: 1fr;
        width: 100%;
    }

    #messages-view {
        height: 100%;
        width: 1fr;
        overflow-y: scroll;
        padding: 1 2;
    }

    #sidebar {
        width: 32;
        height: 100%;
        background: #141414;
        border-left: solid #222222;
        padding: 1 2;
    }

    #sidebar.collapsed {
        display: none;
    }

    #sidebar-container {
        height: 100%;
    }

    #sidebar-main {
        height: 1fr;
    }

    .sidebar-session-title {
        text-style: bold;
        color: #ffffff;
        margin-bottom: 1;
    }

    #sidebar-context-group {
        height: auto;
    }

    .sidebar-context-title {
        text-style: bold;
        color: #ffffff;
        margin-bottom: 0;
    }

    .sidebar-context-tokens {
        color: #888888;
    }

    .sidebar-context-percent {
        color: #888888;
    }

    #sidebar-footer {
        dock: bottom;
        height: auto;
    }

    .sidebar-workspace-text {
        color: #666666;
    }

    .sidebar-compat-hidden {
        display: none;
    }

    .sidebar-header {
        text-style: bold;
        color: #cba6f7;
        text-align: center;
        border-bottom: solid #313244;
        margin-bottom: 1;
    }

    .sidebar-section {
        height: auto;
        margin-bottom: 1;
        padding: 0 1;
        border: round #313244;
        background: #181825;
    }

    .sidebar-label {
        text-style: bold;
        color: #89b4fa;
    }

    .sidebar-value {
        color: #a6e3a1;
    }

    .sidebar-subtext {
        color: #6c7086;
    }

    .sidebar-session-text {
        color: #f5c2e7;
    }

    .sidebar-meter {
        color: #f9e2af;
    }

    .sidebar-status-idle {
        color: #a6e3a1;
        text-style: bold;
    }

    .sidebar-status-active {
        color: #fab387;
        text-style: bold;
    }

    #footer-area {
        dock: bottom;
        height: auto;
        background: #11111b;
        border-top: solid #313244;
        padding: 0 1;
    }

    #autocomplete-popup {
        height: 7;
        max-height: 8;
        background: #1e1e2e;
        border: solid #89b4fa;
        margin-bottom: 1;
    }

    #autocomplete-popup.hidden {
        display: none;
    }

    #input-bar {
        height: auto;
        min-height: 3;
        max-height: 10;
    }

    #chat-input {
        width: 1fr;
        height: auto;
        min-height: 3;
        max-height: 9;
        background: #181825;
        border: solid #45475a;
    }

    #chat-input:focus {
        border: solid #89b4fa;
    }

    #send-btn {
        width: 10;
        height: 3;
        margin-left: 1;
        background: #89b4fa;
        color: #11111b;
        text-style: bold;
    }

    .code-header {
        height: 3;
        background: #1e1e2e;
        border-top: solid #89b4fa;
        border-left: solid #89b4fa;
        border-right: solid #89b4fa;
        padding: 0 1;
    }

    .code-title {
        width: 1fr;
        text-style: bold;
        color: #89dceb;
    }

    .copy-btn {
        width: 12;
        height: 1;
        min-width: 10;
        background: #313244;
        color: #a6e3a1;
    }

    .copy-btn:hover {
        background: #45475a;
    }

    .code-content {
        border-bottom: solid #89b4fa;
        border-left: solid #89b4fa;
        border-right: solid #89b4fa;
        margin-bottom: 1;
    }

    .diff-header {
        height: 3;
        background: #1e1e2e;
        border-top: solid #fab387;
        border-left: solid #fab387;
        border-right: solid #fab387;
        padding: 0 1;
    }

    .diff-title {
        width: 1fr;
        text-style: bold;
        color: #fab387;
    }

    .diff-time {
        color: #6c7086;
    }

    .diff-content {
        border-bottom: solid #fab387;
        border-left: solid #fab387;
        border-right: solid #fab387;
        margin-bottom: 1;
    }

    .message-role {
        text-style: bold;
        margin-top: 1;
    }

    .role-user {
        color: #89b4fa;
    }

    .role-assistant {
        color: #a6e3a1;
    }

    .message-body {
        margin-bottom: 1;
        padding-left: 1;
    }

    .thought-header {
        height: 3;
        background: #1e1e2e;
        border-top: solid #cba6f7;
        border-left: solid #cba6f7;
        border-right: solid #cba6f7;
        padding: 0 1;
    }

    .thought-title {
        width: 1fr;
        text-style: bold italic;
        color: #cba6f7;
    }

    .thought-btn {
        width: 12;
        height: 1;
        background: #313244;
        color: #cba6f7;
    }

    .thought-content {
        border-bottom: solid #cba6f7;
        border-left: solid #cba6f7;
        border-right: solid #cba6f7;
        color: #bac2de;
        margin-bottom: 1;
        padding: 0 1;
    }

    .step-panel {
        margin-bottom: 1;
    }
    """

    BINDINGS = [
        Binding("ctrl+b", "toggle_sidebar", "Toggle Sidebar", priority=True),
        Binding("ctrl+c", "interrupt_turn", "Interrupt Turn", priority=True),
        Binding("pageup", "scroll_page_up", "Scroll Up", show=False),
        Binding("pagedown", "scroll_page_down", "Scroll Down", show=False),
    ]

    CONTROL_COMMANDS = {
        "/quit",
        "/exit",
        "/q",
        "/help",
        "/clear",
        "/history",
        "/c",
        "/copy",
        "/ca",
        "/copyall",
        "/multi",
        "/send",
        "/thoughts",
        "/expand",
        "/sidebar",
        "/model",
        "/mode",
        "/effort",
        "/diff",
        "/resume",
    }

    AVAILABLE_MODELS = [
        "gemini-3.8-flash-high",
        "gemini-3.8-flash-medium",
        "gemini-3.8-flash-low",
        "gemini-3.7-flash-high",
        "gemini-3.7-flash-medium",
        "gemini-3.7-flash-low",
        "gemini-3.6-flash-high",
        "gemini-3.1-pro-high",
        "claude-sonnet-4-6",
        "claude-opus-4-6-thinking",
        "gpt-oss-120b-medium",
    ]

    AVAILABLE_MODES = ["code", "plan", "architect", "ask", "accept-edits"]
    AVAILABLE_EFFORTS = ["low", "medium", "high", "max"]

    def __init__(
        self,
        engine: AgyEngine | None = None,
        renderer: MarkdownRenderer | None = None,
        clipboard: ClipboardManager | None = None,
        console: Console | None = None,
        **kwargs,
    ) -> None:
        super().__init__(**kwargs)
        self.console = console or Console()
        self.engine = engine or AgyEngine()
        self.renderer = renderer or MarkdownRenderer()
        self._clipboard_manager: ClipboardManager = clipboard or ClipboardManager(output_stream=self.console.file)
        self.history = InputHistory()
        self.session_manager = SessionManager()
        self.diff_registry = DiffRegistry()

        self.running: bool = False
        self.turn_active: bool = False
        self.current_response_text: str = ""
        self.multiline_mode: bool = False
        self.multiline_buffer: list[str] = []
        self._continuation_mode: bool = False
        self.collapse_thoughts: bool = False
        self._segment_start_idx: int = 0
        self._current_thought_text: str = ""
        self._event_task: asyncio.Task | None = None
        self._turn_done_event: asyncio.Event = asyncio.Event()
        self._turn_start_time: float | None = None

        self._active_stream_widget: Static | None = None
        self._active_thought_card: ThoughtCard | None = None
        self._mounted_code_blocks: set[int] = set()

    @property
    def clipboard(self) -> Any:
        return getattr(self, "_clipboard_manager", None) or getattr(self, "_clipboard", "")

    @clipboard.setter
    def clipboard(self, val: Any) -> None:
        if isinstance(val, str):
            self._clipboard = val
        else:
            self._clipboard_manager = val

    def compose(self) -> ComposeResult:
        with Horizontal(id="header-bar"):
            yield Static("Antigravity CLI  |  OpenCode TUI", id="header-title")
            model_info = self.engine.model or "gemini-3.8-flash-high"
            yield Static(f"Model: {model_info}  |  Session: {self.engine.conversation_id[:12]}...", id="header-session")

        with Horizontal(id="body-container"):
            yield VerticalScroll(id="messages-view")
            sidebar = SidebarInspector(id="sidebar")
            sidebar.conversation_id = self.engine.conversation_id
            sidebar.model_name = self.engine.model or "gemini-3.8-flash-high"
            sidebar.mode_name = self.engine.mode or "code"
            sidebar.effort_level = self.engine.effort or "medium"
            rec = self.session_manager.get(self.engine.conversation_id)
            if rec and rec.last_prompt:
                sidebar.session_title = rec.last_prompt.splitlines()[0][:28]
            yield sidebar

        with Vertical(id="footer-area"):
            yield AutocompletePopup(id="autocomplete-popup", classes="hidden")
            with Horizontal(id="input-bar"):
                yield ChatInput(id="chat-input")
                yield Button("Send", id="send-btn")

    async def on_mount(self) -> None:
        self.running = True
        try:
            await self.engine.start()
        except Exception as ex:
            await self._add_system_message(f"Failed to start agy engine: {ex}", style="red")
            return

        self._event_task = asyncio.create_task(self._process_events())
        await self._add_banner_to_chat()
        chat_input = self.query_one("#chat-input", ChatInput)
        chat_input.focus()

    async def on_unmount(self) -> None:
        self.running = False
        if self._event_task and not self._event_task.done():
            self._event_task.cancel()
            try:
                await self._event_task
            except asyncio.CancelledError:
                pass
        await self.engine.stop()

    def action_toggle_sidebar(self) -> None:
        sidebar = self.query_one("#sidebar", SidebarInspector)
        sidebar.display = not sidebar.display

    def action_scroll_page_up(self) -> None:
        try:
            mv = self.query_one("#messages-view", VerticalScroll)
            mv.scroll_page_up(animate=False)
        except Exception:
            pass

    def action_scroll_page_down(self) -> None:
        try:
            mv = self.query_one("#messages-view", VerticalScroll)
            mv.scroll_page_down(animate=False)
        except Exception:
            pass

    async def action_interrupt_turn(self) -> None:
        if self.turn_active:
            await self.engine.interrupt_current_turn()
            await self._add_system_message("Turn interrupted by user.", style="yellow")
            self._update_sidebar_status("Idle")
            if self.is_running:
                try:
                    sb = self.query_one("#sidebar", SidebarInspector)
                    sb.update_telemetry(subagents=[])
                except Exception:
                    pass
            self.turn_active = False
            self._turn_done_event.set()

    def print_banner(self) -> None:
        title = Text.assemble(
            ("Antigravity CLI ", "bold cyan"),
            ("TUI Frontend", "bold green"),
        )
        body = (
            f"[dim]Session ID:[/dim] [cyan]{self.engine.conversation_id}[/cyan]\n"
            "[dim]Commands:[/dim] [bold]/c <id>[/bold] copy block | [bold]/ca[/bold] copy all | "
            "[bold]/model[/bold] switch model | [bold]/mode[/bold] | [bold]/effort[/bold] | [bold]/diff[/bold] | "
            "[bold]/resume[/bold] | [bold]/multi[/bold] | [bold]/thoughts[/bold] | [bold]/sidebar[/bold] | [bold]/clear[/bold] | [bold]/help[/bold] | [bold]/quit[/bold]\n"
            "[dim]Navigation:[/dim] [bold]Up/Down[/bold] history | [bold]Ctrl+B[/bold] toggle sidebar | [bold]Ctrl+C[/bold] interrupt turn"
        )
        self.console.print(Panel(body, title=title, border_style="cyan"))

    def print_help(self) -> None:
        help_text = (
            "[bold cyan]Available Commands:[/bold cyan]\n"
            "  [bold]/model [name][/bold]             Switch or list available models\n"
            "  [bold]/mode [name][/bold]              Set execution mode (code, plan, architect, ask)\n"
            "  [bold]/effort [level][/bold]           Set reasoning effort (low, medium, high)\n"
            "  [bold]/diff[/bold]                      View recent tool file modifications and diffs\n"
            "  [bold]/resume [id][/bold]               List or resume previous sessions\n"
            "  [bold]/c <id>[/bold] or [bold]/copy <id>[/bold]   Copy code block <id> to OS clipboard\n"
            "  [bold]/ca[/bold] or [bold]/copyall[/bold]       Copy entire assistant response to clipboard\n"
            "  [bold]/multi[/bold]                    Toggle multiline input mode\n"
            "  [bold]/send[/bold]                     Submit current multiline buffer\n"
            "  [bold]/thoughts[/bold] or [bold]/expand[/bold]   Toggle reasoning display visibility\n"
            "  [bold]/sidebar[/bold]                  Toggle sidebar inspector (Ctrl+B)\n"
            "  [bold]/history[/bold]                  Display recent command history\n"
            "  [bold]/clear[/bold]                    Clear terminal screen\n"
            "  [bold]/help[/bold]                     Show this help message\n"
            "  [bold]/quit[/bold] or [bold]/exit[/bold]         Exit the application\n\n"
            "[bold cyan]Input & Shortcuts:[/bold cyan]\n"
            "  [bold]Enter[/bold]                     Submit prompt\n"
            "  [bold]Shift+Enter[/bold]               Insert newline\n"
            "  [bold]Ctrl+B[/bold]                    Toggle sidebar inspector\n"
            "  [bold]Up / Down[/bold]                 Browse command history or navigate autocomplete\n"
            "  [bold]Ctrl+C[/bold]                    Interrupt running response"
        )
        self.console.print(Panel(help_text, title="[cyan]Help & Keybindings[/cyan]", border_style="cyan"))

    async def _add_banner_to_chat(self) -> None:
        try:
            mv = self.query_one("#messages-view", VerticalScroll)
            title = "Antigravity CLI  |  OpenCode TUI"
            body = (
                f"Session: {self.engine.conversation_id}\n"
                "Type / to view available commands, Ctrl+B to toggle sidebar, Enter to submit."
            )
            banner = Static(Panel(body, title=title, border_style="cyan"))
            await mv.mount(banner)
            mv.scroll_end(animate=False)
        except Exception:
            pass

    async def _add_system_message(self, message: str, style: str = "cyan") -> None:
        try:
            mv = self.query_one("#messages-view", VerticalScroll)
            panel = Static(Panel(message, border_style=style, expand=False))
            await mv.mount(panel)
            mv.scroll_end(animate=False)
        except Exception:
            self.console.print(f"[{style}]{message}[/{style}]")

    def _update_sidebar_status(self, status: str) -> None:
        try:
            sb = self.query_one("#sidebar", SidebarInspector)
            sb.update_telemetry(status=status)
        except Exception:
            pass

    @on(Button.Pressed, ".copy-btn")
    def on_copy_button_pressed(self, event: Button.Pressed) -> None:
        widget = None
        for a in event.button.ancestors:
            if isinstance(a, CodeBlockWidget):
                widget = a
                break
        if widget and isinstance(widget, CodeBlockWidget):
            success = self.clipboard.copy(widget.code)
            event.button.label = "✓ Copied!" if success else "✗ Failed"
            self.set_timer(2.0, lambda: self._reset_copy_btn(event.button))

    def _reset_copy_btn(self, btn: Button) -> None:
        try:
            btn.label = "📋 Copy"
        except Exception:
            pass

    @on(Button.Pressed, "#toggle-thought-btn")
    def on_toggle_thought_pressed(self, event: Button.Pressed) -> None:
        card = None
        for a in event.button.ancestors:
            if isinstance(a, ThoughtCard):
                card = a
                break
        if card and isinstance(card, ThoughtCard):
            card.toggle()

    @on(Button.Pressed, "#send-btn")
    async def on_send_button_pressed(self, event: Button.Pressed) -> None:
        inp = self.query_one("#chat-input", ChatInput)
        text = inp.text.strip()
        if text:
            inp.text = ""
            inp.styles.height = 3
            await self._handle_input_submission(text)

    @on(ChatInput.Submitted)
    async def on_chat_input_submitted(self, event: ChatInput.Submitted) -> None:
        text = event.value.strip()
        if text:
            await self._handle_input_submission(text)

    @on(ChatInput.HistoryNavigation)
    def on_history_navigation(self, event: ChatInput.HistoryNavigation) -> None:
        inp = self.query_one("#chat-input", ChatInput)
        if event.direction == "up":
            prev = self.history.previous(inp.text)
            inp.text = prev
        elif event.direction == "down":
            nxt = self.history.next()
            inp.text = nxt
        lines = inp.text.split("\n")
        inp.cursor_location = (len(lines) - 1, len(lines[-1]))

    @on(TextArea.Changed, "#chat-input")
    def on_chat_input_changed(self, event: TextArea.Changed) -> None:
        text = event.text_area.text
        lines = max(1, event.text_area.document.line_count)
        event.text_area.styles.height = min(max(3, lines + 2), 9)

        try:
            popup = self.query_one("#autocomplete-popup", AutocompletePopup)
            if text.startswith("/") and " " not in text:
                count = popup.populate(text)
                if count > 0:
                    popup.remove_class("hidden")
                else:
                    popup.add_class("hidden")
            else:
                popup.add_class("hidden")
        except Exception:
            pass

    @on(OptionList.OptionSelected, "#autocomplete-popup")
    def on_autocomplete_selected(self, event: OptionList.OptionSelected) -> None:
        popup = self.query_one("#autocomplete-popup", AutocompletePopup)
        selected_cmd = popup.get_selected_command(event.option_index)
        if selected_cmd:
            inp = self.query_one("#chat-input", ChatInput)
            inp.text = selected_cmd + " "
            inp.cursor_location = (0, len(inp.text))
            popup.add_class("hidden")
            inp.focus()

    async def _handle_input_submission(self, user_line: str) -> None:
        stripped = user_line.strip()
        if not stripped:
            return

        first_token = stripped.split(maxsplit=1)[0].lower() if stripped else ""
        is_path_or_code = (
            stripped.startswith("//")
            or stripped.startswith("/*")
            or ("/" in first_token[1:])
            or first_token == "/"
        )

        if stripped.startswith("/") and not is_path_or_code:
            await self._handle_command(stripped)
            return

        await self._execute_turn(user_line)

    async def _handle_command(self, cmd_line: str) -> bool:
        parts = cmd_line.strip().split(maxsplit=1)
        if not parts:
            return True
        cmd = parts[0].lower()
        arg = parts[1].strip() if len(parts) > 1 else ""

        if cmd in ("/quit", "/exit", "/q"):
            self.running = False
            self.exit(0)
            return False

        if cmd == "/help":
            self.print_help()
            await self._add_system_message("Help displayed. Type / to view commands.", style="cyan")
            return True

        if cmd == "/clear":
            try:
                mv = self.query_one("#messages-view", VerticalScroll)
                await mv.remove_children()
                self._mounted_code_blocks.clear()
                await self._add_banner_to_chat()
            except Exception:
                self.console.clear()
            return True

        if cmd == "/history":
            entries = self.history.get_all()
            if not entries:
                msg = "History is empty."
            else:
                lines = [f"{idx:2d}. {item}" for idx, item in enumerate(entries[-20:], 1)]
                msg = "\n".join(lines)
            await self._add_system_message(msg, style="blue")
            return True

        if cmd in ("/c", "/copy"):
            if not arg.isdigit():
                await self._add_system_message("Usage: /c <block_id> (e.g., /c 1)", style="yellow")
                return True
            block_id = int(arg)
            block = self.renderer.code_registry.get(block_id)
            if block is None:
                total = len(self.renderer.code_registry.all_blocks())
                await self._add_system_message(f"Code block #{block_id} not found. Registered blocks: {total}", style="yellow")
            else:
                success = self.clipboard.copy(block.code)
                if success:
                    await self._add_system_message(
                        f"✓ Code Block #{block_id} ({block.language}) copied to clipboard ({len(block.code)} bytes)",
                        style="green",
                    )
                else:
                    await self._add_system_message(f"Failed to copy Code Block #{block_id}", style="red")
            return True

        if cmd in ("/ca", "/copyall"):
            if not self.current_response_text:
                await self._add_system_message("No response content available to copy.", style="yellow")
            else:
                success = self.clipboard.copy(self.current_response_text)
                if success:
                    await self._add_system_message(
                        f"✓ Full assistant response copied to clipboard ({len(self.current_response_text)} bytes)",
                        style="green",
                    )
                else:
                    await self._add_system_message("Failed to copy response to clipboard", style="red")
            return True

        if cmd in ("/thoughts", "/expand"):
            self.collapse_thoughts = not self.collapse_thoughts
            status = "collapsed" if self.collapse_thoughts else "expanded"
            await self._add_system_message(f"Thought display is now {status}.", style="cyan")
            return True

        if cmd == "/sidebar":
            self.action_toggle_sidebar()
            return True

        if cmd == "/multi":
            self.multiline_mode = not self.multiline_mode
            status = "enabled" if self.multiline_mode else "disabled"
            try:
                inp = self.query_one("#chat-input", ChatInput)
                inp.multiline_submit_ctrl_enter = self.multiline_mode
            except Exception:
                pass
            await self._add_system_message(f"Multiline mode {status}.", style="cyan")
            return True

        if cmd == "/send":
            self.multiline_mode = False
            if self.multiline_buffer:
                full_prompt = "\n".join(self.multiline_buffer)
                self.multiline_buffer.clear()
                await self._execute_turn(full_prompt)
            else:
                await self._add_system_message("Multiline buffer is empty.", style="yellow")
            return True

        if cmd == "/model":
            if self.turn_active:
                await self._add_system_message("Cannot change model while a turn is active. Interrupt first with Ctrl+C.", style="yellow")
                return True
            if not arg:
                active = self.engine.model or "default (gemini-3.8-flash-high)"
                models_str = ", ".join(self.AVAILABLE_MODELS)
                msg = f"Active Model: {active}\nAvailable Models:\n{models_str}\n\nSwitch with /model <name>"
                await self._add_system_message(msg, style="cyan")
            else:
                self.engine.model = arg
                if self.engine.is_running and not self.turn_active:
                    await self.engine.restart()
                try:
                    sb = self.query_one("#sidebar", SidebarInspector)
                    sb.update_telemetry(model=arg)
                    hdr = self.query_one("#header-session", Static)
                    hdr.update(f"Model: {arg}  |  Session: {self.engine.conversation_id[:12]}...")
                except Exception:
                    pass
                await self._add_system_message(f"Active model switched to: {arg}", style="green")
            return True

        if cmd == "/mode":
            if self.turn_active:
                await self._add_system_message("Cannot change mode while a turn is active. Interrupt first with Ctrl+C.", style="yellow")
                return True
            if not arg:
                active = self.engine.mode or "code"
                modes_str = ", ".join(self.AVAILABLE_MODES)
                msg = f"Active Mode: {active}\nAvailable Modes: {modes_str}\nSwitch with /mode <name>"
                await self._add_system_message(msg, style="cyan")
            else:
                if arg not in self.AVAILABLE_MODES:
                    await self._add_system_message(f"Unknown mode '{arg}'. Available: {', '.join(self.AVAILABLE_MODES)}", style="yellow")
                else:
                    self.engine.mode = arg
                    if self.engine.is_running and not self.turn_active:
                        await self.engine.restart()
                    try:
                        sb = self.query_one("#sidebar", SidebarInspector)
                        sb.update_telemetry(mode=arg)
                    except Exception:
                        pass
                    await self._add_system_message(f"Execution mode set to: {arg}", style="green")
            return True

        if cmd == "/effort":
            if self.turn_active:
                await self._add_system_message("Cannot change effort while a turn is active. Interrupt first with Ctrl+C.", style="yellow")
                return True
            if not arg:
                active = self.engine.effort or "medium"
                efforts_str = ", ".join(self.AVAILABLE_EFFORTS)
                msg = f"Active Effort: {active}\nAvailable Efforts: {efforts_str}\nSet with /effort <level>"
                await self._add_system_message(msg, style="cyan")
            else:
                if arg not in self.AVAILABLE_EFFORTS:
                    await self._add_system_message(f"Unknown effort '{arg}'. Available: {', '.join(self.AVAILABLE_EFFORTS)}", style="yellow")
                else:
                    self.engine.effort = arg
                    if self.engine.is_running and not self.turn_active:
                        await self.engine.restart()
                    try:
                        sb = self.query_one("#sidebar", SidebarInspector)
                        sb.update_telemetry(effort=arg)
                    except Exception:
                        pass
                    await self._add_system_message(f"Reasoning effort set to: {arg}", style="green")
            return True

        if cmd == "/diff":
            diffs = self.diff_registry.get_all()
            if not diffs:
                await self._add_system_message("No tool diffs recorded in the current session.", style="yellow")
            elif not arg:
                latest = diffs[-1]
                try:
                    mv = self.query_one("#messages-view", VerticalScroll)
                    await mv.mount(DiffViewWidget(latest))
                    mv.scroll_end(animate=False)
                except Exception:
                    self.console.print(self.renderer.render_diff(latest.file_path, latest.diff_text, latest.tool_name))
            elif arg.lower() in ("list", "-l"):
                lines = [f"Recorded Diffs ({len(diffs)} total):"]
                for idx, d in enumerate(diffs, 1):
                    lines.append(f"  [{idx}] {d.file_path} ({d.tool_name}) - {d.timestamp}")
                lines.append("\nView with /diff <number_or_path>")
                await self._add_system_message("\n".join(lines), style="cyan")
            else:
                found = None
                if arg.isdigit():
                    idx = int(arg) - 1
                    if 0 <= idx < len(diffs):
                        found = diffs[idx]
                if not found:
                    for d in diffs:
                        if arg in d.file_path:
                            found = d
                            break
                if found:
                    try:
                        mv = self.query_one("#messages-view", VerticalScroll)
                        await mv.mount(DiffViewWidget(found))
                        mv.scroll_end(animate=False)
                    except Exception:
                        self.console.print(self.renderer.render_diff(found.file_path, found.diff_text, found.tool_name))
                else:
                    await self._add_system_message(f"Diff '{arg}' not found. Total recorded diffs: {len(diffs)}", style="yellow")
            return True

        if cmd == "/resume":
            if self.turn_active:
                await self._add_system_message("Cannot resume while a turn is active. Interrupt first with Ctrl+C.", style="yellow")
                return True
            if not arg:
                sessions = self.session_manager.list_sessions(limit=10)
                if not sessions:
                    await self._add_system_message("No previous sessions found in history.", style="yellow")
                else:
                    lines = ["Previous Sessions:"]
                    for idx, s in enumerate(sessions, 1):
                        lines.append(f"  [{idx}] {s.conversation_id[:16]}... ({s.model}, {s.mode}) - {s.last_prompt} ({s.timestamp})")
                    lines.append("\nResume with /resume <number_or_uuid>")
                    await self._add_system_message("\n".join(lines), style="cyan")
            else:
                rec = self.session_manager.get_session(arg)
                target_id = rec.conversation_id if rec else arg
                await self.engine.stop()
                self.engine.conversation_id = target_id
                if rec:
                    self.engine.model = rec.model
                    self.engine.mode = rec.mode
                    self.engine.effort = rec.effort
                await self.engine.start()
                try:
                    sb = self.query_one("#sidebar", SidebarInspector)
                    sb.update_telemetry(conversation_id=target_id, model=self.engine.model, mode=self.engine.mode, effort=self.engine.effort)
                    hdr = self.query_one("#header-session", Static)
                    hdr.update(f"Model: {self.engine.model or 'default'}  |  Session: {target_id[:12]}...")
                except Exception:
                    pass
                await self._add_system_message(f"✓ Resumed session: {target_id}", style="green")
            return True

        await self._add_system_message(f"Unknown command: {cmd}. Type /help for available commands.", style="yellow")
        return True

    async def _process_events(self) -> None:
        try:
            while self.running:
                event = await self.engine.events_queue.get()
                if event is None:
                    self._finalize_thought()
                    self._finalize_agent_response()
                    if self.turn_active:
                        self.turn_active = False
                        self._update_sidebar_status("Idle")
                        if self.is_running:
                            try:
                                sb = self.query_one("#sidebar", SidebarInspector)
                                sb.update_telemetry(subagents=[])
                            except Exception:
                                pass
                        if self.engine.stderr_lines:
                            err_info = self.engine.stderr_lines[-1]
                            await self._add_system_message(f"Engine terminated: {err_info}", style="red")
                        else:
                            await self._add_system_message("Engine disconnected.", style="yellow")
                        self._turn_done_event.set()
                    if not self.running:
                        break
                    continue
                self._dispatch_event(event)
        except asyncio.CancelledError:
            pass

    def _render_stream_delta(self, delta: str) -> None:
        if not delta:
            return
        self.current_response_text += delta
        active_chunk = self.current_response_text[self._segment_start_idx:]
        if self.is_running:
            try:
                mv = self.query_one("#messages-view", VerticalScroll)
                rendered = self.renderer.render_markdown_with_code_blocks(active_chunk)
                if self._active_stream_widget is None:
                    self._active_stream_widget = Static(rendered, classes="message-body")
                    mv.mount(self._active_stream_widget)
                else:
                    self._active_stream_widget.update(rendered)
                mv.scroll_end(animate=False)
            except Exception:
                pass

    def _finalize_agent_response(self) -> None:
        active_chunk = self.current_response_text[self._segment_start_idx:]
        if self.is_running:
            if active_chunk.strip():
                try:
                    mv = self.query_one("#messages-view", VerticalScroll)
                    blocks = list(self.renderer.CODE_FENCE_REGEX.finditer(active_chunk))
                    if blocks:
                        if self._active_stream_widget is not None:
                            try:
                                self._active_stream_widget.remove()
                            except Exception:
                                pass
                        last_idx = 0
                        for m in blocks:
                            start, end = m.span()
                            if start > last_idx:
                                part = active_chunk[last_idx:start].strip()
                                if part:
                                    mv.mount(Static(self.renderer.render_markdown_with_code_blocks(part), classes="message-body"))
                            lang = self.renderer._parse_lang(m.group(2))
                            code = m.group(3)
                            block = self.renderer.code_registry.register(lang, code)
                            self._mounted_code_blocks.add(block.id)
                            mv.mount(CodeBlockWidget(block.id, lang, code))
                            last_idx = end
                        if last_idx < len(active_chunk):
                            part = active_chunk[last_idx:].strip()
                            if part:
                                mv.mount(Static(self.renderer.render_markdown_with_code_blocks(part), classes="message-body"))
                        mv.scroll_end(animate=False)
                    else:
                        if self._active_stream_widget is None:
                            rendered = self.renderer.render_markdown_with_code_blocks(active_chunk)
                            mv.mount(Static(rendered, classes="message-body"))
                            mv.scroll_end(animate=False)
                except Exception:
                    pass
            self._active_stream_widget = None
            self.renderer.extract_and_register_code_blocks(self.current_response_text)
            self._mount_new_code_blocks()
        else:
            if active_chunk.strip():
                self.console.print(self.renderer.render_markdown_with_code_blocks(active_chunk))
            self.renderer.extract_and_register_code_blocks(self.current_response_text)
        self._segment_start_idx = len(self.current_response_text)

    def _mount_new_code_blocks(self) -> None:
        if not self.is_running:
            return
        try:
            mv = self.query_one("#messages-view", VerticalScroll)
            for block in self.renderer.code_registry.all_blocks():
                if block.id not in self._mounted_code_blocks:
                    self._mounted_code_blocks.add(block.id)
                    widget = CodeBlockWidget(block.id, block.language, block.code)
                    mv.mount(widget)
                    mv.scroll_end(animate=False)
        except Exception:
            pass

    def _finalize_thought(self) -> None:
        if self._current_thought_text:
            if self.is_running:
                try:
                    mv = self.query_one("#messages-view", VerticalScroll)
                    card = ThoughtCard(self._current_thought_text, is_collapsed=self.collapse_thoughts)
                    mv.mount(card)
                    mv.scroll_end(animate=False)
                except Exception:
                    pass
            else:
                self.console.print(
                    self.renderer.render_thought(self._current_thought_text, is_collapsed=self.collapse_thoughts)
                )
            self._current_thought_text = ""
            self._active_thought_card = None

    def _dispatch_event(self, event: StreamEvent) -> None:
        if event.event_type == "init":
            if event.conversation_id:
                self.engine.conversation_id = event.conversation_id
                if self.is_running:
                    try:
                        sb = self.query_one("#sidebar", SidebarInspector)
                        sb.update_telemetry(conversation_id=event.conversation_id)
                    except Exception:
                        pass

        elif event.thought_text:
            self._finalize_agent_response()
            self._current_thought_text += event.thought_text
            self._update_sidebar_status("Thinking...")

        elif event.event_type == "step_update":
            if event.step_type == "agent_response":
                self._finalize_thought()
                self._update_sidebar_status("Responding...")
                delta = event.text_delta
                if delta:
                    self._render_stream_delta(delta)
                if event.subagents is not None and self.is_running:
                    try:
                        sb = self.query_one("#sidebar", SidebarInspector)
                        sb.update_telemetry(subagents=event.subagents)
                    except Exception:
                        pass
                if event.state == "DONE":
                    self._finalize_agent_response()
            elif event.step_type == "user_input":
                pass
            else:
                self._finalize_thought()
                self._finalize_agent_response()
                tname = event.tool_name or event.step_type or "tool"
                self._update_sidebar_status(f"Running Tool: {tname}")
                if self.is_running:
                    try:
                        sb = self.query_one("#sidebar", SidebarInspector)
                        kwargs: dict[str, Any] = {"tool_step": f"{tname} ({event.state})"}
                        if event.subagents is not None:
                            kwargs["subagents"] = event.subagents
                        sb.update_telemetry(**kwargs)
                    except Exception:
                        pass

                    diff_rec = format_diff_from_tool(tname, event.tool_args, event.tool_diff)
                    if diff_rec:
                        added = self.diff_registry.add(diff_rec)
                        if added:
                            try:
                                mv = self.query_one("#messages-view", VerticalScroll)
                                mv.mount(DiffViewWidget(diff_rec))
                            except Exception:
                                pass

                    try:
                        mv = self.query_one("#messages-view", VerticalScroll)
                        card = StepCard(
                            step_type=event.step_type or "step",
                            state=event.state or "ACTIVE",
                            step_index=event.step_index,
                        )
                        mv.mount(card)
                    except Exception:
                        pass
                else:
                    self.console.print(
                        self.renderer.render_step(
                            step_type=event.step_type or "step",
                            state=event.state or "ACTIVE",
                            step_index=event.step_index,
                        )
                    )

        elif event.event_type == "message":
            self._finalize_thought()
            delta = event.text_delta
            if delta:
                self._render_stream_delta(delta)

        elif event.event_type == "result":
            self.turn_active = False
            self._update_sidebar_status("Idle")
            self._finalize_thought()
            self._finalize_agent_response()
            if self.is_running:
                try:
                    sb = self.query_one("#sidebar", SidebarInspector)
                    sb.update_telemetry(subagents=[])
                except Exception:
                    pass

            if not self.current_response_text and event.result_response:
                self.current_response_text = event.result_response
                self._finalize_agent_response()

            duration = None
            if self._turn_start_time is not None:
                duration = max(0.0, time.monotonic() - self._turn_start_time)
                self._turn_start_time = None
            elif event.result_data:
                duration = event.result_data.get("duration_seconds")

            usage = event.usage
            tokens_per_sec = None
            if usage:
                completion_tokens = usage.get("completion_tokens") or 0
                if duration and duration > 0 and completion_tokens > 0:
                    tokens_per_sec = completion_tokens / duration
                if self.is_running:
                    tokens = usage.get("total_tokens") or (usage.get("prompt_tokens", 0) + completion_tokens)
                    try:
                        sb = self.query_one("#sidebar", SidebarInspector)
                        sb.update_telemetry(tokens=tokens)
                    except Exception:
                        pass

            if self.is_running:
                try:
                    mv = self.query_one("#messages-view", VerticalScroll)
                    rendered_res = self.renderer.render_result(
                        status=event.result_status or "SUCCESS",
                        duration=duration,
                        error_message=event.result_error,
                        tokens_per_sec=tokens_per_sec,
                    )
                    mv.mount(Static(rendered_res))
                except Exception:
                    pass
            else:
                self.console.print(
                    self.renderer.render_result(
                        status=event.result_status or "SUCCESS",
                        duration=duration,
                        error_message=event.result_error,
                        tokens_per_sec=tokens_per_sec,
                    )
                )

            self._turn_done_event.set()

        elif event.is_error:
            self.turn_active = False
            self._update_sidebar_status("Error")
            if self.is_running:
                try:
                    sb = self.query_one("#sidebar", SidebarInspector)
                    sb.update_telemetry(subagents=[])
                except Exception:
                    pass
            self._finalize_thought()
            self._finalize_agent_response()
            err_msg = event.result_error or event.raw.get("error") or "Unknown error"
            if self.is_running:
                try:
                    mv = self.query_one("#messages-view", VerticalScroll)
                    mv.mount(Static(Panel(f"Engine Error: {err_msg}", border_style="red")))
                except Exception:
                    pass
            else:
                self.console.print(f"\n[bold red]Engine Error:[/bold red] {err_msg}")
            self._turn_done_event.set()

    async def _execute_turn(self, prompt: str) -> None:
        if self.turn_active:
            await self._add_system_message("Agent is currently busy. Press Ctrl+C to interrupt the current turn.", style="yellow")
            return
        self.turn_active = True
        self.current_response_text = ""
        self._segment_start_idx = 0
        self._current_thought_text = ""
        self._turn_done_event.clear()

        while not self.engine.events_queue.empty():
            try:
                self.engine.events_queue.get_nowait()
            except asyncio.QueueEmpty:
                break

        self.history.add(prompt)
        self.session_manager.record(
            conversation_id=self.engine.conversation_id,
            model=self.engine.model,
            mode=self.engine.mode,
            effort=self.engine.effort,
            prompt=prompt,
        )
        try:
            readline.add_history(prompt)
        except Exception:
            pass

        try:
            mv = self.query_one("#messages-view", VerticalScroll)
            await mv.mount(MessageCard("user", prompt))
            mv.scroll_end(animate=False)
        except Exception:
            pass

        try:
            sb = self.query_one("#sidebar", SidebarInspector)
            if sb.session_title in ("Nova sessão", "default", ""):
                sb.session_title = prompt.splitlines()[0][:28]
        except Exception:
            pass

        self._update_sidebar_status("Thinking...")
        self._turn_start_time = time.monotonic()

        try:
            await self.engine.send_prompt(prompt)
            await self._turn_done_event.wait()
        except KeyboardInterrupt:
            await self.engine.interrupt_current_turn()
            await self._add_system_message("Turn cancelled by user.", style="yellow")
            try:
                await asyncio.wait_for(self._turn_done_event.wait(), timeout=1.5)
            except TimeoutError:
                pass
            self.turn_active = False
        except Exception as ex:
            await self._add_system_message(f"Execution failed: {ex}", style="red")
            self.turn_active = False
        finally:
            self.turn_active = False
            self._update_sidebar_status("Idle")
            if self.is_running:
                try:
                    sb = self.query_one("#sidebar", SidebarInspector)
                    sb.update_telemetry(subagents=[])
                except Exception:
                    pass
            self._finalize_thought()
            self._finalize_agent_response()

    def _sync_read_line(self, prompt_str: str) -> str:
        return input(prompt_str)

    async def run_line_mode(self) -> None:
        self.running = True
        self.print_banner()

        try:
            await self.engine.start()
        except Exception as ex:
            self.console.print(f"[bold red]Failed to start agy engine:[/bold red] {ex}")
            return

        self._event_task = asyncio.create_task(self._process_events())

        try:
            while self.running:
                prompt_label = "... " if self.multiline_mode else ">>> "
                try:
                    user_line = await asyncio.to_thread(self._sync_read_line, prompt_label)
                except EOFError:
                    if self.multiline_mode and self.multiline_buffer:
                        full_prompt = "\n".join(self.multiline_buffer)
                        self.multiline_buffer.clear()
                        self.multiline_mode = False
                        self._continuation_mode = False
                        await self._execute_turn(full_prompt)
                        continue
                    break
                except KeyboardInterrupt:
                    if self.turn_active:
                        await self.engine.interrupt_current_turn()
                        self.console.print("\n[yellow]Interrupted turn.[/yellow]")
                    else:
                        self.console.print("^C")
                        self.multiline_buffer.clear()
                        self.multiline_mode = False
                        self._continuation_mode = False
                    continue

                stripped = user_line.strip()
                if not stripped and not self.multiline_mode:
                    continue

                first_token = stripped.split(maxsplit=1)[0].lower() if stripped else ""
                is_control_cmd = first_token in self.CONTROL_COMMANDS

                if self.multiline_mode:
                    if is_control_cmd:
                        should_continue = await self._handle_command(stripped)
                        if not should_continue:
                            break
                        continue
                    if self._continuation_mode:
                        if user_line.rstrip().endswith("\\"):
                            self.multiline_buffer.append(user_line.rstrip()[:-1])
                        else:
                            self.multiline_buffer.append(user_line)
                            full_prompt = "\n".join(self.multiline_buffer)
                            self.multiline_buffer.clear()
                            self.multiline_mode = False
                            self._continuation_mode = False
                            await self._execute_turn(full_prompt)
                    else:
                        self.multiline_buffer.append(user_line)
                    continue

                is_path_or_code = (
                    stripped.startswith("//")
                    or stripped.startswith("/*")
                    or ("/" in first_token[1:])
                    or first_token == "/"
                )

                if stripped.startswith("/") and not is_path_or_code:
                    should_continue = await self._handle_command(stripped)
                    if not should_continue:
                        break
                    continue

                if user_line.rstrip().endswith("\\"):
                    self.multiline_buffer.append(user_line.rstrip()[:-1])
                    self.multiline_mode = True
                    self._continuation_mode = True
                    continue

                await self._execute_turn(user_line)

        finally:
            self.running = False
            if self._event_task and not self._event_task.done():
                self._event_task.cancel()
                try:
                    await self._event_task
                except asyncio.CancelledError:
                    pass
            await self.engine.stop()
            self.console.print("[dim]agy session ended cleanly.[/dim]")

    async def run(self, *args, **kwargs) -> Any:
        force_fullscreen = getattr(self, "_force_fullscreen", False)
        is_tty = sys.stdin.isatty() and sys.stdout.isatty()
        if not is_tty and not force_fullscreen:
            return await self.run_line_mode()
        return await self.run_async(*args, **kwargs)
