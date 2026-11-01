use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::highlight::IncrementalCodeHighlighter;
use crate::ui::markdown::parse::{parse_markdown_segments, MarkdownSegment};

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
                    lines.push(Line::from(current_line));
                    current_line = cont_prefix.to_vec();
                    current_width = cont_pw;
                }
            } else if current_width + token_len <= target_width {
                current_line.push(Span::styled(token, style));
                current_width += token_len;
            } else if token_len > target_width.saturating_sub(cont_pw) {
                for ch in token.chars() {
                    if current_width >= target_width {
                        lines.push(Line::from(current_line));
                        current_line = cont_prefix.to_vec();
                        current_width = cont_pw;
                    }
                    current_line.push(Span::styled(ch.to_string(), style));
                    current_width += 1;
                }
            } else {
                lines.push(Line::from(current_line));
                current_line = cont_prefix.to_vec();
                current_width = cont_pw;
                current_line.push(Span::styled(token, style));
                current_width += token_len;
            }
        }
    }

    if current_line.len() > first_prefix.len().max(cont_prefix.len()) || lines.is_empty() {
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

    for segment in segments {
        match segment {
            MarkdownSegment::Heading { level, spans } => {
                let h_style = match level {
                    1 => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::UNDERLINED),
                    2 => Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    3 => Style::default()
                        .fg(Color::LightCyan)
                        .add_modifier(Modifier::BOLD),
                    _ => Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::ITALIC),
                };
                let styled_spans: Vec<Span<'static>> = spans
                    .iter()
                    .map(|s| Span::styled(s.content.clone(), h_style))
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
            MarkdownSegment::ListItem { bullet, spans } => {
                let first_prefix = vec![Span::styled(
                    bullet.clone(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )];
                let indent = " ".repeat(bullet.chars().count());
                let cont_prefix = vec![Span::raw(indent)];
                let wrapped = wrap_styled_spans(spans, width, &first_prefix, &cont_prefix);
                output_lines.extend(wrapped);
            }
            MarkdownSegment::BlockQuote(spans) => {
                let prefix = vec![Span::styled(
                    "▎ ",
                    Style::default()
                        .fg(Color::Blue)
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

                let border_style = Style::default().fg(Color::DarkGray);
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
                    let mut col_widths = vec![4usize; col_count];
                    for (i, h) in headers.iter().enumerate() {
                        let len: usize = h.iter().map(|s| s.content.chars().count()).sum();
                        col_widths[i] = col_widths[i].max(len + 2);
                    }
                    for row in rows {
                        for (i, cell) in row.iter().enumerate() {
                            let len: usize = cell.iter().map(|s| s.content.chars().count()).sum();
                            if i < col_widths.len() {
                                col_widths[i] = col_widths[i].max(len + 2);
                            }
                        }
                    }

                    let avail = width.saturating_sub(col_count + 1);
                    if avail >= col_count * 2 {
                        let sum_w: usize = col_widths.iter().sum();
                        if sum_w > avail {
                            for w in col_widths.iter_mut() {
                                *w = ((*w * avail) / sum_w).max(2);
                            }
                        }
                    }

                    let border_style = Style::default().fg(Color::DarkGray);
                    if !headers.is_empty() {
                        let mut head_spans = vec![Span::styled("│", border_style)];
                        for (i, h) in headers.iter().enumerate() {
                            head_spans.push(Span::raw(" "));
                            let max_cell_w = col_widths[i].saturating_sub(1);
                            let cur_len: usize = h.iter().map(|s| s.content.chars().count()).sum();
                            if cur_len > max_cell_w {
                                let mut budget = max_cell_w;
                                for sp in h {
                                    let sp_len = sp.content.chars().count();
                                    if sp_len <= budget {
                                        head_spans.push(sp.clone());
                                        budget -= sp_len;
                                    } else if budget > 0 {
                                        let sub: String = sp.content.chars().take(budget).collect();
                                        head_spans.push(Span::styled(sub, sp.style));
                                        budget = 0;
                                    }
                                }
                            } else {
                                head_spans.extend(h.clone());
                                let pad = max_cell_w - cur_len;
                                head_spans.push(Span::raw(" ".repeat(pad)));
                            }
                            head_spans.push(Span::styled("│", border_style));
                        }
                        output_lines.push(Line::from(head_spans));

                        let mut sep_spans = vec![Span::styled("├", border_style)];
                        for (i, &w) in col_widths.iter().enumerate() {
                            sep_spans.push(Span::styled("─".repeat(w), border_style));
                            if i + 1 < col_widths.len() {
                                sep_spans.push(Span::styled("┼", border_style));
                            } else {
                                sep_spans.push(Span::styled("┤", border_style));
                            }
                        }
                        output_lines.push(Line::from(sep_spans));
                    }

                    for row in rows {
                        let mut row_spans = vec![Span::styled("│", border_style)];
                        for (i, &col_w) in col_widths.iter().enumerate().take(col_count) {
                            let cell = row.get(i);
                            row_spans.push(Span::raw(" "));
                            let max_cell_w = col_w.saturating_sub(1);
                            if let Some(c) = cell {
                                let len: usize = c.iter().map(|s| s.content.chars().count()).sum();
                                if len > max_cell_w {
                                    let mut budget = max_cell_w;
                                    for sp in c {
                                        let sp_len = sp.content.chars().count();
                                        if sp_len <= budget {
                                            row_spans.push(sp.clone());
                                            budget -= sp_len;
                                        } else if budget > 0 {
                                            let sub: String =
                                                sp.content.chars().take(budget).collect();
                                            row_spans.push(Span::styled(sub, sp.style));
                                            budget = 0;
                                        }
                                    }
                                } else {
                                    row_spans.extend(c.clone());
                                    let pad = max_cell_w - len;
                                    row_spans.push(Span::raw(" ".repeat(pad)));
                                }
                            } else {
                                row_spans.push(Span::raw(" ".repeat(max_cell_w)));
                            }
                            row_spans.push(Span::styled("│", border_style));
                        }
                        output_lines.push(Line::from(row_spans));
                    }
                }
            }
            MarkdownSegment::Rule => {
                let rule_len = width.clamp(1, 40);
                output_lines.push(Line::from(vec![Span::styled(
                    "─".repeat(rule_len),
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }
    }

    output_lines
}
