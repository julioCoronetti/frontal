use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::collections::HashMap;

use crate::protocol::StepState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockKind {
    User,
    Assistant,
    Thought,
    Tool { name: String, state: StepState },
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageBlock {
    pub id: u64,
    pub kind: BlockKind,
    pub content: String,
    pub revision: u64,
    pub consolidated: bool,
}

#[derive(Clone, Debug)]
pub struct CachedBlockLines {
    pub width: u16,
    pub revision: u64,
    pub lines: Vec<Line<'static>>,
}

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

pub struct RenderCache {
    blocks: Vec<MessageBlock>,
    l2_cache: HashMap<u64, CachedBlockLines>,
    next_block_id: u64,
    cache_hits: u64,
    cache_misses: u64,
    last_width: u16,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            l2_cache: HashMap::new(),
            next_block_id: 1,
            cache_hits: 0,
            cache_misses: 0,
            last_width: 80,
        }
    }

    pub fn push_user_message(&mut self, text: &str) -> u64 {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::User,
            content: text.to_string(),
            revision: 1,
            consolidated: true,
        });
        id
    }

    pub fn append_streaming_delta(&mut self, kind: BlockKind, delta: &str) -> u64 {
        for block in self.blocks.iter_mut().rev() {
            if !block.consolidated && block.kind == kind {
                block.content.push_str(delta);
                block.revision += 1;
                return block.id;
            }
        }
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind,
            content: delta.to_string(),
            revision: 1,
            consolidated: false,
        });
        id
    }

    pub fn update_tool_state(&mut self, name: &str, state: StepState, output: Option<&str>) -> u64 {
        let kind = BlockKind::Tool {
            name: name.to_string(),
            state,
        };
        for block in self.blocks.iter_mut().rev() {
            if !block.consolidated {
                if let BlockKind::Tool { name: n, .. } = &block.kind {
                    if n == name {
                        block.kind = kind;
                        if let Some(out) = output {
                            block.content = out.to_string();
                        }
                        block.revision += 1;
                        return block.id;
                    }
                }
            }
        }
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind,
            content: output.unwrap_or("").to_string(),
            revision: 1,
            consolidated: false,
        });
        id
    }

    pub fn push_error(&mut self, text: &str) -> u64 {
        let id = self.next_block_id;
        self.next_block_id += 1;
        self.blocks.push(MessageBlock {
            id,
            kind: BlockKind::Error,
            content: text.to_string(),
            revision: 1,
            consolidated: true,
        });
        id
    }

    pub fn consolidate_active_turn(&mut self, fallback_response: Option<&str>) {
        let mut has_assistant_content = false;
        for block in self.blocks.iter_mut() {
            if !block.consolidated {
                if matches!(block.kind, BlockKind::Assistant) && !block.content.trim().is_empty() {
                    has_assistant_content = true;
                }
                block.consolidated = true;
            }
        }
        if !has_assistant_content {
            if let Some(resp) = fallback_response {
                if !resp.trim().is_empty() {
                    let id = self.next_block_id;
                    self.next_block_id += 1;
                    self.blocks.push(MessageBlock {
                        id,
                        kind: BlockKind::Assistant,
                        content: resp.to_string(),
                        revision: 1,
                        consolidated: true,
                    });
                }
            }
        }
    }

    pub fn ensure_visual_lines(&mut self, width: u16) -> usize {
        let safe_width = width.max(1);
        self.last_width = safe_width;
        let mut total = 0;

        for block in &self.blocks {
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
                let generated_lines = self.format_block_lines(block, safe_width);
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

    fn format_block_lines(&self, block: &MessageBlock, width: u16) -> Vec<Line<'static>> {
        let mut result = Vec::new();
        match &block.kind {
            BlockKind::User => {
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                for (i, row) in wrapped.into_iter().enumerate() {
                    let prefix = if i == 0 { "> " } else { "  " };
                    result.push(Line::from(vec![
                        Span::styled(
                            prefix.to_string(),
                            Style::default()
                                .fg(Color::Cyan)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(row, Style::default().fg(Color::White)),
                    ]));
                }
                result.push(Line::from(""));
            }
            BlockKind::Assistant => {
                let wrapped = wrap_text_utf8(&block.content, width as usize);
                for row in wrapped {
                    result.push(Line::from(vec![Span::styled(
                        row,
                        Style::default().fg(Color::Gray),
                    )]));
                }
                if block.consolidated {
                    result.push(Line::from(""));
                }
            }
            BlockKind::Thought => {
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                for (i, row) in wrapped.into_iter().enumerate() {
                    let prefix = if i == 0 { "~ " } else { "  " };
                    result.push(Line::from(vec![
                        Span::styled(prefix.to_string(), Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            row,
                            Style::default()
                                .fg(Color::DarkGray)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                }
            }
            BlockKind::Tool { name, state } => {
                let state_str = match state {
                    StepState::Active => "running",
                    StepState::Done => "completed",
                    StepState::Other => "pending",
                };
                let header = format!("[tool: {} ({})]", name, state_str);
                result.push(Line::from(vec![Span::styled(
                    header,
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )]));
                if !block.content.is_empty() {
                    let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                    for row in wrapped {
                        result.push(Line::from(vec![
                            Span::styled("  ".to_string(), Style::default().fg(Color::Yellow)),
                            Span::styled(row, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                }
                result.push(Line::from(""));
            }
            BlockKind::Error => {
                let wrapped = wrap_text_utf8(&block.content, width.saturating_sub(4) as usize);
                for (i, row) in wrapped.into_iter().enumerate() {
                    let prefix = if i == 0 { "! " } else { "  " };
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

    pub fn visible_slice(&self, offset: usize, count: usize) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut current_idx = 0;
        let end_idx = offset.saturating_add(count);

        for block in &self.blocks {
            if let Some(cached) = self.l2_cache.get(&block.id) {
                for line in &cached.lines {
                    if current_idx >= offset && current_idx < end_idx {
                        lines.push(line.clone());
                    }
                    current_idx += 1;
                    if current_idx >= end_idx {
                        return lines;
                    }
                }
            }
        }

        lines
    }

    pub fn total_lines(&self) -> usize {
        let mut sum = 0;
        for block in &self.blocks {
            if let Some(cached) = self.l2_cache.get(&block.id) {
                sum += cached.lines.len();
            }
        }
        sum
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    pub fn cache_misses(&self) -> u64 {
        self.cache_misses
    }

    pub fn blocks_count(&self) -> usize {
        self.blocks.len()
    }
}

impl Default for RenderCache {
    fn default() -> Self {
        Self::new()
    }
}
