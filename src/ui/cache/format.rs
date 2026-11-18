use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::protocol::StepState;
use crate::ui::cache::user_format::format_user_block_lines;
use crate::ui::cache::{BlockKind, CachedBlockLines, MessageBlock, RenderCache};
use crate::ui::markdown::{
    parse_markdown_segments, render_markdown_to_lines, render_segments_to_lines,
};
use crate::ui::theme::{ACCENT_HIGHLIGHT, ACCENT_PRIMARY, BORDER_SUBTLE, TEXT_MUTED, TEXT_PRIMARY};

pub const TOOL_OUTPUT_HEAD_LINES: usize = 100;
pub const TOOL_OUTPUT_TAIL_LINES: usize = 50;
pub const TOOL_OUTPUT_MAX_LINE_CHARS: usize = 2048;

fn clamp_single_line(line: &str, max_chars: usize) -> std::borrow::Cow<'_, str> {
    if line.len() <= max_chars {
        return std::borrow::Cow::Borrowed(line);
    }
    if let Some((byte_idx, _)) = line.char_indices().nth(max_chars) {
        std::borrow::Cow::Owned(format!("{}… [line truncated]", &line[..byte_idx]))
    } else {
        std::borrow::Cow::Borrowed(line)
    }
}

pub fn clamp_tool_output(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    let max_lines = TOOL_OUTPUT_HEAD_LINES + TOOL_OUTPUT_TAIL_LINES;
    let mut head: Vec<std::borrow::Cow<'_, str>> = Vec::with_capacity(TOOL_OUTPUT_HEAD_LINES);
    let mut tail: std::collections::VecDeque<&str> =
        std::collections::VecDeque::with_capacity(TOOL_OUTPUT_TAIL_LINES);
    let mut total_lines = 0usize;
    let mut modified = false;

    for line in raw.lines() {
        total_lines += 1;
        if head.len() < TOOL_OUTPUT_HEAD_LINES {
            let clamped = clamp_single_line(line, TOOL_OUTPUT_MAX_LINE_CHARS);
            if matches!(clamped, std::borrow::Cow::Owned(_)) {
                modified = true;
            }
            head.push(clamped);
        } else {
            if tail.len() == TOOL_OUTPUT_TAIL_LINES {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    }

    if total_lines <= max_lines {
        let mut clamped_tail = Vec::with_capacity(tail.len());
        for line in tail {
            let clamped = clamp_single_line(line, TOOL_OUTPUT_MAX_LINE_CHARS);
            if matches!(clamped, std::borrow::Cow::Owned(_)) {
                modified = true;
            }
            clamped_tail.push(clamped);
        }
        if !modified {
            return raw.to_string();
        }
        head.extend(clamped_tail);
        return head.join("\n");
    }

    let elided = total_lines - max_lines;
    let mut out = Vec::with_capacity(max_lines + 1);
    out.extend(head);
    out.push(std::borrow::Cow::Owned(format!(
        "[... {} lines elided in UI ...]",
        elided
    )));
    for line in tail {
        out.push(clamp_single_line(line, TOOL_OUTPUT_MAX_LINE_CHARS));
    }
    out.join("\n")
}

pub fn sanitize_terminal_line(line: &str) -> String {
    let mut raw = line.trim_end_matches('\r');
    if let Some(pos) = raw.rfind('\r') {
        raw = &raw[pos + 1..];
    }
    let mut expanded = String::with_capacity(raw.len());
    let mut col = 0;
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some(&'[') = chars.peek() {
                chars.next();
                while let Some(&c) = chars.peek() {
                    chars.next();
                    if c.is_ascii_alphabetic() || c == '@' || c == 'm' {
                        break;
                    }
                }
            }
        } else if ch == '\t' {
            let spaces = 4 - (col % 4);
            for _ in 0..spaces {
                expanded.push(' ');
                col += 1;
            }
        } else if !ch.is_control() || ch == ' ' {
            expanded.push(ch);
            col += 1;
        }
    }
    expanded
}

fn with_left_margin(margin: &'static str, spans: Vec<Span<'static>>) -> Line<'static> {
    if margin.is_empty() || spans.iter().all(|s| s.content.is_empty()) {
        return Line::from(spans);
    }
    let mut out = Vec::with_capacity(spans.len() + 1);
    out.push(Span::raw(margin));
    out.extend(spans);
    Line::from(out)
}

fn format_duration(seconds: f64) -> String {
    if seconds >= 60.0 {
        let total = seconds.round() as u64;
        format!("{}m {}s", total / 60, total % 60)
    } else {
        format!("{:.1}s", seconds)
    }
}

pub fn wrap_text_utf8(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let max_w = max_width.max(1);
    for raw_line in text.split('\n') {
        let clean = sanitize_terminal_line(raw_line);
        if clean.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current_line = String::new();
        let mut current_width = 0;
        for ch in clean.chars() {
            let ch_w = 1;
            if current_width + ch_w > max_w && !current_line.is_empty() {
                lines.push(current_line);
                current_line = String::new();
                current_width = 0;
            }
            current_line.push(ch);
            current_width += ch_w;
        }
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

impl RenderCache {
    pub fn ensure_visual_lines(&mut self, width: u16) -> usize {
        let safe_width = width.max(1);
        self.last_width = safe_width;
        let mut total = 0;

        for block in &mut self.blocks {
            let hit = if let Some(entry) = self.l2_cache.get(&block.id) {
                entry.width == safe_width && entry.revision == block.revision
            } else {
                false
            };

            if hit {
                self.cache_hits += 1;
                total += self.l2_cache.get(&block.id).unwrap().lines.len();
            } else {
                self.cache_misses += 1;
                let generated_lines = Self::format_block_lines(block, safe_width);
                total += generated_lines.len();
                self.l2_cache.insert(
                    block.id,
                    CachedBlockLines {
                        width: safe_width,
                        revision: block.revision,
                        lines: generated_lines,
                    },
                );
            }
        }

        total
    }

    pub(crate) fn format_block_lines(block: &mut MessageBlock, width: u16) -> Vec<Line<'static>> {
        let mut result = Vec::new();
        let margin = if width > 4 { "  " } else { "" };
        match &block.kind {
            BlockKind::User => {
                result.extend(format_user_block_lines(&block.content, width));
            }
            BlockKind::Assistant => {
                let content_width = (width as usize).saturating_sub(margin.len());
                let lines = if block.consolidated {
                    if block.markdown_ast.is_none() {
                        block.markdown_ast = Some(parse_markdown_segments(&block.content, true));
                    }
                    render_segments_to_lines(
                        block.markdown_ast.as_ref().unwrap(),
                        content_width,
                        &mut block.code_highlighters,
                    )
                } else {
                    render_markdown_to_lines(
                        &block.content,
                        content_width,
                        false,
                        &mut block.code_highlighters,
                    )
                };
                for line in lines {
                    result.push(with_left_margin(margin, line.spans));
                }
                if block.consolidated {
                    if let Some(meta) = &block.turn_meta {
                        let display = crate::ui::labels::model_display_name(&meta.model);
                        let model_label = if display.is_empty() {
                            meta.model.clone()
                        } else {
                            display
                        };
                        let mut meta_parts = vec![model_label];
                        if let Some(d) = meta.duration_seconds {
                            if d > 0.0 {
                                meta_parts.push(format_duration(d));
                                if let Some(tok) = meta.output_tokens {
                                    if tok > 0 {
                                        meta_parts.push(format!("{:.1} tok/s", (tok as f64) / d));
                                    }
                                }
                            }
                        }
                        result.push(Line::from(""));
                        result.push(with_left_margin(
                            margin,
                            vec![Span::styled(
                                meta_parts.join(" · "),
                                Style::default().fg(TEXT_MUTED),
                            )],
                        ));
                    }
                    result.push(Line::from(""));
                }
            }
            BlockKind::Thought => {
                if block.collapsed {
                    result.push(with_left_margin(
                        margin,
                        vec![Span::styled("+ Thought", Style::default().fg(TEXT_MUTED))],
                    ));
                } else {
                    result.push(with_left_margin(
                        margin,
                        vec![Span::styled("- Thought", Style::default().fg(TEXT_MUTED))],
                    ));
                    result.push(Line::from(""));
                    let wrapped = wrap_text_utf8(
                        &block.content,
                        (width as usize).saturating_sub(4 + margin.len()),
                    );
                    for row in wrapped {
                        result.push(with_left_margin(
                            margin,
                            vec![
                                Span::styled("│ ", Style::default().fg(BORDER_SUBTLE)),
                                Span::styled(
                                    row,
                                    Style::default()
                                        .fg(TEXT_MUTED)
                                        .add_modifier(Modifier::ITALIC),
                                ),
                            ],
                        ));
                    }
                }
                result.push(Line::from(""));
            }
            BlockKind::Tool { name, state } => {
                let max_args_len =
                    (width as usize).saturating_sub(name.chars().count() + 25 + margin.len());
                let clean_args = block
                    .tool_args
                    .as_deref()
                    .map(sanitize_terminal_line)
                    .unwrap_or_default();
                let clamped_args = if clean_args.is_empty() {
                    String::new()
                } else if clean_args.chars().count() > max_args_len && max_args_len > 4 {
                    format!(
                        " ({}...)",
                        clean_args
                            .chars()
                            .take(max_args_len.saturating_sub(4))
                            .collect::<String>()
                    )
                } else {
                    format!(" ({})", clean_args)
                };

                let mut header_spans = Vec::new();
                match state {
                    StepState::Active => {
                        header_spans
                            .push(Span::styled("● ", Style::default().fg(ACCENT_HIGHLIGHT)));
                        header_spans
                            .push(Span::styled("Running ", Style::default().fg(TEXT_MUTED)));
                        header_spans.push(Span::styled(
                            name.clone(),
                            Style::default().fg(TEXT_PRIMARY),
                        ));
                        header_spans.push(Span::styled(
                            format!("{}...", clamped_args),
                            Style::default().fg(TEXT_MUTED),
                        ));
                    }
                    StepState::Done => {
                        header_spans.push(Span::styled("● ", Style::default().fg(ACCENT_PRIMARY)));
                        header_spans.push(Span::styled(
                            name.clone(),
                            Style::default().fg(TEXT_PRIMARY),
                        ));
                        if !clamped_args.is_empty() {
                            header_spans
                                .push(Span::styled(clamped_args, Style::default().fg(TEXT_MUTED)));
                        }
                    }
                    StepState::Error => {
                        header_spans.push(Span::styled("● ", Style::default().fg(Color::Red)));
                        header_spans.push(Span::styled(
                            name.clone(),
                            Style::default().fg(TEXT_PRIMARY),
                        ));
                        if !clamped_args.is_empty() {
                            header_spans
                                .push(Span::styled(clamped_args, Style::default().fg(TEXT_MUTED)));
                        }
                    }
                    StepState::Other => {
                        header_spans.push(Span::styled("● ", Style::default().fg(TEXT_MUTED)));
                        header_spans.push(Span::styled(
                            name.clone(),
                            Style::default().fg(TEXT_PRIMARY),
                        ));
                        if !clamped_args.is_empty() {
                            header_spans
                                .push(Span::styled(clamped_args, Style::default().fg(TEXT_MUTED)));
                        }
                    }
                }
                result.push(with_left_margin(margin, header_spans));

                if !block.collapsed && !block.content.is_empty() {
                    let wrapped = wrap_text_utf8(
                        &block.content,
                        (width as usize).saturating_sub(4 + margin.len()),
                    );
                    for row in wrapped {
                        let text_style = if matches!(state, StepState::Error) {
                            Style::default().fg(Color::LightRed)
                        } else {
                            Style::default().fg(TEXT_MUTED)
                        };
                        result.push(with_left_margin(
                            margin,
                            vec![
                                Span::styled("│ ", Style::default().fg(BORDER_SUBTLE)),
                                Span::styled(row, text_style),
                            ],
                        ));
                    }
                }
                result.push(Line::from(""));
            }
            BlockKind::Error => {
                let wrapped = wrap_text_utf8(
                    &block.content,
                    (width as usize).saturating_sub(9 + margin.len()),
                );
                for (i, row) in wrapped.into_iter().enumerate() {
                    let prefix = if i == 0 { "● Error: " } else { "│ " };
                    result.push(with_left_margin(
                        margin,
                        vec![
                            Span::styled(
                                prefix.to_string(),
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(row, Style::default().fg(Color::LightRed)),
                        ],
                    ));
                }
                result.push(Line::from(""));
            }
            BlockKind::Interrupted => {
                result.push(with_left_margin(
                    margin,
                    vec![Span::styled(
                        block.content.clone(),
                        Style::default().fg(TEXT_MUTED),
                    )],
                ));
                result.push(Line::from(""));
            }
        }
        result
    }
}
