use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::protocol::StepState;
use crate::ui::cache::{BlockKind, CachedBlockLines, MessageBlock, RenderCache};
use crate::ui::markdown::{
    parse_markdown_segments, render_markdown_to_lines, render_segments_to_lines,
};
use crate::ui::theme::{ACCENT_PRIMARY, BG_USER_CONTAINER, TEXT_MUTED, TEXT_PRIMARY};

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
        match &block.kind {
            BlockKind::User => {
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                let pad_empty = (width as usize).saturating_sub(2);
                let bar_style = Style::default()
                    .fg(ACCENT_PRIMARY)
                    .bg(BG_USER_CONTAINER)
                    .add_modifier(Modifier::BOLD);
                let top_line = Line::from(vec![
                    Span::styled("▌ ", bar_style),
                    Span::styled(
                        " ".repeat(pad_empty),
                        Style::default().bg(BG_USER_CONTAINER),
                    ),
                ]);
                result.push(top_line);

                for row in wrapped.into_iter() {
                    let row_len = row.chars().count();
                    let pad_len = (width as usize).saturating_sub(2 + row_len);
                    let line = Line::from(vec![
                        Span::styled("▌ ", bar_style),
                        Span::styled(row, Style::default().fg(TEXT_PRIMARY).bg(BG_USER_CONTAINER)),
                        Span::styled(" ".repeat(pad_len), Style::default().bg(BG_USER_CONTAINER)),
                    ]);
                    result.push(line);
                }

                let bottom_line = Line::from(vec![
                    Span::styled("▌ ", bar_style),
                    Span::styled(
                        " ".repeat(pad_empty),
                        Style::default().bg(BG_USER_CONTAINER),
                    ),
                ]);
                result.push(bottom_line);
                result.push(Line::from(""));
            }
            BlockKind::Assistant => {
                let content_width = width.saturating_sub(2) as usize;
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
                result.extend(lines);
                if block.consolidated {
                    if let Some(meta) = &block.turn_meta {
                        let mut meta_parts = vec![format!("▣ {}", meta.model)];
                        if let Some(d) = meta.duration_seconds {
                            if d > 0.0 {
                                meta_parts.push(format!("{:.1}s", d));
                                if let Some(tok) = meta.output_tokens {
                                    if tok > 0 {
                                        meta_parts.push(format!("{:.1} tok/s", (tok as f64) / d));
                                    }
                                }
                            }
                        }
                        result.push(Line::from(""));
                        result.push(Line::from(vec![Span::styled(
                            meta_parts.join(" · "),
                            Style::default().fg(TEXT_MUTED),
                        )]));
                    }
                    result.push(Line::from(""));
                }
            }
            BlockKind::Thought => {
                if block.collapsed {
                    result.push(Line::from(vec![Span::styled(
                        "+ Thought",
                        Style::default().fg(Color::Yellow),
                    )]));
                } else {
                    result.push(Line::from(vec![Span::styled(
                        "- Thought",
                        Style::default().fg(Color::Yellow),
                    )]));
                    result.push(Line::from(""));
                    let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                    for row in wrapped {
                        result.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(TEXT_MUTED)),
                            Span::styled(
                                row,
                                Style::default()
                                    .fg(TEXT_MUTED)
                                    .add_modifier(Modifier::ITALIC),
                            ),
                        ]));
                    }
                }
                result.push(Line::from(""));
            }
            BlockKind::Tool { name, state } => {
                let max_args_len = (width as usize).saturating_sub(name.chars().count() + 25);
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
                        header_spans.push(Span::styled("● ", Style::default().fg(Color::Yellow)));
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
                        header_spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
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
                result.push(Line::from(header_spans));

                if !block.collapsed && !block.content.is_empty() {
                    let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                    for row in wrapped {
                        let text_style = if matches!(state, StepState::Error) {
                            Style::default().fg(Color::LightRed)
                        } else {
                            Style::default().fg(TEXT_MUTED)
                        };
                        result.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(TEXT_MUTED)),
                            Span::styled(row, text_style),
                        ]));
                    }
                }
                result.push(Line::from(""));
            }
            BlockKind::Error => {
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                for (i, row) in wrapped.into_iter().enumerate() {
                    let prefix = if i == 0 { "● Error: " } else { "│ " };
                    result.push(Line::from(vec![
                        Span::styled(
                            prefix.to_string(),
                            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(row, Style::default().fg(Color::LightRed)),
                    ]));
                }
                result.push(Line::from(""));
            }
        }
        result
    }
}
