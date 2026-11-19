use frontal::ui::markdown::{
    render_markdown_to_lines, wrap_lines_preserving_indent, wrap_styled_spans,
};
use frontal::ui::theme::{ACCENT_RUBY, TEXT_PRIMARY};
use ratatui::style::Style;
use ratatui::text::{Line, Span};

#[test]
fn test_table_cell_wrapping_and_wrap_styled_spans_edge_cases() {
    let md = "| Model | Details |\n|---|---|\n| Gemini | Gemini 4 Neon, Gemini 4 Hydrogen, Orchestrator Pro |\n";
    let mut hl = Vec::new();
    let lines = render_markdown_to_lines(md, 40, true, &mut hl);
    let text_lines: Vec<String> = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(text_lines.len() > 5);
    let first_w = text_lines[0].chars().count();
    assert!(first_w <= 40);
    for l in &text_lines {
        assert_eq!(l.chars().count(), first_w);
    }
    let body_rows: Vec<&String> = text_lines.iter().filter(|l| l.starts_with('│')).collect();
    assert!(body_rows.iter().all(|l| l.ends_with(" │")));
    let joined = text_lines.join("\n");
    assert!(joined.contains("Gemini 4 Neon"));
    assert!(joined.contains("Orchestrator"));
    assert!(joined.contains("Pro"));

    let narrow_lines = render_markdown_to_lines(md, 5, true, &mut hl);
    let narrow_text: Vec<String> = narrow_lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    let nw = narrow_text[0].chars().count();
    assert!(nw <= 5);
    for l in &narrow_text {
        assert_eq!(l.chars().count(), nw);
    }

    let nl_wrapped = wrap_styled_spans(
        &[Span::raw("line one\nline two")],
        40,
        &[Span::raw("   Status: ")],
        &[Span::raw("     ")],
    );
    assert_eq!(nl_wrapped.len(), 2);
    let first_text: String = nl_wrapped[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let second_text: String = nl_wrapped[1]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(first_text, "   Status: line one");
    assert_eq!(second_text, "     line two");

    let prefix_wrapped = wrap_styled_spans(
        &[Span::raw("superlongword")],
        18,
        &[Span::raw("   Status: ")],
        &[Span::raw("     ")],
    );
    assert_eq!(prefix_wrapped.len(), 2);
    let p2: String = prefix_wrapped[1]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(p2, "     superlongword");

    let styled_line = vec![Line::from(vec![
        Span::raw("   "),
        Span::styled(
            " ● Global Usage & Tool Stats ",
            Style::default().bg(ACCENT_RUBY).fg(TEXT_PRIMARY),
        ),
    ])];
    let wrapped_badge = wrap_lines_preserving_indent(styled_line, 24);
    assert!(wrapped_badge.len() >= 2);
    let badge_leading_space = wrapped_badge[0]
        .spans
        .iter()
        .find(|s| s.style.bg == Some(ACCENT_RUBY))
        .expect("ruby background span preserved");
    assert!(badge_leading_space.content.starts_with(' '));
}

#[test]
fn test_wrap_tabs_trailing_newlines_single_span_and_codeblock_wrap() {
    let tabbed = wrap_styled_spans(&[Span::raw("a\tb")], 20, &[], &[]);
    let tabbed_text: String = tabbed[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(tabbed_text, "a    b");

    let trailing = wrap_styled_spans(&[Span::raw("foo\n")], 20, &[], &[]);
    assert_eq!(trailing.len(), 2);

    let split = wrap_styled_spans(&[Span::raw("abcdefghij")], 4, &[], &[]);
    assert_eq!(split.len(), 3);
    assert!(split.iter().all(|l| l.spans.len() == 1));
    let joined_split: Vec<String> = split
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert_eq!(joined_split, vec!["abcd", "efgh", "ij"]);

    let md = "```rust\nlet very_long_identifier_name = 1234567890;\n```\n";
    let mut hl = Vec::new();
    let lines = render_markdown_to_lines(md, 16, true, &mut hl);
    let rendered: Vec<String> = lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    assert!(rendered.iter().all(|l| l.chars().count() <= 16));
    let bar_lines: Vec<&String> = rendered.iter().filter(|l| l.starts_with("│ ")).collect();
    assert!(bar_lines.len() >= 3);

    let indented_md = "```rust\n    let indented_code_identifier_here = 999;\n```\n";
    let indented_lines = render_markdown_to_lines(indented_md, 24, true, &mut hl);
    let indented_rendered: Vec<String> = indented_lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect();
    let indented_bars: Vec<&String> = indented_rendered
        .iter()
        .filter(|l| l.starts_with("│ "))
        .collect();
    assert!(indented_bars.len() >= 2);
    assert!(indented_bars[1].starts_with("│     "));

    let clamped_tab = wrap_styled_spans(&[Span::raw("\tword")], 2, &[], &[]);
    let first_tab_row: String = clamped_tab[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert_eq!(first_tab_row, "  ");

    for narrow_w in 1..=2 {
        let narrow_code = render_markdown_to_lines("```rust\nabcd\n```\n", narrow_w, true, &mut hl);
        assert!(narrow_code
            .iter()
            .map(|l| l
                .spans
                .iter()
                .map(|s| s.content.chars().count())
                .sum::<usize>())
            .all(|w| w <= narrow_w));
    }
}
