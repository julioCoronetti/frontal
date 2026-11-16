use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::highlight::IncrementalCodeHighlighter;
use crate::ui::markdown::parse::{parse_markdown_segments, MarkdownSegment};
use crate::ui::theme::{
    ACCENT_HIGHLIGHT, ACCENT_PRIMARY, ACCENT_RUBY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY,
};

pub fn wrap_styled_spans(
    spans: &[Span<'static>],
    max_width: usize,
    first_prefix: &[Span<'static>],
    cont_prefix: &[Span<'static>],
) -> Vec<Line<'static>> {
    let target_width = max_width.max(1);
    let first_pw: usize = first_prefix.iter().map(|s| s.content.chars().count()).sum();
    let cont_pw: usize = cont_prefix.iter().map(|s| s.content.chars().count()).sum();

    let mut lines = Vec::new();
    let mut current_line = first_prefix.to_vec();
    let mut current_width = first_pw;
    let mut on_cont = false;

    for span in spans {
        let style = span.style;
        let mut words = Vec::new();
        let mut current_word = String::new();

        for c in span.content.chars() {
            if c.is_whitespace() {
                if !current_word.is_empty() {
                    words.push((current_word.clone(), false));
                    current_word.clear();
                }
                words.push((c.to_string(), true));
            } else {
                current_word.push(c);
            }
        }
        if !current_word.is_empty() {
            words.push((current_word, false));
        }

        for (token, is_ws) in words {
            let token_len = token.chars().count();
            if is_ws {
                if current_width + token_len <= target_width {
                    current_line.push(Span::styled(token, style));
                    current_width += token_len;
                } else {
                    let active_prefix_len = if on_cont {
                        cont_prefix.len()
                    } else {
                        first_prefix.len()
                    };
                    if current_line.len() > active_prefix_len {
                        lines.push(Line::from(current_line));
                        current_line = cont_prefix.to_vec();
                        current_width = cont_pw;
                        on_cont = true;
                    }
                }
            } else if current_width + token_len <= target_width {
                current_line.push(Span::styled(token, style));
                current_width += token_len;
            } else if token_len > target_width.saturating_sub(cont_pw) {
                for ch in token.chars() {
                    let active_prefix_len = if on_cont {
                        cont_prefix.len()
                    } else {
                        first_prefix.len()
                    };
                    if current_width >= target_width && current_line.len() > active_prefix_len {
                        lines.push(Line::from(current_line));
                        current_line = cont_prefix.to_vec();
                        current_width = cont_pw;
                        on_cont = true;
                    }
                    current_line.push(Span::styled(ch.to_string(), style));
                    current_width += 1;
                }
            } else {
                let active_prefix_len = if on_cont {
                    cont_prefix.len()
                } else {
                    first_prefix.len()
                };
                if current_line.len() > active_prefix_len {
                    lines.push(Line::from(current_line));
                    current_line = cont_prefix.to_vec();
                    current_width = cont_pw;
                    on_cont = true;
                }
                current_line.push(Span::styled(token, style));
                current_width += token_len;
            }
        }
    }

    let active_prefix_len = if on_cont {
        cont_prefix.len()
    } else {
        first_prefix.len()
    };
    if current_line.len() > active_prefix_len || lines.is_empty() {
        lines.push(Line::from(current_line));
    }

    lines
}

pub fn render_markdown_to_lines(
    markdown_text: &str,
    width: usize,
    consolidated: bool,
    highlighters: &mut Vec<IncrementalCodeHighlighter>,
) -> Vec<Line<'static>> {
    let segments = parse_markdown_segments(markdown_text, consolidated);
    render_segments_to_lines(&segments, width, highlighters)
}

pub fn render_segments_to_lines(
    segments: &[MarkdownSegment],
    width: usize,
    highlighters: &mut Vec<IncrementalCodeHighlighter>,
) -> Vec<Line<'static>> {
    let mut output_lines = Vec::new();
    let mut code_block_idx = 0;

    for (idx, segment) in segments.iter().enumerate() {
        if idx > 0
            && !matches!(
                (&segments[idx - 1], segment),
                (
                    MarkdownSegment::ListItem { .. },
                    MarkdownSegment::ListItem { .. }
                )
            )
        {
            output_lines.push(Line::from(""));
        }

        match segment {
            MarkdownSegment::Heading { level, spans } => {
                let h_style = match level {
                    1 => Style::default()
                        .fg(ACCENT_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                    2 => Style::default()
                        .fg(ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                    3 => Style::default()
                        .fg(TEXT_PRIMARY)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(TEXT_MUTED)
                        .add_modifier(Modifier::BOLD | Modifier::ITALIC),
                };
                let styled_spans: Vec<Span<'static>> = spans
                    .iter()
                    .map(|s| Span::styled(s.content.clone(), h_style.patch(s.style)))
                    .collect();
                let empty_prefix = vec![];
                let wrapped = wrap_styled_spans(&styled_spans, width, &empty_prefix, &empty_prefix);
                output_lines.extend(wrapped);
            }
            MarkdownSegment::Paragraph(spans) => {
                let empty_prefix = vec![];
                let wrapped = wrap_styled_spans(spans, width, &empty_prefix, &empty_prefix);
                output_lines.extend(wrapped);
            }
            MarkdownSegment::ListItem {
                depth,
                bullet,
                spans,
            } => {
                let indent = "  ".repeat(depth.saturating_sub(1));
                let mut first_prefix = Vec::new();
                if !indent.is_empty() {
                    first_prefix.push(Span::raw(indent.clone()));
                }
                first_prefix.push(Span::styled(
                    bullet.clone(),
                    Style::default().fg(TEXT_MUTED),
                ));
                let cont_indent = format!("{}{}", indent, " ".repeat(bullet.chars().count()));
                let cont_prefix = vec![Span::raw(cont_indent)];
                let wrapped = wrap_styled_spans(spans, width, &first_prefix, &cont_prefix);
                output_lines.extend(wrapped);
            }
            MarkdownSegment::BlockQuote(spans) => {
                let prefix = vec![Span::styled(
                    "▎ ",
                    Style::default()
                        .fg(ACCENT_RUBY)
                        .add_modifier(Modifier::BOLD),
                )];
                let wrapped = wrap_styled_spans(spans, width, &prefix, &prefix);
                output_lines.extend(wrapped);
            }
            MarkdownSegment::CodeBlock {
                language,
                code,
                is_unclosed,
            } => {
                while highlighters.len() <= code_block_idx {
                    highlighters.push(IncrementalCodeHighlighter::new());
                }
                let highlighted_lines = highlighters[code_block_idx].highlight_code(language, code);
                code_block_idx += 1;

                let border_style = Style::default().fg(BORDER_SUBTLE);
                let lang_label = if language.is_empty() {
                    String::from("code")
                } else {
                    language.clone()
                };
                let status_label = if *is_unclosed { " [streaming...]" } else { "" };
                let top_border = format!("┌─ {}{} ─", lang_label, status_label);
                output_lines.push(Line::from(vec![Span::styled(top_border, border_style)]));

                let bar = Span::styled("│ ", border_style);
                for hl_spans in highlighted_lines {
                    let mut line_spans = vec![bar.clone()];
                    line_spans.extend(hl_spans);
                    output_lines.push(Line::from(line_spans));
                }

                output_lines.push(Line::from(vec![Span::styled("└────", border_style)]));
            }
            MarkdownSegment::Table { headers, rows } => {
                let col_count = headers
                    .len()
                    .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));
                if col_count > 0 {
                    let col_widths = compute_table_col_widths(headers, rows, col_count, width);
                    let border_style = Style::default().fg(TEXT_MUTED);
                    let header_style = Style::default()
                        .fg(ACCENT_HIGHLIGHT)
                        .add_modifier(Modifier::BOLD);

                    output_lines.push(build_table_border_line(
                        &col_widths,
                        "┌",
                        "┬",
                        "┐",
                        border_style,
                    ));
                    if !headers.is_empty() {
                        output_lines.push(build_table_row_line(
                            headers,
                            &col_widths,
                            border_style,
                            Some(header_style),
                        ));
                        if !rows.is_empty() {
                            output_lines.push(build_table_border_line(
                                &col_widths,
                                "├",
                                "┼",
                                "┤",
                                border_style,
                            ));
                        }
                    }
                    for (r_idx, row) in rows.iter().enumerate() {
                        output_lines.push(build_table_row_line(
                            row,
                            &col_widths,
                            border_style,
                            None,
                        ));
                        if r_idx + 1 < rows.len() {
                            output_lines.push(build_table_border_line(
                                &col_widths,
                                "├",
                                "┼",
                                "┤",
                                border_style,
                            ));
                        }
                    }
                    output_lines.push(build_table_border_line(
                        &col_widths,
                        "└",
                        "┴",
                        "┘",
                        border_style,
                    ));
                }
            }
            MarkdownSegment::Rule => {
                let rule_len = width.clamp(1, 40);
                output_lines.push(Line::from(vec![Span::styled(
                    "─".repeat(rule_len),
                    Style::default().fg(BORDER_SUBTLE),
                )]));
            }
        }
    }

    output_lines
}

fn compute_table_col_widths(
    headers: &[Vec<Span<'static>>],
    rows: &[Vec<Vec<Span<'static>>>],
    col_count: usize,
    width: usize,
) -> Vec<usize> {
    let mut col_widths = vec![4usize; col_count];
    for (i, h) in headers.iter().enumerate().take(col_count) {
        let len: usize = h.iter().map(|s| s.content.chars().count()).sum();
        col_widths[i] = col_widths[i].max(len + 2);
    }
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(col_count) {
            let len: usize = cell.iter().map(|s| s.content.chars().count()).sum();
            col_widths[i] = col_widths[i].max(len + 2);
        }
    }

    let avail = width.saturating_sub(col_count + 1);
    if avail == 0 {
        return vec![0; col_count];
    }

    let sum_w: usize = col_widths.iter().sum();
    let target_avail = ((avail * 85) / 100).max(col_count.min(avail));

    if sum_w > avail {
        let min_w = usize::from(avail >= col_count);
        for w in col_widths.iter_mut() {
            *w = ((*w * avail) / sum_w).max(min_w);
        }
        while col_widths.iter().sum::<usize>() > avail {
            if let Some(widest) = col_widths.iter_mut().filter(|w| **w > 0).max() {
                *widest -= 1;
            } else {
                break;
            }
        }
    } else if sum_w < target_avail && sum_w > 0 {
        let orig = col_widths.clone();
        for (i, w) in col_widths.iter_mut().enumerate() {
            *w = ((orig[i] * target_avail) / sum_w).max(orig[i]);
        }
        let mut cur_sum: usize = col_widths.iter().sum();
        let mut idx = 0;
        while cur_sum < target_avail {
            col_widths[idx % col_count] += 1;
            cur_sum += 1;
            idx += 1;
        }
    }

    col_widths
}

fn build_table_border_line(
    col_widths: &[usize],
    left: &'static str,
    mid: &'static str,
    right: &'static str,
    border_style: Style,
) -> Line<'static> {
    let mut spans = Vec::with_capacity(col_widths.len() * 2 + 1);
    spans.push(Span::styled(left, border_style));
    for (i, &w) in col_widths.iter().enumerate() {
        spans.push(Span::styled("─".repeat(w), border_style));
        if i + 1 < col_widths.len() {
            spans.push(Span::styled(mid, border_style));
        } else {
            spans.push(Span::styled(right, border_style));
        }
    }
    Line::from(spans)
}

fn build_table_row_line(
    cells: &[Vec<Span<'static>>],
    col_widths: &[usize],
    border_style: Style,
    cell_base_style: Option<Style>,
) -> Line<'static> {
    let mut spans = vec![Span::styled("│", border_style)];
    for (i, &col_w) in col_widths.iter().enumerate() {
        if col_w == 0 {
            spans.push(Span::styled("│", border_style));
            continue;
        }
        spans.push(Span::raw(" "));
        let max_cell_w = col_w - 1;
        if let Some(cell) = cells.get(i) {
            let len: usize = cell.iter().map(|s| s.content.chars().count()).sum();
            if len > max_cell_w {
                let mut budget = max_cell_w;
                for sp in cell {
                    let style = match cell_base_style {
                        Some(base) => sp.style.patch(base),
                        None => sp.style,
                    };
                    let sp_len = sp.content.chars().count();
                    if sp_len <= budget {
                        spans.push(Span::styled(sp.content.clone(), style));
                        budget -= sp_len;
                    } else if budget > 0 {
                        let sub: String = sp.content.chars().take(budget).collect();
                        spans.push(Span::styled(sub, style));
                        budget = 0;
                    }
                }
            } else {
                for sp in cell {
                    let style = match cell_base_style {
                        Some(base) => sp.style.patch(base),
                        None => sp.style,
                    };
                    spans.push(Span::styled(sp.content.clone(), style));
                }
                let pad = max_cell_w - len;
                if pad > 0 {
                    spans.push(Span::raw(" ".repeat(pad)));
                }
            }
        } else if max_cell_w > 0 {
            spans.push(Span::raw(" ".repeat(max_cell_w)));
        }
        spans.push(Span::styled("│", border_style));
    }
    Line::from(spans)
}
