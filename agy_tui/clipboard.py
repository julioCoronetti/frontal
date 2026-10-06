import base64
import os
import shutil
import subprocess
import sys
import time
from typing import TextIO


class ClipboardManager:
    DEFAULT_CLIP_EXE = "/mnt/c/Windows/System32/clip.exe"

    def __init__(
        self,
        clip_exe_path: str | None = None,
        enable_osc52: bool = True,
        output_stream: TextIO | None = None,
    ) -> None:
        if clip_exe_path is not None:
            self._clip_exe = clip_exe_path if os.path.exists(clip_exe_path) else None
        elif os.path.exists(self.DEFAULT_CLIP_EXE):
            self._clip_exe = self.DEFAULT_CLIP_EXE
        else:
            self._clip_exe = shutil.which("clip.exe")
        self._enable_osc52 = enable_osc52
        self._output_stream = output_stream or sys.stdout
        self._fallback_buffer: str = ""
        self._clip_written: bool = False

    @property
    def clip_exe_available(self) -> bool:
        return self._clip_exe is not None and os.path.isfile(self._clip_exe)

    def encode_osc52(self, text: str) -> str:
        b64 = base64.b64encode(text.encode("utf-8")).decode("ascii")
        return f"\x1b]52;c;{b64}\x07"

    def copy_osc52(self, text: str) -> bool:
        if not self._enable_osc52:
            return False
        try:
            seq = self.encode_osc52(text)
            self._output_stream.write(seq)
            self._output_stream.flush()
            return True
        except Exception:
            return False

    def copy_clip_exe(self, text: str) -> bool:
        if not self.clip_exe_available or not self._clip_exe:
            return False
        for attempt in range(2):
            try:
                proc = subprocess.Popen(
                    [self._clip_exe],
                    stdin=subprocess.PIPE,
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                proc.communicate(input=text.encode("utf-8"), timeout=2.0)
                if proc.returncode == 0:
                    return True
            except subprocess.TimeoutExpired:
                try:
                    proc.kill()
                    proc.wait()
                except Exception:
                    pass
                return False
            except Exception:
                pass
            if attempt == 0:
                time.sleep(0.05)
        return False

    def copy(self, text: str) -> bool:
        self._fallback_buffer = text
        copied = False
        if self.clip_exe_available:
            copied = self.copy_clip_exe(text)
            self._clip_written = copied
        else:
            self._clip_written = False
        if self._enable_osc52:
            osc_ok = self.copy_osc52(text)
            copied = copied or osc_ok
        if not copied:
            if shutil.which("wl-copy"):
                try:
                    p = subprocess.Popen(
                        ["wl-copy"],
                        stdin=subprocess.PIPE,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    p.communicate(input=text.encode("utf-8"), timeout=2.0)
                    copied = p.returncode == 0
                except Exception:
                    pass
            elif shutil.which("xclip"):
                try:
                    p = subprocess.Popen(
                        ["xclip", "-selection", "clipboard"],
                        stdin=subprocess.PIPE,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                    p.communicate(input=text.encode("utf-8"), timeout=2.0)
                    copied = p.returncode == 0
                except Exception:
                    pass
        return copied or (self._fallback_buffer is not None)

    def read_clipboard(self, normalize_newlines: bool = False) -> str:
        if self.clip_exe_available and (self._clip_written or not self._fallback_buffer):
            powershell_bin = shutil.which("powershell.exe") or "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe"
            if os.path.exists(powershell_bin):
                ps_script = (
                    "for ($i = 0; $i -lt 5; $i++) { "
                    "try { "
                    "$val = Get-Clipboard -Raw -ErrorAction Stop; "
                    "if ($val -ne $null) { [Console]::Out.Write($val) }; "
                    "exit 0; "
                    "} catch { "
                    "Start-Sleep -Milliseconds 60; "
                    "} "
                    "}; "
                    "exit 1"
                )
                for _ in range(2):
                    try:
                        proc = subprocess.run(
                            [powershell_bin, "-NoProfile", "-NonInteractive", "-Command", ps_script],
                            stdin=subprocess.DEVNULL,
                            capture_output=True,
                            timeout=4.0,
                        )
                        if proc.returncode == 0:
                            out = proc.stdout.decode("utf-8", errors="replace")
                            if out or not self._fallback_buffer:
                                if normalize_newlines:
                                    return out.replace("\r\n", "\n")
                                return out
                    except Exception:
                        pass
                    time.sleep(0.05)
        if shutil.which("wl-paste"):
            try:
                proc = subprocess.run(
                    ["wl-paste", "-n"],
                    stdin=subprocess.DEVNULL,
                    capture_output=True,
                    timeout=2.0,
                )
                if proc.returncode == 0:
                    out = proc.stdout.decode("utf-8", errors="replace")
                    if out or not self._fallback_buffer:
                        if normalize_newlines:
                            return out.replace("\r\n", "\n")
                        return out
            except Exception:
                pass
        if shutil.which("xclip"):
            try:
                proc = subprocess.run(
                    ["xclip", "-selection", "clipboard", "-o"],
                    stdin=subprocess.DEVNULL,
                    capture_output=True,
                    timeout=2.0,
                )
                if proc.returncode == 0:
                    out = proc.stdout.decode("utf-8", errors="replace")
                    if out or not self._fallback_buffer:
                        if normalize_newlines:
                            return out.replace("\r\n", "\n")
                        return out
            except Exception:
                pass
        if normalize_newlines and self._fallback_buffer:
            return self._fallback_buffer.replace("\r\n", "\n")
        return self._fallback_buffer
