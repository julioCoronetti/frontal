import asyncio
import os

from agy_tui.engine import AgyEngine


def test_engine_command_isolation():
    engine1 = AgyEngine(agy_bin="/usr/bin/agy", project="proj-alpha", mode="plan", effort="high")
    cmd1 = engine1.build_command()
    assert "--conversation" in cmd1
    conv_idx1 = cmd1.index("--conversation")
    conv_id1 = cmd1[conv_idx1 + 1]
    assert conv_id1 == engine1.conversation_id
    assert "--project" in cmd1
    assert "--mode" in cmd1
    assert "--effort" in cmd1

    engine2 = AgyEngine(agy_bin="/usr/bin/agy")
    cmd2 = engine2.build_command()
    conv_idx2 = cmd2.index("--conversation")
    conv_id2 = cmd2[conv_idx2 + 1]

    assert conv_id1 != conv_id2


def test_engine_lifecycle_and_streaming():
    async def _run():
        mock_script = os.path.join(os.path.dirname(__file__), "mock_engine.py")
        engine = AgyEngine(
            agy_bin=mock_script,
            conversation_id="session-isolated-123",
        )

        await engine.start()
        assert engine.is_running

        init_event = await asyncio.wait_for(engine.events_queue.get(), timeout=2.0)
        assert init_event is not None
        assert init_event.event_type == "init"
        assert init_event.conversation_id == "session-isolated-123"

        await engine.send_prompt("calculate 2+2")

        collected_deltas = []
        received_thought = False
        result_event = None

        while True:
            event = await asyncio.wait_for(engine.events_queue.get(), timeout=3.0)
            if event is None:
                break
            if event.thought_text:
                received_thought = True
            if event.text_delta:
                collected_deltas.append(event.text_delta)
            if event.is_done:
                result_event = event
                break

        assert received_thought is True
        full_text = "".join(collected_deltas)
        assert "Response to: calculate 2+2" in full_text
        assert result_event is not None
        assert result_event.result_status == "SUCCESS"

        returncode = await engine.stop(timeout=2.0)
        assert not engine.is_running
        assert returncode == 0
        assert engine.returncode is not None

    asyncio.run(_run())


def test_engine_interruption():
    async def _run():
        mock_script = os.path.join(os.path.dirname(__file__), "mock_engine.py")
        engine = AgyEngine(
            agy_bin=mock_script,
            conversation_id="session-interrupt-test",
        )

        await engine.start()
        init_event = await asyncio.wait_for(engine.events_queue.get(), timeout=2.0)
        assert init_event is not None

        await engine.send_prompt("SLOW")
        await asyncio.sleep(0.1)

        await engine.interrupt_current_turn()

        interrupted_result = None
        for _ in range(20):
            event = await asyncio.wait_for(engine.events_queue.get(), timeout=2.0)
            if event is not None and event.is_done:
                interrupted_result = event
                break

        assert interrupted_result is not None
        assert interrupted_result.result_error == "Turn interrupted by user"

        await engine.stop(timeout=2.0)
        assert not engine.is_running

    asyncio.run(_run())


def test_engine_recovery_preserves_session():
    async def _run():
        mock_script = os.path.join(os.path.dirname(__file__), "mock_engine.py")
        engine = AgyEngine(
            agy_bin=mock_script,
            conversation_id="session-recovery-abc",
        )

        await engine.start()
        ev_init = await asyncio.wait_for(engine.events_queue.get(), timeout=2.0)
        assert ev_init is not None
        assert engine.conversation_id == "session-recovery-abc"

        if engine._process:
            engine._process.terminate()
            await engine._process.wait()
        assert not engine.is_running

        await engine.send_prompt("recover and continue")
        assert engine.is_running
        assert engine.conversation_id == "session-recovery-abc"

        ev_res = None
        for _ in range(20):
            event = await asyncio.wait_for(engine.events_queue.get(), timeout=2.0)
            if event is not None and event.is_done:
                ev_res = event
                break

        assert ev_res is not None
        assert ev_res.result_status == "SUCCESS"

        await engine.stop(timeout=2.0)

    asyncio.run(_run())


def test_engine_captures_init_conversation_id():
    async def _run():
        mock_script = os.path.join(os.path.dirname(__file__), "mock_engine.py")
        engine = AgyEngine(
            agy_bin=mock_script,
            conversation_id="initial-uuid-1",
        )
        await engine.start()
        ev = await asyncio.wait_for(engine.events_queue.get(), timeout=2.0)
        assert ev is not None
        assert engine.conversation_id == "initial-uuid-1"
        await engine.stop(timeout=2.0)

    asyncio.run(_run())
