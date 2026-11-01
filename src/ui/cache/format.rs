use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::protocol::StepState;
use crate::ui::cache::{BlockKind, CachedBlockLines, MessageBlock, RenderCache};
use crate::ui::markdown::{
    parse_markdown_segments, render_markdown_to_lines, render_segments_to_lines,
};
use crate::ui::theme::{ACCENT_PRIMARY, BG_USER_CONTAINER, TEXT_MUTED, TEXT_PRIMARY};

pub fn wrap_text_utf8(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let max_w = max_width.max(1);
    for raw_line in text.split('\n') {
        if raw_line.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current_line = String::new();
        let mut current_width = 0;
        for ch in raw_line.chars() {
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
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(6) as usize);
                let pad_empty = (width as usize).saturating_sub(3);
                let top_line = Line::from(vec![
                    Span::styled(
                        " ┃ ",
                        Style::default()
                            .fg(ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " ".repeat(pad_empty),
                        Style::default().bg(BG_USER_CONTAINER),
                    ),
                ])
                .style(Style::default().bg(BG_USER_CONTAINER));
                result.push(top_line);

                for row in wrapped.into_iter() {
                    let row_len = row.chars().count();
                    let pad_len = (width as usize).saturating_sub(3 + row_len);
                    let mut line = Line::from(vec![
                        Span::styled(
                            " ┃ ",
                            Style::default()
                                .fg(ACCENT_PRIMARY)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(row, Style::default().fg(TEXT_PRIMARY)),
                        Span::styled(" ".repeat(pad_len), Style::default().bg(BG_USER_CONTAINER)),
                    ]);
                    line = line.style(Style::default().bg(BG_USER_CONTAINER));
                    result.push(line);
                }

                let bottom_line = Line::from(vec![
                    Span::styled(
                        " ┃ ",
                        Style::default()
                            .fg(ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        " ".repeat(pad_empty),
                        Style::default().bg(BG_USER_CONTAINER),
                    ),
                ])
                .style(Style::default().bg(BG_USER_CONTAINER));
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
                        let mut meta_parts = vec![format!("▣ {} · {}", meta.mode, meta.model)];
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
                    let line_count = block.content.lines().count().max(1);
                    result.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            format!("▸ Thought [collapsed - {} lines]", line_count),
                            Style::default()
                                .fg(TEXT_MUTED)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                } else {
                    result.push(Line::from(vec![
                        Span::raw("   "),
                        Span::styled(
                            "▾ Thought",
                            Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
                        ),
                    ]));
                    let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(8) as usize);
                    for row in wrapped {
                        result.push(Line::from(vec![
                            Span::styled("   ▎ ", Style::default().fg(TEXT_MUTED)),
                            Span::styled(
                                row,
                                Style::default()
                                    .fg(TEXT_MUTED)
                                    .add_modifier(Modifier::ITALIC),
                            ),
                        ]));
                    }
                }
            }
            BlockKind::Tool { name, state } => {
                let (state_badge, state_style) = match state {
                    StepState::Active => (
                        "[RUNNING]",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    StepState::Done => (
                        "[DONE]",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    StepState::Error => (
                        "[ERROR]",
                        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                    ),
                    StepState::Other => (
                        "[PENDING]",
                        Style::default().fg(TEXT_MUTED).add_modifier(Modifier::BOLD),
                    ),
                };

                let args_str = block
                    .tool_args
                    .as_deref()
                    .map(|a| format!("({})", a))
                    .unwrap_or_default();

                if block.collapsed {
                    let header = format!("   {} {}{} ▸ [collapsed]", state_badge, name, args_str);
                    result.push(Line::from(vec![Span::styled(header, state_style)]));
                } else {
                    let header = format!("   {} {}{}", state_badge, name, args_str);
                    result.push(Line::from(vec![Span::styled(header, state_style)]));

                    if !block.content.is_empty() {
                        let wrapped =
                            wrap_text_utf8(&block.content, width.saturating_sub(8) as usize);
                        for row in wrapped {
                            let text_style = if matches!(state, StepState::Error) {
                                Style::default().fg(Color::LightRed)
                            } else {
                                Style::default().fg(TEXT_MUTED)
                            };
                            result.push(Line::from(vec![
                                Span::styled("     ", Style::default()),
                                Span::styled(row, text_style),
                            ]));
                        }
                    }
                }
                result.push(Line::from(""));
            }
            BlockKind::Error => {
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(8) as usize);
                for (i, row) in wrapped.into_iter().enumerate() {
                    let prefix = if i == 0 { "   ! " } else { "     " };
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
