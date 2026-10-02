import asyncio
import os
import shutil
import signal
import sys
import uuid
from collections.abc import AsyncGenerator

from agy_tui.protocol import StreamEvent, format_user_message, parse_stream_line


class AgyEngine:
    DEFAULT_AGY_PATH = "/home/julio_regino/.local/bin/agy"

    def __init__(
        self,
        agy_bin: str | None = None,
        conversation_id: str | None = None,
        project: str | None = None,
        model: str | None = None,
        mode: str | None = None,
        effort: str | None = None,
        cwd: str | None = None,
        extra_args: list[str] | None = None,
    ) -> None:
        if agy_bin:
            self._agy_bin = agy_bin
        elif shutil.which("agy"):
            self._agy_bin = shutil.which("agy") or "agy"
        elif os.path.exists(self.DEFAULT_AGY_PATH):
            self._agy_bin = self.DEFAULT_AGY_PATH
        else:
            self._agy_bin = "agy"

        self.conversation_id: str = conversation_id or str(uuid.uuid4())
        self.project: str | None = project
        self.model: str | None = model
        self.mode: str | None = mode
        self.effort: str | None = effort
        self.cwd: str | None = cwd
        self.extra_args: list[str] = extra_args or []

        if not self.model and "--model" in self.extra_args:
            try:
                idx = self.extra_args.index("--model")
                if idx + 1 < len(self.extra_args):
                    self.model = self.extra_args[idx + 1]
            except Exception:
                pass

        self._process: asyncio.subprocess.Process | None = None
        self._stdout_task: asyncio.Task | None = None
        self._stderr_task: asyncio.Task | None = None
        self.events_queue: asyncio.Queue[StreamEvent | None] = asyncio.Queue()
        self.stderr_lines: list[str] = []
        self._stopped: bool = False

    @property
    def is_running(self) -> bool:
        return self._process is not None and self._process.returncode is None

    @property
    def returncode(self) -> int | None:
        if self._process:
            return self._process.returncode
        return None

    def build_command(self) -> list[str]:
        cmd = [sys.executable, self._agy_bin] if self._agy_bin.endswith(".py") else [self._agy_bin]
        cmd.extend([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--conversation",
            self.conversation_id,
        ])
        if self.project:
            cmd.extend(["--project", self.project])
        if self.model and "--model" not in self.extra_args:
            cmd.extend(["--model", self.model])
        if self.mode:
            cmd.extend(["--mode", self.mode])
        if self.effort:
            cmd.extend(["--effort", self.effort])
        cmd.extend(self.extra_args)
        return cmd

    async def restart(self) -> None:
        await self.stop(timeout=1.0)
        await self.start()

    async def start(self) -> None:
        if self.is_running:
            return
        if self._stdout_task and not self._stdout_task.done():
            self._stdout_task.cancel()
            try:
                await self._stdout_task
            except asyncio.CancelledError:
                pass
        if self._stderr_task and not self._stderr_task.done():
            self._stderr_task.cancel()
            try:
                await self._stderr_task
            except asyncio.CancelledError:
                pass
        if self._process:
            try:
                await self._process.wait()
            except Exception:
                pass
        while not self.events_queue.empty():
            try:
                self.events_queue.get_nowait()
            except asyncio.QueueEmpty:
                break
        cmd = self.build_command()
        self._stopped = False
        self._process = await asyncio.create_subprocess_exec(
            *cmd,
            stdin=asyncio.subprocess.PIPE,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
            cwd=self.cwd,
            start_new_session=True,
        )
        self._stdout_task = asyncio.create_task(self._read_stdout())
        self._stderr_task = asyncio.create_task(self._read_stderr())

    async def _read_stdout(self) -> None:
        if not self._process or not self._process.stdout:
            return
        reader = self._process.stdout
        try:
            while not reader.at_eof():
                line = await reader.readline()
                if not line:
                    break
                event = parse_stream_line(line)
                if event is not None:
                    if event.event_type == "init" and event.conversation_id:
                        self.conversation_id = event.conversation_id
                    await self.events_queue.put(event)
            await self.events_queue.put(None)
        except asyncio.CancelledError:
            pass
        except Exception:
            pass

    async def _read_stderr(self) -> None:
        if not self._process or not self._process.stderr:
            return
        reader = self._process.stderr
        while not reader.at_eof():
            try:
                line = await reader.readline()
                if not line:
                    break
                decoded = line.decode("utf-8", errors="replace").rstrip()
                if decoded:
                    self.stderr_lines.append(decoded)
            except asyncio.CancelledError:
                break
            except Exception:
                pass

    async def ensure_running(self) -> None:
        if self._stopped:
            return
        if not self.is_running:
            await self.start()

    async def send_prompt(self, prompt: str) -> None:
        if not self.is_running:
            await self.ensure_running()
        if not self.is_running or not self._process or not self._process.stdin:
            raise RuntimeError("Engine process is not running")
        formatted = format_user_message(prompt)
        try:
            self._process.stdin.write(formatted.encode("utf-8"))
            await self._process.stdin.drain()
        except (BrokenPipeError, ConnectionResetError):
            await self.ensure_running()
            if not self.is_running or not self._process or not self._process.stdin:
                raise RuntimeError("Engine process could not be restarted")
            self._process.stdin.write(formatted.encode("utf-8"))
            await self._process.stdin.drain()

    async def interrupt_current_turn(self) -> None:
        if not self.is_running or not self._process:
            return
        try:
            pgid = os.getpgid(self._process.pid)
            os.killpg(pgid, signal.SIGINT)
        except ProcessLookupError:
            pass
        except Exception:
            try:
                self._process.send_signal(signal.SIGINT)
            except Exception:
                pass

    async def stop(self, timeout: float = 3.0) -> int:
        if self._stopped:
            return self.returncode or 0
        self._stopped = True

        if self._process:
            if self._process.stdin and not self._process.stdin.is_closing():
                try:
                    self._process.stdin.close()
                    await self._process.stdin.wait_closed()
                except Exception:
                    pass

            if self.is_running:
                try:
                    await asyncio.wait_for(self._process.wait(), timeout=timeout)
                except TimeoutError:
                    try:
                        pgid = os.getpgid(self._process.pid)
                        os.killpg(pgid, signal.SIGTERM)
                    except Exception:
                        self._process.terminate()

                    try:
                        await asyncio.wait_for(self._process.wait(), timeout=1.0)
                    except TimeoutError:
                        try:
                            pgid = os.getpgid(self._process.pid)
                            os.killpg(pgid, signal.SIGKILL)
                        except Exception:
                            self._process.kill()
                        await self._process.wait()

        if self._stdout_task and not self._stdout_task.done():
            self._stdout_task.cancel()
            try:
                await self._stdout_task
            except asyncio.CancelledError:
                pass

        if self._stderr_task and not self._stderr_task.done():
            self._stderr_task.cancel()
            try:
                await self._stderr_task
            except asyncio.CancelledError:
                pass

        return self.returncode or 0

    async def iter_events(self) -> AsyncGenerator[StreamEvent, None]:
        while True:
            event = await self.events_queue.get()
            if event is None:
                break
            yield event
