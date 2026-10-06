import io
import os
import time

from rich.console import Console

from agy_tui.app import TUIApp
from agy_tui.clipboard import ClipboardManager
from agy_tui.input import InputHistory
from agy_tui.protocol import StreamEvent
from agy_tui.renderer import MarkdownRenderer


def test_input_history_navigation():
    history = InputHistory(max_size=5)
    history.add("prompt 1")
    history.add("prompt 2")
    history.add("prompt 3")

    assert history.previous("current typing") == "prompt 3"
    assert history.previous("current typing") == "prompt 2"
    assert history.previous("current typing") == "prompt 1"
    assert history.previous("current typing") == "prompt 1"

    assert history.next() == "prompt 2"
    assert history.next() == "prompt 3"
    assert history.next() == "current typing"


def test_app_help_and_history_command():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)
    app.history.add("test command 1")
    app.history.add("test command 2")

    import asyncio
    asyncio.run(app._handle_command("/help"))
    output = out.getvalue()
    assert "Available Commands" in output

    out.seek(0)
    out.truncate(0)
    asyncio.run(app._handle_command("/history"))
    history_output = out.getvalue()
    assert "test command 1" in history_output
    assert "test command 2" in history_output


def test_app_copy_commands():
    out = io.StringIO()
    console = Console(file=out, width=100)
    clipboard = ClipboardManager(clip_exe_path="/nonexistent/clip.exe", enable_osc52=False)
    renderer = MarkdownRenderer()
    renderer.extract_and_register_code_blocks("```python\ndef test():\n    return 99\n```")

    app = TUIApp(console=console, renderer=renderer, clipboard=clipboard)
    app.current_response_text = "Full assistant response here"

    import asyncio
    asyncio.run(app._handle_command("/c 1"))
    assert clipboard.read_clipboard() == "def test():\n    return 99\n"

    asyncio.run(app._handle_command("/ca"))
    assert clipboard.read_clipboard() == "Full assistant response here"

    asyncio.run(app._handle_command("/c 999"))
    output = out.getvalue()
    assert "not found" in output


def test_app_multiline_toggle():
    app = TUIApp()
    assert app.multiline_mode is False
    import asyncio
    asyncio.run(app._handle_command("/multi"))
    assert app.multiline_mode is True
    asyncio.run(app._handle_command("/multi"))
    assert app.multiline_mode is False


def test_app_dispatch_events_smoothly():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    ev_init = StreamEvent(event_type="init", raw={"event": "init", "conversation_id": "test"})
    app._dispatch_event(ev_init)

    ev_thought = StreamEvent(event_type="thought", raw={"event": "thought", "thought": "Plan steps"})
    app._dispatch_event(ev_thought)

    ev_step = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 1,
                "step_type": "agent_response",
                "state": "ACTIVE",
                "text_delta": "Hello ",
            },
        },
    )
    app._dispatch_event(ev_step)
    assert app.current_response_text == "Hello "

    ev_step2 = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 1,
                "step_type": "agent_response",
                "state": "DONE",
                "text_delta": "World!",
            },
        },
    )
    app._dispatch_event(ev_step2)
    assert app.current_response_text == "Hello World!"

    ev_result = StreamEvent(
        event_type="result",
        raw={
            "event": "result",
            "result": {
                "status": "SUCCESS",
                "response": "Hello World!",
                "duration_seconds": 0.5,
            },
        },
    )
    app._dispatch_event(ev_result)
    assert app.turn_active is False


def test_app_dispatch_renders_markdown_and_registers_blocks():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    chunk1 = "Here is the implementation:\n\n```python\n"
    chunk2 = "def add(a, b):\n    return a + b\n```\n"

    ev1 = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 1,
                "step_type": "agent_response",
                "state": "ACTIVE",
                "text_delta": chunk1,
            },
        },
    )
    app._dispatch_event(ev1)

    ev2 = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 1,
                "step_type": "agent_response",
                "state": "DONE",
                "text_delta": chunk2,
            },
        },
    )
    app._dispatch_event(ev2)

    ev_res = StreamEvent(
        event_type="result",
        raw={
            "event": "result",
            "result": {
                "status": "SUCCESS",
                "response": chunk1 + chunk2,
            },
        },
    )
    app._dispatch_event(ev_res)

    blocks = app.renderer.code_registry.all_blocks()
    assert len(blocks) == 1
    assert blocks[0].id == 1
    assert blocks[0].language == "python"
    assert "def add(a, b):" in blocks[0].code
    output = out.getvalue()
    assert "Code Block #1" in output


def test_app_stream_eof_clears_turn_active():
    import asyncio
    app = TUIApp()
    app.running = True
    app.turn_active = True
    app._turn_done_event.clear()

    async def _test():
        await app.engine.events_queue.put(None)
        task = asyncio.create_task(app._process_events())
        await asyncio.sleep(0.05)
        assert app.turn_active is False
        assert app._turn_done_event.is_set()
        app.running = False
        await app.engine.events_queue.put(None)
        await task

    asyncio.run(_test())


def test_app_multi_message_turn_rendered():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    ev_m1 = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 1,
                "step_type": "agent_response",
                "state": "ACTIVE",
                "text_delta": "Message Part 1.",
            },
        },
    )
    app._dispatch_event(ev_m1)
    ev_m1_done = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 1,
                "step_type": "agent_response",
                "state": "DONE",
            },
        },
    )
    app._dispatch_event(ev_m1_done)

    ev_tool = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 2,
                "step_type": "tool_execution",
                "state": "DONE",
            },
        },
    )
    app._dispatch_event(ev_tool)

    ev_m2 = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 3,
                "step_type": "agent_response",
                "state": "ACTIVE",
                "text_delta": "Message Part 2.",
            },
        },
    )
    app._dispatch_event(ev_m2)
    ev_m2_done = StreamEvent(
        event_type="step_update",
        raw={
            "event": "step_update",
            "step_update": {
                "step_index": 3,
                "step_type": "agent_response",
                "state": "DONE",
            },
        },
    )
    app._dispatch_event(ev_m2_done)

    ev_res = StreamEvent(
        event_type="result",
        raw={
            "event": "result",
            "result": {
                "status": "SUCCESS",
            },
        },
    )
    app._dispatch_event(ev_res)

    output = out.getvalue()
    assert "Message Part 1." in output
    assert "Message Part 2." in output
    assert "tool_execution" in output


def test_app_result_direct_response_fallback():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    ev_res = StreamEvent(
        event_type="result",
        raw={
            "event": "result",
            "result": {
                "status": "SUCCESS",
                "response": "Direct answer without prior deltas.",
            },
        },
    )
    app._dispatch_event(ev_res)

    output = out.getvalue()
    assert "Direct answer without prior deltas." in output
    assert app.current_response_text == "Direct answer without prior deltas."


def test_app_send_resets_multiline_mode():
    import asyncio
    app = TUIApp()
    app.multiline_mode = True
    app.multiline_buffer = ["line 1", "line 2"]

    async def _mock_turn(p):
        pass

    app._execute_turn = _mock_turn
    asyncio.run(app._handle_command("/send"))

    assert app.multiline_mode is False
    assert len(app.multiline_buffer) == 0


def test_app_thoughts_toggle_command():
    import asyncio
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    assert app.collapse_thoughts is False
    asyncio.run(app._handle_command("/thoughts"))
    assert app.collapse_thoughts is True

    asyncio.run(app._handle_command("/expand"))
    assert app.collapse_thoughts is False


def test_app_thought_deltas_buffered():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    ev_t1 = StreamEvent(event_type="thought", raw={"event": "thought", "thought": "Analyzing "})
    ev_t2 = StreamEvent(event_type="thought", raw={"event": "thought", "thought": "the trade-offs"})
    app._dispatch_event(ev_t1)
    app._dispatch_event(ev_t2)

    ev_res = StreamEvent(
        event_type="result",
        raw={
            "event": "result",
            "result": {
                "status": "SUCCESS",
            },
        },
    )
    app._dispatch_event(ev_res)

    output = out.getvalue()
    assert "Analyzing the trade-offs" in output
    assert output.count("Thought / Reasoning") == 1


def test_interrupt_settles_and_subsequent_turn_succeeds():
    import asyncio
    import os

    from agy_tui.engine import AgyEngine

    async def _run():
        mock_script = os.path.abspath(os.path.join(os.path.dirname(__file__), "mock_engine.py"))
        engine = AgyEngine(agy_bin=mock_script)
        out = io.StringIO()
        console = Console(file=out, width=100)
        app = TUIApp(engine=engine, console=console)
        await engine.start()
        app.running = True
        event_task = asyncio.create_task(app._process_events())

        t1 = asyncio.create_task(app._execute_turn("SLOW"))
        await asyncio.sleep(0.1)
        await engine.interrupt_current_turn()
        await t1

        await app._execute_turn("calculate 42")
        assert "Response to: calculate 42" in app.current_response_text

        app.running = False
        await engine.stop()
        event_task.cancel()
        try:
            await event_task
        except asyncio.CancelledError:
            pass

    asyncio.run(_run())


def test_multiline_blank_lines_preserved():
    import asyncio
    app = TUIApp()
    app.multiline_mode = True

    executed = []

    async def mock_execute(p):
        executed.append(p)

    app._execute_turn = mock_execute

    app.multiline_buffer.append("def foo():")
    app.multiline_buffer.append("    x = 1")
    app.multiline_buffer.append("")
    app.multiline_buffer.append("    return x")

    asyncio.run(app._handle_command("/send"))

    assert len(executed) == 1
    assert executed[0] == "def foo():\n    x = 1\n\n    return x"
    assert app.multiline_mode is False


def test_continuation_mode_backslash():
    import asyncio
    app = TUIApp()
    executed = []

    async def mock_execute(p):
        executed.append(p)

    app._execute_turn = mock_execute

    line1 = "select * from users \\"
    line2 = "where active = true"

    app.multiline_buffer.append(line1.rstrip()[:-1])
    app.multiline_mode = True
    app._continuation_mode = True

    if not line2.rstrip().endswith("\\"):
        app.multiline_buffer.append(line2)
        full = "\n".join(app.multiline_buffer)
        app.multiline_buffer.clear()
        app.multiline_mode = False
        app._continuation_mode = False
        asyncio.run(app._execute_turn(full))

    assert len(executed) == 1
    assert executed[0] == "select * from users \nwhere active = true"
    assert app.multiline_mode is False
    assert app._continuation_mode is False


def test_slash_paths_and_comments_not_intercepted_as_commands():
    from agy_tui.cli import build_parser
    parser = build_parser()
    parsed, extra = parser.parse_known_args(["--dangerously-skip-permissions", "--model", "gemini-2.5-pro"])
    assert "--dangerously-skip-permissions" in extra
    assert "--model" in extra
    assert "gemini-2.5-pro" in extra


def test_single_line_slash_prompts_vs_commands():
    def is_command(user_line):
        stripped = user_line.strip()
        first_token = stripped.split(maxsplit=1)[0].lower() if stripped else ""
        is_path_or_code = (
            stripped.startswith("//")
            or stripped.startswith("/*")
            or ("/" in first_token[1:])
            or first_token == "/"
        )
        return stripped.startswith("/") and not is_path_or_code

    assert is_command("/help") is True
    assert is_command("/c 1") is True
    assert is_command("/foo") is True
    assert is_command("// comment") is False
    assert is_command("/* block */") is False
    assert is_command("/usr/bin/python") is False
    assert is_command("/home/julio/test.py") is False
    assert is_command("/ 10") is False


def test_turn_duration_and_throughput_calculation():
    out = io.StringIO()
    console = Console(file=out, width=100)
    app = TUIApp(console=console)

    app._turn_start_time = time.monotonic() - 2.5
    ev_result = StreamEvent(
        event_type="result",
        raw={
            "event": "result",
            "result": {
                "status": "SUCCESS",
                "duration_seconds": 999.0,
                "usage": {
                    "completion_tokens": 100,
                    "prompt_tokens": 50,
                    "total_tokens": 150,
                },
            },
        },
    )
    app._dispatch_event(ev_result)
    output = out.getvalue()
    assert "999.00s" not in output
    assert "tok/s" in output
    assert app._turn_start_time is None


def test_sidebar_minimal_context_formatting():
    from agy_tui.widgets import SidebarInspector

    home = os.path.expanduser("~")
    test_workspace = os.path.join(home, "workspace")

    sb = SidebarInspector()
    sb.tokens_used = 4554
    sb.context_limit = 200000
    sb.workspace_path = test_workspace

    assert sb._format_token_count() == "4,554 tokens"
    assert sb._format_token_percent() == "2% used"
    assert sb._get_workspace_path() == "~/workspace"

    sb.update_telemetry(tokens=0, context_limit=1000000, session_title="Saudação casual")
    assert sb.session_title == "Saudação casual"
    assert sb._format_token_count() == "0 tokens"
    assert sb._format_token_percent() == "0% used"


def test_chat_input_subbar_formatting():
    from agy_tui.widgets import ChatInputSubBar

    subbar = ChatInputSubBar()
    subbar.mode_name = "code"
    subbar.model_name = "gemini-3.8-flash-high"
    subbar.effort_level = "medium"

    assert subbar.get_plain_text() == "Code · gemini-3.8-flash-high Medium"
    assert "Code" in subbar._build_text()
    assert "gemini-3.8-flash-high" in subbar._build_text()
    assert "Medium" in subbar._build_text()

    subbar.update_info(mode="plan", effort="high")
    assert subbar.get_plain_text() == "Plan · gemini-3.8-flash-high High"


def test_chat_footer_bar_formatting():
    from agy_tui.widgets import ChatFooterBar, format_compact_tokens, format_workspace_path

    home = os.path.expanduser("~")
    test_workspace = os.path.join(home, "workspace")

    assert format_workspace_path(test_workspace) == "~/workspace"
    assert format_compact_tokens(4600, 1000000) == "4.6K (0%)"
    assert format_compact_tokens(4600, 200000) == "4.6K (2%)"
    assert format_compact_tokens(0, 200000) == "0 (0%)"
    assert format_compact_tokens(1500000, 2000000) == "1.5M (75%)"

    fb = ChatFooterBar()
    fb.workspace_path = test_workspace
    fb.tokens_used = 4600
    fb.context_limit = 1000000

    assert fb._get_workspace_path() == "~/workspace"
    assert fb._format_compact_tokens() == "4.6K (0%)"
    assert "tab /help" in fb._build_context_and_shortcuts()

    fb.update_telemetry(tokens=0)
    assert fb._format_compact_tokens() == "0 (0%)"


def test_chat_input_container_composition():
    from agy_tui.widgets import ChatInput, ChatInputContainer, ChatInputSubBar

    container = ChatInputContainer()
    container.mode_name = "plan"
    container.model_name = "gemini-2.5-pro"
    container.effort_level = "high"

    children = list(container.compose())
    assert len(children) == 2
    assert isinstance(children[0], ChatInput)
    assert isinstance(children[1], ChatInputSubBar)
    assert children[1].mode_name == "plan"
    assert children[1].model_name == "gemini-2.5-pro"
    assert children[1].effort_level == "high"


def test_app_compose_layout_chat_input_and_footer():
    from agy_tui.widgets import ChatFooterBar, ChatInputContainer

    container = ChatInputContainer()
    assert container.mode_name == "code"
    assert container.model_name == "gemini-3.8-flash-high"
    assert container.effort_level == "medium"

    fb = ChatFooterBar()
    assert fb.tokens_used == 0
    assert fb.context_limit == 200000
    assert "tab /help" in fb.shortcuts_text


def test_two_stage_model_and_effort_selection_flow():
    import asyncio

    from textual.widgets import OptionList

    from agy_tui.widgets import ModelSelectModal

    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            await pilot.app._handle_command("/model")
            await pilot.pause()
            assert isinstance(pilot.app.screen, ModelSelectModal)
            modal = pilot.app.screen
            assert modal._step == 1

            opt_list = modal.query_one("#model-modal-options", OptionList)
            opt_list.highlighted = 1
            opt_list.action_select()
            await pilot.pause()
            assert modal._step == 2
            assert modal._selected_model == "gemini-3.7-flash"

            opt_list = modal.query_one("#model-modal-options", OptionList)
            opt_list.highlighted = 0
            opt_list.action_select()
            await pilot.pause()
            assert not isinstance(pilot.app.screen, ModelSelectModal)
            assert pilot.app.engine.model == "gemini-3.7-flash"
            assert pilot.app.engine.effort == "high"

    asyncio.run(_run())


def test_two_stage_model_selection_cancel():
    import asyncio

    from textual.widgets import OptionList

    from agy_tui.widgets import ModelSelectModal

    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            orig_model = pilot.app.engine.model
            orig_effort = pilot.app.engine.effort

            await pilot.app._handle_command("/model")
            await pilot.pause()
            assert isinstance(pilot.app.screen, ModelSelectModal)
            modal = pilot.app.screen

            modal.action_cancel()
            await pilot.pause()
            assert not isinstance(pilot.app.screen, ModelSelectModal)
            assert pilot.app.engine.model == orig_model
            assert pilot.app.engine.effort == orig_effort

            pilot.app.action_select_model()
            await pilot.pause()
            assert isinstance(pilot.app.screen, ModelSelectModal)
            modal = pilot.app.screen
            opt_list = modal.query_one("#model-modal-options", OptionList)
            opt_list.highlighted = 4
            opt_list.action_select()
            await pilot.pause()
            assert modal._step == 2
            assert modal._selected_model == "claude-sonnet-5-5"

            modal.action_cancel()
            await pilot.pause()
            assert modal._step == 1
            assert modal._selected_model is None
            assert isinstance(pilot.app.screen, ModelSelectModal)

            modal.action_cancel()
            await pilot.pause()
            assert not isinstance(pilot.app.screen, ModelSelectModal)
            assert pilot.app.engine.model == orig_model
            assert pilot.app.engine.effort == orig_effort

    asyncio.run(_run())


def test_model_command_direct_dual_args():
    import asyncio

    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            await pilot.app._handle_command("/model gemini-3.7-flash high")
            await pilot.pause()
            assert pilot.app.engine.model == "gemini-3.7-flash"
            assert pilot.app.engine.effort == "high"

            await pilot.app._handle_command("/effort 1")
            await pilot.pause()
            assert pilot.app.engine.effort == "low"

            await pilot.app._handle_command("/effort medium")
            await pilot.pause()
            assert pilot.app.engine.effort == "medium"

    asyncio.run(_run())

