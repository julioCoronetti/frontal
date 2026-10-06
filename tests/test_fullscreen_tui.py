import asyncio
import os
import shutil

from textual.widgets import Static

from agy_tui.app import TUIApp
from agy_tui.clipboard import ClipboardManager
from agy_tui.diff import format_diff_from_tool
from agy_tui.session import SessionManager
from agy_tui.widgets import (
    AutocompletePopup,
    ChatInput,
    CodeBlockWidget,
    DiffViewWidget,
    ModelSelectModal,
    SidebarInspector,
)


def test_fullscreen_layout_and_sidebar_toggle():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sidebar = pilot.app.query_one("#sidebar", SidebarInspector)
            assert sidebar.display is True

            await pilot.press("ctrl+b")
            await pilot.pause()
            assert sidebar.display is False

            await pilot.press("ctrl+b")
            await pilot.pause()
            assert sidebar.display is True

            await pilot.app._handle_command("/sidebar")
            await pilot.pause()
            assert sidebar.display is False

            await pilot.app._handle_command("/sidebar")
            await pilot.pause()
            assert sidebar.display is True

    asyncio.run(_run())


def test_messages_viewport_scrolling():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")
            for i in range(50):
                await mv.mount(CodeBlockWidget(i + 1, "python", f"x_{i} = {i}\n"))
            await pilot.pause()

            mv.scroll_to(y=0, animate=False)
            await pilot.pause()
            assert mv.scroll_y == 0

            pilot.app.action_scroll_page_down()
            await pilot.pause()
            assert mv.scroll_y > 0

            pilot.app.action_scroll_page_up()
            await pilot.pause()
            assert mv.scroll_y == 0

    asyncio.run(_run())


def test_sidebar_telemetry_live_updates():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sidebar = pilot.app.query_one("#sidebar", SidebarInspector)
            await pilot.pause()
            assert sidebar.agent_status == "Idle"

            sidebar.update_telemetry(
                status="Running Tool: write_to_file",
                model="claude-sonnet-4-6",
                mode="architect",
                effort="high",
                tokens=15420,
                tool_step="write_to_file (DONE)",
                subagents=["implementer@swe_light"],
                conversation_id="conv-uuid-telemetry-test",
            )
            await pilot.pause()

            assert sidebar.agent_status == "Running Tool: write_to_file"
            assert sidebar.model_name == "claude-sonnet-4-6"
            assert sidebar.mode_name == "architect"
            assert sidebar.effort_level == "high"
            assert sidebar.tokens_used == 15420
            assert sidebar.tool_steps_count == 1
            assert sidebar.subagents_count == 1
            assert "conv-uuid" in sidebar.conversation_id

    asyncio.run(_run())


def test_code_block_interactive_copy_button():
    async def _run():
        clipboard = ClipboardManager()
        app = TUIApp(clipboard=clipboard)
        test_code = (
            "def compute_metrics(records: list[dict]) -> dict:\n"
            "    valid = [r for r in records if r.get('active')]\n"
            "    return {\n"
            "        'count': len(valid),\n"
            "        'ratio': len(valid) / max(1, len(records)),\n"
            "    }\n"
        )

        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")
            widget = CodeBlockWidget(block_id=1, language="python", code=test_code)
            await mv.mount(widget)
            await pilot.pause()

            btn = widget.query_one(".copy-btn")
            btn.press()
            await pilot.pause()

            assert btn.label == "✓ Copied!"

            powershell_bin = shutil.which("powershell.exe") or "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"
            if os.path.exists(powershell_bin) and clipboard.clip_exe_available:
                retrieved = clipboard.read_clipboard(normalize_newlines=True)
                assert retrieved == test_code
            else:
                assert clipboard.read_clipboard() == test_code

    asyncio.run(_run())


def test_model_mode_effort_commands_reactive():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sidebar = pilot.app.query_one("#sidebar", SidebarInspector)

            await pilot.app._handle_command("/model claude-sonnet-4-6")
            await pilot.pause()
            assert pilot.app.engine.model == "claude-sonnet-4-6"
            assert sidebar.model_name == "claude-sonnet-4-6"

            await pilot.app._handle_command("/mode plan")
            await pilot.pause()
            assert pilot.app.engine.mode == "plan"
            assert sidebar.mode_name == "plan"

            await pilot.app._handle_command("/effort high")
            await pilot.pause()
            assert pilot.app.engine.effort == "high"
            assert sidebar.effort_level == "high"

    asyncio.run(_run())


def test_tool_diff_generation_and_view():
    async def _run():
        diff_rec = format_diff_from_tool(
            tool_name="replace_file_content",
            args={
                "TargetFile": "/workspace/app.py",
                "TargetContent": "def old():\n    return False\n",
                "ReplacementContent": "def old():\n    return True\n",
            },
        )
        assert diff_rec is not None
        assert diff_rec.file_path == "/workspace/app.py"
        assert "-    return False" in diff_rec.diff_text
        assert "+    return True" in diff_rec.diff_text

        app = TUIApp()
        app.diff_registry.add(diff_rec)
        async with app.run_test(size=(100, 30)) as pilot:
            await pilot.app._handle_command("/diff")
            await pilot.pause()

            mv = pilot.app.query_one("#messages-view")
            diff_widgets = [w for w in mv.children if isinstance(w, DiffViewWidget)]
            assert len(diff_widgets) >= 1
            assert diff_widgets[-1].record.file_path == "/workspace/app.py"

    asyncio.run(_run())


def test_autocomplete_popup_trigger_and_selection():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            popup = pilot.app.query_one("#autocomplete-popup", AutocompletePopup)
            inp = pilot.app.query_one("#chat-input", ChatInput)

            assert popup.has_class("hidden")

            inp.text = "/mod"
            await pilot.pause()
            assert not popup.has_class("hidden")
            assert popup.option_count >= 2

            popup.highlighted = 0
            cmd = popup.get_selected_command()
            assert cmd in ("/model", "/mode")

            inp.text = "/model "
            await pilot.pause()
            assert popup.has_class("hidden")

    asyncio.run(_run())


def test_session_manager_and_resume():
    async def _run():
        test_storage = os.path.join(os.path.dirname(__file__), ".test_sessions.json")
        mgr = SessionManager(storage_path=test_storage)
        mgr.clear()

        mgr.record("conv-alpha-1", model="gemini-3.8-flash-high", mode="code", prompt="First task")
        mgr.record("conv-beta-2", model="claude-sonnet-4-6", mode="plan", prompt="Second task")

        sessions = mgr.list_sessions()
        assert len(sessions) == 2
        assert sessions[0].conversation_id == "conv-beta-2"

        found = mgr.get_session("conv-alpha-1")
        assert found is not None
        assert found.last_prompt == "First task"

        by_idx = mgr.get_session("2")
        assert by_idx is not None
        assert by_idx.conversation_id == "conv-alpha-1"

        app = TUIApp()
        app.session_manager = mgr
        async with app.run_test(size=(100, 30)) as pilot:
            await pilot.app._handle_command("/resume conv-alpha-1")
            await pilot.pause()
            assert pilot.app.engine.conversation_id == "conv-alpha-1"

        mgr.clear()

    asyncio.run(_run())


def test_chat_input_multiline_and_history():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            inp = pilot.app.query_one("#chat-input", ChatInput)
            pilot.app.history.add("first command")
            pilot.app.history.add("second command")

            inp.text = ""
            pilot.app.on_history_navigation(ChatInput.HistoryNavigation("up"))
            assert inp.text == "second command"

            pilot.app.on_history_navigation(ChatInput.HistoryNavigation("up"))
            assert inp.text == "first command"

            pilot.app.on_history_navigation(ChatInput.HistoryNavigation("down"))
            assert inp.text == "second command"

    asyncio.run(_run())


def test_autocomplete_keyboard_navigation_and_selection():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            popup = pilot.app.query_one("#autocomplete-popup", AutocompletePopup)
            inp = pilot.app.query_one("#chat-input", ChatInput)
            inp.text = "/m"
            await pilot.pause()
            assert not popup.has_class("hidden")

            await pilot.press("down")
            await pilot.pause()
            assert popup.highlighted == 0

            await pilot.press("down")
            await pilot.pause()
            assert popup.highlighted == 1

            await pilot.press("up")
            await pilot.pause()
            assert popup.highlighted == 0

            await pilot.press("tab")
            await pilot.pause()
            assert popup.has_class("hidden")
            assert inp.text.startswith("/") and len(inp.text) > 2

            inp.text = "/e"
            await pilot.pause()
            assert not popup.has_class("hidden")

            await pilot.press("escape")
            await pilot.pause()
            assert popup.has_class("hidden")

    asyncio.run(_run())


def test_chat_input_multiline_ctrl_enter_and_page_scroll():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            inp = pilot.app.query_one("#chat-input", ChatInput)
            mv = pilot.app.query_one("#messages-view")

            for i in range(50):
                await mv.mount(CodeBlockWidget(i + 1, "python", f"val_{i} = {i}\n"))
            await pilot.pause()
            mv.scroll_to(y=0, animate=False)
            await pilot.pause()
            assert mv.scroll_y == 0

            inp.focus()
            await pilot.press("pagedown")
            await pilot.pause()
            assert mv.scroll_y > 0

            await pilot.press("pageup")
            await pilot.pause()
            assert mv.scroll_y == 0

            inp.multiline_submit_ctrl_enter = True
            inp.text = "line1"
            inp.cursor_location = (0, len(inp.text))
            await pilot.press("enter")
            await pilot.pause()
            assert "line1\n" in inp.text

            await pilot.press("ctrl+enter")
            await pilot.pause()
            assert inp.text == ""
            assert "line1" in pilot.app.history.get_all()[-1]

    asyncio.run(_run())


def test_streaming_markdown_with_code_blocks_inline_no_duplication():
    async def _run():
        from agy_tui.protocol import StreamEvent

        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")

            ev_chunk = StreamEvent(
                event_type="step_update",
                raw={
                    "step_update": {
                        "step_index": 1,
                        "step_type": "agent_response",
                        "state": "ACTIVE",
                        "text_delta": "Preceding text\n\n```python\nsecret = 1337\n```\n\nTrailing text\n",
                    }
                },
            )
            pilot.app._dispatch_event(ev_chunk)
            await pilot.pause()

            ev_done = StreamEvent(
                event_type="step_update",
                raw={
                    "step_update": {
                        "step_index": 1,
                        "step_type": "agent_response",
                        "state": "DONE",
                    }
                },
            )
            pilot.app._dispatch_event(ev_done)
            await pilot.pause()

            code_widgets = [w for w in mv.children if isinstance(w, CodeBlockWidget)]
            assert len(code_widgets) == 1
            assert code_widgets[0].code == "secret = 1337\n"

            btn = code_widgets[0].query_one(".copy-btn")
            btn.press()
            await pilot.pause()
            assert btn.label == "✓ Copied!"

    asyncio.run(_run())


def test_turn_active_busy_guard_and_resume_guard():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            pilot.app.turn_active = True
            await pilot.app._handle_input_submission("hello when busy")
            await pilot.pause()

            await pilot.app._handle_command("/resume conv-busy-test")
            await pilot.pause()
            assert pilot.app.engine.conversation_id != "conv-busy-test"
            pilot.app.turn_active = False

    asyncio.run(_run())


def test_tool_diff_alternative_keys_and_sidebar_subagents():
    async def _run():
        rec1 = format_diff_from_tool(
            tool_name="write_file",
            args={"path": "config.json", "content": "{\n  \"active\": true\n}\n"},
        )
        assert rec1 is not None
        assert rec1.file_path == "config.json"
        assert "+  \"active\": true" in rec1.diff_text

        rec2 = format_diff_from_tool(
            tool_name="create_script",
            args={"file_path": "script.sh", "text": "#!/bin/bash\necho hi\n"},
        )
        assert rec2 is not None
        assert rec2.file_path == "script.sh"

        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sb = pilot.app.query_one("#sidebar", SidebarInspector)
            sb.update_telemetry(subagents=["auditor@swe", "sentinel@swe"])
            await pilot.pause()
            assert sb.subagents_count == 2
            list_text = sb.query_one("#subagents-list").render()
            assert "auditor@swe" in str(list_text)

    asyncio.run(_run())


def test_clear_command_resets_mounted_code_blocks():
    async def _run():
        from agy_tui.protocol import StreamEvent

        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")
            ev1 = StreamEvent(
                event_type="step_update",
                raw={
                    "step_update": {
                        "step_index": 1,
                        "step_type": "agent_response",
                        "state": "DONE",
                        "text_delta": "```python\nx = 42\n```\n",
                    }
                },
            )
            pilot.app._dispatch_event(ev1)
            await pilot.pause()

            assert len(pilot.app._mounted_code_blocks) > 0
            code_widgets = [w for w in mv.children if isinstance(w, CodeBlockWidget)]
            assert len(code_widgets) == 1

            await pilot.app._handle_command("/clear")
            await pilot.pause()

            assert len(pilot.app._mounted_code_blocks) == 0

            ev2 = StreamEvent(
                event_type="step_update",
                raw={
                    "step_update": {
                        "step_index": 2,
                        "step_type": "agent_response",
                        "state": "DONE",
                        "text_delta": "```python\nx = 42\n```\n",
                    }
                },
            )
            pilot.app._dispatch_event(ev2)
            await pilot.pause()

            code_widgets2 = [w for w in mv.children if isinstance(w, CodeBlockWidget)]
            assert len(code_widgets2) == 1

    asyncio.run(_run())


def test_resume_updates_effort_telemetry():
    async def _run():
        test_storage = os.path.join(os.path.dirname(__file__), ".test_sessions_effort.json")
        mgr = SessionManager(storage_path=test_storage)
        mgr.clear()
        mgr.record("conv-effort-spec", model="claude-sonnet-4-6", mode="plan", effort="high", prompt="Effort task")

        app = TUIApp()
        app.session_manager = mgr
        async with app.run_test(size=(100, 30)) as pilot:
            sb = pilot.app.query_one("#sidebar", SidebarInspector)
            assert sb.effort_level == "medium"

            await pilot.app._handle_command("/resume conv-effort-spec")
            await pilot.pause()

            assert pilot.app.engine.effort == "high"
            assert sb.effort_level == "high"

        mgr.clear()

    asyncio.run(_run())


def test_active_turn_guards_config_commands():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            pilot.app.turn_active = True

            orig_model = pilot.app.engine.model
            await pilot.app._handle_command("/model claude-sonnet-4-6")
            await pilot.pause()
            assert pilot.app.engine.model == orig_model

            orig_mode = pilot.app.engine.mode
            await pilot.app._handle_command("/mode architect")
            await pilot.pause()
            assert pilot.app.engine.mode == orig_mode

            orig_effort = pilot.app.engine.effort
            await pilot.app._handle_command("/effort high")
            await pilot.pause()
            assert pilot.app.engine.effort == orig_effort

            pilot.app.action_select_model()
            await pilot.pause()
            assert not isinstance(pilot.app.screen, ModelSelectModal)

            pilot.app.turn_active = False

    asyncio.run(_run())


def test_model_select_modal_hotkey_and_esc_backtrack():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            await pilot.press("ctrl+m")
            await pilot.pause()
            assert isinstance(pilot.app.screen, ModelSelectModal)
            modal = pilot.app.screen
            assert modal._step == 1

            opt_list = modal.query_one("#model-modal-options")
            opt_list.highlighted = 2
            opt_list.action_select()
            await pilot.pause()
            assert modal._step == 2
            assert modal._selected_model == "gemini-3.6-flash"

            await pilot.press("escape")
            await pilot.pause()
            assert isinstance(pilot.app.screen, ModelSelectModal)
            assert modal._step == 1
            assert modal._selected_model is None

            await pilot.press("escape")
            await pilot.pause()
            assert not isinstance(pilot.app.screen, ModelSelectModal)

    asyncio.run(_run())


def test_model_switch_adjusts_context_limit():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sb = pilot.app.query_one("#sidebar", SidebarInspector)

            sb.update_telemetry(model="claude-sonnet-4-6")
            await pilot.pause()
            assert sb.context_limit == 200000

            sb.update_telemetry(model="gemini-3.8-flash-high")
            await pilot.pause()
            assert sb.context_limit == 1000000

            sb.update_telemetry(model="gpt-oss-120b-medium")
            await pilot.pause()
            assert sb.context_limit == 128000

    asyncio.run(_run())


def test_multiline_history_navigation_cursor_position():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            inp = pilot.app.query_one("#chat-input", ChatInput)
            pilot.app.history.add("line_a\nline_b_longer\nline_c")

            inp.text = ""
            pilot.app.on_history_navigation(ChatInput.HistoryNavigation("up"))
            assert inp.text == "line_a\nline_b_longer\nline_c"
            assert inp.cursor_location == (2, len("line_c"))

    asyncio.run(_run())


def test_chat_input_newline_keycode_alias():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            inp = pilot.app.query_one("#chat-input", ChatInput)
            inp.multiline_submit_ctrl_enter = True
            inp.text = "prompt_line"

            from textual.events import Key
            ev = Key(key="newline", character="\n")
            inp._on_key(ev)
            await pilot.pause()
            pilot.app._turn_done_event.set()
            await pilot.pause()

            assert inp.text == ""
            assert "prompt_line" in pilot.app.history.get_all()[-1]

    asyncio.run(_run())


def test_subagents_telemetry_resets_on_error_and_interrupt():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sb = pilot.app.query_one("#sidebar", SidebarInspector)
            sb.update_telemetry(subagents=["worker_alpha"])
            await pilot.pause()
            assert sb.subagents_count == 1

            pilot.app.turn_active = True
            await pilot.app.action_interrupt_turn()
            await pilot.pause()
            assert sb.subagents_count == 0
            assert "None" in str(sb.query_one("#subagents-list").render())

            sb.update_telemetry(subagents=["worker_beta"])
            await pilot.pause()
            assert sb.subagents_count == 1

            from agy_tui.protocol import StreamEvent
            err_ev = StreamEvent(
                event_type="error",
                raw={"event": "error", "error": "fatal subprocess fault"},
            )
            pilot.app._dispatch_event(err_ev)
            await pilot.pause()
            assert sb.subagents_count == 0
            assert "None" in str(sb.query_one("#subagents-list").render())

    asyncio.run(_run())


def test_subagents_empty_list_clears_active_workers_during_stream():
    async def _run():
        from agy_tui.protocol import StreamEvent

        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            sb = pilot.app.query_one("#sidebar", SidebarInspector)
            ev_active = StreamEvent(
                event_type="step_update",
                raw={
                    "step_update": {
                        "step_index": 1,
                        "step_type": "agent_response",
                        "state": "ACTIVE",
                        "subagents": ["auditor@swe"],
                    }
                },
            )
            pilot.app._dispatch_event(ev_active)
            await pilot.pause()
            assert sb.subagents_count == 1

            ev_cleared = StreamEvent(
                event_type="step_update",
                raw={
                    "step_update": {
                        "step_index": 2,
                        "step_type": "agent_response",
                        "state": "ACTIVE",
                        "subagents": [],
                    }
                },
            )
            pilot.app._dispatch_event(ev_cleared)
            await pilot.pause()
            assert sb.subagents_count == 0
            assert "None" in str(sb.query_one("#subagents-list").render())

    asyncio.run(_run())


def test_token_meter_ascii_fallback_mode():
    sb = SidebarInspector()
    sb.tokens_used = 50000
    sb.context_limit = 100000
    meter_unicode = sb._build_token_meter(ascii_fallback=False)
    assert "█" in meter_unicode
    assert "░" in meter_unicode

    meter_ascii = sb._build_token_meter(ascii_fallback=True)
    assert "#" in meter_ascii
    assert "-" in meter_ascii
    assert "█" not in meter_ascii
    assert "░" not in meter_ascii

    os.environ["AGY_ASCII"] = "1"
    meter_env = sb._build_token_meter()
    assert "#" in meter_env
    assert "█" not in meter_env
    os.environ.pop("AGY_ASCII", None)


def test_autocomplete_tab_completion_when_highlighted_is_none():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            popup = pilot.app.query_one("#autocomplete-popup", AutocompletePopup)
            inp = pilot.app.query_one("#chat-input", ChatInput)

            inp.text = "/diff"
            await pilot.pause()
            assert not popup.has_class("hidden")
            assert popup.highlighted is None

            await pilot.press("tab")
            await pilot.pause()
            assert popup.has_class("hidden")
            assert inp.text == "/diff "

    asyncio.run(_run())


def test_diff_command_deduplication_and_list_subcommand():
    async def _run():
        from agy_tui.protocol import StreamEvent

        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")
            tool_raw = {
                "step_update": {
                    "step_index": 1,
                    "step_type": "tool_execution",
                    "state": "ACTIVE",
                    "tool_name": "replace_file_content",
                    "args": {
                        "TargetFile": "src/core.py",
                        "TargetContent": "v = 1\n",
                        "ReplacementContent": "v = 2\n",
                    },
                }
            }
            ev1 = StreamEvent(event_type="step_update", raw=tool_raw)
            ev2 = StreamEvent(event_type="step_update", raw=tool_raw)

            pilot.app._dispatch_event(ev1)
            await pilot.pause()
            pilot.app._dispatch_event(ev2)
            await pilot.pause()

            diff_widgets = [w for w in mv.children if isinstance(w, DiffViewWidget)]
            assert len(diff_widgets) == 1
            assert len(pilot.app.diff_registry.get_all()) == 1

            await pilot.app._handle_command("/diff list")
            await pilot.pause()
            assert any("Recorded Diffs" in str(getattr(getattr(w.render(), "_renderable", None), "renderable", str(w.render()))) for w in mv.children if isinstance(w, Static))

    asyncio.run(_run())


def test_streaming_empty_delta_no_op():
    async def _run():
        app = TUIApp()
        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")
            initial_count = len(mv.children)
            pilot.app._render_stream_delta("")
            await pilot.pause()
            assert len(mv.children) == initial_count
            assert pilot.app._active_stream_widget is None

    asyncio.run(_run())


def test_code_block_copy_button_failure_feedback():
    async def _run():
        class FailingClipboard:
            def copy(self, text: str) -> bool:
                return False

        app = TUIApp(clipboard=FailingClipboard())
        async with app.run_test(size=(100, 30)) as pilot:
            mv = pilot.app.query_one("#messages-view")
            widget = CodeBlockWidget(block_id=1, language="python", code="x = 10\n")
            await mv.mount(widget)
            await pilot.pause()

            btn = widget.query_one(".copy-btn")
            btn.press()
            await pilot.pause()
            assert btn.label == "✗ Failed"

    asyncio.run(_run())
