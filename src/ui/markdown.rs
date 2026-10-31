use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::highlight::IncrementalCodeHighlighter;

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedMarkdown {
    pub text: String,
    pub has_unclosed_fence: bool,
    pub fence_lang: Option<String>,
}

pub fn normalize_streaming_markdown(input: &str, consolidated: bool) -> NormalizedMarkdown {
    if consolidated || input.is_empty() {
        return NormalizedMarkdown {
            text: input.to_string(),
            has_unclosed_fence: false,
            fence_lang: None,
        };
    }

    let mut in_fence = false;
    let mut fence_char = '`';
    let mut fence_len = 0;
    let mut current_lang = None;

    for line in input.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                let marker = trimmed.chars().next().unwrap();
                let count = trimmed.chars().take_while(|&c| c == marker).count();
                if count >= 3 {
                    in_fence = true;
                    fence_char = marker;
                    fence_len = count;
                    let info = trimmed[count..].trim();
                    let lang = info.split_whitespace().next().unwrap_or("");
                    current_lang = if lang.is_empty() {
                        None
                    } else {
                        Some(lang.to_string())
                    };
                }
            }
        } else {
            let marker_str = std::iter::repeat_n(fence_char, fence_len).collect::<String>();
            if trimmed.starts_with(&marker_str) {
                let rest = trimmed[fence_len..].trim();
                if rest.is_empty() {
                    in_fence = false;
                    current_lang = None;
                }
            }
        }
    }

    let mut output = input.to_string();
    let mut has_unclosed = false;

    if in_fence {
        has_unclosed = true;
        if !output.ends_with('\n') {
            output.push('\n');
        }
        for _ in 0..fence_len {
            output.push(fence_char);
        }
        output.push('\n');
    } else {
        let last_line = input.split('\n').next_back().unwrap_or("");
        let backticks = last_line.chars().filter(|&c| c == '`').count();
        if backticks % 2 != 0 {
            output.push('`');
        }
    }

    NormalizedMarkdown {
        text: output,
        has_unclosed_fence: has_unclosed,
        fence_lang: current_lang,
    }
}

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

#[derive(Clone, Debug)]
pub enum MarkdownSegment {
    Heading {
        level: u8,
        spans: Vec<Span<'static>>,
    },
    Paragraph(Vec<Span<'static>>),
    ListItem {
        bullet: String,
        spans: Vec<Span<'static>>,
    },
    BlockQuote(Vec<Span<'static>>),
    CodeBlock {
        language: String,
        code: String,
        is_unclosed: bool,
    },
    Table {
        headers: Vec<Vec<Span<'static>>>,
        rows: Vec<Vec<Vec<Span<'static>>>>,
    },
    Rule,
}

pub fn parse_markdown_segments(markdown_text: &str, consolidated: bool) -> Vec<MarkdownSegment> {
    let normalized = normalize_streaming_markdown(markdown_text, consolidated);
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(&normalized.text, options);
    let mut segments: Vec<MarkdownSegment> = Vec::new();

    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut active_styles: Vec<Style> = vec![Style::default().fg(Color::White)];
    let mut current_heading_level: Option<u8> = None;
    let mut in_code_block: Option<(String, String)> = None;
    let mut list_depth: usize = 0;
    let mut is_ordered_list: Vec<u64> = Vec::new();
    let mut in_blockquote = false;

    let mut in_table = false;
    let mut in_table_head = false;
    let mut table_headers: Vec<Vec<Span<'static>>> = Vec::new();
    let mut table_rows: Vec<Vec<Vec<Span<'static>>>> = Vec::new();
    let mut current_table_row: Vec<Vec<Span<'static>>> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Heading { level, .. } => {
                    let lvl = match level {
                        HeadingLevel::H1 => 1,
                        HeadingLevel::H2 => 2,
                        HeadingLevel::H3 => 3,
                        HeadingLevel::H4 => 4,
                        HeadingLevel::H5 => 5,
                        HeadingLevel::H6 => 6,
                    };
                    current_heading_level = Some(lvl);
                    current_spans.clear();
                    let h_style = match lvl {
                        1 => Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                            .add_modifier(Modifier::UNDERLINED),
                        2 => Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                        _ => Style::default()
                            .fg(Color::LightCyan)
                            .add_modifier(Modifier::BOLD),
                    };
                    active_styles.push(h_style);
                }
                Tag::Paragraph => {
                    current_spans.clear();
                }
                Tag::BlockQuote(_) => {
                    in_blockquote = true;
                    current_spans.clear();
                }
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(l) => l.to_string(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    in_code_block = Some((lang, String::new()));
                }
                Tag::List(start_num) => {
                    list_depth += 1;
                    if let Some(start) = start_num {
                        is_ordered_list.push(start);
                    } else {
                        is_ordered_list.push(0);
                    }
                }
                Tag::Item => {
                    current_spans.clear();
                }
                Tag::Emphasis => {
                    let cur = *active_styles.last().unwrap_or(&Style::default());
                    active_styles.push(cur.add_modifier(Modifier::ITALIC));
                }
                Tag::Strong => {
                    let cur = *active_styles.last().unwrap_or(&Style::default());
                    active_styles.push(cur.add_modifier(Modifier::BOLD));
                }
                Tag::Strikethrough => {
                    let cur = *active_styles.last().unwrap_or(&Style::default());
                    active_styles.push(cur.add_modifier(Modifier::CROSSED_OUT));
                }
                Tag::Table(_) => {
                    in_table = true;
                    table_headers.clear();
                    table_rows.clear();
                }
                Tag::TableHead => {
                    in_table_head = true;
                    table_headers.clear();
                }
                Tag::TableRow => {
                    current_table_row.clear();
                }
                Tag::TableCell => {
                    current_spans.clear();
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Heading(_) => {
                    if let Some(lvl) = current_heading_level.take() {
                        segments.push(MarkdownSegment::Heading {
                            level: lvl,
                            spans: std::mem::take(&mut current_spans),
                        });
                    }
                    active_styles.pop();
                }
                TagEnd::Paragraph => {
                    if !current_spans.is_empty() {
                        segments.push(MarkdownSegment::Paragraph(std::mem::take(
                            &mut current_spans,
                        )));
                    }
                }
                TagEnd::BlockQuote(_) => {
                    in_blockquote = false;
                    if !current_spans.is_empty() {
                        segments.push(MarkdownSegment::BlockQuote(std::mem::take(
                            &mut current_spans,
                        )));
                    }
                }
                TagEnd::CodeBlock => {
                    if let Some((lang, code)) = in_code_block.take() {
                        segments.push(MarkdownSegment::CodeBlock {
                            language: lang,
                            code,
                            is_unclosed: normalized.has_unclosed_fence,
                        });
                    }
                }
                TagEnd::List(_) => {
                    list_depth = list_depth.saturating_sub(1);
                    is_ordered_list.pop();
                }
                TagEnd::Item => {
                    let bullet = if let Some(order) = is_ordered_list.last_mut() {
                        if *order > 0 {
                            let b = format!("{}. ", order);
                            *order += 1;
                            b
                        } else {
                            String::from("• ")
                        }
                    } else {
                        String::from("• ")
                    };
                    segments.push(MarkdownSegment::ListItem {
                        bullet,
                        spans: std::mem::take(&mut current_spans),
                    });
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                    active_styles.pop();
                }
                TagEnd::TableCell => {
                    let cell_spans = std::mem::take(&mut current_spans);
                    if in_table_head {
                        table_headers.push(cell_spans);
                    } else {
                        current_table_row.push(cell_spans);
                    }
                }
                TagEnd::TableRow => {
                    if !in_table_head && !current_table_row.is_empty() {
                        table_rows.push(std::mem::take(&mut current_table_row));
                    }
                }
                TagEnd::TableHead => {
                    in_table_head = false;
                }
                TagEnd::Table => {
                    in_table = false;
                    segments.push(MarkdownSegment::Table {
                        headers: std::mem::take(&mut table_headers),
                        rows: std::mem::take(&mut table_rows),
                    });
                }
                _ => {}
            },
            Event::Text(txt) => {
                if let Some((_, ref mut code)) = in_code_block {
                    code.push_str(&txt);
                } else {
                    let style = *active_styles.last().unwrap_or(&Style::default());
                    current_spans.push(Span::styled(txt.to_string(), style));
                }
            }
            Event::Code(code_txt) => {
                let code_style = Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(30, 30, 45));
                current_spans.push(Span::styled(format!("`{}`", code_txt), code_style));
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_code_block.is_none() && !in_table {
                    current_spans.push(Span::raw(" "));
                }
            }
            Event::Rule => {
                segments.push(MarkdownSegment::Rule);
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                let marker_style = Style::default()
                    .fg(if checked {
                        Color::Green
                    } else {
                        Color::DarkGray
                    })
                    .add_modifier(Modifier::BOLD);
                current_spans.insert(0, Span::styled(marker, marker_style));
            }
            _ => {}
        }
    }

    if let Some((lang, code)) = in_code_block {
        segments.push(MarkdownSegment::CodeBlock {
            language: lang,
            code,
            is_unclosed: normalized.has_unclosed_fence,
        });
    }
    if !current_spans.is_empty() {
        if let Some(lvl) = current_heading_level {
            segments.push(MarkdownSegment::Heading {
                level: lvl,
                spans: current_spans,
            });
        } else if in_blockquote {
            segments.push(MarkdownSegment::BlockQuote(current_spans));
        } else {
            segments.push(MarkdownSegment::Paragraph(current_spans));
        }
    }

    segments
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
