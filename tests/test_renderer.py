from rich.panel import Panel

from agy_tui.renderer import CodeBlockRegistry, MarkdownRenderer


def test_code_block_registry():
    registry = CodeBlockRegistry()
    b1 = registry.register("python", "x = 10\ny = 20\n")
    assert b1.id == 1
    assert b1.language == "python"
    assert b1.code == "x = 10\ny = 20\n"

    b2 = registry.register("", "raw text")
    assert b2.id == 2
    assert b2.language == "text"

    assert registry.get(1) == b1
    assert registry.get(2) == b2
    assert registry.get(3) is None
    assert len(registry.all_blocks()) == 2

    registry.clear()
    assert len(registry.all_blocks()) == 0


def test_extract_code_blocks_byte_exact():
    original_code = (
        "def complex_algorithm(items: list[int]) -> int:\n"
        "    # Indented logic\n"
        "    total = 0\n"
        "    for item in items:\n"
        "        if item % 2 == 0:\n"
        "            total += item\n"
        "    return total\n"
    )
    markdown_text = f"Here is the code:\n\n```python\n{original_code}```\n\nHope this helps!"
    renderer = MarkdownRenderer()
    blocks = renderer.extract_and_register_code_blocks(markdown_text)

    assert len(blocks) == 1
    assert blocks[0].language == "python"
    assert blocks[0].code == original_code


def test_code_block_no_hard_wrap():
    very_long_line = "a" * 300 + " = 1"
    code = f"x = 1\n{very_long_line}\ny = 2\n"
    markdown_text = f"```python\n{code}```"
    renderer = MarkdownRenderer()
    blocks = renderer.extract_and_register_code_blocks(markdown_text)

    assert len(blocks) == 1
    assert "\n" not in very_long_line
    lines = blocks[0].code.splitlines()
    assert len(lines) == 3
    assert lines[1] == very_long_line


def test_render_thought_panel():
    renderer = MarkdownRenderer()
    panel = renderer.render_thought("Analyzing requirements...", is_collapsed=False)
    assert isinstance(panel, Panel)
    assert panel.border_style == "dim cyan"

    collapsed_panel = renderer.render_thought("Analyzing...", is_collapsed=True)
    assert isinstance(collapsed_panel, Panel)


def test_render_step_panel():
    renderer = MarkdownRenderer()
    panel_active = renderer.render_step(step_type="browser_action", state="ACTIVE", step_index=2)
    assert isinstance(panel_active, Panel)
    assert panel_active.border_style == "yellow"

    panel_done = renderer.render_step(step_type="tool_execution", state="DONE", step_index=3, duration=0.85)
    assert isinstance(panel_done, Panel)


def test_render_result_panel():
    renderer = MarkdownRenderer()
    panel_success = renderer.render_result(status="SUCCESS", duration=2.1)
    assert isinstance(panel_success, Panel)
    assert panel_success.border_style == "green"

    panel_error = renderer.render_result(status="ERROR", duration=0.5, error_message="Connection aborted")
    assert isinstance(panel_error, Panel)
    assert panel_error.border_style == "red"

    panel_tps = renderer.render_result(status="SUCCESS", duration=1.5, tokens_per_sec=42.0)
    assert isinstance(panel_tps, Panel)
    rendered_plain = str(panel_tps.renderable)
    assert "1.50s" in rendered_plain
    assert "42.0 tok/s" in rendered_plain


def test_render_markdown_with_multiple_blocks():
    md = (
        "Intro text\n\n"
        "```python\nprint(1)\n```\n\n"
        "Middle explanation\n\n"
        "```bash\necho 2\n```\n\n"
        "Outro"
    )
    renderer = MarkdownRenderer()
    renderable = renderer.render_markdown_with_code_blocks(md)
    assert renderable is not None
    assert len(renderer.code_registry.all_blocks()) == 2
    assert renderer.code_registry.get(1).language == "python"
    assert renderer.code_registry.get(2).language == "bash"


def test_code_fence_crlf_and_languages():
    renderer = MarkdownRenderer()
    crlf_md = "```python\r\nx = 1\r\ny = 2\r\n```"
    blocks_crlf = renderer.extract_and_register_code_blocks(crlf_md)
    assert len(blocks_crlf) == 1
    assert blocks_crlf[0].language == "python"

    csharp_md = "```c#\nint x = 42;\n```"
    blocks_cs = renderer.extract_and_register_code_blocks(csharp_md)
    assert len(blocks_cs) == 1
    assert blocks_cs[0].language == "c#"

    fsharp_md = "```f#   \nlet x = 1\n```"
    blocks_fs = renderer.extract_and_register_code_blocks(fsharp_md)
    assert len(blocks_fs) == 1
    assert blocks_fs[0].language == "f#"


def test_code_block_deduplication():
    renderer = MarkdownRenderer()
    sample = "```python\nval = 100\n```"
    b1 = renderer.extract_and_register_code_blocks(sample)
    b2 = renderer.extract_and_register_code_blocks(sample)
    assert len(renderer.code_registry.all_blocks()) == 1
    assert b1[0].id == b2[0].id == 1


def test_nested_fences_and_tildes():
    renderer = MarkdownRenderer()
    four_ticks = "````markdown\n```python\nx = 1\n```\n````"
    blocks = renderer.extract_and_register_code_blocks(four_ticks)
    assert len(blocks) == 1
    assert blocks[0].language == "markdown"
    assert "```python\nx = 1\n```" in blocks[0].code

    tildes = "~~~bash\necho hello\n~~~"
    blocks_tilde = renderer.extract_and_register_code_blocks(tildes)
    assert len(blocks_tilde) == 1
    assert blocks_tilde[0].language == "bash"
    assert "echo hello" in blocks_tilde[0].code


def test_code_fence_with_info_attributes_and_colons():
    renderer = MarkdownRenderer()
    md = (
        '```python title="app.py" {1-3}\n'
        "def main():\n"
        "    pass\n"
        "```\n\n"
        "```bash:deploy.sh\n"
        "exit 0\n"
        "```\n"
    )
    blocks = renderer.extract_and_register_code_blocks(md)
    assert len(blocks) == 2
    assert blocks[0].language == "python"
    assert "def main():" in blocks[0].code
    assert blocks[1].language == "bash"
    assert "exit 0" in blocks[1].code


def test_code_fence_line_ending_with_backticks():
    renderer = MarkdownRenderer()
    md = "```python\nx = 1```\nprint(x)\n```"
    blocks = renderer.extract_and_register_code_blocks(md)
    assert len(blocks) == 1
    assert blocks[0].language == "python"
    assert blocks[0].code == "x = 1```\nprint(x)\n"
