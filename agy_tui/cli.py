import argparse
import asyncio
import sys

from rich.console import Console

from agy_tui.app import TUIApp
from agy_tui.clipboard import ClipboardManager
from agy_tui.engine import AgyEngine
from agy_tui.renderer import MarkdownRenderer


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="agy-tui",
        description="Decoupled high-performance TUI frontend for Antigravity CLI (agy)",
    )
    parser.add_argument(
        "--agy-bin",
        type=str,
        default=None,
        help="Path to agy executable",
    )
    parser.add_argument(
        "--conversation",
        "-c",
        type=str,
        default=None,
        help="Explicit conversation ID for session isolation or resuming",
    )
    parser.add_argument(
        "--project",
        type=str,
        default=None,
        help="Project ID or project name",
    )
    parser.add_argument(
        "--mode",
        type=str,
        default=None,
        choices=["code", "plan", "architect", "ask", "accept-edits"],
        help="Execution mode for the session",
    )
    parser.add_argument(
        "--effort",
        type=str,
        default=None,
        choices=["low", "medium", "high"],
        help="Reasoning effort level (low, medium, high)",
    )
    parser.add_argument(
        "--cwd",
        type=str,
        default=None,
        help="Working directory for the agy process",
    )
    parser.add_argument(
        "--no-osc52",
        action="store_true",
        help="Disable OSC 52 clipboard sequences",
    )
    parser.add_argument(
        "--fullscreen",
        action="store_true",
        help="Force fullscreen alternate-screen mode",
    )
    return parser


def main(args: list[str] | None = None) -> int:
    parser = build_parser()
    parsed, extra_args = parser.parse_known_args(args)

    engine = AgyEngine(
        agy_bin=parsed.agy_bin,
        conversation_id=parsed.conversation,
        project=parsed.project,
        mode=parsed.mode,
        effort=parsed.effort,
        cwd=parsed.cwd,
        extra_args=extra_args,
    )

    clipboard = ClipboardManager(enable_osc52=not parsed.no_osc52)
    renderer = MarkdownRenderer()
    console = Console()

    app = TUIApp(
        engine=engine,
        renderer=renderer,
        clipboard=clipboard,
        console=console,
    )
    if parsed.fullscreen:
        app._force_fullscreen = True

    try:
        asyncio.run(app.run())
        return 0
    except (KeyboardInterrupt, SystemExit):
        return 0
    except Exception as ex:
        Console(stderr=True).print(f"[bold red]Fatal error:[/bold red] {ex}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
